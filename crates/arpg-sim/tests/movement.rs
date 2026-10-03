use arpg_core::{CollisionMap, LevelDefId, LevelInstanceId, PlayerId, WorldPos};
use arpg_sim::{ClientCommand, CommandEnvelope, GameInstance, MoveIntent, MovementMode};
use arpg_world::LevelInstance;
use std::sync::Arc;

fn setup() -> GameInstance {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    GameInstance::new(data, rules, [42u8; 32])
}

fn move_to(player: PlayerId, seq: u32, target: WorldPos) -> CommandEnvelope {
    CommandEnvelope {
        sequence: seq,
        client_tick: arpg_core::Tick(1),
        player,
        command: ClientCommand::Move(MoveIntent {
            direction: target,
            movement_mode: MovementMode::Walk,
            sequence: seq,
        }),
    }
}

fn level_with_blocking_tile(blocked: (i32, i32)) -> LevelInstance {
    let mut collision = CollisionMap::new(8, 8);
    collision.set_walkable(blocked.0, blocked.1, false);
    LevelInstance {
        id: LevelInstanceId(1),
        definition: LevelDefId(1),
        collision,
        rooms: Vec::new(),
    }
}

#[test]
fn movement_applies_without_level() {
    let mut inst = setup();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.submit_command(move_to(PlayerId(1), 1, WorldPos::new(256, 0)));
    // input delay: command executes after DEFAULT_INPUT_DELAY_TICKS
    for _ in 0..5 {
        inst.tick();
    }
    assert_eq!(
        inst.state.players.get(&PlayerId(1)).unwrap().pos,
        WorldPos::new(256, 0)
    );
}

#[test]
fn movement_blocked_by_terrain() {
    let mut inst = setup();
    inst.level = Some(level_with_blocking_tile((1, 0)));
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.submit_command(move_to(PlayerId(1), 1, WorldPos::new(256, 0)));
    for _ in 0..5 {
        inst.tick();
    }
    assert_eq!(
        inst.state.players.get(&PlayerId(1)).unwrap().pos,
        WorldPos::new(0, 0)
    );
}

#[test]
fn movement_blocked_by_other_entity() {
    let mut inst = setup();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.add_player(PlayerId(2), WorldPos::new(256, 0));
    inst.submit_command(move_to(PlayerId(1), 1, WorldPos::new(256, 0)));
    for _ in 0..5 {
        inst.tick();
    }
    assert_eq!(
        inst.state.players.get(&PlayerId(1)).unwrap().pos,
        WorldPos::new(0, 0)
    );
}

#[test]
fn simultaneous_moves_resolve_in_canonical_entity_order() {
    // Two players target the same free tile; the lower EntityId (player 1)
    // is processed first (SPEC.md section 26) and wins the tile.
    let mut a = setup();
    a.add_player(PlayerId(1), WorldPos::new(0, 0));
    a.add_player(PlayerId(2), WorldPos::new(512, 0));
    a.submit_command(move_to(PlayerId(1), 1, WorldPos::new(256, 0)));
    a.submit_command(move_to(PlayerId(2), 1, WorldPos::new(256, 0)));

    let mut b = setup();
    b.add_player(PlayerId(2), WorldPos::new(512, 0));
    b.add_player(PlayerId(1), WorldPos::new(0, 0));
    b.submit_command(move_to(PlayerId(2), 1, WorldPos::new(256, 0)));
    b.submit_command(move_to(PlayerId(1), 1, WorldPos::new(256, 0)));

    let mut ha = Vec::new();
    let mut hb = Vec::new();
    for _ in 0..5 {
        ha.push(a.tick().state_hash);
        hb.push(b.tick().state_hash);
    }
    assert_eq!(ha, hb);
    assert_eq!(
        a.state.players.get(&PlayerId(1)).unwrap().pos,
        WorldPos::new(256, 0)
    );
    assert_eq!(
        a.state.players.get(&PlayerId(2)).unwrap().pos,
        WorldPos::new(512, 0)
    );
}

#[test]
fn movement_is_deterministic_with_level() {
    let run = || {
        let mut inst = setup();
        inst.level = Some(level_with_blocking_tile((2, 2)));
        inst.add_player(PlayerId(1), WorldPos::new(0, 0));
        inst.submit_command(move_to(PlayerId(1), 1, WorldPos::new(256, 256)));
        (0..5).map(|_| inst.tick().state_hash).collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}
