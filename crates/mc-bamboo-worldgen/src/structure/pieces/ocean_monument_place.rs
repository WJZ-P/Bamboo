//! `OceanMonumentPieces` 里各结构件的 `postProcess`。
//!
//! 逐行照官方 26.3 翻译：`MonumentBuilding` 的水体 / 两翼 / 门廊 / 墙 / 屋顶 / 7×7 柱子，
//! 入口房间、核心房间、六种双房间、两种简单房间、两翼和阁楼。公共的
//! `generateWaterBox`（`FILL_KEEP` 四个方块、`getWorldY(y) >= seaLevel` 与
//! `block != FILL_BLOCK` 的状态比较）、`generateBoxOnFillOnly`、`generateDefaultFloor`、
//! `chunkIntersects`（只用 XZ）都照抄。
//!
//! 远古守卫者（`spawnElder`）只做 `chunkBB.isInside(pos)` 的位置判定，不建实体：官方这里
//! 的随机取数全在 `level.getRandom()`（`WorldGenRegion` 的位置随机源）上，不消耗
//! `postProcess` 的 `random`，也不写任何方块，所以对逐方块对拍没有影响。
//!
//! `postProcess` of every `OceanMonumentPieces` structure piece, translated line by line
//! from vanilla 26.3: the `MonumentBuilding` water body / wings / entrance arches and
//! wall / roof / 7×7 pillars, the entry room, the core room, the six double rooms, the two
//! simple rooms, the wings and the penthouse. The shared `generateWaterBox` (the four
//! `FILL_KEEP` blocks, `getWorldY(y) >= seaLevel` and the `block != FILL_BLOCK` state
//! comparison), `generateBoxOnFillOnly`, `generateDefaultFloor` and `chunkIntersects`
//! (XZ only) are transcribed too.
//!
//! `spawnElder` only performs the `chunkBB.isInside(pos)` check and creates no entity:
//! vanilla draws there exclusively from `level.getRandom()` (the `WorldGenRegion`'s
//! positional source), never from the `postProcess` `random`, and writes no blocks, so it
//! cannot affect the per-block oracle.

use crate::random::WorldgenRandom;
use crate::structure::piece_place::{PieceCtx, PieceLevel, block, fill_column_down, generate_box};

use super::ocean_monument::RoomDef;

/// `OceanMonumentPiece.BASE_GRAY`（海晶石）。
///
/// `OceanMonumentPiece.BASE_GRAY` (prismarine).
pub(super) fn base_gray() -> u32 {
    block("minecraft:prismarine")
}

/// `OceanMonumentPiece.BASE_LIGHT`（海晶石砖）。
///
/// `OceanMonumentPiece.BASE_LIGHT` (prismarine bricks).
pub(super) fn base_light() -> u32 {
    block("minecraft:prismarine_bricks")
}

/// `OceanMonumentPiece.BASE_BLACK`（暗海晶石）。
///
/// `OceanMonumentPiece.BASE_BLACK` (dark prismarine).
pub(super) fn base_black() -> u32 {
    block("minecraft:dark_prismarine")
}

/// `OceanMonumentPiece.LAMP_BLOCK`（海灯）。
///
/// `OceanMonumentPiece.LAMP_BLOCK` (sea lantern).
pub(super) fn lamp() -> u32 {
    block("minecraft:sea_lantern")
}

/// `OceanMonumentPiece.FILL_BLOCK`（水）。
///
/// `OceanMonumentPiece.FILL_BLOCK` (water).
pub(super) fn fill_block() -> u32 {
    block("minecraft:water")
}

/// `OceanMonumentPiece.FILL_KEEP`：`{ICE, PACKED_ICE, BLUE_ICE, WATER}`（按方块身份）。
///
/// `OceanMonumentPiece.FILL_KEEP`: `{ICE, PACKED_ICE, BLUE_ICE, WATER}` (block identity).
fn is_fill_keep(state: u32) -> bool {
    crate::structure::piece_place::is_block(state, "minecraft:ice")
        || crate::structure::piece_place::is_block(state, "minecraft:packed_ice")
        || crate::structure::piece_place::is_block(state, "minecraft:blue_ice")
        || crate::structure::piece_place::is_block(state, "minecraft:water")
}

