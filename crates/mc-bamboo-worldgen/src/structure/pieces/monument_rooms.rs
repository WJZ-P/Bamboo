//! 海底神殿各结构件 `postProcess` 的分发，以及入口、核心、两种简单房间、两翼和阁楼。
//!
//! 双房间在 [`super::monument_double_rooms`]。所有函数逐行照官方
//! `OceanMonumentPieces` 翻译；房间的墙 / 地板 / 开口由房间图的 `connections` /
//! `hasOpening` 决定，从 `MonumentPiece.rooms` 里查。
//!
//! The dispatch of the ocean monument `postProcess` implementations, plus the entry,
//! core, the two simple rooms, the wings and the penthouse. The double rooms live in
//! [`super::monument_double_rooms`]. Every function is translated line by line from
//! vanilla `OceanMonumentPieces`; walls, floors and openings come from the room graph's
//! `connections` / `hasOpening`, looked up through `MonumentPiece.rooms`.

use crate::random::WorldgenRandom;

use super::monument_double_rooms::{
    double_x_room, double_xy_room, double_y_room, double_yz_room, double_z_room,
};
use super::ocean_monument::RoomKind;
use super::ocean_monument_place::{
    MonumentPiece, base_black, base_gray, base_light, connection, lamp, opening, own_room,
};

/// `Direction.get3DDataValue()`：DOWN=0 UP=1 NORTH=2 SOUTH=3 WEST=4 EAST=5。
///
/// `Direction.get3DDataValue()`: DOWN=0, UP=1, NORTH=2, SOUTH=3, WEST=4, EAST=5.
pub(super) const D_DOWN: usize = 0;
pub(super) const D_UP: usize = 1;
pub(super) const D_NORTH: usize = 2;
pub(super) const D_SOUTH: usize = 3;
pub(super) const D_WEST: usize = 4;
pub(super) const D_EAST: usize = 5;

/// 子结构件的 `postProcess` 分发。`Building` 不在此列（由调用方单独调
/// [`super::ocean_monument_place::building_post_process`]）。
///
/// Dispatch a child piece's `postProcess`. `Building` is excluded; call
/// [`super::ocean_monument_place::building_post_process`] for it.
pub fn child_post_process(
    piece: &mut MonumentPiece<'_>,
    kind: RoomKind,
    main_design: i32,
    wing_design: i32,
    random: &WorldgenRandom,
) {
    match kind {
        RoomKind::Entry => entry_room(piece),
        RoomKind::Core => core_room(piece),
        RoomKind::DoubleX => double_x_room(piece),
        RoomKind::DoubleXY => double_xy_room(piece),
        RoomKind::DoubleY => double_y_room(piece),
        RoomKind::DoubleYZ => double_yz_room(piece),
        RoomKind::DoubleZ => double_z_room(piece),
        RoomKind::Simple => simple_room(piece, main_design, random),
        RoomKind::SimpleTop => simple_top_room(piece, random),
        RoomKind::Wing => wing_room(piece, wing_design),
        RoomKind::Penthouse => penthouse(piece),
        RoomKind::Building => panic!("MonumentBuilding is placed by building_post_process"),
    }
}

/// `OceanMonumentEntryRoom.postProcess`。
///
/// `OceanMonumentEntryRoom.postProcess`.
fn entry_room(piece: &mut MonumentPiece<'_>) {
    let light = base_light();
    piece.box_at([0, 3, 0, 2, 3, 7], light, light);
    piece.box_at([5, 3, 0, 7, 3, 7], light, light);
    piece.box_at([0, 2, 0, 1, 2, 7], light, light);
    piece.box_at([6, 2, 0, 7, 2, 7], light, light);
    piece.box_at([0, 1, 0, 0, 1, 7], light, light);
    piece.box_at([7, 1, 0, 7, 1, 7], light, light);
    piece.box_at([0, 1, 7, 7, 3, 7], light, light);
    piece.box_at([1, 1, 0, 2, 3, 0], light, light);
    piece.box_at([5, 1, 0, 6, 3, 0], light, light);
    let room = own_room(piece);
    if opening(piece, room, D_NORTH) {
        piece.water_box(3, 1, 7, 4, 2, 7);
    }
    if opening(piece, room, D_WEST) {
        piece.water_box(0, 1, 3, 1, 2, 4);
    }
    if opening(piece, room, D_EAST) {
        piece.water_box(6, 1, 3, 7, 2, 4);
    }
}

// __CORE__

