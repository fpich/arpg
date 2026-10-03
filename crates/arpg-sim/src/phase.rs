use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    BeginTick,
    SessionTransitions,
    IngestCommands,
    CanonicalizeCommands,
    ValidateCommands,
    UpdatePlayerIntent,
    Perception,
    AiDecision,
    ActionStateAdvance,
    MovementIntent,
    MovementResolution,
    InteractionResolution,
    MissileMovement,
    MissileCollision,
    EffectGeneration,
    EffectResolution,
    PeriodicStates,
    Regeneration,
    PendingDeathResolution,
    SpawnResolution,
    LootResolution,
    InventoryTransactions,
    QuestResolution,
    WorldObjectUpdate,
    Expiration,
    ReplicationEventBuild,
    EndTick,
}

impl Phase {
    pub const fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

pub const PHASE_ORDER: [Phase; 27] = [
    Phase::BeginTick,
    Phase::SessionTransitions,
    Phase::IngestCommands,
    Phase::CanonicalizeCommands,
    Phase::ValidateCommands,
    Phase::UpdatePlayerIntent,
    Phase::Perception,
    Phase::AiDecision,
    Phase::ActionStateAdvance,
    Phase::MovementIntent,
    Phase::MovementResolution,
    Phase::InteractionResolution,
    Phase::MissileMovement,
    Phase::MissileCollision,
    Phase::EffectGeneration,
    Phase::EffectResolution,
    Phase::PeriodicStates,
    Phase::Regeneration,
    Phase::PendingDeathResolution,
    Phase::SpawnResolution,
    Phase::LootResolution,
    Phase::InventoryTransactions,
    Phase::QuestResolution,
    Phase::WorldObjectUpdate,
    Phase::Expiration,
    Phase::ReplicationEventBuild,
    Phase::EndTick,
];

pub const PHASES: &[Phase] = &PHASE_ORDER;
