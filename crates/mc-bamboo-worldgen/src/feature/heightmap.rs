//! `Heightmap` 的增量更新：`ProtoChunk.setBlockState` 里对已 primed 高度图的
//! `Heightmap.update(localX, localY, localZ, state)`。
//!
//! 官方 `ChunkStatus.FINAL_HEIGHTMAPS` 是 `WORLD_SURFACE`、`OCEAN_FLOOR`、
//! `MOTION_BLOCKING`、`MOTION_BLOCKING_NO_LEAVES` 四种，features 阶段只更新这四种；
//! `WORLD_SURFACE_WG` / `OCEAN_FLOOR_WG` 在 doFill 里增量更新后固定，features 阶段不动。
//! `OreFeature` 走 `BulkSectionAccess` 直接写 `LevelChunkSection`，绕过 `setBlockState`，
//! 所以 step 6 期间任何高度图都不更新；这条路径仍走 `world::set_block`。
//!
//! Incremental `Heightmap.update` from `ProtoChunk.setBlockState`.  Only the four
//! `FINAL_HEIGHTMAPS` types are updated during features; the two WG types are fixed
//! after `doFill`.  `OreFeature` writes through `BulkSectionAccess`, bypassing
//! `setBlockState` entirely, so step 6 leaves all heightmaps untouched.

use mc_bamboo_data::{facts, registries, state_facts, state_fluid};

use super::{ChunkBlocks, Heightmaps, world::HeightmapKind};

/// features 阶段要增量更新的四种高度图，顺序照 `ChunkStatus.FINAL_HEIGHTMAPS`。
///
/// The four heightmap types updated during features, in `FINAL_HEIGHTMAPS` order.
pub const FINAL_KINDS: [HeightmapKind; 4] = [
    HeightmapKind::OceanFloor,
    HeightmapKind::WorldSurface,
    HeightmapKind::MotionBlocking,
    HeightmapKind::MotionBlockingNoLeaves,
];

/// 官方 `Heightmap.Types.isOpaque()`：这一格算不算这一列的地面。
///
/// Official `Heightmap.Types.isOpaque()`: whether this state counts as ground for
/// this heightmap column.
pub fn is_opaque(state: u32, kind: HeightmapKind) -> bool {
    let bits = state_facts(state);
    // `!state.getFluidState().isEmpty()`：只有空流体才算没有流体。
    // `!state.getFluidState().isEmpty()`: only the empty fluid counts as no fluid.
    // Only the empty fluid means "no fluid"; waterlogged blocks have a fluid.
    let has_fluid = state_fluid(state).fluid != registries::fluid::EMPTY;
    match kind {
        HeightmapKind::WorldSurfaceWg | HeightmapKind::WorldSurface => bits & facts::AIR == 0,
        HeightmapKind::OceanFloorWg | HeightmapKind::OceanFloor => {
            bits & facts::MOTION_BLOCKING != 0
        }
        HeightmapKind::MotionBlocking => bits & facts::MOTION_BLOCKING != 0 || has_fluid,
        HeightmapKind::MotionBlockingNoLeaves => {
            bits & facts::MOTION_BLOCKING_NO_LEAVES != 0 || has_fluid
        }
    }
}

/// 把 `(kind, column)` 映射到 `Heightmaps` 里对应的可变槽位。
///
/// Map `(kind, column)` to the matching mutable slot in `Heightmaps`.
fn slot(heights: &mut Heightmaps, kind: HeightmapKind, column: usize) -> &mut i32 {
    match kind {
        HeightmapKind::WorldSurfaceWg => &mut heights.world_surface_wg[column],
        HeightmapKind::WorldSurface => &mut heights.world_surface[column],
        HeightmapKind::OceanFloorWg => &mut heights.ocean_floor_wg[column],
        HeightmapKind::OceanFloor => &mut heights.ocean_floor[column],
        HeightmapKind::MotionBlocking => &mut heights.motion_blocking[column],
        HeightmapKind::MotionBlockingNoLeaves => &mut heights.motion_blocking_no_leaves[column],
    }
}

/// 读一个区块在 `(x, y, z)` 的方块；`blocks` 是 `height` 层、每层 `x*16+z`。
///
/// Read one block from a chunk; `blocks` is `height` layers of `x*16+z`.
fn block_at(blocks: &ChunkBlocks, min_y: i32, x: i32, y: i32, z: i32) -> u32 {
    let local_y = y - min_y;
    if local_y < 0 || local_y as usize >= blocks.len() {
        return mc_bamboo_data::blocks::AIR;
    }
    blocks[local_y as usize][(x & 15) as usize * 16 + (z & 15) as usize]
}

