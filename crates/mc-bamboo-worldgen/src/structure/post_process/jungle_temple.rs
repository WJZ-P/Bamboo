//! 丛林神殿（`JungleTemplePiece.postProcess`）的方块部分。
//!
//! 官方先 `updateAverageGroundHeight(level, chunkBB, 0)`，再按固定顺序摆墙体、地板、
//! 楼梯、两个陷阱的红石机关和两只箱子。所有 `generateBox(..., STONE_SELECTOR)` 走
//! `MossStoneSelector`：每格取一次 `postProcess` 随机源的 `nextFloat()`，
//! `< 0.4` 放圆石，否则放苔石；循环顺序是官方 `y → x → z`。
//!
//! 指令表由官方 `JungleTemplePiece.postProcess` 的 `generateBox` / `generateAirBox` /
//! `placeBlock` / `createChest` / `createDispenser` 调用序列逐条导出，见
//! `tools/mc-bamboo-vanilla/java/StructurePiecePlaceOracle.java` 的对拍。
//!
//! The block part of `JungleTemplePiece.postProcess`. Vanilla first runs
//! `updateAverageGroundHeight(level, chunkBB, 0)`, then lays out walls, floors, stairs,
//! the two redstone traps and the two chests. Every `generateBox(..., STONE_SELECTOR)`
//! uses `MossStoneSelector`: one `nextFloat()` per cell from the postProcess random,
//! cobblestone below `0.4` and mossy cobblestone otherwise, in vanilla's `y → x → z`
//! order. The instruction table is the official helper-call sequence.

use mc_bamboo_data::blocks;

use crate::{
    feature::world::FeatureWorld,
    random::WorldgenRandom,
    structure::{
        place::{PieceContext, WriteLog},
        post_process::create_chest,
        scattered::average_ground_height,
    },
};

/// 一条绘制指令。
///
/// One paint instruction.
enum Op {
    /// `generateBox(..., skipAir, random, STONE_SELECTOR)`。
    Moss {
        from: (i32, i32, i32),
        to: (i32, i32, i32),
        skip_air: bool,
    },
    /// `generateAirBox(from, to)`。
    Air {
        from: (i32, i32, i32),
        to: (i32, i32, i32),
    },
    /// `placeBlock(state, x, y, z)`。
    One { state: u32, at: (i32, i32, i32) },
    /// `createChest(level, chunkBB, random, x, y, z, table)`。
    Chest {
        at: (i32, i32, i32),
        table: &'static str,
    },
    /// `createDispenser(level, chunkBB, random, x, y, z, facing, table)`。
    Dispenser {
        at: (i32, i32, i32),
        facing: &'static str,
        table: &'static str,
    },
}

/// 按属性名/值列表设方块状态；属性缺失时报错（数据版本变更立刻暴露）。
///
/// Set a block state from property pairs; a missing property is an error.
fn state_with(state: u32, props: &[(&str, &str)]) -> u32 {
    let mut result = state;
    for &(name, value) in props {
        result = crate::feature::state::with_property(result, name, value)
            .unwrap_or_else(|| panic!("missing block property {name}={value}"));
    }
    result
}

/// `MossStoneSelector.next`：每格一次 `nextFloat()`。
///
/// `MossStoneSelector.next`: one `nextFloat()` per cell.
fn moss_selector(random: &WorldgenRandom) -> u32 {
    if random.next_float() < 0.4 {
        blocks::COBBLESTONE
    } else {
        blocks::MOSSY_COBBLESTONE
    }
}

