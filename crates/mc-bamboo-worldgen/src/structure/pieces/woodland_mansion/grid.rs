//! `WoodlandMansionPieces.MansionGrid`：网格生成、走廊递归、边缘清理、房间识别、三楼。
//!
//! 随机源取数顺序逐行照官方：`recursiveCorridor` 的 `nextInt(4)` + 条件 `nextBoolean()`、
//! `identifyRooms` 的 `Util.shuffle` + 两次 `nextBoolean()`、`setupThirdFloor` 的
//! `nextInt(size)`。网格的 Y 对应世界 Z，方向用 `stepX/stepZ` 作 (x, y) 偏移。
//!
//! `WoodlandMansionPieces.MansionGrid`: grid construction, corridor recursion, edge
//! cleanup, room identification, third floor. Random draw order is translated line by
//! line. The grid's Y maps to world Z; directions use `stepX/stepZ` as (x, y) deltas.

use crate::random::WorldgenRandom;

use super::{Dir, HORIZONTAL, SimpleGrid, is_house};

const CLEAR: i32 = 0;
const CORRIDOR: i32 = 1;
const ROOM: i32 = 2;
const START_ROOM: i32 = 3;
const BLOCKED: i32 = 5;

const ROOM_1X1: i32 = 65536;
const ROOM_1X2: i32 = 131072;
const ROOM_2X2: i32 = 262144;
const ROOM_ORIGIN_FLAG: i32 = 1_048_576;
const ROOM_DOOR_FLAG: i32 = 2_097_152;
const ROOM_STAIRS_FLAG: i32 = 4_194_304;
const ROOM_CORRIDOR_FLAG: i32 = 8_388_608;
const ROOM_TYPE_MASK: i32 = 983_040;
const ROOM_ID_MASK: i32 = 65535;

/// 2D 网格里方向的分量：`stepX` 作 x、`stepZ` 作 y。
fn step2(dir: Dir) -> (i32, i32) {
    let (sx, _, sz) = dir.step();
    (sx, sz)
}

/// `Util.shuffle(list, random)`：`for i = size; i > 1; i--` 交换 `(i-1, nextInt(i))`。
fn shuffle<T>(list: &mut [T], random: &WorldgenRandom) {
    let size = list.len();
    for i in (2..=size).rev() {
        let swap_to = random.next_int(i as i32) as usize;
        list.swap(i - 1, swap_to);
    }
}

/// `MansionGrid.recursiveCorridor`。注意 `nextBoolean()` 只在 `nextDir == EAST`
/// 且不等于 heading 反向时被消费（Java 的 `&&` / `||` 短路）。
fn recursive_corridor(
    grid: &mut SimpleGrid,
    random: &WorldgenRandom,
    x: i32,
    y: i32,
    heading: Dir,
    depth: i32,
) {
    if depth <= 0 {
        return;
    }
    let (hx, hy) = step2(heading);
    grid.set(x, y, CORRIDOR);
    grid.set_if(x + hx, y + hy, CLEAR, CORRIDOR);

    for _ in 0..8 {
        let next_dir = Dir::from_2d(random.next_int(4));
        let passes =
            next_dir != heading.opposite() && (next_dir != Dir::East || !random.next_boolean());
        if passes {
            let nx = x + hx;
            let ny = y + hy;
            let (dx, dy) = step2(next_dir);
            if grid.get(nx + dx, ny + dy) == CLEAR && grid.get(nx + dx * 2, ny + dy * 2) == CLEAR {
                recursive_corridor(grid, random, x + hx + dx, y + hy + dy, next_dir, depth - 1);
                break;
            }
        }
    }

    let cw = heading.clock_wise();
    let ccw = heading.counter_clock_wise();
    let (cwx, cwy) = step2(cw);
    let (ccwx, ccwy) = step2(ccw);
    grid.set_if(x + cwx, y + cwy, CLEAR, ROOM);
    grid.set_if(x + ccwx, y + ccwy, CLEAR, ROOM);
    grid.set_if(x + hx + cwx, y + hy + cwy, CLEAR, ROOM);
    grid.set_if(x + hx + ccwx, y + hy + ccwy, CLEAR, ROOM);
    grid.set_if(x + hx * 2, y + hy * 2, CLEAR, ROOM);
    grid.set_if(x + cwx * 2, y + cwy * 2, CLEAR, ROOM);
    grid.set_if(x + ccwx * 2, y + ccwy * 2, CLEAR, ROOM);
}

