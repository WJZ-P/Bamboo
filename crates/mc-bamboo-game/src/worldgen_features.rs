//! 把 `ChunkStatus.FEATURES` 接进 Director 的 `Staged` 流水线。
//!
//! 官方这一步是 `ChunkGenerator.applyBiomeDecoration`：读中心和半径 1 的 8 个
//! 邻居（3×3）的 CARVERS 方块，写中心、也会写到邻居里。`mc_bamboo_worldgen::feature`
//! 的 [`Generator::features`] 是这段的纯函数；这里负责在游戏侧拼出 3×3 视图、
//! 调它、把结果（中心的新内容 + 给邻居的写增量）交回世界线程。
//!
//! 世界线程把每个区块的半成品存成不可变 [`Arc<GeneratedChunk>`]：发出任务后这些
//! `Arc` 不再被改，合并增量时通过 `Arc::make_mut` 写时复制出新的 `Arc`（设计稿
//! 5.3.1、`worldgen-features-plan.md` 3.2）。计算池只读这些快照、不碰世界状态。
//!
//! Wire `ChunkStatus.FEATURES` into the Director's `Staged` pipeline. Vanilla's
//! `ChunkGenerator.applyBiomeDecoration` reads a 3×3 of CARVERS chunks and
//! writes the center plus neighbor overflow. `mc_bamboo_worldgen::feature`'s
//! `Generator::features` is the pure function; this module assembles the 3×3
//! view, calls it, and returns the center's new content plus neighbor deltas.
//! The world thread keeps each chunk's half-built column as an immutable
//! `Arc<GeneratedChunk>` and merges deltas copy-on-write via `Arc::make_mut`.

use std::{cell::RefCell, collections::HashMap, sync::Arc};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT};
use mc_bamboo_worldgen::feature::{
    CENTER, DECORATION_STEPS, FeatureChunks, FeatureResult, Heightmaps, world::prime_heightmaps,
};

use crate::{
    region::RegionColumn,
    world_options::WorldGenConfig,
    worldgen::{GeneratedChunk, build_region_column, cached_generate_chunk, with_generator},
};

/// 一次 `setBlock` 写增量：世界坐标 `(x, y, z, 状态 ID)`。
///
/// One feature `setBlock` write in world coordinates `(x, y, z, state id)`.
pub(crate) type BlockDelta = (i32, i32, i32, u32);

/// 一个生成阶段的产物。
///
/// Output of one generation stage.
pub(crate) struct GenerateOutput {
    /// 中心推进到这一阶段后的盒子。
    pub(crate) column: Box<RegionColumn>,
    /// `terrain` 阶段的 CARVERS 纯数据，供世界线程的邻居快照缓存用。
    /// Clean CARVERS data from the `terrain` stage, for the world-thread
    /// neighbor snapshot cache.
    pub(crate) terrain: Option<GeneratedChunk>,
    /// 给邻居（和中心自身）的写增量，按目标区块坐标分组。
    /// Feature writes grouped by target chunk position.
    pub(crate) deltas: Vec<(ChunkPos, Vec<BlockDelta>)>,
}

impl GenerateOutput {
    /// 没有增量、没有快照的阶段（透传）。
    /// A pass-through stage: no snapshot, no deltas.
    fn passthrough(column: Box<RegionColumn>) -> Self {
        Self {
            column,
            terrain: None,
            deltas: Vec::new(),
        }
    }
}

/// 邻居只读视图：目标是哪些区块、各自不可变的 CARVERS 快照。
/// `None` 表示世界线程还没有这个位置的快照，计算线程用纯函数现算。
///
/// Read-only neighbor view: target positions and their immutable CARVERS
/// snapshots; `None` means the compute thread regenerates it purely.
pub(crate) type SnapshotView<'a> = &'a [(ChunkPos, Option<Arc<GeneratedChunk>>)];