/// 一个海底神殿结构件的放置上下文。
///
/// Placement context of one ocean monument piece.
pub struct MonumentPiece<'a> {
    pub ctx: PieceCtx,
    pub level: &'a mut dyn PieceLevel,
    pub chunk_bb: [i32; 6],
    /// `level.getSeaLevel()`。
    pub sea_level: i32,
    /// `MonumentBuilding.generateRoomGraph` 造出来的房间图（子件按 index 查）。
    ///
    /// The room graph built by `MonumentBuilding.generateRoomGraph`; children look rooms
    /// up by index.
    pub rooms: &'a [RoomDef],
    /// `OceanMonumentPiece.roomDefinition.index`；两翼和阁楼为 `None`。
    ///
    /// `OceanMonumentPiece.roomDefinition.index`; `None` for wings and the penthouse.
    pub room: Option<i32>,
}

/// 单个房间节点。
///
/// One room node.
pub(super) fn room_of<'r>(piece: &'r MonumentPiece<'_>, index: i32) -> &'r RoomDef {
    &piece.rooms[index as usize]
}

/// `roomDefinition.connections[dir]`，null 用 `None` 表示。
///
/// `roomDefinition.connections[dir]`, with null as `None`.
pub(super) fn connection(piece: &MonumentPiece<'_>, index: i32, dir: usize) -> Option<i32> {
    let target = room_of(piece, index).connections[dir];
    (target >= 0).then_some(target)
}

/// `roomDefinition.hasOpening[dir]`。
///
/// `roomDefinition.hasOpening[dir]`.
pub(super) fn opening(piece: &MonumentPiece<'_>, index: i32, dir: usize) -> bool {
    room_of(piece, index).has_opening[dir]
}

/// 子件自己的房间 index。
///
/// The child's own room index.
pub(super) fn own_room(piece: &MonumentPiece<'_>) -> i32 {
    piece
        .room
        .unwrap_or_else(|| panic!("monument piece without a room definition"))
}

