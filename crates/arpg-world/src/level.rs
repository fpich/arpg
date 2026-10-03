use crate::generation::RoomInstance;
use arpg_core::{CollisionMap, LevelDefId, LevelInstanceId};

/// A generated, immutable-during-game level instance (SPEC.md section 28).
#[derive(Debug, Clone)]
pub struct LevelInstance {
    pub id: LevelInstanceId,
    pub definition: LevelDefId,
    pub collision: CollisionMap,
    pub rooms: Vec<RoomInstance>,
}

impl LevelInstance {
    pub fn walkable_at(&self, pos: arpg_core::WorldPos) -> bool {
        self.collision.walkable_at(pos)
    }
}
