use arpg_core::{PlayerId, Tick};
use arpg_sim::scheduler::Scheduler;
use arpg_sim::{Admission, ClientCommand, CommandEnvelope};

fn envelope(seq: u32, player: u32, client_tick: u64) -> CommandEnvelope {
    CommandEnvelope {
        sequence: seq,
        client_tick: Tick(client_tick),
        player: PlayerId(player),
        command: ClientCommand::NoOp,
    }
}

#[test]
fn admission_states_are_reported() {
    let mut s = Scheduler::new();
    assert_eq!(s.admit(envelope(1, 1, 1), Tick(0), 2), Admission::Accepted);
    assert_eq!(s.admit(envelope(1, 1, 1), Tick(0), 2), Admission::Duplicate);
    assert_eq!(s.admit(envelope(2, 1, 5), Tick(9), 2), Admission::Accepted);
    // sequence 1 is now older than the accepted 2 for player 1
    assert_eq!(
        s.admit(envelope(1, 1, 5), Tick(9), 2),
        Admission::RejectedTooOld
    );
    assert_eq!(
        s.admit(envelope(0, 1, 1), Tick(0), 2),
        Admission::RejectedInvalid
    );
    assert_eq!(
        s.admit(envelope(1, 2, 1), Tick(0), 2),
        Admission::Accepted,
        "per-player sequences are independent"
    );
}

#[test]
fn execute_tick_respects_input_delay_bounds() {
    let mut s = Scheduler::new();
    // input delay clamped to MAX (4)
    s.admit(envelope(1, 1, 0), Tick(10), 99);
    // earliest = 10 + 4
    assert_eq!(s.take_due(Tick(14)).len(), 1);
}

#[test]
fn canonical_order_is_tick_player_sequence() {
    let mut s = Scheduler::new();
    // arrival order mixes players, but per-player sequences are monotonic
    for &(player, seq) in &[(2u32, 1u32), (1, 1), (2, 2), (1, 2)] {
        let env = CommandEnvelope {
            sequence: seq,
            client_tick: Tick(5),
            player: PlayerId(player),
            command: ClientCommand::NoOp,
        };
        s.admit(env, Tick(0), 1);
    }
    let due = s.take_due(Tick(5));
    assert_eq!(due.len(), 4);
    assert_eq!(due[0].player, PlayerId(1));
    assert_eq!(due[0].sequence, 1);
    assert_eq!(due[1].player, PlayerId(1));
    assert_eq!(due[1].sequence, 2);
    assert_eq!(due[2].player, PlayerId(2));
    assert_eq!(due[2].sequence, 1);
    assert_eq!(due[3].player, PlayerId(2));
    assert_eq!(due[3].sequence, 2);
    assert!(s.is_empty());
}
