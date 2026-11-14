//! `ChunkStatus.FEATURES`：`ChunkGenerator.applyBiomeDecoration` 的纯计算部分。
//!
//! 官方这一步按 `GenerationStep.Decoration` 的顺序放置配置好的地物，读写中心区块加周围
//! 8 个区块（3×3）。这里把 3×3 的方块当成输入，输出 9 个区块跑完地物之后的样子和给
//! 8 个邻居的写增量；函数不碰世界状态，随机数只从世界种子和区块坐标派生。
//!
//! 数据来自 `mc-bamboo-data::worldgen::WORLDGEN` 里的 `feature`、`placed_feature`、
//! `biome_features` 和 `features_per_step` 四个键，见 `docs/worldgen-features-plan.md`。

pub mod ores;
pub mod placement;
pub mod provider;
pub mod registry;
pub mod rule_test;
pub mod run;
pub mod state;
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
