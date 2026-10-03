use arpg_ai::{AiParams, HfsmBrain};
use arpg_core::{EntityId, PlayerId, WorldPos};
use arpg_sim::GameInstance;
use std::sync::Arc;

fn setup() -> GameInstance {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    GameInstance::new(data, rules, [42u8; 32])
}

fn interval() -> AiParams {
    AiParams {
        think_interval_ticks: 1,
        ..AiParams::default()
    }
}

#[test]
fn spawn_monster_notifies_brain() {
    let mut inst = setup();
    let brain = HfsmBrain::new(interval());
    inst.set_ai_brain(Box::new(brain));
    let m = inst.spawn_monster(WorldPos::new(2048, 0));
    assert!(inst.monsters.contains_key(&m));
    assert!(inst.actors.contains_key(&m));
}

#[test]
fn monster_chases_and_attacks_player() {
    let mut inst = setup();
    inst.set_ai_brain(Box::new(HfsmBrain::new(interval())));
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    let monster = inst.spawn_monster(WorldPos::new(1024, 0));
    let life_before = inst.state.players.get(&PlayerId(1)).unwrap().life;

    for _ in 0..40 {
        inst.tick();
    }
    // monster must have closed in and landed hits
    let m = inst.monsters.get(&monster).unwrap();
    let p = inst.state.players.get(&PlayerId(1)).unwrap();
    assert!(
        m.pos.dist2(p.pos) <= (4 * 256) * (4 * 256),
        "monster should be near the player, at {:?} vs {:?}",
        m.pos,
        p.pos
    );
    assert!(
        p.life < life_before,
        "player must have taken damage: {} vs {life_before}",
        p.life
    );
}

#[test]
fn brain_without_brain_is_noop() {
    let mut inst = setup();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    let m = inst.spawn_monster(WorldPos::new(1024, 0));
    let before = inst.monsters.get(&m).unwrap().pos;
    for _ in 0..10 {
        inst.tick();
    }
    assert_eq!(
        inst.monsters.get(&m).unwrap().pos,
        before,
        "no brain: no movement"
    );
}

#[test]
fn monster_respects_blocked_terrain() {
    let mut inst = setup();
    inst.set_ai_brain(Box::new(HfsmBrain::new(interval())));
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    // wall between monster and player
    let mut collision = arpg_core::CollisionMap::new(64, 64);
    for y in 0..64 {
        collision.set_walkable(4, y, false);
    }
    inst.level = Some(arpg_world::LevelInstance {
        id: arpg_core::LevelInstanceId(1),
        definition: arpg_core::LevelDefId(1),
        collision,
        rooms: Vec::new(),
        objects: Vec::new(),
    });
    let monster = inst.spawn_monster(WorldPos::new(8 * 256, 0));
    for _ in 0..30 {
        inst.tick();
    }
    let m = inst.monsters.get(&monster).unwrap();
    assert_ne!(m.pos.tile().0, 4, "monster never walks into the wall");
}

#[test]
fn monster_flow_is_deterministic() {
    let run = || {
        let mut inst = setup();
        inst.set_ai_brain(Box::new(HfsmBrain::new(AiParams::default())));
        inst.add_player(PlayerId(1), WorldPos::new(0, 0));
        inst.add_player(PlayerId(2), WorldPos::new(512, 512));
        inst.spawn_monster(WorldPos::new(1024, 0));
        inst.spawn_monster(WorldPos::new(0, 2048));
        (0..25).map(|_| inst.tick().state_hash).collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}

#[test]
fn monster_ids_are_disjoint_from_players() {
    let mut inst = setup();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    let m = inst.spawn_monster(WorldPos::new(0, 0));
    assert_ne!(m, EntityId(1));
    assert!(m.0 >= 2 << 40);
}
