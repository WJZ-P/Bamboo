//! 海底神殿双房间结构件的 `postProcess`：`OceanMonumentDoubleXRoom` /
//! `DoubleXYRoom` / `DoubleYRoom` / `DoubleYZRoom` / `DoubleZRoom`。
//!
//! 逐行照官方 `OceanMonumentPieces` 翻译。房间的墙 / 地板 / 开口由房间图的
//! `connections` / `hasOpening` 决定，所以每个函数都从 `MonumentPiece.rooms` 里查。
//!
//! `postProcess` of the ocean monument double rooms, translated line by line from
//! vanilla `OceanMonumentPieces`. Walls, floors and openings come from the room graph's
//! `connections` / `hasOpening`, looked up through `MonumentPiece.rooms`.

use super::monument_rooms::{D_DOWN, D_EAST, D_NORTH, D_SOUTH, D_UP, D_WEST};
use super::ocean_monument_place::{
    MonumentPiece, base_black, base_gray, base_light, connection, lamp, opening, own_room, room_of,
};

/// `OceanMonumentDoubleXRoom.postProcess`。
///
/// `OceanMonumentDoubleXRoom.postProcess`.
pub(super) fn double_x_room(piece: &mut MonumentPiece<'_>) {
    let gray = base_gray();
    let light = base_light();
    let lamp = lamp();
    let room = own_room(piece);
    let east = room_of(piece, room).connections[D_EAST];
    let west = room;
    if room / 25 > 0 {
        let down = opening(piece, east, D_DOWN);
        piece.default_floor(8, 0, down);
        let down = opening(piece, west, D_DOWN);
        piece.default_floor(0, 0, down);
    }
    if connection(piece, west, D_UP).is_none() {
        piece.box_on_fill_only(1, 4, 1, 7, 4, 6, gray);
    }
    if connection(piece, east, D_UP).is_none() {
        piece.box_on_fill_only(8, 4, 1, 14, 4, 6, gray);
    }

    piece.box_at(0, 3, 0, 0, 3, 7, light, light);
    piece.box_at(15, 3, 0, 15, 3, 7, light, light);
    piece.box_at(1, 3, 0, 15, 3, 0, light, light);
    piece.box_at(1, 3, 7, 14, 3, 7, light, light);
    piece.box_at(0, 2, 0, 0, 2, 7, gray, gray);
    piece.box_at(15, 2, 0, 15, 2, 7, gray, gray);
    piece.box_at(1, 2, 0, 15, 2, 0, gray, gray);
    piece.box_at(1, 2, 7, 14, 2, 7, gray, gray);
    piece.box_at(0, 1, 0, 0, 1, 7, light, light);
    piece.box_at(15, 1, 0, 15, 1, 7, light, light);
    piece.box_at(1, 1, 0, 15, 1, 0, light, light);
    piece.box_at(1, 1, 7, 14, 1, 7, light, light);
    piece.box_at(5, 1, 0, 10, 1, 4, light, light);
    piece.box_at(6, 2, 0, 9, 2, 3, gray, gray);
    piece.box_at(5, 3, 0, 10, 3, 4, light, light);
    piece.place(lamp, 6, 2, 3);
    piece.place(lamp, 9, 2, 3);
    if opening(piece, west, D_SOUTH) {
        piece.water_box(3, 1, 0, 4, 2, 0);
    }
    if opening(piece, west, D_NORTH) {
        piece.water_box(3, 1, 7, 4, 2, 7);
    }
    if opening(piece, west, D_WEST) {
        piece.water_box(0, 1, 3, 0, 2, 4);
    }
    if opening(piece, east, D_SOUTH) {
        piece.water_box(11, 1, 0, 12, 2, 0);
    }
    if opening(piece, east, D_NORTH) {
        piece.water_box(11, 1, 7, 12, 2, 7);
    }
    if opening(piece, east, D_EAST) {
        piece.water_box(15, 1, 3, 15, 2, 4);
    }
}

