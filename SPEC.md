# Spécification technique v4.0  
## Moteur ARPG headless multijoueur en Rust inspiré de Diablo II

**Statut : conception normative**  
**Langage : Rust stable**  
**Architecture : serveur autoritaire**  
**Simulation : déterministe, fixed timestep**  
**Référence fonctionnelle initiale : Diablo II: Lord of Destruction 1.10+**  
**Compatibilité avec Diablo II original : non requise**

---

# 1. Objectif

Le projet implémente un moteur ARPG multijoueur headless moderne dont les systèmes de jeu sont fortement inspirés de Diablo II LoD.

Le moteur doit permettre :

- 1 à 8 joueurs par partie ;
- simulation complète sans interface graphique ;
- serveur autoritaire ;
- clients distants ;
- bots ;
- simulations de masse ;
- replays déterministes ;
- datapacks remplaçables ;
- persistance des personnages ;
- progression sur plusieurs actes et difficultés.

Le moteur ne cherche pas à reproduire :

- le protocole Battle.net ;
- les sauvegardes `.d2s` ;
- le RNG historique ;
- les bugs historiques ;
- les limitations du moteur graphique original ;
- l'ordre exact des opérations du binaire Diablo II ;
- les formats MPQ ou autres formats propriétaires au runtime ;
- la compatibilité avec le client original.

Diablo II est une référence de **game design**, de contenu et de proportions, pas une ABI à émuler.

---

# 2. Terminologie normative

Les mots suivants ont un sens précis.

**MUST / DOIT**  
Condition obligatoire.

**MUST NOT / NE DOIT PAS**  
Comportement interdit.

**SHOULD / DEVRAIT**  
Choix recommandé dont une dérogation doit être justifiée.

**MAY / PEUT**  
Option libre.

Trois catégories de configuration sont distinguées.

```text
ENGINE
    invariant technique du moteur

RULESET
    règle de gameplay d'une partie

POLICY
    politique opérationnelle du serveur
```

Exemples :

```text
25 ticks/s                        ENGINE
ordre des phases                  ENGINE
atomicité inventaire              ENGINE

nombre maximum de joueurs         RULESET
scaling monstres                  RULESET
loot FFA/instancié                RULESET
pénalité de mort                  RULESET

timeout de reconnexion            POLICY
fréquence des snapshots réseau    POLICY
nombre de parties/processus       POLICY
```

Une valeur appartenant à `POLICY` ne doit jamais modifier les résultats gameplay d'un replay.

---

# 3. Principes architecturaux

Les invariants fondamentaux sont :

```text
INV-001
Le serveur est l'unique autorité gameplay.

INV-002
Une GameInstance est simulée par un unique propriétaire logique à un instant donné.

INV-003
Un tick ne réalise aucun I/O.

INV-004
Aucun temps système n'intervient dans une règle gameplay.

INV-005
Aucun RNG implicite n'est autorisé.

INV-006
Aucun f32/f64 n'est utilisé pour un calcul gameplay déterministe.

INV-007
L'ordre d'itération d'une collection non ordonnée ne doit jamais modifier le résultat.

INV-008
Un datapack et un ruleset sont immuables pendant toute la durée d'une partie.

INV-009
Une commande client décrit une intention, jamais un résultat.

INV-010
Un ItemId possède exactement un emplacement logique.

INV-011
Toute transaction d'inventaire ou d'échange est atomique.

INV-012
Toute modification persistante possède une révision.

INV-013
Les données envoyées au client sont un sous-ensemble de l'état serveur.

INV-014
Les systèmes gameplay n'accèdent ni au réseau, ni au disque, ni à SQL.

INV-015
Une règle gameplay possède un point d'implémentation unique.
```

---

# 4. Séparation fondamentale : Data / Rules / State

Le moteur est structuré autour de trois concepts séparés.

```rust
pub struct GameInstance {
    pub data: Arc<GameData>,
    pub rules: Arc<GameRules>,
    pub state: GameState,

    command_queue: CommandQueue,
    scheduler: Scheduler,
    events: EventBuffer,
}
```

## 4.1 `GameData`

Contenu statique.

Exemples :

```text
classes
skills
items
affixes
monsters
missiles
levels
Treasure Classes
sets
uniques
runewords
recipes
quests
NPC
hirelings
```

`GameData` est immuable.

## 4.2 `GameRules`

Règles d'une partie.

Exemples :

```text
max_players
difficulty
loot policy
PvP policy
death rules
party XP rules
monster scaling
inventory dimensions
level cap
resistance caps
```

`GameRules` est immuable pendant une partie.

## 4.3 `GameState`

État mutable courant.

Exemples :

```text
positions
HP
mana
monstres vivants
inventaires
états temporaires
progression de quêtes
portails
missiles
RNG counters
```

Cette séparation est obligatoire.

---

# 5. Architecture du workspace Rust

```text
arpg/
├── crates/
│   ├── arpg-core/
│   ├── arpg-data/
│   ├── arpg-rules/
│   ├── arpg-sim/
│   ├── arpg-world/
│   ├── arpg-ai/
│   ├── arpg-protocol/
│   ├── arpg-server/
│   ├── arpg-persistence/
│   ├── arpg-replay/
│   └── arpg-tools/
│
├── datapacks/
├── tests/
│   ├── unit/
│   ├── conformance/
│   ├── determinism/
│   ├── multiplayer/
│   ├── fuzz/
│   └── load/
│
└── docs/
    ├── GAMEPLAY_SEMANTICS.md
    ├── DATA_MODEL.md
    ├── PROTOCOL.md
    ├── PERSISTENCE.md
    └── INVARIANTS.md
```

---

# 6. Dépendances entre crates

Dépendances autorisées :

```text
core
 ↑
data      rules
 ↑          ↑
 └──── sim ─┘
       ↑
     world
       ↑
      ai

protocol ← core

server
 ├── sim
 ├── protocol
 └── persistence

replay
 ├── sim
 └── protocol
```

Interdictions :

```text
arpg-sim → tokio
arpg-sim → quinn
arpg-sim → sqlx
arpg-sim → filesystem
arpg-sim → SystemTime
```

---

# 7. Temps

La simulation fonctionne à :

```rust
pub const TICKS_PER_SECOND: u32 = 25;
pub const TICK_DURATION_MS: u32 = 40;
```

`25 Hz` est un invariant `ENGINE`.

Toutes les durées gameplay sont exprimées en ticks ou sous-unités fixed-point de tick.

```rust
#[repr(transparent)]
pub struct Tick(pub u64);
```

Aucune règle gameplay ne dépend de :

```rust
std::time::Instant
std::time::SystemTime
```

---

# 8. Sémantique exacte d'un tick

Chaque tick exécute exactement les phases suivantes.

```text
00 BeginTick
01 SessionTransitions
02 IngestCommands
03 CanonicalizeCommands
04 ValidateCommands
05 UpdatePlayerIntent
06 Perception
07 AiDecision
08 ActionStateAdvance
09 MovementIntent
10 MovementResolution
11 InteractionResolution
12 MissileMovement
13 MissileCollision
14 EffectGeneration
15 EffectResolution
16 PeriodicStates
17 Regeneration
18 PendingDeathResolution
19 SpawnResolution
20 LootResolution
21 InventoryTransactions
22 QuestResolution
23 WorldObjectUpdate
24 Expiration
25 ReplicationEventBuild
26 EndTick
```

L'ordre est contractuel.

Une modification de cet ordre constitue une modification du moteur gameplay.

---

# 9. Visibilité intra-tick

Une modification réalisée dans une phase n'est observable que par les phases suivantes.

Une phase ne peut pas rétroagir sur une phase déjà terminée.

Exemple :

```text
MovementResolution
    modifie Position

InteractionResolution
    observe la nouvelle Position
```

Mais :

```text
QuestResolution
```

ne peut pas modifier le résultat d'un combat déjà terminé dans `EffectResolution`.

---

# 10. Entités créées pendant un tick

Toute nouvelle entité possède :

```rust
pub struct SpawnMetadata {
    pub born_tick: Tick,
    pub active_from: Tick,
}
```

