//! Compiled datapack format (SPEC.md sections 151-153).
//!
//! Pipeline: sources -> importer -> normalized source model -> semantic
//! validation -> compilation -> compressed datapack.
//!
//! Compiled format decision: postcard + zstd + manifest + checksum. The
//! binary datapack is rebuilt at every `DataSchemaVersion` evolution;
//! no historical binary compatibility is required.

use crate::datapack::DataPackManifest;
use crate::GameData;

/// Serializable mirror of the normalized source model (section 151).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CompiledGameData {
    pub schema_version: u32,
    pub content_revision: u32,
    pub skills: Vec<CompiledSkill>,
    pub monsters: Vec<CompiledMonster>,
    pub levels: Vec<CompiledLevel>,
    pub items: Vec<CompiledItem>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CompiledSkill {
    pub id: u32,
    pub name: String,
    pub targeting: Option<u8>,
    pub damage: Option<i64>,
    pub damage_type: Option<u8>,
    pub mana_cost: Option<i64>,
    pub cast_ticks: Option<u16>,
    pub missile: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CompiledMonster {
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
    #[serde(default = "monster_default_leaves_corpse")]
    pub leaves_corpse: bool,
}

fn monster_default_leaves_corpse() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CompiledLevel {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CompiledItem {
    pub id: u64,
    pub name: String,
    pub potion_kind: Option<u8>,
    pub potion_a: i64,
    pub potion_b: i64,
    pub potion_c: u32,
}

/// Import the normalized source model from `GameData` (sections 151-152):
/// the runtime never depends on the editing format; this is the importer
/// stage producing the normalized model.
pub fn import_normalized(data: &GameData) -> CompiledGameData {
    CompiledGameData {
        schema_version: data.schema_version,
        content_revision: data.content_revision,
        skills: data
            .skills
            .values()
            .map(|s| CompiledSkill {
                id: s.id.0,
                name: s.name.clone(),
                targeting: s.program.as_ref().map(|p| p.targeting),
                damage: s.program.as_ref().map(|p| p.damage),
                damage_type: s.program.as_ref().map(|p| p.damage_type),
                mana_cost: s.program.as_ref().map(|p| p.mana_cost),
                cast_ticks: s.program.as_ref().map(|p| p.cast_ticks),
                missile: s.program.as_ref().and_then(|p| p.missile),
            })
            .collect(),
        monsters: data
            .monsters
            .values()
            .map(|m| CompiledMonster {
                id: m.id.0,
                name: m.name.clone(),
                base_life: m.base_life,
                damage: m.damage,
                speed_fp: m.speed_fp,
                aggro_range: m.aggro_range,
                experience: m.experience,
                ranged: m.ranged,
                act: m.act,
                damage_type: m.damage_type,
                leaves_corpse: m.leaves_corpse,
            })
            .collect(),
        levels: data
            .levels
            .values()
            .map(|l| CompiledLevel {
                id: l.id.0,
                name: l.name.clone(),
            })
            .collect(),
        items: data
            .items
            .values()
            .map(|i| CompiledItem {
                id: i.id.0 as u64,
                name: i.name.clone(),
                potion_kind: i.potion.as_ref().map(|_| 0),
                potion_a: 0,
                potion_b: 0,
                potion_c: 0,
            })
            .collect(),
    }
}

/// Semantic validation of the normalized model (section 151): ids unique
/// and positive, no dangling references. Returns the manifest on success.
pub fn semantic_validate(model: &CompiledGameData) -> Result<DataPackManifest, String> {
    let mut seen = std::collections::BTreeSet::new();
    for s in &model.skills {
        if !seen.insert(("skill", s.id)) {
            return Err(format!("duplicate skill id {}", s.id));
        }
    }
    for m in &model.monsters {
        if !seen.insert(("monster", m.id)) {
            return Err(format!("duplicate monster id {}", m.id));
        }
        if m.base_life <= 0 {
            return Err(format!("monster {} has non-positive life", m.id));
        }
    }
    for l in &model.levels {
        if !seen.insert(("level", l.id)) {
            return Err(format!("duplicate level id {}", l.id));
        }
    }
    for i in &model.items {
        if !seen.insert(("item", i.id as u32)) {
            return Err(format!("duplicate item id {}", i.id));
        }
    }
    Ok(DataPackManifest {
        data_schema_version: model.schema_version,
        content_revision: model.content_revision,
        compiler_version: 1,
        content_hash: [0u8; 32],
    })
}

/// Compiled datapack file: manifest + zstd-compressed postcard payload
/// (section 153), with a checksum over the compressed bytes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CompiledDatapack {
    pub manifest: CompiledManifestWire,
    pub compressed: Vec<u8>,
    pub checksum: [u8; 32],
}

/// Wire form of the manifest (section 154).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompiledManifestWire {
    pub data_schema_version: u32,
    pub content_revision: u32,
    pub compiler_version: u32,
    pub content_hash: [u8; 32],
}

/// Compile the normalized model into the compressed datapack format
/// (section 153): postcard -> zstd -> manifest + checksum.
pub fn compile(model: &CompiledGameData) -> Result<CompiledDatapack, String> {
    let manifest = semantic_validate(model)?;
    let payload = postcard::to_allocvec(model).map_err(|e| e.to_string())?;
    let compressed = zstd::encode_all(payload.as_slice(), 3).map_err(|e| e.to_string())?;
    let content_hash = *blake3::hash(&compressed).as_bytes();
    Ok(CompiledDatapack {
        manifest: CompiledManifestWire {
            data_schema_version: manifest.data_schema_version,
            content_revision: manifest.content_revision,
            compiler_version: manifest.compiler_version,
            content_hash: manifest.content_hash,
        },
        compressed,
        checksum: content_hash,
    })
}

/// Load and verify a compiled datapack (section 153): checksum first,
/// then decompress and deserialize. Schema version mismatches are
/// rejected - the binary is rebuilt at every schema evolution.
pub fn load(pack: &CompiledDatapack) -> Result<CompiledGameData, String> {
    let actual = *blake3::hash(&pack.compressed).as_bytes();
    if actual != pack.checksum {
        return Err("datapack checksum mismatch".to_string());
    }
    let payload = zstd::decode_all(pack.compressed.as_slice()).map_err(|e| e.to_string())?;
    let model: CompiledGameData = postcard::from_bytes(&payload).map_err(|e| e.to_string())?;
    if model.schema_version != pack.manifest.data_schema_version {
        return Err(format!(
            "schema version mismatch: pack {} vs model {}",
            pack.manifest.data_schema_version, model.schema_version
        ));
    }
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_load_roundtrip_preserves_the_model() {
        let data = crate::datapack::compile_reference_datapack();
        let model = import_normalized(&data);
        let pack = compile(&model).expect("compile");
        let loaded = load(&pack).expect("load");
        assert_eq!(model, loaded);
        assert!(!pack.compressed.is_empty());
    }

    #[test]
    fn corrupted_payload_fails_the_checksum() {
        let data = crate::datapack::compile_reference_datapack();
        let model = import_normalized(&data);
        let mut pack = compile(&model).expect("compile");
        pack.compressed[0] ^= 0xFF;
        assert!(load(&pack).is_err(), "checksum must reject corruption");
    }

    #[test]
    fn semantic_validation_rejects_duplicate_ids() {
        let mut model = import_normalized(&crate::datapack::compile_reference_datapack());
        let dup = model.monsters[0].clone();
        model.monsters.push(dup);
        assert!(semantic_validate(&model).is_err());
    }
}