/// `OceanMonumentDoubleXYRoom.postProcess`。
///
/// `OceanMonumentDoubleXYRoom.postProcess`.
pub(super) fn double_xy_room(piece: &mut MonumentPiece<'_>) {
    let gray = base_gray();
    let light = base_light();
    let lamp = lamp();
    let room = own_room(piece);
    let east = room_of(piece, room).connections[D_EAST];
    let west = room;
    let west_up = room_of(piece, west).connections[D_UP];
    let east_up = room_of(piece, east).connections[D_UP];
    if room / 25 > 0 {
        let down = opening(piece, east, D_DOWN);
        piece.default_floor(8, 0, down);
        let down = opening(piece, west, D_DOWN);
        piece.default_floor(0, 0, down);
    }
    if connection(piece, west_up, D_UP).is_none() {
        piece.box_on_fill_only(1, 8, 1, 7, 8, 6, gray);
    }
    if connection(piece, east_up, D_UP).is_none() {
        piece.box_on_fill_only(8, 8, 1, 14, 8, 6, gray);
    }

    for y in 1..=7 {
        let mut block = light;
        if y == 2 || y == 6 {
            block = gray;
        }
        piece.box_at(0, y, 0, 0, y, 7, block, block);
        piece.box_at(15, y, 0, 15, y, 7, block, block);
        piece.box_at(1, y, 0, 15, y, 0, block, block);
        piece.box_at(1, y, 7, 14, y, 7, block, block);
    }

    piece.box_at(2, 1, 3, 2, 7, 4, light, light);
    piece.box_at(3, 1, 2, 4, 7, 2, light, light);
    piece.box_at(3, 1, 5, 4, 7, 5, light, light);
    piece.box_at(13, 1, 3, 13, 7, 4, light, light);
    piece.box_at(11, 1, 2, 12, 7, 2, light, light);
    piece.box_at(11, 1, 5, 12, 7, 5, light, light);
    piece.box_at(5, 1, 3, 5, 3, 4, light, light);
    piece.box_at(10, 1, 3, 10, 3, 4, light, light);
    piece.box_at(5, 7, 2, 10, 7, 5, light, light);
    piece.box_at(5, 5, 2, 5, 7, 2, light, light);
    piece.box_at(10, 5, 2, 10, 7, 2, light, light);
    piece.box_at(5, 5, 5, 5, 7, 5, light, light);
    piece.box_at(10, 5, 5, 10, 7, 5, light, light);
    piece.place(light, 6, 6, 2);
    piece.place(light, 9, 6, 2);
    piece.place(light, 6, 6, 5);
    piece.place(light, 9, 6, 5);
    piece.box_at(5, 4, 3, 6, 4, 4, light, light);
    piece.box_at(9, 4, 3, 10, 4, 4, light, light);
    piece.place(lamp, 5, 4, 2);
    piece.place(lamp, 5, 4, 5);
    piece.place(lamp, 10, 4, 2);
    piece.place(lamp, 10, 4, 5);
    if opening(piece, west, D_SOUTH) {
        piece.water_box(3, 1, 0, 4, 2, 0);
    }
    if opening(piece, west, D_NORTH) {
        piece.water_box(3, 1, 7, 4, 2, 7);
    }
    if opening(piece, west, D_WEST) {
        piece.water_box(0, 1, 3, 0, 2, 4);
    }
    if opening(piece, east, D_SOUTH) {
        piece.water_box(11, 1, 0, 12, 2, 0);
    }
    if opening(piece, east, D_NORTH) {
        piece.water_box(11, 1, 7, 12, 2, 7);
    }
    if opening(piece, east, D_EAST) {
        piece.water_box(15, 1, 3, 15, 2, 4);
    }
    if opening(piece, west_up, D_SOUTH) {
        piece.water_box(3, 5, 0, 4, 6, 0);
    }
    if opening(piece, west_up, D_NORTH) {
        piece.water_box(3, 5, 7, 4, 6, 7);
    }
    if opening(piece, west_up, D_WEST) {
        piece.water_box(0, 5, 3, 0, 6, 4);
    }
    if opening(piece, east_up, D_SOUTH) {
        piece.water_box(11, 5, 0, 12, 6, 0);
    }
    if opening(piece, east_up, D_NORTH) {
        piece.water_box(11, 5, 7, 12, 6, 7);
    }
    if opening(piece, east_up, D_EAST) {
        piece.water_box(15, 5, 3, 15, 6, 4);
    }
}

