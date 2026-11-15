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

use mc_bamboo_blocks::BlockState;
use mc_bamboo_data::{blocks, state_facts};

use crate::{
    feature::world::{FeatureWorld, HeightmapKind},
    structure::place::PieceContext,
};

pub mod igloo;
pub mod ocean_ruin;
pub mod pyramid;
pub mod ruined_portal;
pub mod shipwreck;
pub mod swamp_hut;

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
    mut writes: Option<&mut Vec<crate::structure::place::Write>>,
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
                    if let Some(log) = writes.as_deref_mut() {
                        log.push((relative.0, relative.1, relative.2, place));
                    }
                }
            }
            return Some(((x, y, z), [x, y, z, x, y, z]));
        }
        y -= 1;
    }
    None
}

/// `StructurePiece.createChest(level, chunkBB, random, pos, lootTable, null)` 的方块部分：
/// 盒子内且不是箱子时写一个箱子（朝向用 `reorient`），返回是否写了。
///
/// 战利品表要 `random.nextLong()`，那一半留给调用方；这里只写方块。
///
/// The block part of `StructurePiece.createChest(level, chunkBB, random, pos, lootTable,
/// null)`: inside the box and not already a chest, write a chest (facing chosen by
/// `reorient`) and report whether it was written. The loot table draws `random.nextLong()`;
/// that half stays with the caller.
pub fn create_chest(
    world: &mut FeatureWorld,
    chunk_bb: &[i32; 6],
    pos: (i32, i32, i32),
    block_state: Option<u32>,
) -> Option<u32> {
    if !PieceContext::inside(chunk_bb, pos) || world.get_block(pos.0, pos.1, pos.2) == blocks::CHEST
    {
        return None;
    }
    let state = block_state.unwrap_or_else(|| reorient(world, pos, blocks::CHEST));
    world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
    Some(state)
}

/// `StructurePiece.reorient(level, pos, chestState)`：按周围实心方块决定箱子朝向。
///
/// `StructurePiece.reorient(level, pos, chestState)`: pick the chest facing from the solid
/// neighbours.
fn reorient(world: &FeatureWorld, pos: (i32, i32, i32), chest: u32) -> u32 {
    // 官方第一段：遍历四个水平方向，遇到箱子原样返回；累计「唯一实心邻居」，两个以上就放弃。
    // Vanilla part one: walk the four horizontal directions; a chest neighbour returns the
    // state unchanged, and two or more solid neighbours abandon the search.
    let mut solid: Option<&'static str> = None;
    for facing in ["north", "east", "south", "west"] {
        let (dx, dz) = step(facing);
        let neighbour = world.get_block(pos.0 + dx, pos.1, pos.2 + dz);
        if neighbour == blocks::CHEST {
            return chest;
        }
        if is_solid_render(neighbour) {
            if solid.is_some() {
                solid = None;
                break;
            }
            solid = Some(facing);
        }
    }
    if let Some(facing) = solid {
        return with_facing(chest, opposite_of(facing));
    }
    // 官方第二段：从当前 FACING（默认 NORTH）开始，指向实心块就反向，再顺时针，再反向。
    // Vanilla part two: start from the current FACING (default NORTH); a solid block ahead
    // flips to the opposite, then clockwise, then the opposite again.
    let mut facing = "north";
    let (dx, dz) = step(facing);
    if is_solid_render(world.get_block(pos.0 + dx, pos.1, pos.2 + dz)) {
        facing = opposite_of(facing);
        let (dx, dz) = step(facing);
        if is_solid_render(world.get_block(pos.0 + dx, pos.1, pos.2 + dz)) {
            facing = clockwise_of(facing);
            let (dx, dz) = step(facing);
            if is_solid_render(world.get_block(pos.0 + dx, pos.1, pos.2 + dz)) {
                facing = opposite_of(facing);
            }
        }
    }
    with_facing(chest, facing)
}

fn step(facing: &str) -> (i32, i32) {
    match facing {
        "north" => (0, -1),
        "south" => (0, 1),
        "east" => (1, 0),
        _ => (-1, 0),
    }
}

fn opposite_of(facing: &str) -> &'static str {
    match facing {
        "north" => "south",
        "south" => "north",
        "east" => "west",
        _ => "east",
    }
}

fn clockwise_of(facing: &str) -> &'static str {
    match facing {
        "north" => "east",
        "east" => "south",
        "south" => "west",
        _ => "north",
    }
}

fn with_facing(chest: u32, facing: &str) -> u32 {
    BlockState(chest)
        .try_with_text("facing", facing)
        .map(|state| state.0)
        .unwrap_or(chest)
}

fn is_solid_render(state: u32) -> bool {
    state_facts(state) & mc_bamboo_data::facts::SOLID_RENDER != 0
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
fn _piece_context(_: PieceContext<'_, '_>) {}

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
