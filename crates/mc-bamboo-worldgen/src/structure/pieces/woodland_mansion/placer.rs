//! `WoodlandMansionPieces.MansionPiecePlacer.createMansion` 及全部辅助方法。
//!
//! 逐分支照官方翻译：入口、外墙遍历（含 `PlacementData` 的可变 position/rotation）、
//! 屋顶、三层走廊地毯、房间墙/门、三类房间名与门朝向随机、`addRoom1x1/1x2/2x2`。
//! 每个 `WoodlandMansionPiece` 记模板名 / 位置 / rotation / mirror。
//!
//! `WoodlandMansionPieces.MansionPiecePlacer.createMansion` and all helpers, translated
//! branch by branch: entrance, outer-wall traversal (mutable `PlacementData`), roof,
//! corridor carpets, room walls/doors, room-collection and door-direction randoms,
//! `addRoom1x1/1x2/2x2`. Each piece records template name / position / rotation / mirror.

use crate::random::WorldgenRandom;

use super::grid::MansionGrid;
use super::{
    Dir, HORIZONTAL, MansionPiece, Mir, Pos, Rot, SimpleGrid, above, is_house, offset, relative,
    rotate_pos, zero_position_with_transform,
};

const CORRIDOR: i32 = 1;
const START_ROOM: i32 = 3;
const ROOM_1X1: i32 = 65536;
const ROOM_1X2: i32 = 131072;
const ROOM_2X2: i32 = 262144;
const ROOM_ORIGIN_FLAG: i32 = 1_048_576;
const ROOM_DOOR_FLAG: i32 = 2_097_152;
const ROOM_STAIRS_FLAG: i32 = 4_194_304;
const ROOM_CORRIDOR_FLAG: i32 = 8_388_608;
const ROOM_TYPE_MASK: i32 = 983_040;
const ROOM_ID_MASK: i32 = 65535;

/// `PlacementData`：可变的位置、rotation 和墙模板名。
struct PlacementData {
    position: Pos,
    rotation: Rot,
    wall_type: &'static str,
}

/// 第 0/1/2 层的房间名集合（第三层继承第二层）。
#[derive(Debug, Clone, Copy)]
enum Floor {
    First,
    Second,
    Third,
}

impl Floor {
    fn get_1x1(self, random: &WorldgenRandom) -> String {
        match self {
            Floor::First => format!("1x1_a{}", random.next_int(5) + 1),
            Floor::Second | Floor::Third => format!("1x1_b{}", random.next_int(5) + 1),
        }
    }

    fn get_1x1_secret(self, random: &WorldgenRandom) -> String {
        format!("1x1_as{}", random.next_int(4) + 1)
    }

    /// 官方第一层忽略 `is_stairs_room`，恒取 `nextInt(9)`；第二/三层为楼梯房时
    /// 返回固定名且不取数。
    fn get_1x2_side(self, random: &WorldgenRandom, is_stairs: bool) -> String {
        match self {
            Floor::First => format!("1x2_a{}", random.next_int(9) + 1),
            Floor::Second | Floor::Third => {
                if is_stairs {
                    "1x2_c_stairs".to_owned()
                } else {
                    format!("1x2_c{}", random.next_int(4) + 1)
                }
            }
        }
    }

    fn get_1x2_front(self, random: &WorldgenRandom, is_stairs: bool) -> String {
        match self {
            Floor::First => format!("1x2_b{}", random.next_int(5) + 1),
            Floor::Second | Floor::Third => {
                if is_stairs {
                    "1x2_d_stairs".to_owned()
                } else {
                    format!("1x2_d{}", random.next_int(5) + 1)
                }
            }
        }
    }

    /// 第一层 `nextInt(2)`；第二/三层 `nextInt(1)`（仍消费一次取数）。
    fn get_1x2_secret(self, random: &WorldgenRandom) -> String {
        match self {
            Floor::First => format!("1x2_s{}", random.next_int(2) + 1),
            Floor::Second | Floor::Third => format!("1x2_se{}", random.next_int(1) + 1),
        }
    }

    fn get_2x2(self, random: &WorldgenRandom) -> String {
        match self {
            Floor::First => format!("2x2_a{}", random.next_int(4) + 1),
            Floor::Second | Floor::Third => format!("2x2_b{}", random.next_int(5) + 1),
        }
    }

