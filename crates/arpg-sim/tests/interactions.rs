use arpg_core::{
    InteractableKind, ObjectDefId, ObjectId, ObjectInstance, ObjectInstanceState, PlayerId, Tick,
    WorldPos,
};
use arpg_sim::{ClientCommand, CommandEnvelope, GameInstance, InteractIntent};
use arpg_world::LevelInstance;
use std::sync::Arc;

fn setup() -> GameInstance {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    GameInstance::new(data, rules, [42u8; 32])
}

fn interact_cmd(player: PlayerId, seq: u32, target: ObjectId) -> CommandEnvelope {
    CommandEnvelope {
        sequence: seq,
        client_tick: Tick(1),
        player,
        command: ClientCommand::Interact(InteractIntent { target }),
    }
}

fn chest_at(id: u64, pos: WorldPos) -> ObjectInstance {
    ObjectInstance::new(
        ObjectId(id),
        ObjectDefId(1),
        InteractableKind::Chest,
        pos,
        Tick(0),
    )
}

fn shrine_at(id: u64, pos: WorldPos) -> ObjectInstance {
    ObjectInstance::new(
        ObjectId(id),
        ObjectDefId(2),
        InteractableKind::Shrine,
        pos,
        Tick(0),
    )
}

#[test]
fn interact_destroys_chest_when_close() {
    let mut inst = setup();
    let pos = WorldPos::new(0, 0);
    inst.level = Some(LevelInstance {
        id: arpg_core::LevelInstanceId(1),
        definition: arpg_core::LevelDefId(1),
        collision: arpg_core::CollisionMap::new(8, 8),
        rooms: Vec::new(),
        objects: vec![chest_at(1, WorldPos::new(256, 0))],
    });
    inst.add_player(PlayerId(1), pos);
    inst.submit_command(interact_cmd(PlayerId(1), 1, ObjectId(1)));
    for _ in 0..5 {
        inst.tick();
    }
    let level = inst.level.as_ref().unwrap();
    assert_eq!(level.objects[0].state, ObjectInstanceState::Destroyed);
}

#[test]
fn interact_rejected_when_too_far() {
    let mut inst = setup();
    inst.level = Some(LevelInstance {
        id: arpg_core::LevelInstanceId(1),
        definition: arpg_core::LevelDefId(1),
        collision: arpg_core::CollisionMap::new(64, 64),
        rooms: Vec::new(),
        objects: vec![chest_at(1, WorldPos::new(20 * 256, 0))],
    });
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.submit_command(interact_cmd(PlayerId(1), 1, ObjectId(1)));
    for _ in 0..5 {
        inst.tick();
    }
    let level = inst.level.as_ref().unwrap();
    assert_eq!(level.objects[0].state, ObjectInstanceState::Default);
}

#[test]
fn shrine_recharges_after_cooldown() {
    let mut inst = setup();
    inst.level = Some(LevelInstance {
        id: arpg_core::LevelInstanceId(1),
        definition: arpg_core::LevelDefId(1),
        collision: arpg_core::CollisionMap::new(8, 8),
        rooms: Vec::new(),
        objects: vec![shrine_at(1, WorldPos::new(256, 0))],
    });
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.submit_command(interact_cmd(PlayerId(1), 1, ObjectId(1)));
    for _ in 0..5 {
        inst.tick();
    }
    {
        let level = inst.level.as_ref().unwrap();
        assert_eq!(level.objects[0].state, ObjectInstanceState::OnRecharge);
    }
    for _ in 0..300 {
        inst.tick();
    }
    let level = inst.level.as_ref().unwrap();
    assert_eq!(level.objects[0].state, ObjectInstanceState::Default);
}

#[test]
fn interact_during_cooldown_is_ignored() {
    let mut inst = setup();
    inst.level = Some(LevelInstance {
        id: arpg_core::LevelInstanceId(1),
        definition: arpg_core::LevelDefId(1),
        collision: arpg_core::CollisionMap::new(8, 8),
        rooms: Vec::new(),
        objects: vec![shrine_at(1, WorldPos::new(256, 0))],
    });
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.submit_command(interact_cmd(PlayerId(1), 1, ObjectId(1)));
    for _ in 0..5 {
        inst.tick();
    }
    inst.submit_command(interact_cmd(PlayerId(1), 2, ObjectId(1)));
    for _ in 0..5 {
        inst.tick();
    }
    let level = inst.level.as_ref().unwrap();
    assert_eq!(level.objects[0].state, ObjectInstanceState::OnRecharge);
    // cooldown still in the future
    assert!(level.objects[0].cooldown_until.0 > inst.state.tick.0);
}

#[test]
fn interactions_are_deterministic() {
    let run = || {
        let mut inst = setup();
        inst.level = Some(LevelInstance {
            id: arpg_core::LevelInstanceId(1),
            definition: arpg_core::LevelDefId(1),
            collision: arpg_core::CollisionMap::new(8, 8),
            rooms: Vec::new(),
            objects: vec![
                chest_at(1, WorldPos::new(256, 0)),
                shrine_at(2, WorldPos::new(512, 0)),
            ],
        });
        inst.add_player(PlayerId(1), WorldPos::new(0, 0));
        inst.submit_command(interact_cmd(PlayerId(1), 1, ObjectId(1)));
        inst.submit_command(interact_cmd(PlayerId(1), 2, ObjectId(2)));
        (0..10).map(|_| inst.tick().state_hash).collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}

#[test]
fn shrine_interaction_applies_blessing_state() {
    let mut inst = setup();
    inst.level = Some(LevelInstance {
        id: arpg_core::LevelInstanceId(1),
        definition: arpg_core::LevelDefId(1),
        collision: arpg_core::CollisionMap::new(8, 8),
        rooms: Vec::new(),
        objects: vec![shrine_at(1, WorldPos::new(256, 0))],
    });
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.submit_command(interact_cmd(PlayerId(1), 1, ObjectId(1)));
    for _ in 0..5 {
        inst.tick();
    }
    let entity = arpg_core::EntityId(1);
    let states = inst.states.entity_states(entity);
    let blessing = states
        .iter()
        .find(|s| s.state == GameInstance::STATE_SHRINE_BOOST);
    assert!(blessing.is_some(), "the shrine must apply its blessing");
    assert_eq!(blessing.unwrap().magnitude_bp, 5000);
    // the shrine recharges before it can bless again
    let level = inst.level.as_ref().unwrap();
    assert_eq!(level.objects[0].state, ObjectInstanceState::OnRecharge);
}
