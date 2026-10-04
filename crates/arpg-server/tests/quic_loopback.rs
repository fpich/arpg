use arpg_core::PlayerId;
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

#[tokio::test]
async fn quic_loopback_handshake_join_and_commands() {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let game = Arc::new(Mutex::new(GameInstance::new(data, rules, [7u8; 32])));

    let (server, cert_der) = GameServer::bind_loopback(Arc::clone(&game)).unwrap();
    let server = Arc::new(server);
    let addr = server.local_addr;
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(Arc::clone(&server).serve(shutdown_rx));

    // client
    let client = client_endpoint(cert_der.clone());
    let conn = client.connect(addr, "localhost").unwrap().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();

    // ClientHello
    let hello = msg::ClientHello {
        protocol_version: arpg_server::PROTOCOL_VERSION.0,
        client_build: 1,
        supported_features: vec![],
    };
    send.write_all(&encode_frame(&hello)).await.unwrap();
    let reply = read_frame(&mut recv).await;
    let server_hello = msg::ServerHello::decode(reply.as_slice()).unwrap();
    assert!(server_hello.accepted);

    // JoinGame
    let join = msg::JoinGameRequest {
        game_id: vec![1, 2, 3],
        character_id: 1,
    };
    send.write_all(&encode_frame(&join)).await.unwrap();
    let reply = read_frame(&mut recv).await;
    let accepted = msg::JoinGameAccepted::decode(reply.as_slice()).unwrap();
    assert_eq!(accepted.player_id, 1);
    assert_eq!(accepted.tick_rate, arpg_core::TICKS_PER_SECOND);

    // Commands
    for seq in 1..=3u32 {
        let env = msg::CommandEnvelope {
            sequence: seq,
            client_tick: 1,
            player_id: 1,
            command: Some(msg::command_envelope::Command::Move(msg::MoveCommand {
                x: (seq as i32) * 100,
                y: 0,
                movement_mode: 0,
            })),
        };
        send.write_all(&encode_frame(&env)).await.unwrap();
        let reply = read_frame(&mut recv).await;
        let ack = msg::CommandAck::decode(reply.as_slice()).unwrap();
        assert_eq!(ack.last_processed_sequence, seq);
        assert_eq!(ack.admission, "Accepted");
    }

    // run a few ticks: commands flow through the sim
    for _ in 0..10 {
        game.lock().unwrap().tick();
    }
    let pos = game
        .lock()
        .unwrap()
        .state
        .players
        .get(&PlayerId(1))
        .map(|p| p.pos);
    assert!(pos.is_some(), "player must be in the game");

    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test]
async fn quic_loopback_movement_datagram() {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let game = Arc::new(Mutex::new(GameInstance::new(data, rules, [7u8; 32])));

    let (server, cert_der) = GameServer::bind_loopback(Arc::clone(&game)).unwrap();
    let server = Arc::new(server);
    let addr = server.local_addr;
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(Arc::clone(&server).serve(shutdown_rx));

    let client = client_endpoint(cert_der.clone());
    let conn = client.connect(addr, "localhost").unwrap().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();

    // handshake
    let hello = msg::ClientHello {
        protocol_version: arpg_server::PROTOCOL_VERSION.0,
        client_build: 1,
        supported_features: vec![],
    };
    send.write_all(&encode_frame(&hello)).await.unwrap();
    let _ = read_frame(&mut recv).await;

    // join
    let join = msg::JoinGameRequest {
        game_id: vec![],
        character_id: 1,
    };
    send.write_all(&encode_frame(&join)).await.unwrap();
    let _ = read_frame(&mut recv).await;

    // movement via unreliable datagram (section 136)
    let env = msg::CommandEnvelope {
        sequence: 1,
        client_tick: 1,
        player_id: 1,
        command: Some(msg::command_envelope::Command::Move(msg::MoveCommand {
            x: 777,
            y: 0,
            movement_mode: 0,
        })),
    };
    conn.send_datagram(encode_frame(&env).into()).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    for _ in 0..10 {
        game.lock().unwrap().tick();
    }
    let pos = game
        .lock()
        .unwrap()
        .state
        .players
        .get(&PlayerId(1))
        .map(|p| p.pos);
    assert!(pos.is_some(), "datagram movement must reach the sim");

    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test]
