//! `ChunkStatus.INITIALIZE_LIGHT` / `LIGHT`：生成期光照接进流水线。
//!
//! 官方 `GENERATION_PYRAMID` 里 `LIGHT` 的直接依赖是半径 1 的
//! `INITIALIZE_LIGHT`，而每格的 `INITIALIZE_LIGHT` 又排在 `FEATURES` 之后：
//! 所以中心与 8 个邻居在算光时都已跑完自己的 `FEATURES`。光引擎通过
//! `LightChunkGetter` 读到的就是这 3×3 里当时已有的方块，包括邻居中心的地物
//! 溢出写进来的格子（`ChunkPyramid.<clinit>`、`ChunkStatusTasks.initializeLight`
//! / `light`、`LevelLightEngine.propagateLightSources`）。
//!
//! 这里照做：把流水线存的不可变 CARVERS 快照拼成 3×3，再用与完工组装同一份
//! `feature_writes_into` + `merge_feature_writes` 规则，把 9 个来源中心的地物写
//! 折进每个格子，最后经 `generate_light_from_layers` 直接读逐层方块（不先组装
//! 完整区块），把中心柱的天空光 + 方块光交回世界线程。树冠、萤石、灯笼、结构
//! 光源因此都参与生成期光照；运行时不会补算，`isLightOn=true` 交付的区块会一直
//! 带着这次算出的光。
//!
//! `mc_bamboo_world::light::generate::generate_light` 复用运行时那套已和官方逐格
//! 对拍过的光引擎；`gen-light` 只证明这批方块喂进引擎后结果一致，整块光照对拍
//! （`light-pipeline`）另证流水线挑的方块和官方一致。
//!
//! Wire `ChunkStatus.LIGHT` into the pipeline. In vanilla's `GENERATION_PYRAMID`,
//! `LIGHT` requires radius-1 `INITIALIZE_LIGHT` and each chunk reaches
//! `INITIALIZE_LIGHT` after `FEATURES`, so the center and all 8 neighbors have
//! completed their own features when light runs. The engine reads those very
//! blocks through `LightChunkGetter`, including the cells a neighbor's features
//! spilled into. Here the immutable CARVERS snapshots are folded with the same
//! `feature_writes_into` + `merge_feature_writes` rule used at final assembly,
//! so canopies, glowstone, lanterns and structure light sources all affect the
//! baked light. `generate_light_from_layers` then reads the layers directly
//! without assembling full chunks. `gen-light` validates the engine on a given
//! set of blocks; `light-pipeline` validates that the pipeline picked the same
//! blocks as vanilla.

use std::sync::Arc;

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::light::generate::{GeneratedLight, generate_light_from_layers};

use crate::{
    world_options::WorldGenConfig,
    worldgen::GeneratedChunk,
    worldgen_features::{
        SnapshotView, cached_generate_chunk_with_structures as terrain_cell, feature_writes_into,
        merge_feature_writes,
    },
    worldgen_profile::{LIGHT_CALLS, LIGHT_NS},
};

/// 整块光照对拍用的输入开关：默认（和游戏里一样）把 FEATURES 的写折进 3×3；
/// `Carvers` 故意退回旧行为，证明比较器会抓住树冠 / 光源附近的差异。
/// 中文注释在上、英文在下；这里是公开项，给 `mc-bamboo-vanilla` 的改坏确认用。
///
/// Input switch for the whole-chunk light oracle. The default (and the game) folds
/// FEATURES writes into the 3x3; `Carvers` deliberately reverts to the old input
/// so the comparator must report canopy / emitter differences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LightInput {
    /// FEATURES 后视图，和官方 LIGHT 阶段一致。
    /// Post-FEATURES view, matching vanilla's LIGHT step.
    #[default]
    PostFeatures,
    /// 旧的 CARVERS 输入，只给改坏确认用。
    /// The old CARVERS input, only for break confirmation.
    Carvers,
}

