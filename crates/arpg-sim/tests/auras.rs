//! Auras (SPEC.md section 55): state producers refreshed on a data-owned
//! interval, applying their state to entities inside the radius.

use arpg_core::{EntityId, PlayerId, Tick, WorldPos};
use arpg_sim::aura::{AuraDefinition, AuraSystem, TargetFilter};
use arpg_sim::{GameInstance, StackPolicy, StateInstance};
use std::sync::Arc;

fn game() -> GameInstance {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [77u8; 32]);
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst
}

fn fire_aura() -> AuraDefinition {
    AuraDefinition {
        id: 30,
        refresh_interval: 5,
        radius_fp: 4 * 256,
        target_filter: TargetFilter::Party,
        state: GameInstance::STATE_RESIST_FIRE,
        magnitude_bp: 3000,
        duration_ticks: 10,
    }
}

#[test]
fn aura_applies_state_to_players_in_radius() {
    let mut inst = game();
    inst.add_player(PlayerId(2), WorldPos::new(2 * 256, 0));
    inst.add_player(PlayerId(3), WorldPos::new(50 * 256, 0));
    inst.auras.definitions.insert(30, fire_aura());
    inst.auras.activate(EntityId(1), 30, Tick(0));
    for _ in 0..6 {
        inst.tick();
    }
    let p1 = EntityId(1);
    let p2 = EntityId(2);
    let p3 = EntityId(3);
    assert!(inst
        .states
        .get(p1, GameInstance::STATE_RESIST_FIRE, EntityId(1))
        .is_some());
    assert!(inst
        .states
        .get(p2, GameInstance::STATE_RESIST_FIRE, EntityId(1))
        .is_some());
    // player 3 is far outside the radius
    assert!(inst
        .states
        .get(p3, GameInstance::STATE_RESIST_FIRE, EntityId(1))
        .is_none());
    // effective resistance reflects the aura magnitude
    assert_eq!(inst.effective_resists(p1).fire_bp, 3000);
}

#[test]
fn aura_state_expires_without_refresh() {
    let mut inst = game();
    inst.auras.definitions.insert(30, fire_aura());
    inst.auras.activate(EntityId(1), 30, Tick(0));
    // deactivate before the first refresh applies anything
    inst.auras.deactivate(EntityId(1));
    for _ in 0..10 {
        inst.tick();
    }
    assert!(inst
        .states
        .get(EntityId(1), GameInstance::STATE_RESIST_FIRE, EntityId(1))
        .is_none());
}

#[test]
fn aura_refresh_extends_state() {
    let mut inst = game();
    inst.auras.definitions.insert(30, fire_aura());
    inst.auras.activate(EntityId(1), 30, Tick(0));
    for _ in 0..6 {
        inst.tick();
    }
    let e1 = EntityId(1);
    let first = inst
        .states
        .get(e1, GameInstance::STATE_RESIST_FIRE, EntityId(1))
        .copied()
        .unwrap();
    for _ in 0..5 {
        inst.tick();
    }
    let second = inst
        .states
        .get(e1, GameInstance::STATE_RESIST_FIRE, EntityId(1))
        .copied()
        .unwrap();
    assert!(second.expires_tick.unwrap().0 >= first.expires_tick.unwrap().0);
}

#[test]
fn aura_tick_is_deterministic() {
    let run = || {
        let mut inst = game();
        inst.auras.definitions.insert(30, fire_aura());
        inst.auras.activate(EntityId(1), 30, Tick(0));
        for _ in 0..20 {
            inst.tick();
        }
        let p1 = EntityId(1);
        let s = inst
            .states
            .get(p1, GameInstance::STATE_RESIST_FIRE, EntityId(1))
            .copied();
        (inst.state.state_hash(), s.map(|s| s.expires_tick))
    };
    let a = run();
    let b = run();
    assert_eq!(a, b);
}

#[test]
fn manual_state_apply_uses_refresh_policy() {
    let mut sys = AuraSystem::new();
    sys.definitions.insert(30, fire_aura());
    sys.activate(EntityId(1), 30, Tick(0));
    assert_eq!(sys.due(Tick(5)).len(), 1);
    assert_eq!(sys.due(Tick(4)).len(), 0);
    // direct state store interaction: refresh extends expiry
    let mut store = arpg_sim::StateStore::new();
    let inst = StateInstance {
        state: 9,
        source: EntityId(2),
        source_skill: None,
        applied_tick: Tick(1),
        expires_tick: Some(Tick(10)),
        stack_key: (9, 2),
        magnitude_bp: 500,
    };
    store.apply(EntityId(3), inst, StackPolicy::Refresh);
    let inst2 = StateInstance {
        expires_tick: Some(Tick(20)),
        applied_tick: Tick(5),
        ..inst
    };
    store.apply(EntityId(3), inst2, StackPolicy::Refresh);
    assert_eq!(
        store.get(EntityId(3), 9, EntityId(2)).unwrap().expires_tick,
        Some(Tick(20))
    );
}
