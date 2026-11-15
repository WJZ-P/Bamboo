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
    DECORATION_STEPS, FeatureChunks, FeatureResult, composition, placement::PlacementEnv,
    provider::WorldGenerationContext, registry::Registry, world::FeatureWorld,
};

/// 一次 `features` 调用需要的、按种子建一次的东西。
pub struct FeatureContext<'a> {
    pub registry: &'a Registry,
    pub samplers: &'a Samplers,
    pub biomes: &'a BiomeManager<'a>,
    pub settings: &'a Settings,
    pub seed: i64,
    /// 只跑这些 step；`(1<<11)-1` 表示全部。过渡期诊断用。
    pub steps_mask: i32,
    /// 只跑这些条目；`None` 表示全部。每个条目是 Feature 类型 ID 或 placed feature
    /// 注册名，和 Java 侧 `--feature-types` / `--types` 的白名单判定一致（精确匹配其一）。
    /// 过渡期诊断用。
    ///
    /// Only run these entries; `None` means all. Each entry is a Feature type ID or
    /// a placed-feature registry name, matched the same way as Java's
    /// `--feature-types` / `--types` allow list (exact match on either). Diagnostic only.
    pub allowed_kinds: Option<&'a [String]>,
    /// `WorldGenRegion.getRandom()`：`MossyCarpetBlock.placeAt` 一类调用读的是它，
    /// 不是 `(index, step)` 播种的地物 `random`。整个 `features` 调用共用一份。
    ///
    /// `WorldGenRegion.getRandom()`, read by calls such as `MossyCarpetBlock.placeAt`
    /// instead of the per-feature `random`. Shared by the whole `features` call.
    pub region_random: &'a WorldgenRandom,
}

/// `ChunkGenerator.applyBiomeDecoration`，纯函数。
pub fn features(input: &FeatureChunks, ctx: &FeatureContext<'_>) -> Result<FeatureResult, String> {
    let mut world = FeatureWorld::new(input, ctx.settings.sea_level, ctx.seed)?;
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
    // BiomeManager 的 resolver 是 createUncachedResolver(state)：官方用
    // SamplerContext.EMPTY_UNCACHED，单点求值不记忆缓存。这里不能用带缓存的
    // Context，否则同一坐标在不同调用顺序下会读到上一次成批求值的缓冲。
    let mut density_context = Context::uncached();

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
                let Some(id) = ctx.registry.placed_id(name) else {
                    continue;
                };
                // 过渡期 `--feature-types` / `--types` / `--ore-only`：Java 侧把不在
                // 白名单里的 HolderSet 换空了，这里要用同一套判定收集 indices，否则
                // 两边跑的 (index, step) 集合不同。白名单条目既可以是 placed feature
                // 注册名，也可以是 Feature 类型 ID；两处都精确匹配其一，和 Java 对称。
                // Java empties the non-whitelisted HolderSets; collect indices with
                // the same rule so both sides run the same (index, step) set. A
                // selector is a placed-feature name OR a Feature type ID, matched
                // exactly, symmetric with Java's allow list.
                if let Some(selectors) = ctx.allowed_kinds {
                    let by_name = selectors.iter().any(|s| s == name);
                    if !by_name {
                        let Some(placed) = ctx.registry.placed(id) else {
                            continue;
                        };
                        let Some((kind, _)) = resolve_feature(ctx.registry, &placed.feature) else {
                            continue;
                        };
                        if !selectors.iter().any(|s| s == &kind) {
                            continue;
                        }
                    }
                }
                // featuresPerStep[step] 是按全局 index 排好的列表，下标即 index。
                if let Some(index) = globals.iter().position(|&g| g == id) {
                    indices.push(index as u32);
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
                region_random: ctx.region_random,
            };
            // 组合类会在 Feature.place 里递归再走一轮修饰器链；整棵树是否都移植
            // 由 place_placed 汇总，任一未移植分支都让本区块标 unsupported。
            // Composition features recurse into more placement chains inside Feature.place;
            // place_placed reports whether the whole tree is ported.
            let outcome = composition::place_placed(
                ctx.registry,
                &placed.feature,
                &placed.placement,
                &mut world,
                &mut env,
                &random,
                origin,
            );
            if !outcome.supported {
                unsupported = true;
            }
        }
    }

    Ok(world.finish(unsupported))
}

/// 解析 `PlacedFeature.feature`：注册表引用（字符串）或内联配置（对象）。
/// 返回 `(类型 ID, 配置对象)`。
pub(crate) fn resolve_feature<'a>(
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
