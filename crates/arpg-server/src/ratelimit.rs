//! Rate limiting policies (SPEC.md section 171). Policies are separate per
//! category: network messages/sec, commands/sec, inventory operations/sec,
//! trade operations/sec, chat messages/sec, connection attempts/sec.
//!
//! This is a POLICY domain: it runs on server tick time (not wall clock) so it
//! stays deterministic, and it never modifies the result of an already
//! accepted command — it only drops or defers *new* arrivals.

use arpg_sim::{ClientCommand, CommandEnvelope};

/// A budget refilled per tick: `max_burst` allows short bursts while
/// `sustained_per_tick` bounds the average rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    pub sustained_per_tick: u32,
    pub max_burst: u32,
}

impl RateLimit {
    pub const fn per_tick(n: u32) -> RateLimit {
        RateLimit {
            sustained_per_tick: n,
            max_burst: n,
        }
    }

    pub const fn burst(sustained_per_tick: u32, max_burst: u32) -> RateLimit {
        RateLimit {
            sustained_per_tick,
            max_burst,
        }
    }
}

/// Reference policies (SPEC.md section 171). Server-side only: values belong
/// to the POLICY domain and can be tuned without touching replay results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatePolicy {
    pub network_messages: RateLimit,
    pub commands: RateLimit,
    pub inventory_operations: RateLimit,
    pub trade_operations: RateLimit,
    pub chat_messages: RateLimit,
    pub connection_attempts: RateLimit,
}

impl RatePolicy {
    /// 20 ticks/sec reference server: Move at client tick rate dominates the
    /// command budget.
    pub const fn reference() -> RatePolicy {
        RatePolicy {
            network_messages: RateLimit::burst(10, 100),
            commands: RateLimit::burst(10, 60),
            inventory_operations: RateLimit::per_tick(5),
            trade_operations: RateLimit::per_tick(2),
            chat_messages: RateLimit::per_tick(2),
            connection_attempts: RateLimit::burst(1, 5),
        }
    }
}

/// Categorized arrival: the category is derived from the payload, so
/// classification cannot be spoofed by the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrivalKind {
    NetworkMessage,
    Command,
    InventoryOperation,
    TradeOperation,
    ChatMessage,
    ConnectionAttempt,
}

impl ArrivalKind {
    pub fn of_command(envelope: &CommandEnvelope) -> ArrivalKind {
        match &envelope.command {
            ClientCommand::NoOp => ArrivalKind::NetworkMessage,
            ClientCommand::Interact(_) | ClientCommand::UseItem(_) => {
                ArrivalKind::InventoryOperation
            }
            _ => ArrivalKind::Command,
        }
    }
}

/// Outcome of a rate-limit check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateDecision {
    Allowed,
    Denied,
}

/// Fixed-capacity token bucket per (kind) per player. Memory is bounded by
/// construction: `ARRIVAL_KINDS` counters per player (SPEC section 170: no
/// allocation proportional to unbounded client input).
#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: u32,
}

pub const ARRIVAL_KINDS: usize = 6;

fn limit_for(policy: &RatePolicy, kind: ArrivalKind) -> RateLimit {
    match kind {
        ArrivalKind::NetworkMessage => policy.network_messages,
        ArrivalKind::Command => policy.commands,
        ArrivalKind::InventoryOperation => policy.inventory_operations,
        ArrivalKind::TradeOperation => policy.trade_operations,
        ArrivalKind::ChatMessage => policy.chat_messages,
        ArrivalKind::ConnectionAttempt => policy.connection_attempts,
    }
}

pub const ALL_KINDS: [ArrivalKind; ARRIVAL_KINDS] = [
    ArrivalKind::NetworkMessage,
    ArrivalKind::Command,
    ArrivalKind::InventoryOperation,
    ArrivalKind::TradeOperation,
    ArrivalKind::ChatMessage,
    ArrivalKind::ConnectionAttempt,
];

fn index(kind: ArrivalKind) -> usize {
    match kind {
        ArrivalKind::NetworkMessage => 0,
        ArrivalKind::Command => 1,
        ArrivalKind::InventoryOperation => 2,
        ArrivalKind::TradeOperation => 3,
        ArrivalKind::ChatMessage => 4,
        ArrivalKind::ConnectionAttempt => 5,
    }
}

/// Server-wide rate limiter. Each player slot has one bounded bucket array;
/// registering a player beyond the configured capacity returns false (the
/// caller applies its own connection policy).
pub struct RateLimiter {
    policy: RatePolicy,
    players: std::collections::BTreeMap<arpg_core::PlayerId, [Bucket; ARRIVAL_KINDS]>,
    max_players: usize,
}

impl RateLimiter {
    pub fn new(policy: RatePolicy, max_players: usize) -> RateLimiter {
        RateLimiter {
            policy,
            players: std::collections::BTreeMap::new(),
            max_players,
        }
    }

    /// Register a player slot; returns false if the player capacity is full.
    /// Capacity is the hard bound on memory (section 170).
    pub fn register(&mut self, player: arpg_core::PlayerId) -> bool {
        if self.players.contains_key(&player) {
            return true;
        }
        if self.players.len() >= self.max_players {
            return false;
        }
        let mut buckets = [Bucket { tokens: 0 }; ARRIVAL_KINDS];
        for kind in ALL_KINDS {
            buckets[index(kind)].tokens = limit_for(&self.policy, kind).max_burst;
        }
        self.players.insert(player, buckets);
        true
    }

    pub fn unregister(&mut self, player: arpg_core::PlayerId) {
        self.players.remove(&player);
    }

