//! `ChunkStatus.INITIALIZE_LIGHT` / `LIGHT`：生成期光照接进流水线。
//!
//! 官方 `light` 阶段用一个 `WorldGenRegion`（中心 + 半径 1 的 8 个邻居，都跑完
//! `FEATURES`）给光引擎登记非空段、点亮光源、跑到底，中心区块柱里就带着天空光和
//! 方块光，`isLightOn=true`。`mc_bamboo_world::light::generate::generate_light`
//! 是这段的纯函数（复用运行时那套已和官方逐格对拍过的光引擎），这里负责把流水线
//! 的邻居只读快照拼成 3×3 视图，经 `generate_light_from_layers` 直接读快照里的
//! 逐层方块（不先组装完整区块），把结果交回世界线程。
//!
//! The stage passes the snapshots' block layers straight to
//! `generate_light_from_layers` without assembling full chunks first.
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
//! this module assembles the 3×3 view from the pipeline's immutable
//! neighbor snapshots and returns the light for the world thread.
//!
//! The neighbor snapshots are CARVERS terrain. The only ported features are ores
//! (opaque, non-emitting, replacing equally opaque stone), which change neither
//! light sources nor occlusion, so CARVERS light equals vanilla's post-FEATURES
//! light cell-by-cell. When light-affecting features merge, their writes must be
//! folded into the light input here (see README known limitations, report doubts).

use std::sync::Arc;

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::light::generate::{GeneratedLight, generate_light_from_layers};

use crate::{
    world_options::WorldGenConfig,
    worldgen::{GeneratedChunk, cached_generate_chunk},
    worldgen_features::SnapshotView,
    worldgen_profile::{LIGHT_CALLS, LIGHT_NS},
};

/// 和 `resolve_view` 一样拼 3×3，但快照只共享 `Arc`、不复制方块；缺快照时现算。
///
/// 下标 `(dz + 1) * 3 + (dx + 1)`，中心在 4。快照是不可变的，计算线程只读它们。
///
/// Assemble the 3×3 like `resolve_view`, but share snapshot `Arc`s instead of
/// copying their blocks; missing snapshots are generated. Index
/// `(dz + 1) * 3 + (dx + 1)`, center at 4. Snapshots are immutable and only read.
fn resolve_shared_view(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    center: Option<Arc<GeneratedChunk>>,
    neighbors: SnapshotView<'_>,
) -> Result<[Arc<GeneratedChunk>; 9], String> {
    let mut slots: [Option<Arc<GeneratedChunk>>; 9] = std::array::from_fn(|_| None);
    slots[4] = Some(match center {
        Some(arc) => arc,
        None => cached_generate_chunk(config, cx, cz)?,
    });
    for (pos, snapshot) in neighbors {
        let dx = pos.x - cx;
        let dz = pos.z - cz;
        if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dz) {
            continue;
        }
        let index = ((dz + 1) * 3 + (dx + 1)) as usize;
        slots[index] = Some(match snapshot {
            Some(arc) => Arc::clone(arc),
            None => cached_generate_chunk(config, pos.x, pos.z)?,
        });
    }
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
    center: Option<Arc<GeneratedChunk>>,
    neighbors: SnapshotView<'_>,
) -> Result<GeneratedLight, String> {
    let _profile_timer = crate::worldgen_profile::timer(&LIGHT_NS);
    crate::worldgen_profile::count(&LIGHT_CALLS);
    let view = resolve_shared_view(config, cx, cz, center, neighbors)?;
    let columns: Vec<(ChunkPos, &[Vec<u32>])> = view
        .iter()
        .enumerate()
        .map(|(index, generated)| {
            let dx = index as i32 % 3 - 1;
            let dz = index as i32 / 3 - 1;
            (ChunkPos::new(cx + dx, cz + dz), generated.blocks.as_slice())
        })
        .collect();
    generate_light_from_layers(ChunkPos::new(cx, cz), &columns)
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
