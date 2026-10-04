# GAMEPLAY_SEMANTICS

> Document normatif (SPEC.md section 200). Sémantique gameplay canonique du moteur.
> Source de vérité : `crates/arpg-sim`. En cas de divergence, le code fait foi et ce
> document doit être corrigé dans le même commit.

## Phase ordering (§8, §12)

Un tick exécute exactement les 26 phases dans l'ordre fixe défini par
`arpg_sim::phase::Phase` :

```text
BeginTick, SessionTransitions, IngestCommands, CanonicalizeCommands,
ValidateCommands, UpdatePlayerIntent, Perception, AiDecision,
ActionStateAdvance, MovementIntent, MovementResolution,
InteractionResolution, MissileMovement, MissileCollision,
EffectGeneration, EffectResolution, PeriodicStates, Regeneration,
PendingDeathResolution, SpawnResolution, LootResolution,
InventoryTransactions, QuestResolution, WorldObjectUpdate,
Expiration, ReplicationEventBuild, EndTick
```

Règles :

- Un système s'exécute dans une seule phase (INV-015).
- Aucune phase n'effectue d'I/O ni n'accède au temps système (INV-003/004).
- L'ordre des itérations intra-phase est canonique (BTreeMap par identifiant ;
  commandes triées par `execute_tick -> player_slot -> sequence`, §134).

## Simultaneous effects (§46)

Quand plusieurs sources produisent le même effet sur la même cible au cours
d'un tick (aura + dot + skill) :

1. Les dégâts s'appliquent par packet (`DamagePacket`), dans l'ordre
   d'émission des ops du programme de skill.
2. Les contributions de stats se résolvent par le graphe de stats
   (`StatBlock` : stages par `ModifierOp`, insertion-order indépendant,
   priorité puis séquence de source comme départage).
3. Aucun effet n'est "fusionné avant application" : chaque op produit son
   propre évènement traçable dans `GameEvent`.

## Death semantics (§23, §100)

- La mort est résolue en `PendingDeathResolution`, jamais immédiatement au
  moment des dégâts : un acteur à `life <= 0` passe par la file de mort.
- `SpawnResolution`/`LootResolution` consomment la file : drops (`pending_drops`)
  roulés depuis la seed de mort (dérivée BLAKE3 root_seed/attacker/tick/index).
- Un monstre dont la définition autorise `leaves_corpse` produit un cadavre
  (type, position, killer, `consumed`, né-tick) expirant après 600 ticks.
- Les effets "on death" (explosion de cadavre, etc.) consomment le cadavre
  exactement une fois (`consume`, single-use par op de cast).

## Spawn activation (§33)

- Tout spawn passait par `spawn_*` de `GameInstance` : entité allouée avec
  identifiant déterministe (`spawn_sequence`), état initial posé avant que
  l'entité ne soit visible dans les phases suivantes du même tick.
- Les monstres pack/champion sont scalés à la création (facteurs fixés au
  spawn, jamais recalculés ensuite).
- Aucune entité n'est "activée à distance" : les systèmes voient toutes les
  entités vivantes, l'interest management (§140) filtre uniquement la
  réplication.

## Interruptions

- Une action (Cast/Attack) avancée en `ActionStateAdvance` n'est annulée que
  par : mort de l'acteur, transition de session, ou hit recovery
  (`hit_recovery_bonus_bp` du graphe de stats).
- Une action annulée ne produit jamais son impact : les ops ne s'exécutent
  qu'au passage `Impact` (§36).
- Les intents de mouvement sont supplantables à tout moment (dernier
  gagnant, §129) ; les actions à effet de bord ne le sont pas.

## Target invalidation (§36, §46)

- Une compétence résout sa cible à l'impact, pas au lancement : `Target::Position`
  capturé au `start_cast`, validé à l'exécution (`resolve_cast_impacts`).
- Les missiles portent leur cible positionnelle ; la collision teste les
  entités vivantes seulement.
- Un cadavre ciblé mais déjà consommé fait échouer l'op silencieusement
  (aucune dépense de ressource, aucun dégât).
- Les serviteurs (summons) et mercenaires héritent du propriétaire pour la
  hostile-check ; cible invalide = aucun effet, jamais de panic.

## Movement conflicts (§11, §12)

- `MovementResolution` est autoritaire : intentions de mouvement par joueur,
  résolution une entité à la fois dans l'ordre des identifiants.
- Collision : un déplacement bloqué ne s'applique pas (aucun glissement
  implicite) ; l'intention reste pour le tick suivant si non supplantée.
- Les missiles utilisent la même résolution spatiale (`MissileMovement`
  puis `MissileCollision`) avec liste d'entités déjà touchées par missile.
