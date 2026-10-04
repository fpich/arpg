//! Auras (SPEC.md section 55): an aura is a state producer. The refresh
//! interval, radius, target filter and produced state all belong to the
//! data; no global constant is required.

use arpg_core::{EntityId, Tick, WorldPos};
use std::collections::BTreeMap;

/// Which entities an aura applies to (SPEC.md section 55).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFilter {
    Party,
    Allies,
    Enemies,
    Everyone,
}

/// A datapack aura definition (SPEC.md section 55).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuraDefinition {
    pub id: u32,
    /// Ticks between two refreshes; owned by the data.
    pub refresh_interval: u16,
    /// Radius in fixed-point tiles (256 = 1 tile).
    pub radius_fp: i32,
    pub target_filter: TargetFilter,
    /// State id applied to each target while inside the radius.
    pub state: u32,
    /// Magnitude of the applied state in basis points.
    pub magnitude_bp: i32,
    /// Duration granted on each refresh in ticks.
    pub duration_ticks: u16,
}

/// A currently active aura instance in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuraInstance {
    pub definition: u32,
    pub owner: EntityId,
    /// Last tick the aura produced states.
    pub last_refresh: Tick,
}

#[derive(Debug, Default, Clone)]
pub struct AuraSystem {
    pub definitions: BTreeMap<u32, AuraDefinition>,
    pub active: BTreeMap<EntityId, AuraInstance>,
}

impl AuraSystem {
    pub fn new() -> AuraSystem {
        AuraSystem::default()
    }

    /// Activate an aura on an owner entity.
    pub fn activate(&mut self, owner: EntityId, definition: u32, tick: Tick) {
        self.active.insert(
            owner,
            AuraInstance {
                definition,
                owner,
                last_refresh: tick,
            },
        );
    }

    pub fn deactivate(&mut self, owner: EntityId) {
        self.active.remove(&owner);
    }

    /// Canonical hash input: active auras in owner order. Definitions are
    /// datapack content, hashed via the content hash, not per-tick.
    pub fn hash_bytes(&self, buf: &mut Vec<u8>) {
        for (owner, a) in &self.active {
            buf.extend_from_slice(&owner.0.to_le_bytes());
            buf.extend_from_slice(&a.definition.to_le_bytes());
            buf.extend_from_slice(&a.last_refresh.0.to_le_bytes());
        }
    }

    /// Auras due for a refresh this tick (deterministic owner order).
    pub fn due(&self, tick: Tick) -> Vec<(EntityId, &AuraInstance, &AuraDefinition)> {
        self.active
            .iter()
            .filter(|(_, a)| {
                let def = self.definitions.get(&a.definition);
                def.is_some_and(|d| {
                    let interval = d.refresh_interval.max(1) as u64;
                    tick.0.saturating_sub(a.last_refresh.0) >= interval
                })
            })
            .filter_map(|(e, a)| self.definitions.get(&a.definition).map(|d| (*e, a, d)))
            .collect()
    }
}

/// True if `target` is inside the aura radius of `origin`.
pub fn within_radius(origin: WorldPos, target: WorldPos, radius_fp: i32) -> bool {
    let dx = target.x - origin.x;
    let dy = target.y - origin.y;
    dx * dx + dy * dy <= radius_fp * radius_fp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_respects_refresh_interval() {
        let mut sys = AuraSystem::new();
        sys.definitions.insert(
            7,
            AuraDefinition {
                id: 7,
                refresh_interval: 5,
                radius_fp: 3 * 256,
                target_filter: TargetFilter::Party,
                state: 20,
                magnitude_bp: 1000,
                duration_ticks: 8,
            },
        );
        sys.activate(EntityId(1), 7, Tick(0));
        assert_eq!(sys.due(Tick(4)).len(), 0);
        assert_eq!(sys.due(Tick(5)).len(), 1);
    }

    #[test]
    fn radius_check_is_fp() {
        let origin = WorldPos::new(0, 0);
        assert!(within_radius(origin, WorldPos::new(256, 0), 256));
        assert!(within_radius(origin, WorldPos::new(181, 181), 256));
        assert!(!within_radius(origin, WorldPos::new(182, 182), 256));
        assert!(!within_radius(origin, WorldPos::new(257, 0), 256));
    }
}
