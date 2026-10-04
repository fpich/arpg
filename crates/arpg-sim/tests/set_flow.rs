//! Set bonuses end-to-end (SPEC.md section 89): equipping set pieces
//! through the pickup path recomputes partial and full bonuses.

use arpg_core::{ItemDefId, ItemId, PlayerId, WorldPos};
use arpg_sim::item::EquipmentSlot;
use arpg_sim::set::{PieceCondition, SetBonusTier, SetDefinition};
use arpg_sim::{GameInstance, ItemInstance, ItemLocation, ItemQuality};
use std::collections::BTreeMap;
use std::sync::Arc;

fn game() -> GameInstance {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [107u8; 32]);
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst
}

fn equip_item(inst: &mut GameInstance, def: ItemDefId, slot: EquipmentSlot) {
    let id = ItemId(inst.inventory.highest_item_id() + 1);
    let item = ItemInstance {
        id,
        definition: def,
        quality: ItemQuality::Normal,
        item_level: 1,
        generation_seed: [0; 32],
        affixes: smallvec::SmallVec::new(),
        sockets: smallvec::SmallVec::new(),
        durability: None,
        flags: 0,
        charges: None,
        hands: Default::default(),
    };
    inst.inventory
        .spawn_ground(item, arpg_core::LevelInstanceId(0), WorldPos::new(0, 0));
    inst.pick_up_item(
        PlayerId(1),
        id,
        ItemLocation::Ground(arpg_core::LevelInstanceId(0), WorldPos::new(0, 0)),
        ItemLocation::Equipment(PlayerId(1), slot),
    )
    .expect("equip");
}

#[test]
fn equipping_set_pieces_builds_up_bonuses() {
    let mut inst = game();
    let mut pieces = BTreeMap::new();
    pieces.insert(ItemDefId(1), EquipmentSlot::Head);
    pieces.insert(ItemDefId(2), EquipmentSlot::Body);
    inst.sets.definitions.insert(
        1,
        SetDefinition {
            id: 1,
            name: "Test Set".into(),
            pieces,
            partial: vec![SetBonusTier {
                pieces: 1,
                magnitude_bp: 100,
                condition: None,
            }],
            full: SetBonusTier {
                pieces: 2,
                magnitude_bp: 1000,
                condition: None,
            },
        },
    );
    equip_item(&mut inst, ItemDefId(1), EquipmentSlot::Head);
    // one piece: partial only
    assert_eq!(inst.sets.bonus_of(PlayerId(1), 1), 100);
    equip_item(&mut inst, ItemDefId(2), EquipmentSlot::Body);
    // both pieces: partial + full
    assert_eq!(inst.sets.bonus_of(PlayerId(1), 1), 1100);
}

#[test]
fn piece_conditions_gate_tiers() {
    let mut inst = game();
    let mut pieces = BTreeMap::new();
    pieces.insert(ItemDefId(1), EquipmentSlot::Head);
    pieces.insert(ItemDefId(2), EquipmentSlot::Boots);
    inst.sets.definitions.insert(
        2,
        SetDefinition {
            id: 2,
            name: "Gated".into(),
            pieces,
            partial: vec![SetBonusTier {
                pieces: 1,
                magnitude_bp: 250,
                condition: Some(PieceCondition {
                    required_defs: vec![ItemDefId(2)],
                }),
            }],
            full: SetBonusTier {
                pieces: 2,
                magnitude_bp: 5000,
                condition: None,
            },
        },
    );
    // head only: the gated tier does not activate
    equip_item(&mut inst, ItemDefId(1), EquipmentSlot::Head);
    assert_eq!(inst.sets.bonus_of(PlayerId(1), 2), 0);
    // boots equipped: gated tier activates
    equip_item(&mut inst, ItemDefId(2), EquipmentSlot::Boots);
    assert_eq!(inst.sets.bonus_of(PlayerId(1), 2), 5250);
}

#[test]
fn equipped_affixes_fold_into_player_stats() {
    let mut inst = game();
    let id = ItemId(inst.inventory.highest_item_id() + 1);
    let item = ItemInstance {
        id,
        definition: ItemDefId(2001),
        quality: ItemQuality::Normal,
        item_level: 1,
        generation_seed: [0; 32],
        affixes: smallvec::smallvec![1, 2],
        sockets: smallvec::SmallVec::new(),
        durability: None,
        flags: 0,
        charges: None,
        hands: Default::default(),
    };
    inst.inventory
        .spawn_ground(item, arpg_core::LevelInstanceId(0), WorldPos::new(0, 0));
    let before = inst.player_stat(PlayerId(1), arpg_sim::stat::STAT_STRENGTH);
    inst.pick_up_item(
        PlayerId(1),
        id,
        ItemLocation::Ground(arpg_core::LevelInstanceId(0), WorldPos::new(0, 0)),
        ItemLocation::Equipment(PlayerId(1), EquipmentSlot::MainHand),
    )
    .expect("equip");
    let after = inst.player_stat(PlayerId(1), arpg_sim::stat::STAT_STRENGTH);
    // each affix contributes a flat +10; affixes 1 and 2 map to stats
    // 2 and 3, so strength stays and dexterity gains +10
    assert_eq!(after, before, "affix 1 and 2 do not touch strength");
    let dex = inst.player_stat(PlayerId(1), arpg_sim::stat::STAT_DEXTERITY);
    assert!(dex >= 10, "an affix must raise dexterity, got {}", dex);
    // re-equipping the same item must not double the modifiers:
    // recompute_equipment_stats clears Equipment sources first
    inst.recompute_equipment_stats(PlayerId(1));
    let rerun = inst.player_stat(PlayerId(1), arpg_sim::stat::STAT_DEXTERITY);
    assert_eq!(rerun, dex, "recompute must be idempotent");
}