    /// `get2x2Secret`：三层都是 `"2x2_s1"`，不取数。
    fn get_2x2_secret(self) -> String {
        "2x2_s1".to_owned()
    }
}

/// 网格里方向的分量（`stepX` 作 x、`stepZ` 作 y）。
fn step2(dir: Dir) -> (i32, i32) {
    let (sx, _, sz) = dir.step();
    (sx, sz)
}

/// `MansionPiecePlacer`。
pub struct MansionPiecePlacer<'a> {
    random: &'a WorldgenRandom,
    start_x: i32,
    start_y: i32,
    pub pieces: Vec<MansionPiece>,
}

impl<'a> MansionPiecePlacer<'a> {
    pub fn new(random: &'a WorldgenRandom) -> Self {
        Self {
            random,
            start_x: 0,
            start_y: 0,
            pieces: Vec::new(),
        }
    }

    /// 四参构造器 `WoodlandMansionPiece(stm, template, position, rotation)` → mirror NONE。
    fn add(&mut self, template: impl Into<String>, pos: Pos, rot: Rot) {
        self.add_mir(template, pos, rot, Mir::None);
    }

    /// 五参构造器 `WoodlandMansionPiece(stm, template, position, rotation, mirror)`。
    fn add_mir(&mut self, template: impl Into<String>, pos: Pos, rot: Rot, mir: Mir) {
        self.pieces.push(MansionPiece {
            template: template.into(),
            pos,
            rot,
            mir,
        });
    }

    /// `entrance`。
    fn entrance(&mut self, data: &mut PlacementData) {
        let west = data.rotation.rotate_dir(Dir::West);
        self.add("entrance", relative(data.position, west, 9), data.rotation);
        data.position = relative(data.position, data.rotation.rotate_dir(Dir::South), 16);
    }

    /// `traverseWallPiece`。
    fn traverse_wall_piece(&mut self, data: &mut PlacementData) {
        let pos = relative(data.position, data.rotation.rotate_dir(Dir::East), 7);
        self.add(data.wall_type, pos, data.rotation);
        data.position = relative(data.position, data.rotation.rotate_dir(Dir::South), 8);
    }

    /// `traverseTurn`。
    fn traverse_turn(&mut self, data: &mut PlacementData) {
        data.position = relative(data.position, data.rotation.rotate_dir(Dir::South), -1);
        self.add("wall_corner", data.position, data.rotation);
        data.position = relative(data.position, data.rotation.rotate_dir(Dir::South), -7);
        data.position = relative(data.position, data.rotation.rotate_dir(Dir::West), -6);
        data.rotation = data.rotation.rotated(Rot::Cw90);
    }

    /// `traverseInnerTurn`。
    fn traverse_inner_turn(&mut self, data: &mut PlacementData) {
        data.position = relative(data.position, data.rotation.rotate_dir(Dir::South), 6);
        data.position = relative(data.position, data.rotation.rotate_dir(Dir::East), 8);
        data.rotation = data.rotation.rotated(Rot::Ccw90);
    }

    /// `traverseOuterWalls`：`data` 的 position/rotation 在遍历中变化。
    /// `bounds = (startX, startY, endX, endY)`。
    fn traverse_outer_walls(
        &mut self,
        data: &mut PlacementData,
        grid: &SimpleGrid,
        mut dir: Dir,
        bounds: (i32, i32, i32, i32),
    ) {
        let (start_x, start_y, end_x, end_y) = bounds;
        let (mut gx, mut gy) = (start_x, start_y);
        let start_direction = dir;
        loop {
            let (dx, dy) = step2(dir);
            let ccw = dir.counter_clock_wise();
            let (ccwx, ccwy) = step2(ccw);
            if !is_house(grid, gx + dx, gy + dy) {
                self.traverse_turn(data);
                dir = dir.clock_wise();
                if gx != end_x || gy != end_y || start_direction != dir {
                    self.traverse_wall_piece(data);
                }
            } else if is_house(grid, gx + dx, gy + dy)
                && is_house(grid, gx + dx + ccwx, gy + dy + ccwy)
            {
                self.traverse_inner_turn(data);
                gx += dx;
                gy += dy;
                dir = ccw;
            } else {
                gx += dx;
                gy += dy;
                if gx != end_x || gy != end_y || start_direction != dir {
                    self.traverse_wall_piece(data);
                }
            }
            if gx == end_x && gy == end_y && start_direction == dir {
                break;
            }
        }
    }

