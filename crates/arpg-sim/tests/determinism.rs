use arpg_core::{PlayerId, WorldPos};
use arpg_sim::{ClientCommand, CommandEnvelope, GameInstance, MoveIntent, MovementMode};
use std::sync::Arc;

fn setup() -> GameInstance {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [42u8; 32]);
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst
}

fn run_scenario(inst: &mut GameInstance, order: &[(u32, i32)]) -> Vec<[u8; 32]> {
    for &(seq, x) in order {
        let env = CommandEnvelope {
            sequence: seq,
            client_tick: arpg_core::Tick(1),
            player: PlayerId(1),
            command: ClientCommand::Move(MoveIntent {
                direction: WorldPos::new(x, 0),
                movement_mode: MovementMode::Walk,
                sequence: seq,
            }),
        };
        inst.submit_command(env);
    }
    (0..10).map(|_| inst.tick().state_hash).collect()
}

#[test]
fn same_seed_same_commands_same_hashes() {
    let order: Vec<(u32, i32)> = (1..=5u32).map(|s| (s, (s as i32) * 256)).collect();
    let mut a = setup();
    let mut b = setup();
    assert_eq!(run_scenario(&mut a, &order), run_scenario(&mut b, &order));
}

#[test]
fn cross_player_arrival_order_same_hashes() {
    // SPEC.md section 134: after admission, real network arrival order has
    // no influence. Per-player sequences stay monotonic (section 133), but
    // packets of different players arrive in opposite orders.
    let mut a = setup();
    let mut b = setup();
    a.add_player(PlayerId(2), WorldPos::new(0, 0));
    b.add_player(PlayerId(2), WorldPos::new(0, 0));

    fn env(player: u32, seq: u32, x: i32) -> CommandEnvelope {
        CommandEnvelope {
            sequence: seq,
            client_tick: arpg_core::Tick(1),
            player: PlayerId(player),
            command: ClientCommand::Move(MoveIntent {
                direction: WorldPos::new(x, x),
                movement_mode: MovementMode::Walk,
                sequence: seq,
            }),
        }
    }

    // run A: player 1 packets first; run B: player 2 packets first.
    for seq in 1..=4u32 {
        a.submit_command(env(1, seq, (seq as i32) * 100));
        a.submit_command(env(2, seq, (seq as i32) * 200));
    }
    for seq in 1..=4u32 {
        b.submit_command(env(2, seq, (seq as i32) * 200));
        b.submit_command(env(1, seq, (seq as i32) * 100));
    }
    for _ in 0..10 {
        assert_eq!(a.tick().state_hash, b.tick().state_hash);
    }
}

#[test]
fn two_players_deterministic() {
    let mut a = setup();
    let mut b = setup();
    a.add_player(PlayerId(2), WorldPos::new(10, 10));
    b.add_player(PlayerId(2), WorldPos::new(10, 10));

    for seq in 1..=4u32 {
        for player in [1u32, 2u32] {
            let env = CommandEnvelope {
                sequence: seq,
                client_tick: arpg_core::Tick(1),
                player: PlayerId(player),
                command: ClientCommand::Move(MoveIntent {
                    direction: WorldPos::new((seq as i32 + player as i32) * 100, 5),
                    movement_mode: MovementMode::Run,
                    sequence: seq,
                }),
            };
            a.submit_command(env.clone());
            b.submit_command(env);
        }
    }
    for _ in 0..10 {
        assert_eq!(a.tick().state_hash, b.tick().state_hash);
    }
}