impl MonumentPiece<'_> {
    /// `StructurePiece.getWorldY(y)`。
    ///
    /// `StructurePiece.getWorldY(y)`.
    pub(super) fn world_y(&self, y: i32) -> i32 {
        self.ctx.world_y(y)
    }

    /// `placeBlock`。
    ///
    /// `placeBlock`.
    pub(super) fn place(&mut self, state: u32, x: i32, y: i32, z: i32) {
        let chunk_bb = self.chunk_bb;
        self.ctx.place_block(self.level, state, x, y, z, &chunk_bb);
    }

    /// `getBlock`。
    ///
    /// `getBlock`.
    pub(super) fn get(&self, x: i32, y: i32, z: i32) -> u32 {
        self.ctx.get_block(self.level, &self.chunk_bb, x, y, z)
    }

    /// `generateBox(..., skipAir = false)`。
    ///
    /// `generateBox(..., skipAir = false)`.
    pub(super) fn box_at(&mut self, range: [i32; 6], edge: u32, fill: u32) {
        let chunk_bb = self.chunk_bb;
        generate_box(&self.ctx, self.level, &chunk_bb, range, edge, fill, false);
    }

    /// `OceanMonumentPiece.generateWaterBox`。
    ///
    /// `OceanMonumentPiece.generateWaterBox`.
    pub(super) fn water_box(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        let water = fill_block();
        let air = mc_bamboo_data::blocks::AIR;
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    let current = self.get(x, y, z);
                    if !is_fill_keep(current) {
                        if self.world_y(y) >= self.sea_level && current != water {
                            self.place(air, x, y, z);
                        } else {
                            self.place(water, x, y, z);
                        }
                    }
                }
            }
        }
    }

    /// `OceanMonumentPiece.generateBoxOnFillOnly`。
    ///
    /// `OceanMonumentPiece.generateBoxOnFillOnly`.
    pub(super) fn box_on_fill_only(&mut self, range: [i32; 6], target: u32) {
        let [x0, y0, z0, x1, y1, z1] = range;
        let water = fill_block();
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if self.get(x, y, z) == water {
                        self.place(target, x, y, z);
                    }
                }
            }
        }
    }

    /// `OceanMonumentPiece.generateDefaultFloor`。
    ///
    /// `OceanMonumentPiece.generateDefaultFloor`.
    pub(super) fn default_floor(&mut self, x_off: i32, z_off: i32, down_opening: bool) {
        if down_opening {
            let gray = base_gray();
            let light = base_light();
            self.box_at([x_off, 0, z_off, x_off + 2, 0, z_off + 7], gray, gray);
            self.box_at([x_off + 5, 0, z_off, x_off + 7, 0, z_off + 7], gray, gray);
            self.box_at([x_off + 3, 0, z_off, x_off + 4, 0, z_off + 2], gray, gray);
            self.box_at(
                [x_off + 3, 0, z_off + 5, x_off + 4, 0, z_off + 7],
                gray,
                gray,
            );
            self.box_at(
                [x_off + 3, 0, z_off + 2, x_off + 4, 0, z_off + 2],
                light,
                light,
            );
            self.box_at(
                [x_off + 3, 0, z_off + 5, x_off + 4, 0, z_off + 5],
                light,
                light,
            );
            self.box_at(
                [x_off + 2, 0, z_off + 3, x_off + 2, 0, z_off + 4],
                light,
                light,
            );
            self.box_at(
                [x_off + 5, 0, z_off + 3, x_off + 5, 0, z_off + 4],
                light,
                light,
            );
        } else {
            let gray = base_gray();
            self.box_at([x_off, 0, z_off, x_off + 7, 0, z_off + 7], gray, gray);
        }
    }

    /// `OceanMonumentPiece.chunkIntersects`：只用 XZ 的 `intersects`。
    ///
    /// `OceanMonumentPiece.chunkIntersects`: an `intersects` over XZ only.
    fn chunk_intersects(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> bool {
        let wx0 = self.ctx.world_x(x0, z0);
        let wz0 = self.ctx.world_z(x0, z0);
        let wx1 = self.ctx.world_x(x1, z1);
        let wz1 = self.ctx.world_z(x1, z1);
        let (min_x, max_x) = (wx0.min(wx1), wx0.max(wx1));
        let (min_z, max_z) = (wz0.min(wz1), wz0.max(wz1));
        let bb = &self.chunk_bb;
        max_x >= bb[0] && min_x <= bb[3] && max_z >= bb[2] && min_z <= bb[5]
    }

    /// `OceanMonumentPiece.spawnElder` 的远古守卫者位置（实体不比，所以只算坐标）。
    ///
    /// 官方在这里 `EntityTypes.ELDER_GUARDIAN.create(level.getLevel(), ...)` 并把它加进
    /// 世界；实体不比，它也不写方块，`finalizeSpawn` 的取数全在 `level.getRandom()` 上
    /// （`WorldGenRegion` 的位置随机源），不消耗 `postProcess` 的 `random`。
    ///
    /// The elder guardian position of `OceanMonumentPiece.spawnElder` (entities are not
    /// compared, so only the coordinate is computed). Vanilla creates an
    /// `ELDER_GUARDIAN` and adds it to the world; entities are not compared, no block is
    /// written, and `finalizeSpawn`'s draws all come from `level.getRandom()` (the
    /// `WorldGenRegion`'s positional source), never from the `postProcess` `random`.
    pub(super) fn elder_position(&self, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
        self.ctx.world_pos(x, y, z)
    }
}

