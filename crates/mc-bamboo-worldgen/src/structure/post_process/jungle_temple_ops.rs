// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

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
