//! `MineshaftStructure` / `MineshaftPieces` 的结构件布局（普通、恶地两种）。
//!
//! 只移植 `findGenerationPoint` → `generatePiecesAndAdjust` 的「结构件列表」生成：
//! 起始房间、递归 `addChildren`、`moveBelowSeaLevel` / 恶地的竖直平移。不含
//! `postProcess`。随机源取数顺序逐步照官方 26.3 翻译。
//!
//! Layout of `MineshaftStructure` / `MineshaftPieces` (normal and mesa): the piece
//! list from `findGenerationPoint` -> `generatePiecesAndAdjust` (start room,
//! recursive `addChildren`, `moveBelowSeaLevel` / mesa vertical shift). No
//! `postProcess`. The random-source draw order is translated step by step.

use crate::random::WorldgenRandom;

use super::{
    Box, Builder, HDir, Piece, block_at, center_y, make_random, new_box, translate, x_span, y_span,
    z_span,
};

/// 官方常量：海平面、最低 Y、`moveBelowSeaLevel` 的 offset。
const SEA_LEVEL: i32 = 63;
const MIN_Y: i32 = -64;
const BELOW_SEA_LEVEL_OFFSET: i32 = 10;
/// `MAX_DEPTH = 8`。
const MAX_DEPTH: i32 = 8;

const ID_ROOM: &str = "minecraft:msroom";
const ID_CORRIDOR: &str = "minecraft:mscorridor";
const ID_CROSSING: &str = "minecraft:mscrossing";
const ID_STAIRS: &str = "minecraft:msstairs";

/// 一个结构件的行为数据；`to_piece` 产出对拍用的 `Piece`。
#[derive(Debug, Clone)]
enum Shaft {
    Room {
        bb: Box,
    },
    Corridor {
        bb: Box,
        gd: i32,
        dir: HDir,
        has_rails: bool,
        spider: bool,
        num_sections: i32,
    },
    Crossing {
        bb: Box,
        gd: i32,
        dir: HDir,
        two_floored: bool,
    },
    Stairs {
        bb: Box,
        gd: i32,
        dir: HDir,
    },
}

impl Shaft {
    fn bb(&self) -> Box {
        match *self {
            Self::Room { bb }
            | Self::Corridor { bb, .. }
            | Self::Crossing { bb, .. }
            | Self::Stairs { bb, .. } => bb,
        }
    }

    /// `addAdditionalSaveData`：所有矿井件都写 `MST`（`type.ordinal()`），
    /// 各子类再写自己的字段。
    fn to_piece(&self, mesa: bool) -> Piece {
        let mst = i32::from(mesa);
        let mut piece = match *self {
            Self::Room { bb } => Piece::plain(ID_ROOM, bb, 0),
            Self::Corridor {
                bb,
                gd,
                dir,
                has_rails,
                spider,
                num_sections,
            } => {
                let mut p = Piece::oriented(ID_CORRIDOR, bb, gd, dir);
                p.put_bool("hr", has_rails);
                p.put_bool("sc", spider);
                p.put_bool("hps", false);
                p.put_int("Num", num_sections);
                p
            }
            Self::Crossing {
                bb,
                gd,
                dir,
                two_floored,
            } => {
                let mut p = Piece::plain(ID_CROSSING, bb, gd);
                p.put_bool("tf", two_floored);
                // 官方 `tag.store("D", Direction.LEGACY_ID_CODEC_2D, dir)` 写的是 Byte。
                // Vanilla writes `D` through `Direction.LEGACY_ID_CODEC_2D`, which is a Byte.
                p.put_byte("D", dir.to_2d());
                p
            }
            Self::Stairs { bb, gd, dir } => Piece::oriented(ID_STAIRS, bb, gd, dir),
        };
        piece.put_int("MST", mst);
        piece
    }
}

/// 递归生成期间共享的不变量：起始房间的 `minX/minZ`（距离检查用）和矿井类型。
struct Ctx<'a> {
    random: &'a WorldgenRandom,
    start_min_x: i32,
    start_min_z: i32,
    mesa: bool,
}

