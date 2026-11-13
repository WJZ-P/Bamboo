//! `FeaturePlacer` 的栈 DFS 和 26.3 的 `PlacementModifier`。
//!
//! 26.3 把旧的 `Stream<BlockPos>` 链改成了显式栈 DFS（`FeaturePlacer.place` 字节码
//! 598–780）：`positions` 和 `modifierIndices` 两个平行栈，每轮 pop 一个位置和它
//! 要走的 modifier 下标；最后一个 modifier 产生的候选位置按正序交给 `Feature.place`。
//! 中间层的候选位置倒序压栈，所以最终消费随机数的顺序和顺序展开不同，必须照抄。

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    provider::{IntProvider, WorldGenerationContext},
    registry::Registry,
    state::parse_state,
    world::{FeatureWorld, HeightmapKind},
};

/// 一个 `PlacementModifier`。
#[derive(Debug, Clone)]
pub enum Modifier {
    Count(IntProvider),
    RarityFilter(i32),
    InSquare,
    HeightRange(super::provider::HeightProvider),
    Heightmap(HeightmapKind),
    Biome,
    Offset {
        x: IntProvider,
        y: IntProvider,
        z: IntProvider,
    },
    /// 已解析但本轮不生效的修饰器（例如 `surface_relative_threshold_filter`）。
    /// 命中时置 `unsupported`，让对拍跳过这个区块。
    Unsupported(String),
}

impl Modifier {
    pub fn parse(value: &Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or("placement modifier must be an object")?;
        let kind = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or("placement modifier has no type")?;
        let field = |name: &str| -> Result<&Value, String> {
            object
                .get(name)
                .ok_or_else(|| format!("{kind} has no {name}"))
        };
        Ok(match kind {
            "minecraft:count" => Self::Count(IntProvider::parse(field("count")?)?),
            "minecraft:rarity_filter" => Self::RarityFilter(
                field("chance")?
                    .as_i64()
                    .ok_or("rarity_filter chance is not an int")? as i32,
            ),
            "minecraft:in_square" => Self::InSquare,
            "minecraft:height_range" => {
                Self::HeightRange(super::provider::HeightProvider::parse(field("height")?)?)
            }
            "minecraft:heightmap" => Self::Heightmap(HeightmapKind::parse(
                field("heightmap")?
                    .as_str()
                    .ok_or("heightmap is not a string")?,
            )?),
            "minecraft:biome" => Self::Biome,
            "minecraft:offset" => Self::Offset {
                x: IntProvider::parse(field("x")?)?,
                y: IntProvider::parse(field("y")?)?,
                z: IntProvider::parse(field("z")?)?,
            },
            _ => Self::Unsupported(kind.to_owned()),
        })
    }

    pub fn is_supported(&self) -> bool {
        !matches!(self, Self::Unsupported(_))
    }
}

/// `FeaturePlacer.place(PlacedFeature, random, origin)` 需要的上下文。
pub struct PlacementContext<'a> {
    /// 修饰器链只读世界（`getHeight` 等）；放置阶段另借可变引用。
    pub world: &'a FeatureWorld<'a>,
    pub generation: WorldGenerationContext,
    /// `BiomeFilter` 要问 `level.getBiome(pos)`：由主循环给一个逐点群系回调。
    pub biome_at: &'a mut dyn FnMut(i32, i32, i32) -> u32,
    pub registry: &'a Registry,
    /// `PlacementContext.topFeature()`：`placeWithBiomeCheck` 才有。
    pub top_feature: Option<u32>,
}

/// 与 `FeatureWorld` 无关的修饰器链参数，单独放一份，方便在每次 `modify` 时
/// 临时构造一个短生命周期的 [`PlacementContext`]。
pub struct PlacementEnv<'a> {
    pub generation: WorldGenerationContext,
    pub biome_at: &'a mut dyn FnMut(i32, i32, i32) -> u32,
    pub registry: &'a Registry,
    pub top_feature: Option<u32>,
}

/// 一个候选位置。
type Pos = (i32, i32, i32);