/// 3×3 视图按 `(dz, dx)` 从 -1 到 1 排列，中心在 [`CENTER`]。
/// 用区块坐标算下标，和 `Task::Generate.neighbors` 的列出顺序无关。
///
/// Build the 9-chunk view in `(dz, dx)` order, center at `CENTER`, indexing by
/// chunk position so the list order does not matter.
fn resolve_view(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    center: Option<Arc<GeneratedChunk>>,
    neighbors: SnapshotView<'_>,
) -> Result<[GeneratedChunk; 9], String> {
    let mut slots: [Option<GeneratedChunk>; 9] = std::array::from_fn(|_| None);
    slots[CENTER] = Some(match center {
        Some(arc) => (*arc).clone(),
        None => cached_generate_chunk(config, cx, cz)?,
    });
    for (pos, snapshot) in neighbors {
        let dx = pos.x - cx;
        let dz = pos.z - cz;
        if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dz) {
            continue;
        }
        let index = ((dz + 1) * 3 + (dx + 1)) as usize;
        let chunk = match snapshot {
            Some(arc) => (**arc).clone(),
            None => cached_generate_chunk(config, pos.x, pos.z)?,
        };
        slots[index] = Some(chunk);
    }
    // 补上没给快照的格子（理论上 8 个邻居都该带上；补齐保证视图完整）。
    // Fill any missing neighbor slots (all 8 should be listed; be defensive).
    for dz in -1..=1 {
        for dx in -1..=1 {
            let index = ((dz + 1) * 3 + (dx + 1)) as usize;
            if slots[index].is_none() {
                slots[index] = Some(cached_generate_chunk(config, cx + dx, cz + dz)?);
            }
        }
    }
    Ok(slots.map(Option::unwrap))
}

/// `ChunkGenerator.applyBiomeDecoration`，纯函数：只依赖配置、坐标和 3×3 视图。
///
/// `ChunkGenerator.applyBiomeDecoration` as a pure function of config, position
/// and the 3×3 view.
pub(crate) fn run_features(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    view: [GeneratedChunk; 9],
) -> Result<FeatureResult, String> {
    if config.level_type.is_flat() {
        // flat 世界没有噪声地物；流水线也不是 Staged，这里只是防御。
        // Flat worlds have no noise features; the pipeline is not Staged either.
        return Ok(FeatureResult {
            blocks: std::array::from_fn(|_| {
                vec![vec![mc_bamboo_data::blocks::AIR; 256]; WORLD_HEIGHT as usize]
            }),
            deltas: std::array::from_fn(|_| Vec::new()),
            unsupported: false,
        });
    }
    let blocks: [Vec<Vec<u32>>; 9] = view.each_ref().map(|chunk| chunk.blocks.clone());
    let biomes: [Vec<Vec<u32>>; 9] = view.each_ref().map(|chunk| chunk.biomes.clone());
    let heights: [Heightmaps; 9] = blocks
        .each_ref()
        .map(|chunk| prime_heightmaps(chunk, MIN_Y, WORLD_HEIGHT));
    let input = FeatureChunks {
        center_x: cx,
        center_z: cz,
        min_y: MIN_Y,
        height: WORLD_HEIGHT,
        blocks,
        biomes,
        heights,
    };
    with_generator(config, |generator| {
        generator.features(&input, (1 << DECORATION_STEPS) - 1, None)
    })
}

/// `gen-world --through features` 对拍入口：只用配置和坐标，把 3×3 CARVERS
/// 视图拼好、调游戏里同一份 [`run_features`]，返回 9 宫格结果给工具逐方块比较。
///
/// `gen-world --through features` oracle entry: from config + position alone,
/// build the 3×3 CARVERS view and call the game's own [`run_features`], returning
/// the 9-chunk result for block-by-block comparison.
pub fn game_features(config: &WorldGenConfig, cx: i32, cz: i32) -> Result<FeatureResult, String> {
    let view = build_view(config, cx, cz)?;
    run_features(config, cx, cz, view)
}

/// 一次 `applyBiomeDecoration` 里需要复用的紧凑结果：给 3×3 九宫格各格的
/// 写增量（中心那格恒为空）。
///
/// Compact reusable result of one `applyBiomeDecoration`: the write deltas for
/// each of the 9 grid cells (the center cell is always empty).
struct FeatureRun {
    deltas: [Vec<BlockDelta>; 9],
}

