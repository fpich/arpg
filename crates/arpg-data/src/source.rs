//! Datapack source formats (SPEC.md section 152): TOML / CSV / JSON editing
//! formats, by nature of the table. The runtime never depends on the
//! source format - this module is the importer stage feeding the
//! normalized source model (section 151), which then goes through
//! semantic validation and compilation.

use crate::compiled::{
    CompiledGameData, CompiledItem, CompiledLevel, CompiledMonster, CompiledSkill,
};
use std::collections::BTreeMap;

/// Parse error for source formats (section 152).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceError(pub String);

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "datapack source error: {}", self.0)
    }
}

impl std::error::Error for SourceError {}

/// A bestiary row (CSV): the natural table format for large monster lists.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct BestiaryRow {
    pub id: u32,
    pub name: String,
    pub base_life: i64,
    pub damage: i64,
    pub speed_fp: i32,
    pub aggro_range: i32,
    pub experience: u64,
    #[serde(default)]
    pub ranged: bool,
    #[serde(default)]
    pub act: u8,
    #[serde(default)]
    pub damage_type: u8,
    #[serde(default = "default_true")]
    pub leaves_corpse: bool,
}

fn default_true() -> bool {
    true
}

/// Import a bestiary CSV (section 152) into monster entries.
pub fn import_bestiary_csv(csv_text: &str) -> Result<Vec<CompiledMonster>, SourceError> {
    let mut rdr = csv::Reader::from_reader(csv_text.as_bytes());
    let mut monsters = Vec::new();
    for record in rdr.deserialize() {
        let row: BestiaryRow = record.map_err(|e| SourceError(e.to_string()))?;
        monsters.push(CompiledMonster {
            id: row.id,
            name: row.name,
            base_life: row.base_life,
            damage: row.damage,
            speed_fp: row.speed_fp,
            aggro_range: row.aggro_range,
            experience: row.experience,
            ranged: row.ranged,
            act: row.act,
            damage_type: row.damage_type,
            leaves_corpse: row.leaves_corpse,
        });
    }
    Ok(monsters)
}

/// A whole datapack in TOML form (section 152): the natural format for
/// hand-edited structured tables (skills, levels, items).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct TomlDatapack {
    pub schema_version: u32,
    pub content_revision: u32,
    #[serde(default)]
    pub skills: Vec<CompiledSkill>,
    #[serde(default)]
    pub monsters: Vec<CompiledMonster>,
    #[serde(default)]
    pub levels: Vec<CompiledLevel>,
    #[serde(default)]
    pub items: Vec<CompiledItem>,
}

/// Import a TOML datapack (section 152) into the normalized model.
pub fn import_toml(toml_text: &str) -> Result<CompiledGameData, SourceError> {
    let pack: TomlDatapack = toml::from_str(toml_text).map_err(|e| SourceError(e.to_string()))?;
    Ok(CompiledGameData {
        schema_version: pack.schema_version,
        content_revision: pack.content_revision,
        skills: pack.skills,
        monsters: pack.monsters,
        levels: pack.levels,
        items: pack.items,
    })
}

/// Merge source fragments into one normalized model (section 151): a
/// bestiary CSV can be merged into a TOML-authored pack. Conflicting ids
/// are an error - the merge is deterministic.
pub fn merge_fragments(
    base: CompiledGameData,
    extra_monsters: Vec<CompiledMonster>,
) -> Result<CompiledGameData, SourceError> {
    let mut monsters: BTreeMap<u32, CompiledMonster> =
        base.monsters.into_iter().map(|m| (m.id, m)).collect();
    for m in extra_monsters {
        if monsters.contains_key(&m.id) {
            return Err(SourceError(format!("conflicting monster id {}", m.id)));
        }
        monsters.insert(m.id, m);
    }
    Ok(CompiledGameData {
        schema_version: base.schema_version,
        content_revision: base.content_revision,
        skills: base.skills,
        monsters: monsters.into_values().collect(),
        levels: base.levels,
        items: base.items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_import_produces_the_normalized_model() {
        let src = r#"
schema_version = 1
content_revision = 7

[[levels]]
id = 0
name = "Blood Moor"

[[monsters]]
id = 1
name = "Fallen"
base_life = 40
damage = 3
speed_fp = 64
aggro_range = 5
experience = 10
ranged = false
act = 0
damage_type = 0
leaves_corpse = true
"#;
        let model = import_toml(src).expect("valid toml");
        assert_eq!(model.schema_version, 1);
        assert_eq!(model.levels.len(), 1);
        assert_eq!(model.monsters.len(), 1);
        assert_eq!(model.monsters[0].name, "Fallen");
    }

    #[test]
    fn csv_bestiary_import_and_merge() {
        let csv =
            "id,name,base_life,damage,speed_fp,aggro_range,experience\n5,Zombie,60,4,48,4,12\n";
        let monsters = import_bestiary_csv(csv).expect("valid csv");
        assert_eq!(monsters.len(), 1);
        assert_eq!(monsters[0].name, "Zombie");
        assert!(monsters[0].leaves_corpse, "default true");

        let base = CompiledGameData {
            schema_version: 1,
            content_revision: 1,
            skills: vec![],
            monsters: vec![],
            levels: vec![],
            items: vec![],
        };
        let merged = merge_fragments(base, monsters).expect("merge ok");
        assert_eq!(merged.monsters.len(), 1);
    }

    #[test]
    fn merge_rejects_conflicting_ids() {
        let csv =
            "id,name,base_life,damage,speed_fp,aggro_range,experience\n1,Fallen,40,3,64,5,10\n";
        let monsters = import_bestiary_csv(csv).unwrap();
        let base = import_toml(
            r#"
schema_version = 1
content_revision = 1

[[monsters]]
id = 1
name = "Fallen"
base_life = 40
damage = 3
speed_fp = 64
aggro_range = 5
experience = 10
"#,
        )
        .unwrap();
        assert!(merge_fragments(base, monsters).is_err());
    }

    #[test]
    fn malformed_toml_is_a_source_error() {
        assert!(import_toml("not [ valid toml").is_err());
    }
}