    /// `createMansion` 主流程。
    pub fn create_mansion(&mut self, origin: Pos, rotation: Rot, mansion: &MansionGrid) {
        let mut data = PlacementData {
            position: origin,
            rotation,
            wall_type: "wall_flat",
        };
        let mut second_data = PlacementData {
            position: (0, 0, 0),
            rotation,
            wall_type: "wall_window",
        };
        self.entrance(&mut data);
        second_data.position = above(data.position, 8);
        second_data.rotation = data.rotation;

        let base_grid = &mansion.base_grid;
        let third_grid = &mansion.third_floor_grid;
        self.start_x = mansion.entrance_x + 1;
        self.start_y = mansion.entrance_y + 1;
        let end_x = mansion.entrance_x + 1;
        let end_y = mansion.entrance_y;
        self.traverse_outer_walls(
            &mut data,
            base_grid,
            Dir::South,
            (self.start_x, self.start_y, end_x, end_y),
        );
        self.traverse_outer_walls(
            &mut second_data,
            base_grid,
            Dir::South,
            (self.start_x, self.start_y, end_x, end_y),
        );

        let mut third_data = PlacementData {
            position: above(data.position, 19),
            rotation: data.rotation,
            wall_type: "wall_window",
        };
        let mut done = false;
        for y in 0..third_grid.height {
            if done {
                break;
            }
            for x in (0..third_grid.width).rev() {
                if is_house(third_grid, x, y) {
                    third_data.position = relative(
                        third_data.position,
                        rotation.rotate_dir(Dir::South),
                        8 + (y - self.start_y) * 8,
                    );
                    third_data.position = relative(
                        third_data.position,
                        rotation.rotate_dir(Dir::East),
                        (x - self.start_x) * 8,
                    );
                    self.traverse_wall_piece(&mut third_data);
                    self.traverse_outer_walls(
                        &mut third_data,
                        third_grid,
                        Dir::South,
                        (x, y, x, y),
                    );
                    done = true;
                    break;
                }
            }
        }

        self.create_roof(above(origin, 16), rotation, base_grid, Some(third_grid));
        self.create_roof(above(origin, 27), rotation, third_grid, None);

        let floors = [Floor::First, Floor::Second, Floor::Third];
        for (floor_num, &floor) in floors.iter().enumerate() {
            self.floor_pieces(origin, rotation, mansion, floor_num, floor);
        }
    }