/// `MonumentBuilding.postProcess`。
///
/// `MonumentBuilding.postProcess`.
pub fn building_post_process(piece: &mut MonumentPiece<'_>, random: &WorldgenRandom) {
    let water_height = piece.sea_level.max(64) - piece.ctx.bb[1];
    piece.water_box(0, 0, 0, 58, water_height, 58);
    generate_wing(piece, false, 0);
    generate_wing(piece, true, 33);
    generate_entrance_archs(piece);
    generate_entrance_wall(piece);
    generate_roof_piece(piece);
    generate_lower_wall(piece);
    generate_middle_wall(piece);
    generate_upper_wall(piece);

    for pillar_x in 0..7 {
        let mut pillar_z = 0;
        while pillar_z < 7 {
            if pillar_z == 0 && pillar_x == 3 {
                pillar_z = 6;
            }
            let bx = pillar_x * 9;
            let bz = pillar_z * 9;
            for w in 0..4 {
                for d in 0..4 {
                    let light = base_light();
                    piece.place(light, bx + w, 0, bz + d);
                    let chunk_bb = piece.chunk_bb;
                    fill_column_down(
                        &piece.ctx,
                        piece.level,
                        &chunk_bb,
                        light,
                        bx + w,
                        -1,
                        bz + d,
                    );
                }
            }
            if pillar_x != 0 && pillar_x != 6 {
                pillar_z += 6;
            } else {
                pillar_z += 1;
            }
        }
    }

    for i in 0..5 {
        piece.water_box(-1 - i, i * 2, -1 - i, -1 - i, 23, 58 + i);
        piece.water_box(58 + i, i * 2, -1 - i, 58 + i, 23, 58 + i);
        piece.water_box(-i, i * 2, -1 - i, 57 + i, 23, -1 - i);
        piece.water_box(-i, i * 2, 58 + i, 57 + i, 23, 58 + i);
    }
    // `postProcess` 自己不取数（子件的 `postProcess` 才取）。
    // `postProcess` itself draws nothing; the children's `postProcess` do.
    let _ = random;
}

/// `MonumentBuilding.generateWing`。
///
/// `MonumentBuilding.generateWing`.
fn generate_wing(piece: &mut MonumentPiece<'_>, flipped: bool, x_off: i32) {
    if !piece.chunk_intersects(x_off, 0, x_off + 23, 20) {
        return;
    }
    let gray = base_gray();
    let light = base_light();
    piece.box_at([x_off, 0, 0, x_off + 24, 0, 20], gray, gray);
    piece.water_box(x_off, 1, 0, x_off + 24, 10, 20);

    for i in 0..4 {
        piece.box_at([x_off + i, i + 1, i, x_off + i, i + 1, 20], light, light);
        piece.box_at(
            [x_off + i + 7, i + 5, i + 7, x_off + i + 7, i + 5, 20],
            light,
            light,
        );
        piece.box_at(
            [x_off + 17 - i, i + 5, i + 7, x_off + 17 - i, i + 5, 20],
            light,
            light,
        );
        piece.box_at(
            [x_off + 24 - i, i + 1, i, x_off + 24 - i, i + 1, 20],
            light,
            light,
        );
        piece.box_at(
            [x_off + i + 1, i + 1, i, x_off + 23 - i, i + 1, i],
            light,
            light,
        );
        piece.box_at(
            [x_off + i + 8, i + 5, i + 7, x_off + 16 - i, i + 5, i + 7],
            light,
            light,
        );
    }

    piece.box_at([x_off + 4, 4, 4, x_off + 6, 4, 20], gray, gray);
    piece.box_at([x_off + 7, 4, 4, x_off + 17, 4, 6], gray, gray);
    piece.box_at([x_off + 18, 4, 4, x_off + 20, 4, 20], gray, gray);
    piece.box_at([x_off + 11, 8, 11, x_off + 13, 8, 20], gray, gray);
    piece.place(light, x_off + 12, 9, 12);
    piece.place(light, x_off + 12, 9, 15);
    piece.place(light, x_off + 12, 9, 18);
    let left_pos = x_off + if flipped { 19 } else { 5 };
    let right_pos = x_off + if flipped { 5 } else { 19 };

    let mut z = 20;
    while z >= 5 {
        piece.place(light, left_pos, 5, z);
        z -= 3;
    }
    let mut z = 19;
    while z >= 7 {
        piece.place(light, right_pos, 5, z);
        z -= 3;
    }

    for i in 0..4 {
        let pos = if flipped {
            x_off + 24 - (17 - i * 3)
        } else {
            x_off + 17 - i * 3
        };
        piece.place(light, pos, 5, 5);
    }
    piece.place(light, right_pos, 5, 5);
    piece.box_at([x_off + 11, 1, 12, x_off + 13, 7, 12], gray, gray);
    piece.box_at([x_off + 12, 1, 11, x_off + 12, 7, 13], gray, gray);
}

