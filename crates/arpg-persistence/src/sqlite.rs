//! SQLite backend for persistence transactions (SPEC.md sections 117-121,
//! 192). Implements the same `PersistenceStore` contract as `MemoryStore`:
//! staged mutations are invisible until commit, revisions are validated at
//! `begin` and bumped at `commit`, all inside a single SQL transaction so
//! the commit is atomic. Enabled with the `sqlite` feature.

use crate::{CharacterRevision, Mutation, PersistenceError, PersistenceStore, TradeId};
use arpg_core::{ItemId, PlayerId};
use rusqlite::Connection;

pub struct SqliteStore {
    conn: Connection,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS character_revision (
    player_id INTEGER PRIMARY KEY,
    revision  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS character_gold (
    player_id INTEGER PRIMARY KEY,
    carried   INTEGER NOT NULL,
    stash     INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS item_location (
    item_id   INTEGER PRIMARY KEY,
    location  BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS open_trade (
    trade_id  INTEGER PRIMARY KEY,
    created   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS trade_party (
    trade_id  INTEGER NOT NULL,
    player_id INTEGER NOT NULL,
    PRIMARY KEY (trade_id, player_id)
);
CREATE TABLE IF NOT EXISTS staged_mutation (
    trade_id  INTEGER NOT NULL,
    seq       INTEGER NOT NULL,
    mutation  BLOB NOT NULL,
    PRIMARY KEY (trade_id, seq)
);
";

impl SqliteStore {
    /// Open (or create) the database at `path` and ensure the schema.
    pub fn open(path: &str) -> Result<SqliteStore, PersistenceError> {
        let conn = Connection::open(path).map_err(|e| PersistenceError::Backend(e.to_string()))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        Ok(SqliteStore { conn })
    }

    /// In-memory database, useful for tests.
    pub fn in_memory() -> Result<SqliteStore, PersistenceError> {
        let conn =
            Connection::open_in_memory().map_err(|e| PersistenceError::Backend(e.to_string()))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        Ok(SqliteStore { conn })
    }

    fn revision_of(&self, player: PlayerId) -> i64 {
        self.conn
            .query_row(
                "SELECT revision FROM character_revision WHERE player_id = ?1",
                [player.0 as i64],
                |row| row.get(0),
            )
            .unwrap_or(0)
    }

    pub fn gold(&self, player: PlayerId) -> (u64, u64) {
        self.conn
            .query_row(
                "SELECT carried, stash FROM character_gold WHERE player_id = ?1",
                [player.0 as i64],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64)),
            )
            .unwrap_or((0, 0))
    }

    pub fn item_location(&self, item: ItemId) -> Option<Vec<u8>> {
        self.conn
            .query_row(
                "SELECT location FROM item_location WHERE item_id = ?1",
                [item.0 as i64],
                |row| row.get(0),
            )
            .ok()
    }
}

/// Canonical mutation encoding shared with the replay/persistence format:
/// kind byte + payload. The location payload is opaque to the backend.
fn encode_mutation(m: &Mutation) -> Vec<u8> {
    let mut out = Vec::new();
    match m {
        Mutation::SetGold {
            player,
            carried,
            stash,
        } => {
            out.push(0u8);
            out.extend_from_slice(&player.0.to_le_bytes());
            out.extend_from_slice(&carried.to_le_bytes());
            out.extend_from_slice(&stash.to_le_bytes());
        }
        Mutation::MoveItem { item, location } => {
            out.push(1u8);
            out.extend_from_slice(&item.0.to_le_bytes());
            out.extend_from_slice(&(location.len() as u32).to_le_bytes());
            out.extend_from_slice(location);
        }
        Mutation::RemoveItem { item } => {
            out.push(2u8);
            out.extend_from_slice(&item.0.to_le_bytes());
        }
    }
    out
}

impl PersistenceStore for SqliteStore {
    fn begin(
        &mut self,
        trade: TradeId,
        parties: &[(PlayerId, CharacterRevision)],
    ) -> Result<TradeId, PersistenceError> {
        for (player, expected) in parties {
            if self.revision_of(*player) as u64 != expected.0 {
                return Err(PersistenceError::RevisionMismatch(*player));
            }
        }
        self.conn
            .execute(
                "INSERT OR REPLACE INTO open_trade (trade_id, created) VALUES (?1, 0)",
                [trade.0 as i64],
            )
            .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        for (player, _) in parties {
            self.conn
                .execute(
                    "INSERT OR REPLACE INTO trade_party (trade_id, player_id) VALUES (?1, ?2)",
                    rusqlite::params![trade.0 as i64, player.0 as i64],
                )
                .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        }
        Ok(trade)
    }

    fn stage(&mut self, trade: TradeId, mutation: Mutation) -> Result<(), PersistenceError> {
        let exists: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM open_trade WHERE trade_id = ?1",
                [trade.0 as i64],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if !exists {
            return Err(PersistenceError::UnknownTransaction(trade.0));
        }
        let seq: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(seq) + 1, 0) FROM staged_mutation WHERE trade_id = ?1",
                [trade.0 as i64],
                |row| row.get(0),
            )
            .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        self.conn
            .execute(
                "INSERT INTO staged_mutation (trade_id, seq, mutation) VALUES (?1, ?2, ?3)",
                rusqlite::params![trade.0 as i64, seq, encode_mutation(&mutation)],
            )
            .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Atomic commit: apply every staged mutation and bump the revisions of
    /// the mutated characters in a single SQL transaction. On any error the
    /// whole transaction rolls back and staged data survives.
    fn commit(&mut self, trade: TradeId) -> Result<(), PersistenceError> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        let rows: Vec<(i64, Vec<u8>)> = {
            let mut stmt = tx
                .prepare(
                    "SELECT seq, mutation FROM staged_mutation WHERE trade_id = ?1 ORDER BY seq",
                )
                .map_err(|e| PersistenceError::Backend(e.to_string()))?;
            let rows = stmt
                .query_map([trade.0 as i64], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .map_err(|e| PersistenceError::Backend(e.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| PersistenceError::Backend(e.to_string()))?;
            rows
        };
        if rows.is_empty() {
            let exists: bool = tx
                .query_row(
                    "SELECT 1 FROM open_trade WHERE trade_id = ?1",
                    [trade.0 as i64],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            if !exists {
                return Err(PersistenceError::UnknownTransaction(trade.0));
            }
        }
        for (_, blob) in &rows {
            match decode_and_apply(&tx, blob) {
                Ok(()) => {}
                Err(e) => return Err(e),
            }
        }
        // bump the revision of every party of the transaction
        tx.execute(
            "INSERT INTO character_revision (player_id, revision)
             SELECT player_id, 1 FROM trade_party WHERE trade_id = ?1
             ON CONFLICT(player_id) DO UPDATE SET revision = revision + 1",
            [trade.0 as i64],
        )
        .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        tx.execute(
            "DELETE FROM trade_party WHERE trade_id = ?1",
            [trade.0 as i64],
        )
        .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        tx.execute(
            "DELETE FROM staged_mutation WHERE trade_id = ?1",
            [trade.0 as i64],
        )
        .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        tx.execute(
            "DELETE FROM open_trade WHERE trade_id = ?1",
            [trade.0 as i64],
        )
        .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        tx.commit()
            .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        Ok(())
    }

    fn rollback(&mut self, trade: TradeId) -> Result<(), PersistenceError> {
        self.conn
            .execute(
                "DELETE FROM staged_mutation WHERE trade_id = ?1",
                [trade.0 as i64],
            )
            .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        self.conn
            .execute(
                "DELETE FROM open_trade WHERE trade_id = ?1",
                [trade.0 as i64],
            )
            .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        self.conn
            .execute(
                "DELETE FROM trade_party WHERE trade_id = ?1",
                [trade.0 as i64],
            )
            .map_err(|e| PersistenceError::Backend(e.to_string()))?;
        Ok(())
    }
}

/// Apply one decoded mutation inside the open SQL transaction.
fn decode_and_apply(tx: &rusqlite::Transaction, blob: &[u8]) -> Result<(), PersistenceError> {
    let backend = |e: rusqlite::Error| PersistenceError::Backend(e.to_string());
    if blob.is_empty() {
        return Err(PersistenceError::Backend("empty mutation".into()));
    }
    match blob[0] {
        0 => {
            // SetGold
            if blob.len() < 1 + 4 + 8 + 8 {
                return Err(PersistenceError::Backend("short SetGold".into()));
            }
            let player = u32::from_le_bytes(blob[1..5].try_into().unwrap()) as i64;
            let carried = u64::from_le_bytes(blob[5..13].try_into().unwrap());
            let stash = u64::from_le_bytes(blob[13..21].try_into().unwrap());
            tx.execute(
                "INSERT INTO character_gold (player_id, carried, stash) VALUES (?1, ?2, ?3)
                 ON CONFLICT(player_id) DO UPDATE SET carried = ?2, stash = ?3",
                rusqlite::params![player, carried as i64, stash as i64],
            )
            .map_err(backend)?;
        }
        1 => {
            // MoveItem
            if blob.len() < 1 + 16 + 4 {
                return Err(PersistenceError::Backend("short MoveItem".into()));
            }
            let item = u128::from_le_bytes(blob[1..17].try_into().unwrap()) as i64;
            let len = u32::from_le_bytes(blob[17..21].try_into().unwrap()) as usize;
            if blob.len() < 21 + len {
                return Err(PersistenceError::Backend("short MoveItem payload".into()));
            }
            let location = &blob[21..21 + len];
            tx.execute(
                "INSERT INTO item_location (item_id, location) VALUES (?1, ?2)
                 ON CONFLICT(item_id) DO UPDATE SET location = ?2",
                rusqlite::params![item, location],
            )
            .map_err(backend)?;
        }
        2 => {
            // RemoveItem
            if blob.len() < 1 + 16 {
                return Err(PersistenceError::Backend("short RemoveItem".into()));
            }
            let item = u128::from_le_bytes(blob[1..17].try_into().unwrap()) as i64;
            tx.execute("DELETE FROM item_location WHERE item_id = ?1", [item])
                .map_err(backend)?;
        }
        other => {
            return Err(PersistenceError::Backend(format!(
                "unknown mutation kind {other}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_is_atomic_and_bumps_revisions() {
        let mut store = SqliteStore::in_memory().unwrap();
        let trade = TradeId(7);
        let p = PlayerId(1);
        store.begin(trade, &[(p, CharacterRevision(0))]).unwrap();
        store
            .stage(
                trade,
                Mutation::SetGold {
                    player: p,
                    carried: 900,
                    stash: 100,
                },
            )
            .unwrap();
        store
            .stage(
                trade,
                Mutation::MoveItem {
                    item: ItemId(42),
                    location: vec![9, 9, 9],
                },
            )
            .unwrap();
        store.commit(trade).unwrap();
        assert_eq!(store.gold(p), (900, 100));
        assert_eq!(store.item_location(ItemId(42)), Some(vec![9, 9, 9]));
        // second begin with the stale revision fails
        let err = store.begin(TradeId(8), &[(p, CharacterRevision(0))]);
        assert!(matches!(err, Err(PersistenceError::RevisionMismatch(_))));
    }

    #[test]
    fn rollback_discards_staged_mutations() {
        let mut store = SqliteStore::in_memory().unwrap();
        let trade = TradeId(1);
        let p = PlayerId(2);
        store.begin(trade, &[(p, CharacterRevision(0))]).unwrap();
        store
            .stage(
                trade,
                Mutation::SetGold {
                    player: p,
                    carried: 5,
                    stash: 0,
                },
            )
            .unwrap();
        store.rollback(trade).unwrap();
        // the trade is gone: staging again is an unknown transaction
        let err = store.stage(trade, Mutation::RemoveItem { item: ItemId(1) });
        assert!(matches!(err, Err(PersistenceError::UnknownTransaction(1))));
        assert_eq!(store.gold(p), (0, 0));
    }

    #[test]
    fn double_commit_is_rejected() {
        let mut store = SqliteStore::in_memory().unwrap();
        let trade = TradeId(3);
        store.begin(trade, &[]).unwrap();
        store.commit(trade).unwrap();
        let err = store.commit(trade);
        assert!(matches!(err, Err(PersistenceError::UnknownTransaction(3))));
    }

    #[test]
    fn persists_across_reopen() {
        let path = std::env::temp_dir().join(format!("arpg-test-{}.sqlite", std::process::id()));
        let path = path.to_str().unwrap();
        let _ = std::fs::remove_file(path);
        {
            let mut store = SqliteStore::open(path).unwrap();
            let trade = TradeId(1);
            store.begin(trade, &[]).unwrap();
            store
                .stage(
                    trade,
                    Mutation::SetGold {
                        player: PlayerId(5),
                        carried: 1234,
                        stash: 77,
                    },
                )
                .unwrap();
            store.commit(trade).unwrap();
        }
        let store = SqliteStore::open(path).unwrap();
        assert_eq!(store.gold(PlayerId(5)), (1234, 77));
        let _ = std::fs::remove_file(path);
    }
}