async fn quic_loopback_rejects_incompatible_protocol() {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let game = Arc::new(Mutex::new(GameInstance::new(data, rules, [7u8; 32])));

    let (server, cert_der) = GameServer::bind_loopback(Arc::clone(&game)).unwrap();
    let server = Arc::new(server);
    let addr = server.local_addr;
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(Arc::clone(&server).serve(shutdown_rx));

    let client = client_endpoint(cert_der.clone());
    let conn = client.connect(addr, "localhost").unwrap().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();

    let bad = msg::ClientHello {
        protocol_version: 999,
        client_build: 1,
        supported_features: vec![],
    };
    send.write_all(&encode_frame(&bad)).await.unwrap();
    let reply = read_frame(&mut recv).await;
    let server_hello = msg::ServerHello::decode(reply.as_slice()).unwrap();
    assert!(!server_hello.accepted);

    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test]
async fn quic_loopback_use_item_command() {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let game = Arc::new(Mutex::new(GameInstance::new(data, rules, [7u8; 32])));
    let (server, cert_der) = GameServer::bind_loopback(Arc::clone(&game)).unwrap();
    let server = Arc::new(server);
    let addr = server.local_addr;
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(Arc::clone(&server).serve(shutdown_rx));

    let client = client_endpoint(cert_der.clone());
    let conn = client.connect(addr, "localhost").unwrap().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();

    let hello = msg::ClientHello {
        protocol_version: arpg_server::PROTOCOL_VERSION.0,
        client_build: 1,
        supported_features: vec![],
    };
    send.write_all(&encode_frame(&hello)).await.unwrap();
    let _ = read_frame(&mut recv).await;
    let join = msg::JoinGameRequest {
        game_id: vec![1, 2, 3],
        character_id: 1,
    };
    send.write_all(&encode_frame(&join)).await.unwrap();
    let _ = read_frame(&mut recv).await;

    // UseItem for an unknown item: still acknowledged (admission is
    // scheduler-level), no crash (section 170)
    let env = msg::CommandEnvelope {
        sequence: 1,
        client_tick: 1,
        player_id: 1,
        command: Some(msg::command_envelope::Command::UseItem(
            msg::UseItemCommand { item_id: 9999 },
        )),
    };
    send.write_all(&encode_frame(&env)).await.unwrap();
    let reply = read_frame(&mut recv).await;
    let ack = msg::CommandAck::decode(reply.as_slice()).unwrap();
    assert_eq!(ack.last_processed_sequence, 1);
    shutdown_tx.send(()).await.unwrap();
}
#[tokio::test]
async fn empty_grace_expires_saves_and_destroys() {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let game = Arc::new(Mutex::new(GameInstance::new(data, rules, [7u8; 32])));
    let policy = arpg_server::policy::ServerPolicy {
        empty_grace_ticks: 3,
        reconnect_grace_ticks: 3,
    };
    let (mut server, cert_der) =
        GameServer::bind_loopback_with_policy(Arc::clone(&game), policy).unwrap();
    let repository = Arc::new(std::sync::Mutex::new(
        arpg_persistence::CharacterRepository::new(),
    ));
    server.set_repository(Arc::clone(&repository));
    let server = Arc::new(server);
    let addr = server.local_addr;
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(Arc::clone(&server).serve(shutdown_rx));
    // client joins then drops the connection
    let client = client_endpoint(cert_der.clone());
    let conn = client.connect(addr, "localhost").unwrap().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    let hello = msg::ClientHello {
        protocol_version: arpg_server::PROTOCOL_VERSION.0,
        client_build: 1,
        supported_features: vec![],
    };
    send.write_all(&encode_frame(&hello)).await.unwrap();
    let _ = read_frame(&mut recv).await;
    let join = msg::JoinGameRequest {
        game_id: vec![],
        character_id: 1,
    };
    send.write_all(&encode_frame(&join)).await.unwrap();
    let _ = read_frame(&mut recv).await;
    conn.close(quinn::VarInt::from(0u32), b"done");
    // host loop ticks the short grace down (3 ticks = 120ms), saves the
    // character and destroys the GameState (sections 148-149)
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let snapshot = repository
        .lock()
        .unwrap()
        .load(PlayerId(1))
        .unwrap()
        .cloned();
    assert!(snapshot.is_some(), "character must be saved at expiry");
    assert!(
        game.lock().unwrap().state.players.is_empty(),
        "GameState must be destroyed at expiry"
    );
    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test]
