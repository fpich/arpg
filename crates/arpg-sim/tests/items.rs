use arpg_core::{ItemDefId, ItemId, LevelInstanceId, PlayerId, WorldPos};
use arpg_sim::{
    EquipmentSlot, GridPos, InventorySystem, ItemError, ItemInstance, ItemLocation, ItemQuality,
    LootRoller, StashPos, TreasureClass, TreasureKind, WeightedTreasureEntry,
};

fn simple_tc() -> TreasureClass {
    TreasureClass {
        picks: 1,
        no_drop_weight: 0,
        entries: vec![WeightedTreasureEntry {
            weight: 10,
            kind: TreasureKind::Item(ItemDefId(7)),
        }],
    }
}

fn nested_tc() -> TreasureClass {
    TreasureClass {
        picks: 1,
        no_drop_weight: 0,
        entries: vec![WeightedTreasureEntry {
            weight: 5,
            kind: TreasureKind::TreasureClass(simple_tc()),
        }],
    }
}

#[test]
fn loot_is_deterministic_and_reproducible() {
    let seed = [9u8; 32];
    let roll = || {
        let mut roller = LootRoller::new();
        let item = roller
            .roll(&simple_tc(), seed, 30)
            .expect("valid tc")
            .expect("always drops");
        (item.id, item.definition, item.quality, item.generation_seed)
    };
    assert_eq!(roll(), roll());
    let (_, def, _, gen) = roll();
    assert_eq!(def, ItemDefId(7));
    assert_eq!(gen, seed, "generation seed kept for reproducibility");
}

#[test]
fn loot_resolves_nested_treasure_classes() {
    let mut roller = LootRoller::new();
    let item = roller
        .roll(&nested_tc(), [1u8; 32], 25)
        .unwrap()
        .expect("nested drop");
    assert_eq!(item.definition, ItemDefId(7));
    assert_eq!(item.item_level, 25);
}

#[test]
fn no_drop_weight_yields_none() {
    let tc = TreasureClass {
        picks: 1,
        no_drop_weight: 10,
        entries: vec![],
    };
    let mut roller = LootRoller::new();
    let result = roller.roll(&tc, [1u8; 32], 10).unwrap();
    assert!(result.is_none());
}

#[test]
fn invalid_treasure_classes_are_rejected() {
    let zero_weight = TreasureClass {
        picks: 1,
        no_drop_weight: 0,
        entries: vec![WeightedTreasureEntry {
            weight: 0,
            kind: TreasureKind::Item(ItemDefId(1)),
        }],
    };
    assert!(matches!(
        zero_weight.validate(),
        Err(ItemError::InvalidTreasureClass("zero weight"))
    ));

    // unbounded-looking deep nesting must be rejected (section 72)
    let mut deep = simple_tc();
    for _ in 0..20 {
        deep = TreasureClass {
            picks: 1,
            no_drop_weight: 0,
            entries: vec![WeightedTreasureEntry {
                weight: 1,
                kind: TreasureKind::TreasureClass(deep),
            }],
        };
    }
    let mut roller = LootRoller::new();
    assert!(roller.roll(&deep, [1u8; 32], 10).is_err());
}

#[test]
fn quality_and_affix_counts_follow_constraints() {
    // run many rolls; affix counts must respect Magic 1-2, Rare 3-6
    let tc = TreasureClass {
        picks: 1,
        no_drop_weight: 0,
        entries: vec![WeightedTreasureEntry {
            weight: 3,
            kind: TreasureKind::Item(ItemDefId(1)),
        }],
    };
    let mut roller = LootRoller::new();
    for i in 0..64u64 {
        let mut seed = [0u8; 32];
        seed[..8].copy_from_slice(&i.to_le_bytes());
        let item = roller.roll(&tc, seed, 20).unwrap().expect("always drops");
        match item.quality {
            ItemQuality::Magic => {
                assert!((1..=2).contains(&item.affixes.len()), "magic: 1-2 affixes")
            }
            ItemQuality::Rare => assert!(
                (3..=6).contains(&item.affixes.len()),
                "rare: up to 3 prefixes + 3 suffixes"
            ),
            ItemQuality::Unique | ItemQuality::Set => {
                assert!(item.affixes.is_empty(), "fixed quality")
            }
            _ => assert!(item.affixes.is_empty()),
        }
    }
}

