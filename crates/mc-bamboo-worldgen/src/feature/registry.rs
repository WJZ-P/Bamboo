//! `feature` / `placed_feature` / `biome_features` / `features_per_step` 的解析。
//!
//! 这四个键都是官方 codec 或注册表名导出的 JSON，这里只做「按名字索引」，
//! 具体的类型解析（feature 配置、placement modifier）放在各自的模块里。

use std::collections::HashMap;

use serde_json::Value;

/// `GenerationStep.Decoration` 的 11 个步骤名，顺序即 ordinal。
pub const STEP_NAMES: [&str; crate::feature::DECORATION_STEPS] = [
    "raw_generation",
    "lakes",
    "local_modifications",
    "underground_structures",
    "surface_structures",
    "strongholds",
    "underground_ores",
    "underground_decoration",
    "fluid_springs",
    "vegetal_decoration",
    "top_layer_modification",
];

/// 一个 `feature` 注册表条目：类型 ID 加官方 codec 编出的原始配置。
#[derive(Debug, Clone)]
pub struct FeatureDef {
    pub name: String,
    pub kind: String,
    pub config: Value,
}

/// 一个 `placed_feature` 注册表条目。
#[derive(Debug, Clone)]
pub struct PlacedFeatureDef {
    pub name: String,
    /// `feature` 字段的值：注册表引用（字符串）或内联配置（对象）。
    pub feature: Value,
    /// `placement` 数组，逐条保留原始 JSON。
    pub placement: Vec<Value>,
}

/// 地物注册表，按名字索引；`features_per_step` 用 [`PlacedFeatureId`] 引用。
#[derive(Debug)]
pub struct Registry {
    features: Vec<FeatureDef>,
    feature_ids: HashMap<String, usize>,
    placed: Vec<PlacedFeatureDef>,
    placed_ids: HashMap<String, usize>,
    /// 每群系 11 个 step 的 placed feature 名字，群系按注册表 ID 索引。
    biome_features: Vec<Vec<Vec<String>>>,
    biome_ids: HashMap<String, usize>,
    /// `FeatureSorter` 排好的每步全局顺序；每个元素是 `placed` 的下标，
    /// 下标本身即 `setFeatureSeed(decorationSeed, index, step)` 用的 index。
    steps: Vec<Vec<u32>>,
}

/// `placed` 里的下标。
pub type PlacedFeatureId = u32;

impl Registry {
    /// 从 `worldgen.json` 的顶层对象解析。
    pub fn parse(root: &serde_json::Map<String, Value>) -> Result<Self, String> {
        let feature_object = object_field(root, "feature")?;
        let placed_object = object_field(root, "placed_feature")?;
        let biome_object = object_field(root, "biome_features")?;
        let steps_value = root
            .get("features_per_step")
            .and_then(Value::as_array)
            .ok_or("missing features_per_step")?;
        if steps_value.len() != crate::feature::DECORATION_STEPS {
            return Err(format!(
                "features_per_step has {} steps, expected {}",
                steps_value.len(),
                crate::feature::DECORATION_STEPS
            ));
        }

        let mut features = Vec::with_capacity(feature_object.len());
        let mut feature_ids = HashMap::with_capacity(feature_object.len());
        for (name, value) in feature_object.iter() {
            let object = value
                .as_object()
                .ok_or_else(|| format!("feature {name} is not an object"))?;
            let kind = object
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("feature {name} has no type"))?
                .to_owned();
            feature_ids.insert(name.clone(), features.len());
            features.push(FeatureDef {
                name: name.clone(),
                kind,
                config: value.clone(),
            });
        }

