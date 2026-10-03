use crate::id::EntityId;
use crate::tick::Tick;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventOrderKey {
    pub priority: u16,
    pub target: EntityId,
    pub source: EntityId,
    pub sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameEvent {
    EntitySpawned(EntityId),
    EntityRemoved(EntityId),
    ActionStarted(EntityId),
    ActionInterrupted(EntityId),
    ActionResolved(EntityId),
    DamageApplied {
        target: EntityId,
        source: EntityId,
        amount: i64,
    },
    HealingApplied {
        target: EntityId,
        amount: i64,
    },
    StateApplied(EntityId),
    StateExpired(EntityId),
    EntityKilled {
        target: EntityId,
        killer: EntityId,
    },
    ItemGenerated(crate::id::ItemId),
    ItemDropped(crate::id::ItemId),
    ItemPickedUp(crate::id::ItemId),
    ItemTransferred(crate::id::ItemId),
    QuestChanged(crate::id::QuestDefId),
    PlayerJoined(crate::id::PlayerId),
    PlayerDisconnected(crate::id::PlayerId),
    PlayerRemoved(crate::id::PlayerId),
}

pub struct SpawnMetadata {
    pub born_tick: Tick,
    pub active_from: Tick,
}

impl SpawnMetadata {
    pub fn new(born_tick: Tick) -> SpawnMetadata {
        SpawnMetadata {
            born_tick,
            active_from: born_tick.next(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Alive,
    PendingDeath,
    Dead,
    PendingRemoval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorMode {
    Neutral,
    Walk,
    Run,
    Attack,
    Cast,
    Block,
    HitRecovery,
    Stunned,
    Knockback,
    Interact,
    Dead,
}

pub enum ActionPhase {
    Windup,
    Impact,
    Recovery,
    Complete,
}

impl fmt::Display for Lifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Lifecycle::Alive => write!(f, "Alive"),
            Lifecycle::PendingDeath => write!(f, "PendingDeath"),
            Lifecycle::Dead => write!(f, "Dead"),
            Lifecycle::PendingRemoval => write!(f, "PendingRemoval"),
        }
    }
}
