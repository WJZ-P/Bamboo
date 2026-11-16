//! 沙漠神殿（`DesertPyramidPiece.postProcess`）的方块部分。
//!
//! 官方先 `updateHeightPositionToLowestGroundHeight(level, -random.nextInt(3))`（整块范围内
//! `MOTION_BLOCKING_NO_LEAVES` 的最低高度），成功后按固定坐标摆塔身、走廊、地下室，
//! 最后在四个水平方向各放一个箱子、再 `addCellar` 造地下墓室。
//!
//! 与随机无关的绘制指令按官方源码顺序写进一张表（[`Op`]），逐条执行；顺序和坐标照抄源码。
//! 三处随机源：`random.nextInt(3)`（高度偏移）、`level.getRandom()` 的 `nextBoolean` /
//! `nextFloat`（地下室楼梯与塌陷屋顶），以及 `createChest` 的 `nextLong`（战利品表，
//! 本模块不写，只写箱子方块）。
//!
//! The block part of `DesertPyramidPiece.postProcess`. Vanilla first runs
//! `updateHeightPositionToLowestGroundHeight(level, -random.nextInt(3))` (the lowest
//! `MOTION_BLOCKING_NO_LEAVES` height over the whole box), then lays out the pyramid,
//! corridors and cellar, places one chest per horizontal direction, and runs `addCellar`.
//! The randomness-free paint calls go into a table ([`Op`]) in vanilla source order and are
//! executed in turn. Three random sources are involved: `random.nextInt(3)` (height offset),
//! `level.getRandom()`'s `nextBoolean` / `nextFloat` (cellar stairs and collapsed roof), and
//! `createChest`'s `nextLong` (loot table, not written here; only the chest block is placed).

use mc_bamboo_blocks::BlockState;

use crate::{
    feature::world::FeatureWorld,
    random::Random,
    structure::{place::PieceContext, post_process::create_chest, scattered::lowest_ground_height},
};

/// `DesertPyramidPiece.WIDTH` / `DEPTH`。
///
/// `DesertPyramidPiece.WIDTH` / `DEPTH`.
const SIDE: i32 = 21;

/// 一条绘制指令。
///
/// One paint instruction.
enum Op {
    /// `generateBox(from, to, edge, fill, skipAir)`。
    ///
    /// `generateBox(from, to, edge, fill, skipAir)`.
    Fill {
        from: (i32, i32, i32),
        to: (i32, i32, i32),
        edge: u32,
        fill: u32,
        skip_air: bool,
    },
    /// `placeBlock(state, x, y, z)`。
    ///
    /// `placeBlock(state, x, y, z)`.
    One { state: u32, at: (i32, i32, i32) },
    /// `fillColumnDown(state, x, startY, z)`。
    ///
    /// `fillColumnDown(state, x, startY, z)`.
    ColumnDown { state: u32, at: (i32, i32, i32) },
}

/// `generateBox(..., edge, fill, skipAir)`，追加到指令表。
///
/// `generateBox(..., edge, fill, skipAir)`, appended to the instruction table.
fn fill(
    ops: &mut Vec<Op>,
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    edge: u32,
    fill_state: u32,
    skip_air: bool,
) {
    ops.push(Op::Fill {
        from,
        to,
        edge,
        fill: fill_state,
        skip_air,
    });
}

/// `placeBlock(state, x, y, z)`，追加到指令表。
///
/// `placeBlock(state, x, y, z)`, appended to the instruction table.
fn one(ops: &mut Vec<Op>, state: u32, at: (i32, i32, i32)) {
    ops.push(Op::One { state, at });
}

impl Op {
    fn run(&self, ctx: &mut PieceContext<'_, '_>) {
        match *self {
            Self::Fill {
                from,
                to,
                edge,
                fill,
                skip_air,
            } => ctx.generate_box(from, to, edge, fill, skip_air),
            Self::One { state, at } => ctx.place_block(state, at.0, at.1, at.2),
            Self::ColumnDown { state, at } => ctx.fill_column_down(state, at.0, at.1, at.2),
        }
    }
}