type FeatureCache = HashMap<(i64, Option<&'static str>, i32, i32), Arc<FeatureRun>>;
thread_local! {
    // 每条计算线程各缓存一份「以某区块为中心」的地物结果。中心区块在生成
    // 自己的 `features` 时，要重算 8 个邻居中心的地物、取它们写进本区块的
    // 增量；缓存后每个中心只真正算一次。`applyBiomeDecoration` 是纯函数，
    // 命中与否不改结果。条目数封顶，超了整表清掉。
    // Each compute thread caches the feature result centered on a chunk. A
    // center needs the 8 neighbor centers' results to collect the writes they
    // spill into it; caching makes each center run once. Pure, so hits never
    // change the result. Bounded: clear the whole table past the cap.
    static FEATURE_CACHE: RefCell<FeatureCache> = RefCell::new(HashMap::new());
}
const FEATURE_CACHE_CAP: usize = 256;

/// 以 `(cx, cz)` 为中心造 3×3 的 CARVERS 视图（纯函数，带线程本地缓存）。
/// Build a 3×3 CARVERS view centered on `(cx, cz)` (pure, thread-local cached).
fn build_view(config: &WorldGenConfig, cx: i32, cz: i32) -> Result<[GeneratedChunk; 9], String> {
    let mut view: [Option<GeneratedChunk>; 9] = std::array::from_fn(|_| None);
    for dz in -1..=1 {
        for dx in -1..=1 {
            let index = ((dz + 1) * 3 + (dx + 1)) as usize;
            view[index] = Some(cached_generate_chunk(config, cx + dx, cz + dz)?);
        }
    }
    Ok(view.map(Option::unwrap))
}

/// 以某区块为中心跑一遍 `applyBiomeDecoration`，结果按中心缓存复用。
/// Run `applyBiomeDecoration` centered on a chunk, cached per center.
fn cached_feature_run(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
) -> Result<Arc<FeatureRun>, String> {
    let key = (config.seed, config.level_type.settings_id(), cx, cz);
    FEATURE_CACHE.with(|cache| {
        if let Some(run) = cache.borrow().get(&key) {
            return Ok(Arc::clone(run));
        }
        let view = build_view(config, cx, cz)?;
        let result = run_features(config, cx, cz, view)?;
        let run = Arc::new(FeatureRun {
            deltas: result.deltas.clone(),
        });
        let mut cache = cache.borrow_mut();
        if cache.len() >= FEATURE_CACHE_CAP {
            cache.clear();
        }
        cache.insert(key, Arc::clone(&run));
        Ok(run)
    })
}

/// 8 个邻居中心的地物写进本区块的增量合起来。
///
/// 官方里这些写来自邻居区块各自跑到 `FEATURES` 的 `WorldGenRegion`；Bamboo
/// 的调度不保证它们都在本区块组装前完成，所以在中心这次 `features` 里纯函数
/// 重算一遍（带缓存），保证本区块需要的增量集合**从自己的 features 结果就齐**，
/// 与邻居任务什么时候落地无关。世界线程仍按 `TaskId` 顺序把各中心的增量合并、
/// 完工排规范序，1/4 线程结果逐字节相同。
///
/// Writes that the 8 neighbor centers' features spill into this chunk. Vanilla
/// gets these from neighbors' `WorldGenRegion`s; Bamboo's scheduler does not
/// guarantee they all finish before this chunk is assembled, so recompute them
/// purely here (cached) so this chunk's delta set is complete from its OWN
/// features result, independent of neighbor timing. The world thread still
/// merges every center's deltas in `TaskId` order and sorts canonically, so
/// 1/4-thread results are byte-identical.
fn neighbor_spills_into(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
) -> Result<Vec<BlockDelta>, String> {
    let mut out = Vec::new();
    for dz in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dz == 0 {
                continue;
            }
            let run = cached_feature_run(config, cx + dx, cz + dz)?;
            // 本区块在「以邻居为中心」的 3×3 里的格子下标。
            // This chunk's grid index in the neighbor-centered 3×3 view.
            let index = ((-dz + 1) * 3 + (-dx + 1)) as usize;
            out.extend_from_slice(&run.deltas[index]);
        }
    }
    Ok(out)
}

/// `FeatureResult` 里给 8 个邻居的写增量，按目标区块坐标分组。
///
/// Group `FeatureResult`'s neighbor deltas by target chunk position.
fn neighbor_deltas(cx: i32, cz: i32, result: &FeatureResult) -> Vec<(ChunkPos, Vec<BlockDelta>)> {
    let mut out = Vec::new();
    for index in 0..9 {
        if index == CENTER || result.deltas[index].is_empty() {
            continue;
        }
        let dz = index as i32 / 3 - 1;
        let dx = index as i32 % 3 - 1;
        out.push((
            ChunkPos::new(cx + dx, cz + dz),
            result.deltas[index].clone(),
        ));
    }
    out
}