/// 官方 `postProcess` 的全部绘制指令，顺序照调用序列。
///
/// Every paint instruction of vanilla `postProcess`, in call order.
fn ops() -> Vec<Op> {
    vec![
        Op::Moss {
            from: (0, -4, 0),
            to: (11, 0, 14),
            skip_air: false,
        },
        Op::Moss {
            from: (2, 1, 2),
            to: (9, 2, 2),
            skip_air: false,
        },
        Op::Moss {
            from: (2, 1, 12),
            to: (9, 2, 12),
            skip_air: false,
        },
        Op::Moss {
            from: (2, 1, 3),
            to: (2, 2, 11),
            skip_air: false,
        },
        Op::Moss {
            from: (9, 1, 3),
            to: (9, 2, 11),
            skip_air: false,
        },
        Op::Moss {
            from: (1, 3, 1),
            to: (10, 6, 1),
            skip_air: false,
        },
        Op::Moss {
            from: (1, 3, 13),
            to: (10, 6, 13),
            skip_air: false,
        },
        Op::Moss {
            from: (1, 3, 2),
            to: (1, 6, 12),
            skip_air: false,
        },
        Op::Moss {
            from: (10, 3, 2),
            to: (10, 6, 12),
            skip_air: false,
        },
        Op::Moss {
            from: (2, 3, 2),
            to: (9, 3, 12),
            skip_air: false,
        },
        Op::Moss {
            from: (2, 6, 2),
            to: (9, 6, 12),
            skip_air: false,
        },
        Op::Moss {
            from: (3, 7, 3),
            to: (8, 7, 11),
            skip_air: false,
        },
        Op::Moss {
            from: (4, 8, 4),
            to: (7, 8, 10),
            skip_air: false,
        },
        Op::Air {
            from: (3, 1, 3),
            to: (8, 2, 11),
        },
        Op::Air {
            from: (4, 3, 6),
            to: (7, 3, 9),
        },
        Op::Air {
            from: (2, 4, 2),
            to: (9, 5, 12),
        },
        Op::Air {
            from: (4, 6, 5),
            to: (7, 6, 9),
        },
        Op::Air {
            from: (5, 7, 6),
            to: (6, 7, 8),
        },
        Op::Air {
            from: (5, 1, 2),
            to: (6, 2, 2),
        },
        Op::Air {
            from: (5, 2, 12),
            to: (6, 2, 12),
        },
        Op::Air {
            from: (5, 5, 1),
            to: (6, 5, 1),
        },
        Op::Air {
            from: (5, 5, 13),
            to: (6, 5, 13),
        },
        Op::One {
            state: blocks::AIR,
            at: (1, 5, 5),
        },
        Op::One {
            state: blocks::AIR,
            at: (10, 5, 5),
        },
        Op::One {
            state: blocks::AIR,
            at: (1, 5, 9),
        },
        Op::One {
            state: blocks::AIR,
            at: (10, 5, 9),
        },
        Op::Moss {
            from: (2, 4, 0),
            to: (2, 5, 0),
            skip_air: false,
        },
        Op::Moss {
            from: (4, 4, 0),
            to: (4, 5, 0),
            skip_air: false,
        },
        Op::Moss {
            from: (7, 4, 0),
            to: (7, 5, 0),
            skip_air: false,
        },
        Op::Moss {
            from: (9, 4, 0),
            to: (9, 5, 0),
            skip_air: false,
        },
        Op::Moss {
            from: (2, 4, 14),
            to: (2, 5, 14),
            skip_air: false,
        },
        Op::Moss {
            from: (4, 4, 14),
            to: (4, 5, 14),
            skip_air: false,
        },
        Op::Moss {
            from: (7, 4, 14),
            to: (7, 5, 14),
            skip_air: false,
        },
        Op::Moss {
            from: (9, 4, 14),
            to: (9, 5, 14),
            skip_air: false,
        },
        Op::Moss {
            from: (5, 6, 0),
            to: (6, 6, 0),
            skip_air: false,
        },
        Op::Moss {
            from: (0, 4, 2),
            to: (0, 5, 2),
            skip_air: false,
        },
        Op::Moss {
            from: (0, 4, 4),
            to: (0, 5, 4),
            skip_air: false,
        },
        Op::Moss {
            from: (0, 4, 6),
            to: (0, 5, 6),
            skip_air: false,
        },
        Op::Moss {
            from: (0, 4, 8),
            to: (0, 5, 8),
            skip_air: false,
        },
        Op::Moss {
            from: (0, 4, 10),
            to: (0, 5, 10),
            skip_air: false,
        },
        Op::Moss {
            from: (0, 4, 12),
            to: (0, 5, 12),
            skip_air: false,
        },
        Op::Moss {
            from: (0, 6, 5),
            to: (0, 6, 5),
            skip_air: false,
        },
        Op::Moss {
            from: (0, 6, 9),
            to: (0, 6, 9),
            skip_air: false,
        },
        Op::Moss {
            from: (11, 4, 2),
            to: (11, 5, 2),
            skip_air: false,
        },
        Op::Moss {
            from: (11, 4, 4),
            to: (11, 5, 4),
            skip_air: false,
        },
        Op::Moss {
            from: (11, 4, 6),
            to: (11, 5, 6),
            skip_air: false,
        },
        Op::Moss {
            from: (11, 4, 8),
            to: (11, 5, 8),
            skip_air: false,
        },
        Op::Moss {
            from: (11, 4, 10),
            to: (11, 5, 10),
            skip_air: false,
        },
        Op::Moss {
            from: (11, 4, 12),
            to: (11, 5, 12),
            skip_air: false,
        },
        Op::Moss {
            from: (11, 6, 5),
            to: (11, 6, 5),
            skip_air: false,
        },
        Op::Moss {
            from: (11, 6, 9),
            to: (11, 6, 9),
            skip_air: false,
        },
        Op::Moss {
            from: (2, 7, 2),
            to: (2, 9, 2),
            skip_air: false,
        },
        Op::Moss {
            from: (9, 7, 2),
            to: (9, 9, 2),
            skip_air: false,
        },
        Op::Moss {
            from: (2, 7, 12),
            to: (2, 9, 12),
            skip_air: false,
        },
        Op::Moss {
            from: (9, 7, 12),
            to: (9, 9, 12),
            skip_air: false,
        },
        Op::Moss {
            from: (4, 9, 4),
            to: (4, 9, 4),
            skip_air: false,
        },
        Op::Moss {
            from: (7, 9, 4),
            to: (7, 9, 4),
            skip_air: false,
        },
        Op::Moss {
            from: (4, 9, 10),
            to: (4, 9, 10),
            skip_air: false,
        },
        Op::Moss {
            from: (7, 9, 10),
            to: (7, 9, 10),
            skip_air: false,
        },
        Op::Moss {
            from: (5, 9, 7),
            to: (6, 9, 7),
            skip_air: false,
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "north"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (5, 9, 6),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "north"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (6, 9, 6),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "south"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (5, 9, 8),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "south"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (6, 9, 8),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "north"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (4, 0, 0),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "north"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (5, 0, 0),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "north"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (6, 0, 0),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "north"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (7, 0, 0),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "north"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (4, 1, 8),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "north"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (4, 2, 9),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "north"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (4, 3, 10),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "north"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (7, 1, 8),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "north"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (7, 2, 9),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "north"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (7, 3, 10),
        },
        Op::Moss {
            from: (4, 1, 9),
            to: (4, 1, 9),
            skip_air: false,
        },
        Op::Moss {
            from: (7, 1, 9),
            to: (7, 1, 9),
            skip_air: false,
        },
        Op::Moss {
            from: (4, 1, 10),
            to: (7, 2, 10),
            skip_air: false,
        },
        Op::Moss {
            from: (5, 4, 5),
            to: (6, 4, 5),
            skip_air: false,
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "east"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (4, 4, 5),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "west"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (7, 4, 5),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "south"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (5, 0, 6),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "south"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (6, 0, 6),
        },
        Op::Air {
            from: (5, 0, 7),
            to: (6, 0, 9),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "south"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (5, -1, 7),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "south"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (6, -1, 7),
        },
        Op::Air {
            from: (5, -1, 8),
            to: (6, -1, 10),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "south"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (5, -2, 8),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "south"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (6, -2, 8),
        },
        Op::Air {
            from: (5, -2, 9),
            to: (6, -2, 11),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "south"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (5, -3, 9),
        },
        Op::One {
            state: state_with(
                blocks::COBBLESTONE_STAIRS,
                &[
                    ("facing", "south"),
                    ("half", "bottom"),
                    ("shape", "straight"),
                    ("waterlogged", "false"),
                ],
            ),
            at: (6, -3, 9),
        },
        Op::Air {
            from: (5, -3, 10),
            to: (6, -3, 12),
        },
        Op::Air {
            from: (1, -3, 12),
            to: (10, -1, 13),
        },
        Op::Air {
            from: (1, -3, 1),
            to: (3, -1, 13),
        },
        Op::Air {
            from: (1, -3, 1),
            to: (9, -1, 5),
        },
        Op::Moss {
            from: (1, -3, 1),
            to: (1, -2, 1),
            skip_air: false,
        },
        Op::Moss {
            from: (1, -3, 3),
            to: (1, -2, 3),
            skip_air: false,
        },
        Op::Moss {
            from: (1, -3, 5),
            to: (1, -2, 5),
            skip_air: false,
        },
        Op::Moss {
            from: (1, -3, 7),
            to: (1, -2, 7),
            skip_air: false,
        },
        Op::Moss {
            from: (1, -3, 9),
            to: (1, -2, 9),
            skip_air: false,
        },
        Op::Moss {
            from: (1, -3, 11),
            to: (1, -2, 11),
            skip_air: false,
        },
        Op::Moss {
            from: (1, -3, 13),
            to: (1, -2, 13),
            skip_air: false,
        },
        Op::Moss {
            from: (1, -1, 2),
            to: (3, -1, 2),
            skip_air: false,
        },
        Op::Moss {
            from: (1, -1, 4),
            to: (3, -1, 4),
            skip_air: false,
        },
        Op::Moss {
            from: (1, -1, 6),
            to: (3, -1, 6),
            skip_air: false,
        },
        Op::Moss {
            from: (1, -1, 8),
            to: (3, -1, 8),
            skip_air: false,
        },
        Op::Moss {
            from: (1, -1, 10),
            to: (3, -1, 10),
            skip_air: false,
        },
        Op::Moss {
            from: (1, -1, 12),
            to: (3, -1, 12),
            skip_air: false,
        },
        Op::Moss {
            from: (2, -2, 1),
            to: (5, -2, 1),
            skip_air: false,
        },
        Op::Moss {
            from: (7, -2, 1),
            to: (9, -2, 1),
            skip_air: false,
        },
        Op::Moss {
            from: (6, -3, 1),
            to: (6, -3, 1),
            skip_air: false,
        },
        Op::Moss {
            from: (6, -1, 1),
            to: (6, -1, 1),
            skip_air: false,
        },
        Op::One {
            state: state_with(
                blocks::TRIPWIRE_HOOK,
                &[
                    ("attached", "true"),
                    ("facing", "east"),
                    ("powered", "false"),
                ],
            ),
            at: (1, -3, 8),
        },
        Op::One {
            state: state_with(
                blocks::TRIPWIRE_HOOK,
                &[
                    ("attached", "true"),
                    ("facing", "west"),
                    ("powered", "false"),
                ],
            ),
            at: (4, -3, 8),
        },
        Op::One {
            state: state_with(
                blocks::TRIPWIRE,
                &[
                    ("attached", "true"),
                    ("disarmed", "false"),
                    ("east", "true"),
                    ("north", "false"),
                    ("powered", "false"),
                    ("south", "false"),
                    ("west", "true"),
                ],
            ),
            at: (2, -3, 8),
        },
        Op::One {
            state: state_with(
                blocks::TRIPWIRE,
                &[
                    ("attached", "true"),
                    ("disarmed", "false"),
                    ("east", "true"),
                    ("north", "false"),
                    ("powered", "false"),
                    ("south", "false"),
                    ("west", "true"),
                ],
            ),
            at: (3, -3, 8),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "none"),
                    ("north", "side"),
                    ("power", "0"),
                    ("south", "side"),
                    ("west", "none"),
                ],
            ),
            at: (5, -3, 7),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "none"),
                    ("north", "side"),
                    ("power", "0"),
                    ("south", "side"),
                    ("west", "none"),
                ],
            ),
            at: (5, -3, 6),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "none"),
                    ("north", "side"),
                    ("power", "0"),
                    ("south", "side"),
                    ("west", "none"),
                ],
            ),
            at: (5, -3, 5),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "none"),
                    ("north", "side"),
                    ("power", "0"),
                    ("south", "side"),
                    ("west", "none"),
                ],
            ),
            at: (5, -3, 4),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "none"),
                    ("north", "side"),
                    ("power", "0"),
                    ("south", "side"),
                    ("west", "none"),
                ],
            ),
            at: (5, -3, 3),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "none"),
                    ("north", "side"),
                    ("power", "0"),
                    ("south", "side"),
                    ("west", "none"),
                ],
            ),
            at: (5, -3, 2),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "none"),
                    ("north", "side"),
                    ("power", "0"),
                    ("south", "none"),
                    ("west", "side"),
                ],
            ),
            at: (5, -3, 1),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "side"),
                    ("north", "none"),
                    ("power", "0"),
                    ("south", "none"),
                    ("west", "side"),
                ],
            ),
            at: (4, -3, 1),
        },
        Op::One {
            state: blocks::MOSSY_COBBLESTONE,
            at: (3, -3, 1),
        },
        Op::Dispenser {
            at: (3, -2, 1),
            facing: "north",
            table: "minecraft:chests/jungle_temple_dispenser",
        },
        Op::One {
            state: state_with(
                blocks::DISPENSER,
                &[("facing", "north"), ("triggered", "false")],
            ),
            at: (3, -2, 1),
        },
        Op::One {
            state: state_with(
                blocks::VINE,
                &[
                    ("east", "false"),
                    ("north", "false"),
                    ("south", "true"),
                    ("up", "false"),
                    ("west", "false"),
                ],
            ),
            at: (3, -2, 2),
        },
        Op::One {
            state: state_with(
                blocks::TRIPWIRE_HOOK,
                &[
                    ("attached", "true"),
                    ("facing", "north"),
                    ("powered", "false"),
                ],
            ),
            at: (7, -3, 1),
        },
        Op::One {
            state: state_with(
                blocks::TRIPWIRE_HOOK,
                &[
                    ("attached", "true"),
                    ("facing", "south"),
                    ("powered", "false"),
                ],
            ),
            at: (7, -3, 5),
        },
        Op::One {
            state: state_with(
                blocks::TRIPWIRE,
                &[
                    ("attached", "true"),
                    ("disarmed", "false"),
                    ("east", "false"),
                    ("north", "true"),
                    ("powered", "false"),
                    ("south", "true"),
                    ("west", "false"),
                ],
            ),
            at: (7, -3, 2),
        },
        Op::One {
            state: state_with(
                blocks::TRIPWIRE,
                &[
                    ("attached", "true"),
                    ("disarmed", "false"),
                    ("east", "false"),
                    ("north", "true"),
                    ("powered", "false"),
                    ("south", "true"),
                    ("west", "false"),
                ],
            ),
            at: (7, -3, 3),
        },
        Op::One {
            state: state_with(
                blocks::TRIPWIRE,
                &[
                    ("attached", "true"),
                    ("disarmed", "false"),
                    ("east", "false"),
                    ("north", "true"),
                    ("powered", "false"),
                    ("south", "true"),
                    ("west", "false"),
                ],
            ),
            at: (7, -3, 4),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "side"),
                    ("north", "none"),
                    ("power", "0"),
                    ("south", "none"),
                    ("west", "side"),
                ],
            ),
            at: (8, -3, 6),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "none"),
                    ("north", "none"),
                    ("power", "0"),
                    ("south", "side"),
                    ("west", "side"),
                ],
            ),
            at: (9, -3, 6),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "none"),
                    ("north", "side"),
                    ("power", "0"),
                    ("south", "up"),
                    ("west", "none"),
                ],
            ),
            at: (9, -3, 5),
        },
        Op::One {
            state: blocks::MOSSY_COBBLESTONE,
            at: (9, -3, 4),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "none"),
                    ("north", "side"),
                    ("power", "0"),
                    ("south", "side"),
                    ("west", "none"),
                ],
            ),
            at: (9, -2, 4),
        },
        Op::Dispenser {
            at: (9, -2, 3),
            facing: "west",
            table: "minecraft:chests/jungle_temple_dispenser",
        },
        Op::One {
            state: state_with(
                blocks::DISPENSER,
                &[("facing", "west"), ("triggered", "false")],
            ),
            at: (9, -2, 3),
        },
        Op::One {
            state: state_with(
                blocks::VINE,
                &[
                    ("east", "true"),
                    ("north", "false"),
                    ("south", "false"),
                    ("up", "false"),
                    ("west", "false"),
                ],
            ),
            at: (8, -1, 3),
        },
        Op::One {
            state: state_with(
                blocks::VINE,
                &[
                    ("east", "true"),
                    ("north", "false"),
                    ("south", "false"),
                    ("up", "false"),
                    ("west", "false"),
                ],
            ),
            at: (8, -2, 3),
        },
        Op::Chest {
            at: (8, -3, 3),
            table: "minecraft:chests/jungle_temple",
        },
        Op::One {
            state: blocks::MOSSY_COBBLESTONE,
            at: (9, -3, 2),
        },
        Op::One {
            state: blocks::MOSSY_COBBLESTONE,
            at: (8, -3, 1),
        },
        Op::One {
            state: blocks::MOSSY_COBBLESTONE,
            at: (4, -3, 5),
        },
        Op::One {
            state: blocks::MOSSY_COBBLESTONE,
            at: (5, -2, 5),
        },
        Op::One {
            state: blocks::MOSSY_COBBLESTONE,
            at: (5, -1, 5),
        },
        Op::One {
            state: blocks::MOSSY_COBBLESTONE,
            at: (6, -3, 5),
        },
        Op::One {
            state: blocks::MOSSY_COBBLESTONE,
            at: (7, -2, 5),
        },
        Op::One {
            state: blocks::MOSSY_COBBLESTONE,
            at: (7, -1, 5),
        },
        Op::One {
            state: blocks::MOSSY_COBBLESTONE,
            at: (8, -3, 5),
        },
        Op::Moss {
            from: (9, -1, 1),
            to: (9, -1, 5),
            skip_air: false,
        },
        Op::Air {
            from: (8, -3, 8),
            to: (10, -1, 10),
        },
        Op::One {
            state: blocks::CHISELED_STONE_BRICKS,
            at: (8, -2, 11),
        },
        Op::One {
            state: blocks::CHISELED_STONE_BRICKS,
            at: (9, -2, 11),
        },
        Op::One {
            state: blocks::CHISELED_STONE_BRICKS,
            at: (10, -2, 11),
        },
        Op::One {
            state: state_with(
                blocks::LEVER,
                &[("face", "wall"), ("facing", "north"), ("powered", "false")],
            ),
            at: (8, -2, 12),
        },
        Op::One {
            state: state_with(
                blocks::LEVER,
                &[("face", "wall"), ("facing", "north"), ("powered", "false")],
            ),
            at: (9, -2, 12),
        },
        Op::One {
            state: state_with(
                blocks::LEVER,
                &[("face", "wall"), ("facing", "north"), ("powered", "false")],
            ),
            at: (10, -2, 12),
        },
        Op::Moss {
            from: (8, -3, 8),
            to: (8, -3, 10),
            skip_air: false,
        },
        Op::Moss {
            from: (10, -3, 8),
            to: (10, -3, 10),
            skip_air: false,
        },
        Op::One {
            state: blocks::MOSSY_COBBLESTONE,
            at: (10, -2, 9),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "none"),
                    ("north", "side"),
                    ("power", "0"),
                    ("south", "side"),
                    ("west", "none"),
                ],
            ),
            at: (8, -2, 9),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "none"),
                    ("north", "side"),
                    ("power", "0"),
                    ("south", "side"),
                    ("west", "none"),
                ],
            ),
            at: (8, -2, 10),
        },
        Op::One {
            state: state_with(
                blocks::REDSTONE_WIRE,
                &[
                    ("east", "side"),
                    ("north", "side"),
                    ("power", "0"),
                    ("south", "side"),
                    ("west", "side"),
                ],
            ),
            at: (10, -1, 9),
        },
        Op::One {
            state: state_with(
                blocks::STICKY_PISTON,
                &[("extended", "false"), ("facing", "up")],
            ),
            at: (9, -2, 8),
        },
        Op::One {
            state: state_with(
                blocks::STICKY_PISTON,
                &[("extended", "false"), ("facing", "west")],
            ),
            at: (10, -2, 8),
        },
        Op::One {
            state: state_with(
                blocks::STICKY_PISTON,
                &[("extended", "false"), ("facing", "west")],
            ),
            at: (10, -1, 8),
        },
        Op::One {
            state: state_with(
                blocks::REPEATER,
                &[
                    ("delay", "1"),
                    ("facing", "north"),
                    ("locked", "false"),
                    ("powered", "false"),
                ],
            ),
            at: (10, -2, 10),
        },
        Op::Chest {
            at: (9, -3, 10),
            table: "minecraft:chests/jungle_temple",
        },
    ]
}