/// `findCorridorSize`：先 `nextInt(3)+2`，再从大到小试长度，遇到无碰撞就返回。
fn find_corridor_size(
    builder: &Builder,
    random: &WorldgenRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: HDir,
) -> Option<Box> {
    let mut corridor_length = random.next_int(3) + 2;
    while corridor_length > 0 {
        let block_length = corridor_length * 5;
        let mut b = match dir {
            HDir::North => new_box(0, 0, -(block_length - 1), 2, 2, 0),
            HDir::South => new_box(0, 0, 0, 2, 2, block_length - 1),
            HDir::West => new_box(-(block_length - 1), 0, 0, 0, 2, 2),
            HDir::East => new_box(0, 0, 0, block_length - 1, 2, 2),
        };
        translate(&mut b, foot_x, foot_y, foot_z);
        if !builder.collides(&b) {
            return Some(b);
        }
        corridor_length -= 1;
    }
    None
}

/// `findCrossing`：`nextInt(4)==0` 时两层，否则一层；再查碰撞。
fn find_crossing(
    builder: &Builder,
    random: &WorldgenRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: HDir,
) -> Option<Box> {
    let y1 = if random.next_int(4) == 0 { 6 } else { 2 };
    let mut b = match dir {
        HDir::North => new_box(-1, 0, -4, 3, y1, 0),
        HDir::South => new_box(-1, 0, 0, 3, y1, 4),
        HDir::West => new_box(-4, 0, -1, 0, y1, 3),
        HDir::East => new_box(0, 0, -1, 4, y1, 3),
    };
    translate(&mut b, foot_x, foot_y, foot_z);
    (!builder.collides(&b)).then_some(b)
}

/// `findStairs`：固定尺寸盒再查碰撞，无随机。
fn find_stairs(builder: &Builder, foot_x: i32, foot_y: i32, foot_z: i32, dir: HDir) -> Option<Box> {
    let mut b = match dir {
        HDir::North => new_box(0, -5, -8, 2, 2, 0),
        HDir::South => new_box(0, -5, 0, 2, 2, 8),
        HDir::West => new_box(-8, -5, 0, 0, 2, 2),
        HDir::East => new_box(0, -5, 0, 8, 2, 2),
    };
    translate(&mut b, foot_x, foot_y, foot_z);
    (!builder.collides(&b)).then_some(b)
}

/// `createRandomShaftPiece`：`nextInt(100)` 选类型，各分支的 `find*` 失败就返回 null
/// （不回退到别的分支）。
fn create_random_shaft_piece(
    builder: &Builder,
    ctx: &Ctx<'_>,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: HDir,
    gen_depth: i32,
) -> Option<Shaft> {
    let selection = ctx.random.next_int(100);
    if selection >= 80 {
        let bb = find_crossing(builder, ctx.random, foot_x, foot_y, foot_z, dir)?;
        Some(Shaft::Crossing {
            bb,
            gd: gen_depth,
            dir,
            two_floored: y_span(&bb) > 3,
        })
    } else if selection >= 70 {
        let bb = find_stairs(builder, foot_x, foot_y, foot_z, dir)?;
        Some(Shaft::Stairs {
            bb,
            gd: gen_depth,
            dir,
        })
    } else {
        let bb = find_corridor_size(builder, ctx.random, foot_x, foot_y, foot_z, dir)?;
        // 走廊构造器里的随机：hasRails 的 nextInt(3)，以及仅当 !hasRails 时
        // spiderCorridor 的 nextInt(23)（&& 短路）。
        let has_rails = ctx.random.next_int(3) == 0;
        let spider = !has_rails && ctx.random.next_int(23) == 0;
        let num_sections = if dir.axis_z() {
            z_span(&bb) / 5
        } else {
            x_span(&bb) / 5
        };
        Some(Shaft::Corridor {
            bb,
            gd: gen_depth,
            dir,
            has_rails,
            spider,
            num_sections,
        })
    }
}

/// `generateAndAddPiece`：深度/距离门 → 造件 → 加入 builder → 递归 addChildren。
/// 返回新件的包围盒（`MineShaftRoom` 记 Entrances 用）。
fn generate_and_add_piece(
    builder: &mut Builder,
    ctx: &Ctx<'_>,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: HDir,
    depth: i32,
) -> Option<Box> {
    if depth > MAX_DEPTH {
        return None;
    }
    if (foot_x - ctx.start_min_x).abs() > 80 || (foot_z - ctx.start_min_z).abs() > 80 {
        return None;
    }
    let shaft = create_random_shaft_piece(builder, ctx, foot_x, foot_y, foot_z, dir, depth + 1)?;
    let bb = shaft.bb();
    builder.add(shaft.to_piece(ctx.mesa));
    add_children(builder, ctx, &shaft);
    Some(bb)
}