/// 中心「自己的地物」写增量：`blocks[CENTER]` 相对 CARVERS 地形变了的格子。
///
/// 官方 `applyBiomeDecoration` 把中心自己的地物写进中心 ProtoChunk；把它表示成
/// 增量，好和邻居溢出的增量一起按 `TaskId` 顺序合并（设计稿 5.3.1）。
///
/// The center's own feature writes: cells where `blocks[CENTER]` differs from
/// the clean CARVERS terrain. Expressed as deltas so they merge with neighbor
/// overflow in `TaskId` order (design 5.3.1).
fn center_writes(
    cx: i32,
    cz: i32,
    terrain: &GeneratedChunk,
    result: &FeatureResult,
) -> Vec<BlockDelta> {
    let after = &result.blocks[CENTER];
    let mut out = Vec::new();
    for (y, layer) in after.iter().enumerate() {
        let world_y = MIN_Y + y as i32;
        let before = &terrain.blocks[y];
        for column in 0..256 {
            if layer[column] != before[column] {
                let x = cx * 16 + (column / 16) as i32;
                let z = cz * 16 + (column % 16) as i32;
                out.push((x, world_y, z, layer[column]));
            }
        }
    }
    out
}

/// 跑一次 `features`：拼 3×3 视图、调纯函数、返回中心盒子 + 全部写增量。
///
/// Run one `features` stage: build the 3×3 view, call the pure function, return
/// the center's new column and all deltas (center's own + neighbor overflow).
pub(crate) fn generate_features_stage(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    center: Option<Arc<GeneratedChunk>>,
    neighbors: SnapshotView<'_>,
) -> Result<GenerateOutput, String> {
    let view = resolve_view(config, cx, cz, center.clone(), neighbors)?;
    let terrain = view[CENTER].clone();
    let result = run_features(config, cx, cz, view)?;

    // 给 8 个邻居的写增量（本中心的地物溢出），按目标坐标分组。
    // Writes this center's features spill into the 8 neighbors, by target pos.
    let mut deltas = neighbor_deltas(cx, cz, &result);

    // 本区块自己需要的全部写增量：自己的地物 + 8 个邻居中心溢过来的部分。
    // 后者纯函数重算（带缓存），保证本区块的增量集合从「自己的 features 结果」
    // 就完整，不依赖邻居任务何时落地 —— 这样世界线程按 `TaskId` 合并后，
    // 完工组装时中心和邻居写入都齐，1/4 线程逐字节相同。
    // All writes this chunk needs: its own features + the 8 neighbor centers'
    // spills. The latter are recomputed purely (cached) so this chunk's delta
    // set is complete from its OWN features result, independent of neighbor
    // timing; after the world thread merges in `TaskId` order, assembly has
    // everything and 1/4-thread results are byte-identical.
    let mut into_center = center_writes(cx, cz, &terrain, &result);
    into_center.extend(neighbor_spills_into(config, cx, cz)?);
    deltas.push((ChunkPos::new(cx, cz), into_center));

    // 中心盒子先回到 CARVERS 地形，增量由世界线程合并进 `Arc` 后再组装。
    // The center column returns to CARVERS terrain here; the world thread
    // merges deltas into the `Arc` snapshot before final assembly.
    let column = build_region_column(&terrain)?;
    Ok(GenerateOutput {
        column,
        terrain: None,
        deltas,
    })
}

/// `terrain` 阶段：算 CARVERS 方块，并把纯数据作为邻居快照交回世界线程。
///
/// `terrain` stage: compute CARVERS blocks and hand the clean data back for the
/// world-thread neighbor snapshot cache.
pub(crate) fn generate_terrain_stage(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
) -> Result<GenerateOutput, String> {
    let chunk = cached_generate_chunk(config, cx, cz)?;
    let column = build_region_column(&chunk)?;
    Ok(GenerateOutput {
        column,
        terrain: Some(chunk),
        deltas: Vec::new(),
    })
}

/// 透传阶段（结构、群系、光照、刷怪、完工）：盒子原样交回。
///
/// Pass-through stages: return the box unchanged.
pub(crate) fn passthrough_stage(center: Option<Box<RegionColumn>>) -> GenerateOutput {
    GenerateOutput::passthrough(center.unwrap_or_else(|| Box::new(RegionColumn::from_template())))
}

/// 把一组世界坐标的写增量落进 `GeneratedChunk` 的方块数组。
///
/// Apply world-coordinate deltas into a `GeneratedChunk`'s block arrays.
pub(crate) fn apply_deltas(chunk: &mut GeneratedChunk, deltas: &[BlockDelta]) {
    for &(x, y, z, state) in deltas {
        if !(MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            continue;
        }
        let column = (x & 15) as usize * 16 + (z & 15) as usize;
        chunk.blocks[(y - MIN_Y) as usize][column] = state;
    }
}