async fn snapshots_stream_to_connected_clients() {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let game = Arc::new(Mutex::new(GameInstance::new(data, rules, [7u8; 32])));
    let (server, cert_der) = GameServer::bind_loopback(Arc::clone(&game)).unwrap();
    let server = Arc::new(server);
    let addr = server.local_addr;
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(Arc::clone(&server).serve(shutdown_rx));
    let client = client_endpoint(cert_der.clone());
    let conn = client.connect(addr, "localhost").unwrap().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    let hello = msg::ClientHello {
        protocol_version: arpg_server::PROTOCOL_VERSION.0,
        client_build: 1,
        supported_features: vec![],
    };
    send.write_all(&encode_frame(&hello)).await.unwrap();
    let _ = read_frame(&mut recv).await;
    let join = msg::JoinGameRequest {
        game_id: vec![],
        character_id: 1,
    };
    send.write_all(&encode_frame(&join)).await.unwrap();
    let _ = read_frame(&mut recv).await;
    // the host loop ticks at 25 tps; within a few hundred ms the client
    // must receive a wire snapshot (section 143) on the reliable stream
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(800);
    let mut got_snapshot = false;
    while std::time::Instant::now() < deadline {
        let frame =
            tokio::time::timeout(std::time::Duration::from_millis(100), read_frame(&mut recv))
                .await;
        match frame {
            Ok(bytes) => {
                if let Ok(snap) = msg::Snapshot::decode(bytes.as_slice()) {
                    assert!(snap.tick > 0, "snapshot carries a live tick");
                    assert!(snap.snapshot_id > 0);
                    got_snapshot = true;
                    break;
                }
            }
            _ => continue,
        }
    }
    assert!(got_snapshot, "client must receive wire snapshots each tick");
    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test]
async fn command_flood_is_rate_limited() {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let game = Arc::new(Mutex::new(GameInstance::new(data, rules, [7u8; 32])));
    let (server, cert_der) = GameServer::bind_loopback(Arc::clone(&game)).unwrap();
    let server = Arc::new(server);
    let addr = server.local_addr;
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(Arc::clone(&server).serve(shutdown_rx));
    let client = client_endpoint(cert_der.clone());
    let conn = client.connect(addr, "localhost").unwrap().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    let hello = msg::ClientHello {
        protocol_version: arpg_server::PROTOCOL_VERSION.0,
        client_build: 1,
        supported_features: vec![],
    };
    send.write_all(&encode_frame(&hello)).await.unwrap();
    let _ = read_frame(&mut recv).await;
    let join = msg::JoinGameRequest {
        game_id: vec![],
        character_id: 1,
    };
    send.write_all(&encode_frame(&join)).await.unwrap();
    let _ = read_frame(&mut recv).await;
    // flood far beyond the reference burst budget (60 commands)
    let mut accepted = 0;
    let mut rate_limited = 0;
    for seq in 1..=200u32 {
        let env = msg::CommandEnvelope {
            sequence: seq,
            client_tick: 1,
            player_id: 1,
            command: Some(msg::command_envelope::Command::Move(msg::MoveCommand {
                x: 0,
                y: 0,
                movement_mode: 0,
            })),
        };
        send.write_all(&encode_frame(&env)).await.unwrap();
        // drain acks and snapshots until we find the ack for this sequence
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        loop {
            let frame =
                tokio::time::timeout(std::time::Duration::from_millis(200), read_frame(&mut recv));
            match frame.await {
                Ok(bytes) => {
                    if let Ok(ack) = msg::CommandAck::decode(bytes.as_slice()) {
                        if ack.last_processed_sequence == seq {
                            if ack.admission == "Accepted" {
                                accepted += 1;
                            } else if ack.admission == "RateLimited" {
                                rate_limited += 1;
                            }
                            break;
                        }
                    }
                }
                _ => break,
            }
            if std::time::Instant::now() > deadline {
                break;
            }
        }
    }
    assert!(
        rate_limited > 0,
        "a 200-command flood must hit the rate limiter (accepted {accepted}, limited {rate_limited})"
    );
    assert!(accepted > 0, "the first commands within burst must pass");
    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test]
