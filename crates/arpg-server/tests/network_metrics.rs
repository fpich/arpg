//! Network byte metrics on the loopback QUIC server (SPEC.md section 190:
//! network_bytes_sent / network_bytes_received; POLICY domain, counters only).

use arpg_metrics::{names, Metrics};
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
async fn network_byte_counters_track_wire_traffic() {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let game = Arc::new(Mutex::new(GameInstance::new(data, rules, [7u8; 32])));
    let (mut server, cert_der) = GameServer::bind_loopback(Arc::clone(&game)).unwrap();
    let metrics = Arc::new(Mutex::new(Metrics::new()));
    server.set_metrics(Arc::clone(&metrics));
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
        game_id: vec![1, 2, 3],
        character_id: 1,
    };
    send.write_all(&encode_frame(&join)).await.unwrap();
    let _ = read_frame(&mut recv).await;

    // one command + ack
    let env = msg::CommandEnvelope {
        sequence: 1,
        client_tick: 1,
        player_id: 1,
        command: Some(msg::command_envelope::Command::Move(msg::MoveCommand {
            x: 100,
            y: 0,
            movement_mode: 0,
        })),
    };
    send.write_all(&encode_frame(&env)).await.unwrap();
    let _ = read_frame(&mut recv).await;

    // let the server tasks settle
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let (sent, received, sim_commands) = {
        let m = metrics.lock().unwrap();
        (
            m.counter(names::NETWORK_BYTES_SENT),
            m.counter(names::NETWORK_BYTES_RECEIVED),
            m.counter(names::COMMANDS_RECEIVED),
        )
    };
    let received = received as usize;
    assert!(
        received >= hello.encoded_len() + join.encoded_len() + env.encoded_len(),
        "received bytes must cover hello+join+command frames, got {received}"
    );
    // server sent: ServerHello + JoinGameAccepted + CommandAck
    assert!(sent > 0, "sent bytes must be counted");
    assert_eq!(sim_commands, 0, "network metrics are server-side only");

    shutdown_tx.send(()).await.unwrap();
}
