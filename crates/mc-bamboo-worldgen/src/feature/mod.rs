//! `ChunkStatus.FEATURES`：`ChunkGenerator.applyBiomeDecoration` 的纯计算部分。
//!
//! 官方这一步按 `GenerationStep.Decoration` 的顺序放置配置好的地物，读写中心区块加周围
//! 8 个区块（3×3）。这里把 3×3 的方块当成输入，输出 9 个区块跑完地物之后的样子和给
//! 8 个邻居的写增量；函数不碰世界状态，随机数只从世界种子和区块坐标派生。
//!
//! 数据来自 `mc-bamboo-data::worldgen::WORLDGEN` 里的 `feature`、`placed_feature`、
//! `biome_features` 和 `features_per_step` 四个键，见 `docs/worldgen-features-plan.md`。

pub mod bamboo;
pub mod block_blob;
pub mod block_column;
pub mod block_pile;
pub mod block_predicate;
pub mod blue_ice;
pub mod composition;
pub mod coral;
pub mod disk;
pub mod dispatch;
#[cfg(test)]
pub mod dripstone_probe;
pub mod dripstone_utils;
pub mod fossil;
pub mod freeze_top_layer;
pub mod geode;
pub mod heightmap;
pub mod huge_mushroom;
pub mod iceberg;
pub mod java_long_set;
pub mod java_set;
pub mod lake;
pub mod large_dripstone;
pub mod monster_room;
pub mod mossy_carpet;
pub mod multiface;
pub mod noise_state;
pub mod ores;
pub mod placement;
pub mod provider;
pub mod registry;
pub mod root_system;
pub mod rule_test;
pub mod run;
pub mod sculk_patch;
pub mod shape_update;
pub mod simple_block;
pub mod single_block_pillar;
pub mod speleothem;
pub mod speleothem_cluster;
pub mod spike;
pub mod spring;
pub mod state;
pub mod state_provider;
pub mod survival;
pub mod template_feature;
pub mod tree;
pub mod underwater_magma;
pub mod vegetation_patch;
pub mod vines;
pub mod world;

/// `GenerationStep.Decoration` 的枚举项个数（26.3 是 11 个）。
///
/// 官方 `applyBiomeDecoration` 的循环上界是
/// `max(featuresPerStep.size(), Decoration.values().length)`；导出的表恰好是 11 步。
pub const DECORATION_STEPS: usize = 11;

/// 3×3 视图里中心区块的下标：`(dx, dz)` 各从 -1 到 1，dz 在外、dx 在内。
pub const CENTER: usize = 4;

/// 一个区块到 `features` 之前的方块，按 `height` 层、每层 `x*16 + z` 排列。
pub type ChunkBlocks = Vec<Vec<u32>>;

/// 中心区块加周围 8 个区块，各自是 `terrain` 状态跑完的结果。
///
/// 下标顺序固定为 `for dz in -1..=1 { for dx in -1..=1 { ... } }`，中心在
/// [`CENTER`]。输入只读；写入都落在 [`FeatureResult`] 里。
#[derive(Debug)]
pub struct FeatureChunks {
    pub center_x: i32,
    pub center_z: i32,
    pub min_y: i32,
    pub height: i32,
    /// 9 个区块的方块。
    pub blocks: [ChunkBlocks; 9],
    /// 9 个区块的 quart 群系格（BIOMES 阶段的结果），每段 64 个群系注册表 ID。
    pub biomes: [Vec<Vec<u32>>; 9],
    /// 9 个区块的 6 张 primed heightmap，每张 256 个高度（`ChunkAccess.getHeight` 的值）。
    pub heights: [Heightmaps; 9],
    /// 9 个区块从 `doFill` 带来的 `PostProcessing` 位置，按世界坐标。
    /// `PostProcessing` positions from `doFill`, per chunk, in world coordinates.
    pub post_processing: [Vec<(i32, i32, i32)>; 9],
}

/// `heightmaps::TYPES` 的六种，按官方声明顺序。
///
/// 初值是 `minY - 1`：官方 `Heightmap` 的空柱从这个值开始。
#[derive(Debug, Clone, Copy)]
pub struct Heightmaps {
    pub world_surface_wg: [i32; 256],
    pub world_surface: [i32; 256],
    pub ocean_floor_wg: [i32; 256],
    pub ocean_floor: [i32; 256],
    pub motion_blocking: [i32; 256],
    pub motion_blocking_no_leaves: [i32; 256],
}

impl Heightmaps {
    pub fn new(min_y: i32) -> Self {
        let empty = [min_y - 1; 256];
        Self {
            world_surface_wg: empty,
            world_surface: empty,
            ocean_floor_wg: empty,
            ocean_floor: empty,
            motion_blocking: empty,
            motion_blocking_no_leaves: empty,
        }
    }
}

/// 一次 `features` 阶段的结果。
#[derive(Debug)]
pub struct FeatureResult {
    /// 9 个区块跑完 `features` 之后的方块。中心的最终内容由 Director 直接换进
    /// `ProtoChunk`；邻居只按 [`Self::deltas`] 合并。
    pub blocks: [ChunkBlocks; 9],
    /// 8 个邻居（不含中心）相对输入的写增量，只记真正被 `setBlock` 改过的格子。
    /// 下标和输入的 9 宫格一致，`[CENTER]` 恒为空。
    pub deltas: [Vec<(i32, i32, i32, u32)>; 9],
    /// 写进去的方块实体 nbt，`(世界坐标, nbt)`，同一位置后写覆盖前写。
    /// 现在只有结构件放置会填（`set_block_entity`）。
    ///
    /// Block-entity nbt written into the world as `(world position, nbt)`; a later write to
    /// the same position replaces the earlier one. Only structure placement fills it today.
    pub block_entities: Vec<((i32, i32, i32), mc_bamboo_nbt::Tag)>,
    /// 官方 `PostProcessing` 的世界坐标列表，按 `markPosForPostProcessing` 调用顺序。
    /// Official `PostProcessing` positions in `markPosForPostProcessing` call order.
    pub post_processing: Vec<(i32, i32, i32)>,
    /// 本区块涉及了未移植的地物 / 修饰器 / 谓词，对拍时不参与逐方块比较。
    pub unsupported: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nine_chunks_are_indexed_by_dz_then_dx() {
        for dz in -1..=1 {
            for dx in -1..=1 {
                let index = ((dz + 1) * 3 + dx + 1) as usize;
                assert!(index < 9);
            }
        }
        assert_eq!(CENTER, 4);
    }
}
