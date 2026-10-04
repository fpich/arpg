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
    /// Merchant services (SPEC.md sections 95-97): buy, sell, repair,
    /// gamble. Personal stock per player (section 95 v1 decision).
    Merchant(MerchantIntent),
    /// Weapon swap (SPEC.md section 78): exchange the active weapon
    /// slots with the secondary loadout in one gameplay action.
    SwapWeapons,
    NoOp,
}

/// A merchant operation (SPEC.md sections 95-97).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MerchantIntent {
    Buy {
        merchant: u64,
        def: arpg_core::ItemDefId,
        price: Option<u64>,
    },
    Sell {
        merchant: u64,
        item: arpg_core::ItemId,
        base_price: u64,
    },
    Repair {
        merchant: u64,
        item: Option<arpg_core::ItemId>,
    },
    /// Recharge (SPEC.md section 79): restore a charged item's skill
    /// charges; cost scales with the number of missing charges.
    Recharge {
        merchant: u64,
        item: arpg_core::ItemId,
    },
    Gamble {
        merchant: u64,
        offer: u32,
    },
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
