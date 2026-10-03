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