// __DYZ__

/// `OceanMonumentDoubleYRoom.postProcess`。
///
/// `OceanMonumentDoubleYRoom.postProcess`.
pub(super) fn double_y_room(piece: &mut MonumentPiece<'_>) {
    let gray = base_gray();
    let light = base_light();
    let room = own_room(piece);
    if room / 25 > 0 {
        let down = opening(piece, room, D_DOWN);
        piece.default_floor(0, 0, down);
    }

    let above = room_of(piece, room).connections[D_UP];
    if connection(piece, above, D_UP).is_none() {
        piece.box_on_fill_only(1, 8, 1, 6, 8, 6, gray);
    }

    piece.box_at(0, 4, 0, 0, 4, 7, light, light);
    piece.box_at(7, 4, 0, 7, 4, 7, light, light);
    piece.box_at(1, 4, 0, 6, 4, 0, light, light);
    piece.box_at(1, 4, 7, 6, 4, 7, light, light);
    piece.box_at(2, 4, 1, 2, 4, 2, light, light);
    piece.box_at(1, 4, 2, 1, 4, 2, light, light);
    piece.box_at(5, 4, 1, 5, 4, 2, light, light);
    piece.box_at(6, 4, 2, 6, 4, 2, light, light);
    piece.box_at(2, 4, 5, 2, 4, 6, light, light);
    piece.box_at(1, 4, 5, 1, 4, 5, light, light);
    piece.box_at(5, 4, 5, 5, 4, 6, light, light);
    piece.box_at(6, 4, 5, 6, 4, 5, light, light);
    let mut definition = room;

    for y in [1, 5] {
        let z = 0;
        if opening(piece, definition, D_SOUTH) {
            piece.box_at(2, y, z, 2, y + 2, z, light, light);
            piece.box_at(5, y, z, 5, y + 2, z, light, light);
            piece.box_at(3, y + 2, z, 4, y + 2, z, light, light);
        } else {
            piece.box_at(0, y, z, 7, y + 2, z, light, light);
            piece.box_at(0, y + 1, z, 7, y + 1, z, gray, gray);
        }

        if opening(piece, definition, D_NORTH) {
            piece.box_at(2, y, 7, 2, y + 2, 7, light, light);
            piece.box_at(5, y, 7, 5, y + 2, 7, light, light);
            piece.box_at(3, y + 2, 7, 4, y + 2, 7, light, light);
        } else {
            piece.box_at(0, y, 7, 7, y + 2, 7, light, light);
            piece.box_at(0, y + 1, 7, 7, y + 1, 7, gray, gray);
        }

        if opening(piece, definition, D_WEST) {
            piece.box_at(0, y, 2, 0, y + 2, 2, light, light);
            piece.box_at(0, y, 5, 0, y + 2, 5, light, light);
            piece.box_at(0, y + 2, 3, 0, y + 2, 4, light, light);
        } else {
            piece.box_at(0, y, 0, 0, y + 2, 7, light, light);
            piece.box_at(0, y + 1, 0, 0, y + 1, 7, gray, gray);
        }

        if opening(piece, definition, D_EAST) {
            piece.box_at(7, y, 2, 7, y + 2, 2, light, light);
            piece.box_at(7, y, 5, 7, y + 2, 5, light, light);
            piece.box_at(7, y + 2, 3, 7, y + 2, 4, light, light);
        } else {
            piece.box_at(7, y, 0, 7, y + 2, 7, light, light);
            piece.box_at(7, y + 1, 0, 7, y + 1, 7, gray, gray);
        }

        definition = above;
    }
}

