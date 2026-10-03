# GAMEPLAY_SEMANTICS.md

Document normatif (SPEC.md §200). Décrit les sémantiques gameplay contractuelles du moteur. Toute divergence entre ce document et le code est un bug.

## Phase ordering

L'ordre des phases est contractuel (SPEC.md §8, implémenté dans `arpg-sim/src/phase.rs::PHASE_ORDER`) :

```
BeginTick
SessionTransitions
IngestCommands
CanonicalizeCommands
ValidateCommands
UpdatePlayerIntent
Perception
AiDecision
ActionStateAdvance
MovementIntent
MovementResolution
InteractionResolution
MissileMovement
MissileCollision
EffectGeneration
EffectResolution
PeriodicStates
Regeneration
PendingDeathResolution
SpawnResolution
LootResolution
InventoryTransactions
QuestResolution
WorldObjectUpdate
Expiration
ReplicationEventBuild
EndTick
```

**Visibilité intra-tick** : une modification réalisée dans une phase n'est observable que par les phases suivantes. Une file (`Vec` vidée par la phase consommatrice) matérialise chaque dépendance inter-phases :

- `pending_drops` : PendingDeathResolution → LootResolution
- `pending_quest_events` : phases gameplay → QuestResolution

Modifier cet ordre constitue une modification du moteur gameplay.

## Simultaneous effects

- **Pickup simultané** (§86) : le premier pickup valide gagne l'item ; les suivants reçoivent `ItemUnavailable`. La transaction d'inventaire valide tout avant d'appliquer : une erreur avant application produit zéro mutation.
- **Événements de quête** (§106) : un événement n'est évalué qu'une fois par quête et par tick (garde `(QuestDefId, trigger_key)`). Les actions game-level (flags, variables, complétion) s'appliquent une fois ; les actions joueur (XP, items, waypoints) par joueur éligible.
- **Commandes** : l'admission est par joueur avec séquence monotone ; la canonicalisation trie par `execute_tick`, puis `player_slot`, puis `sequence` — l'ordre d'arrivée réseau n'a aucune influence après admission.

## Death semantics

Cycle de vie d'un acteur : `Alive → PendingDeath → Dead`.

- Les dégâts qui amènent la vie ≤ 0 passent l'acteur en `PendingDeath` et émettent `EntityKilled` avec crédit au tueur.
- `PendingDeathResolution` applique la mort : l'acteur passe `Dead`/mode `Dead`, l'action en cours est annulée, le monstre est retiré du store et son drop (si TC) est mis en file pour `LootResolution`.
- Un acteur `PendingDeath` ne peut pas démarrer une nouvelle action.
- Un objet droppé au tick T est lootable à partir de T+1.

## Spawn activation

- `GameInstance::spawn_monster[_with_tc]` crée l'entrée, notifie l'AiBrain (`on_spawn`) et émet `EntitySpawned` — visible seulement par les phases suivantes du même tick.

## Interruptions

Priorités d'interruption (`InterruptPriority`) : un knockback ou une action de priorité supérieure interrompt l'action en cours. Une compétence invalide empêche le chargement du datapack (§1093) ; une compétence invalide soumise en jeu est rejetée à l'enregistrement.

## Target invalidation

- Une cible morte/retirée invalide l'action qui la référence ; les missiles vérifient leur cible à l'impact.
- Le kill crédite le `killer` enregistré au moment du dégât fatal.

## Movement conflicts

- Les intents de mouvement sont enregistrés pendant `UpdatePlayerIntent` et résolus pendant `MovementResolution` — un seul intent par joueur et par tick (le dernier soumis gagne).
- La résolution vérifie l'occupation de la tile cible par un autre joueur ; un mouvement vers une tile occupée est annulé (pas de déplacement partiel).
- Les monstres résolvent leurs intents de mouvement dans la même phase, après les joueurs.
