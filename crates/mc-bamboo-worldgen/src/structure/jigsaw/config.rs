//! `JigsawStructure` 的字段解析与 `HeightProvider.sample`。
//!
//! 照官方 `JigsawStructure.CODEC` / `UniformHeight.sample` / `Mth.randomBetweenInclusive`
//! / `MaxDistance` / `DimensionPadding` 用 Rust 重写。本轮支持 `start_height` 的
//! constant 与 uniform（覆盖主世界全部 jigsaw 结构），其他 provider 标 unsupported。
//!
//! Parse `JigsawStructure` fields and `HeightProvider.sample` following vanilla
//! `JigsawStructure.CODEC` / `UniformHeight.sample` / `Mth.randomBetweenInclusive` /
//! `MaxDistance` / `DimensionPadding`. Constant and uniform `start_height` are
//! supported (they cover every overworld jigsaw structure); others are unsupported.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::random::WorldgenRandom;

/// `WorldGenerationContext`：主世界生成器与高度访问器一致，取 settings 的三个值。
///
/// `WorldGenerationContext`: for the overworld the generator and height accessor agree,
/// so use the three settings values.
#[derive(Debug, Clone, Copy)]
pub struct WorldGenerationContext {
    pub min_y: i32,
    pub height: i32,
    pub sea_level: i32,
}

/// `start_height`。`ConstantHeight` 不消耗随机数；`UniformHeight.sample` 用
/// `Mth.randomBetweenInclusive(random, min, max)` = `min + nextInt(max-min+1)`。
///
/// `start_height`. `ConstantHeight` draws nothing; `UniformHeight.sample` uses
/// `Mth.randomBetweenInclusive(random, min, max)` = `min + nextInt(max-min+1)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeightProvider {
    Constant(i32),
    Uniform { min: i32, max: i32 },
}

impl HeightProvider {
    pub fn parse(raw: Option<&Value>) -> Result<Self, String> {
        let raw = raw.ok_or("missing start_height")?;
        if let Some(absolute) = raw.get("absolute").and_then(Value::as_i64) {
            return Ok(Self::Constant(absolute as i32));
        }
        match raw.get("type").and_then(Value::as_str) {
            Some("minecraft:uniform") => Ok(Self::Uniform {
                min: absolute_anchor(raw.get("min_inclusive"))?,
                max: absolute_anchor(raw.get("max_inclusive"))?,
            }),
            other => Err(format!("unsupported start_height {other:?}")),
        }
    }

    pub fn sample(&self, random: &WorldgenRandom, _context: &WorldGenerationContext) -> i32 {
        match self {
            Self::Constant(value) => *value,
            Self::Uniform { min, max } => {
                if min > max {
                    *min
                } else {
                    min + random.next_int(max - min + 1)
                }
            }
        }
    }
}

/// `JigsawStructure` 的配置字段。
///
/// The `JigsawStructure` config fields.
#[derive(Debug, Clone, PartialEq)]
pub struct JigsawConfig {
    pub start_pool: String,
    pub start_jigsaw_name: Option<String>,
    pub max_depth: i32,
    pub start_height: HeightProvider,
    pub use_expansion_hack: bool,
    pub project_start_to_heightmap: Option<String>,
    /// `(horizontal, vertical)`；int 简写是 `(v, v)`。
    pub max_distance: (i32, i32),
    /// `(bottom, top)`，默认 `ZERO` = `(0, 0)`。
    pub dimension_padding: (i32, i32),
    pub liquid_settings: String,
    /// 原始 `pool_aliases`，`PoolAliasLookup` 移植前先保留。
    pub pool_aliases: Vec<Value>,
}

impl JigsawConfig {
    /// 把任意一个 pool 当起始 pool 的合成配置：`maxDepth` 4、`use_expansion_hack` 开、
    /// 起点 Y 固定 64、不投影、`max_distance` 80、无别名、默认液体设置。`jigsaw pools`
    /// 对拍用它逐 pool 走 `addPieces`，覆盖 feature / list / empty 元素和每条拼接路径。
    ///
    /// Synthetic config treating an arbitrary pool as the start pool: `maxDepth` 4,
    /// `use_expansion_hack` on, fixed start Y 64, no projection, `max_distance` 80, no
    /// aliases and the default liquid settings. The `jigsaw pools` oracle uses it to run
    /// `addPieces` per pool, covering feature / list / empty elements and every
    /// attachment path.
    pub fn for_pool(pool: &str, start_y: i32) -> Self {
        Self {
            start_pool: pool.to_owned(),
            start_jigsaw_name: None,
            max_depth: 4,
            start_height: HeightProvider::Constant(start_y),
            use_expansion_hack: true,
            project_start_to_heightmap: None,
            max_distance: (80, 80),
            dimension_padding: (0, 0),
            liquid_settings: "apply_waterlogging".to_owned(),
            pool_aliases: Vec::new(),
        }
    }