#[test]
fn item_ids_are_sequential_and_unique() {
    let tc = simple_tc();
    let mut roller = LootRoller::new();
    let a = roller.roll(&tc, [1u8; 32], 10).unwrap().unwrap();
    let b = roller.roll(&tc, [2u8; 32], 10).unwrap().unwrap();
    assert_ne!(a.id, b.id);
    assert_eq!(a.id.0 + 1, b.id.0);
}

#[test]
fn inventory_move_is_transactional() {
    let mut inv = InventorySystem::new();
    let item = ItemInstance {
        id: ItemId(1),
        definition: ItemDefId(1),
        quality: ItemQuality::Normal,
        item_level: 10,
        generation_seed: [0u8; 32],
        affixes: Default::default(),
        sockets: Default::default(),
        durability: None,
        flags: 0,
        charges: None,
        hands: Default::default(),
        requirements: Default::default(),
    };
    let level = LevelInstanceId(1);
    inv.spawn_ground(item, level, WorldPos::new(0, 0));
    let ground = ItemLocation::Ground(level, WorldPos::new(0, 0));
    let to = ItemLocation::PlayerInventory(PlayerId(1), GridPos { x: 0, y: 0 });
    inv.pick_up(PlayerId(1), ItemId(1), ground, to).unwrap();
    assert_eq!(inv.location(ItemId(1)), Some(to));

    // moving from a stale source must fail without mutation
    let stale = inv
        .pick_up(
            PlayerId(2),
            ItemId(1),
            ground,
            ItemLocation::PlayerInventory(PlayerId(2), GridPos { x: 0, y: 0 }),
        )
        .unwrap_err();
    assert!(matches!(stale, ItemError::ItemUnavailable));
    assert_eq!(inv.location(ItemId(1)), Some(to), "no mutation on failure");
}

#[test]
fn simultaneous_pickup_first_wins() {
    let mut inv = InventorySystem::new();
    let item = ItemInstance {
        id: ItemId(1),
        definition: ItemDefId(1),
        quality: ItemQuality::Normal,
        item_level: 10,
        generation_seed: [0u8; 32],
        affixes: Default::default(),
        sockets: Default::default(),
        durability: None,
        flags: 0,
        charges: None,
        hands: Default::default(),
        requirements: Default::default(),
    };
    let level = LevelInstanceId(1);
    inv.spawn_ground(item, level, WorldPos::new(0, 0));
    let ground = ItemLocation::Ground(level, WorldPos::new(0, 0));

    let first = inv.pick_up(
        PlayerId(1),
        ItemId(1),
        ground,
        ItemLocation::PlayerInventory(PlayerId(1), GridPos { x: 0, y: 0 }),
    );
    assert!(first.is_ok());
    let second = inv.pick_up(
        PlayerId(2),
        ItemId(1),
        ground,
        ItemLocation::PlayerInventory(PlayerId(2), GridPos { x: 0, y: 0 }),
    );
    assert!(matches!(second, Err(ItemError::ItemUnavailable)));
}

#[test]
fn slot_conflicts_are_rejected() {
    let mut inv = InventorySystem::new();
    for id in [ItemId(1), ItemId(2)] {
        let item = ItemInstance {
            id,
            definition: ItemDefId(1),
            quality: ItemQuality::Normal,
            item_level: 10,
            generation_seed: [0u8; 32],
            affixes: Default::default(),
            sockets: Default::default(),
            durability: None,
            flags: 0,
            charges: None,
            hands: Default::default(),
            requirements: Default::default(),
        };
        inv.spawn_ground(item, LevelInstanceId(1), WorldPos::new(0, 0));
    }
    let level = LevelInstanceId(1);
    let ground = ItemLocation::Ground(level, WorldPos::new(0, 0));
    let slot = ItemLocation::Equipment(PlayerId(1), EquipmentSlot::MainHand);
    inv.pick_up(PlayerId(1), ItemId(1), ground, slot).unwrap();
    let conflict = inv
        .pick_up(PlayerId(1), ItemId(2), ground, slot)
        .unwrap_err();
    assert!(matches!(conflict, ItemError::SlotOccupied));
    // second item still on the ground
    assert_eq!(
        inv.location(ItemId(2)),
        Some(ItemLocation::Ground(level, WorldPos::new(0, 0)))
    );
}

