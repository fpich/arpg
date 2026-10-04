//! Invariant counters (SPEC.md section 191): a violated invariant must be
//! immediately visible. Each counter is exercised at its detection point.

use arpg_core::{ItemId, PlayerId, WorldPos};
use arpg_metrics::{names, Metrics};
use arpg_persistence::PersistenceStore;
use arpg_sim::GameInstance;
use std::sync::{Arc, Mutex};

fn game_with_metrics() -> (GameInstance, Arc<Mutex<Metrics>>) {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [91u8; 32]);
    let metrics = Arc::new(Mutex::new(Metrics::new()));
    inst.metrics = Some(Arc::clone(&metrics));
    (inst, metrics)
}

#[test]
fn scheduler_overflow_counts_dropped_commands() {
    let (mut inst, metrics) = game_with_metrics();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    // flood beyond MAX_QUEUED_COMMANDS: overflow must be counted, and the
    // queue itself stays bounded (no unbounded allocation, section 170)
    for i in 0..(GameInstance::MAX_QUEUED_COMMANDS + 50) {
        inst.submit_command(arpg_sim::CommandEnvelope {
            sequence: i as u32,
            client_tick: arpg_core::Tick(0),
            player: PlayerId(1),
            command: arpg_sim::ClientCommand::NoOp,
        });
    }
    let m = metrics.lock().unwrap();
    assert_eq!(
        m.counter(names::SCHEDULER_OVERFLOW),
        50,
        "excess commands are dropped and counted"
    );
}

#[test]
fn invalid_pickup_counts_both_counters() {
    let (mut inst, metrics) = game_with_metrics();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    // pickup of an item that does not exist at the claimed location
    let err = inst.pick_up_item(
        PlayerId(1),
        ItemId(999),
        arpg_sim::item::ItemLocation::Ground(arpg_core::LevelInstanceId(0), WorldPos::new(0, 0)),
        arpg_sim::item::ItemLocation::PlayerInventory(
            PlayerId(1),
            arpg_sim::item::GridPos { x: 0, y: 0 },
        ),
    );
    assert!(err.is_err());
    let m = metrics.lock().unwrap();
    assert_eq!(m.counter(names::ITEM_TRANSACTION_FAILURES), 1);
    assert_eq!(m.counter(names::INVALID_ITEM_LOCATION), 1);
}

#[test]
fn resync_requested_counts_divergent_clients() {
    let (mut inst, metrics) = game_with_metrics();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    // a client that never acknowledges revisions: base stays 0 while the
    // entity advances
    for _ in 0..3 {
        let _ = inst.replication_for(PlayerId(1));
        inst.tick();
    }
    let repl = inst.replication_for(PlayerId(1));
    // fresh tracker: first replications carry base 0; whether a resync is
    // demanded depends on delta windows, but the counter must equal the
    // number of resync demands produced
    let m = metrics.lock().unwrap();
    let expected: u64 = if repl.resync_entities.is_empty() {
        0
    } else {
        1
    };
    assert_eq!(m.counter(names::RESYNC_REQUESTED), expected);
}

#[test]
fn persistence_stats_count_conflicts_and_failures() {
    let mut store = arpg_persistence::MemoryStore::new();
    let trade = arpg_persistence::TradeId(1);
    let (a, b) = (PlayerId(1), PlayerId(2));
    // bump revisions through successful commits first
    let ra = store.revision(a);
    store.begin(trade, &[(a, ra)]).unwrap();
    store
        .stage(
            trade,
            arpg_persistence::Mutation::SetGold {
                player: a,
                carried: 1,
                stash: 0,
            },
        )
        .unwrap();
    store.commit(trade).unwrap();
    let ra2 = store.revision(a);
    assert!(ra2.0 > ra.0);
    // now a stale expected revision for b must conflict
    store
        .begin(trade, &[(b, arpg_persistence::CharacterRevision(7))])
        .unwrap_err();
    assert_eq!(store.stats.revision_conflicts, 1);

    let rb = store.revision(b);
    store.begin(trade, &[(a, ra2), (b, rb)]).unwrap();
    store.fail_next_commit();
    store.commit(trade).unwrap_err();
    assert_eq!(store.stats.save_failures, 1);
    assert_eq!(store.stats.save_count, 1, "the first commit succeeded");
}
