//! Merchant services (SPEC.md sections 95-97): buy, sell, repair and
//! gamble against a player's personal stock, driven by wire commands.

use arpg_core::{ItemDefId, ItemId, PlayerId, WorldPos};
use arpg_sim::command::{ClientCommand, CommandEnvelope, MerchantIntent};
use arpg_sim::economy::{GambleOffer, MerchantEntry};
use arpg_sim::{GameInstance, Gold, ItemInstance, ItemLocation, ItemQuality};
use std::sync::Arc;

fn game() -> GameInstance {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [103u8; 32]);
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst
}

fn submit(inst: &mut GameInstance, seq: u32, intent: MerchantIntent) {
    inst.submit_command(CommandEnvelope {
        sequence: seq,
        client_tick: arpg_core::Tick(0),
        player: PlayerId(1),
        command: ClientCommand::Merchant(intent),
    });
}

fn flush(inst: &mut GameInstance) {
    for _ in 0..(arpg_sim::DEFAULT_INPUT_DELAY_TICKS + 2) {
        inst.tick();
    }
}

fn stock(inst: &mut GameInstance, def: ItemDefId, price: u64, qty: u32) {
    inst.economy.merchant(1).refresh(
        PlayerId(1),
        vec![MerchantEntry {
            def,
            price,
            quantity: qty,
        }],
    );
}

#[test]
fn buy_debits_gold_and_delivers_item() {
    let mut inst = game();
    inst.economy.gold.insert(
        PlayerId(1),
        Gold {
            carried: 500,
            stash: 0,
        },
    );
    stock(&mut inst, ItemDefId(1001), 100, 3);
    submit(
        &mut inst,
        1,
        MerchantIntent::Buy {
            merchant: 1,
            def: ItemDefId(1001),
            price: None,
        },
    );
    flush(&mut inst);
    assert_eq!(inst.economy.gold_of(PlayerId(1)).carried, 400);
    // item spawned at the player's feet, pickup rules apply (section 86)
    assert_eq!(
        inst.inventory
            .items_on_ground(arpg_core::LevelInstanceId(0))
            .len(),
        1
    );
    // stock decremented
    assert_eq!(inst.economy.merchant(1).stock(PlayerId(1))[0].quantity, 2);
}

#[test]
fn buy_rejects_insufficient_gold() {
    let mut inst = game();
    inst.economy.gold.insert(
        PlayerId(1),
        Gold {
            carried: 10,
            stash: 0,
        },
    );
    stock(&mut inst, ItemDefId(1001), 100, 3);
    submit(
        &mut inst,
        1,
        MerchantIntent::Buy {
            merchant: 1,
            def: ItemDefId(1001),
            price: None,
        },
    );
    flush(&mut inst);
    assert_eq!(inst.economy.gold_of(PlayerId(1)).carried, 10);
    assert_eq!(
        inst.inventory
            .items_on_ground(arpg_core::LevelInstanceId(0))
            .len(),
        0
    );
}

