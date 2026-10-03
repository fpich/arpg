//! Load scenarios and performance budgets (SPEC.md sections 176-180):
//! melee players, projectile-heavy players, dense monster packs, loot
//! explosion, massive AoE. The scenarios measure per-tick simulation time
//! (p50/p95, hard budget 40 ms at 25 Hz) and prove that the server never
//! degrades gameplay to catch up: identical inputs always produce the
//! identical state hash regardless of load.

use crate::FuzzRng;
use arpg_core::{PlayerId, Tick, WorldPos};
use arpg_sim::damage::{DamagePacket, DamageRange};
use arpg_sim::{
    ClientCommand, CommandEnvelope, CostFormula, GameInstance, MoveIntent, MovementMode,
    SkillDefinition, SkillOp, SkillProgram, TargetingSpec, TimingFormula, UseSkillIntent,
};
use std::sync::Arc;
use std::time::Duration;

/// Hard budget per tick: 25 Hz reference machine (section 176).
pub const HARD_BUDGET: Duration = Duration::from_millis(40);
/// Warmup ticks excluded from measurements (JIT, allocator, caches).
pub const WARMUP_TICKS: u32 = 20;

pub struct TickStats {
    pub p50: Duration,
    pub p95: Duration,
    pub max: Duration,
    pub samples: u32,
}

pub fn percentiles(mut durations: Vec<Duration>) -> TickStats {
    durations.sort();
    let idx = |p: f64| -> usize {
        if durations.is_empty() {
            0
        } else {
            (((durations.len() as f64) * p).ceil() as usize).clamp(1, durations.len()) - 1
        }
    };
    TickStats {
        p50: durations[idx(0.50)],
        p95: durations[idx(0.95)],
        max: durations.last().copied().unwrap_or(Duration::ZERO),
        samples: durations.len() as u32,
    }
}

fn reference_game() -> GameInstance {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut game = GameInstance::new(data, rules, [11u8; 32]);
    let melee = SkillDefinition {
        id: arpg_core::SkillId(1),
        targeting: TargetingSpec::Entity,
        cost: CostFormula::default(),
        timing: TimingFormula::Instant,
        program: SkillProgram {
            ops: vec![SkillOp::DealDamage(DamagePacket {
                physical: DamageRange::new(9999, 9999),
                ..Default::default()
            })],
        },
    };
    let barrage = SkillDefinition {
        id: arpg_core::SkillId(2),
        targeting: TargetingSpec::Position,
        cost: CostFormula::default(),
        timing: TimingFormula::Instant,
        program: SkillProgram {
            ops: (0..8).map(|_| SkillOp::SpawnMissile(1)).collect::<Vec<_>>(),
        },
    };
    game.register_skill(melee).unwrap();
    game.register_skill(barrage).unwrap();
    game
}

fn command(player: u32, seq: u32, cmd: ClientCommand) -> CommandEnvelope {
    CommandEnvelope {
        sequence: seq,
        client_tick: Tick(0),
        player: PlayerId(player),
        command: cmd,
    }
}

fn move_cmd(player: u32, seq: u32, dir: WorldPos) -> CommandEnvelope {
    command(
        player,
        seq,
        ClientCommand::Move(MoveIntent {
            direction: dir,
            movement_mode: MovementMode::Run,
            sequence: seq,
        }),
    )
}

fn skill_cmd(player: u32, seq: u32, skill: arpg_core::SkillId) -> CommandEnvelope {
    command(
        player,
        seq,
        ClientCommand::UseSkill(UseSkillIntent {
            skill,
            target: Some(WorldPos::new(20, 20)),
        }),
    )
}

fn run_scenario(
    setup: impl FnOnce(&mut GameInstance, &mut FuzzRng),
    drive: impl Fn(&mut GameInstance, &mut FuzzRng, u32),
    ticks: u32,
) -> TickStats {
    let mut game = reference_game();
    let mut rng = FuzzRng(4242);
    setup(&mut game, &mut rng);
    let mut durations = Vec::new();
    let mut seq = 0u32;
    for t in 0..ticks {
        drive(&mut game, &mut rng, t);
        let start = std::time::Instant::now();
        game.tick();
        let elapsed = start.elapsed();
        if t >= WARMUP_TICKS {
            durations.push(elapsed);
        }
        seq = seq.wrapping_add(1);
    }
    percentiles(durations)
}

/// 8 melee players whirling into a dense monster pack (section 179).
pub fn scenario_melee(players: u32, monsters: u32, ticks: u32) -> TickStats {
    run_scenario(
        |game, _| {
            for p in 1..=players {
                game.add_player(PlayerId(p), WorldPos::new(p as i32 * 3, 0));
            }
            for m in 0..monsters {
                game.spawn_monster(WorldPos::new((m % 40) as i32, 10 + (m / 40) as i32));
            }
        },
        |game, rng, t| {
            for p in 1..=players {
                let dir = WorldPos::new(rng.below(3) as i32 - 1, rng.below(3) as i32 - 1);
                game.submit_command(move_cmd(p, t * players + p, dir));
                if rng.below(2) == 0 {
                    game.submit_command(skill_cmd(p, t * players + p, arpg_core::SkillId(1)));
                }
            }
        },
        ticks,
    )
}