/// `MansionGrid.cleanEdges`：返回是否改动过（无随机）。
fn clean_edges(grid: &mut SimpleGrid) -> bool {
    let mut touched = false;
    for y in 0..grid.height {
        for x in 0..grid.width {
            if grid.get(x, y) == CLEAR {
                let mut direct = 0;
                direct += i32::from(is_house(grid, x + 1, y));
                direct += i32::from(is_house(grid, x - 1, y));
                direct += i32::from(is_house(grid, x, y + 1));
                direct += i32::from(is_house(grid, x, y - 1));
                if direct >= 3 {
                    grid.set(x, y, ROOM);
                    touched = true;
                } else if direct == 2 {
                    let mut diagonal = 0;
                    diagonal += i32::from(is_house(grid, x + 1, y + 1));
                    diagonal += i32::from(is_house(grid, x - 1, y + 1));
                    diagonal += i32::from(is_house(grid, x + 1, y - 1));
                    diagonal += i32::from(is_house(grid, x - 1, y - 1));
                    if diagonal <= 1 {
                        grid.set(x, y, ROOM);
                        touched = true;
                    }
                }
            }
        }
    }
    touched
}

/// `MansionGrid.identifyRooms`：`Util.shuffle` + 每个房间两次 `nextBoolean()`。
fn identify_rooms(from_grid: &SimpleGrid, room_grid: &mut SimpleGrid, random: &WorldgenRandom) {
    let mut room_pos: Vec<(i32, i32)> = Vec::new();
    for y in 0..from_grid.height {
        for x in 0..from_grid.width {
            if from_grid.get(x, y) == ROOM {
                room_pos.push((x, y));
            }
        }
    }
    shuffle(&mut room_pos, random);
    let mut room_id = 10;

    for (x, y) in room_pos {
        if room_grid.get(x, y) != 0 {
            continue;
        }
        let (mut x0, mut x1) = (x, x);
        let (mut y0, mut y1) = (y, y);
        let mut room_type = ROOM_1X1;
        if room_grid.get(x + 1, y) == 0
            && room_grid.get(x, y + 1) == 0
            && room_grid.get(x + 1, y + 1) == 0
            && from_grid.get(x + 1, y) == ROOM
            && from_grid.get(x, y + 1) == ROOM
            && from_grid.get(x + 1, y + 1) == ROOM
        {
            x1 += 1;
            y1 += 1;
            room_type = ROOM_2X2;
        } else if room_grid.get(x - 1, y) == 0
            && room_grid.get(x, y + 1) == 0
            && room_grid.get(x - 1, y + 1) == 0
            && from_grid.get(x - 1, y) == ROOM
            && from_grid.get(x, y + 1) == ROOM
            && from_grid.get(x - 1, y + 1) == ROOM
        {
            x0 -= 1;
            y1 += 1;
            room_type = ROOM_2X2;
        } else if room_grid.get(x - 1, y) == 0
            && room_grid.get(x, y - 1) == 0
            && room_grid.get(x - 1, y - 1) == 0
            && from_grid.get(x - 1, y) == ROOM
            && from_grid.get(x, y - 1) == ROOM
            && from_grid.get(x - 1, y - 1) == ROOM
        {
            x0 -= 1;
            y0 -= 1;
            room_type = ROOM_2X2;
        } else if room_grid.get(x + 1, y) == 0 && from_grid.get(x + 1, y) == ROOM {
            x1 += 1;
            room_type = ROOM_1X2;
        } else if room_grid.get(x, y + 1) == 0 && from_grid.get(x, y + 1) == ROOM {
            y1 += 1;
            room_type = ROOM_1X2;
        } else if room_grid.get(x - 1, y) == 0 && from_grid.get(x - 1, y) == ROOM {
            x0 -= 1;
            room_type = ROOM_1X2;
        } else if room_grid.get(x, y - 1) == 0 && from_grid.get(x, y - 1) == ROOM {
            y0 -= 1;
            room_type = ROOM_1X2;
        }

        let mut door_x = if random.next_boolean() { x0 } else { x1 };
        let mut door_y = if random.next_boolean() { y0 } else { y1 };
        let mut door_flag = ROOM_DOOR_FLAG;
        if !from_grid.edges_to(door_x, door_y, CORRIDOR) {
            door_x = if door_x == x0 { x1 } else { x0 };
            door_y = if door_y == y0 { y1 } else { y0 };
            if !from_grid.edges_to(door_x, door_y, CORRIDOR) {
                door_y = if door_y == y0 { y1 } else { y0 };
                if !from_grid.edges_to(door_x, door_y, CORRIDOR) {
                    door_x = if door_x == x0 { x1 } else { x0 };
                    door_y = if door_y == y0 { y1 } else { y0 };
                    if !from_grid.edges_to(door_x, door_y, CORRIDOR) {
                        door_flag = 0;
                        door_x = x0;
                        door_y = y0;
                    }
                }
            }
        }

        for ry in y0..=y1 {
            for rx in x0..=x1 {
                let value = if rx == door_x && ry == door_y {
                    ROOM_ORIGIN_FLAG | door_flag | room_type | room_id
                } else {
                    room_type | room_id
                };
                room_grid.set(rx, ry, value);
            }
        }
        room_id += 1;
    }
}