Politique par défaut :

```text
active_from = born_tick + 1
```

Donc :

- un monstre créé au tick T ne pense pas au tick T ;
- une invocation créée au tick T ne frappe pas au tick T ;
- un missile créé au tick T commence son déplacement au tick T+1 ;
- un objet tombé au tick T est ramassable à T+1.

Une exception doit être explicitement définie par type d'effet.

---

# 11. Destruction

Une entité n'est pas supprimée physiquement immédiatement.

Elle est marquée :

```rust
Lifecycle::PendingRemoval
```

Puis retirée durant :

```text
Expiration
```

Cela évite d'invalider les itérations en cours.

---

# 12. Résolution des événements simultanés

Chaque événement mutable possède une clé canonique :

```rust
pub struct EventOrderKey {
    pub priority: u16,
    pub target: EntityId,
    pub source: EntityId,
    pub sequence: u64,
}
```

Tri lexicographique.

Ainsi deux événements simultanés produisent toujours le même résultat.

---

# 13. Mort

Quand :

```text
life <= 0
```

pendant `EffectResolution`, l'entité devient :

```text
Alive
→ PendingDeath
```

Une entité `PendingDeath` :

- ne peut plus lancer d'action ;
- ne peut plus recevoir de soins ordinaires ;
- peut recevoir uniquement un effet explicitement marqué `CanAffectPendingDeath`.

La mort définitive est traitée dans :

```text
PendingDeathResolution
```

Puis :

```text
PendingDeath
→ Dead
```

Le loot, XP et progression de quête utilisent cette transition.

---

# 14. Attribution du kill

Le `killer` est la source du premier événement ordonné faisant passer :

```text
life > 0
```

à :

```text
life <= 0
```

L'assistance est calculée séparément.

Le kill credit ne dépend donc pas du timing réseau.

---

# 15. RNG

Le moteur utilise un RNG déterministe moderne.

Algorithme de référence :

```text
ChaCha8
```

La version du crate est verrouillée.

Des vecteurs de tests RNG sont intégrés afin qu'une mise à jour de dépendance ne puisse modifier silencieusement les résultats.

---

# 16. RNG hiérarchique

Le moteur n'utilise pas simplement un flux global `COMBAT`, `LOOT`, etc.

Les seeds sont dérivées par domaine et contexte.

```text
RootSeed
│
├── World(LevelId)
│
├── Spawn(LevelId, SpawnSequence)
│
├── AI(EntityId, DecisionSequence)
│
├── Attack(EntityId, ActionSequence)
│
├── Missile(EntityId)
│
├── Drop(EntityId, DeathSequence)
│   ├── TreasureClass
│   ├── Quality
│   ├── Affixes
│   └── Sockets
│
└── Merchant(NpcId, RefreshSequence)
```

Seed :

```text
BLAKE3(
    root_seed
    || domain
    || stable identifiers
    || sequence
)
```

Cela garantit qu'ajouter une décision IA ne change pas le loot futur.

---

# 17. Séquences RNG

Les compteurs suivants font partie de `GameState` :

```text
attack_sequence
ai_decision_sequence
spawn_sequence
death_sequence
merchant_refresh_sequence
```

Ils sont incrémentés explicitement.

Aucun compteur RNG n'est caché dans un objet global.

---

# 18. IDs

Types distincts :

```rust
pub struct EntityId(u64);
pub struct PlayerId(u32);
pub struct ItemId(u128);
pub struct GameId(u128);

pub struct SkillId(u32);
pub struct StatId(u32);
pub struct MonsterDefId(u32);
pub struct ItemDefId(u32);
pub struct LevelDefId(u32);
pub struct QuestDefId(u32);
```

Un ID runtime n'est jamais confondu avec un ID de définition.

---

# 19. Definitions / Instances

Le modèle suivant est obligatoire partout.

```text
CharacterClassDefinition
CharacterInstance

SkillDefinition
SkillInstance

MonsterDefinition
MonsterInstance

ItemDefinition
ItemInstance

LevelDefinition
LevelInstance

QuestDefinition
QuestInstance
```

Les `Definition` sont dans `GameData`.

Les `Instance` sont dans `GameState`.

---

# 20. Modèle d'entités

Décision : pas d'ECS généraliste externe en v1.

Utilisation de stores spécialisés.

```rust
pub struct EntityStore {
    pub lifecycle: ComponentStore<Lifecycle>,
    pub transform: ComponentStore<Transform>,
    pub collider: ComponentStore<Collider>,
    pub vitals: ComponentStore<Vitals>,
    pub stats: ComponentStore<ResolvedStats>,
    pub actions: ComponentStore<ActionState>,
    pub states: ComponentStore<StateSet>,
}
```

Les monstres, joueurs, summons, objets et missiles utilisent des stores spécialisés quand leurs données divergent fortement.

---

# 21. Collections déterministes

Les structures de simulation doivent utiliser :

```text
Vec
BTreeMap
BTreeSet
sorted SmallVec
indexed stores
```

`HashMap` est autorisé uniquement si son ordre n'est jamais observable.

Avant toute sélection gameplay, les candidats doivent être canonisés.

---

# 22. Coordonnées

Espace fixed-point :

```rust
pub struct WorldPos {
    pub x: i32,
    pub y: i32,
}
```

Convention :

```text
1 tile = 256 unités
```

Distance euclidienne au carré :

```text
dx² + dy²
```

est utilisée lorsque seule une comparaison est nécessaire afin d'éviter les racines carrées.

---

# 23. Collision

Un collider :

```rust
pub struct Collider {
    pub radius: u16,
    pub movement_mask: CollisionMask,
    pub projectile_mask: CollisionMask,
}
```

Catégories :

```text
Terrain
Wall
Door
Actor
Object
ProjectileBlocker
VisionBlocker
```

La collision gameplay ne dépend jamais des sprites.

---

# 24. Spatial index

Décision :

```text
UniformSpatialGrid
```

Taille de cellule définie en unités monde.

Les cellules stockent les `EntityId`.

Avant toute résolution où l'ordre compte :

```text
sort(EntityId)
```

---

# 25. Mouvement

Le client envoie une intention.

```rust
MoveIntent {
    direction,
    movement_mode,
    sequence,
}
```

Le client n'envoie jamais une nouvelle position autoritaire.

Modes :

```text
Walk
Run
Forced
Knockback
Teleport
```

Pipeline :

```text
desired displacement
→ terrain collision
→ entity collision
→ final displacement
→ position update
```

---

# 26. Résolution de collision simultanée

Lorsque plusieurs acteurs veulent occuper le même espace :

1. calculer les candidats ;
2. trier par `EntityId`;
3. appliquer les mouvements dans l'ordre ;
4. chaque résolution observe les positions déjà validées du tick.

Ce comportement privilégie le déterminisme à une résolution physique sophistiquée.

---

# 27. Pathfinding

Algorithme initial :

```text
A*
```

Ordre fixe des voisins :

```text
N, NE, E, SE, S, SW, W, NW
```

Tie-break :

```text
f_score
h_score
node_index
```

Le résultat est donc stable.

---

# 28. Monde

Structure :

```text
World
 └── LevelInstance
      ├── RoomInstance
      ├── CollisionMap
      ├── NavigationMap
      ├── ObjectInstances
      ├── SpawnRegions
      └── Connections
```

---

# 29. Génération procédurale

La génération se fait en deux étapes.

## 29.1 Graphe logique

```text
LevelGraph
├── Entry
├── MandatoryRooms
├── QuestRooms
├── Waypoint
├── Exit
└── OptionalBranches
```

## 29.2 Matérialisation spatiale

```text
graph
→ room templates
→ connectors
→ coordinates
→ collision
→ decoration
→ spawn regions
```

---

# 30. Contraintes de génération

Un générateur doit garantir :

```text
Entry reachable from every mandatory node
Exit reachable from Entry
Waypoint reachable
Quest objective reachable
No mandatory connector blocked
No overlapping mandatory room
```

Une validation BFS/DFS est obligatoire.

---

# 31. Retry de génération