#[test]
fn stash_positions_are_distinct() {
    let mut inv = InventorySystem::new();
    let level = LevelInstanceId(1);
    for i in 0..3u8 {
        let item = ItemInstance {
            id: ItemId(i as u128 + 1),
            definition: ItemDefId(1),
            quality: ItemQuality::Normal,
            item_level: 1,
            generation_seed: [0u8; 32],
            affixes: Default::default(),
            sockets: Default::default(),
            durability: None,
            flags: 0,
            charges: None,
            hands: Default::default(),
            requirements: Default::default(),
        };
        inv.spawn_ground(item, level, WorldPos::new(i as i32 * 256, 0));
        let ground = ItemLocation::Ground(level, WorldPos::new(i as i32 * 256, 0));
        inv.pick_up(
            PlayerId(1),
            ItemId(i as u128 + 1),
            ground,
            ItemLocation::Stash(
                PlayerId(1),
                StashPos {
                    page: i,
                    x: 0,
                    y: 0,
                },
            ),
        )
        .unwrap();
    }
    assert_eq!(inv.len(), 3);
}

#[test]
fn weapon_swap_exchanges_loadouts() {
    let mut inst = arpg_sim::GameInstance::new(
        std::sync::Arc::new(arpg_data::GameData::default()),
        std::sync::Arc::new(arpg_rules::GameRules::default()),
        [42u8; 32],
    );
    inst.add_player(arpg_core::PlayerId(1), WorldPos::new(0, 0));
    let mk = |id: u128, def: u32| arpg_sim::ItemInstance {
        id: arpg_core::ItemId(id),
        definition: arpg_core::ItemDefId(def),
        quality: arpg_sim::item::ItemQuality::Normal,
        item_level: 1,
        generation_seed: [0; 32],
        affixes: smallvec::SmallVec::new(),
        sockets: smallvec::SmallVec::new(),
        durability: None,
        flags: 0,
        charges: None,
        hands: Default::default(),
        requirements: Default::default(),
    };
    use arpg_sim::item::EquipmentSlot as Slot;
    let primary = mk(1, 2001);
    let secondary = mk(2, 2002);
    inst.inventory
        .spawn_ground(primary, arpg_core::LevelInstanceId(0), WorldPos::new(0, 0));
    inst.inventory.spawn_ground(
        secondary,
        arpg_core::LevelInstanceId(0),
        WorldPos::new(0, 0),
    );
    inst.pick_up_item(
        arpg_core::PlayerId(1),
        arpg_core::ItemId(1),
        ItemLocation::Ground(arpg_core::LevelInstanceId(0), WorldPos::new(0, 0)),
        ItemLocation::Equipment(arpg_core::PlayerId(1), Slot::MainHand),
    )
    .expect("equip primary");
    inst.pick_up_item(
        arpg_core::PlayerId(1),
        arpg_core::ItemId(2),
        ItemLocation::Ground(arpg_core::LevelInstanceId(0), WorldPos::new(0, 0)),
        ItemLocation::Equipment(arpg_core::PlayerId(1), Slot::PrimarySet),
    )
    .expect("stash secondary");
    // the swap exchanges the active and secondary weapons
    inst.swap_weapons(arpg_core::PlayerId(1));
    assert_eq!(
        inst.inventory.location(arpg_core::ItemId(1)),
        Some(ItemLocation::Equipment(
            arpg_core::PlayerId(1),
            Slot::PrimarySet
        )),
        "the first weapon moved to the secondary set"
    );
    assert_eq!(
        inst.inventory.location(arpg_core::ItemId(2)),
        Some(ItemLocation::Equipment(
            arpg_core::PlayerId(1),
            Slot::MainHand
        )),
        "the second weapon became active"
    );
    // swapping back restores the original loadout
    inst.swap_weapons(arpg_core::PlayerId(1));
    assert_eq!(
        inst.inventory.location(arpg_core::ItemId(1)),
        Some(ItemLocation::Equipment(
            arpg_core::PlayerId(1),
            Slot::MainHand
        ))
    );
    assert_eq!(
        inst.inventory.location(arpg_core::ItemId(2)),
        Some(ItemLocation::Equipment(
            arpg_core::PlayerId(1),
            Slot::PrimarySet
        ))
    );
}

