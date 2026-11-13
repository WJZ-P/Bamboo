//! `ChunkGenerator.applyBiomeDecoration` 的主循环。
//!
//! 逐段照字节码（`ChunkGenerator.java` 604–833）：
//! 收 3×3 的群系调色板并集 → 每个 step 收集全局 index → 排序 →
//! `setFeatureSeed(decorationSeed, index, step)` → `FeaturePlacer` 栈 DFS → 放置。

use std::collections::HashSet;

use crate::{
    biome_manager::BiomeManager,
    random::{Algorithm, WorldgenRandom},
    sampler::Context,
    settings::{Samplers, Settings},
};

use super::{
    DECORATION_STEPS, FeatureChunks, FeatureResult,
    placement::{Modifier, OreFeature, PlacementEnv, place_candidates},
    provider::WorldGenerationContext,
    registry::Registry,
    world::FeatureWorld,
};

/// 一次 `features` 调用需要的、按种子建一次的东西。
pub struct FeatureContext<'a> {
    pub registry: &'a Registry,
    pub samplers: &'a Samplers,
    pub biomes: &'a BiomeManager<'a>,
    pub settings: &'a Settings,
    pub seed: i64,
    pub cache_count: usize,
    /// 只跑这些 step；`(1<<11)-1` 表示全部。过渡期诊断用。
    pub steps_mask: i32,
    /// 只跑这些配置类型；`None` 表示全部。过渡期诊断用。
    pub allowed_kinds: Option<&'static [&'static str]>,
}

/// `ChunkGenerator.applyBiomeDecoration`，纯函数。
pub fn features(input: &FeatureChunks, ctx: &FeatureContext<'_>) -> Result<FeatureResult, String> {
    let mut world = FeatureWorld::new(input)?;
    let mut unsupported = false;

    // 字节码 132-177：ChunkPos.rangeClosed(center,1) 九个区块，把每个 section 的
    // quart 群系调色板收进 ObjectArraySet（插入序），再 retainAll(possibleBiomes)。
    // 之后 Arrays.sort(indices) 会排序，所以并集内容正确即可，顺序不影响结果。
    let mut biome_set: Vec<u32> = Vec::new();
    let mut seen = HashSet::new();
    for sections in input.biomes.iter() {
        for cells in sections {
            for &id in cells {
                if seen.insert(id) {
                    biome_set.push(id);
                }
            }
        }
    }

    // 90-130：WorldgenRandom(Xoroshiro) + setDecorationSeed(level.getSeed(),
    // origin.x, origin.z)。origin = (centerX*16, minSectionY*16, centerZ*16)。
    let random = WorldgenRandom::new(Algorithm::Xoroshiro.new_instance(0));
    let decoration_seed =
        random.set_decoration_seed(ctx.seed, input.center_x * 16, input.center_z * 16);

    let generation = WorldGenerationContext {
        min_y: ctx.settings.min_y,
        height: ctx.settings.height,
        sea_level: ctx.settings.sea_level,
    };
    let origin = (input.center_x * 16, ctx.settings.min_y, input.center_z * 16);
    let mut density_context = Context::new(ctx.cache_count);

    for step in 0..DECORATION_STEPS {
        if ctx.steps_mask & (1 << step) == 0 {
            continue;
        }
        let globals = &ctx.registry.steps()[step];
        if globals.is_empty() {
            continue;
        }
        // 418-561：IntArraySet indices，遍历并集里的每个群系的 features[step]，
        // 把每个 placed feature 换成 featuresPerStep[step] 的全局 index。
        let mut indices: Vec<u32> = Vec::new();
        for &biome in &biome_set {
            for name in ctx.registry.biome_features(biome, step) {
                if let Some(id) = ctx.registry.placed_id(name) {
                    // featuresPerStep[step] 是按全局 index 排好的列表，下标即 index。
                    if let Some(index) = globals.iter().position(|&g| g == id) {
                        indices.push(index as u32);
                    }
                }
            }
        }
        indices.sort_unstable();
        indices.dedup();

        for &index in &indices {
            // 643-651：setFeatureSeed(decorationSeed, index, step)。
            random.set_feature_seed(decoration_seed, index as i32, step as i32);
            let placed_id = globals[index as usize];
            let Some(placed) = ctx.registry.placed(placed_id) else {
                unsupported = true;
                continue;
            };

            // 解析 feature 配置；未移植的类型标 unsupported 并跳过（不消费随机数）。
            let Some((kind, config)) = resolve_feature(ctx.registry, &placed.feature) else {
                unsupported = true;
                continue;
            };
            if ctx
                .allowed_kinds
                .is_some_and(|kinds| !kinds.contains(&kind.as_str()))
            {
                continue;
            }
            if kind != "minecraft:ore" {
                unsupported = true;
                continue;
            }
            let ore = OreFeature::parse(config)?;
            let modifiers: Vec<Modifier> = placed
                .placement
                .iter()
                .map(Modifier::parse)
                .collect::<Result<Vec<_>, _>>()?;
            if modifiers.iter().any(|m| !m.is_supported()) {
                unsupported = true;
                continue;
            }

            // BiomeFilter 需要 level.getBiome(pos)，即 BiomeManager.getBiome 的
            // 八角 fiddled distance；这里用一个闭包把采样器和上下文带进去。
            let mut biome_at = |x: i32, y: i32, z: i32| -> u32 {
                ctx.biomes
                    .biome(ctx.samplers, &mut density_context, x, y, z)
            };
            let mut env = PlacementEnv {
                generation,
                biome_at: &mut biome_at,
                registry: ctx.registry,
                top_feature: Some(placed_id),
            };
            // 官方在栈 DFS 过程中交错执行 modifier 和 Feature.place；不能先收集
            // 全部候选位置，否则 Feature.place 消费的随机数会打乱后续 modifier。
            let mut place_one = |world: &mut FeatureWorld<'_>, pos: (i32, i32, i32)| {
                super::ores::place(&ore, world, &random, pos);
            };
            place_candidates(
                &modifiers,
                &mut world,
                &mut env,
                &random,
                origin,
                &mut place_one,
            );
        }
    }

    Ok(world.finish(unsupported))
}

/// 解析 `PlacedFeature.feature`：注册表引用（字符串）或内联配置（对象）。
/// 返回 `(类型 ID, 配置对象)`。
fn resolve_feature<'a>(
    registry: &'a Registry,
    value: &'a serde_json::Value,
) -> Option<(String, &'a serde_json::Value)> {
    match value {
        serde_json::Value::String(name) => {
            let id = registry.feature_id(name)?;
            let def = registry.feature(id)?;
            Some((def.kind.clone(), &def.config))
        }
        serde_json::Value::Object(object) => {
            let kind = object.get("type")?.as_str()?.to_owned();
            Some((kind, value))
        }
        _ => None,
    }
}

/// 过渡期诊断：step 6 里所有配置类型都是 `minecraft:ore` 时返回 true。
pub fn step_is_ore_only(registry: &Registry, step: usize) -> bool {
    registry.steps()[step].iter().all(|&id| {
        registry
            .placed(id)
            .and_then(|p| resolve_feature(registry, &p.feature))
            .is_some_and(|(kind, _)| kind == "minecraft:ore")
    })
}
