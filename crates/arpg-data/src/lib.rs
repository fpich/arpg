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
}
