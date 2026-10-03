use crate::id::{ObjectDefId, ObjectId};
use crate::world::WorldPos;

/// Interactive world object kinds (SPEC.md section 98).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InteractableKind {
    Chest,
    Barrel,
    Urn,
    Door,
    Shrine,
    Well,
    Waypoint,
    QuestObject,
    GenericInteractable,
}

/// State machine of an interactive object (SPEC.md section 98).
/// Containers open, doors open and close, shrines recharge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectInstanceState {
    Default,
    InUse,
    OnRecharge,
    Destroyed,
}

/// Maximum interaction distance in world units (SPEC.md section 98).
pub const INTERACTION_DISTANCE: i64 = 3 * 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectInstance {
    pub id: ObjectId,
    pub definition: ObjectDefId,
    pub kind: InteractableKind,
    pub pos: WorldPos,
    pub state: ObjectInstanceState,
    /// Tick at which the object may be interacted with again (cooldown).
    pub cooldown_until: crate::tick::Tick,
}

impl ObjectInstance {
    pub fn new(
        id: ObjectId,
        definition: ObjectDefId,
        kind: InteractableKind,
        pos: WorldPos,
        born_tick: crate::tick::Tick,
    ) -> ObjectInstance {
        ObjectInstance {
            id,
            definition,
            kind,
            pos,
            state: ObjectInstanceState::Default,
            cooldown_until: born_tick,
        }
    }

    pub fn can_interact(&self, actor: WorldPos, tick: crate::tick::Tick) -> bool {
        self.state == ObjectInstanceState::Default
            && tick >= self.cooldown_until
            && self.pos.dist2(actor) <= INTERACTION_DISTANCE * INTERACTION_DISTANCE
    }

    /// Duration of the post-interaction cooldown in ticks, by kind.
    pub fn cooldown_ticks(kind: InteractableKind) -> u64 {
        match kind {
            InteractableKind::Shrine | InteractableKind::Well => 250,
            InteractableKind::Door => 10,
            _ => 0,
        }
    }
}