/// `MonumentBuilding.generateEntranceArchs`。
///
/// `MonumentBuilding.generateEntranceArchs`.
fn generate_entrance_archs(piece: &mut MonumentPiece<'_>) {
    if !piece.chunk_intersects(22, 5, 35, 17) {
        return;
    }
    let gray = base_gray();
    let light = base_light();
    let lamp = lamp();
    piece.water_box(25, 0, 0, 32, 8, 20);

    for i in 0..4 {
        piece.box_at([24, 2, 5 + i * 4, 24, 4, 5 + i * 4], light, light);
        piece.box_at([22, 4, 5 + i * 4, 23, 4, 5 + i * 4], light, light);
        piece.place(light, 25, 5, 5 + i * 4);
        piece.place(light, 26, 6, 5 + i * 4);
        piece.place(lamp, 26, 5, 5 + i * 4);
        piece.box_at([33, 2, 5 + i * 4, 33, 4, 5 + i * 4], light, light);
        piece.box_at([34, 4, 5 + i * 4, 35, 4, 5 + i * 4], light, light);
        piece.place(light, 32, 5, 5 + i * 4);
        piece.place(light, 31, 6, 5 + i * 4);
        piece.place(lamp, 31, 5, 5 + i * 4);
        piece.box_at([27, 6, 5 + i * 4, 30, 6, 5 + i * 4], gray, gray);
    }
}

/// `MonumentBuilding.generateEntranceWall`。
///
/// `MonumentBuilding.generateEntranceWall`.
fn generate_entrance_wall(piece: &mut MonumentPiece<'_>) {
    if !piece.chunk_intersects(15, 20, 42, 21) {
        return;
    }
    let gray = base_gray();
    let light = base_light();
    let black = base_black();
    piece.box_at([15, 0, 21, 42, 0, 21], gray, gray);
    piece.water_box(26, 1, 21, 31, 3, 21);
    piece.box_at([21, 12, 21, 36, 12, 21], gray, gray);
    piece.box_at([17, 11, 21, 40, 11, 21], gray, gray);
    piece.box_at([16, 10, 21, 41, 10, 21], gray, gray);
    piece.box_at([15, 7, 21, 42, 9, 21], gray, gray);
    piece.box_at([16, 6, 21, 41, 6, 21], gray, gray);
    piece.box_at([17, 5, 21, 40, 5, 21], gray, gray);
    piece.box_at([21, 4, 21, 36, 4, 21], gray, gray);
    piece.box_at([22, 3, 21, 26, 3, 21], gray, gray);
    piece.box_at([31, 3, 21, 35, 3, 21], gray, gray);
    piece.box_at([23, 2, 21, 25, 2, 21], gray, gray);
    piece.box_at([32, 2, 21, 34, 2, 21], gray, gray);
    piece.box_at([28, 4, 20, 29, 4, 21], light, light);
    piece.place(light, 27, 3, 21);
    piece.place(light, 30, 3, 21);
    piece.place(light, 26, 2, 21);
    piece.place(light, 31, 2, 21);
    piece.place(light, 25, 1, 21);
    piece.place(light, 32, 1, 21);

    for i in 0..7 {
        piece.place(black, 28 - i, 6 + i, 21);
        piece.place(black, 29 + i, 6 + i, 21);
    }
    for i in 0..4 {
        piece.place(black, 28 - i, 9 + i, 21);
        piece.place(black, 29 + i, 9 + i, 21);
    }

    piece.place(black, 28, 12, 21);
    piece.place(black, 29, 12, 21);

    for i in 0..3 {
        piece.place(black, 22 - i * 2, 8, 21);
        piece.place(black, 22 - i * 2, 9, 21);
        piece.place(black, 35 + i * 2, 8, 21);
        piece.place(black, 35 + i * 2, 9, 21);
    }

    piece.water_box(15, 13, 21, 42, 15, 21);
    piece.water_box(15, 1, 21, 15, 6, 21);
    piece.water_box(16, 1, 21, 16, 5, 21);
    piece.water_box(17, 1, 21, 20, 4, 21);
    piece.water_box(21, 1, 21, 21, 3, 21);
    piece.water_box(22, 1, 21, 22, 2, 21);
    piece.water_box(23, 1, 21, 24, 1, 21);
    piece.water_box(42, 1, 21, 42, 6, 21);
    piece.water_box(41, 1, 21, 41, 5, 21);
    piece.water_box(37, 1, 21, 40, 4, 21);
    piece.water_box(36, 1, 21, 36, 3, 21);
    piece.water_box(33, 1, 21, 34, 1, 21);
    piece.water_box(35, 1, 21, 35, 2, 21);
}