Une génération invalide ne consomme pas arbitrairement la seed principale.

Sous-seed :

```text
derive(level_seed, retry_index)
```

Nombre maximum :

```text
MAX_GENERATION_RETRIES
```

appartient à `ENGINE`.

Si toutes les tentatives échouent :

```text
GameCreationError::WorldGenerationFailed
```

---

# 32. Action State Machine

Chaque acteur possède exactement un `ActionState`.

```rust
pub enum ActorMode {
    Neutral,
    Walk,
    Run,
    Attack,
    Cast,
    Block,
    HitRecovery,
    Stunned,
    Knockback,
    Interact,
    Dead,
}
```

Chaque action possède :

```rust
pub struct ActiveAction {
    pub id: ActionInstanceId,
    pub mode: ActorMode,
    pub start_tick: Tick,
    pub phase: ActionPhase,
    pub target: Option<Target>,
}
```

---

# 33. Phases d'une action

```text
Windup
Impact
Recovery
Complete
```

Timing :

```rust
pub struct ActionTiming {
    pub windup_ticks: u16,
    pub impact_tick: u16,
    pub recovery_ticks: u16,
}
```

---

# 34. Interruptions

Chaque action définit :

```text
interruptible_during_windup
interruptible_after_impact
interruptible_by_stun
interruptible_by_knockback
interruptible_by_hit_recovery
```

Priorité des interruptions :

```text
Death
Stun
Knockback
HitRecovery
PlayerCancel
NewAction
```

Une interruption de priorité inférieure ne remplace pas une interruption supérieure déjà programmée.

---

# 35. Compétences

Les compétences utilisent une représentation intermédiaire typée.

```rust
pub struct SkillDefinition {
    pub id: SkillId,
    pub targeting: TargetingSpec,
    pub requirements: SkillRequirements,
    pub timing: TimingFormula,
    pub cost: CostFormula,
    pub program: SkillProgram,
}
```

---

# 36. IR des compétences

Opcodes autorisés :

```text
Sequence
Conditional
Repeat
RandomChoice

SelectTarget
SelectArea
FilterTarget

DealDamage
Heal
RestoreMana

ApplyState
RemoveState
Dispel

SpawnMissile
SpawnArea
SpawnObject

Summon
Revive
ConsumeCorpse

Teleport
Knockback

ModifyStat

CreateItem
ConsumeItem

TriggerSkill
```

L'IR n'est pas Turing-complet.

Pas de boucle non bornée.

---

# 37. Validation du Skill IR

Au chargement :

- toutes les références existent ;
- les boucles ont une borne ;
- les coûts ne sont pas négatifs sauf autorisation ;
- les récursions `TriggerSkill` sont acycliques ou explicitement limitées ;
- les sélections ont une taille maximale ;
- chaque opcode est supporté.

Une compétence invalide empêche le chargement du datapack.

---

# 38. Native Effects

Les effets impossibles ou déraisonnables à décrire avec l'IR peuvent utiliser :

```rust
EffectOp::Native(NativeEffectId)
```

Condition :

- justification documentée ;
- test unitaire dédié ;
- entrée/sortie déterministe ;
- aucune I/O ;
- aucun RNG implicite.

Objectif :

```text
>= 90 % des compétences sans NativeEffect
```

---

# 39. Stats : modèle source

Les statistiques ne sont jamais stockées uniquement comme valeurs finales.

Chaque contribution conserve :

```rust
pub struct StatModifier {
    pub stat: StatId,
    pub source: ModifierSource,
    pub operation: ModifierOp,
    pub value: i64,
    pub priority: i16,
}
```

---

# 40. Ordre normatif des modificateurs

Pipeline :

```text
Base
→ FlatAdd
→ PercentAdd
→ Multiplicative
→ Override
→ ClampMin
→ ClampMax
→ DerivedCalculation
```

Les modificateurs de même étape sont ordonnés :

```text
priority
source stable ID
modifier sequence
```

---

# 41. Opérations de stats

```rust
pub enum ModifierOp {
    FlatAdd,
    PercentAddBp,
    MultiplyBp,
    Override,
    MinClamp,
    MaxClamp,
}
```

Pourcentage :

```text
10000 basis points = 100 %
```

---

# 42. Stat graph

Les statistiques dérivées déclarent leurs dépendances.

Exemple :

```text
Strength ───┐
            ├→ PhysicalDamageBonus
Weapon ─────┘

Dexterity ──┬→ AttackRating
            └→ BlockChance
```

Le graphe doit être acyclique.

Détection de cycle au chargement.

---

# 43. Cache des stats

Version initiale :

```text
recalcul complet lors d'un changement pertinent
```

Optimisation future :

```text
dirty dependency propagation
```

Le cache n'est jamais sérialisé dans un replay ou une sauvegarde.

---

# 44. Arithmétique

Calculs :

```text
i32/i64/u32/u64
```

Les produits pouvant dépasser 32 bits utilisent `i64/u64`.

Overflow :

```text
checked en debug
saturating uniquement si règle explicitement définie
```

Aucun overflow wrapping gameplay implicite.

---

# 45. Arrondis

Règle par défaut :

```text
division entière vers zéro
```

Chaque formule peut déclarer :

```text
Floor
Ceil
TowardZero
Nearest
```

Une formule ne doit pas dépendre d'un arrondi accidentel du langage.

---

# 46. Combat

Pipeline :

```text
AttackEligibility
→ TargetValidation
→ HitResolution
→ BlockResolution
→ DamageConstruction
→ OffensiveModifiers
→ DefensiveModifiers
→ ResistanceResolution
→ AbsorbResolution
→ VitalDelta
→ Leech
→ HitReaction
→ TriggerResolution
→ PendingDeath
```

---

# 47. Chance de toucher

Ruleset D2-like initial :

```text
CTH =
2
× AR / (AR + Defense)
× attacker_level / (attacker_level + defender_level)
```

Clamp :

```text
5 % .. 95 %
```

Cette formule appartient au ruleset, pas à `ENGINE`.

---

# 48. Bloc

Le bloc possède :

```text
chance
recovery duration
allowed attack classes
movement penalty
```

Ruleset initial :

```text
max block = 75 %
running effectiveness = 1/3
```

---

# 49. Types de dégâts

```rust
pub struct DamagePacket {
    pub physical: DamageRange,
    pub magic: DamageRange,
    pub fire: DamageRange,
    pub cold: DamageRange,
    pub lightning: DamageRange,
    pub poison: Option<PoisonPayload>,
}
```

Chaque type peut être nul.

---

# 50. Dégâts secondaires

Support obligatoire :

```text
CriticalStrike
DeadlyStrike
CrushingBlow
OpenWounds
Knockback
LifeLeech
ManaLeech
Reflect
Thorns
PreventHealing
```

Chaque mécanisme possède son propre module.

Ils ne sont pas implémentés comme simples « affixes spéciaux » dispersés.

---

# 51. Résistances

Stats séparées :

```text
PhysicalResist
MagicResist
FireResist
ColdResist
LightningResist
PoisonResist
```

et plafonds :

```text
MaxFireResist
MaxColdResist
...
```

Ruleset initial :

```text
elemental max = 75 %
minimum = -100 %
physical reduction max = 50 %
```

---

# 52. Immunités

Par défaut :

```text
resistance >= 100 % => immune
```

Les effets réduisant les résistances définissent :

```rust
pub struct ResistanceReduction {
    pub amount_bp: i32,
    pub can_break_immunity: bool,
    pub immunity_effectiveness_bp: i32,
}
```

La règle exacte appartient au ruleset.

---

# 53. Poison / DoT

Un DoT conserve un accumulateur fixed-point.

```rust
pub struct DamageOverTime {
    pub total_damage_fp: i64,
    pub remaining_ticks: u32,
    pub accumulator: i64,
}
```

Politique de stacking :

```text
Replace
Refresh
Stack
HighestWins
Independent
```

définie par effet.

---

# 54. États

```rust
pub struct StateInstance {
    pub state: StateId,
    pub source: EntityId,
    pub source_skill: Option<SkillId>,
    pub applied_tick: Tick,
    pub expires_tick: Option<Tick>,
    pub stack_key: StackKey,
}
```

