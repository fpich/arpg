# DATA_MODEL.md

Document normatif (SPEC.md §200). Schémas complets des données du moteur. Toute divergence entre ce document et le code est un bug.

## GameData (arpg-data)

```text
GameData
├── skills: Vec<SkillDef>
└── items: Vec<ItemDefinition>      // id: ItemId, name: String
```

Un datapack est immuable pendant toute la durée d'une partie. Un datapack et un ruleset sont identifiés par leur hash BLAKE3 (`DataPackHash`, `RulesetHash`).

## GameRules (arpg-rules)

```text
GameRules
├── max_players: u8                 // défaut 8
├── difficulty: u32
├── ruleset_hash: [u8; 32]
├── LootMode                        // FreeForAll | RoundRobin | Instanced
└── PvpMode                         // Disabled | Consent | Hostility | Arena
```

## Skill IR (arpg-sim/src/skill.rs)

```text
SkillDefinition
├── id: SkillId
├── targeting: TargetingSpec        // NoTarget | ...
├── cost: CostFormula               // mana_cost, life_cost
├── timing: TimingFormula            // windup, impact, recovery (ticks)
└── program: SkillProgram           // ops: Vec<SkillOp> — IR déterministe
```

Validation : coût non négatif, timing cohérent ; une compétence invalide empêche le chargement du datapack.

## Stat graph (arpg-sim/src/stat.rs)

```text
StatBlock
└── modifiers: Vec<StatModifier>
    ├── op: ModifierOp              // Add | Multiply | Override
    ├── source: ModifierSource      // Skill | Item | State | Shrine
    └── value / stat id
```

Le graphe est recalculé depuis les sources ; le cache n'est jamais sérialisé dans un replay ou une sauvegarde (§1220).

## Item model (arpg-sim/src/item.rs)

```text
ItemInstance
├── id: ItemId (u128, unique)       // ne influence jamais le gameplay RNG
├── definition: ItemDefId
├── quality: ItemQuality            // Low..Crafted
├── item_level: u16
├── generation_seed: [u8; 32]       // propriétés reproductibles sans replay (§69)
├── affixes: SmallVec<[u32; 6]>
├── sockets: SmallVec<[ItemId; 6]>
├── durability: Option<u16>
└── flags: u32

ItemLocation (exactement une location par ItemId, §84)
├── PlayerInventory(PlayerId, GridPos)
├── Equipment(PlayerId, EquipmentSlot)
├── Belt(PlayerId, u8)
├── Stash(PlayerId, StashPos)
├── Cube(PlayerId, GridPos)
└── Ground(LevelInstanceId, WorldPos)

TreasureClass (§72)
├── picks: i16
├── no_drop_weight: u32
└── entries: Vec<WeightedTreasureEntry>   // weight, kind: Nothing|Item|TC
    // validation : poids positifs, refs existantes, pas de cycle non borné,
    // profondeur ≤ MAX_TC_DEPTH (16)
```

## Inventory model (arpg-sim/src/inventory.rs)

```text
InventorySystem
├── items: BTreeMap<ItemId, ItemInstance>
├── locations: BTreeMap<ItemId, ItemLocation>
└── slot_owner: BTreeMap<slot_key, ItemId>   // lookup rapide par slot
```

Transactions (§85) : valider source, ownership, destination, requirements ; réserver ; appliquer ; émettre. Une erreur avant apply → zéro mutation.

## Quest DSL (arpg-sim/src/quest.rs)

```text
QuestDefinition
├── id: QuestDefId, name, act
├── objectives: Vec<QuestObjective>       // id, description
├── access: QuestAccess                   // OwnerOnly | Party | Everyone
└── rules: Vec<QuestRule>
    ├── trigger: QuestTrigger             // §102 : AreaEntered, MonsterKilled, ...
    ├── conditions: Vec<QuestCondition>   // §103 : HasFlag, VariableAtLeast, ...
    └── actions: Vec<QuestAction>          // §104 : SetFlag, GrantItem, ...

GameQuestState          // état monde de la partie (porte ouverte)
├── status: QuestStatus // NotStarted|Active|Completed|Rewarded|Failed
├── variables, flags, completed_objectives

CharacterQuestState     // récompenses personnelles déjà reçues
├── quests, rewarded, variables, flags
```

## Economy model (arpg-sim/src/economy.rs, trade.rs)

```text
Gold                    // carried + stash — numérique, pas un item (§94)
GroundCurrency          // pile d'or au sol : amount, pos, level
Merchant
├── buy/sell_multiplier, repair/recharge_cost_per_point
├── stock: BTreeMap<PlayerId, Vec<MerchantEntry>>        // stock personnel (§95)
└── gamble_stock: BTreeMap<PlayerId, Vec<GambleOffer>>    // qualité cachée (§97)

Trade (§116)
├── state: TradeState  // Open → Persisting → Committed | Open → Cancelled
├── offer_a/offer_b: TradeOffer { items: Vec<ItemId>, gold: u64 }
└── accepted_a/accepted_b
```

## World (arpg-world)

```text
LevelInstance
├── id: LevelInstanceId
└── objects: Vec<WorldObject>       // id, state, cooldown_until
```
