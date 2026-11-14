//! `FeaturePlacer` 的栈 DFS 和 26.3 的 `PlacementModifier`。
//!
//! 26.3 把旧的 `Stream<BlockPos>` 链改成了显式栈 DFS（`FeaturePlacer.place` 字节码
//! 598–780）：`positions` 和 `modifierIndices` 两个平行栈，每轮 pop 一个位置和它
//! 要走的 modifier 下标；最后一个 modifier 产生的候选位置按正序交给 `Feature.place`。
//! 中间层的候选位置倒序压栈，所以最终消费随机数的顺序和顺序展开不同，必须照抄。

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    block_predicate::BlockPredicate,
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
    /// `BlockPredicateFilter`：不过滤时把候选位置原样放行，不消费随机数。
    ///
    /// `BlockPredicateFilter`: passes a candidate through unchanged, no random.
    BlockPredicateFilter(BlockPredicate),
    /// `surface_relative_threshold_filter`：按高度图差值过滤候选 y。
    ///
    /// `surface_relative_threshold_filter`: filters the candidate y by a heightmap delta.
    SurfaceRelativeThresholdFilter {
        heightmap: HeightmapKind,
        min_inclusive: i64,
        max_inclusive: i64,
    },
    /// 已解析但本轮不生效的修饰器。
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
            // 未移植的谓词解析成 `BlockPredicate::Unsupported`，整条修饰器置
            // unsupported；不把它当解析错误，避免让整个区块对拍直接报错。
            // An unported predicate becomes `BlockPredicate::Unsupported` and marks
            // the modifier unsupported, instead of failing the whole comparison.
            "minecraft:block_predicate_filter" => {
                let predicate = BlockPredicate::parse(field("predicate")?);
                if predicate.is_supported() {
                    Self::BlockPredicateFilter(predicate)
                } else {
                    Self::Unsupported(kind.to_owned())
                }
            }
            "minecraft:surface_relative_threshold_filter" => Self::SurfaceRelativeThresholdFilter {
                heightmap: HeightmapKind::parse(
                    field("heightmap")?
                        .as_str()
                        .ok_or("surface_relative_threshold_filter heightmap is not a string")?,
                )?,
                min_inclusive: object
                    .get("min_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(i32::MIN as i64),
                max_inclusive: object
                    .get("max_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(i32::MAX as i64),
            },
            _ => Self::Unsupported(kind.to_owned()),
        })
    }

    pub fn is_supported(&self) -> bool {
        match self {
            Self::Unsupported(_) | Self::BlockPredicateFilter(BlockPredicate::Unsupported) => false,
            Self::BlockPredicateFilter(predicate) => predicate.is_supported(),
            _ => true,
        }
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
        // `PlacementFilter.modify`：shouldPlace 为真才把 origin 原样放行。
        // `PlacementFilter.modify`: forward origin only when shouldPlace is true.
        Modifier::BlockPredicateFilter(predicate) => {
            if predicate.test(ctx.world, pos) {
                out.push(pos);
            }
        }
        Modifier::SurfaceRelativeThresholdFilter {
            heightmap,
            min_inclusive,
            max_inclusive,
        } => {
            // surfaceY = context.getHeight(heightmap, x, z)，官方用 long 算差值。
            // surfaceY = context.getHeight(heightmap, x, z); official uses long arithmetic.
            let surface_y = ctx.world.get_height(*heightmap, x, z) as i64;
            let min_y = surface_y + *min_inclusive;
            let max_y = surface_y + *max_inclusive;
            if min_y <= y as i64 && y as i64 <= max_y {
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
/// 一次 `Feature.place` 的结果：`supported` 表示涉及的分支是否都已移植，`placed`
/// 是官方 `FeaturePlacer.place` 的 `placedAny`（Sequence 的短路要用）。
///
/// Outcome of one `Feature.place`: `supported` says whether every branch is ported;
/// `placed` is the official `FeaturePlacer.place` `placedAny` flag.
#[derive(Debug, Clone, Copy)]
pub struct PlaceOutcome {
    pub supported: bool,
    pub placed: bool,
}

impl PlaceOutcome {
    pub fn unsupported() -> Self {
        Self {
            supported: false,
            placed: false,
        }
    }

    fn combine(&mut self, other: Self) {
        self.supported &= other.supported;
        self.placed |= other.placed;
    }
}

pub fn place_candidates(
    modifiers: &[Modifier],
    world: &mut FeatureWorld<'_>,
    env: &mut PlacementEnv<'_>,
    random: &WorldgenRandom,
    origin: Pos,
    place: &mut dyn FnMut(&mut FeatureWorld<'_>, &mut PlacementEnv<'_>, Pos) -> PlaceOutcome,
) -> PlaceOutcome {
    let mut total = PlaceOutcome {
        supported: true,
        placed: false,
    };
    if modifiers.is_empty() {
        total.combine(place(world, env, origin));
        return total;
    }
    let mut positions = vec![origin];
    let mut indices = vec![0usize];
    let mut modified: Vec<Pos> = Vec::new();
    while let (Some(pos), Some(index)) = (positions.pop(), indices.pop()) {
        modified.clear();
        {
            // `modify` 只读世界，借用在这一个作用域里结束，放置回调才能可变借用。
            // `modify` only reads the world; ending this scope lets the place callback borrow it.
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
                total.combine(place(world, env, candidate));
            }
        }
    }
    total
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
    fn parses_every_step_six_ore_modifier() {
        let text = mc_bamboo_data::worldgen::WORLDGEN;
        let root: Value = serde_json::from_str(text).unwrap();
        let registry = Registry::parse(root.as_object().unwrap()).unwrap();
        for &id in &registry.steps()[6] {
            let placed = registry.placed(id).unwrap();
            let (kind, config) = super::super::run::resolve_feature(&registry, &placed.feature)
                .unwrap_or_else(|| panic!("{} has no feature", placed.name));
            if kind != "minecraft:ore" {
                continue;
            }
            OreFeature::parse(config).unwrap_or_else(|e| panic!("{}: {e}", placed.name));
            for m in &placed.placement {
                Modifier::parse(m).unwrap_or_else(|e| panic!("{} modifier: {e}", placed.name));
            }
        }
    }

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

    #[test]
    fn parses_block_predicate_filter_and_flags_unported_predicates() {
        let air: Value = serde_json::from_str(
            r#"{"type":"minecraft:block_predicate_filter","predicate":{"type":"minecraft:matching_block_tag","tag":"minecraft:air"}}"#,
        )
        .unwrap();
        assert!(Modifier::parse(&air).unwrap().is_supported());
        // would_survive 的 sapling 存活判定已移植，整条修饰器现在支持。
        // A would_survive with a sapling has a ported survival check, so it is supported.
        let sapling: Value = serde_json::from_str(
            r#"{"type":"minecraft:block_predicate_filter","predicate":{"type":"minecraft:would_survive","state":"minecraft:oak_sapling"}}"#,
        )
        .unwrap();
        assert!(Modifier::parse(&sapling).unwrap().is_supported());
        // 存活判定未移植的 state 仍让整条修饰器 unsupported。
        // A state with an unported survival check still makes the modifier unsupported.
        let cane: Value = serde_json::from_str(
            r#"{"type":"minecraft:block_predicate_filter","predicate":{"type":"minecraft:would_survive","state":"minecraft:sugar_cane"}}"#,
        )
        .unwrap();
        assert!(!Modifier::parse(&cane).unwrap().is_supported());
    }
}
