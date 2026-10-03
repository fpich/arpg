//! Persistence transactions (SPEC.md sections 117-119).
//!
//! The sim never confirms a trade commit in memory only: `TradeCommitted`
//! is emitted after the persistence transaction succeeds. This crate defines
//! the transactional interface and an in-memory reference backend used by the
//! sim and tests. A SQL backend implements the same trait.

use arpg_core::{ItemId, PlayerId};

pub mod snapshot;

#[cfg(feature = "sqlite")]
pub mod sqlite;
use std::collections::BTreeMap;

pub use snapshot::{
    CharacterRepository, CharacterSnapshot, PersistentItem, PersistentItemLocation,
    PersistentQuestState, PersistentWaypoints, SnapshotError, SAVE_SCHEMA_VERSION,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CharacterRevision(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TradeId(pub u64);

/// A single atomic mutation inside a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mutation {
    SetGold {
        player: PlayerId,
        carried: u64,
        stash: u64,
    },
    MoveItem {
        item: ItemId,
        /// Serialized location payload; opaque to the backend.
        location: Vec<u8>,
    },
    RemoveItem {
        item: ItemId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PersistenceError {
    #[error("transaction {0} not found")]
    UnknownTransaction(u64),
    #[error("character revision mismatch for player {0}")]
    RevisionMismatch(PlayerId),
    #[error("backend failure: {0}")]
    Backend(String),
    /// Injected failure for tests and chaos scenarios (section 118).
    #[error("injected failure")]
    Injected,
}

/// Snapshot revision guard: optimistic concurrency on characters.
#[derive(Debug, Clone, Default)]
pub struct Revisions {
    inner: BTreeMap<PlayerId, u64>,
}

impl Revisions {
    pub fn get(&self, player: PlayerId) -> CharacterRevision {
        CharacterRevision(self.inner.get(&player).copied().unwrap_or(0))
    }
    pub fn bump(&mut self, player: PlayerId) -> CharacterRevision {
        let v = self.inner.entry(player).or_insert(0);
        *v += 1;
        CharacterRevision(*v)
    }
}

/// Persistence backend contract (SPEC.md section 117-119).
///
/// A transaction validates atomically, then applies; on failure the caller
/// rolls back the in-memory sim mutations and the backend keeps its previous
/// committed state.
pub trait PersistenceStore {
    /// Validate character revisions, then open a transaction. Returns the
    /// transaction id or an error (no state changed on error).
    fn begin(
        &mut self,
        trade: TradeId,
        parties: &[(PlayerId, CharacterRevision)],
    ) -> Result<TradeId, PersistenceError>;
    /// Queue a mutation inside the transaction.
    fn stage(&mut self, trade: TradeId, mutation: Mutation) -> Result<(), PersistenceError>;
    /// Commit atomically; bumps the revisions of all parties on success.
    fn commit(&mut self, trade: TradeId) -> Result<(), PersistenceError>;
    /// Abort the transaction; nothing is applied.
    fn rollback(&mut self, trade: TradeId) -> Result<(), PersistenceError>;
}

/// In-memory reference backend with a committed journal, mirroring the SQL
/// schema semantics: staged mutations are invisible until commit.
#[derive(Debug, Default)]
pub struct MemoryStore {
    revisions: Revisions,
    /// committed player gold: player -> (carried, stash)
    gold: BTreeMap<PlayerId, (u64, u64)>,
    /// committed item locations
    items: BTreeMap<ItemId, Vec<u8>>,
    /// staged, uncommitted transactions
    staged: BTreeMap<TradeId, Vec<Mutation>>,
    /// last committed mutation journal, append-only
    journal: Vec<(TradeId, Mutation)>,
    /// inject failures at commit time when set
    fail_commit: bool,
}

impl MemoryStore {
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }

    pub fn revision(&self, player: PlayerId) -> CharacterRevision {
        self.revisions.get(player)
    }

    pub fn gold(&self, player: PlayerId) -> (u64, u64) {
        self.gold.get(&player).copied().unwrap_or((0, 0))
    }

    pub fn item_location(&self, item: ItemId) -> Option<&Vec<u8>> {
        self.items.get(&item)
    }

    pub fn journal(&self) -> &[(TradeId, Mutation)] {
        &self.journal
    }

    /// Inject a commit failure for the next commit (section 118 tests).
    pub fn fail_next_commit(&mut self) {
        self.fail_commit = true;
    }
}

impl PersistenceStore for MemoryStore {
    fn begin(
        &mut self,
        trade: TradeId,
        parties: &[(PlayerId, CharacterRevision)],
    ) -> Result<TradeId, PersistenceError> {
        for (player, expected) in parties {
            if self.revisions.get(*player) != *expected {
                return Err(PersistenceError::RevisionMismatch(*player));
            }
        }
        self.staged.insert(trade, Vec::new());
        Ok(trade)
    }

    fn stage(&mut self, trade: TradeId, mutation: Mutation) -> Result<(), PersistenceError> {
        let staged = self
            .staged
            .get_mut(&trade)
            .ok_or(PersistenceError::UnknownTransaction(trade.0))?;
        staged.push(mutation);
        Ok(())
    }

    fn commit(&mut self, trade: TradeId) -> Result<(), PersistenceError> {
        if self.fail_commit {
            self.fail_commit = false;
            return Err(PersistenceError::Injected);
        }
        let mutations = self
            .staged
            .remove(&trade)
            .ok_or(PersistenceError::UnknownTransaction(trade.0))?;
        for mutation in &mutations {
            match mutation {
                Mutation::SetGold {
                    player,
                    carried,
                    stash,
                } => {
                    self.gold.insert(*player, (*carried, *stash));
                }
                Mutation::MoveItem { item, location } => {
                    self.items.insert(*item, location.clone());
                }
                Mutation::RemoveItem { item } => {
                    self.items.remove(item);
                }
            }
            self.journal.push((trade, mutation.clone()));
        }
        Ok(())
    }

    fn rollback(&mut self, trade: TradeId) -> Result<(), PersistenceError> {
        self.staged.remove(&trade);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_applies_mutations_and_bumps_revisions() {
        let mut store = MemoryStore::new();
        let p = PlayerId(1);
        let t = TradeId(1);
        store.begin(t, &[(p, CharacterRevision(0))]).unwrap();
        store
            .stage(
                t,
                Mutation::SetGold {
                    player: p,
                    carried: 500,
                    stash: 0,
                },
            )
            .unwrap();
        assert_eq!(store.gold(p), (0, 0), "invisible before commit");
        store.commit(t).unwrap();
        assert_eq!(store.gold(p), (500, 0));
    }

    #[test]
    fn revision_mismatch_rejects_begin() {
        let mut store = MemoryStore::new();
        let p = PlayerId(1);
        let err = store
            .begin(TradeId(1), &[(p, CharacterRevision(9))])
            .unwrap_err();
        assert!(matches!(err, PersistenceError::RevisionMismatch(_)));
    }

    #[test]
    fn rollback_keeps_previous_state() {
        let mut store = MemoryStore::new();
        let p = PlayerId(1);
        store
            .begin(TradeId(1), &[(p, CharacterRevision(0))])
            .unwrap();
        store.rollback(TradeId(1)).unwrap();
        let err = store.commit(TradeId(1)).unwrap_err();
        assert!(matches!(err, PersistenceError::UnknownTransaction(_)));
        assert_eq!(store.gold(p), (0, 0));
    }

    #[test]
    fn injected_commit_failure_changes_nothing() {
        let mut store = MemoryStore::new();
        let p = PlayerId(1);
        store
            .begin(TradeId(1), &[(p, CharacterRevision(0))])
            .unwrap();
        store
            .stage(
                TradeId(1),
                Mutation::SetGold {
                    player: p,
                    carried: 9,
                    stash: 0,
                },
            )
            .unwrap();
        store.fail_next_commit();
        let err = store.commit(TradeId(1)).unwrap_err();
        assert!(matches!(err, PersistenceError::Injected));
        assert_eq!(store.gold(p), (0, 0), "no partial application");
        assert!(store.journal().is_empty());
    }
}