/// `OceanMonumentDoubleYZRoom.postProcess`。
///
/// `OceanMonumentDoubleYZRoom.postProcess`.
pub(super) fn double_yz_room(piece: &mut MonumentPiece<'_>) {
    let gray = base_gray();
    let light = base_light();
    let black = base_black();
    let lamp = lamp();
    let room = own_room(piece);
    let north = room_of(piece, room).connections[D_NORTH];
    let south = room;
    let north_up = room_of(piece, north).connections[D_UP];
    let south_up = room_of(piece, south).connections[D_UP];
    if room / 25 > 0 {
        let down = opening(piece, north, D_DOWN);
        piece.default_floor(0, 8, down);
        let down = opening(piece, south, D_DOWN);
        piece.default_floor(0, 0, down);
    }
    if connection(piece, south_up, D_UP).is_none() {
        piece.box_on_fill_only(1, 8, 1, 6, 8, 7, gray);
    }
    if connection(piece, north_up, D_UP).is_none() {
        piece.box_on_fill_only(1, 8, 8, 6, 8, 14, gray);
    }

    for y in 1..=7 {
        let mut block = light;
        if y == 2 || y == 6 {
            block = gray;
        }
        piece.box_at(0, y, 0, 0, y, 15, block, block);
        piece.box_at(7, y, 0, 7, y, 15, block, block);
        piece.box_at(1, y, 0, 6, y, 0, block, block);
        piece.box_at(1, y, 15, 6, y, 15, block, block);
    }

    for y in 1..=7 {
        let mut block = black;
        if y == 2 || y == 6 {
            block = lamp;
        }
        piece.box_at(3, y, 7, 4, y, 8, block, block);
    }

    if opening(piece, south, D_SOUTH) {
        piece.water_box(3, 1, 0, 4, 2, 0);
    }
    if opening(piece, south, D_EAST) {
        piece.water_box(7, 1, 3, 7, 2, 4);
    }
    if opening(piece, south, D_WEST) {
        piece.water_box(0, 1, 3, 0, 2, 4);
    }
    if opening(piece, north, D_NORTH) {
        piece.water_box(3, 1, 15, 4, 2, 15);
    }
    if opening(piece, north, D_WEST) {
        piece.water_box(0, 1, 11, 0, 2, 12);
    }
    if opening(piece, north, D_EAST) {
        piece.water_box(7, 1, 11, 7, 2, 12);
    }
    if opening(piece, south_up, D_SOUTH) {
        piece.water_box(3, 5, 0, 4, 6, 0);
    }
    if opening(piece, south_up, D_EAST) {
        piece.water_box(7, 5, 3, 7, 6, 4);
        piece.box_at(5, 4, 2, 6, 4, 5, light, light);
        piece.box_at(6, 1, 2, 6, 3, 2, light, light);
        piece.box_at(6, 1, 5, 6, 3, 5, light, light);
    }
    if opening(piece, south_up, D_WEST) {
        piece.water_box(0, 5, 3, 0, 6, 4);
        piece.box_at(1, 4, 2, 2, 4, 5, light, light);
        piece.box_at(1, 1, 2, 1, 3, 2, light, light);
        piece.box_at(1, 1, 5, 1, 3, 5, light, light);
    }
    if opening(piece, north_up, D_NORTH) {
        piece.water_box(3, 5, 15, 4, 6, 15);
    }
    if opening(piece, north_up, D_WEST) {
        piece.water_box(0, 5, 11, 0, 6, 12);
        piece.box_at(1, 4, 10, 2, 4, 13, light, light);
        piece.box_at(1, 1, 10, 1, 3, 10, light, light);
        piece.box_at(1, 1, 13, 1, 3, 13, light, light);
    }
    if opening(piece, north_up, D_EAST) {
        piece.water_box(7, 5, 11, 7, 6, 12);
        piece.box_at(5, 4, 10, 6, 4, 13, light, light);
        piece.box_at(6, 1, 10, 6, 3, 10, light, light);
        piece.box_at(6, 1, 13, 6, 3, 13, light, light);
    }
}