/// 一次 `Heightmap.update`，对四种 FINAL 高度图分别执行。
///
/// `Heightmaps` 存的是「该列最高不透明方块的 y」（`prime_heightmaps` 的约定），
/// 官方 `getFirstAvailable` = 这个值 + 1。四种都照 `Heightmap.update` 字节码：
///   * `y <= firstAvailable - 2`（即 `y < stored`）不动；
///   * 新状态 opaque 且 `y >= firstAvailable`（即 `y > stored`）时抬到 `y`；
///   * 非 opaque 且 `y == stored`（挖掉了最高那格）时向下扫，找到第一个 opaque，
///     找不到就回到 `min_y - 1`。
///
/// One `Heightmap.update` for each of the four FINAL types.  `Heightmaps` stores
/// the highest opaque block's y; official `getFirstAvailable` is that value + 1.
/// The update follows the bytecode exactly, including the downward rescan when the
/// top opaque block is replaced by a non-opaque one.
pub fn update(
    heights: &mut Heightmaps,
    blocks: &ChunkBlocks,
    min_y: i32,
    x: i32,
    y: i32,
    z: i32,
    state: u32,
) {
    let column = (x & 15) as usize * 16 + (z & 15) as usize;
    for kind in FINAL_KINDS {
        let stored = *slot(heights, kind, column);
        let first_available = stored + 1;
        // `if (localY <= firstAvailable - 2) return false;`
        if y <= first_available - 2 {
            continue;
        }
        if is_opaque(state, kind) {
            // `if (localY >= firstAvailable) setHeight(localY + 1);`
            if y >= first_available {
                *slot(heights, kind, column) = y;
            }
        } else if first_available - 1 == y {
            // 挖掉了最高那格：向下找新的最高 opaque。
            // The top opaque block was removed: rescan downward.
            let mut found = min_y - 1;
            for candidate_y in (min_y..y).rev() {
                let below = block_at(blocks, min_y, x, candidate_y, z);
                if is_opaque(below, kind) {
                    found = candidate_y;
                    break;
                }
            }
            *slot(heights, kind, column) = found;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::blocks;

    fn empty(min_y: i32) -> Heightmaps {
        Heightmaps::new(min_y)
    }

    #[test]
    fn placing_a_solid_block_raises_all_four_final_maps() {
        let min_y = -64;
        let mut heights = empty(min_y);
        let column = 3 * 16 + 5;
        // 一列全空气。
        // A column of all air.
        let blocks: ChunkBlocks = vec![vec![blocks::AIR; 256]; 8];
        update(&mut heights, &blocks, min_y, 3, min_y, 5, blocks::STONE);
        for kind in FINAL_KINDS {
            assert_eq!(*slot(&mut heights, kind, column), min_y, "{kind:?}");
        }
    }

    #[test]
    fn removing_the_top_block_recomputes_downward() {
        let min_y = -64;
        let column = 0;
        let mut blocks: ChunkBlocks = vec![vec![blocks::AIR; 256]; 8];
        blocks[0][column] = blocks::STONE;
        blocks[1][column] = blocks::STONE;
        let mut heights = empty(min_y);
        for kind in FINAL_KINDS {
            *slot(&mut heights, kind, column) = min_y + 1;
        }
        // 把最高那格换成空气，应回落到下面那块石头。
        // Replace the top block with air; the map should fall back to the stone below.
        blocks[1][column] = blocks::AIR;
        update(&mut heights, &blocks, min_y, 0, min_y + 1, 0, blocks::AIR);
        for kind in FINAL_KINDS {
            assert_eq!(*slot(&mut heights, kind, column), min_y, "{kind:?}");
        }
    }

    #[test]
    fn incremental_updates_match_a_fresh_prime() {
        let min_y = -64;
        // 一列自顶向下写若干方块，再逐格用 `update` 维护高度图；最后四种 FINAL 高度图
        // 应和「对最终 blocks 重新 prime」一致。水只算 WORLD_SURFACE，不算 OCEAN_FLOOR。
        // Write a column top-down with `update` at each step; the four FINAL maps must
        // equal a fresh prime of the final blocks. Water counts for WORLD_SURFACE only.
        let column = 7 * 16 + 9;
        let x = 7;
        let z = 9;
        let mut blocks: ChunkBlocks = vec![vec![blocks::AIR; 256]; 16];
        let mut heights = empty(min_y);
        // 自顶向下的写入序列（y 递减），穿插水和空气。
        // Top-down write sequence (descending y), mixing stone, water and air.
        let writes: [(i32, u32); 10] = [
            (min_y + 12, blocks::STONE),
            (min_y + 11, blocks::WATER),
            (min_y + 10, blocks::STONE),
            (min_y + 9, blocks::AIR),
            (min_y + 8, blocks::STONE),
            (min_y + 7, blocks::STONE),
            (min_y + 6, blocks::AIR),
            (min_y + 5, blocks::STONE),
            (min_y + 4, blocks::WATER),
            (min_y + 3, blocks::STONE),
        ];
        for &(y, state) in &writes {
            blocks[(y - min_y) as usize][column] = state;
            update(&mut heights, &blocks, min_y, x, y, z, state);
        }
        let mut fresh = super::super::world::prime_heightmaps(&blocks, min_y, 16);
        for kind in FINAL_KINDS {
            assert_eq!(
                *slot(&mut heights, kind, column),
                *slot(&mut fresh, kind, column),
                "incremental {kind:?} differs from a fresh prime"
            );
        }
    }
}
