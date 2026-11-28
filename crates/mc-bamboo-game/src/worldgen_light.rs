//! `ChunkStatus.INITIALIZE_LIGHT` / `LIGHT`：生成期光照接进流水线。
//!
//! 官方 `GENERATION_PYRAMID` 里 `LIGHT` 的直接依赖是半径 1 的
//! `INITIALIZE_LIGHT`，而每格的 `INITIALIZE_LIGHT` 又排在 `FEATURES` 之后：
//! 所以中心与 8 个邻居在算光时都已跑完自己的 `FEATURES`。光引擎通过
//! `LightChunkGetter` 读到的就是这 3×3 里当时已有的方块，包括邻居中心的地物
//! 溢出写进来的格子（`ChunkPyramid.<clinit>`、`ChunkStatusTasks.initializeLight`
//! / `light`、`LevelLightEngine.propagateLightSources`）。
//!
//! 这里仅折 3×3 内来源中心的地物写，避免为光照额外生成外圈。相同快照及非空来源
//! 集合共享方块层；光引擎、交付方块和完工合并规则不变。
//!
//! Wire `ChunkStatus.LIGHT` into the pipeline. In vanilla's `GENERATION_PYRAMID`,
//! `LIGHT` requires radius-1 `INITIALIZE_LIGHT` and each chunk reaches
//! `INITIALIZE_LIGHT` after `FEATURES`, so the center and all 8 neighbors have
//! completed their own features when light runs. The engine reads those very
//! blocks through `LightChunkGetter`, including the cells a neighbor's features
//! spilled into. Here we fold only sources within the 3×3 to avoid generating
//! an outer ring for light. Identical snapshots and nonempty source sets share
//! block layers; the light engine, delivered blocks and assembly rules are unchanged.

use std::sync::Arc;

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::light::generate::{GeneratedLight, generate_light_from_layers};

use crate::{
    world_options::WorldGenConfig,
    worldgen::GeneratedChunk,
    worldgen_features::{
        LightFeatureRuns, SnapshotView, cached_generate_chunk_with_structures as terrain_cell,
    },
    worldgen_profile::{LIGHT_CALLS, LIGHT_NS},
};

mod final_blocks;

pub(crate) use final_blocks::clear as clear_final_blocks_cache;

/// 整块光照对拍用的输入：默认（和游戏里一样）只折 3×3 内来源的 FEATURES 写；
/// `Carvers` 故意退回旧行为，证明比较器会抓住树冠 / 光源附近的差异。
///
/// Input for the whole-chunk light oracle. The default (and the game) folds
/// FEATURES writes from sources within the 3x3; `Carvers` reverts to the old input
/// so the comparator must report canopy / emitter differences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LightInput {
    /// 默认的局部 FEATURES 后视图。
    ///
    /// The default local post-FEATURES view.
    #[default]
    PostFeatures,
    /// 旧的 CARVERS 输入，只给改坏确认用。
    /// The old CARVERS input, only for break confirmation.
    #[cfg(feature = "oracle-samples")]
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

/// 只折来源中心和写入格都在光照目标 3×3 内的地物写。
///
/// Fold feature writes only when the source center and destination both lie
/// inside the light target's 3×3.
fn post_features_view(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    terrain: &[Arc<GeneratedChunk>; 9],
) -> Result<Vec<final_blocks::Layers>, String> {
    let runs = LightFeatureRuns::load(config, cx, cz)?;
    Ok(view_positions(cx, cz)
        .zip(terrain)
        .enumerate()
        .map(|(slot, (pos, cell))| {
            final_blocks::get(config, pos.x, pos.z, cell, runs.sources(slot))
        })
        .collect())
}

