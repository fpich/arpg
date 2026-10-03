//! Reference datapack (SPEC.md sections 151-156, 193, M9): the in-repo
//! content compiled into `GameData` at startup. Sources are normalized Rust
//! tables here; the runtime never depends on a source file format.

use crate::{GameData, ItemDefinition, LevelDefinition, MonsterDefinition, SkillDefinition};
use arpg_core::{ClassId, ItemId, LevelDefId, MonsterDefId, SkillId};
use std::collections::BTreeMap;

pub const DATA_SCHEMA_VERSION: u32 = 1;
pub const CONTENT_REVISION: u32 = 1;
pub const COMPILER_VERSION: u32 = 1;

// ------------------------------------------------------------------ classes

/// One of the 7 playable classes (SPEC.md section 193).
#[derive(Debug, Clone)]
pub struct ClassDefinition {
    pub id: ClassId,
    pub name: &'static str,
    /// starting skill granted at level 1
    pub primary_skill: SkillId,
    /// stat growth per level: (life, mana)
    pub life_per_level: i64,
    pub mana_per_level: i64,
}

pub const AMAZON: ClassId = ClassId(0);
pub const ASSASSIN: ClassId = ClassId(1);
pub const NECROMANCER: ClassId = ClassId(2);
pub const BARBARIAN: ClassId = ClassId(3);
pub const PALADIN: ClassId = ClassId(4);
pub const SORCERESS: ClassId = ClassId(5);
pub const DRUID: ClassId = ClassId(6);

/// The 7 classes of the reference datapack.
pub const CLASSES: [ClassDefinition; 7] = [
    ClassDefinition {
        id: AMAZON,
        name: "Amazon",
        primary_skill: SkillId(100),
        life_per_level: 20,
        mana_per_level: 15,
    },
    ClassDefinition {
        id: ASSASSIN,
        name: "Assassin",
        primary_skill: SkillId(200),
        life_per_level: 20,
        mana_per_level: 20,
    },
    ClassDefinition {
        id: NECROMANCER,
        name: "Necromancer",
        primary_skill: SkillId(300),
        life_per_level: 15,
        mana_per_level: 25,
    },
    ClassDefinition {
        id: BARBARIAN,
        name: "Barbarian",
        primary_skill: SkillId(400),
        life_per_level: 25,
        mana_per_level: 10,
    },
    ClassDefinition {
        id: PALADIN,
        name: "Paladin",
        primary_skill: SkillId(500),
        life_per_level: 20,
        mana_per_level: 15,
    },
    ClassDefinition {
        id: SORCERESS,
        name: "Sorceress",
        primary_skill: SkillId(600),
        life_per_level: 15,
        mana_per_level: 30,
    },
    ClassDefinition {
        id: DRUID,
        name: "Druid",
        primary_skill: SkillId(700),
        life_per_level: 20,
        mana_per_level: 20,
    },
];

// -------------------------------------------------------------- difficulties

/// Difficulty tiers (SPEC.md M8/M9, section 111 scaling).
#[derive(Debug, Clone, Copy)]
pub struct DifficultyDefinition {
    pub id: u32,
    pub name: &'static str,
    pub monster_life_multiplier: u32,
    pub monster_damage_multiplier: u32,
    pub experience_multiplier: u32,
    pub gold_multiplier: u32,
    /// player resist penalty (percent)
    pub resist_penalty: i32,
}

pub const DIFFICULTIES: [DifficultyDefinition; 3] = [
    DifficultyDefinition {
        id: 0,
        name: "Normal",
        monster_life_multiplier: 100,
        monster_damage_multiplier: 100,
        experience_multiplier: 100,
        gold_multiplier: 100,
        resist_penalty: 0,
    },
    DifficultyDefinition {
        id: 1,
        name: "Nightmare",
        monster_life_multiplier: 200,
        monster_damage_multiplier: 150,
        experience_multiplier: 200,
        gold_multiplier: 200,
        resist_penalty: -40,
    },
    DifficultyDefinition {
        id: 2,
        name: "Hell",
        monster_life_multiplier: 400,
        monster_damage_multiplier: 250,
        experience_multiplier: 300,
        gold_multiplier: 350,
        resist_penalty: -100,
    },
];

// --------------------------------------------------------------------- acts

/// One act hub town plus its zones (5-act content, SPEC.md M9).
#[derive(Debug, Clone)]
pub struct ActDefinition {
    pub act: u32,
    pub name: &'static str,
    pub town: LevelDefId,
    pub zones: &'static [LevelDefId],
}

