use arpg_core::{EntityId, ItemDefId, ItemId, LevelInstanceId, PlayerId, WorldPos};
use arpg_sim::{
    GameInstance, GridPos, ItemLocation, TreasureClass, TreasureKind, WeightedTreasureEntry,
};

fn dropping_tc() -> TreasureClass {
    TreasureClass {
        picks: 1,
        no_drop_weight: 0,
        entries: vec![WeightedTreasureEntry {
            weight: 10,
            kind: TreasureKind::Item(ItemDefId(7)),
        }],
    }
}

fn instance_with_monster() -> (GameInstance, EntityId) {
    let data = std::sync::Arc::new(arpg_data::GameData::default());
    let rules = std::sync::Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [42u8; 32]);
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.add_player(PlayerId(2), WorldPos::new(1, 0));
    let monster = inst.spawn_monster_with_tc(WorldPos::new(3, 0), Some(dropping_tc()));
    (inst, monster)
}

const LEVEL: LevelInstanceId = LevelInstanceId(0);

#[test]
fn killed_monster_drops_item_on_ground_then_player_picks_it_up() {
    let (mut inst, monster) = instance_with_monster();
    let killer = EntityId(PlayerId(1).0 as u64);
    inst.apply_damage(monster, killer, 1000);
    let result = inst.tick();
    assert!(!inst.monsters.contains_key(&monster), "monster removed");
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, arpg_core::GameEvent::ItemDropped(_))),
        "ItemDropped event emitted"
    );
    let ground = inst.inventory.items_on_ground(LEVEL);
    assert_eq!(ground.len(), 1, "drop landed on the ground");
    let (item, pos) = ground[0];
    assert_eq!(pos, WorldPos::new(3, 0));
    assert_eq!(
        inst.inventory.location(item),
        Some(ItemLocation::Ground(LEVEL, WorldPos::new(3, 0)))
    );

    let ground_loc = ItemLocation::Ground(LEVEL, pos);
    let to = ItemLocation::PlayerInventory(PlayerId(1), GridPos { x: 0, y: 0 });
    inst.pick_up_item(PlayerId(1), item, ground_loc, to)
        .unwrap();
    assert!(
        inst.inventory.items_on_ground(LEVEL).is_empty(),
        "no longer on the ground"
    );
    assert_eq!(inst.inventory.location(item), Some(to));

    let err = inst.pick_up_item(
        PlayerId(2),
        item,
        to,
        ItemLocation::PlayerInventory(PlayerId(2), GridPos { x: 0, y: 0 }),
    );
    assert!(err.is_err(), "second pickup rejected");
}

#[test]
fn monster_without_treasure_class_drops_nothing() {
    let (mut inst, _monster) = instance_with_monster();
    let killer = EntityId(PlayerId(1).0 as u64);
    let plain = inst.spawn_monster(WorldPos::new(10, 0));
    inst.apply_damage(plain, killer, 1000);
    inst.tick();
    assert!(inst.inventory.items_on_ground(LEVEL).is_empty());
    assert_eq!(inst.state.expired_items, 0);
}

#[test]
fn loot_flow_is_deterministic() {
    let run = || {
        let (mut inst, monster) = instance_with_monster();
        let killer = EntityId(PlayerId(1).0 as u64);
        inst.apply_damage(monster, killer, 1000);
        inst.tick();
        let ground = inst.inventory.items_on_ground(LEVEL);
        assert_eq!(ground.len(), 1);
        (ground[0].0, inst.inventory.location(ground[0].0))
    };
    assert_eq!(run(), run());
    let (a, _) = instance_with_monster();
    let (b, _) = instance_with_monster();
    assert_eq!(a.state.state_hash(), b.state.state_hash());
}

#[test]
fn ground_items_expire_after_lifetime() {
    let (mut inst, monster) = instance_with_monster();
    let killer = EntityId(PlayerId(1).0 as u64);
    inst.apply_damage(monster, killer, 1000);
    inst.tick();
    let item: ItemId = inst.inventory.items_on_ground(LEVEL)[0].0;
    for _ in 0..GameInstance::DEFAULT_GROUND_LIFETIME_TICKS {
        inst.tick();
    }
    assert!(
        inst.inventory
            .items_on_ground(LEVEL)
            .contains(&(item, WorldPos::new(3, 0))),
        "still fresh"
    );
    inst.tick();
    assert!(
        !inst
            .inventory
            .items_on_ground(LEVEL)
            .iter()
            .any(|(id, _)| *id == item),
        "expired"
    );
    assert_eq!(inst.state.expired_items, 1);
}