    /// 单层：走廊地毯 + 房间墙/门/房间模板。
    fn floor_pieces(
        &mut self,
        origin: Pos,
        rotation: Rot,
        mansion: &MansionGrid,
        floor_num: usize,
        floor: Floor,
    ) {
        let floor_origin = above(
            origin,
            8 * floor_num as i32 + if floor_num == 2 { 3 } else { 0 },
        );
        let rooms = &mansion.floor_rooms[floor_num];
        let grid = if floor_num == 2 {
            &mansion.third_floor_grid
        } else {
            &mansion.base_grid
        };
        let south_piece = if floor_num == 0 {
            "carpet_south_1"
        } else {
            "carpet_south_2"
        };
        let west_piece = if floor_num == 0 {
            "carpet_west_1"
        } else {
            "carpet_west_2"
        };

        for y in 0..grid.height {
            for x in 0..grid.width {
                if grid.get(x, y) == CORRIDOR {
                    let mut pos = relative(
                        floor_origin,
                        rotation.rotate_dir(Dir::South),
                        8 + (y - self.start_y) * 8,
                    );
                    pos = relative(pos, rotation.rotate_dir(Dir::East), (x - self.start_x) * 8);
                    self.add("corridor_floor", pos, rotation);
                    if grid.get(x, y - 1) == CORRIDOR
                        || (rooms.get(x, y - 1) & ROOM_CORRIDOR_FLAG) == ROOM_CORRIDOR_FLAG
                    {
                        let p = relative(pos, rotation.rotate_dir(Dir::East), 1);
                        self.add("carpet_north", above(p, 1), rotation);
                    }
                    if grid.get(x + 1, y) == CORRIDOR
                        || (rooms.get(x + 1, y) & ROOM_CORRIDOR_FLAG) == ROOM_CORRIDOR_FLAG
                    {
                        let p = relative(
                            relative(pos, rotation.rotate_dir(Dir::South), 1),
                            rotation.rotate_dir(Dir::East),
                            5,
                        );
                        self.add("carpet_east", above(p, 1), rotation);
                    }
                    if grid.get(x, y + 1) == CORRIDOR
                        || (rooms.get(x, y + 1) & ROOM_CORRIDOR_FLAG) == ROOM_CORRIDOR_FLAG
                    {
                        let p = relative(
                            relative(pos, rotation.rotate_dir(Dir::South), 5),
                            rotation.rotate_dir(Dir::West),
                            1,
                        );
                        self.add(south_piece, p, rotation);
                    }
                    if grid.get(x - 1, y) == CORRIDOR
                        || (rooms.get(x - 1, y) & ROOM_CORRIDOR_FLAG) == ROOM_CORRIDOR_FLAG
                    {
                        let p = relative(
                            relative(pos, rotation.rotate_dir(Dir::West), 1),
                            rotation.rotate_dir(Dir::North),
                            1,
                        );
                        self.add(west_piece, p, rotation);
                    }
                }
            }
        }

        let wall_piece = if floor_num == 0 {
            "indoors_wall_1"
        } else {
            "indoors_wall_2"
        };
        let door_piece = if floor_num == 0 {
            "indoors_door_1"
        } else {
            "indoors_door_2"
        };
        let mut door_dirs: Vec<Dir> = Vec::new();

        for y in 0..grid.height {
            for x in 0..grid.width {
                let mut third_floor_start_room = floor_num == 2 && grid.get(x, y) == START_ROOM;
                if grid.get(x, y) == 2 || third_floor_start_room {
                    let room_data = rooms.get(x, y);
                    let room_type = room_data & ROOM_TYPE_MASK;
                    let room_id = room_data & ROOM_ID_MASK;
                    third_floor_start_room = third_floor_start_room
                        && (room_data & ROOM_CORRIDOR_FLAG) == ROOM_CORRIDOR_FLAG;
                    door_dirs.clear();
                    if (room_data & ROOM_DOOR_FLAG) == ROOM_DOOR_FLAG {
                        for dir in HORIZONTAL {
                            let (sx, sz) = step2(dir);
                            if grid.get(x + sx, y + sz) == CORRIDOR {
                                door_dirs.push(dir);
                            }
                        }
                    }
                    // doorDir：水平朝向是 Direction，UP 用 Dir::Up 表示，null 用 Option::None。
                    let door_dir: Option<Dir> = if !door_dirs.is_empty() {
                        Some(door_dirs[self.random.next_int(door_dirs.len() as i32) as usize])
                    } else if (room_data & ROOM_ORIGIN_FLAG) == ROOM_ORIGIN_FLAG {
                        Some(Dir::Up)
                    } else {
                        None
                    };

                    let mut room_pos = relative(
                        floor_origin,
                        rotation.rotate_dir(Dir::South),
                        8 + (y - self.start_y) * 8,
                    );
                    room_pos = relative(
                        room_pos,
                        rotation.rotate_dir(Dir::East),
                        -1 + (x - self.start_x) * 8,
                    );

                    if is_house(grid, x - 1, y) && !mansion.is_room_id(x - 1, y, floor_num, room_id)
                    {
                        let t = if door_dir == Some(Dir::West) {
                            door_piece
                        } else {
                            wall_piece
                        };
                        self.add(t, room_pos, rotation);
                    }
                    if grid.get(x + 1, y) == CORRIDOR && !third_floor_start_room {
                        let p = relative(room_pos, rotation.rotate_dir(Dir::East), 8);
                        let t = if door_dir == Some(Dir::East) {
                            door_piece
                        } else {
                            wall_piece
                        };
                        self.add(t, p, rotation);
                    }
                    if is_house(grid, x, y + 1) && !mansion.is_room_id(x, y + 1, floor_num, room_id)
                    {
                        let p = relative(
                            relative(room_pos, rotation.rotate_dir(Dir::South), 7),
                            rotation.rotate_dir(Dir::East),
                            7,
                        );
                        let t = if door_dir == Some(Dir::South) {
                            door_piece
                        } else {
                            wall_piece
                        };
                        self.add(t, p, rotation.rotated(Rot::Cw90));
                    }
                    if grid.get(x, y - 1) == CORRIDOR && !third_floor_start_room {
                        let p = relative(
                            relative(room_pos, rotation.rotate_dir(Dir::North), 1),
                            rotation.rotate_dir(Dir::East),
                            7,
                        );
                        let t = if door_dir == Some(Dir::North) {
                            door_piece
                        } else {
                            wall_piece
                        };
                        self.add(t, p, rotation.rotated(Rot::Cw90));
                    }

                    if room_type == ROOM_1X1 {
                        self.add_room_1x1(room_pos, rotation, door_dir, floor);
                    } else if room_type == ROOM_1X2 {
                        if let Some(door) = door_dir {
                            let room_dir = mansion.get_1x2_room_direction(x, y, floor_num, room_id);
                            let is_stairs = (room_data & ROOM_STAIRS_FLAG) == ROOM_STAIRS_FLAG;
                            // roomDir 为 null 时官方 addRoom1x2 不匹配任何分支、不加件也不取数。
                            // A null roomDir matches no branch in vanilla addRoom1x2.
                            if let Some(room_dir) = room_dir {
                                self.add_room_1x2(
                                    room_pos, rotation, room_dir, door, floor, is_stairs,
                                );
                            }
                        }
                    } else if room_type == ROOM_2X2 {
                        match door_dir {
                            Some(Dir::Up) => self.add_room_2x2_secret(room_pos, rotation, floor),
                            Some(door) => {
                                let mut room_dir = door.clock_wise();
                                let (sx, sz) = step2(room_dir);
                                if !mansion.is_room_id(x + sx, y + sz, floor_num, room_id) {
                                    room_dir = room_dir.opposite();
                                }
                                self.add_room_2x2(room_pos, rotation, room_dir, door, floor);
                            }
                            None => {}
                        }
                    }
                }
            }
        }
    }
}