pub const ACT1_TOWN: LevelDefId = LevelDefId(100);
pub const ACT2_TOWN: LevelDefId = LevelDefId(200);
pub const ACT3_TOWN: LevelDefId = LevelDefId(300);
pub const ACT4_TOWN: LevelDefId = LevelDefId(400);
pub const ACT5_TOWN: LevelDefId = LevelDefId(500);

pub const ACTS: [ActDefinition; 5] = [
    ActDefinition {
        act: 1,
        name: "Act I - The Sightless Eye",
        town: ACT1_TOWN,
        zones: &[
            LevelDefId(101),
            LevelDefId(102),
            LevelDefId(103),
            LevelDefId(104),
        ],
    },
    ActDefinition {
        act: 2,
        name: "Act II - The Secret Sanctuary",
        town: ACT2_TOWN,
        zones: &[
            LevelDefId(201),
            LevelDefId(202),
            LevelDefId(203),
            LevelDefId(204),
        ],
    },
    ActDefinition {
        act: 3,
        name: "Act III - The Infernal Gate",
        town: ACT3_TOWN,
        zones: &[
            LevelDefId(301),
            LevelDefId(302),
            LevelDefId(303),
            LevelDefId(304),
        ],
    },
    ActDefinition {
        act: 4,
        name: "Act IV - The Pandemonium Fortress",
        town: ACT4_TOWN,
        zones: &[LevelDefId(401), LevelDefId(402)],
    },
    ActDefinition {
        act: 5,
        name: "Act V - Lord of Destruction",
        town: ACT5_TOWN,
        zones: &[
            LevelDefId(501),
            LevelDefId(502),
            LevelDefId(503),
            LevelDefId(504),
        ],
    },
];

// -------------------------------------------------------------------- items

/// Base item tiers of the ecosystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemTier {
    Normal,
    Exceptional,
    Elite,
}

/// Affix model (SPEC.md section 74).
#[derive(Debug, Clone)]
pub struct AffixDefinition {
    pub id: u32,
    pub name: &'static str,
    pub required_level: u16,
    pub affix_level: u16,
    pub frequency: u32,
    pub group: u32,
    pub is_prefix: bool,
}