/// `WorldGenRegion` 的 `getRandom()`：`randomState.getOrCreateRandomFactory(
/// "minecraft:worldgen_region_random")` 在区块中心 `getWorldPosition()` 上的随机源。
///
/// 官方链路：`RandomState.random = Algorithm.newInstance(seed).forkPositional()`；
/// `getOrCreateRandomFactory(name) = random.fromHashOf(name).forkPositional()`；
/// `WorldGenRegion` 再 `.at(center.getWorldPosition())`。`use_legacy_random` 与 `RandomState`
/// 一致（主世界 `legacy_random_source` 为 false，即 `Xoroshiro`）。
///
/// Vanilla chain: `RandomState.random = Algorithm.newInstance(seed).forkPositional()`;
/// `getOrCreateRandomFactory(name) = random.fromHashOf(name).forkPositional()`; the
/// `WorldGenRegion` then calls `.at(center.getWorldPosition())`. `use_legacy_random` matches
/// `RandomState` (the overworld's `legacy_random_source` is false, i.e. `Xoroshiro`).
pub fn region_random(
    world_seed: i64,
    use_legacy_random: bool,
    center_x: i32,
    center_z: i32,
) -> Random {
    let algorithm = if use_legacy_random {
        crate::random::Algorithm::Legacy
    } else {
        crate::random::Algorithm::Xoroshiro
    };
    algorithm
        .new_instance(world_seed)
        .fork_positional()
        .from_hash_of("minecraft:worldgen_region_random")
        .fork_positional()
        .at(center_x, 0, center_z)
}

/// 按属性名和文本设置方块状态；属性不存在时 panic（数据版本变更会让它立刻暴露）。
///
/// Set a block state property by name and text; panics when the property is gone so a data
/// version change surfaces immediately.
fn facing(block: u32, value: &str) -> u32 {
    BlockState(block)
        .try_with_text("facing", value)
        .map(|state| state.0)
        .unwrap_or_else(|| panic!("{} has no facing={value}", BlockState(block).block().name))
}

