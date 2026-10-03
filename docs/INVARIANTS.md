# INVARIANTS

> Document normatif (SPEC.md section 200). Chaque invariant INV-XXX est tracé vers son point d'implémentation et son test de non-régression.

## Mapping invariants → implémentation (M0)

| Invariant | Description | Implémentation | Test |
|---|---|---|---|
| INV-001 | Le serveur est l'unique autorité gameplay | `arpg-sim::GameInstance` (seul propriétaire des mutations) | `arpg-sim/tests/determinism.rs` |
| INV-002 | Une GameInstance a un unique propriétaire logique | `GameInstance` non-`Clone`, accès `&mut self` sur `tick()` | — |
| INV-003 | Un tick ne réalise aucun I/O | `arpg-sim` n'a aucune dépendance I/O (`Cargo.toml`) | revue de dépendances |
| INV-004 | Aucun temps système dans une règle gameplay | `arpg-sim` ne dépend pas de `std::time` | `cargo tree` en CI (à ajouter) |
| INV-005 | Aucun RNG implicite | Tout RNG passe par `RngStreams::derive` (domaine + séquence explicites) | `arpg-core/tests/rng_vectors.rs::independent_domains_do_not_interfere` |
| INV-006 | Aucun f32/f64 pour un calcul gameplay | `Fixed` (i32), `WorldPos` (i32), distances en entiers | `arpg-core/tests/fixed.rs` |
| INV-007 | L'ordre d'itération non ordonnée ne modifie pas le résultat | `BTreeMap`/tri canonique partout ; `take_due` trie (tick, player, sequence) | `determinism.rs::cross_player_arrival_order_same_hashes` |
| INV-008 | Datapack et ruleset immuables pendant une partie | `Arc<GameData>` / `Arc<GameRules>` sans accès mutable | types |
| INV-009 | Une commande client décrit une intention | `ClientCommand` = intentions ; le serveur résout | `determinism.rs` |
| INV-010 | Un ItemId possède exactement un emplacement | `ItemLocation` (à implémenter en M6) | à implémenter |
| INV-011 | Toute transaction inventaire/échange est atomique | à implémenter (M6/M7) | à implémenter |
| INV-012 | Toute modification persistante possède une révision | à implémenter (M7) | à implémenter |
| INV-013 | Les données client sont un sous-ensemble de l'état serveur | à implémenter (M1, DTO réseau) | à implémenter |
| INV-014 | Les systèmes gameplay n'accèdent ni réseau, ni disque, ni SQL | Graphe de dépendances §6 interdit `tokio`/`sqlx` dans `arpg-sim` | revue de dépendances |
| INV-015 | Une règle gameplay a un point d'implémentation unique | Phases de tick §8 : un système par phase (en cours) | à compléter |

## Notes de mise en œuvre

- **State hash (§160)** : `GameState::canonical_hash_input` sérialise l'état trié (joueurs par `PlayerId`, compteurs RNG) et le scheduler pendin (trié par tick, player, sequence). Les événements, métriques et caches en sont exclus.
- **Canonisation des commandes (§134)** : `Scheduler::take_due` trie par `execute_tick`, puis `player_slot`, puis `sequence`. L'ordre d'arrivée réseau n'a aucune influence après admission — validé par test.
- **Admission (§132/§133)** : `Scheduler::admit` retourne `Accepted` / `Deferred` (implicite via `execute_tick` futur) / `RejectedTooOld` / `RejectedInvalid` / `Duplicate`. Le `execute_tick` suit `max(earliest_allowed, translated_client_tick)` (§130) avec input delay borné 1..4 ticks (§131).
- **Fixed-point (§44/§45)** : `Fixed` sur i32, échelle 1/256. Division exclusivement via `div_fp` avec mode d'arrondi explicite (`Floor`, `Ceil`, `TowardZero`, `Nearest`). Débordements : `checked` (panique = invariant impossible, §174).
- **RNG (§15/§16)** : ChaCha8, seeds dérivées `BLAKE3(root_seed || domain || stable_ids || sequence)`. Vecteurs de test verrouillés pour détecter toute dérive de dépendance.