#[test]
fn two_handed_weapon_reserves_both_hand_slots() {
    let mut inv = InventorySystem::new();
    let mk = |id: u128, hands: arpg_sim::item::ItemHands| ItemInstance {
        id: ItemId(id),
        definition: ItemDefId(30),
        quality: ItemQuality::Normal,
        item_level: 1,
        generation_seed: [0; 32],
        affixes: smallvec::SmallVec::new(),
        sockets: smallvec::SmallVec::new(),
        durability: None,
        flags: 0,
        charges: None,
        hands,
        requirements: Default::default(),
    };
    let ground = || ItemLocation::Ground(LevelInstanceId(0), WorldPos::new(0, 0));
    inv.spawn_ground(
        mk(1, arpg_sim::item::ItemHands::TwoHanded),
        LevelInstanceId(0),
        WorldPos::new(0, 0),
    );
    inv.pick_up(
        PlayerId(1),
        ItemId(1),
        ground(),
        ItemLocation::Equipment(PlayerId(1), EquipmentSlot::MainHand),
    )
    .expect("equip two-handed in main hand");
    assert_eq!(
        inv.hand_slot_owner(PlayerId(1), EquipmentSlot::OffHand),
        Some(ItemId(1)),
        "the off-hand slot is reserved by the two-handed weapon"
    );
    // an off-hand item cannot join while the two-hander holds both slots
    inv.spawn_ground(
        mk(2, arpg_sim::item::ItemHands::OneHanded),
        LevelInstanceId(0),
        WorldPos::new(0, 0),
    );
    assert!(inv
        .pick_up(
            PlayerId(1),
            ItemId(2),
            ground(),
            ItemLocation::Equipment(PlayerId(1), EquipmentSlot::OffHand)
        )
        .is_err());
    // moving the two-hander to the inventory frees both slots
    inv.move_item(
        ItemId(1),
        ItemLocation::Equipment(PlayerId(1), EquipmentSlot::MainHand),
        ItemLocation::PlayerInventory(PlayerId(1), GridPos { x: 0, y: 0 }),
    )
    .expect("unequip");
    assert_eq!(
        inv.hand_slot_owner(PlayerId(1), EquipmentSlot::OffHand),
        None
    );
    // dual wield: two one-handed weapons coexist
    inv.pick_up(
        PlayerId(1),
        ItemId(2),
        ground(),
        ItemLocation::Equipment(PlayerId(1), EquipmentSlot::MainHand),
    )
    .expect("equip one-handed main hand");
    inv.spawn_ground(
        mk(3, arpg_sim::item::ItemHands::OneHanded),
        LevelInstanceId(0),
        WorldPos::new(0, 0),
    );
    inv.pick_up(
        PlayerId(1),
        ItemId(3),
        ground(),
        ItemLocation::Equipment(PlayerId(1), EquipmentSlot::OffHand),
    )
    .expect("one-handed off-hand is allowed");
}

