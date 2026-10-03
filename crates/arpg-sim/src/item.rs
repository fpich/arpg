use arpg_core::{ItemDefId, ItemId, LevelInstanceId, PlayerId, WorldPos};
use smallvec::SmallVec;

/// Item qualities (SPEC.md section 70).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemQuality {
    Low,
    Normal,
    Superior,
    Magic,
    Rare,
    Set,
    Unique,
    Crafted,
}

/// Item instance (SPEC.md section 68). Generated properties are fully
/// reproducible from (datapack hash, definition, generation seed, context)
/// without replaying the game (section 69).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemInstance {
    pub id: ItemId,
    pub definition: ItemDefId,
    pub quality: ItemQuality,
    pub item_level: u16,
    pub generation_seed: [u8; 32],
    pub affixes: SmallVec<[u32; 6]>,
    pub sockets: SmallVec<[ItemId; 6]>,
    pub durability: Option<u16>,
    pub flags: u32,
}

/// Exactly one location per ItemId (SPEC.md section 84).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemLocation {
    PlayerInventory(PlayerId, GridPos),
    Equipment(PlayerId, EquipmentSlot),
    Belt(PlayerId, u8),
    Stash(PlayerId, StashPos),
    Cube(PlayerId, GridPos),
    Ground(LevelInstanceId, WorldPos),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GridPos {
    pub x: u8,
    pub y: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StashPos {
    pub page: u8,
    pub x: u8,
    pub y: u8,
}

/// Equipment slots (SPEC.md sections 77-78): two-handed weapons occupy both
/// hand slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EquipmentSlot {
    Head,
    Body,
    MainHand,
    OffHand,
    Gloves,
    Belt,
    Boots,
    Amulet,
    RingLeft,
    RingRight,
    PrimarySet,
    SecondarySet,
}

/// Weighted treasure entry (SPEC.md section 72).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightedTreasureEntry {
    pub weight: u32,
    pub kind: TreasureKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreasureKind {
    Nothing,
    Item(ItemDefId),
    TreasureClass(TreasureClass),
}

/// A treasure class (SPEC.md section 72). Validation: positive weights, no
/// missing refs, no unbounded cycles, bounded recursion depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreasureClass {
    pub picks: i16,
    pub no_drop_weight: u32,
    pub entries: Vec<WeightedTreasureEntry>,
}

pub const MAX_TC_DEPTH: u16 = 16;

impl TreasureClass {
    pub fn validate(&self) -> Result<(), ItemError> {
        if self.picks < 0 {
            return Err(ItemError::InvalidTreasureClass("negative picks"));
        }
        if self.no_drop_weight == 0 && self.entries.is_empty() {
            return Err(ItemError::InvalidTreasureClass("empty class"));
        }
        for e in &self.entries {
            if e.weight == 0 {
                return Err(ItemError::InvalidTreasureClass("zero weight"));
            }
            if let TreasureKind::TreasureClass(inner) = &e.kind {
                inner.validate_depth(1)?;
            }
        }
        Ok(())
    }

    fn validate_depth(&self, depth: u16) -> Result<(), ItemError> {
        if depth > MAX_TC_DEPTH {
            return Err(ItemError::InvalidTreasureClass("recursion too deep"));
        }
        for e in &self.entries {
            if let TreasureKind::TreasureClass(inner) = &e.kind {
                inner.validate_depth(depth + 1)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemError {
    InvalidTreasureClass(&'static str),
    TransactionInvalid(&'static str),
    ItemUnavailable,
    SlotOccupied,
    RequirementNotMet,
}

/// Deterministic roll helper: pick among weighted entries using a raw u64
/// draw. No RNG hidden state: the draw comes from the DropSeed stream.
pub fn roll_weighted(weights: &[u32], draw: u64) -> Option<usize> {
    let total: u64 = weights.iter().map(|w| *w as u64).sum();
    if total == 0 {
        return None;
    }
    let mut cursor = draw % total;
    for (idx, w) in weights.iter().enumerate() {
        if cursor < *w as u64 {
            return Some(idx);
        }
        cursor -= *w as u64;
    }
    None
}