/// 已生成的网格（构造器完成后的只读结果）。
pub struct MansionGrid {
    pub base_grid: SimpleGrid,
    pub third_floor_grid: SimpleGrid,
    pub floor_rooms: [SimpleGrid; 3],
    pub entrance_x: i32,
    pub entrance_y: i32,
}

impl MansionGrid {
    /// `isRoomId(grid, x, y, floor, roomId)`：官方忽略传入的 `grid`，用
    /// `floorRooms[floor].get(x,y) & 65535`。
    pub fn is_room_id(&self, x: i32, y: i32, floor: usize, room_id: i32) -> bool {
        (self.floor_rooms[floor].get(x, y) & ROOM_ID_MASK) == room_id
    }

    /// `get1x2RoomDirection`：按 `Direction.Plane.HORIZONTAL` 顺序找第一个同 roomId 的邻格。
    pub fn get_1x2_room_direction(
        &self,
        x: i32,
        y: i32,
        floor: usize,
        room_id: i32,
    ) -> Option<Dir> {
        for dir in HORIZONTAL {
            let (sx, sz) = step2(dir);
            if self.is_room_id(x + sx, y + sz, floor, room_id) {
                return Some(dir);
            }
        }
        None
    }

    /// 照 `MansionGrid(random)` 构造器的取数顺序建网格。
    pub fn new(random: &WorldgenRandom) -> Self {
        let entrance_x = 7;
        let entrance_y = 4;
        let mut base_grid = SimpleGrid::new(11, 11, BLOCKED);
        base_grid.set_rect(
            entrance_x,
            entrance_y,
            entrance_x + 1,
            entrance_y + 1,
            START_ROOM,
        );
        base_grid.set_rect(
            entrance_x - 1,
            entrance_y,
            entrance_x - 1,
            entrance_y + 1,
            ROOM,
        );
        base_grid.set_rect(
            entrance_x + 2,
            entrance_y - 2,
            entrance_x + 3,
            entrance_y + 3,
            BLOCKED,
        );
        base_grid.set_rect(
            entrance_x + 1,
            entrance_y - 2,
            entrance_x + 1,
            entrance_y - 1,
            CORRIDOR,
        );
        base_grid.set_rect(
            entrance_x + 1,
            entrance_y + 2,
            entrance_x + 1,
            entrance_y + 3,
            CORRIDOR,
        );
        base_grid.set(entrance_x - 1, entrance_y - 1, CORRIDOR);
        base_grid.set(entrance_x - 1, entrance_y + 2, CORRIDOR);
        base_grid.set_rect(0, 0, 11, 1, BLOCKED);
        base_grid.set_rect(0, 9, 11, 11, BLOCKED);
        recursive_corridor(
            &mut base_grid,
            random,
            entrance_x,
            entrance_y - 2,
            Dir::West,
            6,
        );
        recursive_corridor(
            &mut base_grid,
            random,
            entrance_x,
            entrance_y + 3,
            Dir::West,
            6,
        );
        recursive_corridor(
            &mut base_grid,
            random,
            entrance_x - 2,
            entrance_y - 1,
            Dir::West,
            3,
        );
        recursive_corridor(
            &mut base_grid,
            random,
            entrance_x - 2,
            entrance_y + 2,
            Dir::West,
            3,
        );
        while clean_edges(&mut base_grid) {}

        let mut floor_rooms = [
            SimpleGrid::new(11, 11, BLOCKED),
            SimpleGrid::new(11, 11, BLOCKED),
            SimpleGrid::new(11, 11, BLOCKED),
        ];
        identify_rooms(&base_grid, &mut floor_rooms[0], random);
        identify_rooms(&base_grid, &mut floor_rooms[1], random);
        floor_rooms[0].set_rect(
            entrance_x + 1,
            entrance_y,
            entrance_x + 1,
            entrance_y + 1,
            ROOM_CORRIDOR_FLAG,
        );
        floor_rooms[1].set_rect(
            entrance_x + 1,
            entrance_y,
            entrance_x + 1,
            entrance_y + 1,
            ROOM_CORRIDOR_FLAG,
        );
        let mut third_floor_grid = SimpleGrid::new(base_grid.width, base_grid.height, BLOCKED);
        Self::setup_third_floor(random, &base_grid, &mut third_floor_grid, &mut floor_rooms);
        identify_rooms(&third_floor_grid, &mut floor_rooms[2], random);

        Self {
            base_grid,
            third_floor_grid,
            floor_rooms,
            entrance_x,
            entrance_y,
        }
    }

