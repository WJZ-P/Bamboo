//! 生成期光照：`ChunkStatusTasks.initializeLight` + `ChunkStatusTasks.light`。
//!
//! 官方生成区块时，`LIGHT` 这一步把中心区块和半径 1 的 8 个邻居（都跑完了
//! `FEATURES`）的方块交给 `LevelLightEngine`：先给每个有方块的段登记段状态
//! （`initializeLight`），再逐个点亮光源（`light` / `propagateLightSources`），
//! 最后把光照队列跑到底，中心区块柱里的天空光和方块光就是结果。
//!
//! 这里直接复用 [`super::LevelLight`]（也就是运行时光照引擎）下面那套已经和官方
//! 逐格对拍过的 `Engines` / `LightStorage` / `SkyLightEngine` / `BlockLightEngine`，
//! 用同一段序列在一小片真实地形上从零算一遍，取出中心柱的 26×2 段。视图之外的
//! 位置按全空气读（天空光 15、方块光 0），和生成区边界一致；光最多走 15 格，
//! 所以中心柱的结果只取决于它和半径 1 的邻居，外面补什么都不会传进来。
//!
//! Generation-time light: `ChunkStatusTasks.initializeLight` + `light`. Vanilla
//! hands the center chunk and its radius-1 neighbors (all past `FEATURES`) to the
//! `LevelLightEngine`, registers non-empty sections, propagates light sources,
//! and runs the light queue to completion. This reuses the very same engine that
//! the runtime (and the `light` oracle) validates against vanilla, driving the
//! same sequence over a small patch of real terrain and reading the center
//! column's 26×2 light sections. Positions outside the view read as air (sky 15,
//! block 0), matching the generation boundary; light travels at most 15 blocks,
//! so only the center and its radius-1 neighbors can affect its result.

use std::collections::BTreeMap;

use mc_bamboo_core::{ChunkPos, LightLayer};

use crate::{chunk::Chunk, store::ChunkMap};

use super::{Chunks, DataLayer, Engines, LightColumn, LightStorage};

/// 生成期算出的一柱光照：天空光、方块光各 [`super::LIGHT_SECTION_COUNT`] 段，
/// `None` 表示官方不存这一段（`DataLayer::Filled(0)` 和它是两回事）。
///
/// Generation-time light for one column: sky and block layers, each
/// `LIGHT_SECTION_COUNT` sections; `None` means vanilla stores no data layer
/// for that section (distinct from `DataLayer::Filled(0)`).
#[derive(Debug, Clone)]
pub struct GeneratedLight {
    pub sky: Vec<Option<DataLayer>>,
    pub block: Vec<Option<DataLayer>>,
}

/// 从中心和其邻居的最终方块，算出中心柱生成期的光照。
///
/// `chunks` 至少要有中心加半径 1 的 8 个邻居（3×3）。每个 [`Chunk`] 必须已经
/// 算好天空光源表（`Chunk::initialize_light_sources`，`ChunkColumn::from_parts`
/// 会自动算）。纯函数：同样的输入总是同样的输出，不碰世界状态。
///
/// Compute the center column's generation-time light from the final blocks of
/// the center and its neighbors. `chunks` needs at least the 3×3 around
/// `center`; every [`Chunk`] must have its sky-light sources computed
/// (`ChunkColumn::from_parts` does this). Pure: no world state, deterministic.
pub fn generate_light(center: ChunkPos, chunks: &BTreeMap<ChunkPos, Chunk>) -> GeneratedLight {
    light_of(center, chunks.clone())
}

/// 和 [`generate_light`] 相同，但直接读世界生成的逐层方块，不先组装完整的 [`Chunk`]。
///
/// 每一项是一个区块的位置和它自下而上 [`crate::WORLD_HEIGHT`] 层方块（每层 256 格、
/// 下标 `x * 16 + z`，即 `GeneratedChunk.blocks` 的布局）。光引擎只读段方块、段计数和
/// 天空光源表，这里只建这些（[`Chunk::for_light`]），省掉调色板往返、四张高度图和
/// 方块实体扫描；同一份方块的结果和 [`generate_light`] 逐位相同。
///
/// Same as [`generate_light`], but reads worldgen block layers directly instead
/// of fully assembled [`Chunk`]s. Each entry is a chunk position and its
/// [`crate::WORLD_HEIGHT`] layers bottom-up (256 cells each, indexed
/// `x * 16 + z`, the `GeneratedChunk.blocks` layout). The light engine only
/// reads section blocks, section counts and sky-light sources, so only those are
/// built ([`Chunk::for_light`]), skipping the palette round trip, the four
/// heightmaps and the block-entity scan; for the same blocks the result is
/// bit-identical to [`generate_light`].
pub fn generate_light_from_layers(
    center: ChunkPos,
    columns: &[(ChunkPos, &[Vec<u32>])],
) -> Result<GeneratedLight, String> {
    let mut chunks = BTreeMap::new();
    for &(pos, layers) in columns {
        chunks.insert(pos, Chunk::for_light(layers)?);
    }
    Ok(light_of(center, chunks))
}