Chaque état possède une politique :

```text
refresh
stack
replace
strongest
unique-by-source
```

---

# 55. Auras

Une aura est un producteur d'états.

Le tick d'aura appartient aux données :

```text
refresh_interval
radius
target_filter
state
```

Pas de constante globale obligatoire à 5 ticks.

---

# 56. Hit Recovery / Cast / Block / Attack speed

Les systèmes suivants sont séparés :

```text
AttackSpeed
CastSpeed
BlockRecovery
HitRecovery
```

Ils peuvent utiliser des courbes de rendement décroissant communes mais des paramètres distincts.

Ils ne dépendent pas des animations client.

---

# 57. Missiles

```rust
pub struct MissileInstance {
    pub entity: EntityId,
    pub definition: MissileId,
    pub owner: EntityId,
    pub source_skill: Option<SkillId>,
    pub position: WorldPos,
    pub velocity: FixedVec2,
    pub lifetime: u16,
}
```

Mouvements :

```text
Linear
Homing
Accelerating
Stationary
Orbit
ScriptedFinite
```

Hooks :

```text
OnSpawn
OnTick
OnHit
OnExpire
```

---

# 58. Piercing

Le missile conserve :

```text
hit_entities
remaining_pierces
```

La même entité ne peut pas être touchée deux fois par le même missile sauf si sa définition l'autorise.

---

# 59. IA

Architecture choisie :

```text
Hierarchical Finite State Machine
```

et non behavior tree général.

Raison :

- plus simple ;
- plus prévisible ;
- plus facile à tester ;
- adapté à un ARPG D2-like.

---

# 60. Pipeline IA

```text
Perception
→ Blackboard update
→ State transition
→ Intent generation
```

L'IA produit des intentions.

Elle ne modifie pas directement le monde.

---

# 61. AI Blackboard

```rust
pub struct AiBlackboard {
    pub current_target: Option<EntityId>,
    pub last_known_target_pos: Option<WorldPos>,
    pub last_damage_source: Option<EntityId>,
    pub home_position: WorldPos,
    pub state_entered_tick: Tick,
}
```

---

# 62. Fréquence IA

Chaque définition possède :

```text
think_interval_ticks
```

Le scheduling est déphasé :

```text
(entity_id % think_interval)
```

pour éviter que tous les monstres réfléchissent au même tick.

---

# 63. Ciblage IA

Ordre :

1. construire la liste des cibles valides ;
2. calculer le score ;
3. trier par score descendant ;
4. `EntityId` comme tie-break.

Un RNG ne doit intervenir que lorsque la définition demande explicitement un choix aléatoire.

---

# 64. Monster packs

Support natif :

```text
PackLeader
Minion
ChampionPack
UniquePack
BossEncounter
```

Un pack possède :

```text
leader
members
formation policy
aggro linkage
```

---

# 65. Champions et uniques

Les modificateurs de monstres sont composables :

```text
ExtraFast
ExtraStrong
Resistant
Aura
Multishot
Teleport
Cursed
etc.
```

Ils sont décrits par données lorsque possible.

---

# 66. Summons

Une invocation possède :

```text
owner
controller
summon_family
slot_index
lifetime
```

Limites configurables par famille.

Replacement policy :

```text
Reject
ReplaceOldest
ReplaceWeakest
```

---

# 67. Mercenaires

Les mercenaires utilisent :

```text
Stats
Actions
Skills
States
AI
Inventory
```

plus :

```text
owner
experience
persistent equipment
death state
revive cost
```

Ils persistent avec le personnage.

---

# 68. Objets

```rust
pub struct ItemInstance {
    pub id: ItemId,
    pub definition: ItemDefId,
    pub quality: ItemQuality,
    pub item_level: u16,
    pub generation_seed: [u8; 32],
    pub affixes: SmallVec<[AffixId; 6]>,
    pub sockets: SmallVec<[ItemId; 6]>,
    pub durability: Option<u16>,
    pub flags: ItemFlags,
}
```

---

# 69. Reproductibilité d'un objet

Les propriétés générées d'un item doivent pouvoir être reproduites avec :

```text
datapack hash
item definition
generation seed
generation context
```

sans rejouer toute la partie.

---

# 70. Qualités

```text
Low
Normal
Superior
Magic
Rare
Set
Unique
Crafted
```

Le moteur n'est pas limité à ces qualités.

---

# 71. Loot pipeline

```text
DropContext
→ TreasureClass
→ BaseItem
→ ItemLevel
→ Quality
→ Unique/Set resolution
→ Affixes
→ Sockets
→ Durability
→ Derived properties
→ ItemInstance
```

Chaque étape utilise une seed dérivée du `DropSeed`.

---

# 72. Treasure Classes

Une TC :

```rust
pub struct TreasureClass {
    pub picks: i16,
    pub no_drop_weight: u32,
    pub entries: Vec<WeightedTreasureEntry>,
}
```

Validation :

```text
weights > 0
no missing refs
no unbounded cycle
bounded recursion depth
```

---

# 73. Magic Find

`MagicFind` intervient uniquement sur la sélection de qualité.

La fonction effective est fournie par `GameRules`.

Le moteur ne connaît pas une formule fixe universelle.

---

# 74. Affixes

Un affixe possède :

```text
required level
affix level
frequency
group
allowed item types
excluded types
properties
```

Contraintes :

```text
Magic:
  1 ou 2 affixes

Rare:
  max 3 prefixes
  max 3 suffixes

Un seul affixe par group
```

configurables dans le ruleset.

---

# 75. Identified / Unidentified

Un objet peut être :

```text
Identified
Unidentified
```

Les propriétés réelles sont déjà générées côté serveur.

Le client ne les reçoit pas tant que l'objet n'est pas identifié.

---

# 76. Requirements

Équiper un objet peut exiger :

```text
level
strength
dexterity
class
skill
```

Validation exclusivement serveur.

---

# 77. Dual wield et deux mains

Les emplacements d'équipement déclarent :

```text
main hand
off hand
two handed occupancy
dual wield eligibility
```

Un objet deux mains réserve les deux slots sauf exception explicite de classe/règle.

---

# 78. Weapon swap

Deux loadouts :

```text
Primary
Secondary
```

Le swap est une action gameplay.

Il recalcule les stats une seule fois après la transaction complète.

---

# 79. Charges

Un objet peut fournir :

```text
skill
current charges
maximum charges
```

Les charges sont persistantes.

`Recharge` chez un marchand agit sur ces données.

---

# 80. Inventaires

Conteneurs :

```text
Backpack
Equipment
Belt
Cube
Stash
Cursor
Trade
HirelingEquipment
Ground
```

Dimensions définies dans `GameRules`.

---

# 81. Stash

Décision moderne :

le moteur supporte un nombre arbitraire de pages.

Le ruleset initial utilise :

```text
10 × 10
4 pages
```

Ce n'est pas un invariant `ENGINE`.

---

# 82. Belt

Le belt possède des slots typés.

Les potions peuvent être utilisées directement depuis :

```text
belt
inventory
```

selon les règles définies.

---

# 83. Potions

Catégories :

```text
Health
Mana
Rejuvenation
Antidote
Thawing
Stamina
```

Une potion peut appliquer :

```text
instant value
value over time
state
resistance modifier
duration
```

---

# 84. Item ownership

```rust
pub enum ItemLocation {
    PlayerInventory(PlayerId, GridPos),
    Equipment(PlayerId, EquipmentSlot),
    Belt(PlayerId, BeltSlot),
    Stash(PlayerId, StashPos),
    Cube(PlayerId, GridPos),
    Trade(TradeId, TradeSide),
    Hireling(PlayerId, HirelingSlot),
    Ground(LevelInstanceId, WorldPos),
}
```

Un `ItemId` possède exactement une `ItemLocation`.

---

# 85. Transaction inventaire

```text
validate source
validate ownership
validate destination
validate requirements
reserve
apply all mutations
recompute stats
emit event
```

Une erreur avant `apply` ne produit aucune mutation.

---