/// `postProcess` 里与随机无关的绘制指令，顺序照官方源码。
///
/// The randomness-free paint instructions of `postProcess`, in vanilla source order.
fn ops() -> Vec<Op> {
    use mc_bamboo_data::blocks;
    let sandstone = blocks::SANDSTONE;
    let cut = blocks::CUT_SANDSTONE;
    let chiseled = blocks::CHISELED_SANDSTONE;
    let air = blocks::AIR;
    let orange = blocks::ORANGE_TERRACOTTA;
    let blue = blocks::BLUE_TERRACOTTA;
    let north = facing(blocks::SANDSTONE_STAIRS, "north");
    let south = facing(blocks::SANDSTONE_STAIRS, "south");
    let east = facing(blocks::SANDSTONE_STAIRS, "east");
    let west = facing(blocks::SANDSTONE_STAIRS, "west");

    let mut ops = Vec::new();

    // 塔身基座与内部空心层。
    // Pyramid base and hollow interior layers.
    fill(
        &mut ops,
        (0, -4, 0),
        (SIDE - 1, 0, SIDE - 1),
        sandstone,
        sandstone,
        false,
    );
    for pos in 1..=9 {
        fill(
            &mut ops,
            (pos, pos, pos),
            (SIDE - 1 - pos, pos, SIDE - 1 - pos),
            sandstone,
            sandstone,
            false,
        );
        fill(
            &mut ops,
            (pos + 1, pos, pos + 1),
            (SIDE - 2 - pos, pos, SIDE - 2 - pos),
            air,
            air,
            false,
        );
    }

    // `fillColumnDown` 的 21×21 列：官方在这段（塔身之后、正面入口之前）对每列调一次。
    // The 21x21 `fillColumnDown` columns: vanilla runs this between the pyramid body and the
    // front entrance, once per column.
    for x in 0..SIDE {
        for z in 0..SIDE {
            ops.push(Op::ColumnDown {
                state: sandstone,
                at: (x, -5, z),
            });
        }
    }

    // 正面入口。
    // Front entrance.
    fill(&mut ops, (0, 0, 0), (4, 9, 4), sandstone, air, false);
    fill(
        &mut ops,
        (1, 10, 1),
        (3, 10, 3),
        sandstone,
        sandstone,
        false,
    );
    one(&mut ops, north, (2, 10, 0));
    one(&mut ops, south, (2, 10, 4));
    one(&mut ops, east, (0, 10, 2));
    one(&mut ops, west, (4, 10, 2));
    fill(
        &mut ops,
        (SIDE - 5, 0, 0),
        (SIDE - 1, 9, 4),
        sandstone,
        air,
        false,
    );
    fill(
        &mut ops,
        (SIDE - 4, 10, 1),
        (SIDE - 2, 10, 3),
        sandstone,
        sandstone,
        false,
    );
    one(&mut ops, north, (SIDE - 3, 10, 0));
    one(&mut ops, south, (SIDE - 3, 10, 4));
    one(&mut ops, east, (SIDE - 5, 10, 2));
    one(&mut ops, west, (SIDE - 1, 10, 2));

    // 入口内部与两侧走廊。
    // Entrance interior and side corridors.
    fill(&mut ops, (8, 0, 0), (12, 4, 4), sandstone, air, false);
    fill(&mut ops, (9, 1, 0), (11, 3, 4), air, air, false);
    for (x, y) in [(9, 1), (9, 2), (9, 3), (10, 3), (11, 3), (11, 2), (11, 1)] {
        one(&mut ops, cut, (x, y, 1));
    }
    fill(&mut ops, (4, 1, 1), (8, 3, 3), sandstone, air, false);
    fill(&mut ops, (4, 1, 2), (8, 2, 2), air, air, false);
    fill(&mut ops, (12, 1, 1), (16, 3, 3), sandstone, air, false);
    fill(&mut ops, (12, 1, 2), (16, 2, 2), air, air, false);
    fill(
        &mut ops,
        (5, 4, 5),
        (SIDE - 6, 4, SIDE - 6),
        sandstone,
        sandstone,
        false,
    );
    fill(&mut ops, (9, 4, 9), (11, 4, 11), air, air, false);
    for (x, z) in [(8, 8), (12, 8), (8, 12), (12, 12)] {
        fill(&mut ops, (x, 1, z), (x, 3, z), cut, cut, false);
    }
    fill(&mut ops, (1, 1, 5), (4, 4, 11), sandstone, sandstone, false);
    fill(
        &mut ops,
        (SIDE - 5, 1, 5),
        (SIDE - 2, 4, 11),
        sandstone,
        sandstone,
        false,
    );
    fill(&mut ops, (6, 7, 9), (6, 7, 11), sandstone, sandstone, false);
    fill(
        &mut ops,
        (SIDE - 7, 7, 9),
        (SIDE - 7, 7, 11),
        sandstone,
        sandstone,
        false,
    );
    fill(&mut ops, (5, 5, 9), (5, 7, 11), cut, cut, false);
    fill(
        &mut ops,
        (SIDE - 6, 5, 9),
        (SIDE - 6, 7, 11),
        cut,
        cut,
        false,
    );
    for at in [
        (5, 5, 10),
        (5, 6, 10),
        (6, 6, 10),
        (SIDE - 6, 5, 10),
        (SIDE - 6, 6, 10),
        (SIDE - 7, 6, 10),
    ] {
        one(&mut ops, air, at);
    }
    fill(&mut ops, (2, 4, 4), (2, 6, 4), air, air, false);
    fill(
        &mut ops,
        (SIDE - 3, 4, 4),
        (SIDE - 3, 6, 4),
        air,
        air,
        false,
    );
    for at in [(2, 4, 5), (2, 3, 4), (SIDE - 3, 4, 5), (SIDE - 3, 3, 4)] {
        one(&mut ops, north, at);
    }
    fill(&mut ops, (1, 1, 3), (2, 2, 3), sandstone, sandstone, false);
    fill(
        &mut ops,
        (SIDE - 3, 1, 3),
        (SIDE - 2, 2, 3),
        sandstone,
        sandstone,
        false,
    );
    one(&mut ops, sandstone, (1, 1, 2));
    one(&mut ops, sandstone, (SIDE - 2, 1, 2));
    one(&mut ops, blocks::SANDSTONE_SLAB, (1, 2, 2));
    one(&mut ops, blocks::SANDSTONE_SLAB, (SIDE - 2, 2, 2));
    one(&mut ops, west, (2, 1, 2));
    one(&mut ops, east, (SIDE - 3, 1, 2));

    // 通往内室的走廊。
    // Corridor to the inner chamber.
    fill(&mut ops, (4, 3, 5), (4, 3, 17), sandstone, sandstone, false);
    fill(
        &mut ops,
        (SIDE - 5, 3, 5),
        (SIDE - 5, 3, 17),
        sandstone,
        sandstone,
        false,
    );
    fill(&mut ops, (3, 1, 5), (4, 2, 16), air, air, false);
    fill(
        &mut ops,
        (SIDE - 6, 1, 5),
        (SIDE - 5, 2, 16),
        air,
        air,
        false,
    );
    let mut z = 5;
    while z <= 17 {
        one(&mut ops, cut, (4, 1, z));
        one(&mut ops, chiseled, (4, 2, z));
        one(&mut ops, cut, (SIDE - 5, 1, z));
        one(&mut ops, chiseled, (SIDE - 5, 2, z));
        z += 2;
    }

    // 中央陶瓦图案。
    // Central terracotta motif.
    for (x, z) in [
        (10, 7),
        (10, 8),
        (9, 9),
        (11, 9),
        (8, 10),
        (12, 10),
        (7, 10),
        (13, 10),
        (9, 11),
        (11, 11),
        (10, 12),
        (10, 13),
    ] {
        one(&mut ops, orange, (x, 0, z));
    }
    one(&mut ops, blue, (10, 0, 10));

    // 两侧的竖向装饰带。
    // Vertical decorative bands on both sides.
    for x in [0, SIDE - 1] {
        for (y, z, block) in [
            (2, 1, cut),
            (2, 2, orange),
            (2, 3, cut),
            (3, 1, cut),
            (3, 2, orange),
            (3, 3, cut),
            (4, 1, orange),
            (4, 2, chiseled),
            (4, 3, orange),
            (5, 1, cut),
            (5, 2, orange),
            (5, 3, cut),
            (6, 1, orange),
            (6, 2, chiseled),
            (6, 3, orange),
            (7, 1, orange),
            (7, 2, orange),
            (7, 3, orange),
            (8, 1, cut),
            (8, 2, cut),
            (8, 3, cut),
        ] {
            one(&mut ops, block, (x, y, z));
        }
    }

    // 正面的两段装饰带：`x` 从 2 起、步长 `SIDE - 3 - 2`。
    // Two decorative bands on the front face: `x` starts at 2 with step `SIDE - 3 - 2`.
    let mut x = 2;
    while x <= SIDE - 3 {
        for (y, dx, block) in [
            (2, -1, cut),
            (2, 0, orange),
            (2, 1, cut),
            (3, -1, cut),
            (3, 0, orange),
            (3, 1, cut),
            (4, -1, orange),
            (4, 0, chiseled),
            (4, 1, orange),
            (5, -1, cut),
            (5, 0, orange),
            (5, 1, cut),
            (6, -1, orange),
            (6, 0, chiseled),
            (6, 1, orange),
            (7, -1, orange),
            (7, 0, orange),
            (7, 1, orange),
            (8, -1, cut),
            (8, 0, cut),
            (8, 1, cut),
        ] {
            one(&mut ops, block, (x + dx, y, 0));
        }
        x += SIDE - 3 - 2;
    }

    fill(&mut ops, (8, 4, 0), (12, 6, 0), cut, cut, false);
    one(&mut ops, air, (8, 6, 0));
    one(&mut ops, air, (12, 6, 0));
    one(&mut ops, orange, (9, 5, 0));
    one(&mut ops, chiseled, (10, 5, 0));
    one(&mut ops, orange, (11, 5, 0));

    // 地下墓室的外壳、宝藏室与 TNT 陷阱。
    // Cellar shell, treasure room and TNT trap.
    fill(&mut ops, (8, -14, 8), (12, -11, 12), cut, cut, false);
    fill(
        &mut ops,
        (8, -10, 8),
        (12, -10, 12),
        chiseled,
        chiseled,
        false,
    );
    fill(&mut ops, (8, -9, 8), (12, -9, 12), cut, cut, false);
    fill(
        &mut ops,
        (8, -8, 8),
        (12, -1, 12),
        sandstone,
        sandstone,
        false,
    );
    fill(&mut ops, (9, -11, 9), (11, -1, 11), air, air, false);
    one(&mut ops, blocks::STONE_PRESSURE_PLATE, (10, -11, 10));
    fill(
        &mut ops,
        (9, -13, 9),
        (11, -13, 11),
        blocks::TNT,
        air,
        false,
    );
    for at in [
        (8, -11, 10),
        (8, -10, 10),
        (12, -11, 10),
        (12, -10, 10),
        (10, -11, 8),
        (10, -10, 8),
        (10, -11, 12),
        (10, -10, 12),
    ] {
        one(&mut ops, air, at);
    }
    for (at, block) in [
        ((7, -10, 10), chiseled),
        ((7, -11, 10), cut),
        ((13, -10, 10), chiseled),
        ((13, -11, 10), cut),
        ((10, -10, 7), chiseled),
        ((10, -11, 7), cut),
        ((10, -10, 13), chiseled),
        ((10, -11, 13), cut),
    ] {
        one(&mut ops, block, at);
    }

    ops
}

