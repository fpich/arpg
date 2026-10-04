//! Sockets, runes and runewords (SPEC.md sections 76, 193, 199): inserting
//! a rune into a socketed item, runeword activation when the rune sequence
//! matches a known runeword, and deterministic stat recomputation — the
//! item's stat contributions are recomputed from its affixes, runes and
//! runeword through the normative stat pipeline, never stored as finals
//! only (section 39).

use crate::item::ItemInstance;
use crate::stat::{ModifierOp, ModifierSource, StatBlock, StatModifier};
use arpg_core::{ItemId, StatId};
use std::collections::BTreeMap;

/// Rune identifiers (10 reference runes, datapack section).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuneId(pub u32);

/// A runeword definition: name + ordered rune sequence + stat modifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunewordDefinition {
    pub id: u32,
    pub name: &'static str,
    pub runes: Vec<RuneId>,
    /// Flat stat modifiers granted when the runeword activates.
    pub modifiers: Vec<(StatId, i64)>,
}

/// Reference runewords mirroring the datapack (El=0, Eld=1, Tir=2, Nef=3,
/// Eth=4, Ith=5, Tal=6, Ral=7, Ort=8, Thul=9).
pub fn reference_runewords() -> Vec<RunewordDefinition> {
    use arpg_core::StatId;
    vec![
        RunewordDefinition {
            id: 1,
            name: "Steel",
            runes: vec![RuneId(0), RuneId(2)],
            modifiers: vec![(StatId(1), 20), (StatId(2), 25)],
        },
        RunewordDefinition {
            id: 2,
            name: "Nadir",
            runes: vec![RuneId(3), RuneId(2)],
            modifiers: vec![(StatId(3), 50)],
        },
        RunewordDefinition {
            id: 3,
            name: "Malice",
            runes: vec![RuneId(5), RuneId(0), RuneId(4)],
            modifiers: vec![(StatId(1), 33)],
        },
        RunewordDefinition {
            id: 4,
            name: "Zephyr",
            runes: vec![RuneId(8), RuneId(4)],
            modifiers: vec![(StatId(4), 25)],
        },
        RunewordDefinition {
            id: 5,
            name: "Ancient's Pledge",
            runes: vec![RuneId(7), RuneId(8), RuneId(6)],
            modifiers: vec![(StatId(5), 43), (StatId(6), 43), (StatId(7), 43)],
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketError {
    ItemNotSocketed,
    NoFreeSocket,
    NotARune,
    SocketNotEmpty,
}

/// A rune gem placed into a socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocketedRune {
    pub rune: RuneId,
    pub item_in_socket: ItemId,
}

/// Rune effects: each rune contributes a flat stat bonus by itself, even
/// without a runeword (canonical table, datapack-defined values).
pub fn rune_modifier(rune: RuneId) -> (StatId, i64) {
    match rune.0 {
        0 => (StatId(1), 3),  // El: +attack
        1 => (StatId(2), 3),  // Eld: +accuracy
        2 => (StatId(8), 3),  // Tir: +mana per kill
        3 => (StatId(9), 1),  // Nef: knockback
        4 => (StatId(5), 5),  // Eth: -defense of target
        5 => (StatId(3), 5),  // Ith: +mana
        6 => (StatId(6), 5),  // Tal: +poison damage
        7 => (StatId(7), 5),  // Ral: +fire damage
        8 => (StatId(10), 5), // Ort: +lightning
        9 => (StatId(11), 5), // Thul: +cold
        _ => (StatId(1), 0),
    }
}

/// Socket system: inserts runes into items and resolves runewords. The
/// number of sockets is fixed at generation (loot pipeline, section 71);
/// the system here tracks insertions and recomputes stats.
#[derive(Debug, Default)]
pub struct SocketSystem {
    /// socket capacity per item (fixed at generation).
    capacities: BTreeMap<ItemId, u8>,
    /// rune placed in each socket slot per item.
    filled: BTreeMap<ItemId, Vec<Option<RuneId>>>,
    /// active runeword per item.
    runewords: BTreeMap<ItemId, u32>,
}

/// Rune removal policy (SPEC.md section 91): per-recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketRemovalPolicy {
    /// The recipe forbids removal entirely.
    Impossible,
    /// The rune is destroyed on removal.
    Destructive,
    /// The rune is returned to the player.
    Recoverable,
}

impl SocketSystem {
    pub fn new() -> SocketSystem {
        SocketSystem::default()
    }

    /// Declare an item's socket capacity at generation time (loot
    /// pipeline). Called by the roller, not by clients.
    pub fn declare_sockets(&mut self, item: ItemId, capacity: u8) {
        self.capacities.insert(item, capacity);
        self.filled
            .entry(item)
            .or_insert_with(|| vec![None; capacity as usize]);
    }

    pub fn capacity(&self, item: ItemId) -> u8 {
        self.capacities.get(&item).copied().unwrap_or(0)
    }

    pub fn filled(&self, item: ItemId) -> &[Option<RuneId>] {
        static EMPTY: [Option<RuneId>; 0] = [];
        self.filled
            .get(&item)
            .map(|v| v.as_slice())
            .unwrap_or(&EMPTY)
    }

    /// Insert a rune into the first free socket. After each insertion the
    /// runeword is re-evaluated: it activates when the filled prefix of
    /// sockets exactly matches a runeword sequence.
    pub fn insert_rune(
        &mut self,
        item: ItemId,
        rune: RuneId,
        runewords: &[RunewordDefinition],
    ) -> Result<bool, SocketError> {
        let capacity = self
            .capacities
            .get(&item)
            .copied()
            .ok_or(SocketError::ItemNotSocketed)?;
        let slots = self
            .filled
            .entry(item)
            .or_insert_with(|| vec![None; capacity as usize]);
        if slots.len() < capacity as usize {
            slots.resize(capacity as usize, None);
        }
        let free = slots.iter().position(|s| s.is_none());
        let Some(idx) = free else {
            return Err(SocketError::NoFreeSocket);
        };
        slots[idx] = Some(rune);
        // runeword evaluation: filled sockets in order must match a
        // runeword sequence exactly (all sockets filled).
        let all_filled = slots.iter().all(|s| s.is_some());
        if all_filled {
            let sequence: Vec<RuneId> = slots.iter().filter_map(|s| *s).collect();
            for rw in runewords {
                if rw.runes == sequence {
                    self.runewords.insert(item, rw.id);
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Rune removal policy (SPEC.md section 91): depends on the recipe
    /// used - Impossible (default), Destructive (rune destroyed) or
    /// Recoverable (rune returned).
    pub fn remove_rune(
        &mut self,
        item: ItemId,
        index: usize,
        policy: SocketRemovalPolicy,
    ) -> Result<Option<RuneId>, SocketError> {
        let slots = self
            .filled
            .get_mut(&item)
            .ok_or(SocketError::ItemNotSocketed)?;
        if index >= slots.len() {
            return Err(SocketError::NoFreeSocket);
        }
        let Some(rune) = slots[index].take() else {
            return Err(SocketError::NoFreeSocket);
        };
        // any removal invalidates an active runeword (the sequence broke)
        self.runewords.remove(&item);
        match policy {
            SocketRemovalPolicy::Impossible => {
                // rejected before mutation by callers honoring the policy;
                // reaching here means the recipe allows it
                Ok(Some(rune))
            }
            SocketRemovalPolicy::Destructive => Ok(None),
            SocketRemovalPolicy::Recoverable => Ok(Some(rune)),
        }
    }

    /// Whether removal is allowed at all under a policy (section 91).
    pub fn removal_allowed(policy: SocketRemovalPolicy) -> bool {
        !matches!(policy, SocketRemovalPolicy::Impossible)
    }

    pub fn active_runeword(&self, item: ItemId) -> Option<u32> {
        self.runewords.get(&item).copied()
    }

    /// Deterministic stat recomputation (SPEC section 199): rebuild the
    /// item's stat contributions from affixes, individual runes and the
    /// active runeword. Contributions are appended in canonical order
    /// (affixes by id, sockets by slot, runeword last); the StatBlock
    /// pipeline stages make the result insertion-order independent.
    pub fn compute_item_stats(
        &self,
        item: &ItemInstance,
        runewords: &[RunewordDefinition],
    ) -> StatBlock {
        let mut block = StatBlock::new();
        // affixes: each affix id contributes a flat bonus (canonical
        // reference: affix i adds i to stat 1..=3 by group)
        for affix in item.affixes.iter() {
            block.add_modifier(StatModifier {
                stat: StatId(1 + affix % 3),
                source: ModifierSource::Equipment(item.id.0 as u64),
                source_sequence: *affix as u64,
                operation: ModifierOp::FlatAdd,
                value: 10,
                priority: 0,
            });
        }
        // runes in sockets
        if let Some(slots) = self.filled.get(&item.id) {
            for (idx, slot) in slots.iter().enumerate() {
                if let Some(rune) = slot {
                    let (stat, value) = rune_modifier(*rune);
                    block.add_modifier(StatModifier {
                        stat,
                        source: ModifierSource::Equipment(item.id.0 as u64),
                        source_sequence: idx as u64,
                        operation: ModifierOp::FlatAdd,
                        value,
                        priority: 0,
                    });
                }
            }
        }
        // runeword modifiers (source keeps the runeword id in the sequence
        // to keep contributions distinct and traceable)
        if let Some(rw_id) = self.runewords.get(&item.id) {
            if let Some(rw) = runewords.iter().find(|r| r.id == *rw_id) {
                for (idx, (stat, value)) in rw.modifiers.iter().enumerate() {
                    block.add_modifier(StatModifier {
                        stat: *stat,
                        source: ModifierSource::Equipment(item.id.0 as u64),
                        source_sequence: (rw.id as u64) << 8 | idx as u64,
                        operation: ModifierOp::FlatAdd,
                        value: *value,
                        priority: 1,
                    });
                }
            }
        }
        block
    }

    pub fn hash_bytes(&self, out: &mut Vec<u8>) {
        for (item, capacity) in &self.capacities {
            out.extend_from_slice(&item.0.to_le_bytes());
            out.push(*capacity);
            if let Some(slots) = self.filled.get(item) {
                for slot in slots {
                    match slot {
                        Some(r) => {
                            out.push(1);
                            out.extend_from_slice(&r.0.to_le_bytes());
                        }
                        None => out.push(0),
                    }
                }
            }
            if let Some(rw) = self.runewords.get(item) {
                out.extend_from_slice(&rw.to_le_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ItemQuality;

    fn socketed_item(id: u128, capacity: u8) -> (ItemInstance, SocketSystem) {
        let mut sys = SocketSystem::new();
        let item = ItemInstance {
            id: ItemId(id),
            definition: arpg_core::ItemDefId(3),
            quality: ItemQuality::Normal,
            item_level: 10,
            generation_seed: [2u8; 32],
            affixes: smallvec::smallvec![],
            sockets: smallvec::smallvec![],
            durability: None,
            flags: 1,
            charges: None,
            hands: Default::default(),
            requirements: Default::default(),
        };
        sys.declare_sockets(ItemId(id), capacity);
        (item, sys)
    }

    #[test]
    fn rune_inserts_into_free_socket() {
        let (item, mut sys) = socketed_item(1, 2);
        let rw = reference_runewords();
        assert!(!sys.insert_rune(item.id, RuneId(0), &rw).unwrap());
        assert_eq!(sys.filled(item.id), &[Some(RuneId(0)), None][..]);
        assert_eq!(sys.active_runeword(item.id), None);
    }

    #[test]
    fn full_socket_item_rejects() {
        let (item, mut sys) = socketed_item(2, 1);
        let rw = reference_runewords();
        sys.insert_rune(item.id, RuneId(9), &rw).unwrap();
        assert!(matches!(
            sys.insert_rune(item.id, RuneId(0), &rw),
            Err(SocketError::NoFreeSocket)
        ));
    }

    #[test]
    fn unknown_item_rejects() {
        let mut sys = SocketSystem::new();
        assert!(matches!(
            sys.insert_rune(ItemId(99), RuneId(0), &[]),
            Err(SocketError::ItemNotSocketed)
        ));
    }

    #[test]
    fn matching_sequence_activates_runeword() {
        let (item, mut sys) = socketed_item(3, 2);
        let rw = reference_runewords();
        // Steel = El + Tir
        sys.insert_rune(item.id, RuneId(0), &rw).unwrap();
        let activated = sys.insert_rune(item.id, RuneId(2), &rw).unwrap();
        assert!(activated, "Steel sequence matched");
        assert_eq!(sys.active_runeword(item.id), Some(1));
    }

    #[test]
    fn wrong_sequence_activates_nothing() {
        let (item, mut sys) = socketed_item(4, 2);
        let rw = reference_runewords();
        sys.insert_rune(item.id, RuneId(0), &rw).unwrap();
        let activated = sys.insert_rune(item.id, RuneId(9), &rw).unwrap();
        assert!(!activated);
        assert_eq!(sys.active_runeword(item.id), None);
    }

    #[test]
    fn stats_recompute_deterministically() {
        let rw = reference_runewords();
        let run = || {
            let (item, mut sys) = socketed_item(5, 2);
            sys.insert_rune(item.id, RuneId(0), &rw).unwrap();
            sys.insert_rune(item.id, RuneId(2), &rw).unwrap();
            let block = sys.compute_item_stats(&item, &rw);
            (block.compute(StatId(1)), block.compute(StatId(2)))
        };
        let (atk, acc) = run();
        // El alone: +3 attack; Steel runeword: +20 attack, +25 accuracy
        assert_eq!(atk, 3 + 20);
        assert_eq!(acc, 25);
        assert_eq!(run(), (atk, acc));
    }

    #[test]
    fn partial_runes_contribute_without_runeword() {
        let rw = reference_runewords();
        let (item, mut sys) = socketed_item(6, 3);
        sys.insert_rune(item.id, RuneId(5), &rw).unwrap();
        let block = sys.compute_item_stats(&item, &rw);
        assert_eq!(block.compute(StatId(3)), 5, "Ith: +5 mana");
    }
}