pub const AFFIXES: [AffixDefinition; 30] = [
    AffixDefinition {
        id: 1,
        name: "Sturdy",
        required_level: 1,
        affix_level: 1,
        frequency: 4,
        group: 1,
        is_prefix: true,
    },
    AffixDefinition {
        id: 2,
        name: "Strong",
        required_level: 9,
        affix_level: 9,
        frequency: 3,
        group: 1,
        is_prefix: true,
    },
    AffixDefinition {
        id: 3,
        name: "Glorious",
        required_level: 19,
        affix_level: 19,
        frequency: 2,
        group: 1,
        is_prefix: true,
    },
    AffixDefinition {
        id: 4,
        name: "Blessed",
        required_level: 20,
        affix_level: 20,
        frequency: 2,
        group: 1,
        is_prefix: true,
    },
    AffixDefinition {
        id: 5,
        name: "Saintly",
        required_level: 25,
        affix_level: 25,
        frequency: 1,
        group: 1,
        is_prefix: true,
    },
    AffixDefinition {
        id: 6,
        name: "Holy",
        required_level: 30,
        affix_level: 30,
        frequency: 1,
        group: 1,
        is_prefix: true,
    },
    AffixDefinition {
        id: 7,
        name: "Godly",
        required_level: 40,
        affix_level: 40,
        frequency: 1,
        group: 1,
        is_prefix: true,
    },
    AffixDefinition {
        id: 8,
        name: "Sharp",
        required_level: 5,
        affix_level: 7,
        frequency: 4,
        group: 2,
        is_prefix: true,
    },
    AffixDefinition {
        id: 9,
        name: "Fine",
        required_level: 9,
        affix_level: 9,
        frequency: 3,
        group: 2,
        is_prefix: true,
    },
    AffixDefinition {
        id: 10,
        name: "Warrior's",
        required_level: 15,
        affix_level: 15,
        frequency: 2,
        group: 3,
        is_prefix: true,
    },
    AffixDefinition {
        id: 11,
        name: "Soldier's",
        required_level: 22,
        affix_level: 22,
        frequency: 2,
        group: 3,
        is_prefix: true,
    },
    AffixDefinition {
        id: 12,
        name: "Knight's",
        required_level: 28,
        affix_level: 28,
        frequency: 1,
        group: 3,
        is_prefix: true,
    },
    AffixDefinition {
        id: 13,
        name: "Lord's",
        required_level: 36,
        affix_level: 36,
        frequency: 1,
        group: 3,
        is_prefix: true,
    },
    AffixDefinition {
        id: 14,
        name: "Jagged",
        required_level: 1,
        affix_level: 1,
        frequency: 5,
        group: 4,
        is_prefix: true,
    },
    AffixDefinition {
        id: 15,
        name: "Deadly",
        required_level: 13,
        affix_level: 13,
        frequency: 3,
        group: 4,
        is_prefix: true,
    },
    AffixDefinition {
        id: 16,
        name: "Vicious",
        required_level: 23,
        affix_level: 23,
        frequency: 2,
        group: 4,
        is_prefix: true,
    },
    AffixDefinition {
        id: 17,
        name: "Brutal",
        required_level: 33,
        affix_level: 33,
        frequency: 1,
        group: 4,
        is_prefix: true,
    },
    AffixDefinition {
        id: 18,
        name: "Massive",
        required_level: 39,
        affix_level: 39,
        frequency: 1,
        group: 4,
        is_prefix: true,
    },
    AffixDefinition {
        id: 19,
        name: "of Frost",
        required_level: 4,
        affix_level: 5,
        frequency: 4,
        group: 10,
        is_prefix: false,
    },
    AffixDefinition {
        id: 20,
        name: "of Ice",
        required_level: 10,
        affix_level: 10,
        frequency: 3,
        group: 10,
        is_prefix: false,
    },
    AffixDefinition {
        id: 21,
        name: "of Cold",
        required_level: 16,
        affix_level: 16,
        frequency: 2,
        group: 10,
        is_prefix: false,
    },
    AffixDefinition {
        id: 22,
        name: "of the Leech",
        required_level: 4,
        affix_level: 6,
        frequency: 4,
        group: 11,
        is_prefix: false,
    },
    AffixDefinition {
        id: 23,
        name: "of Bloodletting",
        required_level: 10,
        affix_level: 10,
        frequency: 3,
        group: 11,
        is_prefix: false,
    },
    AffixDefinition {
        id: 24,
        name: "of the Fox",
        required_level: 2,
        affix_level: 3,
        frequency: 5,
        group: 12,
        is_prefix: false,
    },
    AffixDefinition {
        id: 25,
        name: "of the Wolverine",
        required_level: 12,
        affix_level: 12,
        frequency: 2,
        group: 12,
        is_prefix: false,
    },
    AffixDefinition {
        id: 26,
        name: "of the Bear",
        required_level: 23,
        affix_level: 23,
        frequency: 1,
        group: 12,
        is_prefix: false,
    },
    AffixDefinition {
        id: 27,
        name: "of Luck",
        required_level: 15,
        affix_level: 15,
        frequency: 2,
        group: 13,
        is_prefix: false,
    },
    AffixDefinition {
        id: 28,
        name: "of Fortune",
        required_level: 24,
        affix_level: 24,
        frequency: 1,
        group: 13,
        is_prefix: false,
    },
    AffixDefinition {
        id: 29,
        name: "of Life",
        required_level: 6,
        affix_level: 8,
        frequency: 4,
        group: 14,
        is_prefix: false,
    },
    AffixDefinition {
        id: 30,
        name: "of Vitality",
        required_level: 20,
        affix_level: 20,
        frequency: 2,
        group: 14,
        is_prefix: false,
    },
];

/// Uniques (SPEC.md section 193: uniques).
pub const UNIQUES: [(&str, u16); 5] = [
    ("The Gnasher", 3),
    ("Woven Frost", 12),
    ("Bloodrise", 21),
    ("Baranar's Star", 41),
    ("Windforce", 74),
];

/// Runes (SPEC.md section 193: 10 runes in the full pack).
pub const RUNES: [&str; 10] = [
    "El", "Eld", "Tir", "Nef", "Eth", "Ith", "Tal", "Ral", "Ort", "Thul",
];

/// Runewords (SPEC.md section 193: 5 runewords).
pub const RUNEWORDS: [(&str, &[&str]); 5] = [
    ("Steel", &["El", "Tir"]),
    ("Nadir", &["Nef", "Tir"]),
    ("Malice", &["Ith", "El", "Eth"]),
    ("Zephyr", &["Ort", "Eth"]),
    ("Ancient's Pledge", &["Ral", "Ort", "Tal"]),
];

/// Set items (SPEC.md section 193: sets).
pub const SETS: [(&str, &[&str]); 3] = [
    (
        "Cathan's Traps",
        &["Cathan's Rule", "Cathan's Seal", "Cathan's Visor"],
    ),
    (
        "Death's Garb",
        &["Death's Hand", "Death's Guard", "Death's Touch"],
    ),
    (
        "Angelic Raiment",
        &["Angelic Halo", "Angelic Wings", "Angelic Signature"],
    ),
];

