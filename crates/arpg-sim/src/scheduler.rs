use crate::command::CommandEnvelope;
use arpg_core::{PlayerId, Tick};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct ScheduledCommand {
    pub execute_tick: Tick,
    pub player: PlayerId,
    pub sequence: u32,
    pub envelope: CommandEnvelope,
}

#[derive(Debug, Default)]
pub struct Scheduler {
    queue: BTreeMap<Tick, Vec<ScheduledCommand>>,
    last_accepted_sequence: BTreeMap<PlayerId, u32>,
}

impl Scheduler {
    pub fn new() -> Scheduler {
        Scheduler::default()
    }

    pub fn schedule(&mut self, cmd: ScheduledCommand) -> bool {
        let last = self
            .last_accepted_sequence
            .get(&cmd.player)
            .copied()
            .unwrap_or(0);
        if cmd.sequence <= last {
            return false;
        }
        self.last_accepted_sequence.insert(cmd.player, cmd.sequence);
        self.queue.entry(cmd.execute_tick).or_default().push(cmd);
        true
    }

    pub fn take_due(&mut self, tick: Tick) -> Vec<ScheduledCommand> {
        let expired: Vec<Tick> = self.queue.range(..tick).map(|(&t, _)| t).collect();
        let mut due = Vec::new();
        for t in expired {
            if let Some(cmds) = self.queue.remove(&t) {
                due.extend(cmds);
            }
        }
        if let Some(cmds) = self.queue.remove(&tick) {
            due.extend(cmds);
        }
        due
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}
