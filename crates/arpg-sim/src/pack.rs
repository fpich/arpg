//! Monster pack registry (SPEC.md section 64): the engine-side pack
//! state. Pack composition and formation live here; the AI crate's
//! MonsterPack/ChampionProfile describe behaviour policy for the brain.

use arpg_core::EntityId;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackId(pub u64);

/// Engine-side pack record (SPEC.md section 64): leader, members and
/// the aggro-linkage policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pack {
    pub id: PackId,
    pub leader: EntityId,
    pub members: BTreeSet<EntityId>,
    /// Aggro linkage: attacking one member aggros the whole pack.
    pub aggro_linked: bool,
}

/// Pack registry: canonical BTree collections so the state hash is
/// order-independent.
#[derive(Debug, Default)]
pub struct PackSystem {
    next_pack_id: u64,
    packs: BTreeMap<PackId, Pack>,
    membership: BTreeMap<EntityId, PackId>,
}

impl PackSystem {
    pub fn new() -> PackSystem {
        PackSystem::default()
    }

    /// Register a pack (SPEC.md section 64): the leader is a member.
    pub fn register(
        &mut self,
        leader: EntityId,
        members: &[EntityId],
        aggro_linked: bool,
    ) -> PackId {
        self.next_pack_id += 1;
        let id = PackId(self.next_pack_id);
        let mut set = BTreeSet::new();
        set.insert(leader);
        set.extend(members.iter().copied());
        self.membership.insert(leader, id);
        for m in members {
            self.membership.insert(*m, id);
        }
        self.packs.insert(
            id,
            Pack {
                id,
                leader,
                members: set,
                aggro_linked,
            },
        );
        id
    }

    /// The pack an entity belongs to, if any.
    pub fn pack_of(&self, entity: EntityId) -> Option<PackId> {
        self.membership.get(&entity).copied()
    }

    /// Aggro linkage (SPEC.md section 64): members of a linked pack
    /// share aggro; attacking one returns all the others.
    pub fn linked_members(&self, entity: EntityId) -> Vec<EntityId> {
        let Some(id) = self.pack_of(entity) else {
            return Vec::new();
        };
        let Some(pack) = self.packs.get(&id) else {
            return Vec::new();
        };
        if !pack.aggro_linked {
            return Vec::new();
        }
        pack.members.iter().copied().collect()
    }

    /// Remove a dead entity from its pack.
    pub fn remove_member(&mut self, entity: EntityId) {
        let Some(id) = self.membership.remove(&entity) else {
            return;
        };
        if let Some(pack) = self.packs.get_mut(&id) {
            pack.members.remove(&entity);
        }
    }

    pub fn packs(&self) -> impl Iterator<Item = &Pack> {
        self.packs.values()
    }

    /// Canonical hash input: pack membership is gameplay state.
    pub fn hash_bytes(&self, out: &mut Vec<u8>) {
        for (id, pack) in &self.packs {
            out.extend_from_slice(&id.0.to_le_bytes());
            out.extend_from_slice(&pack.leader.0.to_le_bytes());
            out.extend_from_slice(&[u8::from(pack.aggro_linked)]);
            for m in &pack.members {
                out.extend_from_slice(&m.0.to_le_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_pack_members_share_aggro() {
        let mut packs = PackSystem::new();
        let leader = EntityId(1);
        let members = [EntityId(2), EntityId(3)];
        packs.register(leader, &members, true);
        let linked = packs.linked_members(EntityId(2));
        assert!(linked.contains(&leader));
        assert!(linked.contains(&EntityId(3)));
    }

    #[test]
    fn unlinked_packs_do_not_share_aggro() {
        let mut packs = PackSystem::new();
        packs.register(EntityId(1), &[EntityId(2)], false);
        assert!(packs.linked_members(EntityId(1)).is_empty());
    }

    #[test]
    fn removing_a_member_updates_linkage() {
        let mut packs = PackSystem::new();
        packs.register(EntityId(1), &[EntityId(2), EntityId(3)], true);
        packs.remove_member(EntityId(2));
        let linked = packs.linked_members(EntityId(3));
        assert!(!linked.contains(&EntityId(2)));
    }
}