impl<'a> MansionPiecePlacer<'a> {
    /// `addRoom1x1`：先取 `get1x1`（恒取数），按门朝向定 pieceRot，`EAST` 不额外取、
    /// `UP/null` 再用 `get1x1Secret` 覆盖房间名（第二次取数）。
    fn add_room_1x1(&mut self, room_pos: Pos, rotation: Rot, door_dir: Option<Dir>, floor: Floor) {
        let mut piece_rot = Rot::None;
        let mut room_type = floor.get_1x1(self.random);
        if door_dir != Some(Dir::East) {
            match door_dir {
                Some(Dir::North) => piece_rot = Rot::Ccw90,
                Some(Dir::West) => piece_rot = Rot::Cw180,
                Some(Dir::South) => piece_rot = Rot::Cw90,
                _ => room_type = floor.get_1x1_secret(self.random),
            }
        }
        let orientation = zero_position_with_transform((1, 0, 0), Mir::None, piece_rot, 7, 7);
        piece_rot = piece_rot.rotated(rotation);
        let orientation = rotate_pos(orientation, rotation);
        let pos = offset(room_pos, orientation.0, 0, orientation.2);
        self.add(room_type, pos, piece_rot);
    }

    /// `addRoom1x2`：按 `(doorDir, roomDir)` 逐分支照官方。所有 `relative` 用
    /// `rotation.rotate(dir)`。
    fn add_room_1x2(
        &mut self,
        room_pos: Pos,
        rotation: Rot,
        room_dir: Dir,
        door_dir: Dir,
        floor: Floor,
        is_stairs: bool,
    ) {
        let rel = |p: Pos, dir: Dir, n: i32| relative(p, rotation.rotate_dir(dir), n);
        let side = |this: &mut Self| floor.get_1x2_side(this.random, is_stairs);
        let front = |this: &mut Self| floor.get_1x2_front(this.random, is_stairs);
        let secret = |this: &mut Self| floor.get_1x2_secret(this.random);

        if door_dir == Dir::East && room_dir == Dir::South {
            let pos = rel(room_pos, Dir::East, 1);
            let t = side(self);
            self.add(t, pos, rotation);
        } else if door_dir == Dir::East && room_dir == Dir::North {
            let pos = rel(rel(room_pos, Dir::East, 1), Dir::South, 6);
            let t = side(self);
            self.add_mir(t, pos, rotation, Mir::LeftRight);
        } else if door_dir == Dir::West && room_dir == Dir::North {
            let pos = rel(rel(room_pos, Dir::East, 7), Dir::South, 6);
            let t = side(self);
            self.add(t, pos, rotation.rotated(Rot::Cw180));
        } else if door_dir == Dir::West && room_dir == Dir::South {
            let pos = rel(room_pos, Dir::East, 7);
            let t = side(self);
            self.add_mir(t, pos, rotation, Mir::FrontBack);
        } else if door_dir == Dir::South && room_dir == Dir::East {
            let pos = rel(room_pos, Dir::East, 1);
            let t = side(self);
            self.add_mir(t, pos, rotation.rotated(Rot::Cw90), Mir::LeftRight);
        } else if door_dir == Dir::South && room_dir == Dir::West {
            let pos = rel(room_pos, Dir::East, 7);
            let t = side(self);
            self.add(t, pos, rotation.rotated(Rot::Cw90));
        } else if door_dir == Dir::North && room_dir == Dir::West {
            let pos = rel(rel(room_pos, Dir::East, 7), Dir::South, 6);
            let t = side(self);
            self.add_mir(t, pos, rotation.rotated(Rot::Cw90), Mir::FrontBack);
        } else if door_dir == Dir::North && room_dir == Dir::East {
            let pos = rel(rel(room_pos, Dir::East, 1), Dir::South, 6);
            let t = side(self);
            self.add(t, pos, rotation.rotated(Rot::Ccw90));
        } else if door_dir == Dir::South && room_dir == Dir::North {
            let pos = rel(rel(room_pos, Dir::East, 1), Dir::North, 8);
            let t = front(self);
            self.add(t, pos, rotation);
        } else if door_dir == Dir::North && room_dir == Dir::South {
            let pos = rel(rel(room_pos, Dir::East, 7), Dir::South, 14);
            let t = front(self);
            self.add(t, pos, rotation.rotated(Rot::Cw180));
        } else if door_dir == Dir::West && room_dir == Dir::East {
            let pos = rel(room_pos, Dir::East, 15);
            let t = front(self);
            self.add(t, pos, rotation.rotated(Rot::Cw90));
        } else if door_dir == Dir::East && room_dir == Dir::West {
            let pos = rel(rel(room_pos, Dir::West, 7), Dir::South, 6);
            let t = front(self);
            self.add(t, pos, rotation.rotated(Rot::Ccw90));
        } else if door_dir == Dir::Up && room_dir == Dir::East {
            let pos = rel(room_pos, Dir::East, 15);
            let t = secret(self);
            self.add(t, pos, rotation.rotated(Rot::Cw90));
        } else if door_dir == Dir::Up && room_dir == Dir::South {
            let pos = rel(rel(room_pos, Dir::East, 1), Dir::North, 0);
            let t = secret(self);
            self.add(t, pos, rotation);
        }
    }

