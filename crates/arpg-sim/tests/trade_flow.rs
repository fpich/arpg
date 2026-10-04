//! Trade on the wire (SPEC.md sections 116-118): commands open, offer,
//! accept and cancel trades between two players; the commit validates
//! atomically against a persistence store.

use arpg_core::{ItemDefId, ItemId, PlayerId, WorldPos};
use arpg_persistence::{CharacterRevision, MemoryStore, TradeId};
use arpg_sim::command::{ClientCommand, CommandEnvelope, TradeIntent};
use arpg_sim::{GameInstance, ItemInstance, ItemQuality};
use std::collections::BTreeMap;
use std::sync::Arc;

fn game() -> GameInstance {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [91u8; 32]);
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.add_player(PlayerId(2), WorldPos::new(256, 0));
    inst
}

fn submit(inst: &mut GameInstance, seq: u32, player: PlayerId, intent: TradeIntent) {
    inst.submit_command(CommandEnvelope {
        sequence: seq,
        client_tick: arpg_core::Tick(0),
        player,
        command: ClientCommand::Trade(intent),
    });
}

fn give_item(inst: &mut GameInstance, def: ItemDefId) -> ItemId {
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
    // move it into the player's inventory so ownership validates
    inst.inventory
        .move_item(
            id,
            arpg_sim::ItemLocation::Ground(arpg_core::LevelInstanceId(0), WorldPos::new(0, 0)),
            arpg_sim::ItemLocation::PlayerInventory(PlayerId(1), arpg_sim::GridPos { x: 0, y: 0 }),
        )
        .expect("move to inventory");
    id
}

fn flush(inst: &mut GameInstance) {
    for _ in 0..(arpg_sim::DEFAULT_INPUT_DELAY_TICKS + 2) {
        inst.tick();
    }
}

#[test]
fn full_trade_round_trip() {
    let mut inst = game();
    inst.economy.gold.insert(
        PlayerId(1),
        arpg_sim::Gold {
            carried: 100,
            stash: 0,
        },
    );
    inst.economy.gold.insert(
        PlayerId(2),
        arpg_sim::Gold {
            carried: 50,
            stash: 0,
        },
    );
    let item = give_item(&mut inst, ItemDefId(1));

    // player 1 opens a trade with player 2
    submit(
        &mut inst,
        1,
        PlayerId(1),
        TradeIntent::Open {
            target: PlayerId(2),
        },
    );
    flush(&mut inst);
    let trade_id = inst
        .trades
        .get(TradeId(1))
        .map(|_| 1u64)
        .expect("trade opened");

    // offers: p1 gives the item, p2 gives 30 gold
    submit(
        &mut inst,
        2,
        PlayerId(1),
        TradeIntent::SetOffer {
            trade: trade_id,
            items: vec![item],
            gold: 0,
        },
    );
    flush(&mut inst);
    submit(
        &mut inst,
        3,
        PlayerId(2),
        TradeIntent::SetOffer {
            trade: trade_id,
            items: vec![],
            gold: 30,
        },
    );
    flush(&mut inst);

    // both accept
    submit(
        &mut inst,
        4,
        PlayerId(1),
        TradeIntent::Accept { trade: trade_id },
    );
    flush(&mut inst);
    submit(
        &mut inst,
        5,
        PlayerId(2),
        TradeIntent::Accept { trade: trade_id },
    );
    flush(&mut inst);

    // commit through the persistence-driven path
    let mut store = MemoryStore::new();
    let revisions: BTreeMap<PlayerId, CharacterRevision> = BTreeMap::new();
    let outcome = inst.commit_trade(trade_id, &revisions, &mut store);
    assert!(outcome.is_ok(), "commit must succeed: {:?}", outcome.err());
    assert!(outcome.unwrap().is_ok());
    // gold moved
    assert_eq!(inst.economy.gold_of(PlayerId(1)).carried, 130);
    assert_eq!(inst.economy.gold_of(PlayerId(2)).carried, 20);
}

#[test]
fn cancel_trade_works() {
    let mut inst = game();
    submit(
        &mut inst,
        1,
        PlayerId(1),
        TradeIntent::Open {
            target: PlayerId(2),
        },
    );
    flush(&mut inst);
    submit(&mut inst, 2, PlayerId(1), TradeIntent::Cancel { trade: 1 });
    flush(&mut inst);
    assert_eq!(
        inst.trades.get(TradeId(1)).unwrap().state,
        arpg_sim::TradeState::Cancelled
    );
}

#[test]
fn offer_change_resets_acceptances() {
    let mut inst = game();
    submit(
        &mut inst,
        1,
        PlayerId(1),
        TradeIntent::Open {
            target: PlayerId(2),
        },
    );
    flush(&mut inst);
    submit(&mut inst, 2, PlayerId(1), TradeIntent::Accept { trade: 1 });
    flush(&mut inst);
    assert!(inst.trades.get(TradeId(1)).unwrap().accepted_a);
    // p2 changes their offer: acceptances reset (section 116)
    submit(
        &mut inst,
        3,
        PlayerId(2),
        TradeIntent::SetOffer {
            trade: 1,
            items: vec![],
            gold: 10,
        },
    );
    flush(&mut inst);
    let t = inst.trades.get(TradeId(1)).unwrap();
    assert!(!t.accepted_a && !t.accepted_b);
}

#[test]
fn trade_commands_are_deterministic() {
    let run = || {
        let mut inst = game();
        submit(
            &mut inst,
            1,
            PlayerId(1),
            TradeIntent::Open {
                target: PlayerId(2),
            },
        );
        submit(
            &mut inst,
            2,
            PlayerId(2),
            TradeIntent::SetOffer {
                trade: 1,
                items: vec![],
                gold: 5,
            },
        );
        submit(&mut inst, 3, PlayerId(1), TradeIntent::Accept { trade: 1 });
        for _ in 0..(arpg_sim::DEFAULT_INPUT_DELAY_TICKS + 3) {
            inst.tick();
        }
        inst.state.state_hash()
    };
    assert_eq!(run(), run());
}
