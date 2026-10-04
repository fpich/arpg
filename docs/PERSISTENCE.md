# PERSISTENCE

> Document normatif (SPEC.md sections 116-118, 119-121, §200). Persistance
> des personnages, or, objets et échanges. Source de vérité :
> `crates/arpg-persistence`.

## Modèle de révision (§121)

Toute donnée persistante porte une révision monotone
(`CharacterRevision(u64)`, INV-012) :

- `save` incrémente la révision du propriétaire.
- `begin(players, expected_revisions)` : concurrence optimiste — toute
  divergence entre révision attendue et stockée rejette la transaction
  (`RevisionMismatch`).
- `commit` applique les mutations puis bump les révisions de toutes les
  parties — atomiquement.
- Les conflits sont comptés (`PersistenceStats::revision_conflicts`) et
  observables en métrique, jamais silencieux.

## Schéma SQL (backend `SqliteStore`)

```sql
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
```

`MemoryStore` implémente le même contrat (`Store`) pour les tests et le
fonctionnement sans SQL ; `CharacterRepository` (snapshots de personnages,
§119) réutilise le même modèle de révision.

## Transactions d'échange (§116-118)

Cycle persistant d'un trade :

1. `open_trade` + `trade_party` : le trade ouvert et ses participants
   existent avant toute mutation.
2. `staged_mutation (trade_id, seq, mutation)` : chaque mutation offer/lock
   est stagée, séquencée, jamais appliquée avant commit.
3. Le commit valide les révisions des deux parties, applique les mutations
   stagées dans l'ordre `seq`, bump les deux révisions, supprime le trade
   ouvert et ses lignes — tout ou rien.
4. Un trade annulé ne laisse aucune `staged_mutation` résiduelle : la
   suppression est atomique avec le cleanup.

## Sémantique d'échec (§190-191)

- `RevisionMismatch` : rien n'est écrit ; l'appelant reçoit l'erreur et les
  conflits sont comptés.
- `commit` échoué : aucun bump de révision, aucune mutation appliquée.
- Un store indisponible échoue proprement (`Store::Error`) — la couche
  gameplay ne fait jamais d'I/O elle-même (INV-014) : la persistance est
  toujours pilotée hors tick.

## Migrations

Le schéma est créé par DDL idempotent (`CREATE TABLE IF NOT EXISTS`) à
l'ouverture — le seul mécanisme de migration actuel. Toute évolution de
schéma doit : (1) rester rétro-compatible avec les lignes existantes,
(2) passer par le DDL idempotent, (3) couvrir les deux backends
(SQLite + mémoire) dans les mêmes tests.

## Backups

Aucun mécanisme de backup in-process : les sauvegardes sont une opération
d'exploitation (copie du fichier SQLite à froid ou `VACUUM INTO`), jamais
exécutée par le moteur pendant une partie. L'invariant est qu'un backup
pris hors tick est cohérent car toutes les écritures sont transactionnelles.