    /// `addRoom2x2`：按 `(doorDir, roomDir)` 算 east/south 偏移、rot、mirror。
    fn add_room_2x2(
        &mut self,
        room_pos: Pos,
        rotation: Rot,
        room_dir: Dir,
        door_dir: Dir,
        floor: Floor,
    ) {
        let (mut east, mut south) = (0, 0);
        let mut rot = rotation;
        let mut mir = Mir::None;
        if door_dir == Dir::East && room_dir == Dir::South {
            east = -7;
        } else if door_dir == Dir::East && room_dir == Dir::North {
            east = -7;
            south = 6;
            mir = Mir::LeftRight;
        } else if door_dir == Dir::North && room_dir == Dir::East {
            east = 1;
            south = 14;
            rot = rotation.rotated(Rot::Ccw90);
        } else if door_dir == Dir::North && room_dir == Dir::West {
            east = 7;
            south = 14;
            rot = rotation.rotated(Rot::Ccw90);
            mir = Mir::LeftRight;
        } else if door_dir == Dir::South && room_dir == Dir::West {
            east = 7;
            south = -8;
            rot = rotation.rotated(Rot::Cw90);
        } else if door_dir == Dir::South && room_dir == Dir::East {
            east = 1;
            south = -8;
            rot = rotation.rotated(Rot::Cw90);
            mir = Mir::LeftRight;
        } else if door_dir == Dir::West && room_dir == Dir::North {
            east = 15;
            south = 6;
            rot = rotation.rotated(Rot::Cw180);
        } else if door_dir == Dir::West && room_dir == Dir::South {
            east = 15;
            mir = Mir::FrontBack;
        }
        let pos = relative(
            relative(room_pos, rotation.rotate_dir(Dir::East), east),
            rotation.rotate_dir(Dir::South),
            south,
        );
        let t = floor.get_2x2(self.random);
        self.add_mir(t, pos, rot, mir);
    }