#[test]
fn equip_requirements_block_unqualified_players() {
    let mut inst = arpg_sim::GameInstance::new(
        std::sync::Arc::new(arpg_data::GameData::default()),
        std::sync::Arc::new(arpg_rules::GameRules::default()),
        [51u8; 32],
    );
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    let req = arpg_sim::item::ItemRequirements {
        level: Some(10),
        strength: Some(30),
        dexterity: None,
    };
    let sword = ItemInstance {
        id: ItemId(9),
        definition: ItemDefId(5),
        quality: ItemQuality::Normal,
        item_level: 5,
        generation_seed: [0; 32],
        affixes: smallvec::SmallVec::new(),
        sockets: smallvec::SmallVec::new(),
        durability: None,
        flags: 0,
        charges: None,
        hands: Default::default(),
        requirements: req,
    };
    inst.inventory
        .spawn_ground(sword, LevelInstanceId(0), WorldPos::new(0, 0));
    let ground = ItemLocation::Ground(LevelInstanceId(0), WorldPos::new(0, 0));
    let equip = ItemLocation::Equipment(PlayerId(1), EquipmentSlot::MainHand);
    // level 1, base strength 0: blocked
    assert_eq!(
        inst.pick_up_item(PlayerId(1), ItemId(9), ground, equip),
        Err(arpg_sim::ItemError::RequirementNotMet)
    );
    // raising the character's level and strength satisfies the check
    inst.state.players.get_mut(&PlayerId(1)).unwrap().level = 12;
    let entity = arpg_core::EntityId(1);
    inst.actors
        .get_mut(&entity)
        .unwrap()
        .stats
        .set_base(arpg_sim::stat::STAT_STRENGTH, 35);
    inst.pick_up_item(PlayerId(1), ItemId(9), ground, equip)
        .expect("qualified player equips");
    assert_eq!(inst.inventory.location(ItemId(9)), Some(equip));
}

#[test]
fn identification_changes_display_state_only() {
    let mut item = ItemInstance {
        id: ItemId(11),
        definition: ItemDefId(3),
        quality: ItemQuality::Magic,
        item_level: 12,
        generation_seed: [9; 32],
        affixes: smallvec::smallvec![7, 8],
        sockets: smallvec::SmallVec::new(),
        durability: None,
        flags: arpg_sim::item::FLAG_UNIDENTIFIED,
        charges: None,
        hands: Default::default(),
        requirements: Default::default(),
    };
    assert!(!item.is_identified(), "starts unidentified");
    let seed = item.generation_seed;
    let affixes = item.affixes.clone();
    item.identify();
    assert!(item.is_identified(), "identified after the call");
    // properties are untouched: they were generated server-side already
    assert_eq!(item.generation_seed, seed);
    assert_eq!(item.affixes, affixes);
    assert_eq!(item.quality, ItemQuality::Magic);
}

#[test]
fn rune_removal_follows_recipe_policy() {
    use arpg_sim::socket::{SocketRemovalPolicy, SocketSystem};
    let mut sys = SocketSystem::new();
    sys.declare_sockets(ItemId(21), 2);
    let rws = arpg_sim::socket::reference_runewords();
    let mut item = ItemInstance {
        id: ItemId(21),
        definition: ItemDefId(3),
        quality: ItemQuality::Normal,
        item_level: 1,
        generation_seed: [0; 32],
        affixes: smallvec::SmallVec::new(),
        sockets: smallvec::SmallVec::new(),
        durability: None,
        flags: 0,
        charges: None,
        hands: Default::default(),
        requirements: Default::default(),
    };
    let _ = &mut item;
    sys.insert_rune(ItemId(21), arpg_sim::socket::RuneId(1), &rws)
        .unwrap();
    // Impossible policy: removal is refused by the caller honoring it
    assert!(!SocketSystem::removal_allowed(
        SocketRemovalPolicy::Impossible
    ));
    // Recoverable: the rune comes back
    let out = sys
        .remove_rune(ItemId(21), 0, SocketRemovalPolicy::Recoverable)
        .unwrap();
    assert_eq!(out, Some(arpg_sim::socket::RuneId(1)));
    sys.insert_rune(ItemId(21), arpg_sim::socket::RuneId(1), &rws)
        .unwrap();
    // Destructive: the rune is destroyed
    let out = sys
        .remove_rune(ItemId(21), 0, SocketRemovalPolicy::Destructive)
        .unwrap();
    assert_eq!(out, None, "destructive removal destroys the rune");
    assert_eq!(sys.filled(ItemId(21))[0], None, "socket is free again");
}
