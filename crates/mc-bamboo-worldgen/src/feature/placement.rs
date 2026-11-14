//! `FeaturePlacer` 的栈 DFS 和 26.3 的 `PlacementModifier`。
//!
//! 26.3 把旧的 `Stream<BlockPos>` 链改成了显式栈 DFS（`FeaturePlacer.place` 字节码
//! 598–780）：`positions` 和 `modifierIndices` 两个平行栈，每轮 pop 一个位置和它
//! 要走的 modifier 下标；最后一个 modifier 产生的候选位置按正序交给 `Feature.place`。
//! 中间层的候选位置倒序压栈，所以最终消费随机数的顺序和顺序展开不同，必须照抄。

use serde_json::Value;

use mc_bamboo_data::{block_id_of_state, blocks, facts, state_facts};

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
    /// `NoiseThresholdCountPlacement`：BIOME_INFO_NOISE 阈值决定重复次数。
    NoiseThresholdCount {
        noise_level: f64,
        below_noise: i32,
        above_noise: i32,
    },
    /// `NoiseBasedCountPlacement`：噪声线性映射后 ceil 决定重复次数。
    NoiseBasedCount {
        noise_to_count_ratio: i32,
        noise_factor: f64,
        noise_offset: f64,
    },
    /// `SurfaceRelativeThresholdFilter`：按高度图差值过滤候选 y（复用 `cb-wg-feat-a`）。
    SurfaceRelativeThresholdFilter {
        heightmap: HeightmapKind,
        min_inclusive: i64,
        max_inclusive: i64,
    },
    /// `SurfaceWaterDepthFilter`：WORLD_SURFACE - OCEAN_FLOOR <= max_water_depth。
    SurfaceWaterDepthFilter(i32),
    /// `EnvironmentScanPlacement`：沿竖直方向扫描满足 target 的位置。
    EnvironmentScan {
        step: i32,
        target: BlockPredicate,
        allowed: BlockPredicate,
        max_steps: i32,
    },
    /// `CountOnEveryLayerPlacement`：每层地面各放 count 个候选。
    CountOnEveryLayer(IntProvider),
    /// 已解析但本轮不生效的修饰器（例如含未移植谓词的 `environment_scan`）。
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
            // 两个 `RepeatingPlacement.count`：噪声是静态的，不消费随机数。
            // Two `RepeatingPlacement.count` variants: the noise is static, no random consumed.
            "minecraft:noise_threshold_count" => Self::NoiseThresholdCount {
                noise_level: field("noise_level")?.as_f64().ok_or("bad noise_level")?,
                below_noise: field("below_noise")?.as_i64().ok_or("bad below_noise")? as i32,
                above_noise: field("above_noise")?.as_i64().ok_or("bad above_noise")? as i32,
            },
            "minecraft:noise_based_count" => Self::NoiseBasedCount {
                noise_to_count_ratio: field("noise_to_count_ratio")?
                    .as_i64()
                    .ok_or("bad noise_to_count_ratio")?
                    as i32,
                noise_factor: field("noise_factor")?.as_f64().ok_or("bad noise_factor")?,
                noise_offset: object
                    .get("noise_offset")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
            },
            // `SurfaceRelativeThresholdFilter` 与 `cb-wg-feat-a` 的实现一致。
            "minecraft:surface_relative_threshold_filter" => Self::SurfaceRelativeThresholdFilter {
                heightmap: HeightmapKind::parse(
                    field("heightmap")?
                        .as_str()
                        .ok_or("surface_relative_threshold_filter heightmap is not a string")?,
                )?,
                min_inclusive: object
                    .get("min_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(i64::from(i32::MIN)),
                max_inclusive: object
                    .get("max_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(i64::from(i32::MAX)),
            },
            "minecraft:surface_water_depth_filter" => Self::SurfaceWaterDepthFilter(
                field("max_water_depth")?
                    .as_i64()
                    .ok_or("bad max_water_depth")? as i32,
            ),
            "minecraft:environment_scan" => {
                let step = match field("direction_of_search")?.as_str() {
                    Some("up") => 1,
                    Some("down") => -1,
                    _ => return Err("environment_scan direction is not up/down".to_owned()),
                };
                let target = BlockPredicate::parse(field("target_condition")?);
                let allowed = object
                    .get("allowed_search_condition")
                    .map(BlockPredicate::parse)
                    .unwrap_or(BlockPredicate::True);
                let max_steps = field("max_steps")?.as_i64().ok_or("bad max_steps")? as i32;
                if !target.is_supported() || !allowed.is_supported() {
                    return Ok(Self::Unsupported(kind.to_owned()));
                }
                Self::EnvironmentScan {
                    step,
                    target,
                    allowed,
                    max_steps,
                }
            }
            "minecraft:count_on_every_layer" => {
                Self::CountOnEveryLayer(IntProvider::parse(field("count")?)?)
            }
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
        // `Biome.BIOME_INFO_NOISE.get(x/200, z/200)` 与阈值比较，重复 below/above 次。
        Modifier::NoiseThresholdCount {
            noise_level,
            below_noise,
            above_noise,
        } => {
            let noise = f64::from(crate::biome_temperature::biome_info_noise(
                f64::from(x) / 200.0,
                f64::from(z) / 200.0,
            ));
            let count = if noise < *noise_level {
                *below_noise
            } else {
                *above_noise
            };
            for _ in 0..count.max(0) {
                out.push(pos);
            }
        }
        // `ceil((noise + offset) * ratio)` 次，噪声按 `x/noiseFactor, z/noiseFactor` 取样。
        Modifier::NoiseBasedCount {
            noise_to_count_ratio,
            noise_factor,
            noise_offset,
        } => {
            let noise = f64::from(crate::biome_temperature::biome_info_noise(
                f64::from(x) / *noise_factor,
                f64::from(z) / *noise_factor,
            ));
            let count = ((noise + *noise_offset) * f64::from(*noise_to_count_ratio)).ceil() as i32;
            for _ in 0..count.max(0) {
                out.push(pos);
            }
        }
        Modifier::SurfaceRelativeThresholdFilter {
            heightmap,
            min_inclusive,
            max_inclusive,
        } => {
            let surface_y = i64::from(ctx.world.get_height(*heightmap, x, z));
            let min_y = surface_y + *min_inclusive;
            let max_y = surface_y + *max_inclusive;
            if min_y <= i64::from(y) && i64::from(y) <= max_y {
                out.push(pos);
            }
        }
        Modifier::SurfaceWaterDepthFilter(max_depth) => {
            let ocean = ctx.world.get_height(HeightmapKind::OceanFloor, x, z);
            let surface = ctx.world.get_height(HeightmapKind::WorldSurface, x, z);
            if surface - ocean <= *max_depth {
                out.push(pos);
            }
        }
        Modifier::EnvironmentScan {
            step,
            target,
            allowed,
            max_steps,
        } => {
            let mut scan_y = y;
            if allowed.test(ctx.world, pos) {
                for _ in 0..*max_steps {
                    if target.test(ctx.world, (x, scan_y, z)) {
                        out.push((x, scan_y, z));
                        return;
                    }
                    scan_y += *step;
                    let max_y = ctx.world.min_y + ctx.world.height - 1;
                    if scan_y < ctx.world.min_y || scan_y > max_y {
                        return;
                    }
                    if !allowed.test(ctx.world, (x, scan_y, z)) {
                        break;
                    }
                }
                if target.test(ctx.world, (x, scan_y, z)) {
                    out.push((x, scan_y, z));
                }
            }
        }
        Modifier::CountOnEveryLayer(provider) => {
            let mut layer = 0_i32;
            loop {
                let mut found = false;
                let mut i = 0_i32;
                loop {
                    // 官方 for 循环的条件每次都调 `count.sample(random)`。
                    // The official for-condition calls `count.sample(random)` every iteration.
                    if i >= provider.sample(random) {
                        break;
                    }
                    let cx = x + random.next_int(16);
                    let cz = z + random.next_int(16);
                    let start_y = ctx.world.get_height(HeightmapKind::MotionBlocking, cx, cz);
                    if let Some(found_y) = find_on_ground_y(ctx.world, cx, start_y, cz, layer) {
                        out.push((cx, found_y, cz));
                        found = true;
                    }
                    i += 1;
                }
                layer += 1;
                if !found {
                    break;
                }
            }
        }
        Modifier::Unsupported(_) => {}
    }
}

