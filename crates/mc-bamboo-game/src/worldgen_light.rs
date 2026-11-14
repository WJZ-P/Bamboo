//! `ChunkStatus.INITIALIZE_LIGHT` / `LIGHT`：生成期光照接进流水线。
//!
//! 官方 `light` 阶段用一个 `WorldGenRegion`（中心 + 半径 1 的 8 个邻居，都跑完
//! `FEATURES`）给光引擎登记非空段、点亮光源、跑到底，中心区块柱里就带着天空光和
//! 方块光，`isLightOn=true`。`mc_bamboo_world::light::generate::generate_light`
//! 是这段的纯函数（复用运行时那套已和官方逐格对拍过的光引擎），这里负责把流水线
//! 的邻居只读快照拼成 3×3 的 [`Chunk`] 视图喂给它，把结果交回世界线程。
//!
//! 邻居视图里的快照是 CARVERS 地形。当前已移植的地物只有矿石（不透明、不发光，
//! 替换的是同样不透明的石头），不改光源和遮挡，所以在 `LIGHT` 阶段用地形方块算光
//! 与官方「FEATURES 之后」的结果逐格相同；等会发光 / 会挡天空光的地物合进来后，
//! 需要把它们的写增量一并折进这里的光照输入（见 README 已知限制、汇报疑点清单）。
//!
//! Wire `ChunkStatus.LIGHT` into the pipeline. Vanilla's `light` step hands a
//! `WorldGenRegion` (center + radius-1 neighbors, all past `FEATURES`) to the
//! light engine: register non-empty sections, propagate sources, and drain the
//! queue; the center column then carries sky + block light and `isLightOn=true`.
//! `mc_bamboo_world::light::generate::generate_light` is the pure function
//! (reusing the runtime engine, already validated cell-by-cell against vanilla);
//! this module assembles the 3×3 [`Chunk`] view from the pipeline's immutable
//! neighbor snapshots and returns the light for the world thread.
//!
//! The neighbor snapshots are CARVERS terrain. The only ported features are ores
//! (opaque, non-emitting, replacing equally opaque stone), which change neither
//! light sources nor occlusion, so CARVERS light equals vanilla's post-FEATURES
//! light cell-by-cell. When light-affecting features merge, their writes must be
//! folded into the light input here (see README known limitations, report doubts).

use std::collections::BTreeMap;

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::{chunk::Chunk, light::generate::GeneratedLight};

use crate::{
    world_options::WorldGenConfig,
    worldgen::{GeneratedChunk, build_region_column},
    worldgen_features::{SnapshotView, resolve_view},
};

/// 把一份 CARVERS [`GeneratedChunk`] 转成光引擎能读的 [`Chunk`]。
///
/// `build_region_column` 走官方 `PalettedContainer` 下标组装段，里面
/// `Chunk::from_sections` 会算好天空光源表。纯函数，只依赖传入的方块。
///
/// Convert a CARVERS [`GeneratedChunk`] into a [`Chunk`] the light engine can
/// read. `build_region_column` assembles sections in vanilla `PalettedContainer`
/// order; its `Chunk::from_sections` computes the sky-light source table. Pure.
fn to_chunk(chunk: &GeneratedChunk) -> Result<Chunk, String> {
    let region = build_region_column(chunk)?;
    Ok(region.parts().column.chunk().clone())
}

/// `ChunkStatus.LIGHT`：从中心 + 8 个邻居的 CARVERS 视图算中心柱的生成期光照。
///
/// 3×3 视图按 `(dz, dx)` 排列，中心在下标 4；光最多走 15 格，小于一个区块宽 16，
/// 所以这一圈足够把中心柱照全，视图之外读成全空气（天空光 15、方块光 0）。
///
/// `ChunkStatus.LIGHT`: compute the center column's light from the center and 8
/// neighbors' CARVERS view. Light travels at most 15 blocks < one chunk's 16, so
/// the 3×3 view suffices; outside reads as air (sky 15, block 0). Pure and
/// deterministic: same config + position always gives the same light.
pub(crate) fn generate_light_stage(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    center: Option<std::sync::Arc<GeneratedChunk>>,
    neighbors: SnapshotView<'_>,
) -> Result<GeneratedLight, String> {
    let view = resolve_view(config, cx, cz, center, neighbors)?;
    let mut chunks: BTreeMap<ChunkPos, Chunk> = BTreeMap::new();
    for (index, generated) in view.iter().enumerate() {
        let dx = index as i32 % 3 - 1;
        let dz = index as i32 / 3 - 1;
        chunks.insert(ChunkPos::new(cx + dx, cz + dz), to_chunk(generated)?);
    }
    Ok(mc_bamboo_world::light::generate::generate_light(
        ChunkPos::new(cx, cz),
        &chunks,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_options::LevelType;

    /// 同一输入连跑两遍光照逐段相同（光引擎纯函数，随机源不参与）。
    /// Two runs on the same input give identical light layers (pure engine).
    #[test]
    fn light_stage_is_deterministic() {
        let config = WorldGenConfig {
            seed: 99,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        let a = generate_light_stage(&config, 3, -2, None, &[]).unwrap();
        let b = generate_light_stage(&config, 3, -2, None, &[]).unwrap();
        assert_eq!(a.sky, b.sky);
        assert_eq!(a.block, b.block);
    }

    /// 生成期光照的天空光和方块光都有数据层（不是全 `None`）。
    /// Generated light has real sky and block data layers, not all `None`.
    #[test]
    fn light_stage_produces_both_layers() {
        let config = WorldGenConfig {
            seed: 7,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        let light = generate_light_stage(&config, 0, 0, None, &[]).unwrap();
        assert_eq!(light.sky.len(), mc_bamboo_world::light::LIGHT_SECTION_COUNT);
        assert_eq!(
            light.block.len(),
            mc_bamboo_world::light::LIGHT_SECTION_COUNT
        );
        assert!(light.sky.iter().any(Option::is_some));
        assert!(light.block.iter().any(Option::is_some));
    }
}