/// 各子类的 `addChildren`。`depth` 用结构件自己的 genDepth，照官方逐分支复刻。
fn add_children(builder: &mut Builder, ctx: &Ctx<'_>, shaft: &Shaft) {
    match *shaft {
        Shaft::Room { bb } => room_add_children(builder, ctx, bb),
        Shaft::Corridor { bb, gd, dir, .. } => corridor_add_children(builder, ctx, bb, gd, dir),
        Shaft::Crossing {
            bb,
            gd,
            dir,
            two_floored,
        } => crossing_add_children(builder, ctx, bb, gd, dir, two_floored),
        Shaft::Stairs { bb, gd, dir } => stairs_add_children(builder, ctx, bb, gd, dir),
    }
}

/// `MineShaftCorridor.addChildren`：末端 `nextInt(4)` 选方向，各分支一次带
/// `nextInt(3)` 的 `generateAndAddPiece`；`depth < 8` 时逐段 `nextInt(5)` 再可能生成侧支。
fn corridor_add_children(builder: &mut Builder, ctx: &Ctx<'_>, bb: Box, depth: i32, dir: HDir) {
    let end = ctx.random.next_int(4);
    let min_x = bb[0];
    let min_y = bb[1];
    let min_z = bb[2];
    let max_x = bb[3];
    let max_z = bb[5];
    // 末端分支：每个分支恰好一次 generateAndAddPiece，footY 里的 nextInt(3)
    // 作为参数在调用前就被消费（即使 generateAndAddPiece 内部返回 null）。
    let (fx, fz, branch_dir) = match dir {
        HDir::North => {
            if end <= 1 {
                (min_x, min_z - 1, HDir::North)
            } else if end == 2 {
                (min_x - 1, min_z, HDir::West)
            } else {
                (max_x + 1, min_z, HDir::East)
            }
        }
        HDir::South => {
            if end <= 1 {
                (min_x, max_z + 1, HDir::South)
            } else if end == 2 {
                (min_x - 1, max_z - 3, HDir::West)
            } else {
                (max_x + 1, max_z - 3, HDir::East)
            }
        }
        HDir::West => {
            if end <= 1 {
                (min_x - 1, min_z, HDir::West)
            } else if end == 2 {
                (min_x, min_z - 1, HDir::North)
            } else {
                (min_x, max_z + 1, HDir::South)
            }
        }
        HDir::East => {
            if end <= 1 {
                (max_x + 1, min_z, HDir::East)
            } else if end == 2 {
                (max_x - 3, min_z - 1, HDir::North)
            } else {
                (max_x - 3, max_z + 1, HDir::South)
            }
        }
    };
    let fy = min_y - 1 + ctx.random.next_int(3);
    generate_and_add_piece(builder, ctx, fx, fy, fz, branch_dir, depth);

    // 侧支循环：depth < 8 才走；每段一次 nextInt(5)。
    if depth < MAX_DEPTH {
        if !dir.axis_z() {
            let mut x = min_x + 3;
            while x + 3 <= max_x {
                let selection = ctx.random.next_int(5);
                if selection == 0 {
                    generate_and_add_piece(
                        builder,
                        ctx,
                        x,
                        min_y,
                        min_z - 1,
                        HDir::North,
                        depth + 1,
                    );
                } else if selection == 1 {
                    generate_and_add_piece(
                        builder,
                        ctx,
                        x,
                        min_y,
                        max_z + 1,
                        HDir::South,
                        depth + 1,
                    );
                }
                x += 5;
            }
        } else {
            let mut z = min_z + 3;
            while z + 3 <= max_z {
                let selection = ctx.random.next_int(5);
                if selection == 0 {
                    generate_and_add_piece(
                        builder,
                        ctx,
                        min_x - 1,
                        min_y,
                        z,
                        HDir::West,
                        depth + 1,
                    );
                } else if selection == 1 {
                    generate_and_add_piece(
                        builder,
                        ctx,
                        max_x + 1,
                        min_y,
                        z,
                        HDir::East,
                        depth + 1,
                    );
                }
                z += 5;
            }
        }
    }
}