/// `CountOnEveryLayerPlacement.findOnGroundYPosition`：向下找第 `layer` 层
/// 「下面非空、当前空、下面不是基岩」的位置，返回可放置的 y。
///
/// `CountOnEveryLayerPlacement.findOnGroundYPosition`: scan down for the `layer`-th
/// ground surface (below non-empty, current empty, below not bedrock).
fn find_on_ground_y(
    world: &FeatureWorld<'_>,
    x: i32,
    start_y: i32,
    z: i32,
    layer: i32,
) -> Option<i32> {
    let mut current = world.get_block(x, start_y, z);
    let mut current_layer = 0_i32;
    for y in (world.min_y + 1..=start_y).rev() {
        let below = world.get_block(x, y - 1, z);
        if !count_layer_is_empty(below)
            && count_layer_is_empty(current)
            && !count_layer_is_bedrock(below)
        {
            if current_layer == layer {
                return Some(y);
            }
            current_layer += 1;
        }
        current = below;
    }
    None
}

/// `CountOnEveryLayerPlacement.isEmpty` = isAir || water || lava。
fn count_layer_is_empty(state: u32) -> bool {
    if state_facts(state) & facts::AIR != 0 {
        return true;
    }
    block_id_of_state(state).is_some_and(|id| {
        id == block_id_of_state(blocks::WATER).unwrap_or(usize::MAX)
            || id == block_id_of_state(blocks::LAVA).unwrap_or(usize::MAX)
    })
}