# 86. Ramassage simultané

Ordre canonique :

```text
execute_tick
player_slot
command_sequence
```

Le premier pickup valide acquiert l'objet.

Les suivants obtiennent :

```text
ItemUnavailable
```

---

# 87. Loot ownership

Modes supportés :

```text
FreeForAll
RoundRobin
Instanced
```

`FreeForAll` est le défaut du ruleset D2-like.

`LootMode` fait partie de `GameRules`.

---

# 88. Expiration au sol

Chaque catégorie d'objet définit :

```text
ground_lifetime_ticks
```

Une durée `None` signifie pas d'expiration pendant la durée de la partie.

Les items persistants ne sont pas sauvegardés lorsqu'ils sont au sol sauf règle explicitement activée.

---

# 89. Sets

Les bonus de set utilisent :

```text
equipped_piece_count
specific_piece_conditions
partial bonuses
full bonus
```

Ils sont recalculés après transaction d'équipement.

---

# 90. Uniques

Le moteur autorise par défaut plusieurs instances du même unique dans une partie.

Une limitation d'unicité peut être introduite comme `GameRule`.

---

# 91. Sockets

Un objet socketé contient les `ItemId` insérés.

Retrait :

```text
impossible
destructif
récupérable
```

selon la recette utilisée.

---

# 92. Runewords

Validation :

```text
compatible base type
exact number of sockets
correct rune count
correct order
allowed item quality
allowed ruleset
```

Un runeword ne dépend pas du nom d'un objet.

---

# 93. Cube / recettes

Moteur générique.

```rust
pub struct RecipeDefinition {
    pub inputs: Vec<ItemPredicate>,
    pub outputs: Vec<RecipeOutput>,
    pub constraints: Vec<RecipeConstraint>,
}
```

Predicates :

```text
ExactItem
ItemType
Quality
Affix
Rune
Gem
SocketCount
Ethereal
Identified
Quantity
QuestFlag
```

---

# 94. Or

L'or est une ressource numérique et non un item standard dans les inventaires.

```text
carried_gold
stash_gold
```

Une pile au sol est un `GroundCurrency`.

Les limites appartiennent au ruleset.

---

# 95. Marchands

Services supportés :

```text
Buy
Sell
Repair
Recharge
Gamble
Hire
ResurrectHireling
```

Le stock est une vue serveur.

Décision v1 :

```text
stock personnel au joueur
```

Cela supprime les conflits inutiles entre joueurs.

---

# 96. Refresh marchand

Déclencheurs possibles :

```text
level transition
explicit refresh event
timer policy
```

Le ruleset choisit.

Le refresh utilise :

```text
Merchant(NpcId, RefreshSequence)
```

comme domaine RNG.

---

# 97. Gamble

La définition réelle de l'objet est inconnue du client avant achat.

Le serveur transmet uniquement :

```text
base appearance/category
price
slot
```

La qualité et les propriétés restent côté serveur.

---

# 98. Objets interactifs

Architecture commune :

```text
Chest
Barrel
Urn
Door
Shrine
Well
Waypoint
QuestObject
GenericInteractable
```

Chaque objet définit :

```text
interaction distance
state machine
cooldown
loot generator
effects
```

---

# 99. Sanctuaires

Un shrine définit :

```text
effect
duration
respawn/recharge policy
eligibility
```

L'effet utilise le système normal de `StateInstance`.

---

# 100. Corps de monstres

Les monstres morts peuvent laisser :

```text
CorpseEntity
```

si leur définition l'autorise.

Un corpse possède :

```text
original monster type
position
owner metadata
consumed flag
```

Il permet :

```text
revive
corpse explosion
summoning
corpse consumption
```

---

# 101. Quêtes

Architecture :

```text
QuestDefinition
├── Variables
├── Flags
├── Objectives
├── Triggers
├── Conditions
└── Actions
```

---

# 102. Trigger de quête

Triggers supportés :

```text
AreaEntered
MonsterKilled
BossKilled
NpcInteracted
ObjectActivated
ItemObtained
ItemConsumed
RecipeCompleted
PartyEvent
CustomEvent
```

---

# 103. Conditions de quête

```text
HasFlag
NotFlag
VariableEquals
VariableAtLeast
HasItem
ClassIs
DifficultyIs
InParty
AreaIs
QuestStateIs
```

---

# 104. Actions de quête

```text
SetFlag
ClearFlag
SetVariable
IncrementVariable
GrantExperience
GrantItem
UnlockWaypoint
UnlockArea
SpawnObject
SpawnMonster
OpenPortal
CompleteObjective
CompleteQuest
```

---

# 105. État personnel / partie

Séparation obligatoire :

```text
CharacterQuestState
GameQuestState
```

Exemple :

```text
porte ouverte dans cette partie
    → GameQuestState

récompense déjà reçue
    → CharacterQuestState
```

---

# 106. Événements de quête multijoueur

Un événement n'est évalué qu'une fois par quête et par tick.

Le moteur construit :

```text
QuestEvent
```

puis évalue chaque joueur éligible.

Cela évite les doubles validations lorsque plusieurs membres déclenchent le même événement.

---

# 107. Waypoints

État persistant par :

```text
character × difficulty
```

Un joueur ne peut utiliser que ses waypoints débloqués.

La destination doit appartenir au même ruleset et difficulté.

---

# 108. Town Portals

Un portail possède :

```text
owner
source
destination
access_policy
created_tick
```

Policies :

```text
OwnerOnly
Party
Everyone
```

Le ruleset D2-like utilise `Party`.

---

# 109. Party

```rust
pub struct Party {
    pub id: PartyId,
    pub leader: PlayerId,
    pub members: BTreeSet<PlayerId>,
}
```

Opérations :

```text
Invite
Accept
Decline
Leave
Kick
```

Maximum égal à `GameRules.max_players`.

---

# 110. XP multijoueur

Pipeline :

```text
monster base XP
→ difficulty modifier
→ multiplayer pool modifier
→ participant eligibility
→ party distribution
→ level-difference modifier
→ player XP modifier
```

Chaque étape est configurable.

---

# 111. Scaling monstres

Le moteur expose :

```rust
pub struct MonsterScalingRules {
    pub health: ScalingCurve,
    pub damage: ScalingCurve,
    pub accuracy: ScalingCurve,
    pub density: ScalingCurve,
    pub elite_frequency: ScalingCurve,
    pub experience: ScalingCurve,
    pub loot: ScalingCurve,
}
```

Aucun `+6 % damage/player` n'est un invariant du moteur.

Le ruleset D2-like choisit ses valeurs.

---

# 112. Moment du scaling

Les caractéristiques d'un monstre sont fixées à son spawn.

Le départ ou l'arrivée d'un joueur ne modifie pas rétroactivement ses stats.

---

# 113. PvP

Modes :

```text
Disabled
Consent
Hostility
Arena
```

Le mode fait partie du ruleset.

---

# 114. Relations joueurs

```text
Neutral
Party
Hostile
```

Dommages :

```text
Party    → non
Neutral  → non
Hostile  → oui
```

sauf effet explicitement autorisé.

---

# 115. PvP scaling

Le PvP possède ses propres multiplicateurs.

```rust
pub struct PvpRules {
    pub damage_scale_bp: i32,
    pub life_leech_scale_bp: i32,
    pub crowd_control_scale_bp: i32,
}
```

L'équilibrage PvE ne change donc pas indirectement le PvP.

---

# 116. Trade

Machine :

```text
Requested
Open
LockedA
LockedB
Persisting
Committed
Cancelled
```

Toute modification d'offre replace l'état à :

```text
Open
```

---

# 117. Commit trade

Le trade doit valider atomiquement :

```text
ownership
items exist
gold
inventory capacity
item restrictions
character revisions
```

Puis lancer une transaction de persistance.

Le client ne reçoit `TradeCommitted` qu'après succès de cette transaction.

---

# 118. Échec persistence pendant trade

Si la transaction persistante échoue :

```text
simulation trade mutation rollback
trade state → Open ou Cancelled
client reçoit PersistenceFailure
```

Le commit ne peut jamais être confirmé seulement en mémoire.