    /// Advance one server tick: refill every bucket by its sustained rate,
    /// capped at max_burst.
    pub fn tick(&mut self) {
        for buckets in self.players.values_mut() {
            for kind in ALL_KINDS {
                let limit = limit_for(&self.policy, kind);
                let b = &mut buckets[index(kind)];
                b.tokens = b.tokens.saturating_add(limit.sustained_per_tick);
                if b.tokens > limit.max_burst {
                    b.tokens = limit.max_burst;
                }
            }
        }
    }

    /// Consume one arrival of `kind` for `player`. Returns `Denied` without
    /// consuming when the bucket is empty, so a denied flood never earns
    /// budget.
    pub fn check(&mut self, player: arpg_core::PlayerId, kind: ArrivalKind) -> RateDecision {
        let Some(buckets) = self.players.get_mut(&player) else {
            return RateDecision::Denied;
        };
        let b = &mut buckets[index(kind)];
        if b.tokens == 0 {
            RateDecision::Denied
        } else {
            b.tokens -= 1;
            RateDecision::Allowed
        }
    }

    /// Check an incoming command envelope. Every envelope is at least a
    /// network message; it is additionally charged against its payload
    /// category (section 171: commands/sec and inventory/trade ops/sec are
    /// separate policies).
    pub fn check_command(&mut self, envelope: &CommandEnvelope) -> RateDecision {
        if self.check(envelope.player, ArrivalKind::NetworkMessage) == RateDecision::Denied {
            return RateDecision::Denied;
        }
        let kind = ArrivalKind::of_command(envelope);
        if kind == ArrivalKind::NetworkMessage {
            return RateDecision::Allowed;
        }
        self.check(envelope.player, kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arpg_core::{PlayerId, Tick};

    fn envelope(seq: u32) -> CommandEnvelope {
        CommandEnvelope {
            sequence: seq,
            client_tick: Tick(0),
            player: PlayerId(1),
            command: ClientCommand::Move(arpg_sim::MoveIntent {
                direction: arpg_core::WorldPos::new(0, 0),
                movement_mode: arpg_sim::MovementMode::Run,
                sequence: seq,
            }),
        }
    }

    #[test]
    fn burst_then_deny_until_refill() {
        let mut limiter = RateLimiter::new(RatePolicy::reference(), 8);
        assert!(limiter.register(PlayerId(1)));
        let allowed = RatePolicy::reference().commands.max_burst;
        for i in 0..allowed {
            assert_eq!(
                limiter.check_command(&envelope(i)),
                RateDecision::Allowed,
                "within burst"
            );
        }
        assert_eq!(limiter.check_command(&envelope(999)), RateDecision::Denied);
        limiter.tick();
        assert_eq!(
            limiter.check_command(&envelope(1000)),
            RateDecision::Allowed
        );
    }

    #[test]
    fn sustained_rate_bounds_average() {
        let policy = RatePolicy {
            commands: RateLimit::per_tick(2),
            ..RatePolicy::reference()
        };
        let mut limiter = RateLimiter::new(policy, 8);
        limiter.register(PlayerId(1));
        let mut allowed = 0;
        for _ in 0..100 {
            for i in 0..10 {
                if limiter.check_command(&envelope(i)) == RateDecision::Allowed {
                    allowed += 1;
                }
            }
            limiter.tick();
        }
        assert_eq!(allowed, 2 * 100);
    }

    #[test]
    fn inventory_ops_have_their_own_budget() {
        let policy = RatePolicy {
            commands: RateLimit::per_tick(1),
            inventory_operations: RateLimit::per_tick(1),
            network_messages: RateLimit::per_tick(100),
            ..RatePolicy::reference()
        };
        let mut limiter = RateLimiter::new(policy, 8);
        limiter.register(PlayerId(1));
        let inv = CommandEnvelope {
            command: ClientCommand::Interact(arpg_sim::InteractIntent {
                target: arpg_core::ObjectId(0),
            }),
            ..envelope(1)
        };
        assert_eq!(limiter.check_command(&inv), RateDecision::Allowed);
        assert_eq!(limiter.check_command(&inv), RateDecision::Denied);
        // a Move still has its own command budget
        assert_eq!(limiter.check_command(&envelope(2)), RateDecision::Allowed);
    }

    #[test]
    fn unknown_player_is_denied() {
        let mut limiter = RateLimiter::new(RatePolicy::reference(), 8);
        assert_eq!(
            limiter.check(PlayerId(42), ArrivalKind::Command),
            RateDecision::Denied
        );
    }

    #[test]
    fn capacity_is_bounded() {
        let mut limiter = RateLimiter::new(RatePolicy::reference(), 2);
        assert!(limiter.register(PlayerId(1)));
        assert!(limiter.register(PlayerId(2)));
        assert!(!limiter.register(PlayerId(3)));
        limiter.unregister(PlayerId(2));
        assert!(limiter.register(PlayerId(3)));
    }

    #[test]
    fn denied_flood_never_earns_budget() {
        let policy = RatePolicy {
            connection_attempts: RateLimit::per_tick(1),
            ..RatePolicy::reference()
        };
        let mut limiter = RateLimiter::new(policy, 8);
        limiter.register(PlayerId(1));
        assert_eq!(
            limiter.check(PlayerId(1), ArrivalKind::ConnectionAttempt),
            RateDecision::Allowed
        );
        for _ in 0..1000 {
            assert_eq!(
                limiter.check(PlayerId(1), ArrivalKind::ConnectionAttempt),
                RateDecision::Denied
            );
        }
        limiter.tick();
        assert_eq!(
            limiter.check(PlayerId(1), ArrivalKind::ConnectionAttempt),
            RateDecision::Allowed
        );
    }
}
