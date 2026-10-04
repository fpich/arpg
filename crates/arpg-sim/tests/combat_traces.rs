//! Combat and loot traces (SPEC.md sections 167-168): every attack explains
//! its roll, every drop is auditable, and traces never change the hash.

use arpg_core::{EntityId, PlayerId, WorldPos};
use arpg_sim::GameInstance;
use std::sync::Arc;

fn game() -> GameInstance {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [77u8; 32]);
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst
}

#[test]
fn attack_trace_explains_the_roll() {
    let mut inst = game();
    let monster = inst.spawn_monster_def(arpg_core::MonsterDefId(1), WorldPos::new(0, 0));
    let before = inst.state.players.get(&PlayerId(1)).unwrap().life;

    inst.monster_attack(monster, EntityId(1));

    let trace = inst.traces.recent_attacks().next().unwrap().clone();
    assert_eq!(trace.source, monster);
    assert_eq!(trace.target, EntityId(1));
    assert!(
        trace.chance_bp >= 500 && trace.chance_bp <= 9500,
        "CTH clamped 5%..95%"
    );
    assert!(trace.roll_bp < 9500);
    let after = inst.state.players.get(&PlayerId(1)).unwrap().life;
    if trace.hit {
        assert_eq!(before - after, trace.physical_raw);
        assert!(trace.final_damage > 0);
    } else {
        assert_eq!(before, after, "a miss applies no damage");
        assert_eq!(trace.final_damage, 0);
    }
    // section 167 format: rendered text mentions every field
    let rendered = trace.render();
    assert!(rendered.contains("Attack #"));
    assert!(rendered.contains("AR"));
    assert!(rendered.contains("Defense"));
    assert!(rendered.contains("Chance"));
    assert!(rendered.contains("Roll"));
}

#[test]
fn combat_rolls_are_deterministic() {
    let run = |seed: [u8; 32]| {
        let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
        let rules = Arc::new(arpg_rules::GameRules::default());
        let mut inst = GameInstance::new(data, rules, seed);
        inst.add_player(PlayerId(1), WorldPos::new(0, 0));
        let monster = inst.spawn_monster_def(arpg_core::MonsterDefId(1), WorldPos::new(0, 0));
        let mut hits = Vec::new();
        for _ in 0..20 {
            inst.monster_attack(monster, EntityId(1));
        }
        for t in inst.traces.recent_attacks() {
            hits.push((t.roll_bp, t.hit));
        }
        hits
    };
    assert_eq!(run([1u8; 32]), run([1u8; 32]));
    // different seeds produce different rolls (not all-identical)
    let a = run([1u8; 32]);
    let b = run([2u8; 32]);
    assert!(a.iter().zip(b.iter()).any(|(x, y)| x.0 != y.0));
}

#[test]
fn loot_trace_audits_every_death() {
    let mut inst = game();
    // spawn with a treasure class so a death produces a trace
    let monster = inst.spawn_monster_with_tc(arpg_core::WorldPos::new(0, 0), Some(build_tc()));
    inst.apply_admin_command(&arpg_sim::admin::AdminCommand::Kill { entity: monster })
        .unwrap();
    inst.tick();

    let traces: Vec<_> = inst.traces.recent_loots().cloned().collect();
    assert!(
        !traces.is_empty(),
        "a death with a TC must produce a loot trace"
    );
    let t = &traces[0];
    assert_eq!(t.monster, monster);
    let rendered = t.render();
    assert!(rendered.contains("DeathEvent"));
    assert!(rendered.contains("DropSeed"));
    assert!(rendered.contains("TC"));
    // the outcome is recorded either as a minted item or an explicit no-drop
    assert!(t.item.is_some() || rendered.contains("ItemId none"));
}

fn build_tc() -> arpg_sim::item::TreasureClass {
    use arpg_sim::item::*;
    TreasureClass {
        picks: 1,
        no_drop_weight: 0,
        entries: vec![WeightedTreasureEntry {
            weight: 100,
            kind: TreasureKind::Item(arpg_core::ItemDefId(1)),
        }],
    }
}

#[test]
fn traces_never_change_the_state_hash() {
    // POLICY domain (sections 166-168): recording traces must not alter
    // gameplay. Two instances with identical inputs hash identically
    // regardless of the trace buffer contents.
    let run = || {
        let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
        let rules = Arc::new(arpg_rules::GameRules::default());
        let mut inst = GameInstance::new(data, rules, [31u8; 32]);
        inst.add_player(PlayerId(1), WorldPos::new(0, 0));
        let monster = inst.spawn_monster_def(arpg_core::MonsterDefId(1), WorldPos::new(0, 0));
        for _ in 0..5 {
            inst.monster_attack(monster, EntityId(1));
        }
        let mut hash = [0u8; 32];
        for _ in 0..10 {
            hash = inst.tick().state_hash;
        }
        hash
    };
    assert_eq!(run(), run());
}