/// 完工时用「CARVERS 地形 + 一组写增量」组装交付盒子。
///
/// 写增量在交付前按 `(x, y, z, state)` 排成规范序再落盘：合进缓存的先后
/// （`TaskId` 批处理顺序、1 线程还是 4 线程）不影响最终方块。同格重复写时
/// 规范序里最后那条生效，结果逐字节确定。
///
/// Assemble the delivered box from CARVERS terrain + a set of deltas. Deltas
/// are sorted into canonical `(x, y, z, state)` order before being applied, so
/// the merge order (`TaskId` batching, 1 vs 4 threads) cannot change the final
/// blocks; a duplicate cell resolves to the last entry of that canonical order.
pub(crate) fn assemble_feature_column(
    terrain: &GeneratedChunk,
    deltas: &mut [BlockDelta],
) -> Result<Box<RegionColumn>, String> {
    deltas.sort_unstable();
    let mut chunk = terrain.clone();
    apply_deltas(&mut chunk, deltas);
    build_region_column(&chunk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_options::LevelType;

    fn normal(seed: i64) -> WorldGenConfig {
        WorldGenConfig {
            seed,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        }
    }

    /// 同一输入跑两遍 features，输出逐字节一致（随机源只从种子和坐标派生）。
    /// Two runs of `features` on the same input are byte-identical.
    #[test]
    fn features_are_deterministic() {
        let config = normal(2026);
        let view: [GeneratedChunk; 9] = std::array::from_fn(|i| {
            let dz = i as i32 / 3 - 1;
            let dx = i as i32 % 3 - 1;
            cached_generate_chunk(&config, dx, dz).unwrap()
        });
        let a = run_features(&config, 0, 0, view.clone()).unwrap();
        let b = run_features(&config, 0, 0, view).unwrap();
        assert_eq!(a.blocks, b.blocks);
        assert_eq!(a.deltas, b.deltas);
    }

    /// 写增量落进方块数组后能读回来。
    /// Deltas round-trip through the block arrays.
    #[test]
    fn apply_deltas_writes_the_expected_cells() {
        let config = normal(1);
        let mut chunk = cached_generate_chunk(&config, 0, 0).unwrap();
        apply_deltas(&mut chunk, &[(3, 40, 5, 12345), (0, MIN_Y, 0, 999)]);
        assert_eq!(chunk.blocks[(40 - MIN_Y) as usize][3 * 16 + 5], 12345);
        assert_eq!(chunk.blocks[0][0], 999);
    }

    /// `features` 阶段真的会放地物：中心自己的方块相对 CARVERS 地形有变化，
    /// 写增量非空（否则整条 features 接入就是空转）。
    /// The `features` stage really places features: the center's blocks differ
    /// from CARVERS terrain and the writes are non-empty (guard against a no-op).
    #[test]
    fn features_stage_places_blocks() {
        let config = normal(2026);
        let view = build_view(&config, 0, 0).unwrap();
        let terrain = view[CENTER].clone();
        let result = run_features(&config, 0, 0, view).unwrap();
        let writes = center_writes(0, 0, &terrain, &result);
        assert!(
            !writes.is_empty(),
            "ores should change some center blocks (unsupported={})",
            result.unsupported
        );
    }

    /// 增量合并按 `(x,y,z,state)` 规范序：追加顺序不同，最终方块也相同。
    /// Canonical `(x,y,z,state)` sort: different append orders give equal blocks.
    #[test]
    fn assemble_is_independent_of_delta_order() {
        let config = normal(7);
        let terrain = cached_generate_chunk(&config, 0, 0).unwrap();
        let mut a = vec![(3, 0, 5, 111), (4, 1, 6, 222), (3, 0, 5, 111)];
        let mut b = vec![(4, 1, 6, 222), (3, 0, 5, 111), (3, 0, 5, 111)];
        let ca = assemble_feature_column(&terrain, &mut a).unwrap();
        let cb = assemble_feature_column(&terrain, &mut b).unwrap();
        assert_eq!(ca.parts().column.untouched(), cb.parts().column.untouched());
        let wa = ca.parts().column.chunk();
        let wb = cb.parts().column.chunk();
        for y in MIN_Y..MIN_Y + WORLD_HEIGHT {
            for x in 0..16 {
                for z in 0..16 {
                    assert_eq!(wa.block(x, y, z), wb.block(x, y, z), "at ({x},{y},{z})");
                }
            }
        }
    }
}