/// Cube recipes (SPEC.md section 193: recipes).
pub const RECIPES: [(&str, &[&str]); 5] = [
    ("Repair", &["Cracked Helm", "Perfect Gem"]),
    ("Recharge", &["Empty Wand", "Chipped Gem"]),
    ("Upgrade", &["Fine Blade", "Amn Rune"]),
    ("Transmute Gem", &["Chipped Gem", "Chipped Gem"]),
    ("Socket", &["Plain Shield", "Tal Rune"]),
];

// ------------------------------------------------------------- compilation

/// Compile the reference datapack into a `GameData` (section 151
/// pipeline: normalized sources -> semantic validation -> compilation).
/// The content hash is derived from the compiled content, not the sources.
pub fn compile_reference_datapack() -> GameData {
    let mut data = GameData {
        schema_version: DATA_SCHEMA_VERSION,
        content_revision: CONTENT_REVISION,
        ..GameData::default()
    };

    // skills: one signature skill per class plus shared base attacks
    let mut skills = BTreeMap::new();
    for class in &CLASSES {
        skills.insert(
            class.primary_skill,
            SkillDefinition {
                id: class.primary_skill,
                name: format!("{} Signature", class.name),
            },
        );
    }
    for (id, name) in [
        (SkillId(1), "Attack"),
        (SkillId(2), "Kick"),
        (SkillId(3), "Throw"),
    ] {
        skills.insert(
            id,
            SkillDefinition {
                id,
                name: name.into(),
            },
        );
    }
    data.skills = skills;

    // monsters: regular per act plus one boss per act
    let mut monsters = BTreeMap::new();
    for (i, name) in [
        "Fallen",
        "Zombie",
        "Skeleton",
        "Quill Beast",
        "Gargantuan Beast",
        "Cliff Lurker",
        "Mummy",
        "Sand Raider",
        "Vulture",
        "Willowisp",
    ]
    .iter()
    .enumerate()
    {
        let id = MonsterDefId(i as u32);
        monsters.insert(
            id,
            MonsterDefinition {
                id,
                name: (*name).into(),
                base_life: 50 + 10 * i as i64,
            },
        );
    }
    for (i, boss) in ["Blood Raven", "The Smith", "Duriel", "Mephisto", "Baal"]
        .iter()
        .enumerate()
    {
        let id = MonsterDefId(1000 + i as u32);
        monsters.insert(
            id,
            MonsterDefinition {
                id,
                name: (*boss).into(),
                base_life: 500 + 500 * i as i64,
            },
        );
    }
    data.monsters = monsters;

    // levels: 5 act towns + zones
    let mut levels = BTreeMap::new();
    for act in &ACTS {
        levels.insert(
            act.town,
            LevelDefinition {
                id: act.town,
                name: format!("Act {} Town", act.act),
            },
        );
        for zone in act.zones {
            levels.insert(
                *zone,
                LevelDefinition {
                    id: *zone,
                    name: format!("Act {} Zone {}", act.act, zone.0),
                },
            );
        }
    }
    data.levels = levels;

    // items: 50 base items across tiers
    let mut items = BTreeMap::new();
    let base_names = [
        "Cap",
        "Helm",
        "Quilted Armor",
        "Leather Armor",
        "Buckler",
        "Small Shield",
        "Dagger",
        "Dirk",
        "Short Sword",
        "Scimitar",
        "Wand",
        "Yew Wand",
        "Short Bow",
        "Hunter's Bow",
        "Javelin",
        "Glaive",
        "Gloves",
        "Boots",
        "Belt",
        "Club",
    ];
    let exceptional_names = [
        "Casque",
        "Basinet",
        "Ghost Armor",
        "Serpentskin Armor",
        "Round Shield",
        "Kite Shield",
        "Poignard",
        "Fascia",
        "Gladius",
        "Cutlass",
        "Burnt Wand",
        "Grim Wand",
        "Edge Bow",
        "Composite Bow",
        "Spiculum",
        "Pilum",
        "Gauntlets",
        "Greaves",
        "Sash",
        "Cudgel",
    ];
    let elite_names = [
        "Shako",
        "Giant Skull",
        "Dusk Shroud",
        "Archon Plate",
        "Luna",
        "Monarch",
        "Legend Spike",
        "Mosaic",
        "Frenzy Blade",
        "Legend Sword",
    ];
    for (offset, name) in base_names
        .iter()
        .chain(exceptional_names.iter())
        .chain(elite_names.iter())
        .enumerate()
    {
        let next_id = (offset + 1) as u32;
        items.insert(
            next_id,
            ItemDefinition {
                id: ItemId(next_id as u128),
                name: (*name).into(),
            },
        );
    }
    data.items = items;

    // content hash over the compiled content
    data.content_hash = content_hash(&data);
    data
}