async fn reconnect_grace_expires_saves_and_removes_character() {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let game = Arc::new(Mutex::new(GameInstance::new(data, rules, [9u8; 32])));
    let policy = arpg_server::policy::ServerPolicy {
        empty_grace_ticks: 10_000,
        reconnect_grace_ticks: 3,
    };
    let (mut server, cert_der) =
        GameServer::bind_loopback_with_policy(Arc::clone(&game), policy).unwrap();
    let repository = Arc::new(std::sync::Mutex::new(
        arpg_persistence::CharacterRepository::new(),
    ));
    server.set_repository(Arc::clone(&repository));
    let server = Arc::new(server);
    let addr = server.local_addr;
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(Arc::clone(&server).serve(shutdown_rx));
    let client = client_endpoint(cert_der.clone());
    let conn = client.connect(addr, "localhost").unwrap().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    let hello = msg::ClientHello {
        protocol_version: arpg_server::PROTOCOL_VERSION.0,
        client_build: 1,
        supported_features: vec![],
    };
    send.write_all(&encode_frame(&hello)).await.unwrap();
    let _ = read_frame(&mut recv).await;
    let join = msg::JoinGameRequest {
        game_id: vec![],
        character_id: 1,
    };
    send.write_all(&encode_frame(&join)).await.unwrap();
    let _ = read_frame(&mut recv).await;
    conn.close(quinn::VarInt::from(0u32), b"drop");
    // the reconnect grace (3 ticks = 120ms) expires: the character is
    // saved then removed from the world (sections 146-147)
    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    let snapshot = repository
        .lock()
        .unwrap()
        .load(PlayerId(1))
        .unwrap()
        .cloned();
    assert!(
        snapshot.is_some(),
        "character must be saved at grace expiry"
    );
    assert!(
        game.lock().unwrap().state.players.is_empty(),
        "character must be removed from the world at grace expiry"
    );
    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test]
async fn reconnect_within_grace_resumes_the_session() {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let game = Arc::new(Mutex::new(GameInstance::new(data, rules, [11u8; 32])));
    let policy = arpg_server::policy::ServerPolicy {
        empty_grace_ticks: 10_000,
        reconnect_grace_ticks: 250,
    };
    let (mut server, cert_der) =
        GameServer::bind_loopback_with_policy(Arc::clone(&game), policy).unwrap();
    let repository = Arc::new(std::sync::Mutex::new(
        arpg_persistence::CharacterRepository::new(),
    ));
    server.set_repository(Arc::clone(&repository));
    let server = Arc::new(server);
    let addr = server.local_addr;
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(Arc::clone(&server).serve(shutdown_rx));
    let client = client_endpoint(cert_der.clone());
    let conn = client.connect(addr, "localhost").unwrap().await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    let hello = msg::ClientHello {
        protocol_version: arpg_server::PROTOCOL_VERSION.0,
        client_build: 1,
        supported_features: vec![],
    };
    send.write_all(&encode_frame(&hello)).await.unwrap();
    let _ = read_frame(&mut recv).await;
    let join = msg::JoinGameRequest {
        game_id: vec![],
        character_id: 1,
    };
    send.write_all(&encode_frame(&join)).await.unwrap();
    let _ = read_frame(&mut recv).await;
    conn.close(quinn::VarInt::from(0u32), b"drop");
    // wait inside the 10-second grace window, then reconnect
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let conn2 = client.connect(addr, "localhost").unwrap().await.unwrap();
    let (mut send2, mut recv2) = conn2.open_bi().await.unwrap();
    send2.write_all(&encode_frame(&hello)).await.unwrap();
    let _ = read_frame(&mut recv2).await;
    send2.write_all(&encode_frame(&join)).await.unwrap();
    let _ = read_frame(&mut recv2).await;
    // still in the world: the reconnection resumed the session (146)
    assert!(
        game.lock()
            .unwrap()
            .state
            .players
            .contains_key(&PlayerId(1)),
        "reconnection within the grace window must keep the character"
    );
    let snapshot = repository
        .lock()
        .unwrap()
        .load(PlayerId(1))
        .unwrap()
        .cloned();
    assert!(
        snapshot.is_none(),
        "no save may happen while the grace window is still running"
    );
    conn2.close(quinn::VarInt::from(0u32), b"done");
    shutdown_tx.send(()).await.unwrap();
}

