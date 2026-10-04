use arpg_core::{ObjectId, PlayerId, SkillId, Tick, WorldPos};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementMode {
    Walk,
    Run,
    Forced,
    Knockback,
    Teleport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveIntent {
    pub direction: WorldPos,
    pub movement_mode: MovementMode,
    pub sequence: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UseSkillIntent {
    pub skill: SkillId,
    pub target: Option<WorldPos>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCommand {
    Move(MoveIntent),
    UseSkill(UseSkillIntent),
    Interact(InteractIntent),
    /// Drink a potion from the belt (SPEC.md sections 82-83).
    UseItem(UseItemIntent),
    /// Trade operations (SPEC.md sections 116-118).
    Trade(TradeIntent),
    NoOp,
}

/// A trade operation (SPEC.md sections 116-118): open, set offer,
/// accept or cancel. Wire-level intent; the machine enforces the
/// Requested/Open/Locked/Persisting/Committed/Cancelled lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeIntent {
    Open {
        target: PlayerId,
    },
    SetOffer {
        trade: u64,
        items: Vec<arpg_core::ItemId>,
        gold: u64,
    },
    Accept {
        trade: u64,
    },
    Cancel {
        trade: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UseItemIntent {
    pub item: arpg_core::ItemId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InteractIntent {
    pub target: ObjectId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEnvelope {
    pub sequence: u32,
    pub client_tick: Tick,
    pub player: PlayerId,
    pub command: ClientCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Accepted,
    Deferred,
    RejectedTooOld,
    RejectedInvalid,
    Duplicate,
}