/// `MonumentBuilding.generateRoofPiece`。
///
/// `MonumentBuilding.generateRoofPiece`.
fn generate_roof_piece(piece: &mut MonumentPiece<'_>) {
    if !piece.chunk_intersects(21, 21, 36, 36) {
        return;
    }
    let gray = base_gray();
    let light = base_light();
    let lamp = lamp();
    piece.box_at([21, 0, 22, 36, 0, 36], gray, gray);
    piece.water_box(21, 1, 22, 36, 23, 36);

    for i in 0..4 {
        piece.box_at(
            [21 + i, 13 + i, 21 + i, 36 - i, 13 + i, 21 + i],
            light,
            light,
        );
        piece.box_at(
            [21 + i, 13 + i, 36 - i, 36 - i, 13 + i, 36 - i],
            light,
            light,
        );
        piece.box_at(
            [21 + i, 13 + i, 22 + i, 21 + i, 13 + i, 35 - i],
            light,
            light,
        );
        piece.box_at(
            [36 - i, 13 + i, 22 + i, 36 - i, 13 + i, 35 - i],
            light,
            light,
        );
    }

    piece.box_at([25, 16, 25, 32, 16, 32], gray, gray);
    piece.box_at([25, 17, 25, 25, 19, 25], light, light);
    piece.box_at([32, 17, 25, 32, 19, 25], light, light);
    piece.box_at([25, 17, 32, 25, 19, 32], light, light);
    piece.box_at([32, 17, 32, 32, 19, 32], light, light);
    piece.place(light, 26, 20, 26);
    piece.place(light, 27, 21, 27);
    piece.place(lamp, 27, 20, 27);
    piece.place(light, 26, 20, 31);
    piece.place(light, 27, 21, 30);
    piece.place(lamp, 27, 20, 30);
    piece.place(light, 31, 20, 31);
    piece.place(light, 30, 21, 30);
    piece.place(lamp, 30, 20, 30);
    piece.place(light, 31, 20, 26);
    piece.place(light, 30, 21, 27);
    piece.place(lamp, 30, 20, 27);
    piece.box_at([28, 21, 27, 29, 21, 27], gray, gray);
    piece.box_at([27, 21, 28, 27, 21, 29], gray, gray);
    piece.box_at([28, 21, 30, 29, 21, 30], gray, gray);
    piece.box_at([30, 21, 28, 30, 21, 29], gray, gray);
}

