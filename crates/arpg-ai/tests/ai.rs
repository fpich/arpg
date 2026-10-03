use arpg_ai::{
    AiAgent, AiIntent, AiParams, AiState, ChampionModifier, ChampionProfile, MonsterPack,
    PerceivedEntity, PerceptionView,
};
use arpg_core::{EntityId, Tick, WorldPos};

fn enemy(id: u64, x: i32, y: i32) -> PerceivedEntity {
    PerceivedEntity {
        entity: EntityId(id),
        pos: WorldPos::new(x, y),
    }
}

fn params() -> AiParams {
    AiParams::default()
}

#[test]
fn idle_becomes_chase_when_enemy_in_aggro_radius() {
    let mut agent = AiAgent::new(EntityId(10), WorldPos::new(0, 0));
    let view = PerceptionView {
        enemies: vec![enemy(1, 256, 0)],
    };
    let intent = agent.think(Tick(10), &view, &params());
    assert_eq!(agent.state, AiState::Chase);
    assert!(matches!(intent, AiIntent::MoveTo(_)));
}

#[test]
fn chase_becomes_attack_in_range() {
    let mut agent = AiAgent::new(EntityId(10), WorldPos::new(0, 0));
    let far = PerceptionView {
        enemies: vec![enemy(1, 2048, 0)],
    };
    agent.think(Tick(0), &far, &params());
    assert_eq!(agent.state, AiState::Chase);
    let near = PerceptionView {
        enemies: vec![enemy(1, 256, 0)],
    };
    let intent = agent.think(Tick(5), &near, &params());
    assert_eq!(agent.state, AiState::Attack);
    assert!(matches!(intent, AiIntent::AttackTarget(t) if t == EntityId(1)));
}

#[test]
fn leash_pulls_home_when_target_too_far() {
    let mut agent = AiAgent::new(EntityId(10), WorldPos::new(0, 0));
    // out of aggro radius: stays idle
    let far = PerceptionView {
        enemies: vec![enemy(1, 20_000, 0)],
    };
    agent.think(Tick(0), &far, &params());
    assert_eq!(agent.state, AiState::Idle);
    // close target: idle -> chase
    let close = PerceptionView {
        enemies: vec![enemy(1, 256, 0)],
    };
    let intent = agent.think(Tick(5), &close, &params());
    assert_eq!(agent.state, AiState::Chase);
    assert!(matches!(intent, AiIntent::MoveTo(_)));
    // target runs beyond the leash radius: chase -> leash
    let beyond_leash = PerceptionView {
        enemies: vec![enemy(1, 30_000, 0)],
    };
    let intent = agent.think(Tick(10), &beyond_leash, &params());
    assert_eq!(agent.state, AiState::Leash);
    assert!(matches!(intent, AiIntent::MoveTo(p) if p == WorldPos::new(0, 0)));
}

#[test]
fn think_schedule_is_staggered() {
    let interval = 5u32;
    let agent = AiAgent::new(EntityId(7), WorldPos::new(0, 0));
    // (entity_id % interval): thinks at ticks 7, 12, 17...
    assert!(agent.should_think(Tick(7), interval));
    assert!(!agent.should_think(Tick(8), interval));
    assert!(agent.should_think(Tick(12), interval));
    let other = AiAgent::new(EntityId(9), WorldPos::new(0, 0));
    assert!(other.should_think(Tick(9), interval));
    assert!(!other.should_think(Tick(7), interval));
}

#[test]
fn target_selection_is_nearest_then_entity_id() {
    let mut agent = AiAgent::new(EntityId(10), WorldPos::new(0, 0));
    let view = PerceptionView {
        enemies: vec![enemy(5, 1024, 0), enemy(3, 512, 0), enemy(2, 512, 0)],
    };
    agent.think(Tick(0), &view, &params());
    assert_eq!(
        agent.blackboard.current_target,
        Some(EntityId(2)),
        "nearest, lowest EntityId breaks the tie"
    );
}

#[test]
fn empty_perception_yields_no_intent() {
    let mut agent = AiAgent::new(EntityId(10), WorldPos::new(0, 0));
    let view = PerceptionView::default();
    let intent = agent.think(Tick(0), &view, &params());
    assert_eq!(intent, AiIntent::None);
    assert_eq!(agent.state, AiState::Idle);
}

#[test]
fn pack_aggro_linkage_spreads_to_all_members() {
    let pack = MonsterPack::new(EntityId(1), vec![EntityId(2), EntityId(3), EntityId(4)]);
    let aggroed = pack.aggro_members(EntityId(3));
    assert_eq!(
        aggroed,
        vec![EntityId(1), EntityId(2), EntityId(3), EntityId(4)]
    );
    assert!(pack.contains(EntityId(4)));
    assert!(!pack.contains(EntityId(99)));
}

#[test]
fn pack_without_linkage_only_aggros_trigger() {
    let mut pack = MonsterPack::new(EntityId(1), vec![EntityId(2)]);
    pack.aggro_linkage = false;
    let aggroed = pack.aggro_members(EntityId(2));
    assert_eq!(aggroed, vec![EntityId(2)]);
}

#[test]
fn champion_modifiers_compose() {
    let profile = ChampionProfile {
        modifiers: vec![ChampionModifier::ExtraFast, ChampionModifier::ExtraStrong],
    };
    assert!(profile.is_champion());
    assert_eq!(profile.speed_bonus_bp(), 5_000);
    assert_eq!(profile.damage_bonus_bp(), 7_500);
    assert_eq!(profile.resist_bonus_bp(), 0);
    let plain = ChampionProfile { modifiers: vec![] };
    assert!(!plain.is_champion());
}
