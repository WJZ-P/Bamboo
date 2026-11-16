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
    structure::piece_place::{box_is_inside, reorient_with},
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

/// `StructurePiece.createChest(level, chunkBB, random, pos, lootTable, null)`：
/// 盒子内且不是箱子时写一个箱子（朝向用 `reorient`），再按 `random.nextLong()` 把战利品表
/// 写进方块实体。返回写下的方块状态。
///
/// 官方 `getBlockEntity` 拿到的是 `ChestBlockEntity` 才会取数；这里 `chest` 状态自带方块实体，
/// 所以一定取。`loot_table` 是 `null`（不指定表）时只取数、不写 `LootTable` 键。
///
/// `StructurePiece.createChest`: write a chest (facing chosen by `reorient`) when inside
/// the box and no chest is there yet, then draw `random.nextLong()` for the loot table.
/// Vanilla only draws when `getBlockEntity` returns a `ChestBlockEntity`; a chest state
/// always has one. A `None` loot table still draws the seed but writes no `LootTable` key.
pub fn create_chest(
    world: &mut FeatureWorld,
    chunk_bb: &[i32; 6],
    pos: (i32, i32, i32),
    block_state: Option<u32>,
    random: &crate::random::WorldgenRandom,
    loot_table: Option<&str>,
) -> Option<u32> {
    if !box_is_inside(chunk_bb, pos.0, pos.1, pos.2)
        || world.get_block(pos.0, pos.1, pos.2) == blocks::CHEST
    {
        return None;
    }
    let state = block_state
        .unwrap_or_else(|| reorient_with(|x, y, z| world.get_block(x, y, z), pos, blocks::CHEST));
    world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
    let mut entries = Vec::new();
    if let Some(table) = loot_table {
        entries.push((
            "LootTable".to_owned(),
            mc_bamboo_nbt::Tag::String(table.to_owned()),
        ));
    }
    let seed = random.next_long();
    entries.push(("LootTableSeed".to_owned(), mc_bamboo_nbt::Tag::Long(seed)));
    world.set_block_entity(pos.0, pos.1, pos.2, &mc_bamboo_nbt::Tag::Compound(entries));
    Some(state)
}

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