    /// `setupThirdFloor`：`nextInt(potentialRooms.size())`、可能的 `recursiveCorridor`、
    /// `nextInt(potentialCorridors.size())`。
    fn setup_third_floor(
        random: &WorldgenRandom,
        base_grid: &SimpleGrid,
        third: &mut SimpleGrid,
        floor_rooms: &mut [SimpleGrid; 3],
    ) {
        let mut potential: Vec<(i32, i32)> = Vec::new();
        for y in 0..third.height {
            for x in 0..third.width {
                let room_data = floor_rooms[1].get(x, y);
                let room_type = room_data & ROOM_TYPE_MASK;
                if room_type == ROOM_1X2 && (room_data & ROOM_DOOR_FLAG) == ROOM_DOOR_FLAG {
                    potential.push((x, y));
                }
            }
        }

        if potential.is_empty() {
            third.set_rect(0, 0, third.width, third.height, BLOCKED);
            return;
        }

        let room_pos = potential[random.next_int(potential.len() as i32) as usize];
        let room_data = floor_rooms[1].get(room_pos.0, room_pos.1);
        floor_rooms[1].set(room_pos.0, room_pos.1, room_data | ROOM_STAIRS_FLAG);
        let room_id = room_data & ROOM_ID_MASK;
        // get1x2RoomDirection 用 floorRooms[1]（忽略传入的 baseGrid）。
        let room_dir = {
            let mut found = None;
            for dir in HORIZONTAL {
                let (sx, sz) = step2(dir);
                if (floor_rooms[1].get(room_pos.0 + sx, room_pos.1 + sz) & ROOM_ID_MASK) == room_id
                {
                    found = Some(dir);
                    break;
                }
            }
            found.expect("1x2 room has a matching neighbour")
        };
        let (rdx, rdy) = step2(room_dir);
        let room_end_x = room_pos.0 + rdx;
        let room_end_y = room_pos.1 + rdy;

        for y in 0..third.height {
            for x in 0..third.width {
                if !is_house(base_grid, x, y) {
                    third.set(x, y, BLOCKED);
                } else if x == room_pos.0 && y == room_pos.1 {
                    third.set(x, y, START_ROOM);
                } else if x == room_end_x && y == room_end_y {
                    third.set(x, y, START_ROOM);
                    floor_rooms[2].set(x, y, ROOM_CORRIDOR_FLAG);
                }
            }
        }

        let mut potential_corridors: Vec<Dir> = Vec::new();
        for dir in HORIZONTAL {
            let (sx, sz) = step2(dir);
            if third.get(room_end_x + sx, room_end_y + sz) == CLEAR {
                potential_corridors.push(dir);
            }
        }

        if potential_corridors.is_empty() {
            third.set_rect(0, 0, third.width, third.height, BLOCKED);
            floor_rooms[1].set(room_pos.0, room_pos.1, room_data);
        } else {
            let corridor_dir =
                potential_corridors[random.next_int(potential_corridors.len() as i32) as usize];
            let (cdx, cdy) = step2(corridor_dir);
            recursive_corridor(
                third,
                random,
                room_end_x + cdx,
                room_end_y + cdy,
                corridor_dir,
                4,
            );
            while clean_edges(third) {}
        }
    }
}