/// 和 `resolve_view` 一样拼 3×3，但快照只共享 `Arc`、不复制方块；缺快照时现算。
/// 现算走和 `terrain` 阶段同一个入口（带本格 Beardifier），有没有快照不改变结果。
///
/// 下标 `(dz + 1) * 3 + (dx + 1)`，中心在 4。快照是不可变的，计算线程只读它们。
///
/// Assemble the 3×3 like `resolve_view`, but share snapshot `Arc`s instead of
/// copying their blocks; missing snapshots are generated through the `terrain`
/// stage's entry point (with the cell's Beardifier), so snapshot presence does
/// not change the result. Index `(dz + 1) * 3 + (dx + 1)`, center at 4.
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
        None => terrain_cell(config, cx, cz)?,
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
            None => terrain_cell(config, pos.x, pos.z)?,
        });
    }
    for dz in -1..=1 {
        for dx in -1..=1 {
            let index = ((dz + 1) * 3 + (dx + 1)) as usize;
            if slots[index].is_none() {
                slots[index] = Some(terrain_cell(config, cx + dx, cz + dz)?);
            }
        }
    }
    Ok(slots.map(Option::unwrap))
}

/// 3×3 每个格子都变成「CARVERS + 9 个来源中心的 FEATURES 写」。
/// 官方 `LIGHT` 阶段读的是各区块跑完自己 `FEATURES` 后的 ProtoChunk，包括邻居
/// 中心溢出进来的写；这里用和 Director 完工组装完全相同的合并规则还原那批方块。
///
/// Turn each 3x3 cell into CARVERS + the 9 source centers' FEATURES writes.
/// Vanilla's `LIGHT` step reads each chunk's post-FEATURES ProtoChunk, including
/// neighbor spill; the same merge rule as the Director's final assembly rebuilds
/// those blocks.
fn post_features_view(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    terrain: &[Arc<GeneratedChunk>; 9],
) -> Result<Vec<Vec<Vec<u32>>>, String> {
    let mut blocks = Vec::with_capacity(9);
    for (index, cell) in terrain.iter().enumerate() {
        let dx = index as i32 % 3 - 1;
        let dz = index as i32 / 3 - 1;
        let x = cx + dx;
        let z = cz + dz;
        let mut chunk = (**cell).clone();
        merge_feature_writes(&mut chunk, feature_writes_into(config, x, z)?);
        blocks.push(chunk.blocks);
    }
    Ok(blocks)
}

/// `ChunkStatus.LIGHT`：从中心 + 8 个邻居的 FEATURES 后视图算中心柱生成期光照。
///
/// 3×3 视图按 `(dz, dx)` 排列，中心在下标 4；光最多走 15 格，小于一个区块宽 16，
/// 所以这一圈足够把中心柱照全，视图之外读成全空气（天空光 15、方块光 0）。
/// `input` 只对整块对拍的改坏确认放行 `Carvers`；游戏流水线恒用默认值。
///
/// `ChunkStatus.LIGHT`: compute the center column's light from the center and 8
/// neighbors' post-FEATURES view. Light travels at most 15 blocks < one chunk's
/// 16, so the 3×3 view suffices; outside reads as air (sky 15, block 0).
/// `input` only lets the whole-chunk oracle's break confirmation request
/// `Carvers`; the game pipeline always uses the default.
///
/// 输入来自官方 / 本地配置，`neighbors` 里可以缺快照；所有坐标都由
/// `resolve_shared_view` 夹到半径 1，不会对客户端数据做下标。
/// Inputs come from local config or vanilla data; missing snapshots are
/// regenerated and all positions are clamped to radius 1.
pub fn generate_light_stage_with_input(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    center: Option<Arc<GeneratedChunk>>,
    neighbors: SnapshotView<'_>,
    input: LightInput,
) -> Result<GeneratedLight, String> {
    let _profile_timer = crate::worldgen_profile::timer(&LIGHT_NS);
    crate::worldgen_profile::count(&LIGHT_CALLS);
    let view = resolve_shared_view(config, cx, cz, center, neighbors)?;
    let folded = if input == LightInput::PostFeatures {
        Some(post_features_view(config, cx, cz, &view)?)
    } else {
        None
    };
    let columns: Vec<(ChunkPos, &[Vec<u32>])> = (0..9_i32)
        .map(|index| {
            let dx = index % 3 - 1;
            let dz = index / 3 - 1;
            let layers: &[Vec<u32>] = match &folded {
                Some(blocks) => &blocks[index as usize],
                None => view[index as usize].blocks.as_slice(),
            };
            (ChunkPos::new(cx + dx, cz + dz), layers)
        })
        .collect();
    generate_light_from_layers(ChunkPos::new(cx, cz), &columns)
}