/// `MonumentBuilding.generateLowerWall`。
///
/// `MonumentBuilding.generateLowerWall`.
fn generate_lower_wall(piece: &mut MonumentPiece<'_>) {
    let gray = base_gray();
    let light = base_light();
    if piece.chunk_intersects(0, 21, 6, 58) {
        piece.box_at([0, 0, 21, 6, 0, 57], gray, gray);
        piece.water_box(0, 1, 21, 6, 7, 57);
        piece.box_at([4, 4, 21, 6, 4, 53], gray, gray);

        for i in 0..4 {
            piece.box_at([i, i + 1, 21, i, i + 1, 57 - i], light, light);
        }

        let mut z = 23;
        while z < 53 {
            piece.place(light, 5, 5, z);
            z += 3;
        }
        piece.place(light, 5, 5, 52);

        for i in 0..4 {
            piece.box_at([i, i + 1, 21, i, i + 1, 57 - i], light, light);
        }

        piece.box_at([4, 1, 52, 6, 3, 52], gray, gray);
        piece.box_at([5, 1, 51, 5, 3, 53], gray, gray);
    }

    if piece.chunk_intersects(51, 21, 58, 58) {
        piece.box_at([51, 0, 21, 57, 0, 57], gray, gray);
        piece.water_box(51, 1, 21, 57, 7, 57);
        piece.box_at([51, 4, 21, 53, 4, 53], gray, gray);

        for i in 0..4 {
            piece.box_at([57 - i, i + 1, 21, 57 - i, i + 1, 57 - i], light, light);
        }

        let mut z = 23;
        while z < 53 {
            piece.place(light, 52, 5, z);
            z += 3;
        }
        piece.place(light, 52, 5, 52);
        piece.box_at([51, 1, 52, 53, 3, 52], gray, gray);
        piece.box_at([52, 1, 51, 52, 3, 53], gray, gray);
    }

    if piece.chunk_intersects(0, 51, 57, 57) {
        piece.box_at([7, 0, 51, 50, 0, 57], gray, gray);
        piece.water_box(7, 1, 51, 50, 10, 57);

        for i in 0..4 {
            piece.box_at([i + 1, i + 1, 57 - i, 56 - i, i + 1, 57 - i], light, light);
        }
    }
}

/// `MonumentBuilding.generateMiddleWall`。
///
/// `MonumentBuilding.generateMiddleWall`.
fn generate_middle_wall(piece: &mut MonumentPiece<'_>) {
    let gray = base_gray();
    let light = base_light();
    if piece.chunk_intersects(7, 21, 13, 50) {
        piece.box_at([7, 0, 21, 13, 0, 50], gray, gray);
        piece.water_box(7, 1, 21, 13, 10, 50);
        piece.box_at([11, 8, 21, 13, 8, 53], gray, gray);

        for i in 0..4 {
            piece.box_at([i + 7, i + 5, 21, i + 7, i + 5, 54], light, light);
        }

        let mut z = 21;
        while z <= 45 {
            piece.place(light, 12, 9, z);
            z += 3;
        }
    }

    if piece.chunk_intersects(44, 21, 50, 54) {
        piece.box_at([44, 0, 21, 50, 0, 50], gray, gray);
        piece.water_box(44, 1, 21, 50, 10, 50);
        piece.box_at([44, 8, 21, 46, 8, 53], gray, gray);

        for i in 0..4 {
            piece.box_at([50 - i, i + 5, 21, 50 - i, i + 5, 54], light, light);
        }

        let mut z = 21;
        while z <= 45 {
            piece.place(light, 45, 9, z);
            z += 3;
        }
    }

    if piece.chunk_intersects(8, 44, 49, 54) {
        piece.box_at([14, 0, 44, 43, 0, 50], gray, gray);
        piece.water_box(14, 1, 44, 43, 10, 50);

        let mut x = 12;
        while x <= 45 {
            piece.place(light, x, 9, 45);
            piece.place(light, x, 9, 52);
            if x == 12 || x == 18 || x == 24 || x == 33 || x == 39 || x == 45 {
                piece.place(light, x, 9, 47);
                piece.place(light, x, 9, 50);
                piece.place(light, x, 10, 45);
                piece.place(light, x, 10, 46);
                piece.place(light, x, 10, 51);
                piece.place(light, x, 10, 52);
                piece.place(light, x, 11, 47);
                piece.place(light, x, 11, 50);
                piece.place(light, x, 12, 48);
                piece.place(light, x, 12, 49);
            }
            x += 3;
        }

        for i in 0..3 {
            piece.box_at([8 + i, 5 + i, 54, 49 - i, 5 + i, 54], gray, gray);
        }

        piece.box_at([11, 8, 54, 46, 8, 54], light, light);
        piece.box_at([14, 8, 44, 43, 8, 53], gray, gray);
    }
}

