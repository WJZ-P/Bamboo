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
    /// `RandomChancePlacement`：`nextFloat() < chance` 才放行。
    ///
    /// `RandomChancePlacement`: forward only when `nextFloat() < chance`.
    RandomChance(f32),
    /// `NoiseBasedCountPlacement`：用静态 `SimplexNoise` 算重复次数，不消费随机数。
    ///
    /// `NoiseBasedCountPlacement`: repeats via a static `SimplexNoise`, no random consumed.
    NoiseBasedCount {
        noise_to_count_ratio: i32,
        noise_factor: f64,
        noise_offset: f64,
    },
    /// `EnvironmentScanPlacement`：沿竖直方向逐格扫描，找到 `target_condition` 就放行。
    ///
    /// `EnvironmentScanPlacement`: step along one vertical direction until the
    /// `target_condition` holds, stopping when the search condition fails.
    EnvironmentScan {
        /// `Direction.VERTICAL_CODEC`：只有 up / down 两种。
        direction: i32,
        target: BlockPredicate,
        allowed_search: BlockPredicate,
        max_steps: i32,
    },
    /// `SurfaceRelativeThresholdFilter`：候选位置的 y 落在（地表面高度 + 上下限）之间。
    ///
    /// `SurfaceRelativeThresholdFilter`: keep the candidate only when its y lies within
    /// the surface height plus the configured bounds.
    SurfaceRelativeThreshold {
        heightmap: HeightmapKind,
        min_inclusive: i32,
        max_inclusive: i32,
    },
    /// `SurfaceWaterDepthFilter`：`WORLD_SURFACE - OCEAN_FLOOR <= maxWaterDepth`。
    ///
    /// Official `SurfaceWaterDepthFilter`: `WORLD_SURFACE - OCEAN_FLOOR <= maxWaterDepth`.
    SurfaceWaterDepthFilter(i32),
    /// 已解析但本轮不生效的修饰器（例如 `noise_threshold_count`）。
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
            // `RandomChancePlacement`：`chance` 是 float，范围 0..1。
            // `RandomChancePlacement`: `chance` is a float in 0..1.
            "minecraft:random_chance" => Self::RandomChance(
                field("chance")?
                    .as_f64()
                    .ok_or("random_chance chance is not a number")? as f32,
            ),
            // `SurfaceWaterDepthFilter`：`max_water_depth` 是 int。
            // `SurfaceWaterDepthFilter`: `max_water_depth` is an int.
            "minecraft:surface_water_depth_filter" => Self::SurfaceWaterDepthFilter(
                field("max_water_depth")?
                    .as_i64()
                    .ok_or_else(|| "surface_water_depth_filter has no max_water_depth".to_owned())?
                    as i32,
            ),
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
            // `NoiseBasedCountPlacement`：配置里三个数，缺省 noise_offset=0。
            // `NoiseBasedCountPlacement`: three config numbers, noise_offset defaults to 0.
            "minecraft:noise_based_count" => Self::NoiseBasedCount {
                noise_to_count_ratio: field("noise_to_count_ratio")?
                    .as_i64()
                    .ok_or("noise_based_count has no ratio")?
                    as i32,
                noise_factor: field("noise_factor")?
                    .as_f64()
                    .ok_or("noise_based_count has no factor")?,
                noise_offset: object
                    .get("noise_offset")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
            },
            // `EnvironmentScanPlacement`：`direction_of_search` 只允许 up / down；
            // `allowed_search_condition` 缺省是 `BlockPredicate.alwaysTrue()`。
            // `EnvironmentScanPlacement`: the search direction is up or down only;
            // `allowed_search_condition` defaults to `BlockPredicate.alwaysTrue()`.
            "minecraft:environment_scan" => {
                let direction = field("direction_of_search")?
                    .as_str()
                    .and_then(vertical_direction)
                    .ok_or("environment_scan has no vertical direction")?;
                let target = BlockPredicate::parse(field("target_condition")?);
                let allowed_search = object
                    .get("allowed_search_condition")
                    .map(BlockPredicate::parse)
                    .unwrap_or(BlockPredicate::True);
                if !target.is_supported() || !allowed_search.is_supported() {
                    Self::Unsupported(kind.to_owned())
                } else {
                    Self::EnvironmentScan {
                        direction,
                        target,
                        allowed_search,
                        max_steps: field("max_steps")?
                            .as_i64()
                            .ok_or("environment_scan has no max_steps")?
                            as i32,
                    }
                }
            }
            // `SurfaceRelativeThresholdFilter`：两个边界缺省是 i32 的 MIN/MAX。
            // `SurfaceRelativeThresholdFilter`: both bounds default to i32 MIN/MAX.
            "minecraft:surface_relative_threshold_filter" => Self::SurfaceRelativeThreshold {
                heightmap: HeightmapKind::parse(
                    field("heightmap")?
                        .as_str()
                        .ok_or("surface_relative_threshold_filter has no heightmap")?,
                )?,
                min_inclusive: object
                    .get("min_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(i64::from(i32::MIN)) as i32,
                max_inclusive: object
                    .get("max_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(i64::from(i32::MAX)) as i32,
            },
            _ => Self::Unsupported(kind.to_owned()),
        })
    }

    pub fn is_supported(&self) -> bool {
        match self {
            Self::Unsupported(_) | Self::BlockPredicateFilter(BlockPredicate::Unsupported) => false,
            Self::BlockPredicateFilter(predicate) => predicate.is_supported(),
            // 扫描的三个字段里两个是谓词，任一未移植就整条修饰器不支持。
            // Two of the scan's fields are predicates; an unported one disables the modifier.
            Self::EnvironmentScan {
                target,
                allowed_search,
                ..
            } => target.is_supported() && allowed_search.is_supported(),
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
        // `RandomChancePlacement.shouldPlace`：先取一次 nextFloat 再比。
        // `RandomChancePlacement.shouldPlace`: one nextFloat draw, then compare.
        Modifier::RandomChance(chance) => {
            if random.next_float() < *chance {
                out.push(pos);
            }
        }
        // `NoiseBasedCountPlacement.count`：`ceil((BIOME_INFO_NOISE.get(x/f, z/f) + offset) * ratio)`，
        // 然后把 origin 复制 count 次；不消费随机数。
        // `NoiseBasedCountPlacement.count`: ceil of the scaled static noise, then
        // duplicate the origin that many times; no random consumed.
        Modifier::NoiseBasedCount {
            noise_to_count_ratio,
            noise_factor,
            noise_offset,
        } => {
            let noise = crate::biome_temperature::biome_info_noise(
                f64::from(x) / *noise_factor,
                f64::from(z) / *noise_factor,
            );
            let count = (f64::from(noise) + *noise_offset) * f64::from(*noise_to_count_ratio);
            let count = count.ceil() as i32;
            for _ in 0..count.max(0) {
                out.push(pos);
            }
        }
        // `SurfaceWaterDepthFilter.shouldPlace`：两张高度图之差不超过上限才放行。
        // `SurfaceWaterDepthFilter.shouldPlace`: forward only when the two heightmaps
        // differ by at most the configured depth.
        Modifier::SurfaceWaterDepthFilter(max) => {
            let y_ocean = ctx.world.get_height(HeightmapKind::OceanFloor, x, z);
            let y_surface = ctx.world.get_height(HeightmapKind::WorldSurface, x, z);
            if y_surface - y_ocean <= *max {
                out.push(pos);
            }
        }
        Modifier::Unsupported(_) => {}
        // `EnvironmentScanPlacement`：不消费随机数，只按谓词逐格扫描。
        // `EnvironmentScanPlacement`: no random, purely a predicate walk.
        Modifier::EnvironmentScan {
            direction,
            target,
            allowed_search,
            max_steps,
        } => environment_scan(
            ctx.world,
            *direction,
            target,
            allowed_search,
            *max_steps,
            pos,
            out,
        ),
        // `SurfaceRelativeThresholdFilter.shouldPlace`：`surfaceY + min <= y <= surfaceY + max`，
        // 官方用 long 算，避免溢出。
        // `SurfaceRelativeThresholdFilter.shouldPlace`: `surfaceY + min <= y <= surfaceY + max`,
        // computed in long to avoid overflow.
        Modifier::SurfaceRelativeThreshold {
            heightmap,
            min_inclusive,
            max_inclusive,
        } => {
            let surface = i64::from(ctx.world.get_height(*heightmap, x, z));
            let y = i64::from(y);
            if surface + i64::from(*min_inclusive) <= y && y <= surface + i64::from(*max_inclusive)
            {
                out.push(pos);
            }
        }
    }
}

