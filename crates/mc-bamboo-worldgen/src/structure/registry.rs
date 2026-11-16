//! 结构相关四个注册表的解析骨架：`structure_set` / `structure` / `template_pool` /
//! `processor_list`。
//!
//! 本轮把 `structure_set` 和 `structure` 里放置阶段要用到的字段解析成 Rust 结构
//! （placement 参数、加权表、结构允许的群系），`template_pool` / `processor_list`
//! 只校验是对象，完整解析留到结构件生成的里程碑。群系标签用 `mc-bamboo-data` 的
//! `worldgen/biome` 标签表和 `BIOME_CLIMATES` 展开成群系 ID。
//!
//! Parsing skeleton for the four structure registries. This round parses the fields the
//! placement stage needs from `structure_set` / `structure`; `template_pool` / `processor_list`
//! are only validated as objects, full parsing is a later milestone. Biome tags are expanded
//! to biome IDs via the `mc-bamboo-data` `worldgen/biome` tags and `BIOME_CLIMATES`.

use std::collections::{BTreeMap, HashSet};

use mc_bamboo_data::{BIOME_CLIMATES, tags};
use serde_json::Value;

use crate::structure::placement::Placement;

/// `StructureSet.StructureSelectionEntry`：结构注册名和它在加权表里的权重。
///
/// `StructureSet.StructureSelectionEntry`: a structure id and its weight in the selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionEntry {
    pub structure: String,
    pub weight: i32,
}

/// 一个 `structure_set` 条目。
///
/// One `structure_set` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct StructureSetEntry {
    pub name: String,
    pub placement: Placement,
    pub structures: Vec<SelectionEntry>,
}

/// `structure_set` 注册表，按名字排序（和 `encodeRegistry` 的导出顺序一致）。
///
/// The `structure_set` registry, sorted by name (matching `encodeRegistry` export order).
#[derive(Debug, Clone)]
pub struct StructureSets {
    sets: BTreeMap<String, StructureSetEntry>,
}

impl StructureSets {
    /// 从 `worldgen.json` 的 `structure_set` 对象解析。
    ///
    /// Parse the `structure_set` object of `worldgen.json`.
    pub fn parse(raw: &Value) -> Result<Self, String> {
        let object = raw.as_object().ok_or("structure_set must be an object")?;
        let mut sets = BTreeMap::new();
        for (name, value) in object {
            let placement_value = value
                .get("placement")
                .ok_or_else(|| format!("structure_set {name} missing placement"))?;
            let placement = Placement::parse(placement_value)
                .map_err(|error| format!("structure_set {name}: {error}"))?;
            let structures_value = value
                .get("structures")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("structure_set {name} missing structures"))?;
            let mut structures = Vec::with_capacity(structures_value.len());
            for entry in structures_value {
                let structure = entry
                    .get("structure")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("structure_set {name} has an entry without structure"))?
                    .to_owned();
                let weight =
                    entry.get("weight").and_then(Value::as_i64).ok_or_else(|| {
                        format!("structure_set {name} has an entry without weight")
                    })? as i32;
                structures.push(SelectionEntry { structure, weight });
            }
            sets.insert(
                name.clone(),
                StructureSetEntry {
                    name: name.clone(),
                    placement,
                    structures,
                },
            );
        }
        Ok(Self { sets })
    }

    pub fn get(&self, name: &str) -> Option<&StructureSetEntry> {
        self.sets.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &StructureSetEntry> {
        self.sets.values()
    }

    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    /// 所有 `random_spread` 结构集的名字，按字母序。对拍用它核对两边集合一致。
    ///
    /// Sorted names of all `random_spread` structure sets. The oracle uses it to check the
    /// two sides see the same set.
    pub fn random_spread_names(&self) -> Vec<String> {
        self.sets
            .values()
            .filter(|entry| matches!(entry.placement, Placement::RandomSpread(_)))
            .map(|entry| entry.name.clone())
            .collect()
    }
}

/// 一个 `structure` 条目里放置阶段用到的字段。
///
/// Fields of a `structure` entry used by the placement stage.
#[derive(Debug, Clone)]
pub struct StructureDef {
    pub name: String,
    pub type_id: String,
    pub biomes: Vec<i32>,
    pub step: String,
    pub terrain_adaptation: String,
    /// 该条目的原始 JSON，供具体结构读自己的配置字段（海底遗迹的概率、废弃传送门的
    /// `setups` 等）。不在 `StructureDef` 里逐个解析，是为了让各结构的解析留在自己的模块。
    ///
    /// The raw JSON of this entry so a concrete structure can read its own config fields
    /// (ocean-ruin probabilities, ruined-portal `setups`, ...). Fields are not parsed here so
    /// that each structure keeps its parsing in its own module.
    pub raw: Value,
}

/// `structure` 注册表（本轮只解析上面这几个字段）。
///
/// The `structure` registry (only the fields above are parsed this round).
#[derive(Debug, Clone)]
pub struct Structures {
    defs: BTreeMap<String, StructureDef>,
}

