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

fn run_scenario(inst: &mut GameInstance) -> Vec<[u8; 32]> {
    for seq in 1..=5u32 {
        let env = CommandEnvelope {
            sequence: seq,
            client_tick: arpg_core::Tick(seq as u64),
            player: PlayerId(1),
            command: ClientCommand::Move(MoveIntent {
                direction: WorldPos::new((seq as i32) * 256, 0),
                movement_mode: MovementMode::Walk,
                sequence: seq,
            }),
        };
        inst.submit_command(env, arpg_core::Tick(seq as u64));
    }
    (0..10).map(|_| inst.tick().state_hash).collect()
}

#[test]
fn same_seed_same_commands_same_hashes() {
    let mut a = setup();
    let mut b = setup();
    assert_eq!(run_scenario(&mut a), run_scenario(&mut b));
}

#[test]
fn duplicate_sequence_is_rejected() {
    let mut inst = setup();
    let env = CommandEnvelope {
        sequence: 1,
        client_tick: arpg_core::Tick(1),
        player: PlayerId(1),
        command: ClientCommand::NoOp,
    };
    assert!(inst.submit_command(env.clone(), arpg_core::Tick(2)));
    assert!(!inst.submit_command(env, arpg_core::Tick(2)));
}