/// `OceanMonumentCoreRoom.postProcess`。
///
/// `OceanMonumentCoreRoom.postProcess`.
fn core_room(piece: &mut MonumentPiece<'_>) {
    let gray = base_gray();
    let light = base_light();
    let black = base_black();
    let lamp = lamp();
    piece.box_on_fill_only([1, 8, 0, 14, 8, 14], gray);
    let mut block = light;
    piece.box_at([0, 7, 0, 0, 7, 15], block, block);
    piece.box_at([15, 7, 0, 15, 7, 15], block, block);
    piece.box_at([1, 7, 0, 15, 7, 0], block, block);
    piece.box_at([1, 7, 15, 14, 7, 15], block, block);

    for y in 1..=6 {
        block = light;
        if y == 2 || y == 6 {
            block = gray;
        }
        let mut x = 0;
        while x <= 15 {
            piece.box_at([x, y, 0, x, y, 1], block, block);
            piece.box_at([x, y, 6, x, y, 9], block, block);
            piece.box_at([x, y, 14, x, y, 15], block, block);
            x += 15;
        }
        piece.box_at([1, y, 0, 1, y, 0], block, block);
        piece.box_at([6, y, 0, 9, y, 0], block, block);
        piece.box_at([14, y, 0, 14, y, 0], block, block);
        piece.box_at([1, y, 15, 14, y, 15], block, block);
    }

    piece.box_at([6, 3, 6, 9, 6, 9], black, black);
    let gold = crate::structure::piece_place::block("minecraft:gold_block");
    piece.box_at([7, 4, 7, 8, 5, 8], gold, gold);

    let mut y = 3;
    while y <= 6 {
        let mut x = 6;
        while x <= 9 {
            piece.place(lamp, x, y, 6);
            piece.place(lamp, x, y, 9);
            x += 3;
        }
        y += 3;
    }

    piece.box_at([5, 1, 6, 5, 2, 6], light, light);
    piece.box_at([5, 1, 9, 5, 2, 9], light, light);
    piece.box_at([10, 1, 6, 10, 2, 6], light, light);
    piece.box_at([10, 1, 9, 10, 2, 9], light, light);
    piece.box_at([6, 1, 5, 6, 2, 5], light, light);
    piece.box_at([9, 1, 5, 9, 2, 5], light, light);
    piece.box_at([6, 1, 10, 6, 2, 10], light, light);
    piece.box_at([9, 1, 10, 9, 2, 10], light, light);
    piece.box_at([5, 2, 5, 5, 6, 5], light, light);
    piece.box_at([5, 2, 10, 5, 6, 10], light, light);
    piece.box_at([10, 2, 5, 10, 6, 5], light, light);
    piece.box_at([10, 2, 10, 10, 6, 10], light, light);
    piece.box_at([5, 7, 1, 5, 7, 6], light, light);
    piece.box_at([10, 7, 1, 10, 7, 6], light, light);
    piece.box_at([5, 7, 9, 5, 7, 14], light, light);
    piece.box_at([10, 7, 9, 10, 7, 14], light, light);
    piece.box_at([1, 7, 5, 6, 7, 5], light, light);
    piece.box_at([1, 7, 10, 6, 7, 10], light, light);
    piece.box_at([9, 7, 5, 14, 7, 5], light, light);
    piece.box_at([9, 7, 10, 14, 7, 10], light, light);
    piece.box_at([2, 1, 2, 2, 1, 3], light, light);
    piece.box_at([3, 1, 2, 3, 1, 2], light, light);
    piece.box_at([13, 1, 2, 13, 1, 3], light, light);
    piece.box_at([12, 1, 2, 12, 1, 2], light, light);
    piece.box_at([2, 1, 12, 2, 1, 13], light, light);
    piece.box_at([3, 1, 13, 3, 1, 13], light, light);
    piece.box_at([13, 1, 12, 13, 1, 13], light, light);
    piece.box_at([12, 1, 13, 12, 1, 13], light, light);
}

/// `RoomDefinition.countOpenings`。
///
/// `RoomDefinition.countOpenings`.
fn count_openings(piece: &MonumentPiece<'_>, room: i32) -> i32 {
    (0..6).filter(|&dir| opening(piece, room, dir)).count() as i32
}

// __SIMPLE__