/// `PlacementModifier.modify(ctx, random, pos, consumer)`。
fn modify(
    modifier: &Modifier,
    ctx: &mut PlacementContext<'_>,
    random: &WorldgenRandom,
    pos: Pos,
    out: &mut Vec<Pos>,
) {
    let (x, y, z) = pos;
    match modifier {
        Modifier::Count(provider) => {
            let count = provider.sample(random);
            for _ in 0..count.max(0) {
                out.push(pos);
            }
        }
        Modifier::RarityFilter(chance) => {
            if *chance > 0 && random.next_float() < 1.0 / *chance as f32 {
                out.push(pos);
            }
        }
        Modifier::InSquare => {
            out.push((x + random.next_int(16), y, z + random.next_int(16)));
        }
        Modifier::HeightRange(provider) => {
            out.push((x, provider.sample(random, ctx.generation), z));
        }
        Modifier::Heightmap(kind) => {
            let height = ctx.world.get_height(*kind, x, z);
            if height > ctx.generation.min_y {
                out.push((x, height, z));
            }
        }
        Modifier::Offset {
            x: dx,
            y: dy,
            z: dz,
        } => {
            out.push((
                x + dx.sample(random),
                y + dy.sample(random),
                z + dz.sample(random),
            ));
        }
        Modifier::Biome => {
            let Some(placed) = ctx.top_feature else {
                return;
            };
            let biome = (ctx.biome_at)(x, y, z);
            if ctx.registry.biome_has_feature(biome, placed) {
                out.push(pos);
            }
        }
        Modifier::Unsupported(_) => {}
    }
}

/// `FeaturePlacer` 的栈 DFS；每遇到最后一个 modifier 产生的候选位置就立刻调
/// `place` 回调。
///
/// **不能先收集全部候选位置再放置**：官方在栈 DFS 过程中交错执行
/// `PlacementModifier.modify` 和 `Feature.place`，而 `Feature.place` 会消费
/// 随机数；后续候选位置的 `in_square` / `height_range` 求值会读到被它推进过的
/// 随机源。先收集会得到一个完全不同的随机序列。
pub fn place_candidates(
    modifiers: &[Modifier],
    world: &mut FeatureWorld<'_>,
    env: &mut PlacementEnv<'_>,
    random: &WorldgenRandom,
    origin: Pos,
    place: &mut dyn FnMut(&mut FeatureWorld<'_>, Pos),
) {
    if modifiers.is_empty() {
        place(world, origin);
        return;
    }
    let mut positions = vec![origin];
    let mut indices = vec![0usize];
    let mut modified: Vec<Pos> = Vec::new();
    while let (Some(pos), Some(index)) = (positions.pop(), indices.pop()) {
        modified.clear();
        {
            // `modify` 只读世界，借用在这一个作用域里结束，放置回调才能可变借用。
            let frozen: &FeatureWorld<'_> = world;
            let mut ctx = PlacementContext {
                world: frozen,
                generation: env.generation,
                biome_at: &mut *env.biome_at,
                registry: env.registry,
                top_feature: env.top_feature,
            };
            modify(&modifiers[index], &mut ctx, random, pos, &mut modified);
        }
        if index + 1 < modifiers.len() {
            // 字节码 237-294：倒序压栈。
            for &next in modified.iter().rev() {
                positions.push(next);
                indices.push(index + 1);
            }
        } else {
            // 字节码 300-378：最后一个 modifier，正序放置。
            for &candidate in &modified {
                place(world, candidate);
            }
        }
    }
}

/// `OreFeature` 的配置：`targets[].target` 是 `RuleTest`，`targets[].state` 是方块状态。
#[derive(Debug, Clone)]
pub struct OreFeature {
    pub targets: Vec<(super::rule_test::RuleTest, u32)>,
    pub size: i32,
    pub discard_chance_on_air_exposure: f32,
}

impl OreFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config.as_object().ok_or("ore feature must be an object")?;
        let size = object
            .get("size")
            .and_then(Value::as_i64)
            .ok_or("ore feature has no size")? as i32;
        let discard = object
            .get("discard_chance_on_air_exposure")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as f32;
        let targets = object
            .get("targets")
            .and_then(Value::as_array)
            .ok_or("ore feature has no targets")?;
        let mut parsed = Vec::with_capacity(targets.len());
        for target in targets {
            let rule = super::rule_test::RuleTest::parse(
                target.get("target").ok_or("ore target has no target")?,
            )?;
            let state = parse_state(target.get("state").ok_or("ore target has no state")?)?;
            parsed.push((rule, state));
        }
        Ok(Self {
            targets: parsed,
            size,
            discard_chance_on_air_exposure: discard,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_step_six_modifiers() {
        let cases = [
            (r#"{"type":"minecraft:count","count":30}"#, "count"),
            (r#"{"type":"minecraft:in_square"}"#, "in_square"),
            (r#"{"type":"minecraft:biome"}"#, "biome"),
            (r#"{"type":"minecraft:rarity_filter","chance":2}"#, "rarity"),
            (
                r#"{"type":"minecraft:heightmap","heightmap":"OCEAN_FLOOR_WG"}"#,
                "heightmap",
            ),
        ];
        for (text, label) in cases {
            let value: Value = serde_json::from_str(text).unwrap();
            assert!(Modifier::parse(&value).unwrap().is_supported(), "{label}");
        }
        let unknown: Value =
            serde_json::from_str(r#"{"type":"minecraft:surface_water_depth_filter"}"#).unwrap();
        assert!(!Modifier::parse(&unknown).unwrap().is_supported());
    }
}
