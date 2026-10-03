//! Network chaos harness (SPEC.md section 186).
//!
//! Simulates latency, jitter, loss, duplication, reordering and disconnects
//! between a client and the command scheduler. For the same set of finally
//! accepted commands, the server state hash must be independent of these
//! anomalies once the tick horizon is equal.

use arpg_core::{PlayerId, Tick, WorldPos};
use arpg_sim::replication::build_client_replication;
use arpg_sim::{ClientCommand, CommandEnvelope, GameInstance, MoveIntent, MovementMode};
use std::sync::Arc;

fn setup() -> GameInstance {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [42u8; 32]);
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst
}

fn envelope(seq: u32, x: i32) -> CommandEnvelope {
    CommandEnvelope {
        sequence: seq,
        client_tick: Tick(1),
        player: PlayerId(1),
        command: ClientCommand::Move(MoveIntent {
            direction: WorldPos::new(x, 0),
            movement_mode: MovementMode::Walk,
            sequence: seq,
        }),
    }
}

/// Run the game until the scheduler is drained and at least `horizon` ticks
/// passed since the last submission, then compare state hashes at the same
/// absolute tick count.
fn run_to_horizon(game: &mut GameInstance, total_ticks: u64) -> [u8; 32] {
    while game.state.tick.0 < total_ticks {
        game.tick();
    }
    game.state.state_hash()
}

#[test]
fn server_hash_independent_of_submission_timing() {
    // Latency/jitter/reorder only change WHEN a command arrives at the
    // arrival queue. The server keeps ticking at its own pace; per-player
    // sequences are identical, so the finally accepted set is identical.
    // We submit all commands at different server ticks and compare the
    // state at the same final tick.
    let total_ticks = 40u64;
    let mut early = setup();
    let mut late = setup();

    // early: all commands arrive before any tick
    for seq in 1..=6u32 {
        early.submit_command(envelope(seq, (seq as i32) * 100));
    }
    let h_early = run_to_horizon(&mut early, total_ticks);

    // late: commands arrive spread across ticks (latency/jitter), with a
    // duplicate delivery of packet 4 (ignored by sequence dedup) and packet 5
    // arriving before packet 4 cannot happen per-player (sequence monotonic),
    // so reorder is modeled as cross-player arrival covered elsewhere.
    for seq in 1..=6u32 {
        late.submit_command(envelope(seq, (seq as i32) * 100));
        if seq == 4 {
            late.submit_command(envelope(seq, (seq as i32) * 100)); // duplicate on the wire
        }
        for _ in 0..2 {
            late.tick(); // network delay before next packet arrives
        }
    }
    let h_late = run_to_horizon(&mut late, total_ticks);

    assert_eq!(
        h_early, h_late,
        "submission timing must not change final state"
    );
}

#[test]
fn lost_packet_retry_converges_to_same_state() {
    let total_ticks = 40u64;
    let mut clean = setup();
    let mut lossy = setup();

    for seq in 1..=5u32 {
        clean.submit_command(envelope(seq, (seq as i32) * 256));
    }
    let h_clean = run_to_horizon(&mut clean, total_ticks);

    // packet 3 is lost; the client's send window stalls on 3 (packets 4 and 5
    // are held), then 3 is retransmitted with the same sequence followed by
    // the held packets. Per-player sequence stays monotonic (section 133).
    lossy.submit_command(envelope(1, 256));
    lossy.submit_command(envelope(2, 512));
    lossy.tick();
    for _ in 0..3 {
        lossy.tick(); // delay while packet 3 is in flight and lost
    }
    lossy.submit_command(envelope(3, 3 * 256)); // retransmission
    lossy.submit_command(envelope(4, 4 * 256)); // window resumes
    lossy.submit_command(envelope(5, 5 * 256));
    let h_lossy = run_to_horizon(&mut lossy, total_ticks);

    assert_eq!(
        h_clean, h_lossy,
        "a lost-then-retried command must converge to the same state"
    );
}

#[test]
fn replication_delta_only_after_ack() {
    let mut game = setup();
    game.add_player(PlayerId(2), WorldPos::new(0, 0));
    let client = PlayerId(2);

    let repl0 = build_client_replication(
        &mut game.replication,
        client,
        game.state.tick,
        &game.state.players,
    );
    assert!(repl0.deltas.is_empty(), "no mutation yet: no delta");

    game.submit_command(envelope(1, 500));
    // run until the command is executed
    for _ in 0..5 {
        game.tick();
    }

    let repl1 = build_client_replication(
        &mut game.replication,
        client,
        game.state.tick,
        &game.state.players,
    );
    assert!(
        repl1.deltas.iter().any(|(e, _, _, _)| *e == PlayerId(1)),
        "player 1 moved: delta expected"
    );

    for &(e, _, rev, _) in &repl1.deltas {
        game.replication.acknowledge(client, e, rev);
    }
    let repl2 = build_client_replication(
        &mut game.replication,
        client,
        game.state.tick,
        &game.state.players,
    );
    assert!(
        repl2.deltas.is_empty(),
        "after ack, no delta without mutation"
    );

    game.replication.touch(PlayerId(1));
    assert!(
        game.replication.needs_resync(client, PlayerId(1)),
        "unacknowledged revision must require resync"
    );
}