/// `OceanMonumentSimpleRoom.postProcess`（`mainDesign` 来自构造器）。
///
/// `OceanMonumentSimpleRoom.postProcess` with the constructor's `mainDesign`.
fn simple_room(piece: &mut MonumentPiece<'_>, main_design: i32, random: &WorldgenRandom) {
    let gray = base_gray();
    let light = base_light();
    let black = base_black();
    let lamp = lamp();
    let room = own_room(piece);
    if room / 25 > 0 {
        let down = opening(piece, room, D_DOWN);
        piece.default_floor(0, 0, down);
    }
    if connection(piece, room, D_UP).is_none() {
        piece.box_on_fill_only([1, 4, 1, 6, 4, 6], gray);
    }

    // `centerPillar` 的 `random.nextBoolean()` 只在 `mainDesign != 0` 时才取（短路与）。
    // `centerPillar`'s `random.nextBoolean()` is only drawn when `mainDesign != 0`
    // (short-circuit and).
    let center_pillar = main_design != 0
        && random.next_boolean()
        && !opening(piece, room, D_DOWN)
        && !opening(piece, room, D_UP)
        && count_openings(piece, room) > 1;

    match main_design {
        0 => {
            piece.box_at([0, 1, 0, 2, 1, 2], light, light);
            piece.box_at([0, 3, 0, 2, 3, 2], light, light);
            piece.box_at([0, 2, 0, 0, 2, 2], gray, gray);
            piece.box_at([1, 2, 0, 2, 2, 0], gray, gray);
            piece.place(lamp, 1, 2, 1);
            piece.box_at([5, 1, 0, 7, 1, 2], light, light);
            piece.box_at([5, 3, 0, 7, 3, 2], light, light);
            piece.box_at([7, 2, 0, 7, 2, 2], gray, gray);
            piece.box_at([5, 2, 0, 6, 2, 0], gray, gray);
            piece.place(lamp, 6, 2, 1);
            piece.box_at([0, 1, 5, 2, 1, 7], light, light);
            piece.box_at([0, 3, 5, 2, 3, 7], light, light);
            piece.box_at([0, 2, 5, 0, 2, 7], gray, gray);
            piece.box_at([1, 2, 7, 2, 2, 7], gray, gray);
            piece.place(lamp, 1, 2, 6);
            piece.box_at([5, 1, 5, 7, 1, 7], light, light);
            piece.box_at([5, 3, 5, 7, 3, 7], light, light);
            piece.box_at([7, 2, 5, 7, 2, 7], gray, gray);
            piece.box_at([5, 2, 7, 6, 2, 7], gray, gray);
            piece.place(lamp, 6, 2, 6);
            if opening(piece, room, D_SOUTH) {
                piece.box_at([3, 3, 0, 4, 3, 0], light, light);
            } else {
                piece.box_at([3, 3, 0, 4, 3, 1], light, light);
                piece.box_at([3, 2, 0, 4, 2, 0], gray, gray);
                piece.box_at([3, 1, 0, 4, 1, 1], light, light);
            }
            if opening(piece, room, D_NORTH) {
                piece.box_at([3, 3, 7, 4, 3, 7], light, light);
            } else {
                piece.box_at([3, 3, 6, 4, 3, 7], light, light);
                piece.box_at([3, 2, 7, 4, 2, 7], gray, gray);
                piece.box_at([3, 1, 6, 4, 1, 7], light, light);
            }
            if opening(piece, room, D_WEST) {
                piece.box_at([0, 3, 3, 0, 3, 4], light, light);
            } else {
                piece.box_at([0, 3, 3, 1, 3, 4], light, light);
                piece.box_at([0, 2, 3, 0, 2, 4], gray, gray);
                piece.box_at([0, 1, 3, 1, 1, 4], light, light);
            }
            if opening(piece, room, D_EAST) {
                piece.box_at([7, 3, 3, 7, 3, 4], light, light);
            } else {
                piece.box_at([6, 3, 3, 7, 3, 4], light, light);
                piece.box_at([7, 2, 3, 7, 2, 4], gray, gray);
                piece.box_at([6, 1, 3, 7, 1, 4], light, light);
            }
        }
        1 => {
            piece.box_at([2, 1, 2, 2, 3, 2], light, light);
            piece.box_at([2, 1, 5, 2, 3, 5], light, light);
            piece.box_at([5, 1, 5, 5, 3, 5], light, light);
            piece.box_at([5, 1, 2, 5, 3, 2], light, light);
            piece.place(lamp, 2, 2, 2);
            piece.place(lamp, 2, 2, 5);
            piece.place(lamp, 5, 2, 5);
            piece.place(lamp, 5, 2, 2);
            piece.box_at([0, 1, 0, 1, 3, 0], light, light);
            piece.box_at([0, 1, 1, 0, 3, 1], light, light);
            piece.box_at([0, 1, 7, 1, 3, 7], light, light);
            piece.box_at([0, 1, 6, 0, 3, 6], light, light);
            piece.box_at([6, 1, 7, 7, 3, 7], light, light);
            piece.box_at([7, 1, 6, 7, 3, 6], light, light);
            piece.box_at([6, 1, 0, 7, 3, 0], light, light);
            piece.box_at([7, 1, 1, 7, 3, 1], light, light);
            piece.place(gray, 1, 2, 0);
            piece.place(gray, 0, 2, 1);
            piece.place(gray, 1, 2, 7);
            piece.place(gray, 0, 2, 6);
            piece.place(gray, 6, 2, 7);
            piece.place(gray, 7, 2, 6);
            piece.place(gray, 6, 2, 0);
            piece.place(gray, 7, 2, 1);
            if !opening(piece, room, D_SOUTH) {
                piece.box_at([1, 3, 0, 6, 3, 0], light, light);
                piece.box_at([1, 2, 0, 6, 2, 0], gray, gray);
                piece.box_at([1, 1, 0, 6, 1, 0], light, light);
            }
            if !opening(piece, room, D_NORTH) {
                piece.box_at([1, 3, 7, 6, 3, 7], light, light);
                piece.box_at([1, 2, 7, 6, 2, 7], gray, gray);
                piece.box_at([1, 1, 7, 6, 1, 7], light, light);
            }
            if !opening(piece, room, D_WEST) {
                piece.box_at([0, 3, 1, 0, 3, 6], light, light);
                piece.box_at([0, 2, 1, 0, 2, 6], gray, gray);
                piece.box_at([0, 1, 1, 0, 1, 6], light, light);
            }
            if !opening(piece, room, D_EAST) {
                piece.box_at([7, 3, 1, 7, 3, 6], light, light);
                piece.box_at([7, 2, 1, 7, 2, 6], gray, gray);
                piece.box_at([7, 1, 1, 7, 1, 6], light, light);
            }
        }
        _ => {
            piece.box_at([0, 1, 0, 0, 1, 7], light, light);
            piece.box_at([7, 1, 0, 7, 1, 7], light, light);
            piece.box_at([1, 1, 0, 6, 1, 0], light, light);
            piece.box_at([1, 1, 7, 6, 1, 7], light, light);
            piece.box_at([0, 2, 0, 0, 2, 7], black, black);
            piece.box_at([7, 2, 0, 7, 2, 7], black, black);
            piece.box_at([1, 2, 0, 6, 2, 0], black, black);
            piece.box_at([1, 2, 7, 6, 2, 7], black, black);
            piece.box_at([0, 3, 0, 0, 3, 7], light, light);
            piece.box_at([7, 3, 0, 7, 3, 7], light, light);
            piece.box_at([1, 3, 0, 6, 3, 0], light, light);
            piece.box_at([1, 3, 7, 6, 3, 7], light, light);
            piece.box_at([0, 1, 3, 0, 2, 4], black, black);
            piece.box_at([7, 1, 3, 7, 2, 4], black, black);
            piece.box_at([3, 1, 0, 4, 2, 0], black, black);
            piece.box_at([3, 1, 7, 4, 2, 7], black, black);
            if opening(piece, room, D_SOUTH) {
                piece.water_box(3, 1, 0, 4, 2, 0);
            }
            if opening(piece, room, D_NORTH) {
                piece.water_box(3, 1, 7, 4, 2, 7);
            }
            if opening(piece, room, D_WEST) {
                piece.water_box(0, 1, 3, 0, 2, 4);
            }
            if opening(piece, room, D_EAST) {
                piece.water_box(7, 1, 3, 7, 2, 4);
            }
        }
    }

    if center_pillar {
        piece.box_at([3, 1, 3, 4, 1, 4], light, light);
        piece.box_at([3, 2, 3, 4, 2, 4], gray, gray);
        piece.box_at([3, 3, 3, 4, 3, 4], light, light);
    }
}