fn view_positions(cx: i32, cz: i32) -> impl Iterator<Item = ChunkPos> {
    (0..9).map(move |index| ChunkPos::new(cx + (index % 3 - 1), cz + (index / 3 - 1)))
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
    let columns: Vec<(ChunkPos, &[Vec<u32>])> = view_positions(cx, cz)
        .enumerate()
        .map(|(index, pos)| {
            let layers: &[Vec<u32>] = match &folded {
                Some(blocks) => &blocks[index],
                None => &view[index].blocks,
            };
            (pos, layers)
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
#[cfg(feature = "oracle-samples")]
pub fn post_features_blocks(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
    center: Option<Arc<GeneratedChunk>>,
    neighbors: SnapshotView<'_>,
) -> Result<Vec<Vec<Vec<u32>>>, String> {
    let view = resolve_shared_view(config, cx, cz, center, neighbors)?;
    Ok(post_features_view(config, cx, cz, &view)?
        .into_iter()
        .map(Arc::unwrap_or_clone)
        .collect())
}

/// 对拍专用：按完工规则折入每个视图格的全部来源，包括光照目标外圈。
///
/// Audit only: apply every source to each view cell using delivery's merge
/// rules, including source centers outside the light target's 3×3.
#[cfg(any(test, feature = "oracle-samples"))]
pub fn full_post_features_blocks(
    config: &WorldGenConfig,
    cx: i32,
    cz: i32,
) -> Result<Vec<Vec<Vec<u32>>>, String> {
    let view = resolve_shared_view(config, cx, cz, None, &[])?;
    view_positions(cx, cz)
        .zip(view)
        .map(|(pos, cell)| {
            let mut chunk = (*cell).clone();
            let writes = crate::worldgen_features::feature_writes_into(config, pos.x, pos.z)?;
            crate::worldgen_features::merge_feature_writes(&mut chunk, writes);
            Ok(chunk.blocks)
        })
        .collect()
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
    use mc_bamboo_core::{BlockPos, LightLayer};
    use mc_bamboo_world::light::{LIGHT_SECTION_COUNT, MIN_LIGHT_SECTION};

    use super::*;
    use crate::{
        region::chunk_content,
        world_options::LevelType,
        worldgen::build_region_column,
        worldgen_features::{feature_writes_into, merge_feature_writes},
        worldgen_mobs::spawn_original_mobs,
    };

    #[test]
    fn light_stage_matches_full_fold() {
        let mut original_mobs = 0;
        let mut chunks = 0;
        let mut all_raw_diffs = 0;
        for (seed, x, z, side) in [(2026, 0, 0, 5), (42, -3, 5, 2), (7, 10, -8, 2)] {
            let config = WorldGenConfig {
                seed,
                level_type: LevelType::Normal,
                ..WorldGenConfig::default()
            };
            let mut sky_diffs = 0;
            let mut block_diffs = 0;
            let mut raw_diffs = 0;
            for dz in 0..side {
                for dx in 0..side {
                    let (x, z) = (x + dx, z + dz);
                    let terrain = resolve_shared_view(&config, x, z, None, &[]).unwrap();
                    let baseline = full_post_features_blocks(&config, x, z).unwrap();
                    let shared = post_features_view(&config, x, z, &terrain).unwrap();
                    assert!(baseline[4] == *shared[4], "center blocks: {seed} {x},{z}");
                    let columns: Vec<_> = view_positions(x, z)
                        .zip(baseline.iter().map(Vec::as_slice))
                        .collect();
                    let old = generate_light_from_layers(ChunkPos::new(x, z), &columns).unwrap();
                    let new = generate_light_stage(&config, x, z, None, &[]).unwrap();
                    raw_diffs += old
                        .sky
                        .iter()
                        .chain(&old.block)
                        .zip(new.sky.iter().chain(&new.block))
                        .filter(|(a, b)| a != b)
                        .count();
                    let pos = ChunkPos::new(x, z);
                    let mut assembled = (*terrain[4]).clone();
                    let writes = feature_writes_into(&config, x, z).unwrap();
                    merge_feature_writes(&mut assembled, writes);
                    let deliver = |light| {
                        let mut column = build_region_column(&assembled).unwrap();
                        column.set_generated_light(light);
                        let before = column.parts().entities.rows().len();
                        spawn_original_mobs(&config, pos, &mut column, true);
                        let added = column.parts().entities.rows().len() - before;
                        (column, added)
                    };
                    let (mut old_column, added) = deliver(old);
                    let (new_column, new_added) = deliver(new.clone());
                    for (layer, diffs) in [
                        (LightLayer::Sky, &mut sky_diffs),
                        (LightLayer::Block, &mut block_diffs),
                    ] {
                        let min_y = MIN_LIGHT_SECTION * 16;
                        for y in min_y..min_y + LIGHT_SECTION_COUNT as i32 * 16 {
                            for dx in 0..16 {
                                for dz in 0..16 {
                                    let at = BlockPos::new(x * 16 + dx, y, z * 16 + dz);
                                    let a = old_column.column().light_value(layer, at);
                                    let b = new_column.column().light_value(layer, at);
                                    if a != b {
                                        *diffs += 1;
                                        println!(
                                            "seed={seed} {layer:?} at {at:?}: exact={a} local={b}"
                                        );
                                    }
                                }
                            }
                        }
                    }
                    old_column.set_generated_light(new);
                    let old_nbt = chunk_content::encode(&old_column, pos).unwrap();
                    let new_nbt = chunk_content::encode(&new_column, pos).unwrap();
                    assert!(
                        old_nbt == new_nbt,
                        "non-light chunk or entity NBT: {seed} {x},{z}"
                    );
                    assert_eq!(added, new_added);
                    original_mobs += added;
                    chunks += 1;
                }
            }
            println!(
                "seed={seed} origin=({x},{z}) chunks={} sky_cells={sky_diffs} block_cells={block_diffs} raw_layer_diffs={raw_diffs}",
                side * side
            );
            all_raw_diffs += raw_diffs;
            assert_eq!((sky_diffs, block_diffs), (0, 0), "fixed audit sample");
        }
        assert!(original_mobs > 0, "sample must include original mob spawns");
        println!(
            "seeds=3 origins=3 chunks={chunks} sky_cells=0 block_cells=0 raw_layer_diffs={all_raw_diffs} non_light_nbt_diffs=0 original_mobs={original_mobs}"
        );
    }

    #[test]
    fn light_stage_respects_distinct_terrain_inputs_at_the_same_position() {
        let config = WorldGenConfig::flat_template();
        let original = terrain_cell(&config, 0, 0).unwrap();
        let mut changed = (*original).clone();
        let top_layer = changed.blocks.last_mut().unwrap();
        top_layer[0] = mc_bamboo_data::blocks::STONE;
        let original_layers = final_blocks::get(&config, 0, 0, &original, [&[]; 9]);
        let changed_layers = final_blocks::get(&config, 0, 0, &Arc::new(changed), [&[]; 9]);
        assert_eq!(
            original_layers.last().unwrap()[0],
            mc_bamboo_data::blocks::AIR
        );
        assert_eq!(
            changed_layers.last().unwrap()[0],
            mc_bamboo_data::blocks::STONE
        );
    }

    #[test]
    fn light_layer_cache_does_not_alias_distinct_source_masks() {
        let config = WorldGenConfig::flat_template();
        let terrain = terrain_cell(&config, 41, 12).unwrap();
        let omitted = final_blocks::get(&config, 41, 12, &terrain, [&[]; 9]);
        let y = mc_bamboo_world::MIN_Y + mc_bamboo_world::WORLD_HEIGHT - 1;
        let delta = [(41 * 16, y, 12 * 16, mc_bamboo_data::blocks::STONE)];
        let mut sources: [&[_]; 9] = [&[]; 9];
        sources[4] = &delta;
        let included = final_blocks::get(&config, 41, 12, &terrain, sources);
        assert_ne!(*omitted, *included);
    }

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
    #[cfg(feature = "oracle-samples")]
    #[test]
    fn post_features_light_differs_from_carvers_light() {
        let config = WorldGenConfig {
            seed: 2026,
            level_type: LevelType::Normal,
            generate_structures: false,
            ..WorldGenConfig::default()
        };
        {
            let post = generate_light_stage_with_input(
                &config,
                -2,
                -2,
                None,
                &[],
                LightInput::PostFeatures,
            )
            .unwrap();
            let carvers =
                generate_light_stage_with_input(&config, -2, -2, None, &[], LightInput::Carvers)
                    .unwrap();
            assert_ne!(post.sky, carvers.sky);
        }
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
