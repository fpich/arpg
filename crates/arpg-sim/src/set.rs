//! Item sets (SPEC.md section 89): partial bonuses scale with the
//! equipped piece count, specific-piece conditions gate extra bonuses,
//! and the full bonus applies when every piece is equipped. Bonuses are
//! recomputed after each equipment transaction.

use crate::item::EquipmentSlot;
use arpg_core::{ItemDefId, PlayerId};
use std::collections::BTreeMap;

/// A condition on specific pieces (SPEC.md section 89): a bonus that
/// only counts when exact pieces are equipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PieceCondition {
    pub required_defs: Vec<ItemDefId>,
}

/// One tier of a set bonus (SPEC.md section 89).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetBonusTier {
    /// Minimum equipped piece count to activate this tier.
    pub pieces: u32,
    /// Flat stat contribution in basis points.
    pub magnitude_bp: i32,
    /// Optional specific-piece condition gating this tier.
    pub condition: Option<PieceCondition>,
}

/// A set definition (SPEC.md section 89).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetDefinition {
    pub id: u32,
    pub name: String,
    /// Item definitions that belong to this set, mapped to their slot.
    pub pieces: BTreeMap<ItemDefId, EquipmentSlot>,
    /// Partial bonuses sorted by piece count; the last tier whose
    /// piece count is reached and whose condition holds applies.
    pub partial: Vec<SetBonusTier>,
    /// Full bonus applied when every piece is equipped.
    pub full: SetBonusTier,
}

impl SetDefinition {
    /// Compute the active set bonuses for an equipped loadout (SPEC.md
    /// section 89): equipped_piece_count, specific_piece_conditions,
    /// partial bonuses, full bonus.
    pub fn active_bonuses(
        &self,
        equipped: &BTreeMap<EquipmentSlot, ItemDefId>,
    ) -> Vec<&SetBonusTier> {
        let equipped_from_set: Vec<(ItemDefId, EquipmentSlot)> = self
            .pieces
            .iter()
            .filter(|(def, slot)| equipped.get(slot) == Some(*def))
            .map(|(def, slot)| (*def, *slot))
            .collect();
        let count = equipped_from_set.len() as u32;
        let total = self.pieces.len() as u32;
        let mut active = Vec::new();
        for tier in &self.partial {
            if count < tier.pieces {
                continue;
            }
            if let Some(cond) = &tier.condition {
                let ok = cond
                    .required_defs
                    .iter()
                    .all(|d| equipped_from_set.iter().any(|(def, _)| def == d));
                if !ok {
                    continue;
                }
            }
            active.push(tier);
        }
        if count >= total && total > 0 {
            active.push(&self.full);
        }
        active
    }
}

/// The set system: registry of set definitions plus per-player cached
/// totals (SPEC.md section 89: recomputed after equipment transactions).
#[derive(Debug, Default, Clone)]
pub struct SetSystem {
    pub definitions: BTreeMap<u32, SetDefinition>,
    /// Cached active bonus magnitude per (player, set), recomputed after
    /// each equipment transaction.
    pub active: BTreeMap<(PlayerId, u32), i32>,
}

impl SetSystem {
    pub fn new() -> SetSystem {
        SetSystem::default()
    }

    /// Recompute the cached bonuses of one player from their equipment
    /// (SPEC.md section 89). Returns the total magnitude across sets.
    pub fn recompute(
        &mut self,
        player: PlayerId,
        equipped: &BTreeMap<EquipmentSlot, ItemDefId>,
    ) -> i32 {
        let mut total = 0;
        let mut per_set = BTreeMap::new();
        for (id, def) in &self.definitions {
            let bonuses = def.active_bonuses(equipped);
            if bonuses.is_empty() {
                continue;
            }
            let magnitude: i32 = bonuses.iter().map(|t| t.magnitude_bp).sum();
            per_set.insert(*id, magnitude);
            total += magnitude;
        }
        for (set, magnitude) in per_set {
            self.active.insert((player, set), magnitude);
        }
        total
    }

    /// Cached magnitude of one set for one player.
    pub fn bonus_of(&self, player: PlayerId, set: u32) -> i32 {
        self.active.get(&(player, set)).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_set() -> SetDefinition {
        let mut pieces = BTreeMap::new();
        pieces.insert(ItemDefId(1), EquipmentSlot::Head);
        pieces.insert(ItemDefId(2), EquipmentSlot::Body);
        pieces.insert(ItemDefId(3), EquipmentSlot::Boots);
        SetDefinition {
            id: 7,
            name: "Sample".into(),
            pieces,
            partial: vec![
                SetBonusTier {
                    pieces: 2,
                    magnitude_bp: 500,
                    condition: None,
                },
                SetBonusTier {
                    pieces: 2,
                    magnitude_bp: 300,
                    condition: Some(PieceCondition {
                        required_defs: vec![ItemDefId(1)],
                    }),
                },
            ],
            full: SetBonusTier {
                pieces: 3,
                magnitude_bp: 2000,
                condition: None,
            },
        }
    }

    fn equip(defs: &[(ItemDefId, EquipmentSlot)]) -> BTreeMap<EquipmentSlot, ItemDefId> {
        defs.iter().map(|(d, s)| (*s, *d)).collect()
    }

    #[test]
    fn no_pieces_no_bonus() {
        let set = sample_set();
        let equipped = equip(&[]);
        assert!(set.active_bonuses(&equipped).is_empty());
    }

    #[test]
    fn partial_bonus_scales_with_piece_count() {
        let set = sample_set();
        let equipped = equip(&[
            (ItemDefId(2), EquipmentSlot::Body),
            (ItemDefId(3), EquipmentSlot::Boots),
        ]);
        let bonuses = set.active_bonuses(&equipped);
        // 2 pieces: unconditional 500 applies; the 300 tier needs the head
        assert_eq!(bonuses.len(), 1);
        assert_eq!(bonuses[0].magnitude_bp, 500);
    }

    #[test]
    fn piece_condition_gates_tier() {
        let set = sample_set();
        let equipped = equip(&[
            (ItemDefId(1), EquipmentSlot::Head),
            (ItemDefId(2), EquipmentSlot::Body),
        ]);
        let bonuses = set.active_bonuses(&equipped);
        // head equipped: both 2-piece tiers apply
        assert_eq!(bonuses.len(), 2);
        let total: i32 = bonuses.iter().map(|t| t.magnitude_bp).sum();
        assert_eq!(total, 800);
    }

    #[test]
    fn full_bonus_requires_every_piece() {
        let set = sample_set();
        let equipped = equip(&[
            (ItemDefId(1), EquipmentSlot::Head),
            (ItemDefId(2), EquipmentSlot::Body),
            (ItemDefId(3), EquipmentSlot::Boots),
        ]);
        let bonuses = set.active_bonuses(&equipped);
        assert!(bonuses.iter().any(|t| t.magnitude_bp == 2000));
        let total: i32 = bonuses.iter().map(|t| t.magnitude_bp).sum();
        assert_eq!(total, 2800);
    }

    #[test]
    fn recompute_caches_per_player() {
        let mut sys = SetSystem::new();
        sys.definitions.insert(7, sample_set());
        let equipped = equip(&[
            (ItemDefId(1), EquipmentSlot::Head),
            (ItemDefId(2), EquipmentSlot::Body),
        ]);
        let total = sys.recompute(PlayerId(1), &equipped);
        assert_eq!(total, 800);
        assert_eq!(sys.bonus_of(PlayerId(1), 7), 800);
        assert_eq!(sys.bonus_of(PlayerId(2), 7), 0);
    }
}