/// `DesertPyramidPiece.postProcess` 的方块部分。
///
/// 返回 `Some((height_position, new_bb))`：高度校正成功时给出 `HPos` 和平移后的包围盒。
/// `seed` / `cx` / `cz` 只用于复算 `level.getRandom()`（地下室楼梯与塌陷屋顶）。
///
/// The block part of `DesertPyramidPiece.postProcess`. Returns
/// `Some((height_position, new_bb))` on a successful height adjustment. `seed` / `cx` / `cz`
/// are only used to rebuild `level.getRandom()` (cellar stairs and collapsed roof).
#[allow(clippy::too_many_arguments)]
pub fn desert_pyramid(
    world: &mut FeatureWorld<'_>,
    piece_bb: &[i32; 6],
    chunk_bb: &[i32; 6],
    orientation: Option<crate::structure::pieces::HDir>,
    rotation: i32,
    mirror: i32,
    seed: i64,
    use_legacy_random: bool,
    cx: i32,
    cz: i32,
    random: &crate::random::WorldgenRandom,
    writes: crate::structure::place::WriteLog,
) -> crate::structure::place::Placed {
    // `updateHeightPositionToLowestGroundHeight(level, -random.nextInt(3))`。
    let offset = -random.next_int(3);
    let (height_position, new_bb) = lowest_ground_height(world, piece_bb, offset);
    let chunk = *chunk_bb;
    let mut ctx = PieceContext {
        world,
        chunk_bb: chunk,
        piece_bb: new_bb,
        orientation,
        rotation,
        mirror,
        writes,
    };

    for op in ops() {
        op.run(&mut ctx);
    }

    // 四个箱子：`Direction.Plane.HORIZONTAL` 顺序（NORTH、EAST、SOUTH、WEST）。
    // Four chests in `Direction.Plane.HORIZONTAL` order (NORTH, EAST, SOUTH, WEST).
    for (dx, dz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
        let at = ctx.world_pos(10 + dx * 2, -11, 10 + dz * 2);
        if let Some(state) = create_chest(
            ctx.world,
            &chunk,
            at,
            None,
            random,
            Some("minecraft:chests/desert_pyramid"),
        ) && let Some(writes) = ctx.writes.as_mut()
        {
            writes.push((at.0, at.1, at.2, state));
        }
    }

    // `ChunkPos.getWorldPosition()` 是区块最小角 `(cx*16, 0, cz*16)`，不是中心。
    // `ChunkPos.getWorldPosition()` is the chunk's min corner, not its centre.
    let mut region = region_random(seed, use_legacy_random, cx * 16, cz * 16);
    add_cellar(&mut ctx, &mut region);
    let writes = ctx.writes.take();
    (Some((height_position, new_bb)), writes)
}

