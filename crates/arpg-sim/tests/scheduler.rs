use arpg_core::{PlayerId, Tick};
use arpg_sim::scheduler::{ScheduledCommand, Scheduler};
use arpg_sim::{ClientCommand, CommandEnvelope};

fn scheduled(seq: u32, tick: u64, player: u32) -> ScheduledCommand {
    ScheduledCommand {
        execute_tick: Tick(tick),
        player: PlayerId(player),
        sequence: seq,
        envelope: CommandEnvelope {
            sequence: seq,
            client_tick: Tick(tick),
            player: PlayerId(player),
            command: ClientCommand::NoOp,
        },
    }
}

#[test]
fn scheduler_returns_due_in_tick_order() {
    let mut s = Scheduler::new();
    s.schedule(scheduled(1, 5, 1));
    s.schedule(scheduled(2, 3, 1));
    s.schedule(scheduled(3, 4, 1));

    let due = s.take_due(Tick(4));
    assert_eq!(due.len(), 2);
    assert_eq!(due[0].execute_tick, Tick(3));
    assert_eq!(due[1].execute_tick, Tick(4));
    assert!(!s.is_empty(), "command scheduled for tick 5 must remain");
    assert_eq!(s.take_due(Tick(5)).len(), 1);
    assert!(s.is_empty());
}

#[test]
fn scheduler_per_player_sequences_are_independent() {
    let mut s = Scheduler::new();
    assert!(s.schedule(scheduled(1, 1, 1)));
    assert!(s.schedule(scheduled(1, 1, 2)));
    assert!(!s.schedule(scheduled(1, 2, 1)));
}