/// 8 projectile-heavy players each firing an 8-missile barrage every other
/// tick (section 179: projectile-heavy).
pub fn scenario_projectiles(players: u32, monsters: u32, ticks: u32) -> TickStats {
    run_scenario(
        |game, _| {
            for p in 1..=players {
                game.add_player(PlayerId(p), WorldPos::new(p as i32 * 3, 0));
            }
            for m in 0..monsters {
                game.spawn_monster(WorldPos::new((m % 40) as i32, 10 + (m / 40) as i32));
            }
        },
        |game, rng, t| {
            for p in 1..=players {
                let dir = WorldPos::new(rng.below(3) as i32 - 1, 0);
                game.submit_command(move_cmd(p, t * players + p, dir));
                if t % 2 == 0 {
                    game.submit_command(skill_cmd(p, t * players + p, arpg_core::SkillId(2)));
                }
            }
        },
        ticks,
    )
}

/// Dense monster pack with continuous movement (section 179).
pub fn scenario_dense_pack(monsters: u32, ticks: u32) -> TickStats {
    run_scenario(
        |game, _| {
            game.add_player(PlayerId(1), WorldPos::new(0, 0));
            for m in 0..monsters {
                game.spawn_monster(WorldPos::new((m % 64) as i32, (m / 64) as i32));
            }
        },
        |game, rng, t| {
            let dir = WorldPos::new(rng.below(3) as i32 - 1, rng.below(3) as i32 - 1);
            game.submit_command(move_cmd(1, t, dir));
        },
        ticks,
    )
}

/// Loot explosion: a pack dies simultaneously, resolving drops for every
/// monster in a few ticks (section 179).
pub fn scenario_loot_explosion(monsters: u32, ticks: u32) -> TickStats {
    run_scenario(
        |game, _| {
            game.add_player(PlayerId(1), WorldPos::new(0, 0));
            for m in 0..monsters {
                let tc = arpg_sim::TreasureClass {
                    picks: 3,
                    no_drop_weight: 0,
                    entries: vec![arpg_sim::WeightedTreasureEntry {
                        weight: 1,
                        kind: arpg_sim::TreasureKind::Item(arpg_core::ItemDefId(1)),
                    }],
                };
                game.spawn_monster_with_tc(
                    WorldPos::new((m % 32) as i32, (m / 32) as i32),
                    Some(tc),
                );
            }
        },
        |game, _, t| {
            if t == WARMUP_TICKS {
                for monster in game.monsters.keys().copied().collect::<Vec<_>>() {
                    game.apply_damage(monster, arpg_core::EntityId(1), i64::MAX / 2);
                }
            }
        },
        ticks,
    )
}

/// Massive AoE: an area skill resolves against a dense pack (section 179).
pub fn scenario_massive_aoe(players: u32, monsters: u32, ticks: u32) -> TickStats {
    run_scenario(
        |game, _| {
            for p in 1..=players {
                game.add_player(PlayerId(p), WorldPos::new(p as i32, 0));
            }
            for m in 0..monsters {
                game.spawn_monster(WorldPos::new((m % 48) as i32, 1 + (m / 48) as i32));
            }
        },
        |game, _, t| {
            for p in 1..=players {
                if t % 4 == 0 {
                    game.submit_command(skill_cmd(p, t * players + p, arpg_core::SkillId(1)));
                }
            }
        },
        ticks,
    )
}

/// Deterministic identical-load run: the same input stream on two instances
/// yields identical hashes, whatever the load (section 180: load never
/// changes gameplay results).
pub fn determinism_under_load(ticks: u32) -> ([u8; 32], [u8; 32]) {
    let run = || {
        let mut game = reference_game();
        let mut rng = FuzzRng(99);
        for p in 1..=8u32 {
            game.add_player(PlayerId(p), WorldPos::new(p as i32, 0));
        }
        for m in 0..64 {
            game.spawn_monster(WorldPos::new(m % 16, 5 + m / 16));
        }
        let mut hash = [0u8; 32];
        for t in 0..ticks {
            for p in 1..=8u32 {
                if rng.below(2) == 0 {
                    game.submit_command(move_cmd(p, t * 8 + p, WorldPos::new(1, 0)));
                }
                if rng.below(3) == 0 {
                    game.submit_command(skill_cmd(p, t * 8 + p, arpg_core::SkillId(2)));
                }
            }
            hash = game.tick().state_hash;
        }
        hash
    };
    (run(), run())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_budget(stats: &TickStats, name: &str) {
        assert!(
            stats.max < HARD_BUDGET,
            "{name}: max tick {:?} exceeded hard budget {HARD_BUDGET:?} (p50 {:?}, p95 {:?}, n={})",
            stats.max,
            stats.p50,
            stats.p95,
            stats.samples
        );
    }

    #[test]
    fn melee_scenario_stays_within_budget() {
        let stats = scenario_melee(8, 120, 200);
        assert_budget(&stats, "melee");
    }

    #[test]
    fn projectile_scenario_stays_within_budget() {
        let stats = scenario_projectiles(8, 120, 200);
        assert_budget(&stats, "projectiles");
    }

    #[test]
    fn dense_pack_scenario_stays_within_budget() {
        let stats = scenario_dense_pack(500, 200);
        assert_budget(&stats, "dense_pack");
    }

    #[test]
    fn loot_explosion_scenario_stays_within_budget() {
        let stats = scenario_loot_explosion(200, 60);
        assert_budget(&stats, "loot_explosion");
    }

    #[test]
    fn massive_aoe_scenario_stays_within_budget() {
        let stats = scenario_massive_aoe(8, 300, 200);
        assert_budget(&stats, "massive_aoe");
    }

    #[test]
    fn load_never_changes_gameplay_results() {
        let (a, b) = determinism_under_load(100);
        assert_eq!(a, b);
    }
}
