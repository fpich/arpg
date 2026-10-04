# DATA_MODEL

> Document normatif (SPEC.md section 200). Schémas de référence du moteur.
> Source de vérité : `crates/arpg-core`, `crates/arpg-data`, `crates/arpg-sim`.

## GameData (datapack, §72-73, §93)

`arpg_data::GameData` — compilé depuis le datapack de référence
(`compile_reference_datapack`), immuable en partie (INV-008, `Arc<GameData>`) :

- `items: BTreeMap<ItemDefId, ItemDefinition>` — définitions d'objets
  (type, base, niveau requis, trésor).
- `monsters: BTreeMap<MonsterDefId, MonsterDefinition>` — bestiaire, inclut
  `leaves_corpse`, facteurs champion, capacités IA.
- `skills: BTreeMap<SkillId, ...>` — index de définitions ; les programmes
  complets vivent côté `GameInstance` (la data ne dépend pas de la sim).
- tables d'affixes, treasure classes, entrées marchands, recettes
  cube/runewords.
- `datapack_hash` — empreinte BLAKE3 du datapack, échangée au handshake (§125)
  et verrouillée par partie.

## GameRules

`arpg_rules::GameRules` — constantes d'équilibrage immuables
(`Arc<GameRules>`) : vitesses de base, courbes diminishing returns,
paramètres de régénération, coûts. Aucune règle gameplay dans le code de
données ou de réseau (INV-015).

## Skill IR (§36, §46)

`arpg_sim::skill` :

```text
SkillDefinition { id, targeting, cost, timing, program }
SkillProgram  { ops: Vec<SkillOp> }
SkillOp = StartProjectile | DealDamage | SpawnMissile | Heal |
          RestoreMana | ConsumeCorpse | ...
CostFormula   — mana/coûts, résolus à l'admission puis à l'impact
TimingFormula — Ticks(t) | Instant | AttackTicks(t)
TargetingSpec — Position | Entity | Self
```

Exécution : `execute_program(program, intent, spawn_cb, corpse_cb)` — le
moteur reste propriétaire de la création d'entités ; l'IR ne touche jamais
l'état directement. Une op = un effet observable dans `GameEvent`.

## Stat graph (§39-45)

`arpg_sim::stat` :

```text
StatBlock       — conteneur de modificateurs, évalué en stages
StatModifier    { stat: StatId, source, source_sequence, operation, value, priority }
ModifierSource  — Equipment | Skill | Shrine | State | ...
ModifierOp      — FlatAdd | PercentAdd | PercentMul (stages ordonnés)
```

- Contributions déterministes : départage par priorité puis
  (source, source_sequence) ; indépendant de l'ordre d'insertion.
- Les sources taggent leurs modificateurs ; toute recomputation
  (équipement, set bonus, aura) retire puis réinsère ses modificateurs —
  jamais de résidu.
- Les bonus % passent la courbe partagée (`arpg_sim::speeds`) avant
  application.

## Quest DSL (§101-106)

`arpg_sim::quest` :

```text
QuestDefinition { id, name, act, objectives, rules, access }
QuestRule   { trigger, conditions, actions }       — trigger -> conditions -> actions
QuestObjective { ... }                             — objectifs trackés par joueur
QuestAccess — attribution multiplayer (§106) : individuel / parti / premier
QuestSystem — définitions + états par partie et par personnage
```

Triggers : `QuestTrigger` (kill d'id de monstre, zone, level, interaction,
tick). Conditions : prédicats sur l'état de quête/joueur. Actions : avancer
un objectif, marquer complété, ouvrir un waypoint, donner un objet.
Les récompenses passent par l'inventaire transactionnel (jamais de spawn
direct).

## Item model (§68-69, §77-79, §84-88)

`arpg_sim::item` :

```text
ItemInstance {
  id: ItemId, definition: ItemDefId, quality: ItemQuality,
  item_level, generation_seed: [u8;32],
  affixes: SmallVec<u32>, sockets: SmallVec<ItemId>,
  durability: Option<u16>, flags: u32,
  charges: Option<ChargeState>,   // §79 : skill + current + max
  hands: ItemHands,               // §77 : OneHanded | TwoHanded
}
ItemLocation — exactement un emplacement par ItemId (INV-010) :
  PlayerInventory | Equipment | Belt | Stash | Cube | Ground
EquipmentSlot — Head..RingRight + PrimarySet/SecondarySet (§78)
ItemQuality — Low..Crafted (§70)
```

- Génération : propriétés reproductibles de
  (datapack_hash, definition, generation_seed, contexte) sans rejouer la
  partie (§69). Aucun RNG implicite : tout tirage vient de la seed de
  drop (BLAKE3, INV-005).
- Inventaire (`InventorySystem`) : transactions atomiques (§85), emplacement
  unique, occupation two-handed (§77), swap d'armes atomique (§78).
