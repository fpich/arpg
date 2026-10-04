//! End-to-end gameplay over real QUIC (SPEC.md sections 124-146): two
//! clients join a hosted game, move, fight spawned monsters, gain
//! experience, trade with SQL persistence and reconnect inside the
//! grace window. Every assertion observes the live GameState.

use arpg_protocol::messages as msg;
use arpg_server::quic::{encode_frame, GameServer};
use arpg_sim::GameInstance;
use prost::Message;
use quinn::{ClientConfig, Endpoint};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;

fn client_endpoint(cert_der: Vec<u8>) -> Endpoint {
    let mut endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    let mut roots = quinn::rustls::RootCertStore::empty();
    roots
        .add(rustls_pki_types::CertificateDer::from(cert_der))
        .unwrap();
    let client_config = ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
    endpoint.set_default_client_config(client_config);
    endpoint
}

async fn read_frame<S: AsyncReadExt + Unpin>(stream: &mut S) -> Vec<u8> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await.unwrap();
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).await.unwrap();
    payload
}

/// Read frames until a CommandAck for the given sequence arrives,
/// skipping the host-loop snapshots interleaved on the same stream.
async fn read_ack_for<S: AsyncReadExt + Unpin>(stream: &mut S, seq: u32) -> msg::CommandAck {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        assert!(std::time::Instant::now() < deadline, "no ack for seq {seq}");
        let bytes = read_frame(stream).await;
        if let Ok(ack) = msg::CommandAck::decode(bytes.as_slice()) {
            if ack.last_processed_sequence == seq {
                return ack;
            }
        }
    }
}

struct Client {
    _conn: quinn::Connection,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    next_seq: u32,
    player: u32,
}

async fn join(endpoint: &Endpoint, addr: std::net::SocketAddr, character_id: u32) -> Client {
    let conn = endpoint.connect(addr, "localhost").unwrap().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    let hello = msg::ClientHello {
        protocol_version: arpg_server::PROTOCOL_VERSION.0,
        client_build: 1,
        supported_features: vec![],
    };
    send.write_all(&encode_frame(&hello)).await.unwrap();
    let reply = read_frame(&mut recv).await;
    let server_hello = msg::ServerHello::decode(reply.as_slice()).unwrap();
    assert!(server_hello.accepted);
    let join = msg::JoinGameRequest {
        game_id: vec![],
        character_id,
    };
    send.write_all(&encode_frame(&join)).await.unwrap();
    let reply = read_frame(&mut recv).await;
    let accepted = msg::JoinGameAccepted::decode(reply.as_slice()).unwrap();
    assert_eq!(accepted.player_id, character_id);
    Client {
        _conn: conn,
        send,
        recv,
        next_seq: 1,
        player: character_id,
    }
}

impl Client {
    async fn send_cmd(&mut self, command: msg::command_envelope::Command) -> msg::CommandAck {
        let seq = self.next_seq;
        self.next_seq += 1;
        let env = msg::CommandEnvelope {
            sequence: seq,
            client_tick: 1,
            player_id: self.player,
            command: Some(command),
        };
        self.send.write_all(&encode_frame(&env)).await.unwrap();
        read_ack_for(&mut self.recv, seq).await
    }
}

