use crate::item::{
    roll_weighted, ItemError, ItemInstance, ItemQuality, TreasureClass, TreasureKind,
};
use arpg_core::{ItemDefId, ItemId};

/// Loot pipeline (SPEC.md section 71): DropContext -> TreasureClass ->
/// BaseItem -> ItemLevel -> Quality -> Unique/Set resolution -> Affixes ->
/// Sockets -> Durability -> ItemInstance. Every step draws from a seed
/// derived from the DropSeed so results are reproducible (section 69).
pub struct LootRoller {
    pub next_item_id: u64,
}

impl LootRoller {
    pub fn new() -> LootRoller {
        LootRoller { next_item_id: 1 }
    }

    /// Derive the next deterministic draw from the drop seed and a step
    /// counter (BLAKE3, like every engine stream).
    fn derive(seed: &[u8; 32], step: u64) -> u64 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(seed);
        hasher.update(&step.to_le_bytes());
        let out = *hasher.finalize().as_bytes();
        u64::from_le_bytes(out[..8].try_into().unwrap())
    }

    /// Roll a full item from a treasure class. Returns None on a no-drop.
    pub fn roll(
        &mut self,
        tc: &TreasureClass,
        drop_seed: [u8; 32],
        item_level: u16,
    ) -> Result<Option<ItemInstance>, ItemError> {
        tc.validate()?;
        let mut step = 0u64;
        let mut picks_remaining = tc.picks;
        let mut result: Option<ItemInstance> = None;
        while picks_remaining > 0 {
            picks_remaining -= 1;
            let draw = Self::derive(&drop_seed, step);
            step += 1;
            let resolved = self.resolve_entry(tc, draw, drop_seed, &mut step, 0)?;
            if let Some(def) = resolved {
                let draw = Self::derive(&drop_seed, step);
                step += 1;
                let quality = Self::roll_quality(draw);
                let draw = Self::derive(&drop_seed, step);
                let affix_count = Self::roll_affix_count(quality, draw);
                result = Some(ItemInstance {
                    id: ItemId(self.next_item_id as u128),
                    definition: def,
                    quality,
                    item_level,
                    generation_seed: drop_seed,
                    affixes: (0..affix_count).collect(),
                    sockets: Default::default(),
                    durability: Some(50),
                    flags: 0,
                    charges: None,
                    hands: Default::default(),
                    requirements: Default::default(),
                });
                self.next_item_id += 1;
                break;
            }
        }
        Ok(result)
    }

    fn resolve_entry(
        &self,
        tc: &TreasureClass,
        draw: u64,
        drop_seed: [u8; 32],
        step: &mut u64,
        depth: u16,
    ) -> Result<Option<ItemDefId>, ItemError> {
        if depth > crate::item::MAX_TC_DEPTH {
            return Err(ItemError::InvalidTreasureClass("recursion too deep"));
        }
        let mut weights: Vec<u32> = vec![tc.no_drop_weight];
        let mut kinds: Vec<TreasureKind> = vec![TreasureKind::Nothing];
        for e in &tc.entries {
            weights.push(e.weight);
            kinds.push(e.kind.clone());
        }
        let Some(idx) = roll_weighted(&weights, draw) else {
            return Ok(None);
        };
        match &kinds[idx] {
            TreasureKind::Nothing => Ok(None),
            TreasureKind::Item(def) => Ok(Some(*def)),
            TreasureKind::TreasureClass(inner) => {
                let draw = Self::derive(&drop_seed, *step);
                *step += 1;
                self.resolve_entry(inner, draw, drop_seed, step, depth + 1)
            }
        }
    }

    /// Quality selection (SPEC.md sections 70-73): MagicFind only affects
    /// this step; the effective function belongs to GameRules.
    fn roll_quality(draw: u64) -> ItemQuality {
        match draw % 100 {
            0..=1 => ItemQuality::Unique,
            2..=5 => ItemQuality::Set,
            6..=25 => ItemQuality::Rare,
            26..=60 => ItemQuality::Magic,
            _ => ItemQuality::Normal,
        }
    }

    /// Affix count constraints (SPEC.md section 74): Magic 1-2, Rare up to
    /// 3 prefixes and 3 suffixes. One affix per group is enforced at the
    /// affix-table level by the ruleset.
    fn roll_affix_count(quality: ItemQuality, draw: u64) -> u32 {
        match quality {
            ItemQuality::Magic => 1 + (draw % 2) as u32,
            ItemQuality::Rare => 3 + (draw % 4) as u32,
            ItemQuality::Set | ItemQuality::Unique | ItemQuality::Crafted => 0,
            _ => 0,
        }
    }
}

impl Default for LootRoller {
    fn default() -> LootRoller {
        LootRoller::new()
    }
}