/// `OceanMonumentSimpleTopRoom.postProcess`：湿海绵房间的随机取数。
///
/// `OceanMonumentSimpleTopRoom.postProcess`: the wet-sponge room draws.
fn simple_top_room(piece: &mut MonumentPiece<'_>, random: &WorldgenRandom) {
    let gray = base_gray();
    let light = base_light();
    let black = base_black();
    let room = own_room(piece);
    if room / 25 > 0 {
        let down = opening(piece, room, D_DOWN);
        piece.default_floor(0, 0, down);
    }
    if connection(piece, room, D_UP).is_none() {
        piece.box_on_fill_only([1, 4, 1, 6, 4, 6], gray);
    }

    // 每个 (x, z) 取一次 `nextInt(3)`；非 0 时再取一次 `nextInt(4)` 决定 `y0`。
    // One `nextInt(3)` per (x, z); when non-zero another `nextInt(4)` picks `y0`.
    let sponge = crate::structure::piece_place::block("minecraft:wet_sponge");
    for x in 1..=6 {
        for z in 1..=6 {
            if random.next_int(3) != 0 {
                let y0 = 2 + i32::from(random.next_int(4) != 0);
                piece.box_at([x, y0, z, x, 3, z], sponge, sponge);
            }
        }
    }

    piece.box_at([0, 1, 0, 0, 1, 7], light, light);
    piece.box_at([7, 1, 0, 7, 1, 7], light, light);
    piece.box_at([1, 1, 0, 6, 1, 0], light, light);
    piece.box_at([1, 1, 7, 6, 1, 7], light, light);
    piece.box_at([0, 2, 0, 0, 2, 7], black, black);
    piece.box_at([7, 2, 0, 7, 2, 7], black, black);
    piece.box_at([1, 2, 0, 6, 2, 0], black, black);
    piece.box_at([1, 2, 7, 6, 2, 7], black, black);
    piece.box_at([0, 3, 0, 0, 3, 7], light, light);
    piece.box_at([7, 3, 0, 7, 3, 7], light, light);
    piece.box_at([1, 3, 0, 6, 3, 0], light, light);
    piece.box_at([1, 3, 7, 6, 3, 7], light, light);
    piece.box_at([0, 1, 3, 0, 2, 4], black, black);
    piece.box_at([7, 1, 3, 7, 2, 4], black, black);
    piece.box_at([3, 1, 0, 4, 2, 0], black, black);
    piece.box_at([3, 1, 7, 4, 2, 7], black, black);
    if opening(piece, room, D_SOUTH) {
        piece.water_box(3, 1, 0, 4, 2, 0);
    }
}