    /// `addRoom2x2Secret`。
    fn add_room_2x2_secret(&mut self, room_pos: Pos, rotation: Rot, floor: Floor) {
        let pos = relative(room_pos, rotation.rotate_dir(Dir::East), 1);
        let t = floor.get_2x2_secret();
        self.add_mir(t, pos, rotation, Mir::None);
    }

    /// 屋顶基准位置：`roofOrigin` 按 rotation 旋转后的 SOUTH/EAST 偏移。
    fn roof_pos(&self, roof_origin: Pos, rotation: Rot, x: i32, y: i32) -> Pos {
        let p = relative(
            roof_origin,
            rotation.rotate_dir(Dir::South),
            8 + (y - self.start_y) * 8,
        );
        relative(p, rotation.rotate_dir(Dir::East), (x - self.start_x) * 8)
    }

    /// `createRoof`：三个循环，逐分支照官方（无随机）。
    fn create_roof(
        &mut self,
        roof_origin: Pos,
        rotation: Rot,
        grid: &SimpleGrid,
        above_grid: Option<&SimpleGrid>,
    ) {
        for y in 0..grid.height {
            for x in 0..grid.width {
                let position = self.roof_pos(roof_origin, rotation, x, y);
                let is_above = above_grid.is_some_and(|g| is_house(g, x, y));
                if is_house(grid, x, y) && !is_above {
                    self.add("roof", above(position, 3), rotation);
                    if !is_house(grid, x + 1, y) {
                        let p2 = relative(position, rotation.rotate_dir(Dir::East), 6);
                        self.add("roof_front", p2, rotation);
                    }
                    if !is_house(grid, x - 1, y) {
                        let p2 = relative(
                            relative(position, rotation.rotate_dir(Dir::East), 0),
                            rotation.rotate_dir(Dir::South),
                            7,
                        );
                        self.add("roof_front", p2, rotation.rotated(Rot::Cw180));
                    }
                    if !is_house(grid, x, y - 1) {
                        let p2 = relative(position, rotation.rotate_dir(Dir::West), 1);
                        self.add("roof_front", p2, rotation.rotated(Rot::Ccw90));
                    }
                    if !is_house(grid, x, y + 1) {
                        let p2 = relative(
                            relative(position, rotation.rotate_dir(Dir::East), 6),
                            rotation.rotate_dir(Dir::South),
                            6,
                        );
                        self.add("roof_front", p2, rotation.rotated(Rot::Cw90));
                    }
                }
            }
        }

        if let Some(above) = above_grid {
            for y in 0..grid.height {
                for x in 0..grid.width {
                    let position = self.roof_pos(roof_origin, rotation, x, y);
                    let is_above = is_house(above, x, y);
                    if is_house(grid, x, y) && is_above {
                        if !is_house(grid, x + 1, y) {
                            let p2 = relative(position, rotation.rotate_dir(Dir::East), 7);
                            self.add("small_wall", p2, rotation);
                        }
                        if !is_house(grid, x - 1, y) {
                            let p2 = relative(
                                relative(position, rotation.rotate_dir(Dir::West), 1),
                                rotation.rotate_dir(Dir::South),
                                6,
                            );
                            self.add("small_wall", p2, rotation.rotated(Rot::Cw180));
                        }
                        if !is_house(grid, x, y - 1) {
                            let p2 = relative(
                                relative(position, rotation.rotate_dir(Dir::West), 0),
                                rotation.rotate_dir(Dir::North),
                                1,
                            );
                            self.add("small_wall", p2, rotation.rotated(Rot::Ccw90));
                        }
                        if !is_house(grid, x, y + 1) {
                            let p2 = relative(
                                relative(position, rotation.rotate_dir(Dir::East), 6),
                                rotation.rotate_dir(Dir::South),
                                7,
                            );
                            self.add("small_wall", p2, rotation.rotated(Rot::Cw90));
                        }
                        if !is_house(grid, x + 1, y) {
                            if !is_house(grid, x, y - 1) {
                                let p2 = relative(
                                    relative(position, rotation.rotate_dir(Dir::East), 7),
                                    rotation.rotate_dir(Dir::North),
                                    2,
                                );
                                self.add("small_wall_corner", p2, rotation);
                            }
                            if !is_house(grid, x, y + 1) {
                                let p2 = relative(
                                    relative(position, rotation.rotate_dir(Dir::East), 8),
                                    rotation.rotate_dir(Dir::South),
                                    7,
                                );
                                self.add("small_wall_corner", p2, rotation.rotated(Rot::Cw90));
                            }
                        }
                        if !is_house(grid, x - 1, y) {
                            if !is_house(grid, x, y - 1) {
                                let p2 = relative(
                                    relative(position, rotation.rotate_dir(Dir::West), 2),
                                    rotation.rotate_dir(Dir::North),
                                    1,
                                );
                                self.add("small_wall_corner", p2, rotation.rotated(Rot::Ccw90));
                            }
                            if !is_house(grid, x, y + 1) {
                                let p2 = relative(
                                    relative(position, rotation.rotate_dir(Dir::West), 1),
                                    rotation.rotate_dir(Dir::South),
                                    8,
                                );
                                self.add("small_wall_corner", p2, rotation.rotated(Rot::Cw180));
                            }
                        }
                    }
                }
            }
        }

        for y in 0..grid.height {
            for x in 0..grid.width {
                let position = self.roof_pos(roof_origin, rotation, x, y);
                let is_above = above_grid.is_some_and(|g| is_house(g, x, y));
                if is_house(grid, x, y) && !is_above {
                    if !is_house(grid, x + 1, y) {
                        let p2 = relative(position, rotation.rotate_dir(Dir::East), 6);
                        if !is_house(grid, x, y + 1) {
                            let p3 = relative(p2, rotation.rotate_dir(Dir::South), 6);
                            self.add("roof_corner", p3, rotation);
                        } else if is_house(grid, x + 1, y + 1) {
                            let p3 = relative(p2, rotation.rotate_dir(Dir::South), 5);
                            self.add("roof_inner_corner", p3, rotation);
                        }
                        if !is_house(grid, x, y - 1) {
                            self.add("roof_corner", p2, rotation.rotated(Rot::Ccw90));
                        } else if is_house(grid, x + 1, y - 1) {
                            let p3 = relative(
                                relative(position, rotation.rotate_dir(Dir::East), 9),
                                rotation.rotate_dir(Dir::North),
                                2,
                            );
                            self.add("roof_inner_corner", p3, rotation.rotated(Rot::Cw90));
                        }
                    }
                    if !is_house(grid, x - 1, y) {
                        let p2 = relative(
                            relative(position, rotation.rotate_dir(Dir::East), 0),
                            rotation.rotate_dir(Dir::South),
                            0,
                        );
                        if !is_house(grid, x, y + 1) {
                            let p3 = relative(p2, rotation.rotate_dir(Dir::South), 6);
                            self.add("roof_corner", p3, rotation.rotated(Rot::Cw90));
                        } else if is_house(grid, x - 1, y + 1) {
                            let p3 = relative(
                                relative(p2, rotation.rotate_dir(Dir::South), 8),
                                rotation.rotate_dir(Dir::West),
                                3,
                            );
                            self.add("roof_inner_corner", p3, rotation.rotated(Rot::Ccw90));
                        }
                        if !is_house(grid, x, y - 1) {
                            self.add("roof_corner", p2, rotation.rotated(Rot::Cw180));
                        } else if is_house(grid, x - 1, y - 1) {
                            let p3 = relative(p2, rotation.rotate_dir(Dir::South), 1);
                            self.add("roof_inner_corner", p3, rotation.rotated(Rot::Cw180));
                        }
                    }
                }
            }
        }
    }
}