/// `DesertPyramidPiece.addCellar`：楼梯 + 墓室，`roomCenter = (16, -4, 13)`。
///
/// `DesertPyramidPiece.addCellar`: stairs plus the burial chamber with
/// `roomCenter = (16, -4, 13)`.
fn add_cellar(ctx: &mut PieceContext<'_, '_>, region: &mut Random) {
    add_cellar_stairs(ctx, region);
    add_cellar_room(ctx, region);
}

/// `DesertPyramidPiece.addCellarStairs`。
///
/// `DesertPyramidPiece.addCellarStairs`.
fn add_cellar_stairs(ctx: &mut PieceContext<'_, '_>, region: &mut Random) {
    let (x, y, z) = (16, -4, 13);
    // `sandStoneStairs.rotate(Rotation.COUNTERCLOCKWISE_90)`。
    // `sandStoneStairs.rotate(Rotation.COUNTERCLOCKWISE_90)`.
    let rotated = mc_bamboo_data::state_rotation(mc_bamboo_data::blocks::SANDSTONE_STAIRS, 3);
    ctx.place_block(rotated, 13, -1, 17);
    ctx.place_block(rotated, 14, -2, 17);
    ctx.place_block(rotated, 15, -3, 17);
    let sand = mc_bamboo_data::blocks::SAND;
    let sandstone = mc_bamboo_data::blocks::SANDSTONE;
    // `level.getRandom().nextBoolean()`。
    // `level.getRandom().nextBoolean()`.
    let variant = region.next_boolean();
    for dx in -4..=0 {
        ctx.place_block(sand, x + dx, y + 4, z + 4);
    }
    ctx.place_block(sand, x - 2, y + 3, z + 4);
    ctx.place_block(if variant { sand } else { sandstone }, x - 1, y + 3, z + 4);
    ctx.place_block(if variant { sandstone } else { sand }, x, y + 3, z + 4);
    ctx.place_block(sand, x - 1, y + 2, z + 4);
    ctx.place_block(sandstone, x, y + 2, z + 4);
    ctx.place_block(sand, x, y + 1, z + 4);
}