/// 两个入口共用的计算；`chunks` 按位置顺序装进 `ChunkMap`，遍历顺序因此和入口无关。
///
/// Shared core of both entry points; `chunks` enters the `ChunkMap` in position
/// order, so the iteration order does not depend on the entry point.
fn light_of(center: ChunkPos, chunks: BTreeMap<ChunkPos, Chunk>) -> GeneratedLight {
    // 视图之外读成全空气：`Chunk::empty` 的天空光源表默认就是「整列都通到天」。
    // Positions outside the view read as all-air; `Chunk::empty` already has a
    // default sky-source table meaning "open sky for the whole column".
    let air = Chunk::empty(0);
    let mut edited: ChunkMap<Chunk> = ChunkMap::default();
    for (pos, chunk) in chunks {
        edited.insert(pos, chunk);
    }
    let view = Chunks::new(&air, &edited);

    let empty = |layer| LightStorage::new(layer, LightColumn::empty(), i32::MAX);
    let mut engines = Engines::new(empty(LightLayer::Sky), empty(LightLayer::Block));

    // `ChunkStatusTasks.initializeLight`：登记每个区块里非空段的状态。
    // Register every non-empty section's state, like `initializeLight`.
    for (pos, chunk) in &edited {
        engines.initialize_chunk(chunk, *pos);
    }
    engines.run_light_updates(view);

    // `ChunkStatusTasks.light`：逐个点亮光源（先方块光后天空光），再跑到底。
    // `light`: propagate sources (block then sky), then drain the queue.
    let positions: Vec<ChunkPos> = edited.keys().copied().collect();
    for pos in positions {
        engines.block.propagate_light_sources(view, pos);
        engines.sky.propagate_light_sources(view, pos);
    }
    engines.run_light_updates(view);

    let sky = engines.sky.storage.column_at(center).lit().into_layers();
    let block = engines.block.storage.column_at(center).lit().into_layers();
    GeneratedLight { sky, block }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::BlockPos;

    use super::*;
    use crate::{MIN_Y, flat, store::ChunkStore};

    /// 把生成期光照装进一个真正的 `ChunkColumn`，读 `light_value` 时会走官方
    /// 「`None` 段向上找」的天空光回退，和游戏交付的盒子完全一致。
    /// Install generated light into a real `ChunkColumn` so `light_value` applies
    /// the same `None`-section sky fallback as the delivered box.
    fn column_of_light(generated: &GeneratedLight) -> crate::column::ChunkColumn {
        let mut store = ChunkStore::flat();
        let base = store
            .take_column(ChunkPos::new(0, 0))
            .expect("flat light settled");
        let mut parts = crate::column::ChunkColumnParts::read_from(&base);
        parts.light = [generated.sky.clone(), generated.block.clone()];
        parts.light_on = true;
        crate::column::ChunkColumn::from_parts(parts).expect("valid column parts")
    }

    /// 全超平坦地形：生成期从零算的中心光，必须和运行时模板引擎
    /// （`ChunkStore::flat`，`light` oracle 已和官方逐格对拍过）逐格相同。
    /// On flat terrain, generation-from-scratch light must match the runtime
    /// template engine (already validated against vanilla) cell by cell.
    #[test]
    fn flat_terrain_matches_the_template_engine() {
        let mut chunks = BTreeMap::new();
        for x in -1..=1 {
            for z in -1..=1 {
                let mut chunk = flat::generate_chunk();
                chunk.initialize_light_sources();
                chunks.insert(ChunkPos::new(x, z), chunk);
            }
        }
        let generated = generate_light(ChunkPos::new(0, 0), &chunks);
        let ours = column_of_light(&generated);

        let store = ChunkStore::flat();
        for layer in [LightLayer::Sky, LightLayer::Block] {
            for world_y in MIN_Y..MIN_Y + 384 {
                for z in 0..16 {
                    for x in 0..16 {
                        let pos = BlockPos::new(x, world_y, z);
                        assert_eq!(
                            store.light_value(layer, pos),
                            ours.light_value(layer, pos),
                            "{layer:?} at ({x},{world_y},{z})"
                        );
                    }
                }
            }
        }
    }

    /// 结果是纯函数：同一输入连跑两遍逐段相同。
    /// Pure: two runs on the same input produce identical layers.
    #[test]
    fn generation_light_is_deterministic() {
        let mut chunks = BTreeMap::new();
        for x in -1..=1 {
            for z in -1..=1 {
                chunks.insert(ChunkPos::new(x, z), flat::generate_chunk());
            }
        }
        let a = generate_light(ChunkPos::new(0, 0), &chunks);
        let b = generate_light(ChunkPos::new(0, 0), &chunks);
        assert_eq!(a.sky, b.sky);
        assert_eq!(a.block, b.block);
    }
}
