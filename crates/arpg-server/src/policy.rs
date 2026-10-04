//! Server policy and game lifecycle (SPEC.md sections 148-149).
//!
//! `EmptyGrace`: after the last player leaves, the game stays alive for a
//! policy duration. No gameplay depends on its value. At expiry the
//! remaining characters are saved, then the GameState is destroyed.

use arpg_core::PlayerId;

/// Server-wide policies (section 149): the EmptyGrace duration is pure
/// policy - no gameplay depends on its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerPolicy {
    /// Ticks the game stays alive after becoming empty.
    pub empty_grace_ticks: u32,
}

impl Default for ServerPolicy {
    fn default() -> ServerPolicy {
        // 10 minutes at 25 tps
        ServerPolicy {
            empty_grace_ticks: 10 * 60 * 25,
        }
    }
}

/// Lifecycle of one hosted game (sections 148-149).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameLifecycle {
    /// Active: at least one player connected.
    Active,
    /// Empty but inside the grace window (section 149).
    EmptyGrace,
    /// Grace expired: characters saved, GameState destroyed.
    Destroyed,
}

/// Tracks the empty-grace countdown for one game (section 149).
#[derive(Debug)]
pub struct GameGuard {
    policy: ServerPolicy,
    lifecycle: GameLifecycle,
    grace_remaining: u32,
}

impl GameGuard {
    pub fn new(policy: ServerPolicy) -> GameGuard {
        GameGuard {
            policy,
            lifecycle: GameLifecycle::Active,
            grace_remaining: policy.empty_grace_ticks,
        }
    }

    pub fn lifecycle(&self) -> GameLifecycle {
        self.lifecycle
    }

    /// Player count changed: a non-empty game is Active again with the
    /// grace window rearmed; an empty game starts counting down.
    pub fn observe_players(&mut self, connected: usize) {
        if connected > 0 {
            self.lifecycle = GameLifecycle::Active;
            self.grace_remaining = self.policy.empty_grace_ticks;
        } else if self.lifecycle == GameLifecycle::Active {
            self.lifecycle = GameLifecycle::EmptyGrace;
        }
    }

    /// Advance one tick while empty. Returns true at the tick the grace
    /// expires: the caller must save remaining characters, then destroy
    /// the GameState.
    pub fn tick_empty(&mut self) -> bool {
        if self.lifecycle != GameLifecycle::EmptyGrace {
            return false;
        }
        if self.grace_remaining > 0 {
            self.grace_remaining -= 1;
        }
        if self.grace_remaining == 0 {
            self.lifecycle = GameLifecycle::Destroyed;
            return true;
        }
        false
    }

    pub fn grace_remaining(&self) -> u32 {
        self.grace_remaining
    }
}

/// Characters still present when the grace expires (section 149): the
/// pipeline is save -> destroy; the monster-less GameState is dropped
/// entirely. Monsters never persist anyway (they are spawn-time state).
pub fn empty_grace_expiry_pipeline(players: &[PlayerId]) -> Vec<PlayerId> {
    players.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_game_counts_down_then_destroys() {
        let policy = ServerPolicy {
            empty_grace_ticks: 3,
        };
        let mut guard = GameGuard::new(policy);
        guard.observe_players(1);
        assert_eq!(guard.lifecycle(), GameLifecycle::Active);
        guard.observe_players(0);
        assert_eq!(guard.lifecycle(), GameLifecycle::EmptyGrace);
        assert!(!guard.tick_empty());
        assert!(!guard.tick_empty());
        assert!(guard.tick_empty(), "grace expired: save then destroy");
        assert_eq!(guard.lifecycle(), GameLifecycle::Destroyed);
    }

    #[test]
    fn rejoin_within_grace_rearms_the_window() {
        let policy = ServerPolicy {
            empty_grace_ticks: 3,
        };
        let mut guard = GameGuard::new(policy);
        guard.observe_players(0);
        assert!(!guard.tick_empty());
        guard.observe_players(1);
        assert_eq!(guard.lifecycle(), GameLifecycle::Active);
        assert_eq!(guard.grace_remaining(), 3, "window rearmed");
        // emptying again restarts the full window
        guard.observe_players(0);
        assert_eq!(guard.grace_remaining(), 3);
    }

    #[test]
    fn default_policy_is_ten_minutes() {
        assert_eq!(ServerPolicy::default().empty_grace_ticks, 15_000);
    }
}
