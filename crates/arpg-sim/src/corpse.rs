//! Monster corpses (SPEC.md section 100): dead monsters that leave a
//! body behind. A corpse carries the original monster type, position,
//! owner metadata and a consumed flag; it enables revive, corpse
//! explosion, summoning and corpse consumption.

use arpg_core::{EntityId, MonsterDefId, PlayerId, WorldPos};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CorpseId(pub u64);

/// A monster corpse (SPEC.md section 100).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Corpse {
    pub id: CorpseId,
    /// Original monster type.
    pub monster_def: MonsterDefId,
    pub pos: WorldPos,
    /// Owner metadata: the killer credited with the kill.
    pub killer: Option<PlayerId>,
    /// Once consumed the corpse can no longer fuel skills.
    pub consumed: bool,
    /// Tick the corpse appeared on, for expiry policies.
    pub born_tick: u64,
}

#[derive(Debug, Default)]
pub struct CorpseSystem {
    next_id: u64,
    corpses: BTreeMap<CorpseId, Corpse>,
}

impl CorpseSystem {
    pub fn new() -> CorpseSystem {
        CorpseSystem::default()
    }

    /// Spawn a corpse where a monster died (SPEC.md section 100).
    pub fn spawn(
        &mut self,
        monster_def: MonsterDefId,
        pos: WorldPos,
        killer: Option<PlayerId>,
        born_tick: u64,
    ) -> CorpseId {
        self.next_id += 1;
        let id = CorpseId(self.next_id);
        self.corpses.insert(
            id,
            Corpse {
                id,
                monster_def,
                pos,
                killer,
                consumed: false,
                born_tick,
            },
        );
        id
    }

    /// The nearest unconsumed corpse to a position (SPEC.md section
    /// 100): corpse-targeted skills resolve against it. EntityId-style
    /// tie-break on distance.
    pub fn nearest_unconsumed(&self, pos: WorldPos) -> Option<CorpseId> {
        self.corpses
            .values()
            .filter(|c| !c.consumed)
            .min_by_key(|c| {
                let dx = c.pos.x - pos.x;
                let dy = c.pos.y - pos.y;
                (dx * dx + dy * dy, c.id.0)
            })
            .map(|c| c.id)
    }

    /// Consume a corpse (SPEC.md section 100): returns the corpse if
    /// it was unconsumed, marks it consumed.
    pub fn consume(&mut self, id: CorpseId) -> Option<Corpse> {
        let corpse = self.corpses.get_mut(&id)?;
        if corpse.consumed {
            return None;
        }
        corpse.consumed = true;
        Some(*corpse)
    }

    pub fn get(&self, id: CorpseId) -> Option<&Corpse> {
        self.corpses.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Corpse> {
        self.corpses.values()
    }

    pub fn len(&self) -> usize {
        self.corpses.len()
    }

    pub fn is_empty(&self) -> bool {
        self.corpses.is_empty()
    }

    /// Remove corpses older than `max_age` ticks (cleanup policy).
    pub fn expire(&mut self, tick: u64, max_age: u64) -> Vec<CorpseId> {
        let expired: Vec<CorpseId> = self
            .corpses
            .iter()
            .filter(|(_, c)| tick.saturating_sub(c.born_tick) > max_age)
            .map(|(id, _)| *id)
            .collect();
        for id in &expired {
            self.corpses.remove(id);
        }
        expired
    }

    /// Canonical hash input: corpses are gameplay state.
    pub fn hash_bytes(&self, out: &mut Vec<u8>) {
        for corpse in self.corpses.values() {
            out.extend_from_slice(&corpse.id.0.to_le_bytes());
            out.extend_from_slice(&corpse.monster_def.0.to_le_bytes());
            out.extend_from_slice(&corpse.pos.x.to_le_bytes());
            out.extend_from_slice(&corpse.pos.y.to_le_bytes());
            out.extend_from_slice(&[u8::from(corpse.consumed)]);
            if let Some(k) = corpse.killer {
                out.extend_from_slice(&k.0.to_le_bytes());
            } else {
                out.push(0);
            }
            out.extend_from_slice(&corpse.born_tick.to_le_bytes());
        }
    }
}

/// Who owns a corpse interaction: the killer metadata travels with the
/// corpse for revive attribution (SPEC.md section 100).
pub fn revive_owner(corpse: &Corpse) -> Option<EntityId> {
    corpse.killer.map(|p| EntityId(p.0 as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpses_spawn_consume_and_expire() {
        let mut corpses = CorpseSystem::new();
        let id = corpses.spawn(MonsterDefId(1), WorldPos::new(0, 0), Some(PlayerId(1)), 10);
        assert_eq!(corpses.len(), 1);
        assert!(corpses.nearest_unconsumed(WorldPos::new(5, 5)).is_some());
        // a consumed corpse no longer serves skills
        assert!(corpses.consume(id).is_some());
        assert!(corpses.consume(id).is_none());
        assert!(corpses.nearest_unconsumed(WorldPos::new(0, 0)).is_none());
        // expiry reaps old corpses by age, consumed or not
        corpses.spawn(MonsterDefId(2), WorldPos::new(0, 0), None, 10);
        let expired = corpses.expire(10 + 100, 50);
        assert_eq!(expired.len(), 2);
        assert!(corpses.is_empty());
    }

    #[test]
    fn nearest_ties_break_by_id() {
        let mut corpses = CorpseSystem::new();
        corpses.spawn(MonsterDefId(1), WorldPos::new(256, 0), None, 0);
        corpses.spawn(MonsterDefId(2), WorldPos::new(-256, 0), None, 0);
        let nearest = corpses.nearest_unconsumed(WorldPos::new(0, 0)).unwrap();
        assert_eq!(nearest, CorpseId(1));
    }
}
