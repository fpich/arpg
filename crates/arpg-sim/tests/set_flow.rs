//! Set bonuses end-to-end (SPEC.md section 89): equipping set pieces
//! through the pickup path recomputes partial and full bonuses.

use arpg_core::{ItemDefId, ItemId, PlayerId, WorldPos};
use arpg_sim::item::EquipmentSlot;
use arpg_sim::set::{PieceCondition, SetBonusTier, SetDefinition};
use arpg_sim::{GameInstance, GridPos, ItemInstance, ItemLocation, ItemQuality};
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