---

# 119. Persistance

Séparation :

```text
PersistentCharacter
PersistentAccount
EphemeralGameState
```

Ne sont pas persistés par défaut :

```text
monstres
missiles
loot au sol
états temporaires
map runtime
portails
```

---

# 120. Snapshot personnage

```rust
pub struct CharacterSnapshot {
    pub schema_version: u32,
    pub revision: u64,

    pub class: ClassId,
    pub level: u16,
    pub experience: u64,

    pub allocated_stats: StatAllocation,
    pub skills: SkillAllocation,

    pub inventory: PersistentInventory,
    pub quests: PersistentQuestState,
    pub waypoints: WaypointState,
    pub hireling: Option<HirelingSnapshot>,
}
```

---

# 121. Repository

```rust
pub trait CharacterRepository {
    async fn load(...);
    async fn save(...);
    async fn transact(...);
}
```

Implémentations :

```text
SQLite
PostgreSQL
```

SQLite :

```text
développement
single-server
```

PostgreSQL :

```text
production multi-processus
```

---

# 122. Concurrence de sauvegarde

Chaque personnage possède :

```text
revision
```

Écriture :

```sql
UPDATE ...
WHERE character_id = ?
AND revision = ?
```

Puis :

```text
revision = revision + 1
```

Un conflit produit :

```text
RevisionConflict
```

---

# 123. ItemId global

`ItemId` est globalement unique.

Recommandation :

```text
UUIDv7 / identifiant 128-bit équivalent
```

La génération d'un `ItemId` n'influence jamais le gameplay RNG.

Le stockage impose une contrainte d'unicité.

---

# 124. Protocole réseau

Transport :

```text
QUIC
```

Bibliothèque cible :

```text
quinn
```

Sérialisation protocolaire :

```text
Protocol Buffers via prost
```

Raison :

- versionnement clair ;
- champs optionnels ;
- compatibilité ascendante ;
- outils externes ;
- séparation des structs Rust runtime.

---

# 125. Handshake

Séquence :

```text
ClientHello
→ ServerHello
→ Authentication
→ CharacterSelection
→ JoinGameRequest
→ JoinGameAccepted
→ InitialSnapshot
→ ClientReady
→ Running
```

Le serveur refuse la connexion si :

```text
protocol incompatible
ruleset incompatible
datapack incompatible
character incompatible
game full
```

---

# 126. ClientHello

Contient :

```text
protocol_version
client_build
supported_features
```

Pas de donnée gameplay autoritaire.

---

# 127. JoinGameAccepted

Contient :

```text
game_id
player_id
player_slot
tick_rate
ruleset_hash
datapack_hash
server_tick
input_delay
```

---

# 128. États de session

```text
Connecting
Authenticated
Joining
Synchronizing
Running
DisconnectedGrace
Closed
```

Une commande gameplay n'est acceptée que dans `Running`.

---

# 129. Client commands

```rust
pub struct CommandEnvelope {
    pub sequence: u32,
    pub client_tick: Tick,
    pub command: ClientCommand,
}
```

Le client n'impose jamais `execute_tick`.

---

# 130. Scheduling serveur

Le serveur traduit :

```text
client_tick
```

vers le temps serveur à l'aide de la synchronisation estimée.

Puis :

```text
execute_tick =
max(
    earliest_allowed_tick,
    translated_client_tick
)
```

Le résultat est inscrit dans `ScheduledCommand`.

---

# 131. Input delay

Politique dynamique bornée :

```text
minimum = 1 tick
default = 2 ticks
maximum = 4 ticks
```

La valeur peut être adaptée à partir de :

```text
RTT
jitter
packet loss
```

mais reste stable sur de courtes fenêtres afin d'éviter des changements permanents de sensation.

---

# 132. Commandes obsolètes

Une commande peut être :

```text
Accepted
Deferred
RejectedTooOld
RejectedInvalid
Duplicate
```

Les commandes d'inventaire et de trade ne sont jamais rétroactivement simulées.

---

# 133. Déduplication

Le serveur conserve :

```text
last accepted sequence
recent sequence window
```

par joueur.

Une commande dupliquée est ignorée.

Une séquence trop ancienne est rejetée.

---

# 134. Canonisation des commandes

Ordre gameplay :

```text
execute_tick
player_slot
sequence
```

Le timing réel d'arrivée réseau n'a aucune influence après admission.

---

# 135. Messages fiables

Stream fiable pour :

```text
handshake
join/leave
inventory
trade
quest updates
chat
character management
resync
```

---

# 136. Datagrams

Utilisables pour :

```text
movement command
frequent combat command
entity movement delta
non-critical transient state
```

Perte autorisée.

---

# 137. Réplication : séparation des états

Trois modèles distincts :

```text
AuthoritativeState
ReplicatedState
PresentationState
```

`AuthoritativeState` n'est jamais sérialisé directement sur le réseau.

---

# 138. DTO réseau

Exemples :

```text
PlayerView
MonsterView
MissileView
GroundItemView
ObjectView
QuestView
```

Ils ne contiennent que les données nécessaires au client.

---

# 139. Informations serveur privées

Ne doivent pas être transmises avant nécessité :

```text
RNG state
futur loot
gambling result
contenu coffre fermé
AI target scores
AI blackboard
monstres hors interest set
affixes d'un item non identifié
quest internals non visibles
```

---

# 140. Interest management

Interest set construit à partir de :

```text
current level
current room
adjacent rooms
distance
party metadata
personal quest state
```

Une entité sortant de l'interest set produit :

```text
EntityOutOfScope
```

et non nécessairement un `despawn`.

---

# 141. Entity revisions

Chaque vue répliquée possède :

```text
revision
```

Delta :

```text
entity_id
base_revision
new_revision
field_mask
values
```

---

# 142. Resynchronisation

Si :

```text
base_revision != client_revision
```

le client demande :

```text
EntityResyncRequest
```

Le serveur renvoie l'état complet.

---

# 143. Snapshot global

Un snapshot contient :

```text
snapshot_id
tick
player authoritative state
visible entities
visible objects
visible ground items
quest view
party view
last_processed_command_sequence
```

---

# 144. Prediction client

Prediction autorisée :

```text
local movement only
```

Le combat reste présenté de façon spéculative côté client si désiré, mais n'est jamais confirmé avant retour serveur.

---

# 145. Réconciliation

Après snapshot :

```text
set authoritative position
remove acknowledged commands
replay unacknowledged movement inputs
```

Aucun rollback serveur en v1.

---

# 146. Reconnexion

Policy initiale :

```text
30 secondes
```

modifiable.

Pendant `DisconnectedGrace` :

- le personnage reste dans le monde ;
- il n'effectue aucune nouvelle action ;
- les actions déjà engagées se terminent selon les règles normales ;
- il peut être attaqué ;
- il reçoit XP/quête uniquement si les conditions normales sont remplies.

---

# 147. Déconnexion définitive

Après expiration :

```text
cancel cancellable actions
remove actor from world
save character
release session
```

Les monstres ne disparaissent pas.

---

# 148. Partie

Cycle :

```text
Creating
Lobby
Running
EmptyGrace
Closing
Closed
```

---

# 149. EmptyGrace

Durée = `ServerPolicy`.

Aucun gameplay ne dépend de sa valeur.

À expiration :

```text
save remaining characters
destroy GameState
```

---

# 150. GameConfig

```rust
pub struct GameConfig {
    pub ruleset: RulesetId,
    pub datapack: DataPackHash,
    pub difficulty: DifficultyId,
    pub seed: RootSeed,
}
```

Les paramètres gameplay ne sont pas dispersés directement dans `GameConfig`.

Ils sont résolus dans `GameRules`.

---

# 151. Datapack

Pipeline :

```text
sources
→ importer
→ normalized source model
→ semantic validation
→ compilation
→ compressed datapack
```

---

# 152. Sources de datapack

Format d'édition recommandé :

```text
TOML
CSV
JSON
```

selon la nature des tables.

Le runtime ne dépend pas du format source.

---

# 153. Format compilé

Décision :