/// `MineShaftCrossing.addChildren`：三面各一次 generateAndAddPiece（footY 无随机），
/// 两层时再四次 nextBoolean 可能生成上层支。
fn crossing_add_children(
    builder: &mut Builder,
    ctx: &Ctx<'_>,
    bb: Box,
    depth: i32,
    dir: HDir,
    two_floored: bool,
) {
    let min_x = bb[0];
    let min_y = bb[1];
    let min_z = bb[2];
    let max_x = bb[3];
    let max_z = bb[5];
    let ends: [(i32, i32, HDir); 3] = match dir {
        HDir::North => [
            (min_x + 1, min_z - 1, HDir::North),
            (min_x - 1, min_z + 1, HDir::West),
            (max_x + 1, min_z + 1, HDir::East),
        ],
        HDir::South => [
            (min_x + 1, max_z + 1, HDir::South),
            (min_x - 1, min_z + 1, HDir::West),
            (max_x + 1, min_z + 1, HDir::East),
        ],
        HDir::West => [
            (min_x + 1, min_z - 1, HDir::North),
            (min_x + 1, max_z + 1, HDir::South),
            (min_x - 1, min_z + 1, HDir::West),
        ],
        HDir::East => [
            (min_x + 1, min_z - 1, HDir::North),
            (min_x + 1, max_z + 1, HDir::South),
            (max_x + 1, min_z + 1, HDir::East),
        ],
    };
    for &(fx, fz, d) in &ends {
        generate_and_add_piece(builder, ctx, fx, min_y, fz, d, depth);
    }

    if two_floored {
        let upper_y = min_y + 3 + 1;
        let uppers: [(i32, i32, HDir); 4] = [
            (min_x + 1, min_z - 1, HDir::North),
            (min_x - 1, min_z + 1, HDir::West),
            (max_x + 1, min_z + 1, HDir::East),
            (min_x + 1, max_z + 1, HDir::South),
        ];
        for &(fx, fz, d) in &uppers {
            if ctx.random.next_boolean() {
                generate_and_add_piece(builder, ctx, fx, upper_y, fz, d, depth);
            }
        }
    }
}

/// `MineShaftStairs.addChildren`：按朝向一次 generateAndAddPiece。
fn stairs_add_children(builder: &mut Builder, ctx: &Ctx<'_>, bb: Box, depth: i32, dir: HDir) {
    let (fx, fz) = match dir {
        HDir::North => (bb[0], bb[2] - 1),
        HDir::South => (bb[0], bb[5] + 1),
        HDir::West => (bb[0] - 1, bb[2]),
        HDir::East => (bb[3] + 1, bb[2]),
    };
    generate_and_add_piece(builder, ctx, fx, bb[1], fz, dir, depth);
}

