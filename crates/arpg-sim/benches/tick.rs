//! Simulation tick benchmark (SPEC.md sections 175-176): publishes the
//! per-game simulation cost against the reference budget (p50 < 3 ms,
//! p95 < 8 ms, hard budget 40 ms at 25 Hz for 100 active games).
//! Run with `cargo bench -p arpg-sim` and record the machine profile
//! (CPU model, cores, RAM, OS, Rust version, build profile, datapack
//! hash, scenario hash) next to the numbers (section 175).

use arpg_core::{PlayerId, WorldPos};
use arpg_sim::{ClientCommand, CommandEnvelope, GameInstance, MoveIntent, MovementMode};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::sync::Arc;

fn setup_game(players: u32, monsters: u32) -> GameInstance {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [21u8; 32]);
    inst.register_datapack_skills().unwrap();
    for p in 1..=players {
        inst.add_player(PlayerId(p), WorldPos::new(p as i32 * 512, 0));
    }
    for i in 0..monsters {
        inst.spawn_monster_def(
            arpg_core::MonsterDefId(0),
            WorldPos::new(1024 + (i as i32) * 256, (i as i32 % 4) * 256),
        );
    }
    inst
}

fn bench_tick(c: &mut Criterion) {
    for (players, monsters) in [(1u32, 0u32), (1, 30), (8, 30), (8, 120)] {
        let mut inst = setup_game(players, monsters);
        for seq in 1..=players {
            inst.submit_command(CommandEnvelope {
                sequence: seq,
                client_tick: inst.state.tick,
                player: PlayerId(seq),
                command: ClientCommand::Move(MoveIntent {
                    direction: WorldPos::new(seq as i32 * 512 + 256, 256),
                    movement_mode: MovementMode::Walk,
                    sequence: seq as u32,
                }),
            });
        }
        let cell = std::cell::RefCell::new(inst);
        c.bench_with_input(
            BenchmarkId::new("tick", format!("p{players}m{monsters}")),
            &(),
            |b, _| {
                b.iter(|| {
                    cell.borrow_mut().tick();
                });
            },
        );
    }
}

criterion_group!(benches, bench_tick);
criterion_main!(benches);
