use arpg_core::{ActionPhase, ActorMode, EntityId, StatId, Tick};
use arpg_sim::{
    ActionTiming, Actor, GameInstance, InterruptPriority, ModifierOp, ModifierSource, StackPolicy,
    StatBlock, StatModifier, StateInstance, StateStore, Target,
};
use std::sync::Arc;

const STRENGTH: StatId = StatId(1);
const LIFE: StatId = StatId(2);

fn modifier(stat: StatId, op: ModifierOp, value: i64, seq: u64) -> StatModifier {
    StatModifier {
        stat,
        source: ModifierSource::Skill(1),
        source_sequence: seq,
        operation: op,
        value,
        priority: 0,
    }
}

#[test]
fn stat_pipeline_order_is_normative() {
    let mut block = StatBlock::new();
    block.set_base(STRENGTH, 100);
    block.add_modifier(modifier(STRENGTH, ModifierOp::FlatAdd, 50, 1));
    block.add_modifier(modifier(STRENGTH, ModifierOp::PercentAddBp, 10_000, 2));
    block.add_modifier(modifier(STRENGTH, ModifierOp::MultiplyBp, 15_000, 3));
    // base 100 -> flat 150 -> percent +100% = 300 -> *1.5 = 450
    assert_eq!(block.compute(STRENGTH), 450);
}

#[test]
fn stat_clamps_apply_last() {
    let mut block = StatBlock::new();
    block.set_base(LIFE, 100);
    block.add_modifier(modifier(LIFE, ModifierOp::FlatAdd, 200, 1));
    block.add_modifier(StatModifier {
        stat: LIFE,
        source: ModifierSource::Difficulty(1),
        source_sequence: 2,
        operation: ModifierOp::MaxClamp,
        value: 250,
        priority: 0,
    });
    assert_eq!(block.compute(LIFE), 250);
}

#[test]
fn stat_insertion_order_is_irrelevant() {
    let build = |reversed: bool| {
        let mut block = StatBlock::new();
        block.set_base(STRENGTH, 100);
        let mods = vec![
            modifier(STRENGTH, ModifierOp::MultiplyBp, 15_000, 3),
            modifier(STRENGTH, ModifierOp::FlatAdd, 50, 1),
            modifier(STRENGTH, ModifierOp::PercentAddBp, 10_000, 2),
        ];
        if reversed {
            for m in mods.into_iter().rev() {
                block.add_modifier(m);
            }
        } else {
            for m in mods {
                block.add_modifier(m);
            }
        }
        block.compute(STRENGTH)
    };
    assert_eq!(build(false), build(true));
}

#[test]
fn action_advances_through_phases() {
    let mut actor = Actor::new(EntityId(1));
    let timing = ActionTiming {
        windup_ticks: 2,
        impact_tick: 2,
        recovery_ticks: 3,
    };
    actor
        .start_action(ActorMode::Attack, Tick(0), Target::None, timing)
        .unwrap();
    let action = actor.action.as_ref().unwrap();
    assert_eq!(action.phase, ActionPhase::Windup);

    let action = actor.action.as_mut().unwrap();
    action.advance(Tick(1));
    assert_eq!(action.phase, ActionPhase::Windup);
    action.advance(Tick(2));
    assert_eq!(action.phase, ActionPhase::Impact);
    assert!(!action.advance(Tick(2)));
    assert_eq!(action.phase, ActionPhase::Recovery);
    assert!(action.advance(Tick(5)));
    assert_eq!(action.phase, ActionPhase::Complete);
}

