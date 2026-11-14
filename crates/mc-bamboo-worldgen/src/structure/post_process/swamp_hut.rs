//! 女巫小屋（`SwampHutPiece.postProcess`）的方块部分。
//!
//! 官方先 `updateAverageGroundHeight(level, chunkBB, 0)`（`MOTION_BLOCKING_NO_LEAVES` 平均高度），
//! 成功后按固定坐标摆地板、墙、柱子、楼梯和家具，最后在四角 `fillColumnDown` 补橡木原木地基。
//! 女巫和猫是实体，这里不生成（`postProcess` 的方块部分不含随机数：整个方法一次 `nextInt`
//! 都不取，所以逐方块对拍可以完全确定）。
//!
//! The block part of `SwampHutPiece.postProcess`. Vanilla first runs
//! `updateAverageGroundHeight(level, chunkBB, 0)` (the `MOTION_BLOCKING_NO_LEAVES` average
//! height), then lays out the floor, walls, posts, stairs and furniture at fixed
//! coordinates, and finally `fillColumnDown`s oak log footings at the four corners. The witch
//! and cat are entities and are not spawned here (this `postProcess` draws no randomness at
//! all, so the block comparison is fully deterministic).

use mc_bamboo_blocks::BlockState;

use crate::{
    feature::world::FeatureWorld,
    structure::{place::PieceContext, scattered::average_ground_height},
};

/// 按属性名和文本设一个方块属性；失败说明数据里没有这个属性（版本变了），直接报错。
///
/// Set a block property by name and text; a failure means the property is gone from the data
/// (version change), which is reported rather than silently ignored.
fn with_text(state: u32, property: &str, value: &str) -> Result<u32, String> {
    BlockState(state)
        .try_with_text(property, value)
        .map(|state| state.0)
        .ok_or_else(|| {
            format!(
                "{} has no {property}={value}",
                BlockState(state).block().name
            )
        })
}

/// `SwampHutPiece.postProcess` 的方块部分。
///
/// 返回 `Some((height_position, new_bb))`：高度校正成功时给出 `HPos` 和平移后的包围盒；
/// `None` 表示 `updateAverageGroundHeight` 失败（盒子里没有一列），一个方块也不写。
///
/// The block part of `SwampHutPiece.postProcess`. Returns `Some((height_position, new_bb))`
/// on a successful height adjustment (the `HPos` value and the translated box); `None` when
/// `updateAverageGroundHeight` fails (no column inside the box), in which case nothing is
/// written.
pub fn swamp_hut(
    world: &mut FeatureWorld<'_>,
    piece_bb: &[i32; 6],
    chunk_bb: &[i32; 6],
    orientation: Option<crate::structure::pieces::HDir>,
    rotation: i32,
    mirror: i32,
) -> Result<Option<(i32, [i32; 6])>, String> {
    let Some((height_position, new_bb)) = average_ground_height(world, piece_bb, chunk_bb, 0)
    else {
        return Ok(None);
    };
    let mut ctx = PieceContext {
        world,
        chunk_bb: *chunk_bb,
        piece_bb: new_bb,
        orientation,
        rotation,
        mirror,
    };

    let planks = mc_bamboo_data::blocks::SPRUCE_PLANKS;
    let logs = mc_bamboo_data::blocks::OAK_LOG;
    let fence = mc_bamboo_data::blocks::OAK_FENCE;
    // 八段纯木板的 `generateBox`（edge = fill，`skipAir = false`）。
    // Eight plank-only `generateBox` calls (edge = fill, `skipAir = false`).
    for ((x0, y0, z0), (x1, y1, z1)) in [
        ((1, 1, 1), (5, 1, 7)),
        ((1, 4, 2), (5, 4, 7)),
        ((2, 1, 0), (4, 1, 0)),
        ((2, 2, 2), (3, 3, 2)),
        ((1, 2, 3), (1, 3, 6)),
        ((5, 2, 3), (5, 3, 6)),
        ((2, 2, 7), (4, 3, 7)),
    ] {
        ctx.generate_box((x0, y0, z0), (x1, y1, z1), planks, planks, false);
    }
    // 四根柱子。
    // Four posts.
    for ((x0, y0, z0), (x1, y1, z1)) in [
        ((1, 0, 2), (1, 3, 2)),
        ((5, 0, 2), (5, 3, 2)),
        ((1, 0, 7), (1, 3, 7)),
        ((5, 0, 7), (5, 3, 7)),
    ] {
        ctx.generate_box((x0, y0, z0), (x1, y1, z1), logs, logs, false);
    }

    let air = mc_bamboo_data::blocks::AIR;
    for (state, (x, y, z)) in [
        (fence, (2, 3, 2)),
        (fence, (3, 3, 7)),
        (air, (1, 3, 4)),
        (air, (5, 3, 4)),
        (air, (5, 3, 5)),
        (mc_bamboo_data::blocks::POTTED_RED_MUSHROOM, (1, 3, 5)),
        (mc_bamboo_data::blocks::CRAFTING_TABLE, (3, 2, 6)),
        (mc_bamboo_data::blocks::CAULDRON, (4, 2, 6)),
        (fence, (1, 2, 1)),
        (fence, (5, 2, 1)),
    ] {
        ctx.place_block(state, x, y, z);
    }

    // 楼梯：先按朝向取默认状态，再单独放四个外角。
    // Stairs: take the facing-specific default state, then place the four outer corners.
    let stairs = |facing: &str| -> Result<u32, String> {
        with_text(mc_bamboo_data::blocks::SPRUCE_STAIRS, "facing", facing)
    };
    let north = stairs("north")?;
    let east = stairs("east")?;
    let west = stairs("west")?;
    let south = stairs("south")?;
    for (edge, (x0, y0, z0), (x1, y1, z1)) in [
        (north, (0, 4, 1), (6, 4, 1)),
        (east, (0, 4, 2), (0, 4, 7)),
        (west, (6, 4, 2), (6, 4, 7)),
        (south, (0, 4, 8), (6, 4, 8)),
    ] {
        ctx.generate_box((x0, y0, z0), (x1, y1, z1), edge, edge, false);
    }
    for (state, (x, y, z)) in [
        (with_text(north, "shape", "outer_right")?, (0, 4, 1)),
        (with_text(north, "shape", "outer_left")?, (6, 4, 1)),
        (with_text(south, "shape", "outer_left")?, (0, 4, 8)),
        (with_text(south, "shape", "outer_right")?, (6, 4, 8)),
    ] {
        ctx.place_block(state, x, y, z);
    }

    for z in [2, 7] {
        for x in [1, 5] {
            ctx.fill_column_down(logs, x, -1, z);
        }
    }
    Ok(Some((height_position, new_bb)))
}