```text
postcard
+
zstd
+
manifest
+
checksum
```

Pourquoi acceptable ici :

le format compilé est reconstruit à chaque évolution de `DataSchemaVersion`.

La compatibilité historique du binaire datapack n'est pas nécessaire.

---

# 154. Manifeste

```rust
pub struct DataPackManifest {
    pub data_schema_version: u32,
    pub content_revision: u32,
    pub compiler_version: u32,
    pub content_hash: [u8; 32],
}
```

---

# 155. Hot reload

Interdit dans une partie active.

Nouveau datapack :

```text
nouvelles parties uniquement
```

---

# 156. Ruleset

```rust
pub struct GameRules {
    pub max_players: u8,

    pub character: CharacterRules,
    pub combat: CombatRules,
    pub death: DeathRules,
    pub loot: LootRules,
    pub party: PartyRules,
    pub pvp: PvpRules,
    pub difficulty: DifficultyRules,
    pub monsters: MonsterScalingRules,
    pub inventory: InventoryRules,
}
```

---

# 157. Versionnement

Versions séparées :

```text
EngineVersion
ProtocolVersion
DataSchemaVersion
RulesSchemaVersion
SaveSchemaVersion
ReplayVersion
```

Aucun numéro global unique.

---

# 158. Replay

```rust
pub struct ReplayHeader {
    pub replay_version: u32,
    pub engine_version: EngineVersion,
    pub datapack_hash: DataPackHash,
    pub ruleset_hash: RulesetHash,
    pub root_seed: RootSeed,
}
```

Puis :

```text
initial character snapshots
ordered accepted commands
session transitions affecting gameplay
```

---

# 159. Replay et réseau

Le replay enregistre les commandes **après validation/scheduling serveur**.

Il ne dépend donc pas :

- du jitter ;
- de la duplication réseau ;
- de la retransmission ;
- de QUIC.

---

# 160. State hash

```text
BLAKE3(canonical_game_state)
```

Inclure :

```text
all gameplay state
RNG sequences
scheduler
quests
item locations
```

Exclure :

```text
metrics
logs
network handles
wall clock
cache derivable
```

---

# 161. Sérialisation canonique

Toute map est triée par clé.

Toute collection d'entités est triée par ID.

Aucun padding mémoire ou représentation `repr(Rust)` n'est hashé directement.

---

# 162. Événements gameplay

```rust
pub enum GameEvent {
    EntitySpawned,
    EntityRemoved,

    ActionStarted,
    ActionInterrupted,
    ActionResolved,

    DamageApplied,
    HealingApplied,
    StateApplied,
    StateExpired,

    EntityKilled,

    ItemGenerated,
    ItemDropped,
    ItemPickedUp,
    ItemTransferred,

    QuestChanged,

    PlayerJoined,
    PlayerDisconnected,
    PlayerRemoved,
}
```

---

# 163. Ownership des mutations

Chaque donnée possède un système propriétaire.

Exemple :

```text
Position          MovementSystem
Vitals            EffectSystem
Lifecycle         DeathSystem
Inventory         InventorySystem
QuestState        QuestSystem
```

Un autre système ne modifie pas directement la donnée.

Il soumet une requête au système propriétaire.

---

# 164. Command/Request internes

Exemple :

```text
CombatSystem
→ DamageRequest

SkillSystem
→ SpawnRequest

DeathSystem
→ LootRequest

QuestSystem
→ RewardRequest
```

Les requêtes sont traitées dans leur phase canonique.

---

# 165. Pas d'event sourcing intégral

Le moteur n'est pas entièrement event-sourced.

`GameState` reste l'autorité.

Les événements servent :

- réplication ;
- observabilité ;
- replay diagnostic.

Ils ne constituent pas la base de données primaire du monde.

---

# 166. Logging

Utiliser :

```text
tracing
```

Contexte :

```text
game_id
tick
player_id
entity_id
command_sequence
```

TRACE gameplay désactivé par défaut.

---

# 167. Trace combat

Mode debug détaillé :

```text
Attack #18291
Source     Player:5
Target     Monster:1842

Hit:
  AR          4240
  Defense     1904
  Chance      8142 bp
  Roll        6314
  Result      Hit

Damage:
  Physical raw  162
  Resistance     20 %
  Final          129
```

---

# 168. Trace loot

```text
DeathEvent 2238
Monster 1842
DropSeed ...

TC ...
→ entry ...
→ base ...
→ quality ...
→ affix ...
→ ItemId ...
```

Un item doit être auditable.

---

# 169. Outils administratifs

Commandes de debug :

```text
spawn
give_item
give_skill
teleport
set_stat
kill
complete_quest
dump_entity
dump_rng
```

Elles produisent une `AdminCommand`.

Les parties de test peuvent les enregistrer dans les replays.

---

# 170. Sécurité

Toute entrée externe est non fiable.

Validation :

```text
packet length
array length
enum value
sequence
ownership
distance
requirements
state
rate
```

Aucune allocation proportionnelle à une taille client non bornée.

---

# 171. Rate limiting

Policies séparées :

```text
network messages/sec
commands/sec
inventory operations/sec
trade operations/sec
chat messages/sec
connection attempts/sec
```

Le rate limiting réseau ne modifie jamais le résultat d'une commande déjà acceptée.

---

# 172. Anti-duplication

Contraintes simultanées :

```text
unique ItemId
single ItemLocation
atomic inventory transactions
atomic cross-character persistence
revision locking
```

Un échec d'invariant déclenche :

```text
InternalInvariantError
```

et aucune tentative de « réparation silencieuse ».

---

# 173. Erreurs

```rust
pub enum GameError {
    InvalidCommand,
    InvalidState,
    InsufficientResource,
    OutOfRange,
    InvalidTarget,
    ItemUnavailable,
    InventoryFull,
    RequirementNotMet,
}
```

Séparer :

```text
ClientError
GameRuleError
InternalInvariantError
InfrastructureError
```

---

# 174. Panics

Un `panic!` est acceptable uniquement pour :

```text
invariant impossible
corruption mémoire logique
bug interne
```

Jamais pour une commande client invalide.

---

# 175. Performance : machine de référence

Les benchmarks doivent publier :

```text
CPU model
physical cores
RAM
OS
Rust version
build profile
datapack hash
scenario hash
```

Sans cela, un objectif `100 parties` n'est pas significatif.

---

# 176. Cibles performance

Sur la machine de référence :

```text
100 parties actives
8 joueurs max
25 Hz
```

Budget simulation par partie :

```text
p50 < 3 ms
p95 < 8 ms
p99 < 15 ms
hard budget = 40 ms
```

---

# 177. Mémoire

Mesures :

```text
base process
RAM/game idle
RAM/game combat
RAM/player
RAM/entity
```

Objectif initial :

```text
< 10 MiB / GameInstance
```

hors datapack partagé.

Cette valeur est une cible, pas un invariant.

---

# 178. Bande passante

Mesurer :

```text
bytes/player/sec median
bytes/player/sec p95
snapshot size
delta size
```

Objectif initial :

```text
< 50 KiB/s moyen par joueur
```

dans un combat normal.

---

# 179. Load scenarios

Minimum :

```text
8 melee players
8 projectile-heavy players
8 summoner players
dense monster pack
boss + adds
town trading
loot explosion
massive AoE
```

---

# 180. Dégradation CPU

Le serveur ne doit jamais :

```text
skip gameplay phases
drop accepted gameplay commands
change RNG
```

pour rattraper son retard.

Les tâches non gameplay peuvent être réduites :

```text
metrics frequency
debug traces
non-critical replication frequency
```

---

# 181. Tests unitaires

Chaque formule importante possède :

```text
normal case
boundary
zero
maximum
overflow
rounding
```

---

# 182. Tests de propriétés

Propriétés obligatoires :

```text
ItemId unique
exactly one ItemLocation
HP cannot exceed allowed max
no invalid stat cycle
no invalid TC cycle
no invalid skill recursion
no inaccessible mandatory level node
inventory transaction atomic
trade transaction atomic
```

---

# 183. Fuzzing

Cibles :