/// `OceanMonumentDoubleZRoom.postProcess`。
///
/// `OceanMonumentDoubleZRoom.postProcess`.
pub(super) fn double_z_room(piece: &mut MonumentPiece<'_>) {
    let gray = base_gray();
    let light = base_light();
    let lamp = lamp();
    let room = own_room(piece);
    let north = room_of(piece, room).connections[D_NORTH];
    let south = room;
    if room / 25 > 0 {
        let down = opening(piece, north, D_DOWN);
        piece.default_floor(0, 8, down);
        let down = opening(piece, south, D_DOWN);
        piece.default_floor(0, 0, down);
    }
    if connection(piece, south, D_UP).is_none() {
        piece.box_on_fill_only(1, 4, 1, 6, 4, 7, gray);
    }
    if connection(piece, north, D_UP).is_none() {
        piece.box_on_fill_only(1, 4, 8, 6, 4, 14, gray);
    }

    piece.box_at(0, 3, 0, 0, 3, 15, light, light);
    piece.box_at(7, 3, 0, 7, 3, 15, light, light);
    piece.box_at(1, 3, 0, 7, 3, 0, light, light);
    piece.box_at(1, 3, 15, 6, 3, 15, light, light);
    piece.box_at(0, 2, 0, 0, 2, 15, gray, gray);
    piece.box_at(7, 2, 0, 7, 2, 15, gray, gray);
    piece.box_at(1, 2, 0, 7, 2, 0, gray, gray);
    piece.box_at(1, 2, 15, 6, 2, 15, gray, gray);
    piece.box_at(0, 1, 0, 0, 1, 15, light, light);
    piece.box_at(7, 1, 0, 7, 1, 15, light, light);
    piece.box_at(1, 1, 0, 7, 1, 0, light, light);
    piece.box_at(1, 1, 15, 6, 1, 15, light, light);
    piece.box_at(1, 1, 1, 1, 1, 2, light, light);
    piece.box_at(6, 1, 1, 6, 1, 2, light, light);
    piece.box_at(1, 3, 1, 1, 3, 2, light, light);
    piece.box_at(6, 3, 1, 6, 3, 2, light, light);
    piece.box_at(1, 1, 13, 1, 1, 14, light, light);
    piece.box_at(6, 1, 13, 6, 1, 14, light, light);
    piece.box_at(1, 3, 13, 1, 3, 14, light, light);
    piece.box_at(6, 3, 13, 6, 3, 14, light, light);
    piece.box_at(2, 1, 6, 2, 3, 6, light, light);
    piece.box_at(5, 1, 6, 5, 3, 6, light, light);
    piece.box_at(2, 1, 9, 2, 3, 9, light, light);
    piece.box_at(5, 1, 9, 5, 3, 9, light, light);
    piece.box_at(3, 2, 6, 4, 2, 6, light, light);
    piece.box_at(3, 2, 9, 4, 2, 9, light, light);
    piece.box_at(2, 2, 7, 2, 2, 8, light, light);
    piece.box_at(5, 2, 7, 5, 2, 8, light, light);
    piece.place(lamp, 2, 2, 5);
    piece.place(lamp, 5, 2, 5);
    piece.place(lamp, 2, 2, 10);
    piece.place(lamp, 5, 2, 10);
    piece.place(light, 2, 3, 5);
    piece.place(light, 5, 3, 5);
    piece.place(light, 2, 3, 10);
    piece.place(light, 5, 3, 10);
    if opening(piece, south, D_SOUTH) {
        piece.water_box(3, 1, 0, 4, 2, 0);
    }
    if opening(piece, south, D_EAST) {
        piece.water_box(7, 1, 3, 7, 2, 4);
    }
    if opening(piece, south, D_WEST) {
        piece.water_box(0, 1, 3, 0, 2, 4);
    }
    if opening(piece, north, D_NORTH) {
        piece.water_box(3, 1, 15, 4, 2, 15);
    }
    if opening(piece, north, D_WEST) {
        piece.water_box(0, 1, 11, 0, 2, 12);
    }
    if opening(piece, north, D_EAST) {
        piece.water_box(7, 1, 11, 7, 2, 12);
    }
}