/// `EnvironmentScanPlacement.modify`：从 `origin` 开始逐格沿竖直方向走，最多
/// `max_steps` 步。先查 `allowed_search_condition`，再查 `target_condition`，
/// 命中就放行；走出去（`isOutsideBuildHeight`）直接返回，搜索条件不成立则提前停止，
/// 停止后再试一次 `target_condition`。
///
/// `EnvironmentScanPlacement.modify`: walk from `origin` along the vertical direction for
/// at most `max_steps` blocks. Test `allowed_search_condition` then `target_condition`;
/// emit the position on a target hit. Leaving the build height returns immediately, a
/// failed search condition stops the loop, and the target is tested once more afterwards.
fn environment_scan(
    world: &FeatureWorld<'_>,
    direction: i32,
    target: &BlockPredicate,
    allowed_search: &BlockPredicate,
    max_steps: i32,
    pos: Pos,
    out: &mut Vec<Pos>,
) {
    let (x, _, z) = pos;
    let mut y = pos.1;
    if !allowed_search.test(world, (x, y, z)) {
        return;
    }
    for _ in 0..max_steps {
        if target.test(world, (x, y, z)) {
            out.push((x, y, z));
            return;
        }
        y += direction;
        // `level.isOutsideBuildHeight(pos.getY())` ⇒ `y < minY || y >= minY + height`。
        // `level.isOutsideBuildHeight(pos.getY())` is `y < minY || y >= minY + height`.
        if y < world.min_y || y >= world.min_y + world.height {
            return;
        }
        if !allowed_search.test(world, (x, y, z)) {
            break;
        }
    }
    if target.test(world, (x, y, z)) {
        out.push((x, y, z));
    }
}