/// `JungleTemplePiece.postProcess` 的方块部分。
///
/// `random` 是 `applyBiomeDecoration` 的 `setFeatureSeed` 随机源，供
/// `MossStoneSelector`、箱子和发射器的战利品表种子取数。
///
/// The block part of `JungleTemplePiece.postProcess`. `random` is
/// `applyBiomeDecoration`'s `setFeatureSeed` source, used by `MossStoneSelector` and
/// the chest/dispenser loot table seeds.
#[allow(clippy::too_many_arguments)]
pub fn jungle_temple(
    world: &mut FeatureWorld<'_>,
    piece_bb: &[i32; 6],
    chunk_bb: &[i32; 6],
    orientation: Option<crate::structure::pieces::HDir>,
    rotation: i32,
    mirror: i32,
    random: &WorldgenRandom,
    writes: WriteLog,
) -> crate::structure::place::Placed {
    let Some((height_position, new_bb)) = average_ground_height(world, piece_bb, chunk_bb, 0)
    else {
        return (None, writes);
    };
    let mut ctx = PieceContext {
        world,
        chunk_bb: *chunk_bb,
        piece_bb: new_bb,
        orientation,
        rotation,
        mirror,
        writes,
    };
    for op in ops() {
        match op {
            Op::Moss { from, to, skip_air } => {
                let (x0, y0, z0) = from;
                let (x1, y1, z1) = to;
                // 官方 `generateBox(..., skipAir, random, selector)` 的三重循环顺序。
                // Vanilla `generateBox(..., skipAir, random, selector)` loop order.
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        for z in z0..=z1 {
                            if skip_air && ctx.block(x, y, z) == blocks::AIR {
                                continue;
                            }
                            let state = moss_selector(random);
                            ctx.place_block(state, x, y, z);
                        }
                    }
                }
            }
            Op::Air { from, to } => ctx.generate_air_box(from, to),
            Op::One { state, at } => ctx.place_block(state, at.0, at.1, at.2),
            Op::Chest { at, table } => {
                let pos = ctx.world_pos(at.0, at.1, at.2);
                if let Some(state) =
                    create_chest(ctx.world, &ctx.chunk_bb, pos, None, random, Some(table))
                    && let Some(log) = ctx.writes.as_mut()
                {
                    log.push((pos.0, pos.1, pos.2, state));
                }
            }
            Op::Dispenser { at, facing, table } => {
                let pos = ctx.world_pos(at.0, at.1, at.2);
                let state = state_with(blocks::DISPENSER, &[("facing", facing)]);
                if let Some(state) = crate::structure::post_process::create_dispenser(
                    ctx.world,
                    &ctx.chunk_bb,
                    pos,
                    state,
                    random,
                    table,
                ) && let Some(log) = ctx.writes.as_mut()
                {
                    log.push((pos.0, pos.1, pos.2, state));
                }
            }
        }
    }
    (Some((height_position, new_bb)), ctx.writes.take())
}
