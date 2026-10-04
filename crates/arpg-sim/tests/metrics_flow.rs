//! Metrics wiring (SPEC.md section 190): the tick feeds the reference
//! gauges/durations, command admission feeds the command counters, and the
//! metrics collector never changes the state hash (POLICY domain).

use arpg_core::{PlayerId, WorldPos};
use arpg_metrics::{names, Metrics};
use arpg_sim::{ClientCommand, CommandEnvelope, MoveIntent, MovementMode};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn game_with_metrics() -> (arpg_sim::GameInstance, Arc<Mutex<Metrics>>) {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = arpg_sim::GameInstance::new(data, rules, [31u8; 32]);
    let metrics = Arc::new(Mutex::new(Metrics::new()));
    inst.metrics = Some(Arc::clone(&metrics));
    (inst, metrics)
}

fn move_cmd(player: u32, seq: u32) -> CommandEnvelope {
    CommandEnvelope {
        sequence: seq,
        client_tick: arpg_core::Tick(0),
        player: PlayerId(player),
        command: ClientCommand::Move(MoveIntent {
            direction: WorldPos::new(1, 0),
            movement_mode: MovementMode::Run,
            sequence: seq,
        }),
    }
}

#[test]
fn tick_feeds_reference_metrics() {
    let (mut inst, metrics) = game_with_metrics();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    inst.spawn_monster_def(arpg_core::MonsterDefId(0), WorldPos::new(3, 0));
    for _ in 0..5 {
        inst.tick();
    }
    let m = metrics.lock().unwrap();
    assert_eq!(m.gauge(names::GAME_ENTITY_COUNT), 2.0);
    assert_eq!(m.gauge(names::GAME_MONSTER_COUNT), 1.0);
    assert_eq!(m.gauge(names::GAME_MISSILE_COUNT), 0.0);
    assert!(m.duration_mean_secs(names::GAME_TICK_SECONDS) > 0.0);
}

#[test]
fn command_counters_track_admission() {
    let (mut inst, metrics) = game_with_metrics();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    inst.submit_command(move_cmd(1, 1));
    inst.submit_command(move_cmd(1, 1)); // duplicate
    inst.submit_command(move_cmd(1, 2));
    for _ in 0..10 {
        inst.tick();
    }
    let m = metrics.lock().unwrap();
    assert_eq!(m.counter(names::COMMANDS_RECEIVED), 3);
    assert_eq!(m.counter(names::COMMANDS_REJECTED), 1, "duplicate rejected");
    assert!(m.counter(names::COMMANDS_ACCEPTED) >= 1);
}

#[test]
fn metrics_never_change_the_state_hash() {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let run = |with_metrics: bool| {
        let mut inst =
            arpg_sim::GameInstance::new(Arc::clone(&data), Arc::clone(&rules), [7u8; 32]);
        if with_metrics {
            inst.metrics = Some(Arc::new(Mutex::new(Metrics::new())));
        }
        inst.add_player(PlayerId(1), WorldPos::ZERO);
        inst.spawn_monster_def(arpg_core::MonsterDefId(3), WorldPos::new(4, 0));
        inst.submit_command(move_cmd(1, 1));
        let mut hash = [0u8; 32];
        for _ in 0..10 {
            hash = inst.tick().state_hash;
        }
        hash
    };
    assert_eq!(run(false), run(true), "metrics are POLICY, not gameplay");
}

#[test]
fn bestiary_spawns_with_definition_stats() {
    let (mut inst, _metrics) = game_with_metrics();
    // Fallen (act 1, weakest)
    let fallen = inst.spawn_monster_def(arpg_core::MonsterDefId(0), WorldPos::ZERO);
    let m = inst.monsters.get(&fallen).unwrap();
    assert_eq!(m.damage, 6);
    assert_eq!(m.experience, 15);
    // Baal Minion (act 5)
    let minion = inst.spawn_monster_def(arpg_core::MonsterDefId(29), WorldPos::ZERO);
    let m = inst.monsters.get(&minion).unwrap();
    assert_eq!(m.damage, 48);
    assert_eq!(m.experience, 160);
    assert!(m.life > inst.monsters.get(&fallen).unwrap().life);
    // Blood Raven (boss)
    let boss = inst.spawn_monster_def(arpg_core::MonsterDefId(1000), WorldPos::ZERO);
    let m = inst.monsters.get(&boss).unwrap();
    assert_eq!(m.damage, 25);
    assert_eq!(m.experience, 200);
}

#[test]
fn monster_damage_uses_definition_value() {
    let (mut inst, _metrics) = game_with_metrics();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    // Vulture Demon: ranged, damage 14
    let vulture = inst.spawn_monster_def(arpg_core::MonsterDefId(9), WorldPos::ZERO);
    let life_before = inst.state.players.get(&PlayerId(1)).unwrap().life;
    inst.monster_attack(vulture, arpg_core::EntityId(1));
    let life_after = inst.state.players.get(&PlayerId(1)).unwrap().life;
    assert_eq!(
        life_before - life_after,
        14,
        "damage comes from the definition"
    );
}

#[test]
fn bestiary_xp_flows_through_the_pipeline() {
    let (mut inst, _metrics) = game_with_metrics();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    let zombie = inst.spawn_monster_def(arpg_core::MonsterDefId(2), WorldPos::ZERO);
    // kill: zombie has 40+30*0+12*2 = 64 life (act 0)
    inst.apply_damage(zombie, arpg_core::EntityId(1), i64::MAX / 2);
    inst.tick();
    let xp = inst.state.players.get(&PlayerId(1)).unwrap().experience;
    assert_eq!(xp, 18, "zombie xp = 18 from the definition");
}

#[test]
fn tick_metrics_observed_under_load() {
    let (mut inst, metrics) = game_with_metrics();
    for p in 1..=8u32 {
        inst.add_player(PlayerId(p), WorldPos::new(p as i32, 0));
        for m in 0..10 {
            inst.spawn_monster_def(
                arpg_core::MonsterDefId(m),
                WorldPos::new(p as i32 + m as i32, 5),
            );
        }
    }
    for t in 0..20 {
        for p in 1..=8u32 {
            inst.submit_command(move_cmd(p, t * 8 + p));
        }
        inst.tick();
    }
    let m = metrics.lock().unwrap();
    assert_eq!(m.gauge(names::GAME_MONSTER_COUNT), 80.0);
    assert_eq!(m.counter(names::COMMANDS_RECEIVED), 160);
    assert!(
        m.duration_max_secs(names::GAME_TICK_SECONDS) < 0.04,
        "hard budget"
    );
    let _ = Duration::ZERO;
}