/// 整块光照对拍用：取出 `LIGHT` 阶段实际喂给光引擎的 3×3 方块。
/// 走和游戏流水线完全相同的快照拼装 + 地物写合并路径。
///
/// For the whole-chunk light oracle: the exact 3x3 block layers the `LIGHT`
/// stage hands to the engine, built through the game pipeline's own snapshot
/// assembly and feature-write merge.
pub fn post_features_blocks(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    center: Option<Arc<GeneratedChunk>>,
    neighbors: SnapshotView<'_>,
) -> Result<Vec<Vec<Vec<u32>>>, String> {
    let view = resolve_shared_view(config, cx, cz, center, neighbors)?;
    post_features_view(config, cx, cz, &view)
}

/// 游戏流水线入口：恒用 FEATURES 后视图。
/// Game pipeline entry: always use the post-FEATURES view.
pub(crate) fn generate_light_stage(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    center: Option<Arc<GeneratedChunk>>,
    neighbors: SnapshotView<'_>,
) -> Result<GeneratedLight, String> {
    generate_light_stage_with_input(config, cx, cz, center, neighbors, LightInput::PostFeatures)
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

    /// 缺快照时补的格子和 `terrain` 阶段的快照相同（结构区块带 Beardifier），
    /// 光照输入不随「快照先到还是后到」变。
    /// A snapshot-less cell equals the `terrain` stage snapshot (Beardifier
    /// included), so the light input does not depend on snapshot timing.
    #[test]
    fn light_view_fallback_matches_the_terrain_stage_snapshot() {
        let config = WorldGenConfig {
            seed: 42,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        let (cx, cz) = (17, 7);
        let staged = crate::worldgen_features::generate_terrain_stage(&config, cx, cz)
            .unwrap()
            .terrain
            .expect("terrain stage returns its snapshot");
        let plain = crate::worldgen::generate_chunk(&config, cx, cz).unwrap();
        assert_ne!(staged.blocks, plain.blocks);
        let view = resolve_shared_view(&config, cx, cz, None, &[]).unwrap();
        assert_eq!(view[4].blocks, staged.blocks);
    }

    /// FEATURES 后视图和 CARVERS 输入在有地物的坐标上必须给出不同的光照：
    /// 整块对拍的改坏确认就靠这个差异。
    /// The post-FEATURES view must give different light from CARVERS at a chunk
    /// with features; the whole-chunk oracle's break confirmation relies on it.
    #[test]
    fn post_features_light_differs_from_carvers_light() {
        let config = WorldGenConfig {
            seed: 2026,
            level_type: LevelType::Normal,
            generate_structures: false,
            ..WorldGenConfig::default()
        };
        let post =
            generate_light_stage_with_input(&config, -2, -2, None, &[], LightInput::PostFeatures)
                .unwrap();
        let carvers =
            generate_light_stage_with_input(&config, -2, -2, None, &[], LightInput::Carvers)
                .unwrap();
        assert_ne!(post.sky, carvers.sky);
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