impl Structures {
    pub fn parse(raw: &Value) -> Result<Self, String> {
        let object = raw.as_object().ok_or("structure must be an object")?;
        let mut defs = BTreeMap::new();
        for (name, value) in object {
            let type_id = value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let biomes = value
                .get("biomes")
                .map(resolve_biome_holders)
                .transpose()
                .map_err(|error| format!("structure {name}: {error}"))?
                .unwrap_or_default();
            let step = value
                .get("step")
                .and_then(Value::as_str)
                .unwrap_or("surface_structures")
                .to_owned();
            let terrain_adaptation = value
                .get("terrain_adaptation")
                .and_then(Value::as_str)
                .unwrap_or("none")
                .to_owned();
            defs.insert(
                name.clone(),
                StructureDef {
                    name: name.clone(),
                    type_id,
                    biomes,
                    step,
                    terrain_adaptation,
                    raw: value.clone(),
                },
            );
        }
        Ok(Self { defs })
    }

    pub fn get(&self, name: &str) -> Option<&StructureDef> {
        self.defs.get(name)
    }

    /// 全部注册名，按 BTreeMap 的名字序。
    ///
    /// All registry ids in name order.
    pub fn names(&self) -> Vec<String> {
        self.defs.keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

/// 把 `HolderSet<Biome>` 的 JSON 展开成群系 ID：`"#tag"`、单个名字或数组。
///
/// Expand a `HolderSet<Biome>` JSON value into biome IDs: a `"#tag"`, a single name, or a list.
pub fn resolve_biome_holders(value: &Value) -> Result<Vec<i32>, String> {
    match value {
        Value::String(text) => resolve_one(text),
        Value::Array(items) => {
            let mut ids = Vec::new();
            for item in items {
                let text = item
                    .as_str()
                    .ok_or("biome holder list must contain strings")?;
                ids.extend(resolve_one(text)?);
            }
            Ok(ids)
        }
        _ => Err("biome holder must be a string or a list".to_owned()),
    }
}

fn resolve_one(text: &str) -> Result<Vec<i32>, String> {
    if let Some(tag) = text.strip_prefix('#') {
        return tags::TAG_GROUPS
            .iter()
            .find(|group| group.registry == "minecraft:worldgen/biome")
            .and_then(|group| group.tags.iter().find(|entry| entry.name == tag))
            .map(|entry| entry.ids.to_vec())
            .ok_or_else(|| format!("unknown biome tag {tag}"));
    }
    BIOME_CLIMATES
        .iter()
        .position(|biome| biome.name == text)
        .map(|id| vec![id as i32])
        .ok_or_else(|| format!("unknown biome {text}"))
}

/// `ChunkGeneratorStructureState.createForNormal` 的过滤：只保留「结构允许的群系」
/// 与 `possible_biomes`（群系源全部可能群系）有交集的结构集。
///
/// The filter of `ChunkGeneratorStructureState.createForNormal`: keep only structure sets
/// whose structures' allowed biomes intersect `possible_biomes`.
pub fn possible_structure_sets(
    sets: &StructureSets,
    structures: &Structures,
    possible_biomes: &HashSet<i32>,
) -> Vec<String> {
    sets.iter()
        .filter(|set| {
            set.structures.iter().any(|entry| {
                structures
                    .get(&entry.structure)
                    .is_some_and(|def| def.biomes.iter().any(|id| possible_biomes.contains(id)))
            })
        })
        .map(|set| set.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_structure_sets_and_random_spread_names() {
        let sets = StructureSets::parse(&json!({
            "minecraft:villages": {
                "placement": {"type":"minecraft:random_spread","salt":10387312,"separation":8,"spacing":34},
                "structures": [
                    {"structure":"minecraft:village_plains","weight":1},
                    {"structure":"minecraft:village_desert","weight":2}
                ]
            },
            "minecraft:strongholds": {
                "placement": {"type":"minecraft:concentric_rings","salt":0,"distance":32,
                    "spread":3,"count":128,"preferred_biomes":"#minecraft:stronghold_biased_to"},
                "structures": [{"structure":"minecraft:stronghold","weight":1}]
            }
        }))
        .unwrap();
        assert_eq!(sets.len(), 2);
        assert_eq!(sets.random_spread_names(), vec!["minecraft:villages"]);
        let villages = sets.get("minecraft:villages").unwrap();
        assert_eq!(villages.structures.len(), 2);
        assert_eq!(villages.structures[1].weight, 2);
    }

    #[test]
    fn biome_tag_and_name_resolve_to_ids() {
        let tag = resolve_biome_holders(&json!("#minecraft:has_structure/village_plains"))
            .expect("structure biome tag is exported");
        assert!(!tag.is_empty());
        let single = resolve_biome_holders(&json!("minecraft:plains")).unwrap();
        assert_eq!(single.len(), 1);
        let listed =
            resolve_biome_holders(&json!(["minecraft:plains", "minecraft:desert"])).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(resolve_biome_holders(&json!("#minecraft:no_such_tag")).is_err());
    }

    #[test]
    fn possible_structure_sets_filters_by_biome_intersection() {
        let sets = StructureSets::parse(&json!({
            "minecraft:villages": {
                "placement": {"type":"minecraft:random_spread","salt":1,"separation":8,"spacing":34},
                "structures": [{"structure":"minecraft:village_plains","weight":1}]
            }
        }))
        .unwrap();
        let structures = Structures::parse(&json!({
            "minecraft:village_plains": {"biomes":"minecraft:plains","step":"surface_structures"}
        }))
        .unwrap();
        let plains_id = BIOME_CLIMATES
            .iter()
            .position(|b| b.name == "minecraft:plains")
            .unwrap() as i32;
        assert_eq!(
            possible_structure_sets(&sets, &structures, &HashSet::from([plains_id])),
            vec!["minecraft:villages"]
        );
        assert!(possible_structure_sets(&sets, &structures, &HashSet::from([999])).is_empty());
    }
}