```text
protocol decoder
datapack parser
inventory operations
recipe matcher
skill IR validator
quest validator
```

Une entrée invalide doit produire une erreur, jamais un panic.

---

# 184. Déterminisme

CI :

```text
same seed
same datapack
same ruleset
same initial state
same scheduled commands
```

doit produire :

```text
same hash at every tick
```

---

# 185. Plateformes de déterminisme

Minimum :

```text
Linux x86-64
Linux ARM64
Windows x86-64
```

Tous doivent produire les mêmes hashes.

---

# 186. Test réseau chaotique

Harness réseau simulant :

```text
latency
jitter
loss
duplication
reordering
disconnect
reconnect
```

Le hash serveur doit rester indépendant des anomalies réseau pour une même séquence de commandes finalement acceptées.

---

# 187. Scénarios multijoueur critiques

Obligatoires :

```text
2 joueurs pickup même objet
2 joueurs tuent même monstre même tick
player disconnect au kill boss
trade + disconnect
trade + inventory full
trade + persistence failure
portal disappears during interaction
skill target dies during windup
missile target dies before impact
summon owner leaves game
quest event triggered by several players
```

---

# 188. Tests persistence

Simuler crash :

```text
avant transaction
pendant transaction
après COMMIT
avant réponse réseau
```

Après redémarrage :

```text
aucun item dupliqué
aucun item perdu après COMMIT
révisions cohérentes
```

---

# 189. Tests statistiques

Utiles pour :

```text
Treasure Classes
quality
affix frequencies
monster packs
gambling
```

Le test compare les résultats au ruleset.

Pas au comportement exact de Diablo II.

---

# 190. Métriques

```text
game_tick_seconds
game_lag_ticks
game_entity_count
game_monster_count
game_missile_count

commands_received
commands_accepted
commands_rejected

network_bytes_sent
network_bytes_received

character_save_seconds
character_save_failures

item_generated
item_transaction_failures
```

Pas de labels `player_id` ou `item_id`.

---

# 191. Observabilité des invariants

Compteurs :

```text
invalid_item_location
state_hash_mismatch
revision_conflict
scheduler_overflow
generation_retry
resync_requested
```

Un invariant violé doit être immédiatement visible.

---

# 192. Dépendances recommandées

Core/simulation :

```text
smallvec
bitflags
thiserror
blake3
rand_chacha
rand_core
```

Data :

```text
serde
toml
csv
postcard
zstd
```

Network :

```text
tokio
quinn
prost
```

Persistence :

```text
sqlx
```

Tests :

```text
proptest
criterion
```

Pas de moteur graphique.

Pas de Bevy côté serveur.

---

# 193. Fonctionnalités v1 obligatoires

Systèmes :

```text
7 classes
skills
stats
combat
missiles
states
monsters
AI
summons
hirelings

items
affixes
sets
uniques
sockets
runes
runewords
cube

inventory
belt
stash
weapon swap

merchants
repair
recharge
gambling

world generation
collision
pathfinding
objects
shrines
waypoints
portals

quests

party
trade
PvP mode support

save
replay
multiplayer
```

---

# 194. Hors périmètre v1

```text
Battle.net
client original
save D2 original
graphismes
audio
guildes
auction house
global matchmaking
server rollback
spectator mode
cross-region simulation
MMO persistent open world
```

---

# 195. Vertical slice

Avant l'implémentation complète du contenu :

```text
2 classes
10 skills
10 monster types
1 champion system
1 boss
1 town
1 procedural zone
1 dungeon
50 base items
30 affixes
5 uniques
10 runes
5 runewords
5 recipes
merchant
waypoint
portal
quest chain
2-player multiplayer
trade
save/reconnect
replay
```

Critère :

deux joueurs peuvent terminer le scénario, obtenir du loot, échanger, quitter, revenir et obtenir le même résultat en replay.

---

# 196. Ordre de réalisation

## M0 — Foundation

```text
IDs
fixed point
tick
scheduler
RNG hierarchy
state hash
GameData/GameRules/GameState
```

## M1 — Network foundation

```text
QUIC
handshake
sessions
command scheduling
snapshot
delta
resync
```

## M2 — World

```text
generation graph
rooms
collision
movement
pathfinding
objects
```

## M3 — Actor model

```text
stats
ActionState
states
movement
death
```

## M4 — Skills/combat

```text
Skill IR
damage
missiles
DoT
auras
summons
```

## M5 — AI/monsters

```text
HFSM
packs
champions
boss
hireling
```

## M6 — Items

```text
loot
TC
affixes
unique/set
inventory
socket
runeword
```

## M7 — Economy

```text
merchant
repair
recharge
gambling
cube
trade
persistence transaction
```

## M8 — Quests/world progression

```text
quest DSL
waypoints
portals
acts
difficulties
```

## M9 — Full reference datapack

```text
7 classes
5-act content
3 difficulties
full item ecosystem
```

## M10 — Performance/security

```text
load
fuzzing
network chaos
profiling
memory optimization
```

---

# 197. Definition of Done d'un système

Un système est terminé uniquement lorsqu'il possède :

```text
specification
implementation
data schema
validation
unit tests
property tests
determinism tests
multiplayer tests si applicable
debug tooling
metrics pertinentes
```

---

# 198. Definition of Done d'une compétence

Une compétence est terminée lorsque sont testés, si applicables :

```text
requirements
mana
timing
targeting
damage
states
missiles
interruptions
death interaction
multiplayer
replay
item modifiers
AI usage
```

---

# 199. Definition of Done d'un item system

Doivent être démontrés :

```text
generation deterministic
properties reproducible
ownership invariant
save/load
trade
drop/pickup
socket interactions
requirements
stat recomputation
```

---

# 200. Documents normatifs supplémentaires

La spec doit être accompagnée de quatre documents maintenus avec le code.

## `GAMEPLAY_SEMANTICS.md`

Contient :

```text
phase ordering
simultaneous effects
death semantics
spawn activation
interruptions
target invalidation
movement conflicts
```

## `DATA_MODEL.md`

Contient les schémas complets :

```text
GameData
GameRules
Skill IR
Stat graph
Quest DSL
Item model
```

## `PROTOCOL.md`

Contient :

```text
handshake
state machine
messages
revisions
ACK
resync
timeouts
compatibility
```

## `PERSISTENCE.md`

Contient :

```text
database schema
revision model
trade transactions
failure semantics
migrations
backups
```

Ces documents sont normatifs, pas seulement informatifs.

---

# 201. Arbitrage final en cas de conflit

Priorité :

```text
1. correction et sécurité des invariants
2. déterminisme
3. cohérence gameplay
4. sécurité multijoueur
5. maintenabilité
6. performance
7. ressemblance avec Diablo II
8. fidélité historique
```

Une bizarrerie historique de Diablo II ne doit jamais l'emporter sur un invariant de sécurité ou de déterminisme.

---

# 202. Résumé architectural définitif

```text
                         DataPack
                            │
                         GameData
                            │
                ┌───────────┴───────────┐
                │                       │
             GameRules              GameState
                │                       │
                └───────────┬───────────┘
                            │
                     GameInstance
                       25 ticks/s
                            │
        ┌───────────────────┼──────────────────┐
        │                   │                  │
     Systems             Scheduler           RNG
        │
        ▼
     Events
        │
  ┌─────┼────────────┬─────────────┐
  ▼     ▼            ▼             ▼
Network Replay    Telemetry      Persistence
   │
   ▼
Clients
```

---

# 203. Positionnement

Le produit est :

> **un moteur ARPG multijoueur serveur, déterministe et data-driven, reprenant les structures de gameplay qui font l'identité de Diablo II tout en utilisant une architecture réseau, une persistance, un RNG, un modèle de données et des outils modernes.**

Le moteur ne cherche pas à conserver les accidents d'implémentation du jeu original.

Il cherche à préserver :

```text
progression
densité de règles
construction de personnage
loot
synergies entre objets et compétences
danger des monstres
exploration procédurale
coopération multijoueur
```

tout en rendant le système :

```text
auditable
testable
déterministe
sécurisé
extensible
performant
maintenable
```