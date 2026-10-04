use arpg_core::{ItemId, LevelDefId, MonsterDefId, SkillId};

pub mod datapack;
pub use datapack::{
    compile_reference_datapack, validate, ActDefinition, ClassDefinition, DataPackManifest,
    DifficultyDefinition, AFFIXES, CLASSES, DIFFICULTIES, RECIPES, RUNES, RUNEWORDS, SETS, UNIQUES,
};
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct GameData {
    pub schema_version: u32,
    pub content_hash: [u8; 32],
    pub content_revision: u32,
    pub skills: BTreeMap<SkillId, SkillDefinition>,
    pub monsters: BTreeMap<MonsterDefId, MonsterDefinition>,
    pub levels: BTreeMap<LevelDefId, LevelDefinition>,
    pub items: BTreeMap<u32, ItemDefinition>,
}

#[derive(Debug, Clone)]
pub struct SkillDefinition {
    pub id: SkillId,
    pub name: String,
    /// Gameplay program: target kind and damage profile, compiled into a
    /// sim skill IR at load time. `None` = nominal skill (no program yet).
    pub program: Option<SkillProgramData>,
}

/// Serializable skill program profile (datapack side). The sim translates
/// it into the full typed IR; data cannot depend on sim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillProgramData {
    /// 0 = self, 1 = entity, 2 = position
    pub targeting: u8,
    /// flat damage dealt on impact
    pub damage: i64,
    /// damage type: 0 physical, 1 magic, 2 fire, 3 cold, 4 lightning, 5 poison
    pub damage_type: u8,
    /// mana cost
    pub mana_cost: i64,
    /// cast time in ticks (0 = instant)
    pub cast_ticks: u16,
    /// missile definition spawned on impact (None = no missile)
    pub missile: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct MonsterDefinition {
    pub id: MonsterDefId,
    pub name: String,
    pub base_life: i64,
    /// Melee damage per hit.
    pub damage: i64,
    /// Movement speed in fixed-point tiles per tick (256 = 1 tile).
    pub speed_fp: i32,
    /// Aggro radius in tiles.
    pub aggro_range: i32,
    /// Experience granted on kill.
    pub experience: u64,
    /// True for ranged attackers (they stop and shoot).
    pub ranged: bool,
    /// Act index (0-4) this monster belongs to.
    pub act: u8,
    /// Damage type: 0 physical, 1 magic, 2 fire, 3 cold, 4 lightning,
    /// 5 poison (SPEC.md section 51). Ranged attackers deal elemental
    /// damage so resistances matter in play.
    pub damage_type: u8,
}

#[derive(Debug, Clone)]
pub struct LevelDefinition {
    pub id: LevelDefId,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ItemDefinition {
    pub id: ItemId,
    pub name: String,
    /// Consumable effect (SPEC.md section 83). None for non-potions.
    pub potion: Option<PotionEffect>,
}

/// Potion effect (SPEC.md section 83): instant value, value over time,
/// or a resistance modifier with a duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PotionEffect {
    /// Instant life/mana gain in fixed-point units.
    Instant { life_fp: i64, mana_fp: i64 },
    /// Value over time: total fixed-point gain spread over ticks.
    OverTime {
        life_fp: i64,
        mana_fp: i64,
        ticks: u32,
    },
    /// Temporary resistance modifier (percent) with a duration in ticks.
    Resistance {
        kind: ResistKind,
        percent: i64,
        ticks: u32,
    },
    /// Cures a state (antidote/thawing/stamina).
    Cure { kind: CureKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResistKind {
    Fire,
    Cold,
    Lightning,
    Poison,
    Magic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CureKind {
    Poison,
    Cold,
    Stamina,
}