#[test]
fn interrupt_priority_ordering() {
    let mut actor = Actor::new(EntityId(1));
    let timing = ActionTiming {
        windup_ticks: 10,
        impact_tick: 10,
        recovery_ticks: 10,
    };
    actor
        .start_action(ActorMode::Attack, Tick(0), Target::None, timing)
        .unwrap();
    let action = actor.action.as_mut().unwrap();
    assert!(action.request_interrupt(InterruptPriority::HitRecovery));
    // a lower-priority interruption never replaces a higher one
    assert!(!action.request_interrupt(InterruptPriority::PlayerCancel));
    // stun replaces hit recovery
    assert!(action.request_interrupt(InterruptPriority::Stun));
    let mode = action.apply_interrupt().unwrap();
    assert_eq!(mode, ActorMode::Stunned);
}

#[test]
fn pending_death_actor_cannot_start_action() {
    let mut actor = Actor::new(EntityId(1));
    actor.lifecycle = arpg_core::Lifecycle::PendingDeath;
    assert!(actor
        .start_action(
            ActorMode::Attack,
            Tick(0),
            Target::None,
            ActionTiming::default()
        )
        .is_none());
}

#[test]
fn damage_kills_and_credits_killer() {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [42u8; 32]);
    inst.add_player(arpg_core::PlayerId(1), arpg_core::WorldPos::new(0, 0));
    inst.add_player(arpg_core::PlayerId(2), arpg_core::WorldPos::new(0, 0));
    let target = EntityId(1);
    let killer = EntityId(2);

    inst.hostility
        .declare(arpg_core::PlayerId(2), arpg_core::PlayerId(1));
    inst.apply_damage(target, killer, 100);
    let actor = inst.actors.get(&target).unwrap();
    assert_eq!(actor.lifecycle, arpg_core::Lifecycle::PendingDeath);
    assert!(
        inst.state
            .players
            .get(&arpg_core::PlayerId(1))
            .unwrap()
            .life
            <= 0
    );

    let result = inst.tick();
    assert!(result
        .events
        .iter()
        .any(|e| matches!(e, arpg_core::GameEvent::EntityKilled { target: t, killer: k } if *t == target && *k == killer)));
    assert_eq!(
        inst.actors.get(&target).unwrap().lifecycle,
        arpg_core::Lifecycle::Dead
    );
}

#[test]
fn state_store_policies() {
    let mut store = StateStore::new();
    let entity = EntityId(1);
    let source = EntityId(2);
    let mk = |expires: u64| StateInstance {
        state: 3,
        source,
        source_skill: None,
        applied_tick: Tick(1),
        expires_tick: Some(Tick(expires)),
        stack_key: (3, source.0),
        magnitude_bp: 0,
    };
    // Refresh extends the existing instance
    store.apply(entity, mk(10), StackPolicy::Refresh);
    store.apply(entity, mk(20), StackPolicy::Refresh);
    assert_eq!(store.len(), 1);
    assert_eq!(
        store.get(entity, 3, source).unwrap().expires_tick,
        Some(Tick(20))
    );

    // Strongest keeps the longer expiry
    store.apply(entity, mk(5), StackPolicy::Strongest);
    assert_eq!(
        store.get(entity, 3, source).unwrap().expires_tick,
        Some(Tick(20))
    );

    // expiry removes expired entries
    let expired = store.expire(Tick(30));
    assert_eq!(expired.len(), 1);
    assert!(store.is_empty());
}

#[test]
fn actor_hash_in_state_determinism() {
    let run = || {
        let data = Arc::new(arpg_data::GameData::default());
        let rules = Arc::new(arpg_rules::GameRules::default());
        let mut inst = GameInstance::new(data, rules, [7u8; 32]);
        inst.add_player(arpg_core::PlayerId(1), arpg_core::WorldPos::new(0, 0));
        let actor = inst.actors.get_mut(&EntityId(1)).unwrap();
        actor
            .start_action(
                ActorMode::Attack,
                Tick(0),
                Target::None,
                ActionTiming {
                    windup_ticks: 2,
                    impact_tick: 2,
                    recovery_ticks: 2,
                },
            )
            .unwrap();
        (0..10).map(|_| inst.tick().state_hash).collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}