#[test]
fn sell_credits_gold_and_removes_item() {
    let mut inst = game();
    // place an owned item in the inventory
    let id = ItemId(inst.inventory.highest_item_id() + 1);
    let item = ItemInstance {
        id,
        definition: ItemDefId(1001),
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
    inst.inventory
        .move_item(
            id,
            ItemLocation::Ground(arpg_core::LevelInstanceId(0), WorldPos::new(0, 0)),
            ItemLocation::PlayerInventory(PlayerId(1), arpg_sim::GridPos { x: 0, y: 0 }),
        )
        .unwrap();
    submit(
        &mut inst,
        1,
        MerchantIntent::Sell {
            merchant: 1,
            item: id,
            base_price: 100,
        },
    );
    flush(&mut inst);
    // sell multiplier 25% in the default merchant: payout 25
    assert_eq!(inst.economy.gold_of(PlayerId(1)).carried, 25);
    assert!(inst.inventory.get(id).is_none());
}

#[test]
fn repair_restores_durability_for_gold() {
    let mut inst = game();
    inst.economy.gold.insert(
        PlayerId(1),
        Gold {
            carried: 1000,
            stash: 0,
        },
    );
    let id = ItemId(inst.inventory.highest_item_id() + 1);
    let item = ItemInstance {
        id,
        definition: ItemDefId(1001),
        quality: ItemQuality::Normal,
        item_level: 1,
        generation_seed: [0; 32],
        affixes: smallvec::SmallVec::new(),
        sockets: smallvec::SmallVec::new(),
        durability: Some(40),
        flags: 0,
        charges: None,
        hands: Default::default(),
    };
    inst.inventory
        .spawn_ground(item, arpg_core::LevelInstanceId(0), WorldPos::new(0, 0));
    inst.inventory
        .move_item(
            id,
            ItemLocation::Ground(arpg_core::LevelInstanceId(0), WorldPos::new(0, 0)),
            ItemLocation::PlayerInventory(PlayerId(1), arpg_sim::GridPos { x: 1, y: 0 }),
        )
        .unwrap();
    submit(
        &mut inst,
        1,
        MerchantIntent::Repair {
            merchant: 1,
            item: Some(id),
        },
    );
    flush(&mut inst);
    assert_eq!(inst.inventory.get(id).unwrap().durability, Some(100));
    // 60 missing points * 10 per point = 600
    let spent = 1000 - inst.economy.gold_of(PlayerId(1)).carried;
    assert_eq!(spent, 600);
}

#[test]
fn gamble_pays_and_delivers_hidden_item() {
    let mut inst = game();
    inst.economy.gold.insert(
        PlayerId(1),
        Gold {
            carried: 1000,
            stash: 0,
        },
    );
    inst.economy.merchant(1).refresh_gamble(
        PlayerId(1),
        vec![GambleOffer {
            appearance: ItemDefId(3),
            price: 200,
            secret_seed: [9; 32],
        }],
    );
    submit(
        &mut inst,
        1,
        MerchantIntent::Gamble {
            merchant: 1,
            offer: 0,
        },
    );
    flush(&mut inst);
    assert_eq!(inst.economy.gold_of(PlayerId(1)).carried, 800);
    assert_eq!(
        inst.inventory
            .items_on_ground(arpg_core::LevelInstanceId(0))
            .len(),
        1
    );
}

#[test]
fn merchant_commands_are_deterministic() {
    let run = || {
        let mut inst = game();
        inst.economy.gold.insert(
            PlayerId(1),
            Gold {
                carried: 500,
                stash: 0,
            },
        );
        stock(&mut inst, ItemDefId(1001), 100, 3);
        submit(
            &mut inst,
            1,
            MerchantIntent::Buy {
                merchant: 1,
                def: ItemDefId(1001),
                price: None,
            },
        );
        for _ in 0..(arpg_sim::DEFAULT_INPUT_DELAY_TICKS + 3) {
            inst.tick();
        }
        inst.state.state_hash()
    };
    assert_eq!(run(), run());
}

#[test]
fn recharge_restores_charges_for_gold() {
    let mut inst = game();
    inst.economy.gold.insert(
        PlayerId(1),
        Gold {
            carried: 200,
            stash: 0,
        },
    );
    let charged = ItemInstance {
        id: ItemId(77),
        definition: ItemDefId(1001),
        quality: ItemQuality::Magic,
        item_level: 10,
        generation_seed: [5; 32],
        affixes: smallvec::SmallVec::new(),
        sockets: smallvec::SmallVec::new(),
        durability: None,
        flags: 0,
        charges: Some(arpg_sim::item::ChargeState {
            skill: arpg_core::SkillId(9),
            current: 1,
            max: 4,
        }),
        hands: Default::default(),
    };
    inst.inventory
        .spawn_ground(charged, arpg_core::LevelInstanceId(0), WorldPos::new(0, 0));
    inst.pick_up_item(
        PlayerId(1),
        ItemId(77),
        ItemLocation::Ground(arpg_core::LevelInstanceId(0), WorldPos::new(0, 0)),
        ItemLocation::PlayerInventory(PlayerId(1), arpg_sim::GridPos { x: 0, y: 0 }),
    )
    .expect("take the charged item");
    submit(
        &mut inst,
        1,
        MerchantIntent::Recharge {
            merchant: 1,
            item: ItemId(77),
        },
    );
    flush(&mut inst);
    let charges = inst.inventory.get(ItemId(77)).unwrap().charges.unwrap();
    assert_eq!(charges.current, 4, "charges restored to maximum");
    // 3 missing points x 10 gold per point
    assert_eq!(inst.economy.gold_of(PlayerId(1)).carried, 170);
    // recharging again costs nothing: already full
    submit(
        &mut inst,
        2,
        MerchantIntent::Recharge {
            merchant: 1,
            item: ItemId(77),
        },
    );
    flush(&mut inst);
    assert_eq!(inst.economy.gold_of(PlayerId(1)).carried, 170);
}