fn count_layer_is_bedrock(state: u32) -> bool {
    block_id_of_state(state) == block_id_of_state(blocks::BEDROCK)
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
            serde_json::from_str(r#"{"type":"minecraft:not_a_real_modifier"}"#).unwrap();
        assert!(!Modifier::parse(&unknown).unwrap().is_supported());
    }

    #[test]
    fn parses_surface_scan_and_layer_modifiers() {
        let cases = [
            r#"{"type":"minecraft:surface_relative_threshold_filter","heightmap":"OCEAN_FLOOR_WG","max_inclusive":-13}"#,
            r#"{"type":"minecraft:surface_water_depth_filter","max_water_depth":0}"#,
            r#"{"type":"minecraft:environment_scan","direction_of_search":"down","target_condition":{"type":"minecraft:solid"},"allowed_search_condition":{"type":"minecraft:matching_block_tag","tag":"minecraft:air"},"max_steps":12}"#,
            r#"{"type":"minecraft:count_on_every_layer","count":6}"#,
        ];
        for text in cases {
            let value: Value = serde_json::from_str(text).unwrap();
            assert!(Modifier::parse(&value).unwrap().is_supported(), "{text}");
        }
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

    #[test]
    fn parses_noise_count_modifiers() {
        let threshold: Value = serde_json::from_str(
            r#"{"type":"minecraft:noise_threshold_count","noise_level":-0.8,"below_noise":4,"above_noise":1}"#,
        )
        .unwrap();
        assert!(matches!(
            Modifier::parse(&threshold).unwrap(),
            Modifier::NoiseThresholdCount {
                below_noise: 4,
                above_noise: 1,
                ..
            }
        ));
        assert!(Modifier::parse(&threshold).unwrap().is_supported());
        let based: Value = serde_json::from_str(
            r#"{"type":"minecraft:noise_based_count","noise_to_count_ratio":10,"noise_factor":200.0,"noise_offset":0.3}"#,
        )
        .unwrap();
        assert!(matches!(
            Modifier::parse(&based).unwrap(),
            Modifier::NoiseBasedCount {
                noise_to_count_ratio: 10,
                ..
            }
        ));
        assert!(Modifier::parse(&based).unwrap().is_supported());
        // `noise_offset` 缺省为 0.0。
        let no_offset: Value = serde_json::from_str(
            r#"{"type":"minecraft:noise_based_count","noise_to_count_ratio":5,"noise_factor":100.0}"#,
        )
        .unwrap();
        assert!(Modifier::parse(&no_offset).unwrap().is_supported());
    }

    #[test]
    fn biome_info_noise_is_static_and_deterministic() {
        let a = crate::biome_temperature::biome_info_noise(12.5, -34.25);
        let b = crate::biome_temperature::biome_info_noise(12.5, -34.25);
        assert_eq!(a, b);
        assert_ne!(a, crate::biome_temperature::biome_info_noise(13.5, -34.25));
    }

    /// 造一个 y=2 为石头地面、其余空气的 3×3 世界，高度图固定为 2。
    fn scan_test_world() -> (crate::feature::FeatureChunks, Registry) {
        use crate::feature::{ChunkBlocks, FeatureChunks, Heightmaps};
        let min_y = 0;
        let height = 8;
        let mut blocks: [ChunkBlocks; 9] =
            std::array::from_fn(|_| vec![vec![blocks::AIR; 256]; height as usize]);
        for chunk in &mut blocks {
            chunk[2] = vec![blocks::STONE; 256];
        }
        let mut heights: [Heightmaps; 9] = std::array::from_fn(|_| Heightmaps::new(min_y));
        for h in &mut heights {
            h.world_surface = [2; 256];
            h.ocean_floor = [2; 256];
            h.motion_blocking = [2; 256];
            h.world_surface_wg = [2; 256];
            h.ocean_floor_wg = [2; 256];
            h.motion_blocking_no_leaves = [2; 256];
        }
        let chunks = FeatureChunks {
            center_x: 0,
            center_z: 0,
            min_y,
            height,
            blocks,
            biomes: std::array::from_fn(|_| Vec::new()),
            heights,
        };
        let root: Value = serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).unwrap();
        let registry = Registry::parse(root.as_object().unwrap()).unwrap();
        (chunks, registry)
    }

    #[test]
    fn surface_scan_and_layer_modifiers_produce_official_candidates() {
        use crate::random::Algorithm;
        let (chunks, registry) = scan_test_world();
        let world = FeatureWorld::new(&chunks).unwrap();
        let generation = WorldGenerationContext {
            min_y: 0,
            height: 8,
            sea_level: 63,
        };
        let random = WorldgenRandom::new(Algorithm::Xoroshiro.new_instance(1));
        let mut biome_at = |_x: i32, _y: i32, _z: i32| 0u32;
        let mut run = |modifier: &Modifier, pos: Pos| {
            let mut ctx = PlacementContext {
                world: &world,
                generation,
                biome_at: &mut biome_at,
                registry: &registry,
                top_feature: None,
            };
            let mut out = Vec::new();
            modify(modifier, &mut ctx, &random, pos, &mut out);
            out
        };

        // WORLD_SURFACE - OCEAN_FLOOR = 0：max=0 放行，max=-1 过滤掉。
        let water = Modifier::SurfaceWaterDepthFilter(0);
        assert_eq!(run(&water, (0, 4, 0)), vec![(0, 4, 0)]);
        let water_neg = Modifier::SurfaceWaterDepthFilter(-1);
        assert!(run(&water_neg, (0, 4, 0)).is_empty());

        // surfaceY = 3，max_inclusive=0：y=3 放行，y=4 过滤掉。
        let relative = Modifier::SurfaceRelativeThresholdFilter {
            heightmap: HeightmapKind::WorldSurface,
            min_inclusive: i64::from(i32::MIN),
            max_inclusive: 0,
        };
        assert_eq!(run(&relative, (0, 3, 0)), vec![(0, 3, 0)]);
        assert!(run(&relative, (0, 4, 0)).is_empty());

        // 从 y=6 向下扫到石头地面 y=2（solid）才命中。
        let scan = Modifier::EnvironmentScan {
            step: -1,
            target: BlockPredicate::Solid { offset: [0, 0, 0] },
            allowed: BlockPredicate::True,
            max_steps: 5,
        };
        assert_eq!(run(&scan, (0, 6, 0)), vec![(0, 2, 0)]);

        // 逐层扫描应至少在地面层放一个候选。
        let layers = Modifier::CountOnEveryLayer(IntProvider::Constant(1));
        assert!(!run(&layers, (0, 0, 0)).is_empty());
    }
}
