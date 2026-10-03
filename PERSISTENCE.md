# PERSISTENCE.md

Document normatif (SPEC.md §200). Schéma de base, modèle de révisions, transactions de trade, sémantique d'échec, migrations, sauvegardes. Toute divergence entre ce document et le code est un bug.

## Schéma logique (arpg-persistence)

Backend de référence : `MemoryStore` (journal en mémoire). Un backend SQL (SQLite/PostgreSQL, §121) implémente le même trait `PersistenceStore` avec la même sémantique.

```text
characters
├── player_id: PlayerId        (PK)
├── revision: u64              (optimistic concurrency)
├── schema_version: u32        (SaveSchemaVersion)
├── class, level, experience
├── carried_gold, stash_gold
├── items: Vec<PersistentItem>       // id, definition, location
├── quests: PersistentQuestState     // quest -> status
└── waypoints: PersistentWaypoints   // difficulty -> [waypoint]

trade_journal
├── trade_id: TradeId          (PK)
└── committed mutations, append-only : SetGold | MoveItem | RemoveItem
```

`CharacterSnapshot` (§120) est l'unité de save/load : `schema_version`, `revision`, `class`, `level`, `experience`, items, gold, quêtes, waypoints. Le cache calculé n'est jamais sérialisé.

## Modèle de révisions

- Chaque caractère porte une `CharacterRevision` monotone, bumpée à chaque `save` (`CharacterRepository::save`) et à chaque commit de trade.
- `PersistenceStore::begin(trade, parties)` valide les révisions **avant** d'ouvrir la transaction : un mismatch → `RevisionMismatch`, aucun état changé.

## Transactions de trade (§117-118, arpg-sim/src/trade.rs)

Pipeline contractuel :

```
validate atomiquement : ownership | items existent | gold | capacité | restrictions | révisions
→ state = Persisting
→ store.begin(trade, [(a, rev_a), (b, rev_b)])
→ appliquer les mutations sim en mémoire (échange or + items, journal `applied`)
→ store.stage(...) pour chaque mutation (or des deux parties + localisations)
→ store.commit(trade)
→ state = Committed ; le client reçoit TradeCommitted
```

`TradeCommitted` n'est émis qu'après succès de la transaction de persistance. Le commit n'est jamais confirmé en mémoire seule.

## Sémantique d'échec (§118)

Sur échec de la transaction (validation, staging ou commit) :

1. **Rollback sim** : items re-déplacés en ordre inverse, or restauré aux valeurs d'avant-transaction (`rollback_mutations`).
2. **Rollback backend** : `store.rollback(trade)` — les mutations stagées restent invisibles ; rien n'entre au journal.
3. **État trade** : retour à `Open` (ou `Cancelled`).
4. Le client reçoit `PersistenceFailure`.

Un échec injecté (`MemoryStore::fail_next_commit`) produit zéro application partielle : le journal reste vide, l'or backend inchangé — testé dans `trade::tests::persistence_failure_rolls_back_everything`.

## Save / load / reconnect (§119-120)

- `CharacterRepository::load` vérifie `schema_version == SaveSchemaVersion` ; un schéma non supporté → `UnsupportedSchema`, refus de charger.
- Reconnect : le snapshot est rechargé puis une resynchronisation complète (PROTOCOL.md §resync) est envoyée au client.
- Les commandes d'inventaire et de trade ne sont jamais rétroactivement simulées (§2967).

## Migrations

- Versions séparées (§157) : `SaveSchemaVersion` évolue indépendamment de l'engine/protocol.
- Politique : un save d'une version antérieure est chargé après migration explicite ; jamais de chargement silencieux avec perte de données ; `load` refuse plutôt que de deviner.
- Le journal de trade est append-only : les migrations ne réécrivent jamais l'historique engagé.

## Sauvegardes (backups)

- Le snapshot par caractère est la granularité de sauvegarde ; il est auto-contenu (items, or, quêtes, waypoints).
- Les waypoints sont persistés par `character × difficulty` (§107).
- Un trade ne peut pas être à moitié engagé dans une sauvegarde : grâce au pipeline transactionnel, un snapshot pris à tout moment contient soit l'état pré-trade, soit post-trade complet.
