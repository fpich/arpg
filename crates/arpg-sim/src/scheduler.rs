use crate::command::Admission;
use crate::command::CommandEnvelope;
use arpg_core::{PlayerId, Tick};
use std::collections::BTreeMap;

pub const DEFAULT_INPUT_DELAY_TICKS: u64 = 2;
pub const MIN_INPUT_DELAY_TICKS: u64 = 1;
pub const MAX_INPUT_DELAY_TICKS: u64 = 4;

#[derive(Debug, Clone)]
pub struct ScheduledCommand {
    pub execute_tick: Tick,
    pub player: PlayerId,
    pub sequence: u32,
    pub envelope: CommandEnvelope,
}

/// Arrival-side queue of raw envelopes, drained by the IngestCommands phase.
#[derive(Debug, Default)]
pub struct CommandQueue {
    queue: Vec<CommandEnvelope>,
}

impl CommandQueue {
    pub fn new() -> CommandQueue {
        CommandQueue::default()
    }

    pub fn push(&mut self, envelope: CommandEnvelope) {
        self.queue.push(envelope);
    }

    pub fn drain(&mut self) -> Vec<CommandEnvelope> {
        std::mem::take(&mut self.queue)
    }
}

/// Deterministic command scheduler (SPEC.md sections 130-134).
///
/// Admission: per-player monotonic sequence with deduplication window.
/// Canonicalization: execute_tick, then player_slot, then sequence.
#[derive(Debug, Default)]
pub struct Scheduler {
    queue: BTreeMap<Tick, Vec<ScheduledCommand>>,
    last_accepted_sequence: BTreeMap<PlayerId, u32>,
}

impl Scheduler {
    pub fn new() -> Scheduler {
        Scheduler::default()
    }

    /// Last accepted sequence for a player: the acknowledgement a client
    /// uses to stop retransmitting (SPEC section 186 chaos harness).
    pub fn last_accepted_sequence(&self, player: PlayerId) -> Option<u32> {
        self.last_accepted_sequence.get(&player).copied()
    }

    /// Translate a client envelope into server time and admit it.
    pub fn admit(
        &mut self,
        envelope: CommandEnvelope,
        current_tick: Tick,
        input_delay_ticks: u64,
    ) -> Admission {
        let delay = input_delay_ticks.clamp(MIN_INPUT_DELAY_TICKS, MAX_INPUT_DELAY_TICKS);
        let earliest = current_tick.saturating_add(delay);
        let translated = envelope.client_tick.max(current_tick);
        let execute_tick = translated.max(earliest);

        let last = self
            .last_accepted_sequence
            .get(&envelope.player)
            .copied()
            .unwrap_or(0);

        if envelope.sequence == 0 {
            return Admission::RejectedInvalid;
        }

        if envelope.sequence == last {
            return Admission::Duplicate;
        }
        if envelope.sequence < last {
            return Admission::RejectedTooOld;
        }
        self.last_accepted_sequence
            .insert(envelope.player, envelope.sequence);
        self.queue
            .entry(execute_tick)
            .or_default()
            .push(ScheduledCommand {
                execute_tick,
                player: envelope.player,
                sequence: envelope.sequence,
                envelope,
            });
        Admission::Accepted
    }

    /// Canonicalize all pending commands for a tick:
    /// execute_tick (map key) -> player_slot -> sequence.
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
        due.sort_by(|a, b| {
            a.execute_tick
                .cmp(&b.execute_tick)
                .then(a.player.cmp(&b.player))
                .then(a.sequence.cmp(&b.sequence))
        });
        due
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Canonical bytes of the pending queue, included in the state hash
    /// (SPEC.md section 160: scheduler is part of gameplay state).
    pub fn canonical_hash_input(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        for (tick, cmds) in &self.queue {
            buf.extend_from_slice(&tick.0.to_le_bytes());
            let mut ordered: Vec<&ScheduledCommand> = cmds.iter().collect();
            ordered.sort_by(|a, b| a.player.cmp(&b.player).then(a.sequence.cmp(&b.sequence)));
            for c in ordered {
                buf.extend_from_slice(&c.player.0.to_le_bytes());
                buf.extend_from_slice(&c.sequence.to_le_bytes());
            }
        }
        buf
    }
}
