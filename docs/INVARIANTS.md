# INVARIANTS

> Document normatif (SPEC.md section 200). Chaque invariant INV-XXX est tracé
> vers son point d'implémentation et son test de non-régression.
> Priorité d'arbitrage en cas de conflit : §201.

## Mapping invariants → implémentation

| Invariant | Description | Implémentation | Test |
|---|---|---|---|
| INV-001 | Le serveur est l'unique autorité gameplay | `arpg-sim::GameInstance` (seul propriétaire des mutations) | `arpg-sim/tests/determinism.rs` |
| INV-002 | Une GameInstance a un unique propriétaire logique | `GameInstance` non-`Clone`, accesseurs `&mut self` | revue de type |
| INV-003 | Un tick ne réalise aucun I/O | `arpg-sim` sans dépendance I/O (`Cargo.toml`) | revue de dépendances |
| INV-004 | Aucun temps système dans une règle gameplay | `arpg-sim` sans `std::time` | revue de dépendances |
| INV-005 | Aucun RNG implicite | tout RNG dérive de `RngStreams` / seeds BLAKE3 explicites | `arpg-core/tests/rng_vectors.rs` |
| INV-006 | Aucun f32/f64 pour un calcul gameplay | `Fixed` (i32), `WorldPos` (i32) | `arpg-core/tests/fixed.rs` |
| INV-007 | L'ordre d'arrivée ne modifie pas le résultat | itération `BTreeMap`/tri canonique partout | `determinism.rs::cross_player_arrival_order_same_hashes` |
| INV-008 | Datapack et ruleset immuables pendant une partie | `Arc<GameData>` / `Arc<GameRules>` sans accès mutable | types |
| INV-009 | Une commande client décrit une intention | `ClientCommand` = intentions ; résolution serveur | `determinism.rs` |
| INV-010 | Un ItemId possède exactement un emplacement | `InventorySystem` : `ItemLocation` unique + `slot_owner` | `arpg-sim/tests/items.rs::inventory_move_is_transactional` |
| INV-011 | Toute transaction inventaire/échange est atomique | `move_item`/`swap_slots` (§85) ; `TradeSystem` Requested→Committed | `tests/items.rs`, `tests/trade_flow.rs` |
| INV-012 | Toute modification persistante possède une révision | `CharacterRevision` + concurrence optimiste (`persistence`) | `persistence_stats_count_conflicts_and_failures` |
| INV-013 | Les données client sont un sous-ensemble de l'état serveur | DTO `PlayerView`/`Snapshot`/`EntityDelta` seulement | `arpg-server/tests/session.rs::snapshot_contains_only_client_visible_state` |
| INV-014 | Les systèmes gameplay n'accèdent ni réseau, ni disque, ni SQL | graphe de dépendances ; persistance hors tick | `Cargo.toml` par crate |
| INV-015 | Une règle gameplay a un point d'implémentation unique | un système par phase (`phase::Phase`) | revue par phase |
| INV-016 | Un cadavre sert au plus une consommation | `CorpseSystem::consume` single-use par op | tests corpse (§100) |
| INV-017 | Un emplacement main occupé par un deux-mains est réservé | réservation `slot_owner` des deux mains (§77) | `tests/items.rs::two_handed_weapon_reserves_both_hand_slots` |
| INV-018 | Chaque cast d'un item chargé dépense exactement une charge | `consume_charge` à l'exécution (§79) | `tests/skill_flow.rs::charged_item_spends_one_charge_per_cast` |

## Notes de mise en œuvre

- **State hash (§160)** : `GameInstance::state_hash` sérialise l'état canonique
  (joueurs par `PlayerId`, inventaire avec révisions de charges, systèmes par
  `hash_bytes` triés). Événements, métriques et caches exclus.
- **Canonisation des commandes (§134)** : tri `execute_tick -> player_slot ->
  sequence` ; l'ordre d'arrivée réseau n'a aucune influence après admission —
  validé par le harness réseau chaotique (§186).
- **Admission (§132/§133)** : `Accepted` / `Deferred` / `RejectedTooOld` /
  `RejectedInvalid` / `Duplicate` ; input delay borné 1..4 ticks (§131).
- **Fixed-point (§44/§45)** : `Fixed` i32, échelle 1/256, division par
  `div_fp` avec arrondi explicite ; débordements `checked` (§174).
- **RNG (§15/§16)** : ChaCha8, seeds BLAKE3(root_seed ‖ domaine ‖ ids ‖
  séquence) ; vecteurs de test verrouillés.
- **Réplication (§141/§142)** : révisions monotones par entité, deltas
  seulement au-delà de la base acquittée, resync complet sur demande.
