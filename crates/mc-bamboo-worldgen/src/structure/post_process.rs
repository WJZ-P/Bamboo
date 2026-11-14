//! 四个代码生成单件结构的 `postProcess`：把方块写进已生成到 `features` 阶段的世界。
//!
//! 本轮先做埋藏的宝藏（`BuriedTreasurePieces.BuriedTreasurePiece.postProcess`）：它不取随机数、
//! 不写方块实体，只按 `OCEAN_FLOOR_WG` 高度找到埋宝的那一层砂岩 / 石头，把周围换成沙子。
//! 其余三个（女巫小屋、沙漠神殿、丛林神殿）在各自的结构模块里按官方顺序追加。
//!
//! `postProcess` of the four code-built single-piece structures. This round starts with the
//! buried treasure (`BuriedTreasurePieces.BuriedTreasurePiece.postProcess`): it draws no
//! randomness and writes no block entity, only walks down from the `OCEAN_FLOOR_WG` height to
//! the sandstone / stone layer holding the chest and turns the neighbours into sand. The other
//! three (swamp hut, desert pyramid, jungle temple) are appended per structure.

use mc_bamboo_data::blocks;

use crate::{
    feature::world::{FeatureWorld, HeightmapKind},
    structure::place::PieceContext,
};

/// 官方 `BuriedTreasurePiece.postProcess` 里判「液体」用的两个方块。
///
/// The two blocks `BuriedTreasurePiece.postProcess` treats as liquid.
fn is_liquid(state: u32) -> bool {
    state == blocks::WATER || state == blocks::LAVA
}

/// `BuriedTreasurePiece.postProcess` 的方块部分。
///
/// 结构件包围盒是单点 `(x, 90, z)`，`postProcess` 会把它改成找到的那一格；写方块只发生在
/// `pos` 周围的六个邻居（`createChest` 写箱子那一步只返回位置，战利品表要取随机数）。
///
/// The block part of `BuriedTreasurePiece.postProcess`. The piece box is the single point
/// `(x, 90, z)`; `postProcess` rewrites it to the layer it finds. Writes only touch the six
/// neighbours (`createChest` only reports the position, since its loot table draws a number).
///
/// 返回 `Some((chest_pos, new_bb))`：找到埋宝层时给出箱子位置和更新后的包围盒。
///
/// Returns `Some((chest_pos, new_bb))` when the treasure layer is found: the chest position
/// and the updated bounding box.
pub fn buried_treasure(
    world: &mut FeatureWorld,
    bb: &[i32; 6],
) -> Option<((i32, i32, i32), [i32; 6])> {
    let (x, z) = (bb[0], bb[2]);
    // `level.getHeight(OCEAN_FLOOR_WG, x, z)` 已经是「最高方块 y + 1」。
    // `level.getHeight(OCEAN_FLOOR_WG, x, z)` already is "top block y + 1".
    let mut y = world.get_height(HeightmapKind::OceanFloorWg, x, z);
    let min_y = world.min_y;

    while y > min_y {
        let current = world.get_block(x, y, z);
        let below = world.get_block(x, y - 1, z);
        if below == blocks::SANDSTONE
            || below == blocks::STONE
            || below == blocks::ANDESITE
            || below == blocks::GRANITE
            || below == blocks::DIORITE
        {
            let soft = if current != blocks::AIR && !is_liquid(current) {
                current
            } else {
                blocks::SAND
            };
            // `Direction.values()` = DOWN, UP, NORTH, SOUTH, WEST, EAST（3D 数据值顺序）。
            // `Direction.values()` = DOWN, UP, NORTH, SOUTH, WEST, EAST (3D data value order).
            for (dx, dy, dz) in [
                (0, -1, 0),
                (0, 1, 0),
                (0, 0, -1),
                (0, 0, 1),
                (-1, 0, 0),
                (1, 0, 0),
            ] {
                let relative = (x + dx, y + dy, z + dz);
                let relative_state = world.get_block(relative.0, relative.1, relative.2);
                if relative_state == blocks::AIR || is_liquid(relative_state) {
                    let below_relative = world.get_block(relative.0, relative.1 - 1, relative.2);
                    let place = if (below_relative == blocks::AIR || is_liquid(below_relative))
                        && (dx, dy, dz) != (0, 1, 0)
                    {
                        below
                    } else {
                        soft
                    };
                    world.set_block_with_heightmaps(relative.0, relative.1, relative.2, place);
                }
            }
            return Some(((x, y, z), [x, y, z, x, y, z]));
        }
        y -= 1;
    }
    None
}

/// 埋藏的宝藏不需要 `PieceContext`（它不经 `placeBlock` 的 rotation 变换，自己写方块）。
/// 这个别名让调用方的签名和其它结构保持同一形状。
///
/// The buried treasure needs no `PieceContext` (it writes blocks itself, without
/// `placeBlock`'s rotation). This alias keeps the call shape uniform with other structures.
pub type PlainWorld<'a, 'b> = (&'a mut FeatureWorld<'b>, &'a [i32; 6]);

/// 占位：`place` 模块的 `PieceContext` 给后续三个结构用，这里先引用一次避免未使用告警。
///
/// Placeholder: `PieceContext` from the `place` module is for the remaining three structures;
/// referenced once so the import is not flagged.
#[allow(dead_code)]
fn _piece_context(_: PieceContext<'_>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liquid_is_water_and_lava() {
        assert!(is_liquid(blocks::WATER));
        assert!(is_liquid(blocks::LAVA));
        assert!(!is_liquid(blocks::SAND));
    }
}