/// `DesertPyramidPiece.addCellarRoom`。
///
/// `DesertPyramidPiece.addCellarRoom`.
fn add_cellar_room(ctx: &mut PieceContext<'_, '_>, region: &mut Random) {
    let (x, y, z) = (16, -4, 13);
    let cut = mc_bamboo_data::blocks::CUT_SANDSTONE;
    let chiseled = mc_bamboo_data::blocks::CHISELED_SANDSTONE;
    let orange = mc_bamboo_data::blocks::ORANGE_TERRACOTTA;

    // 三层墙，全部 `skipAir = true`。
    // Three wall layers, all with `skipAir = true`.
    for (from, to, block) in [
        ((x - 3, y + 1, z - 3), (x - 3, y + 1, z + 2), cut),
        ((x + 3, y + 1, z - 3), (x + 3, y + 1, z + 2), cut),
        ((x - 3, y + 1, z - 3), (x + 3, y + 1, z - 2), cut),
        ((x - 3, y + 1, z + 3), (x + 3, y + 1, z + 3), cut),
        ((x - 3, y + 2, z - 3), (x - 3, y + 2, z + 2), chiseled),
        ((x + 3, y + 2, z - 3), (x + 3, y + 2, z + 2), chiseled),
        ((x - 3, y + 2, z - 3), (x + 3, y + 2, z - 2), chiseled),
        ((x - 3, y + 2, z + 3), (x + 3, y + 2, z + 3), chiseled),
        ((x - 3, -1, z - 3), (x - 3, -1, z + 2), cut),
        ((x + 3, -1, z - 3), (x + 3, -1, z + 2), cut),
        ((x - 3, -1, z - 3), (x + 3, -1, z - 2), cut),
        ((x - 3, -1, z + 3), (x + 3, -1, z + 3), cut),
    ] {
        ctx.generate_box(from, to, block, block, true);
    }

    // `placeSandBox` 只记可疑沙子位置，不写方块；`placeCollapsedRoof` 写屋顶。
    // `placeSandBox` only records suspicious-sand positions; `placeCollapsedRoof` writes.
    add_collapsed_roof(ctx, region, (x - 2, y + 4, z - 2), (x + 2, z + 2));

    ctx.place_block(mc_bamboo_data::blocks::BLUE_TERRACOTTA, x, y, z);
    for (dx, dz) in [
        (1, -1),
        (1, 1),
        (-1, -1),
        (-1, 1),
        (2, 0),
        (-2, 0),
        (0, 2),
        (0, -2),
    ] {
        ctx.place_block(orange, x + dx, y, z + dz);
    }
    // 东侧出口：`placeSand` 不写方块，只写外层的切制 / 雕纹砂岩。
    // East exit: `placeSand` writes nothing, only the outer cut / chiseled sandstone is placed.
    ctx.place_block(orange, x + 3, y, z);
    ctx.place_block(cut, x + 4, y + 1, z);
    ctx.place_block(chiseled, x + 4, y + 2, z);
    // 西侧出口。
    // West exit.
    ctx.place_block(orange, x - 3, y, z);
    ctx.place_block(cut, x - 4, y + 1, z);
    ctx.place_block(chiseled, x - 4, y + 2, z);
    // 南北两侧：只有橙陶瓦，`placeSand` 不写方块。
    // North and south: only the orange terracotta; `placeSand` writes nothing.
    ctx.place_block(orange, x, y, z + 3);
    ctx.place_block(orange, x, y, z - 3);
    ctx.place_block(cut, x, y + 1, z - 4);
    ctx.place_block(chiseled, x, -2, z - 4);
}

/// `DesertPyramidPiece.placeCollapsedRoof`：`nextFloat() < 0.33` 决定砂岩还是沙子。
///
/// 官方还会用 `level.getSeed()` 的位置派生随机源算 `randomCollapsedRoofPos`；那个结果只写进
/// 字段，不影响方块，所以这里不算。
///
/// `DesertPyramidPiece.placeCollapsedRoof`: `nextFloat() < 0.33` picks sandstone or sand.
/// Vanilla also derives `randomCollapsedRoofPos` from `level.getSeed()`; that value only feeds
/// a field and no blocks, so it is not reproduced here.
fn add_collapsed_roof(
    ctx: &mut PieceContext<'_, '_>,
    region: &mut Random,
    from: (i32, i32, i32),
    to: (i32, i32),
) {
    let (x0, y0, z0) = from;
    let (x1, z1) = to;
    for x in x0..=x1 {
        for z in z0..=z1 {
            let state = if region.next_float() < 0.33 {
                mc_bamboo_data::blocks::SANDSTONE
            } else {
                mc_bamboo_data::blocks::SAND
            };
            ctx.place_block(state, x, y0, z);
        }
    }
}
