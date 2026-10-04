//! Client-side prediction and reconciliation (SPEC.md sections 144-145).
//!
//! Prediction is local movement only: the client applies its own movement
//! inputs immediately and the server remains the sole authority. After a
//! snapshot arrives, reconciliation sets the authoritative position,
//! removes acknowledged commands and replays unacknowledged movement
//! inputs on top of it. No server rollback exists in v1.
//!
//! This module is a pure library: it holds no I/O, no clock and no RNG
//! (INV-003/004/005). The client feeds it snapshots and its pending
//! inputs; it produces the reconciled position deterministically.

use arpg_core::{PlayerId, Tick, WorldPos};
use std::collections::BTreeMap;
use std::collections::VecDeque;

/// One locally predicted movement input, acknowledged by sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovementInput {
    /// Client command sequence (section 133): monotone per player.
    pub sequence: u32,
    /// Client tick at emission.
    pub tick: Tick,
    /// Target position of the intent (movement is position-targeted).
    pub target: WorldPos,
}

/// Prediction state for one player's client.
#[derive(Debug, Default)]
pub struct Prediction {
    /// Inputs not yet acknowledged by the server, in sequence order.
    pending: VecDeque<MovementInput>,
    /// Last predicted position.
    predicted: Option<WorldPos>,
}

impl Prediction {
    pub fn new() -> Prediction {
        Prediction::default()
    }

    /// Apply a local movement input immediately (section 144): prediction
    /// moves the local view without waiting for the server.
    pub fn apply_input(&mut self, input: MovementInput) -> WorldPos {
        self.predicted = Some(input.target);
        self.pending.push_back(input);
        input.target
    }

    /// Last predicted (or reconciled) position.
    pub fn position(&self) -> Option<WorldPos> {
        self.predicted
    }

    /// Number of unacknowledged inputs.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Reconcile with an authoritative snapshot (section 145):
    /// 1. set the authoritative position,
    /// 2. remove acknowledged commands (sequence <= last acked),
    /// 3. replay unacknowledged movement inputs on top of it.
    ///
    /// No rollback: the server position always wins as the base.
    pub fn reconcile(&mut self, authoritative: WorldPos, last_acked_sequence: u32) -> WorldPos {
        let acked = last_acked_sequence;
        while let Some(front) = self.pending.front() {
            if front.sequence <= acked {
                self.pending.pop_front();
            } else {
                break;
            }
        }
        let mut pos = authoritative;
        for input in &self.pending {
            pos = input.target;
        }
        self.predicted = Some(pos);
        pos
    }
}

/// Client-side prediction bookkeeping across the players a client sees.
#[derive(Debug, Default)]
pub struct PredictionSystem {
    predictions: BTreeMap<PlayerId, Prediction>,
    /// Last acknowledged sequence per player, from CommandAck.
    acked: BTreeMap<PlayerId, u32>,
}

impl PredictionSystem {
    pub fn new() -> PredictionSystem {
        PredictionSystem::default()
    }

    pub fn prediction_mut(&mut self, player: PlayerId) -> &mut Prediction {
        self.predictions.entry(player).or_default()
    }

    /// Record the last sequence acknowledged by the server (CommandAck,
    /// section 135).
    pub fn server_ack(&mut self, player: PlayerId, sequence: u32) {
        let ack = self.acked.entry(player).or_insert(0);
        if sequence > *ack {
            *ack = sequence;
        }
    }

    /// Reconcile one player against an authoritative snapshot.
    pub fn reconcile(&mut self, player: PlayerId, authoritative: WorldPos) -> WorldPos {
        let acked = self.acked.get(&player).copied().unwrap_or(0);
        self.prediction_mut(player).reconcile(authoritative, acked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prediction_applies_local_movement_immediately() {
        let mut pred = Prediction::new();
        let pos = pred.apply_input(MovementInput {
            sequence: 1,
            tick: Tick(1),
            target: WorldPos::new(10, 0),
        });
        assert_eq!(pos, WorldPos::new(10, 0));
        assert_eq!(pred.pending_len(), 1);
    }

    #[test]
    fn reconcile_sets_authoritative_and_replays_unacked() {
        let mut pred = Prediction::new();
        pred.apply_input(MovementInput {
            sequence: 1,
            tick: Tick(1),
            target: WorldPos::new(10, 0),
        });
        pred.apply_input(MovementInput {
            sequence: 2,
            tick: Tick(2),
            target: WorldPos::new(20, 0),
        });
        pred.apply_input(MovementInput {
            sequence: 3,
            tick: Tick(3),
            target: WorldPos::new(30, 0),
        });
        // server confirms inputs 1-2 with its own position for input 2
        let pos = pred.reconcile(WorldPos::new(18, 0), 2);
        assert_eq!(pred.pending_len(), 1, "acked inputs removed");
        assert_eq!(pos, WorldPos::new(30, 0), "unacked input replayed on top");
    }

    #[test]
    fn authoritative_position_wins_when_all_acked() {
        let mut pred = Prediction::new();
        pred.apply_input(MovementInput {
            sequence: 1,
            tick: Tick(1),
            target: WorldPos::new(10, 0),
        });
        // server disagrees with the prediction: it wins, no pending replay
        let pos = pred.reconcile(WorldPos::new(7, 0), 1);
        assert_eq!(pos, WorldPos::new(7, 0));
        assert_eq!(pred.pending_len(), 0);
    }

    #[test]
    fn prediction_system_tracks_acks_per_player() {
        let mut sys = PredictionSystem::new();
        sys.prediction_mut(PlayerId(1)).apply_input(MovementInput {
            sequence: 5,
            tick: Tick(1),
            target: WorldPos::new(50, 0),
        });
        sys.server_ack(PlayerId(1), 3);
        let pos = sys.reconcile(PlayerId(1), WorldPos::new(40, 0));
        assert_eq!(pos, WorldPos::new(50, 0), "input 5 still unacked, replayed");
        sys.server_ack(PlayerId(1), 5);
        let pos = sys.reconcile(PlayerId(1), WorldPos::new(44, 0));
        assert_eq!(pos, WorldPos::new(44, 0), "fully acked: authoritative wins");
    }
}