/// `MineShaftRoom.addChildren`：四面 4 个 while 循环，每轮 `nextInt(span)` +
/// `nextInt(heightSpace)`，成功时记一条 Entrances 盒。
fn room_add_children(builder: &mut Builder, ctx: &Ctx<'_>, bb: Box) {
    let mut height_space = y_span(&bb) - 3 - 1;
    if height_space <= 0 {
        height_space = 1;
    }
    let x_span_v = x_span(&bb);
    let z_span_v = z_span(&bb);
    let mut entrances: Vec<Box> = Vec::new();

    // 北面（NORTH 出口，footZ = minZ-1）。
    let mut pos = 0;
    while pos < x_span_v {
        pos += ctx.random.next_int(x_span_v);
        if pos + 3 > x_span_v {
            break;
        }
        let fy = bb[1] + ctx.random.next_int(height_space) + 1;
        if let Some(child) =
            generate_and_add_piece(builder, ctx, bb[0] + pos, fy, bb[2] - 1, HDir::North, 0)
        {
            entrances.push(new_box(
                child[0],
                child[1],
                bb[2],
                child[3],
                child[4],
                bb[2] + 1,
            ));
        }
        pos += 4;
    }

    // 南面（SOUTH 出口，footZ = maxZ+1）。
    pos = 0;
    while pos < x_span_v {
        pos += ctx.random.next_int(x_span_v);
        if pos + 3 > x_span_v {
            break;
        }
        let fy = bb[1] + ctx.random.next_int(height_space) + 1;
        if let Some(child) =
            generate_and_add_piece(builder, ctx, bb[0] + pos, fy, bb[5] + 1, HDir::South, 0)
        {
            entrances.push(new_box(
                child[0],
                child[1],
                bb[5] - 1,
                child[3],
                child[4],
                bb[5],
            ));
        }
        pos += 4;
    }

    // 西面（WEST 出口，footX = minX-1）。
    pos = 0;
    while pos < z_span_v {
        pos += ctx.random.next_int(z_span_v);
        if pos + 3 > z_span_v {
            break;
        }
        let fy = bb[1] + ctx.random.next_int(height_space) + 1;
        if let Some(child) =
            generate_and_add_piece(builder, ctx, bb[0] - 1, fy, bb[2] + pos, HDir::West, 0)
        {
            entrances.push(new_box(
                bb[0],
                child[1],
                child[2],
                bb[0] + 1,
                child[4],
                child[5],
            ));
        }
        pos += 4;
    }

    // 东面（EAST 出口，footX = maxX+1）。
    pos = 0;
    while pos < z_span_v {
        pos += ctx.random.next_int(z_span_v);
        if pos + 3 > z_span_v {
            break;
        }
        let fy = bb[1] + ctx.random.next_int(height_space) + 1;
        if let Some(child) =
            generate_and_add_piece(builder, ctx, bb[3] + 1, fy, bb[2] + pos, HDir::East, 0)
        {
            entrances.push(new_box(
                bb[3] - 1,
                child[1],
                child[2],
                bb[3],
                child[4],
                child[5],
            ));
        }
        pos += 4;
    }

    // Entrances 记在起始房间（builder 的第 0 个结构件）上。
    if let Some(room) = builder.pieces.first_mut() {
        room.entrances = entrances;
    }
}

/// 生成废弃矿井的结构件列表（普通 / 恶地）。
///
/// `surface_height` 只在恶地类型用到：官方由 `chunkGenerator.getBaseHeight` 提供，
/// 属于 terrain 阶段，这里作为外部输入传入，使布局函数只依赖给定的高度值。
/// 返回的结构件包围盒已包含竖直平移后的最终 Y。
///
/// Build the mineshaft piece list (normal / mesa). `surface_height` is only used
/// for mesa (vanilla takes it from `chunkGenerator.getBaseHeight`, a terrain-stage
/// value); it is passed in as an external input so the layout function only depends
/// on the given height. Returned boxes already include the final vertical shift.
pub fn generate(
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    mesa: bool,
    surface_height: i32,
) -> Vec<Piece> {
    let random = make_random(seed, chunk_x, chunk_z);
    // `findGenerationPoint` 第一行：`context.random().nextDouble()`。
    random.next_double();

    let west = block_at(chunk_x, 2);
    let north = block_at(chunk_z, 2);
    // `MineShaftRoom` 构造器：三次 nextInt(6) 依次给 maxX / maxY / maxZ。
    let max_x = west + 7 + random.next_int(6);
    let max_y = 54 + random.next_int(6);
    let max_z = north + 7 + random.next_int(6);
    let room_bb = new_box(west, 50, north, max_x, max_y, max_z);

    let mut builder = Builder::new();
    builder.add(Shaft::Room { bb: room_bb }.to_piece(mesa));

    let ctx = Ctx {
        random: &random,
        start_min_x: room_bb[0],
        start_min_z: room_bb[2],
        mesa,
    };
    room_add_children(&mut builder, &ctx, room_bb);

    if mesa {
        // 恶地：把中心 Y 移到 [seaLevel, surfaceHeight] 之间的随机值。
        let bb = builder.bounding_box();
        let center_y = center_y(&bb);
        let target_y = if surface_height <= SEA_LEVEL {
            SEA_LEVEL
        } else {
            random.next_int_between_inclusive(SEA_LEVEL, surface_height)
        };
        builder.offset_vertically(target_y - center_y);
    } else {
        builder.move_below_sea_level(SEA_LEVEL, MIN_Y, &random, BELOW_SEA_LEVEL_OFFSET);
    }

    builder.pieces
}