// __WING__

/// `OceanMonumentWingRoom.postProcess`（`mainDesign` 来自构造器的 `randomValue & 1`）。
///
/// `OceanMonumentWingRoom.postProcess` with the constructor's `randomValue & 1`.
fn wing_room(piece: &mut MonumentPiece<'_>, main_design: i32) {
    let light = base_light();
    let black = base_black();
    let lamp = lamp();
    if main_design == 0 {
        for i in 0..4 {
            piece.box_at([10 - i, 3 - i, 20 - i, 12 + i, 3 - i, 20], light, light);
        }
        piece.box_at([7, 0, 6, 15, 0, 16], light, light);
        piece.box_at([6, 0, 6, 6, 3, 20], light, light);
        piece.box_at([16, 0, 6, 16, 3, 20], light, light);
        piece.box_at([7, 1, 7, 7, 1, 20], light, light);
        piece.box_at([15, 1, 7, 15, 1, 20], light, light);
        piece.box_at([7, 1, 6, 9, 3, 6], light, light);
        piece.box_at([13, 1, 6, 15, 3, 6], light, light);
        piece.box_at([8, 1, 7, 9, 1, 7], light, light);
        piece.box_at([13, 1, 7, 14, 1, 7], light, light);
        piece.box_at([9, 0, 5, 13, 0, 5], light, light);
        piece.box_at([10, 0, 7, 12, 0, 7], black, black);
        piece.box_at([8, 0, 10, 8, 0, 12], black, black);
        piece.box_at([14, 0, 10, 14, 0, 12], black, black);

        let mut z = 18;
        while z >= 7 {
            piece.place(lamp, 6, 3, z);
            piece.place(lamp, 16, 3, z);
            z -= 3;
        }

        piece.place(lamp, 10, 0, 10);
        piece.place(lamp, 12, 0, 10);
        piece.place(lamp, 10, 0, 12);
        piece.place(lamp, 12, 0, 12);
        piece.place(lamp, 8, 3, 6);
        piece.place(lamp, 14, 3, 6);
        piece.place(light, 4, 2, 4);
        piece.place(lamp, 4, 1, 4);
        piece.place(light, 4, 0, 4);
        piece.place(light, 18, 2, 4);
        piece.place(lamp, 18, 1, 4);
        piece.place(light, 18, 0, 4);
        piece.place(light, 4, 2, 18);
        piece.place(lamp, 4, 1, 18);
        piece.place(light, 4, 0, 18);
        piece.place(light, 18, 2, 18);
        piece.place(lamp, 18, 1, 18);
        piece.place(light, 18, 0, 18);
        piece.place(light, 9, 7, 20);
        piece.place(light, 13, 7, 20);
        piece.box_at([6, 0, 21, 7, 4, 21], light, light);
        piece.box_at([15, 0, 21, 16, 4, 21], light, light);
        piece.spawn_elder(11, 2, 16);
    } else if main_design == 1 {
        piece.box_at([9, 3, 18, 13, 3, 20], light, light);
        piece.box_at([9, 0, 18, 9, 2, 18], light, light);
        piece.box_at([13, 0, 18, 13, 2, 18], light, light);
        let mut x = 9;
        for _ in 0..2 {
            piece.place(light, x, 6, 20);
            piece.place(lamp, x, 5, 20);
            piece.place(light, x, 4, 20);
            x = 13;
        }

        piece.box_at([7, 3, 7, 15, 3, 14], light, light);
        let mut pos = 10;
        for _ in 0..2 {
            piece.box_at([pos, 0, 10, pos, 6, 10], light, light);
            piece.box_at([pos, 0, 12, pos, 6, 12], light, light);
            piece.place(lamp, pos, 0, 10);
            piece.place(lamp, pos, 0, 12);
            piece.place(lamp, pos, 4, 10);
            piece.place(lamp, pos, 4, 12);
            pos = 12;
        }

        let mut pos = 8;
        for _ in 0..2 {
            piece.box_at([pos, 0, 7, pos, 2, 7], light, light);
            piece.box_at([pos, 0, 14, pos, 2, 14], light, light);
            pos = 14;
        }

        piece.box_at([8, 3, 8, 8, 3, 13], black, black);
        piece.box_at([14, 3, 8, 14, 3, 13], black, black);
        piece.spawn_elder(11, 5, 13);
    }
}