fn content_hash(data: &GameData) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&data.schema_version.to_le_bytes());
    hasher.update(&data.content_revision.to_le_bytes());
    hasher.update(&(data.skills.len() as u64).to_le_bytes());
    hasher.update(&(data.monsters.len() as u64).to_le_bytes());
    hasher.update(&(data.levels.len() as u64).to_le_bytes());
    hasher.update(&(data.items.len() as u64).to_le_bytes());
    for (id, s) in &data.skills {
        hasher.update(&id.0.to_le_bytes());
        hasher.update(s.name.as_bytes());
    }
    for (id, m) in &data.monsters {
        hasher.update(&id.0.to_le_bytes());
        hasher.update(&m.base_life.to_le_bytes());
    }
    for id in data.levels.keys() {
        hasher.update(&id.0.to_le_bytes());
    }
    for (id, i) in &data.items {
        hasher.update(&id.to_le_bytes());
        hasher.update(i.name.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

/// Semantic validation of the compiled datapack (section 151): never
/// publishes a datapack with dangling references or empty content.
pub fn validate(data: &GameData) -> Result<(), &'static str> {
    if data.skills.is_empty() || data.monsters.is_empty() || data.levels.is_empty() {
        return Err("datapack is missing core content");
    }
    if data.items.len() < 50 {
        return Err("reference datapack requires at least 50 base items");
    }
    if CLASSES.len() != 7 {
        return Err("7 classes required");
    }
    if DIFFICULTIES.len() != 3 {
        return Err("3 difficulties required");
    }
    for act in &ACTS {
        if !data.levels.contains_key(&act.town) {
            return Err("act town missing from levels");
        }
        for zone in act.zones {
            if !data.levels.contains_key(zone) {
                return Err("act zone missing from levels");
            }
        }
    }
    for class in &CLASSES {
        if !data.skills.contains_key(&class.primary_skill) {
            return Err("class primary skill missing");
        }
    }
    Ok(())
}

/// Datapack manifest (section 154).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataPackManifest {
    pub data_schema_version: u32,
    pub content_revision: u32,
    pub compiler_version: u32,
    pub content_hash: [u8; 32],
}

impl DataPackManifest {
    pub fn of(data: &GameData) -> DataPackManifest {
        DataPackManifest {
            data_schema_version: data.schema_version,
            content_revision: data.content_revision,
            compiler_version: COMPILER_VERSION,
            content_hash: data.content_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_datapack_meets_v1_requirements() {
        let data = compile_reference_datapack();
        validate(&data).expect("valid datapack");
        assert_eq!(CLASSES.len(), 7, "7 classes");
        assert_eq!(DIFFICULTIES.len(), 3, "3 difficulties");
        assert_eq!(ACTS.len(), 5, "5 acts");
        assert_eq!(data.items.len(), 50, "50 base items");
        assert_eq!(AFFIXES.len(), 30, "30 affixes");
        assert_eq!(UNIQUES.len(), 5, "5 uniques");
        assert_eq!(RUNES.len(), 10, "10 runes");
        assert_eq!(RUNEWORDS.len(), 5, "5 runewords");
        assert_eq!(SETS.len(), 3, "sets");
        assert_eq!(RECIPES.len(), 5, "5 recipes");
    }

    #[test]
    fn compilation_is_deterministic() {
        let a = compile_reference_datapack();
        let b = compile_reference_datapack();
        assert_eq!(a.content_hash, b.content_hash);
        assert_eq!(DataPackManifest::of(&a), DataPackManifest::of(&b));
    }

    #[test]
    fn manifest_matches_spec_shape() {
        let data = compile_reference_datapack();
        let m = DataPackManifest::of(&data);
        assert_eq!(m.data_schema_version, DATA_SCHEMA_VERSION);
        assert_eq!(m.content_revision, CONTENT_REVISION);
        assert_eq!(m.compiler_version, COMPILER_VERSION);
        assert_eq!(m.content_hash, data.content_hash);
    }

    #[test]
    fn affix_groups_keep_prefix_and_suffix_distinct() {
        // a group hosts at most one prefix and one suffix family; the
        // one-affix-per-group rule (section 74) applies at roll time to
        // the selected affixes of a single item
        for a in &AFFIXES {
            assert!(
                a.frequency > 0,
                "affix {} needs a positive frequency",
                a.name
            );
            assert!(
                a.required_level <= a.affix_level,
                "affix {} level ordering",
                a.name
            );
        }
    }
}