        let mut placed = Vec::with_capacity(placed_object.len());
        let mut placed_ids = HashMap::with_capacity(placed_object.len());
        for (name, value) in placed_object.iter() {
            let object = value
                .as_object()
                .ok_or_else(|| format!("placed feature {name} is not an object"))?;
            let feature = object
                .get("feature")
                .ok_or_else(|| format!("placed feature {name} has no feature"))?
                .clone();
            let placement = object
                .get("placement")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("placed feature {name} has no placement"))?
                .clone();
            placed_ids.insert(name.clone(), placed.len());
            placed.push(PlacedFeatureDef {
                name: name.clone(),
                feature,
                placement,
            });
        }

        let mut biome_ids = HashMap::with_capacity(biome_object.len());
        let mut biome_features = Vec::with_capacity(biome_object.len());
        for (name, value) in biome_object.iter() {
            let steps = value
                .as_array()
                .ok_or_else(|| format!("biome {name} features is not an array"))?;
            // 官方 `BiomeGenerationSettings.features()` 允许短于 11 步：
            // `applyBiomeDecoration` 对越界的 step 直接跳过（字节码 484–496）。
            // 末地/下界几个群系只有 0..10 步，不能按 11 强校验。
            if steps.len() > crate::feature::DECORATION_STEPS {
                return Err(format!(
                    "biome {name} has {} feature steps, more than {}",
                    steps.len(),
                    crate::feature::DECORATION_STEPS
                ));
            }
            let mut parsed_steps = Vec::with_capacity(crate::feature::DECORATION_STEPS);
            for (index, step) in steps.iter().enumerate() {
                let entries = step
                    .as_array()
                    .ok_or_else(|| format!("biome {name} step {index} is not an array"))?;
                let mut names = Vec::with_capacity(entries.len());
                for entry in entries {
                    let id = entry.as_str().ok_or_else(|| {
                        format!("biome {name} step {index} has a non-string entry")
                    })?;
                    if !placed_ids.contains_key(id) {
                        return Err(format!(
                            "biome {name} references unknown placed feature {id}"
                        ));
                    }
                    names.push(id.to_owned());
                }
                parsed_steps.push(names);
            }
            parsed_steps.resize_with(crate::feature::DECORATION_STEPS, Vec::new);
            biome_ids.insert(name.clone(), biome_features.len());
            biome_features.push(parsed_steps);
        }

        let mut steps = Vec::with_capacity(steps_value.len());
        for (index, step) in steps_value.iter().enumerate() {
            let entries = step
                .as_array()
                .ok_or_else(|| format!("features_per_step[{index}] is not an array"))?;
            let mut ids = Vec::with_capacity(entries.len());
            for entry in entries {
                let name = entry
                    .as_str()
                    .ok_or_else(|| format!("features_per_step[{index}] has a non-string entry"))?;
                let id = *placed_ids
                    .get(name)
                    .ok_or_else(|| format!("features_per_step references unknown {name}"))?;
                ids.push(id as u32);
            }
            steps.push(ids);
        }

        Ok(Self {
            features,
            feature_ids,
            placed,
            placed_ids,
            biome_features,
            biome_ids,
            steps,
        })
    }

    pub fn feature(&self, id: usize) -> Option<&FeatureDef> {
        self.features.get(id)
    }

    pub fn feature_id(&self, name: &str) -> Option<usize> {
        self.feature_ids.get(name).copied()
    }

    pub fn placed(&self, id: PlacedFeatureId) -> Option<&PlacedFeatureDef> {
        self.placed.get(id as usize)
    }

    pub fn placed_id(&self, name: &str) -> Option<PlacedFeatureId> {
        self.placed_ids.get(name).copied().map(|id| id as u32)
    }

    /// 每个 step 的全局有序 `PlacedFeatureId` 列表。
    pub fn steps(&self) -> &[Vec<u32>] {
        &self.steps
    }

    /// 某个群系某个 step 的 placed feature 名字列表。
    pub fn biome_features(&self, biome_id: u32, step: usize) -> &[String] {
        static EMPTY: Vec<String> = Vec::new();
        self.biome_features
            .get(biome_id as usize)
            .and_then(|steps| steps.get(step))
            .map(Vec::as_slice)
            .unwrap_or(&EMPTY)
    }

    pub fn biome_id(&self, name: &str) -> Option<u32> {
        self.biome_ids.get(name).copied().map(|id| id as u32)
    }

    /// 一个群系是否包含某个 placed feature（任意 step），对应
    /// `BiomeGenerationSettings.hasFeature`。
    pub fn biome_has_feature(&self, biome_id: u32, placed: PlacedFeatureId) -> bool {
        let Some(name) = self.placed.get(placed as usize).map(|p| p.name.as_str()) else {
            return false;
        };
        self.biome_features
            .get(biome_id as usize)
            .is_some_and(|steps| steps.iter().any(|step| step.iter().any(|n| n == name)))
    }
}

fn object_field<'a>(
    root: &'a serde_json::Map<String, Value>,
    name: &str,
) -> Result<&'a serde_json::Map<String, Value>, String> {
    root.get(name)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("missing worldgen field {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Worldgen;

    #[test]
    fn parses_the_exported_feature_registries() {
        let text = mc_bamboo_data::worldgen::WORLDGEN;
        let root: serde_json::Value = serde_json::from_str(text).unwrap();
        let registry = Registry::parse(root.as_object().unwrap()).unwrap();
        // 和 Extractor 导出的条目数一致。
        assert_eq!(registry.features.len(), 240);
        assert_eq!(registry.placed.len(), 273);
        assert_eq!(registry.biome_features.len(), 67);
        assert_eq!(registry.steps.len(), crate::feature::DECORATION_STEPS);

        // step 6 是 34 个全局有序矿石，第一个是 ore_dirt。
        let step6 = &registry.steps[6];
        assert_eq!(step6.len(), 34);
        let first = registry.placed(step6[0]).unwrap();
        assert_eq!(first.name, "minecraft:ore_dirt");
        assert_eq!(first.feature.as_str(), Some("minecraft:ore_dirt"));

        // 每个群系都是 11 步，平原的 step 6 以 ore_dirt 开头。
        let plains = registry.biome_id("minecraft:plains").unwrap();
        assert_eq!(
            registry.biome_features[plains as usize].len(),
            crate::feature::DECORATION_STEPS
        );
        assert_eq!(
            registry
                .biome_features(plains, 6)
                .first()
                .map(String::as_str),
            Some("minecraft:ore_dirt")
        );
        assert!(registry.biome_has_feature(plains, step6[0]));

        // Worldgen::load 也会核对这四个键。
        let _ = Worldgen::load().unwrap();
    }
}