/// `Direction.VERTICAL_CODEC` 的名字到竖直方向：up 是 `+1`，down 是 `-1`。
///
/// `Direction.VERTICAL_CODEC` name to the vertical step: +1 for up, -1 for down.
fn vertical_direction(name: &str) -> Option<i32> {
    match name {
        "up" => Some(1),
        "down" => Some(-1),
        _ => None,
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
            serde_json::from_str(r#"{"type":"minecraft:fixed_placement"}"#).unwrap();
        assert!(!Modifier::parse(&unknown).unwrap().is_supported());
    }

    #[test]
    fn noise_based_count_parses_and_static_noise_is_stable() {
        let value: Value = serde_json::from_str(
            r#"{"noise_factor":80,"noise_to_count_ratio":120,"type":"minecraft:noise_based_count"}"#,
        )
        .unwrap();
        let modifier = Modifier::parse(&value).unwrap();
        assert!(modifier.is_supported());
        assert!(matches!(modifier, Modifier::NoiseBasedCount { .. }));
        // 静态噪声和世界种子无关，同一坐标两次求值必须完全一致且有限。
        // The static noise is seed-independent: identical and finite for one input.
        let first = crate::biome_temperature::biome_info_noise(12.5, -7.25);
        let second = crate::biome_temperature::biome_info_noise(12.5, -7.25);
        assert_eq!(first, second);
        assert!(first.is_finite());
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
        let mushroom: Value = serde_json::from_str(
            r#"{"type":"minecraft:block_predicate_filter","predicate":{"type":"minecraft:would_survive","state":"minecraft:pale_moss_carpet"}}"#,
        )
        .unwrap();
        assert!(!Modifier::parse(&mushroom).unwrap().is_supported());
        // 甘蔗 / 仙人掌的存活判定已移植，整条修饰器现在支持。
        // Sugar cane / cactus survival is ported, so the modifier is now supported.
        for name in ["minecraft:sugar_cane", "minecraft:cactus"] {
            let text = format!(
                r#"{{"type":"minecraft:block_predicate_filter","predicate":{{"type":"minecraft:would_survive","state":"{name}"}}}}"#
            );
            let value: Value = serde_json::from_str(&text).unwrap();
            assert!(Modifier::parse(&value).unwrap().is_supported(), "{name}");
        }
    }
}
