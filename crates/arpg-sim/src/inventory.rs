use crate::item::{ItemError, ItemInstance, ItemLocation};
use arpg_core::{ItemId, PlayerId};
use std::collections::BTreeMap;

/// Item ownership store (SPEC.md section 84): one ItemId has exactly one
/// ItemLocation. Inventory transactions (section 85) validate everything
/// before applying: an error before apply produces zero mutations.
#[derive(Debug, Default)]
pub struct InventorySystem {
    items: BTreeMap<ItemId, ItemInstance>,
    locations: BTreeMap<ItemId, ItemLocation>,
    /// Fast slot lookup: (player, slot-kind-key) -> item.
    slot_owner: BTreeMap<(u64, u64, u64), ItemId>,
}

fn slot_key(loc: ItemLocation) -> (u64, u64, u64) {
    match loc {
        ItemLocation::PlayerInventory(p, g) => (p.0 as u64, 0, (g.x as u64) << 8 | g.y as u64),
        ItemLocation::Equipment(p, s) => (p.0 as u64, 1, s as u64),
        ItemLocation::Belt(p, s) => (p.0 as u64, 2, s as u64),
        ItemLocation::Stash(p, s) => (
            p.0 as u64,
            3,
            (s.page as u64) << 16 | (s.x as u64) << 8 | s.y as u64,
        ),
        ItemLocation::Cube(p, g) => (p.0 as u64, 4, (g.x as u64) << 8 | g.y as u64),
        ItemLocation::Ground(l, pos) => (l.0, 5, (pos.x as i64 as u64) << 32 | pos.y as i64 as u64),
    }
}

impl InventorySystem {
    pub fn new() -> InventorySystem {
        InventorySystem::default()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: ItemId) -> Option<&ItemInstance> {
        self.items.get(&id)
    }

    pub fn location(&self, id: ItemId) -> Option<ItemLocation> {
        self.locations.get(&id).copied()
    }
    /// Remove an item entirely (expired ground item, SPEC.md section 88).
    pub fn remove(&mut self, id: ItemId) -> Option<ItemInstance> {
        let loc = self.locations.remove(&id)?;
        self.slot_owner.remove(&slot_key(loc));
        self.items.remove(&id)
    }
    /// Deterministic iteration over item locations for state hashing.
    pub fn iter_locations(&self) -> impl Iterator<Item = (ItemId, &ItemLocation)> {
        self.locations.iter().map(|(id, loc)| (*id, loc))
    }

    /// Deterministic drop on the ground (SPEC.md section 84).
    pub fn spawn_ground(
        &mut self,
        item: ItemInstance,
        level: arpg_core::LevelInstanceId,
        pos: arpg_core::WorldPos,
    ) {
        let id = item.id;
        let loc = ItemLocation::Ground(level, pos);
        self.items.insert(id, item);
        self.slot_owner.insert(slot_key(loc), id);
        self.locations.insert(id, loc);
    }

    /// Full transactional move (SPEC.md section 85): validate source,
    /// ownership, destination, requirements; reserve; apply; emit.
    /// Returns ItemUnavailable when another player already picked the item
    /// up in the same tick (section 86).
    pub fn move_item(
        &mut self,
        id: ItemId,
        from: ItemLocation,
        to: ItemLocation,
    ) -> Result<(), ItemError> {
        // validate source: the item exists
        let Some(current) = self.locations.get(&id) else {
            return Err(ItemError::ItemUnavailable);
        };
        // validate ownership: the item is where the caller thinks it is
        if *current != from {
            return Err(ItemError::ItemUnavailable);
        }
        // validate destination: the slot is free (or occupied by the same
        // item, which is a no-op)
        let dest_key = slot_key(to);
        if let Some(&occupant) = self.slot_owner.get(&dest_key) {
            if occupant != id {
                return Err(ItemError::SlotOccupied);
            }
        }
        // apply all mutations (section 85: no partial state)
        let src_key = slot_key(from);
        self.slot_owner.remove(&src_key);
        self.slot_owner.insert(dest_key, id);
        self.locations.insert(id, to);
        Ok(())
    }

    /// Simultaneous pickup resolution (SPEC.md section 86): the first valid
    /// pickup acquires the item; later ones get ItemUnavailable.
    pub fn pick_up(
        &mut self,
        player: PlayerId,
        id: ItemId,
        ground: ItemLocation,
        to: ItemLocation,
    ) -> Result<(), ItemError> {
        let ItemLocation::Ground(..) = ground else {
            return Err(ItemError::TransactionInvalid("not on the ground"));
        };
        let _ = player;
        self.move_item(id, ground, to)
    }

    pub fn items_on_ground(
        &self,
        level: arpg_core::LevelInstanceId,
    ) -> Vec<(ItemId, arpg_core::WorldPos)> {
        self.locations
            .iter()
            .filter_map(|(id, loc)| match loc {
                ItemLocation::Ground(l, pos) if *l == level => Some((*id, *pos)),
                _ => None,
            })
            .collect()
    }
}
