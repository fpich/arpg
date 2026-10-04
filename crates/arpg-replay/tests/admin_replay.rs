//! Admin entries in replays (SPEC.md section 169: test games may record
//! admin commands; playback must reproduce identical state hashes).

use arpg_replay::{AdminPayload, Replay, ReplayEntry, ReplayHeader, Replayer, REPLAY_VERSION};
use arpg_sim::GameInstance;
use std::sync::Arc;

fn header() -> ReplayHeader {
    ReplayHeader {
        replay_version: REPLAY_VERSION,
        engine_version: 1,
        datapack_hash: [1; 32],
        ruleset_hash: [2; 32],
        root_seed: [9; 32],
    }
}

fn game() -> GameInstance {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    GameInstance::new(data, rules, [9u8; 32])
}

#[test]
fn admin_entries_roundtrip_through_encode_decode() {
    let mut replay = Replay::new(header());
    replay.record_admin(5, AdminPayload::Spawn { def: 1, x: 3, y: 4 });
    replay.record_admin(6, AdminPayload::GiveItem { player: 1, def: 7 });
    replay.record_admin(7, AdminPayload::DumpRng);

    let decoded = Replay::decode(&replay.encode()).unwrap();
    assert_eq!(decoded.entries.len(), 3);
    assert_eq!(
        decoded.entries[0],
        ReplayEntry::Admin {
            tick: 5,
            payload: AdminPayload::Spawn { def: 1, x: 3, y: 4 }
        }
    );
    assert_eq!(
        decoded.entries[2],
        ReplayEntry::Admin {
            tick: 7,
            payload: AdminPayload::DumpRng
        }
    );
}

#[test]
fn admin_replay_reproduces_identical_hashes() {
    // record a session with an admin command in the middle
    let mut recorder = game();
    recorder.add_player(arpg_core::PlayerId(1), arpg_core::WorldPos::new(0, 0));
    let mut replay = Replay::new(header());
    replay.record_transition(arpg_replay::SessionTransition::Joined(arpg_core::PlayerId(
        1,
    )));
    let mut hashes = Vec::new();
    for t in 0..10 {
        if t == 4 {
            recorder
                .apply_admin_command(&arpg_sim::admin::AdminCommand::Spawn {
                    def: 0,
                    pos: arpg_core::WorldPos::new(2, 2),
                })
                .unwrap();
            replay.record_admin(
                recorder.state.tick.0,
                AdminPayload::Spawn { def: 0, x: 2, y: 2 },
            );
        }
        hashes.push(recorder.tick().state_hash);
    }

    // play it back on a fresh instance: submit each entry at the right
    // tick, then tick in lockstep with the recorded session
    let mut playback = game();
    let mut entry_iter = replay.entries.iter();
    let mut next_entry = entry_iter.next();
    for hash in &hashes {
        while let Some(entry) = next_entry {
            let entry_tick = match entry {
                ReplayEntry::Admin { tick, .. } => Some(*tick),
                ReplayEntry::Transition(_) => None,
                ReplayEntry::Command { execute_tick, .. } => Some(*execute_tick),
            };
            let due = match entry_tick {
                None => true,
                Some(t) => t <= playback.state.tick.0,
            };
            if !due {
                break;
            }
            Replayer::new(&mut playback).submit(entry).unwrap();
            next_entry = entry_iter.next();
        }
        assert_eq!(&playback.tick().state_hash, hash);
    }
}

#[test]
fn replayer_counts_hash_mismatches() {
    // section 191: a diverging tick is counted, not silent
    let mut inst = game();
    inst.add_player(arpg_core::PlayerId(1), arpg_core::WorldPos::new(0, 0));
    let stale = [0u8; 32];
    {
        let mut replayer = Replayer::new(&mut inst);
        assert!(replayer.tick_against(stale).is_err());
        assert_eq!(replayer.hash_mismatches, 1);
    }
}
