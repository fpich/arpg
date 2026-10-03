//! Summons and hirelings (SPEC.md sections 66-67). A summon has an owner,
//! controller, family, slot index and lifetime, with a configurable
//! per-family replacement policy. Hirelings additionally have experience,
//! persistent equipment and a death/revive state; they persist with the
//! character snapshot.

use arpg_core::{EntityId, PlayerId};

/// Summon family (SPEC.md section 66): limits and replacement policy are
/// configured per family by the ruleset/datapack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SummonFamily(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacementPolicy {
    Reject,
    ReplaceOldest,
    ReplaceWeakest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummonLimits {
    pub max_active: u8,
    pub policy: ReplacementPolicy,
}

impl SummonLimits {
    pub const fn new(max_active: u8, policy: ReplacementPolicy) -> SummonLimits {
        SummonLimits { max_active, policy }
    }
}

/// A summon slot (SPEC.md section 66).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summon {
    pub entity: EntityId,
    pub owner: PlayerId,
    pub controller: PlayerId,
    pub family: SummonFamily,
    pub slot_index: u8,
    /// Remaining lifetime in ticks; None = permanent.
    pub lifetime_ticks: Option<u32>,
}

/// Hireling data (SPEC.md section 67): persists with the character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirelingState {
    pub owner: PlayerId,
    pub experience: u64,
    /// Death state: dead hirelings must be revived (revive cost).
    pub dead: bool,
    pub revive_cost: u64,
}

#[derive(Debug)]
pub enum SummonError {
    LimitReached,
    NoSuchSummon,
}

/// Owner-side summon registry. Slots are canonical (BTreeMap keyed by
/// (owner, family, slot)); the registry is hashable in canonical order.
#[derive(Debug, Default)]
pub struct SummonSystem {
    next_slot: u8,
    summons: std::collections::BTreeMap<(PlayerId, SummonFamily, u8), Summon>,
    hirelings: std::collections::BTreeMap<PlayerId, HirelingState>,
}

impl SummonSystem {
    pub fn new() -> SummonSystem {
        SummonSystem::default()
    }

