//! Per-species AI behavior from the datapack (SPEC.md sections 57, 60, 63):
//! aggro range comes from the monster definition, not from a global
//! parameter. A short-sighted species ignores a distant player; a
//! far-sighted one engages.

use arpg_ai::{AiParams, HfsmBrain};
use arpg_core::{EntityId, PlayerId, WorldPos};
use arpg_sim::GameInstance;
use std::sync::Arc;

fn game_with_brain() -> GameInstance {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [41u8; 32]);
    inst.set_ai_brain(Box::new(HfsmBrain::new(AiParams::default())));
    inst
}

#[test]
fn aggro_range_comes_from_the_definition() {
    // Cliff Lurker (def 10): aggro_range 4 tiles — a player at 6 tiles
    // stays unnoticed. Vulture Demon (def 9): aggro_range 9 — engages.
    let mut inst = game_with_brain();
    inst.add_player(PlayerId(1), WorldPos::new(6, 0));
    let lurker = inst.spawn_monster_def(arpg_core::MonsterDefId(10), WorldPos::new(0, 0));
    // run enough ticks for several think intervals
    for _ in 0..60 {
        inst.tick();
    }
    let lurker_pos = inst.monsters.get(&lurker).unwrap().pos;
    // the lurker may patrol within ~2 tiles of home, but must never close
    // the gap to the player at x=6 (aggro 4 < distance 6)
    let home_dx = lurker_pos.x.abs();
    assert!(
        home_dx <= 2 * 256,
        "aggro 4 < distance 6: the Cliff Lurker never engages ({lurker_pos:?})"
    );
}

#[test]
fn far_sighted_species_engages() {
    let mut inst = game_with_brain();
    inst.add_player(PlayerId(1), WorldPos::new(6, 0));
    let vulture = inst.spawn_monster_def(arpg_core::MonsterDefId(9), WorldPos::new(0, 0));
    for _ in 0..60 {
        inst.tick();
    }
    let pos = inst.monsters.get(&vulture).unwrap().pos;
    assert!(
        pos != WorldPos::new(0, 0),
        "aggro 9 > distance 6: the Vulture Demon moves to engage ({pos:?})"
    );
}

#[test]
fn per_species_ai_stays_deterministic() {
    let run = || {
        let mut inst = game_with_brain();
        inst.add_player(PlayerId(1), WorldPos::new(6, 0));
        inst.spawn_monster_def(arpg_core::MonsterDefId(9), WorldPos::new(0, 0));
        inst.spawn_monster_def(arpg_core::MonsterDefId(10), WorldPos::new(0, 2));
        inst.spawn_monster_def(arpg_core::MonsterDefId(0), WorldPos::new(0, 4));
        let mut hash = [0u8; 32];
        for _ in 0..40 {
            hash = inst.tick().state_hash;
        }
        hash
    };
    assert_eq!(run(), run());
}

#[test]
fn monster_stats_reach_the_view() {
    let mut inst = game_with_brain();
    let vulture = inst.spawn_monster_def(arpg_core::MonsterDefId(9), WorldPos::new(0, 0));
    let m = inst.monsters.get(&vulture).unwrap();
    assert_eq!(m.aggro_range, 9);
    assert!(m.ranged);
    let lurker = inst.spawn_monster_def(arpg_core::MonsterDefId(10), WorldPos::new(0, 0));
    let m = inst.monsters.get(&lurker).unwrap();
    assert_eq!(m.aggro_range, 4);
    assert!(!m.ranged);
    let _ = EntityId(0);
}