#[tokio::test]
async fn accepted_trade_commits_against_the_sqlite_backend() {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let game = Arc::new(Mutex::new(GameInstance::new(data, rules, [13u8; 32])));
    let (mut server, cert_der) = GameServer::bind_loopback(Arc::clone(&game)).unwrap();
    let store: Box<dyn arpg_persistence::PersistenceStore + Send> =
        Box::new(arpg_persistence::sqlite::SqliteStore::in_memory().unwrap());
    server.set_store(Arc::new(Mutex::new(store)));
    let server = Arc::new(server);
    let addr = server.local_addr;
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(Arc::clone(&server).serve(shutdown_rx));
    // two players join, open a trade, offer gold, both accept
    let client = client_endpoint(cert_der.clone());
    let mut senders = Vec::new();
    for character_id in 1u32..=2u32 {
        let conn = client.connect(addr, "localhost").unwrap().await.unwrap();
        let (send, recv) = conn.open_bi().await.unwrap();
        senders.push((conn, send, recv, character_id));
    }
    let hello = msg::ClientHello {
        protocol_version: arpg_server::PROTOCOL_VERSION.0,
        client_build: 1,
        supported_features: vec![],
    };
    for (_, send, recv, _) in senders.iter_mut() {
        send.write_all(&encode_frame(&hello)).await.unwrap();
        let _ = read_frame(recv).await;
    }
    for (_, send, recv, character_id) in senders.iter_mut() {
        let join = msg::JoinGameRequest {
            game_id: vec![],
            character_id: *character_id,
        };
        send.write_all(&encode_frame(&join)).await.unwrap();
        let _ = read_frame(recv).await;
    }
    // fund both players so the gold offer validates (section 117)
    {
        let mut game = game.lock().unwrap();
        for p in [PlayerId(1), PlayerId(2)] {
            game.economy.gold.insert(
                p,
                arpg_sim::Gold {
                    carried: 100,
                    stash: 0,
                },
            );
        }
    }
    // player 1 opens a trade with player 2
    let trade_open = msg::CommandEnvelope {
        sequence: 1,
        client_tick: 1,
        player_id: 1,
        command: Some(msg::command_envelope::Command::Trade(msg::TradeCommand {
            op: 0,
            target_player: 2,
            trade_id: 0,
            item_ids: vec![],
            gold: 0,
        })),
    };
    senders[0]
        .1
        .write_all(&encode_frame(&trade_open))
        .await
        .unwrap();
    // give the scheduler a few ticks to execute the open
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    // read the trade id back from the sim through the trades map
    let trade_id = game
        .lock()
        .unwrap()
        .trades
        .iter_open()
        .next()
        .map(|(id, _)| id.0)
        .expect("trade must be open");
    // player 1 offers 30 gold, player 2 offers nothing, both accept
    let offer = msg::CommandEnvelope {
        sequence: 2,
        client_tick: 1,
        player_id: 1,
        command: Some(msg::command_envelope::Command::Trade(msg::TradeCommand {
            op: 1,
            target_player: 0,
            trade_id,
            item_ids: vec![],
            gold: 30,
        })),
    };
    senders[0].1.write_all(&encode_frame(&offer)).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    for (i, player_id) in [1u32, 2u32].iter().enumerate() {
        let accept = msg::CommandEnvelope {
            sequence: 3,
            client_tick: 1,
            player_id: *player_id,
            command: Some(msg::command_envelope::Command::Trade(msg::TradeCommand {
                op: 2,
                target_player: 0,
                trade_id,
                item_ids: vec![],
                gold: 0,
            })),
        };
        senders[i]
            .1
            .write_all(&encode_frame(&accept))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    // the host loop commits the fully accepted trade against the store
    // within a couple of ticks (sections 117-118)
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    shutdown_tx.send(()).await.unwrap();
    let committed = {
        let game = game.lock().unwrap();
        let trade = game
            .trades
            .get(arpg_persistence::TradeId(trade_id))
            .expect("trade must exist");
        trade.state == arpg_sim::TradeState::Committed
    };
    assert!(
        committed,
        "fully accepted trade must be committed via the store"
    );
}