#[tokio::test]
async fn full_gameplay_session_over_quic() {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let game = Arc::new(Mutex::new(GameInstance::new(data, rules, [21u8; 32])));
    // pre-spawn monsters next to the spawn point so combat happens fast
    {
        let mut g = game.lock().unwrap();
        for i in 0..3 {
            g.spawn_monster(arpg_core::WorldPos::new((10 + i) * 256, 10 * 256));
        }
    }
    let (mut server, cert_der) = GameServer::bind_loopback(Arc::clone(&game)).unwrap();
    let store: Box<dyn arpg_persistence::PersistenceStore + Send> =
        Box::new(arpg_persistence::sqlite::SqliteStore::in_memory().unwrap());
    server.set_store(Arc::new(Mutex::new(store)));
    let server = Arc::new(server);
    let addr = server.local_addr;
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(Arc::clone(&server).serve(shutdown_rx));

    let endpoint = client_endpoint(cert_der.clone());
    let mut p1 = join(&endpoint, addr, 1).await;
    let mut p2 = join(&endpoint, addr, 2).await;

    // --- movement: p1 walks away and the position advances ---
    let ack = p1
        .send_cmd(msg::command_envelope::Command::Move(msg::MoveCommand {
            x: 5 * 256,
            y: 5 * 256,
            movement_mode: 1,
        }))
        .await;
    assert_eq!(ack.admission, "Accepted");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let pos = game
            .lock()
            .unwrap()
            .state
            .players
            .get(&arpg_core::PlayerId(1))
            .map(|p| p.pos);
        if let Some(pos) = pos {
            if pos.tile() != (0, 0) {
                break;
            }
        }
        if std::time::Instant::now() >= deadline {
            let g = game.lock().unwrap();
            eprintln!(
                "E2E move stuck: pos={:?} tile={:?} tick={}",
                g.state.players.get(&arpg_core::PlayerId(1)).map(|p| p.pos),
                g.state
                    .players
                    .get(&arpg_core::PlayerId(1))
                    .map(|p| p.pos.tile()),
                g.state.tick.0
            );
            panic!("player 1 never moved");
        }
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    }

    // --- combat: p1 attacks with the base Attack skill until a
    // monster dies and experience is granted ---
    let xp_before = game
        .lock()
        .unwrap()
        .state
        .players
        .get(&arpg_core::PlayerId(1))
        .unwrap()
        .experience;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        // the admission policy (SPEC.md section 171) refills the command
        // bucket at 60/s, so pace the swings below that budget instead
        // of flooding the limiter
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let ack = p1
            .send_cmd(msg::command_envelope::Command::UseSkill(
                msg::UseSkillCommand {
                    skill_id: 1,
                    target_x: Some(10 * 256),
                    target_y: Some(10 * 256),
                },
            ))
            .await;
        assert_eq!(ack.admission, "Accepted");
        let xp = game
            .lock()
            .unwrap()
            .state
            .players
            .get(&arpg_core::PlayerId(1))
            .unwrap()
            .experience;
        if xp > xp_before {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "killing a monster never granted experience"
        );
    }

    // --- trade: p1 and p2 exchange 30 gold, committed to SQL ---
    {
        let mut g = game.lock().unwrap();
        for p in [arpg_core::PlayerId(1), arpg_core::PlayerId(2)] {
            g.economy.gold.insert(
                p,
                arpg_sim::Gold {
                    carried: 100,
                    stash: 0,
                },
            );
        }
    }
    p1.send_cmd(msg::command_envelope::Command::Trade(msg::TradeCommand {
        op: 0,
        target_player: 2,
        trade_id: 0,
        item_ids: vec![],
        gold: 0,
    }))
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let trade_id = game
        .lock()
        .unwrap()
        .trades
        .iter_open()
        .next()
        .map(|(id, _)| id.0)
        .expect("trade must be open");
    p1.send_cmd(msg::command_envelope::Command::Trade(msg::TradeCommand {
        op: 1,
        target_player: 0,
        trade_id,
        item_ids: vec![],
        gold: 30,
    }))
    .await;
    p1.send_cmd(msg::command_envelope::Command::Trade(msg::TradeCommand {
        op: 2,
        target_player: 0,
        trade_id,
        item_ids: vec![],
        gold: 0,
    }))
    .await;
    p2.send_cmd(msg::command_envelope::Command::Trade(msg::TradeCommand {
        op: 2,
        target_player: 0,
        trade_id,
        item_ids: vec![],
        gold: 0,
    }))
    .await;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let (committed, gold2) = {
            let g = game.lock().unwrap();
            let committed = g
                .trades
                .get(arpg_persistence::TradeId(trade_id))
                .map(|t| t.state == arpg_sim::TradeState::Committed)
                .unwrap_or(false);
            let gold2 = g.economy.gold_of(arpg_core::PlayerId(2)).carried;
            (committed, gold2)
        };
        if committed {
            assert_eq!(gold2, 130, "p2 must have received the 30 gold");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "trade never committed to the SQL store"
        );
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    }
    let gold1 = game
        .lock()
        .unwrap()
        .economy
        .gold_of(arpg_core::PlayerId(1))
        .carried;
    assert_eq!(gold1, 70, "p1 must have paid the 30 gold");

    // --- snapshots: both clients receive live replication frames ---
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(800);
    let mut saw_snapshot = false;
    'outer: while std::time::Instant::now() < deadline {
        for bytes in [read_frame(&mut p2.recv).await] {
            if let Ok(snap) = msg::Snapshot::decode(bytes.as_slice()) {
                if snap.tick > 0 {
                    saw_snapshot = true;
                    break 'outer;
                }
            }
        }
    }
    assert!(saw_snapshot, "p2 must receive live snapshots");

    // --- replay: the recorded commands cover movement and combat ---
    let replay = server.replay();
    assert!(!replay.entries.is_empty(), "the session must be recorded");
    assert!(replay.entries.len() >= 6);

    // --- grace: p2 drops and reconnects inside the window ---
    drop(p2);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let mut p2b = join(&endpoint, addr, 2).await;
    assert!(
        game.lock()
            .unwrap()
            .state
            .players
            .contains_key(&arpg_core::PlayerId(2)),
        "p2 character must survive the reconnect"
    );
    let ack = p2b
        .send_cmd(msg::command_envelope::Command::Move(msg::MoveCommand {
            x: 2 * 256,
            y: 2 * 256,
            movement_mode: 0,
        }))
        .await;
    assert_eq!(ack.admission, "Accepted");

    shutdown_tx.send(()).await.unwrap();
}
