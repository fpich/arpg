//! Character snapshots for save/load and reconnect (SPEC.md sections
//! 119-120): a snapshot captures the persistent fraction of a character.
//! The cache never serializes into a replay or save (section 1220 note).

use crate::Revisions;
use arpg_core::{ClassId, ItemId, PlayerId};
use std::collections::BTreeMap;

pub const SAVE_SCHEMA_VERSION: u32 = 1;

/// A persistent item location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistentItemLocation {
    Inventory { x: u8, y: u8 },
    Equipment { slot: u8 },
    Belt { slot: u8 },
    Stash { page: u8, x: u8, y: u8 },
    Cube { x: u8, y: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistentItem {
    pub id: ItemId,
    pub definition: u32,
    pub location: PersistentItemLocation,
}

/// Quest progress: quest -> status.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PersistentQuestState {
    pub quests: BTreeMap<u32, u8>,
}

/// Unlocked waypoints: difficulty -> waypoint ids.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PersistentWaypoints {
    pub unlocked: BTreeMap<u32, Vec<u32>>,
}

/// Hireling snapshot (SPEC.md sections 67, 120): hirelings persist with
/// the character, including experience, death state and persistent
/// equipment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirelingSnapshot {
    pub experience: u64,
    pub dead: bool,
    pub revive_cost: u64,
    /// Persistent hireling equipment (one item per slot).
    pub equipment: Vec<PersistentItem>,
}

/// Character snapshot (SPEC.md section 120).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterSnapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub class: ClassId,
    pub level: u16,
    pub experience: u64,
    pub items: Vec<PersistentItem>,
    pub carried_gold: u64,
    pub stash_gold: u64,
    pub quests: PersistentQuestState,
    pub waypoints: PersistentWaypoints,
    pub hireling: Option<HirelingSnapshot>,
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("schema {0} not supported, expected {1}")]
    UnsupportedSchema(u32, u32),
}

/// Repository over snapshots with the same revision model as the SQL
/// backend (section 121). `save` bumps the revision.
#[derive(Debug, Default)]
pub struct CharacterRepository {
    snapshots: BTreeMap<PlayerId, CharacterSnapshot>,
    revisions: Revisions,
}

impl CharacterRepository {
    pub fn new() -> CharacterRepository {
        CharacterRepository::default()
    }

    pub fn revision(&self, player: PlayerId) -> u64 {
        self.revisions.get(player).0
    }

    pub fn save(&mut self, player: PlayerId, mut snapshot: CharacterSnapshot) -> u64 {
        let rev = self.revisions.bump(player).0;
        snapshot.revision = rev;
        snapshot.schema_version = SAVE_SCHEMA_VERSION;
        self.snapshots.insert(player, snapshot);
        rev
    }

    /// Load checks the schema version before returning (section 157:
    /// SaveSchemaVersion separate from other versions).
    pub fn load(&self, player: PlayerId) -> Result<Option<&CharacterSnapshot>, SnapshotError> {
        match self.snapshots.get(&player) {
            None => Ok(None),
            Some(s) if s.schema_version == SAVE_SCHEMA_VERSION => Ok(Some(s)),
            Some(s) => Err(SnapshotError::UnsupportedSchema(
                s.schema_version,
                SAVE_SCHEMA_VERSION,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> CharacterSnapshot {
        CharacterSnapshot {
            schema_version: SAVE_SCHEMA_VERSION,
            revision: 0,
            class: ClassId(0),
            level: 12,
            experience: 3400,
            items: vec![PersistentItem {
                id: ItemId(1),
                definition: 7,
                location: PersistentItemLocation::Inventory { x: 0, y: 0 },
            }],
            carried_gold: 500,
            stash_gold: 1200,
            quests: PersistentQuestState {
                quests: BTreeMap::from([(1u32, 2u8)]),
            },
            waypoints: PersistentWaypoints {
                unlocked: BTreeMap::from([(0u32, vec![1, 2, 3])]),
            },
            hireling: Some(HirelingSnapshot {
                experience: 950,
                dead: false,
                revive_cost: 750,
                equipment: vec![PersistentItem {
                    id: ItemId(9),
                    definition: 12,
                    location: PersistentItemLocation::Equipment { slot: 0 },
                }],
            }),
        }
    }

    #[test]
    fn save_bumps_revision_and_load_returns_snapshot() {
        let mut repo = CharacterRepository::new();
        let p = PlayerId(1);
        assert!(repo.load(p).unwrap().is_none());
        let rev = repo.save(p, snapshot());
        assert_eq!(rev, 1);
        let rev2 = repo.save(p, snapshot());
        assert_eq!(rev2, 2, "revision strictly increases");
        let loaded = repo.load(p).unwrap().unwrap();
        assert_eq!(loaded.revision, 2);
        assert_eq!(loaded.level, 12);
        assert_eq!(loaded.carried_gold, 500);
        let hireling = loaded.hireling.as_ref().unwrap();
        assert_eq!(hireling.experience, 950);
        assert_eq!(hireling.equipment.len(), 1);
    }

    #[test]
    fn hireling_is_optional_and_round_trips() {
        let mut repo = CharacterRepository::new();
        let p = PlayerId(2);
        let mut s = snapshot();
        s.hireling = None;
        repo.save(p, s.clone());
        let loaded = repo.load(p).unwrap().unwrap();
        assert!(loaded.hireling.is_none());
        let mut s = snapshot();
        s.hireling.as_mut().unwrap().dead = true;
        repo.save(p, s);
        assert!(
            repo.load(p)
                .unwrap()
                .unwrap()
                .hireling
                .clone()
                .unwrap()
                .dead
        );
    }

    #[test]
    fn unsupported_schema_is_rejected_on_load() {
        let mut repo = CharacterRepository::new();
        let p = PlayerId(1);
        let mut s = snapshot();
        repo.save(p, s.clone());
        s.schema_version = 99;
        repo.snapshots.insert(p, s);
        let err = repo.load(p).unwrap_err();
        assert!(matches!(err, SnapshotError::UnsupportedSchema(99, 1)));
    }
}