/// `MonumentBuilding.generateUpperWall`。
///
/// `MonumentBuilding.generateUpperWall`.
fn generate_upper_wall(piece: &mut MonumentPiece<'_>) {
    let gray = base_gray();
    let light = base_light();
    if piece.chunk_intersects(14, 21, 20, 43) {
        piece.box_at([14, 0, 21, 20, 0, 43], gray, gray);
        piece.water_box(14, 1, 22, 20, 14, 43);
        piece.box_at([18, 12, 22, 20, 12, 39], gray, gray);
        piece.box_at([18, 12, 21, 20, 12, 21], light, light);

        for i in 0..4 {
            piece.box_at([i + 14, i + 9, 21, i + 14, i + 9, 43 - i], light, light);
        }

        let mut z = 23;
        while z <= 39 {
            piece.place(light, 19, 13, z);
            z += 3;
        }
    }

    if piece.chunk_intersects(37, 21, 43, 43) {
        piece.box_at([37, 0, 21, 43, 0, 43], gray, gray);
        piece.water_box(37, 1, 22, 43, 14, 43);
        piece.box_at([37, 12, 22, 39, 12, 39], gray, gray);
        piece.box_at([37, 12, 21, 39, 12, 21], light, light);

        for i in 0..4 {
            piece.box_at([43 - i, i + 9, 21, 43 - i, i + 9, 43 - i], light, light);
        }

        let mut z = 23;
        while z <= 39 {
            piece.place(light, 38, 13, z);
            z += 3;
        }
    }

    if piece.chunk_intersects(15, 37, 42, 43) {
        piece.box_at([21, 0, 37, 36, 0, 43], gray, gray);
        piece.water_box(21, 1, 37, 36, 14, 43);
        piece.box_at([21, 12, 37, 36, 12, 39], gray, gray);

        for i in 0..4 {
            piece.box_at([15 + i, i + 9, 43 - i, 42 - i, i + 9, 43 - i], light, light);
        }

        let mut x = 21;
        while x <= 36 {
            piece.place(light, x, 13, 38);
            x += 3;
        }
    }
}

// __ENTRY__

/// 一次区块放置：`StructureStart.placeInChunk` 对海底神殿那一份的编排。
///
/// 官方 `MonumentBuilding.postProcess` 先放整栋楼的水体、两翼、门廊、墙、屋顶和柱子，
/// 最后遍历 `childPieces`，只要子件包围盒与 `chunkBB` 相交就调它的 `postProcess`。
/// 这里按同样顺序、同一份 `random` 做一遍。
///
/// One chunk placement, mirroring `StructureStart.placeInChunk` for the ocean monument.
/// Vanilla's `MonumentBuilding.postProcess` first lays the building's water body, wings,
/// entrance, walls, roof and pillars, then loops over `childPieces` and calls each
/// child's `postProcess` when its box intersects `chunkBB`. This does the same in the
/// same order with the same `random`.
pub fn place_in_chunk(
    layout: &super::ocean_monument::MonumentLayout,
    level: &mut dyn PieceLevel,
    chunk_bb: [i32; 6],
    sea_level: i32,
    random: &WorldgenRandom,
) {
    if !super::intersects(&layout.bb, &chunk_bb) {
        return;
    }

    // 顶层：`MonumentBuilding.postProcess`。
    // The top piece: `MonumentBuilding.postProcess`.
    {
        let mut piece = MonumentPiece {
            ctx: PieceCtx::new(layout.bb, Some(layout.dir)),
            level,
            chunk_bb,
            sea_level,
            rooms: &layout.rooms,
            room: None,
        };
        building_post_process(&mut piece, random);
    }

    // 子件：`if (child.getBoundingBox().intersects(chunkBB)) child.postProcess(...)`。
    // Children: `if (child.getBoundingBox().intersects(chunkBB)) child.postProcess(...)`.
    for child in &layout.children {
        if !super::intersects(&child.bb, &chunk_bb) {
            continue;
        }
        let mut piece = MonumentPiece {
            ctx: PieceCtx::new(child.bb, Some(layout.dir)),
            level,
            chunk_bb,
            sea_level,
            rooms: &layout.rooms,
            room: child.room,
        };
        super::monument_rooms::child_post_process(
            &mut piece,
            child.kind,
            child.main_design,
            child.wing_design,
            random,
        );
    }
}
