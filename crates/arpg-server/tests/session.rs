use arpg_core::{PlayerId, Tick};
use arpg_protocol::messages as msg;
use arpg_server::{Session, SessionState, PROTOCOL_VERSION};
use prost::Message;

#[test]
fn handshake_rejects_incompatible_protocol() {
    let mut session = Session::new(PlayerId(1));
    let bad = msg::ClientHello {
        protocol_version: PROTOCOL_VERSION.0 + 1,
        client_build: 1,
        supported_features: vec![],
    };
    assert!(!session.handle_client_hello(&bad));
    assert_eq!(session.state, SessionState::Connecting);

    let good = msg::ClientHello {
        protocol_version: PROTOCOL_VERSION.0,
        client_build: 1,
        supported_features: vec!["movement".into()],
    };
    assert!(session.handle_client_hello(&good));
    assert_eq!(session.state, SessionState::Authenticated);
}

#[test]
fn commands_only_accepted_while_running() {
    let mut session = Session::new(PlayerId(1));
    session.handle_client_hello(&msg::ClientHello {
        protocol_version: PROTOCOL_VERSION.0,
        client_build: 1,
        supported_features: vec![],
    });
    assert!(!session.is_command_accepted());

    session.handle_join_accepted(Tick(10));
    assert!(session.is_command_accepted());

    session.disconnect();
    assert_eq!(session.state, SessionState::DisconnectedGrace);
    assert!(!session.is_command_accepted(), "no new action during grace");

    session.close();
    assert_eq!(session.state, SessionState::Closed);
}

#[test]
fn wire_roundtrip_preserves_intention() {
    let wire = msg::CommandEnvelope {
        sequence: 7,
        client_tick: 42,
        player_id: 3,
        command: Some(msg::command_envelope::Command::Move(msg::MoveCommand {
            x: 1234,
            y: -567,
            movement_mode: 1,
        })),
    };
    let bytes = wire.encode_to_vec();
    let decoded = msg::CommandEnvelope::decode(bytes.as_slice()).unwrap();
    let sim = arpg_server::bridge::wire_to_sim(&decoded).unwrap();

    assert_eq!(sim.sequence, 7);
    assert_eq!(sim.client_tick, Tick(42));
    assert_eq!(sim.player, PlayerId(3));
    match sim.command {
        arpg_sim::ClientCommand::Move(intent) => {
            assert_eq!(intent.direction.x, 1234);
            assert_eq!(intent.direction.y, -567);
            assert_eq!(intent.movement_mode, arpg_sim::MovementMode::Run);
        }
        other => panic!("expected move, got {other:?}"),
    }
}

#[test]
fn snapshot_contains_only_client_visible_state() {
    use std::collections::BTreeMap;
    let mut players = BTreeMap::new();
    players.insert(
        PlayerId(1),
        arpg_sim::PlayerState {
            player: PlayerId(1),
            pos: arpg_core::WorldPos::new(10, 20),
            life: 90,
            mana: 40,
            level: 1,
            experience: 0,
        },
    );
    let snap = arpg_server::bridge::snapshot_to_wire(1, Tick(5), &players, 9);
    assert_eq!(snap.players.len(), 1);
    assert_eq!(snap.players[0].x, 10);
    assert_eq!(snap.last_processed_command_sequence, 9);

    // decode roundtrip
    let bytes = prost::Message::encode_to_vec(&snap);
    let decoded = msg::Snapshot::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded.players[0].life, 90);
}