    pub fn parse(raw: &Value) -> Result<Self, String> {
        Ok(Self {
            start_pool: string_field(raw, "start_pool")?,
            start_jigsaw_name: raw
                .get("start_jigsaw_name")
                .and_then(Value::as_str)
                .map(str::to_owned),
            max_depth: int_field(raw, "size")?,
            start_height: HeightProvider::parse(raw.get("start_height"))?,
            use_expansion_hack: raw
                .get("use_expansion_hack")
                .and_then(Value::as_bool)
                .ok_or("missing use_expansion_hack")?,
            project_start_to_heightmap: raw
                .get("project_start_to_heightmap")
                .and_then(Value::as_str)
                .map(str::to_owned),
            max_distance: max_distance(raw.get("max_distance_from_center"))?,
            dimension_padding: dimension_padding(raw.get("dimension_padding"))?,
            liquid_settings: raw
                .get("liquid_settings")
                .and_then(Value::as_str)
                .unwrap_or("apply_waterlogging")
                .to_owned(),
            pool_aliases: raw
                .get("pool_aliases")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        })
    }
}

/// `worldgen.json` 的 `structure` 对象里 `type == minecraft:jigsaw` 的条目。
///
/// The `minecraft:jigsaw` entries of `worldgen.json`'s `structure` object.
#[derive(Debug, Clone)]
pub struct JigsawStructures {
    configs: BTreeMap<String, JigsawConfig>,
}

impl JigsawStructures {
    pub fn parse(raw: &Value) -> Result<Self, String> {
        let object = raw.as_object().ok_or("structure is not an object")?;
        let mut configs = BTreeMap::new();
        for (name, value) in object {
            if value.get("type").and_then(Value::as_str) == Some("minecraft:jigsaw") {
                configs.insert(name.clone(), JigsawConfig::parse(value)?);
            }
        }
        Ok(Self { configs })
    }

    pub fn get(&self, name: &str) -> Option<&JigsawConfig> {
        self.configs.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.configs.keys()
    }

    pub fn len(&self) -> usize {
        self.configs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.configs.is_empty()
    }
}

fn absolute_anchor(raw: Option<&Value>) -> Result<i32, String> {
    raw.and_then(|value| value.get("absolute"))
        .and_then(Value::as_i64)
        .map(|value| value as i32)
        .ok_or_else(|| "only absolute vertical anchors are supported".to_owned())
}

fn max_distance(raw: Option<&Value>) -> Result<(i32, i32), String> {
    let raw = raw.ok_or("missing max_distance_from_center")?;
    if let Some(value) = raw.as_i64() {
        return Ok((value as i32, value as i32));
    }
    let horizontal = raw
        .get("horizontal")
        .and_then(Value::as_i64)
        .ok_or("max_distance has no horizontal")? as i32;
    // `FULL_CODEC` 的 vertical 默认 `DimensionType.Y_SIZE`；这里用一个足够大的值，
    // 它只会被 clamp 到世界高度范围。
    // `FULL_CODEC` defaults vertical to `DimensionType.Y_SIZE`; a large value is
    // enough here since it is always clamped to the world height range.
    let vertical = raw
        .get("vertical")
        .and_then(Value::as_i64)
        .unwrap_or(1 << 20) as i32;
    Ok((horizontal, vertical))
}

fn dimension_padding(raw: Option<&Value>) -> Result<(i32, i32), String> {
    match raw {
        None => Ok((0, 0)),
        Some(value) if value.as_i64().is_some() => {
            let padding = value.as_i64().unwrap() as i32;
            Ok((padding, padding))
        }
        Some(value) => Ok((
            value.get("bottom").and_then(Value::as_i64).unwrap_or(0) as i32,
            value.get("top").and_then(Value::as_i64).unwrap_or(0) as i32,
        )),
    }
}

fn string_field(raw: &Value, key: &str) -> Result<String, String> {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("jigsaw config has no {key}"))
}

fn int_field(raw: &Value, key: &str) -> Result<i32, String> {
    raw.get(key)
        .and_then(Value::as_i64)
        .map(|value| value as i32)
        .ok_or_else(|| format!("jigsaw config has no {key}"))
}