    pub fn active_of(&self, owner: PlayerId, family: SummonFamily) -> usize {
        self.summons
            .range((owner, family, u8::MIN)..(owner, family, u8::MAX))
            .count()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Summon> {
        self.summons.values()
    }

    /// Summon with the family's replacement policy applied. Returns the
    /// evicted entity when ReplaceOldest/ReplaceWeakest fires (the caller
    /// removes it from the world).
    pub fn summon(
        &mut self,
        owner: PlayerId,
        family: SummonFamily,
        limits: &SummonLimits,
        spawn: impl FnOnce(PlayerId, u8) -> EntityId,
        lifetime_ticks: Option<u32>,
    ) -> Result<(EntityId, Option<EntityId>), SummonError> {
        let active = self.active_of(owner, family) as u8;
        let (slot, evicted) = if active < limits.max_active {
            let slot = self.next_free_slot(owner, family);
            (slot, None)
        } else {
            match limits.policy {
                ReplacementPolicy::Reject => return Err(SummonError::LimitReached),
                ReplacementPolicy::ReplaceOldest => {
                    let key = self
                        .summons
                        .range((owner, family, u8::MIN)..(owner, family, u8::MAX))
                        .next()
                        .map(|(k, _)| *k);
                    match key {
                        Some(k) => {
                            let old = self.summons.remove(&k).unwrap().entity;
                            (k.2, Some(old))
                        }
                        None => (self.next_free_slot(owner, family), None),
                    }
                }
                ReplacementPolicy::ReplaceWeakest => {
                    // canonical weakest = highest slot index (registry order
                    // is stable; a real datapack keys by stats)
                    let key = self
                        .summons
                        .range((owner, family, u8::MIN)..(owner, family, u8::MAX))
                        .next_back()
                        .map(|(k, _)| *k);
                    match key {
                        Some(k) => {
                            let old = self.summons.remove(&k).unwrap().entity;
                            (k.2, Some(old))
                        }
                        None => (self.next_free_slot(owner, family), None),
                    }
                }
            }
        };
        let controller = owner;
        let entity = spawn(owner, slot);
        self.summons.insert(
            (owner, family, slot),
            Summon {
                entity,
                owner,
                controller,
                family,
                slot_index: slot,
                lifetime_ticks,
            },
        );
        self.next_slot = self.next_slot.wrapping_add(1);
        Ok((entity, evicted))
    }

    fn next_free_slot(&self, owner: PlayerId, family: SummonFamily) -> u8 {
        for slot in 0..=u8::MAX {
            if !self.summons.contains_key(&(owner, family, slot)) {
                return slot;
            }
        }
        0
    }

    /// Expire summons whose lifetime reached zero; returns expired entities
    /// so the caller removes them from the world deterministically.
    pub fn tick_expire(&mut self) -> Vec<EntityId> {
        let mut expired = Vec::new();
        let mut to_remove = Vec::new();
        for (key, s) in self.summons.iter_mut() {
            if let Some(lt) = s.lifetime_ticks {
                if let Some(next) = lt.checked_sub(1) {
                    s.lifetime_ticks = Some(next);
                } else {
                    expired.push(s.entity);
                    to_remove.push(*key);
                }
            }
        }
        for key in to_remove {
            self.summons.remove(&key);
        }
        expired
    }

    pub fn despawn_family(&mut self, owner: PlayerId, family: SummonFamily) -> Vec<EntityId> {
        let keys: Vec<_> = self
            .summons
            .range((owner, family, u8::MIN)..(owner, family, u8::MAX))
            .map(|(k, _)| *k)
            .collect();
        keys.iter()
            .map(|k| self.summons.remove(k).unwrap().entity)
            .collect()
    }

    /// Owner leaves the game: their summons are removed (SPEC.md section
    /// 3977 "summon owner leaves game").
    pub fn owner_left(&mut self, owner: PlayerId) -> Vec<EntityId> {
        let keys: Vec<_> = self
            .summons
            .range((owner, SummonFamily(0), u8::MIN)..(owner, SummonFamily(u32::MAX), u8::MAX))
            .filter(|(_, s)| s.owner == owner)
            .map(|(k, _)| *k)
            .collect();
        let mut out = Vec::new();
        for k in keys {
            if self.summons.get(&k).is_some_and(|s| s.owner == owner) {
                out.push(self.summons.remove(&k).unwrap().entity);
            }
        }
        out
    }

    /// Hireling management (SPEC.md section 67).
    pub fn hire_hireling(&mut self, owner: PlayerId, revive_cost: u64) {
        self.hirelings.entry(owner).or_insert(HirelingState {
            owner,
            experience: 0,
            dead: false,
            revive_cost,
        });
    }

    pub fn hireling_of(&self, owner: PlayerId) -> Option<&HirelingState> {
        self.hirelings.get(&owner)
    }

    pub fn grant_hireling_xp(&mut self, owner: PlayerId, xp: u64) {
        if let Some(h) = self.hirelings.get_mut(&owner) {
            h.experience = h.experience.saturating_add(xp);
        }
    }

    pub fn kill_hireling(&mut self, owner: PlayerId) {
        if let Some(h) = self.hirelings.get_mut(&owner) {
            h.dead = true;
        }
    }

    /// Revive: caller checks gold payment of `revive_cost` first; the
    /// economy side is not duplicated here.
    pub fn revive_hireling(&mut self, owner: PlayerId) -> Result<(), SummonError> {
        let h = self
            .hirelings
            .get_mut(&owner)
            .ok_or(SummonError::NoSuchSummon)?;
        h.dead = false;
        Ok(())
    }

    pub fn hash_bytes(&self, out: &mut Vec<u8>) {
        for (key, s) in &self.summons {
            out.extend_from_slice(&key.0 .0.to_le_bytes());
            out.extend_from_slice(&key.1 .0.to_le_bytes());
            out.extend_from_slice(&[key.2]);
            out.extend_from_slice(&s.entity.0.to_le_bytes());
            if let Some(lt) = s.lifetime_ticks {
                out.extend_from_slice(&lt.to_le_bytes());
            }
        }
        for (owner, h) in &self.hirelings {
            out.extend_from_slice(&owner.0.to_le_bytes());
            out.extend_from_slice(&h.experience.to_le_bytes());
            out.push(u8::from(h.dead));
            out.extend_from_slice(&h.revive_cost.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_entity(owner: PlayerId, _slot: u8) -> EntityId {
        EntityId(owner.0 as u64 * 1000 + 7)
    }

    #[test]
    fn summon_within_limit() {
        let mut sys = SummonSystem::new();
        let limits = SummonLimits::new(3, ReplacementPolicy::Reject);
        let (e, evicted) = sys
            .summon(
                PlayerId(1),
                SummonFamily(1),
                &limits,
                spawn_entity,
                Some(100),
            )
            .unwrap();
        assert!(evicted.is_none());
        assert_eq!(sys.active_of(PlayerId(1), SummonFamily(1)), 1);
        assert_eq!(e, EntityId(1007));
    }

    #[test]
    fn reject_policy_at_limit() {
        let mut sys = SummonSystem::new();
        let limits = SummonLimits::new(1, ReplacementPolicy::Reject);
        sys.summon(PlayerId(1), SummonFamily(1), &limits, spawn_entity, None)
            .unwrap();
        assert!(matches!(
            sys.summon(PlayerId(1), SummonFamily(1), &limits, spawn_entity, None),
            Err(SummonError::LimitReached)
        ));
    }

    #[test]
    fn replace_oldest_evicts() {
        let mut sys = SummonSystem::new();
        let limits = SummonLimits::new(2, ReplacementPolicy::ReplaceOldest);
        sys.summon(PlayerId(1), SummonFamily(1), &limits, spawn_entity, None)
            .unwrap();
        sys.summon(PlayerId(1), SummonFamily(1), &limits, spawn_entity, None)
            .unwrap();
        let (_, evicted) = sys
            .summon(PlayerId(1), SummonFamily(1), &limits, spawn_entity, None)
            .unwrap();
        assert!(evicted.is_some());
        assert_eq!(sys.active_of(PlayerId(1), SummonFamily(1)), 2);
    }

    #[test]
    fn lifetime_expiry_removes_summon() {
        let mut sys = SummonSystem::new();
        let limits = SummonLimits::new(3, ReplacementPolicy::Reject);
        sys.summon(PlayerId(1), SummonFamily(1), &limits, spawn_entity, Some(2))
            .unwrap();
        assert!(sys.tick_expire().is_empty());
        assert!(sys.tick_expire().is_empty());
        let expired = sys.tick_expire();
        assert_eq!(expired.len(), 1);
        assert_eq!(sys.active_of(PlayerId(1), SummonFamily(1)), 0);
    }

    #[test]
    fn owner_departure_removes_their_summons() {
        let mut sys = SummonSystem::new();
        let limits = SummonLimits::new(3, ReplacementPolicy::Reject);
        sys.summon(PlayerId(1), SummonFamily(1), &limits, spawn_entity, None)
            .unwrap();
        sys.summon(PlayerId(2), SummonFamily(1), &limits, spawn_entity, None)
            .unwrap();
        let removed = sys.owner_left(PlayerId(1));
        assert_eq!(removed.len(), 1);
        assert_eq!(sys.active_of(PlayerId(1), SummonFamily(1)), 0);
        assert_eq!(sys.active_of(PlayerId(2), SummonFamily(1)), 1);
    }

    #[test]
    fn hireling_lifecycle() {
        let mut sys = SummonSystem::new();
        sys.hire_hireling(PlayerId(1), 500);
        sys.grant_hireling_xp(PlayerId(1), 250);
        assert_eq!(sys.hireling_of(PlayerId(1)).unwrap().experience, 250);
        sys.kill_hireling(PlayerId(1));
        assert!(sys.hireling_of(PlayerId(1)).unwrap().dead);
        sys.revive_hireling(PlayerId(1)).unwrap();
        assert!(!sys.hireling_of(PlayerId(1)).unwrap().dead);
    }
}