/// `OceanMonumentPenthouse.postProcess`。
///
/// `OceanMonumentPenthouse.postProcess`.
fn penthouse(piece: &mut MonumentPiece<'_>) {
    let gray = base_gray();
    let light = base_light();
    let black = base_black();
    let lamp = lamp();
    piece.box_at([2, -1, 2, 11, -1, 11], light, light);
    piece.box_at([0, -1, 0, 1, -1, 11], gray, gray);
    piece.box_at([12, -1, 0, 13, -1, 11], gray, gray);
    piece.box_at([2, -1, 0, 11, -1, 1], gray, gray);
    piece.box_at([2, -1, 12, 11, -1, 13], gray, gray);
    piece.box_at([0, 0, 0, 0, 0, 13], light, light);
    piece.box_at([13, 0, 0, 13, 0, 13], light, light);
    piece.box_at([1, 0, 0, 12, 0, 0], light, light);
    piece.box_at([1, 0, 13, 12, 0, 13], light, light);

    let mut i = 2;
    while i <= 11 {
        piece.place(lamp, 0, 0, i);
        piece.place(lamp, 13, 0, i);
        piece.place(lamp, i, 0, 0);
        i += 3;
    }

    piece.box_at([2, 0, 3, 4, 0, 9], light, light);
    piece.box_at([9, 0, 3, 11, 0, 9], light, light);
    piece.box_at([4, 0, 9, 9, 0, 11], light, light);
    piece.place(light, 5, 0, 8);
    piece.place(light, 8, 0, 8);
    piece.place(light, 10, 0, 10);
    piece.place(light, 3, 0, 10);
    piece.box_at([3, 0, 3, 3, 0, 7], black, black);
    piece.box_at([10, 0, 3, 10, 0, 7], black, black);
    piece.box_at([6, 0, 10, 7, 0, 10], black, black);
    let mut x = 3;
    for _ in 0..2 {
        let mut z = 2;
        while z <= 8 {
            piece.box_at([x, 0, z, x, 2, z], light, light);
            z += 3;
        }
        x = 10;
    }

    piece.box_at([5, 0, 10, 5, 2, 10], light, light);
    piece.box_at([8, 0, 10, 8, 2, 10], light, light);
    piece.box_at([6, -1, 7, 7, -1, 8], black, black);
    piece.water_box(6, -1, 3, 7, -1, 4);
    piece.spawn_elder(6, 1, 6);
}
