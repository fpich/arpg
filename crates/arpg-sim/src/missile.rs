use arpg_core::{EntityId, FixedVec2, SkillId, WorldPos};

/// Missile movement kinds (SPEC.md section 57).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissileMovement {
    Linear,
    Homing,
    Accelerating,
    Stationary,
}

/// A missile in flight (SPEC.md section 57).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissileInstance {
    pub entity: EntityId,
    pub definition: u32,
    pub owner: EntityId,
    pub source_skill: Option<SkillId>,
    pub position: WorldPos,
    pub velocity: FixedVec2,
    pub lifetime: u16,
    pub movement: MissileMovement,
    /// Entities already hit; a same entity cannot be hit twice unless the
    /// definition allows it (SPEC.md section 58).
    pub hit_entities: Vec<EntityId>,
    pub remaining_pierces: u32,
}

impl MissileInstance {
    /// One tick of movement. Returns true when the missile expired.
    pub fn advance(&mut self) -> bool {
        if self.lifetime == 0 {
            return true;
        }
        self.lifetime -= 1;
        self.position = WorldPos::new(
            self.position.x.saturating_add(self.velocity.x),
            self.position.y.saturating_add(self.velocity.y),
        );
        self.lifetime == 0
    }

    /// Register a hit; honors piercing (SPEC.md section 58). Returns true
    /// when the missile should be destroyed by this hit.
    pub fn register_hit(&mut self, target: EntityId) -> bool {
        if self.hit_entities.contains(&target) {
            return false;
        }
        self.hit_entities.push(target);
        if self.remaining_pierces == 0 {
            true
        } else {
            self.remaining_pierces -= 1;
            false
        }
    }

    /// Whether the missile overlaps a position at fixed-point radius.
    pub fn hits_at(&self, pos: WorldPos, radius: i64) -> bool {
        self.position.dist2(pos) <= radius * radius
    }
}
