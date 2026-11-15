//! `ScatteredFeaturePiece` 的两个高度校正：`updateAverageGroundHeight` 与
//! `updateHeightPositionToLowestGroundHeight`。
//!
//! 这两个方法都用 `MOTION_BLOCKING_NO_LEAVES` 高度图在结构件的包围盒范围内求平均 / 最小值，
//! 再把包围盒整体竖直平移。女巫小屋用前者（`offset = 0`），沙漠神殿用后者
//! （`offset = -random.nextInt(3)`）。移植时把「算出的 `heightPosition`」和「平移后的包围盒」
//! 一起返回，让调用方既能用它写方块，也能把它记进 `HPos`。
//!
//! The two height adjustments of `ScatteredFeaturePiece`:
//! `updateAverageGroundHeight` and `updateHeightPositionToLowestGroundHeight`. Both sample
//! the `MOTION_BLOCKING_NO_LEAVES` heightmap over the piece box and translate the box
//! vertically. The swamp hut uses the first (`offset = 0`), the desert pyramid the second
//! (`offset = -random.nextInt(3)`). The port returns both the resulting `heightPosition` and
//! the translated box so callers can place blocks and record `HPos`.

use crate::{
    feature::world::{FeatureWorld, HeightmapKind},
    structure::place::PieceContext,
};

/// `updateAverageGroundHeight(level, chunkBB, offset)`：盒子内 `chunkBB` 里每一列的
/// `MOTION_BLOCKING_NO_LEAVES` 高度求平均（整数除法）。没有一列在盒子里时返回 `None`。
///
/// `updateAverageGroundHeight(level, chunkBB, offset)`: average the
/// `MOTION_BLOCKING_NO_LEAVES` height of every column of the box that lies inside
/// `chunkBB` (integer division). Returns `None` when no column is inside.
pub fn average_ground_height(
    world: &FeatureWorld,
    piece_bb: &[i32; 6],
    chunk_bb: &[i32; 6],
    offset: i32,
) -> Option<(i32, [i32; 6])> {
    let mut total = 0i64;
    let mut count = 0i64;
    for z in piece_bb[2]..=piece_bb[5] {
        for x in piece_bb[0]..=piece_bb[3] {
            // 官方用 `pos.set(x, 64, z)` 再 `chunkBB.isInside(pos)`：Y 只用来做
            // 盒子包含判定，所以必须是 64 而不是真实高度。
            // Vanilla sets `pos` to y=64 before `chunkBB.isInside(pos)`: the y only takes
            // part in the containment test, so it must be 64, not a real height.
            if !PieceContext::inside(chunk_bb, (x, 64, z)) {
                continue;
            }
            total += i64::from(world.get_height(HeightmapKind::MotionBlockingNoLeaves, x, z));
            count += 1;
        }
    }
    if count == 0 {
        return None;
    }
    let height_position = (total / count) as i32;
    let dy = height_position - piece_bb[1] + offset;
    Some((
        height_position,
        [
            piece_bb[0],
            piece_bb[1] + dy,
            piece_bb[2],
            piece_bb[3],
            piece_bb[4] + dy,
            piece_bb[5],
        ],
    ))
}

/// `updateHeightPositionToLowestGroundHeight(level, offset)`：盒子内所有列的
/// `MOTION_BLOCKING_NO_LEAVES` 高度取最小，再把包围盒平移过去。
///
/// `updateHeightPositionToLowestGroundHeight(level, offset)`: take the minimum
/// `MOTION_BLOCKING_NO_LEAVES` height over the box and translate the box there.
pub fn lowest_ground_height(
    world: &FeatureWorld,
    piece_bb: &[i32; 6],
    offset: i32,
) -> (i32, [i32; 6]) {
    // `lowestGroundHeight = level.getMaxY() + 1`，`foundPositionWithinBoundingBox` 在这里
    // 恒为 true（循环上下界都来自包围盒本身）。
    // `lowestGroundHeight = level.getMaxY() + 1`; `foundPositionWithinBoundingBox` is always
    // true here because both loop bounds come from the box itself.
    let mut lowest = world.min_y + world.height;
    for z in piece_bb[2]..=piece_bb[5] {
        for x in piece_bb[0]..=piece_bb[3] {
            lowest = lowest.min(world.get_height(HeightmapKind::MotionBlockingNoLeaves, x, z));
        }
    }
    let dy = lowest - piece_bb[1] + offset;
    (
        lowest,
        [
            piece_bb[0],
            piece_bb[1] + dy,
            piece_bb[2],
            piece_bb[3],
            piece_bb[4] + dy,
            piece_bb[5],
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn average_ignores_columns_outside_the_chunk_box() {
        // 用一个只有中心区块的 FeatureWorld 不现实，这里只验证「没有一列在盒子里」的分支。
        // A world with only the centre chunk is impractical here; this only covers the
        // "no column inside the box" branch.
        let bb = [0, 64, 0, 0, 70, 0];
        let chunk_bb = [100, 0, 100, 115, 200, 115];
        let chunks = super::tests::empty_chunks(-64, 384);
        let world = FeatureWorld::new(&chunks).unwrap();
        assert_eq!(average_ground_height(&world, &bb, &chunk_bb, 0), None);
    }

    /// 造一份 3×3、全空气、高度图全 `minY - 1` 的输入。
    ///
    /// Build a 3x3 all-air input whose heightmaps are all `minY - 1`.
    pub(super) fn empty_chunks(min_y: i32, height: i32) -> crate::feature::FeatureChunks {
        use crate::feature::{CENTER, ChunkBlocks, FeatureChunks, Heightmaps};
        let blocks: ChunkBlocks = vec![vec![mc_bamboo_data::blocks::AIR; 256]; height as usize];
        let heights = Heightmaps::new(min_y);
        let biomes = std::array::from_fn(|_| vec![vec![0u32; 64]; 4]);
        let mut blocks_all: [ChunkBlocks; 9] = std::array::from_fn(|_| blocks.clone());
        blocks_all[CENTER] = blocks;
        FeatureChunks {
            center_x: 0,
            center_z: 0,
            min_y,
            height,
            blocks: blocks_all,
            biomes,
            heights: [heights; 9],
        }
    }
}
