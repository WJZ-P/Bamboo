//! `OceanMonumentStructure` / `OceanMonumentPieces` 的结构件布局。
//!
//! 移植 `createTopPiece` + `MonumentBuilding` 构造：`generateRoomGraph` 的房间网格、
//! 连接/开口/寻源、`Util.shuffle` 与扫描循环、七个 fitter 的选择与 claimed 标记、
//! 子件包围盒的 `getWorldPos` 平移、两翼与阁楼，以及各处随机源取数。不含
//! `postProcess`。子件（`childPieces`）通过 Java 侧反射列出，逐项对拍。
//!
//! Layout of `OceanMonumentStructure` / `OceanMonumentPieces`: `createTopPiece` +
//! `MonumentBuilding` (room graph, connections/openings/findSource, `Util.shuffle`
//! scan loop, seven fitters, `getWorldPos` child-box translation, wings and
//! penthouse, and every random draw). No `postProcess`. Children are listed by
//! the Java oracle via reflection and compared field by field.

use crate::random::WorldgenRandom;

use super::{Box, HDir, Piece, make_random, new_box, translate};

/// `Direction.get3DDataValue()`：DOWN=0 UP=1 NORTH=2 SOUTH=3 WEST=4 EAST=5。
const D_UP: usize = 1;
const D_NORTH: usize = 2;
const D_SOUTH: usize = 3;
const D_WEST: usize = 4;
const D_EAST: usize = 5;
/// `Direction.values()` 顺序的 (stepX, stepY, stepZ)。
const STEPS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

/// `getRoomIndex(x, y, z) = y*25 + z*5 + x`。
fn room_index(x: i32, y: i32, z: i32) -> i32 {
    y * 25 + z * 5 + x
}

/// 房间节点；`connections` 存房间 index（-1 为 null）。
#[derive(Clone)]
struct RoomDef {
    present: bool,
    connections: [i32; 6],
    has_opening: [bool; 6],
    claimed: bool,
    is_source: bool,
    scan_index: i32,
}

impl RoomDef {
    fn empty() -> Self {
        Self {
            present: false,
            connections: [-1; 6],
            has_opening: [false; 6],
            claimed: false,
            is_source: false,
            scan_index: 0,
        }
    }

    fn update_openings(&mut self) {
        for i in 0..6 {
            self.has_opening[i] = self.connections[i] >= 0;
        }
    }
}

/// `setConnection(direction, definition)`：双向写连接，`opp = d3 ^ 1`。
fn set_connection(rooms: &mut [RoomDef], a: i32, d3: usize, b: i32) {
    rooms[a as usize].connections[d3] = b;
    rooms[b as usize].connections[d3 ^ 1] = a;
}

/// `RoomDefinition.findSource(scanIndex)` 的递归（先把本节点数据拷出来再递归，
/// 避免可变借用冲突）。
fn find_source(rooms: &mut [RoomDef], idx: i32, scan: i32) -> bool {
    let (is_source, connections, has_opening) = {
        let r = &rooms[idx as usize];
        (r.is_source, r.connections, r.has_opening)
    };
    if is_source {
        return true;
    }
    rooms[idx as usize].scan_index = scan;
    for i in 0..6 {
        let n = connections[i];
        if n >= 0 && has_opening[i] {
            let nscan = rooms[n as usize].scan_index;
            if nscan != scan && find_source(rooms, n, scan) {
                return true;
            }
        }
    }
    false
}

/// 子件类型（含固定包围盒尺寸，`rw/rh/rd` 为网格单位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoomKind {
    Building,
    Entry,
    Core,
    DoubleX,
    DoubleXY,
    DoubleY,
    DoubleYZ,
    DoubleZ,
    Simple,
    SimpleTop,
    Wing,
    Penthouse,
}

impl RoomKind {
    fn id(self) -> &'static str {
        match self {
            Self::Building => "minecraft:omb",
            Self::Entry => "minecraft:omentry",
            Self::Core => "minecraft:omcr",
            Self::DoubleX => "minecraft:omdxr",
            Self::DoubleXY => "minecraft:omdxyr",
            Self::DoubleY => "minecraft:omdyr",
            Self::DoubleYZ => "minecraft:omdyzr",
            Self::DoubleZ => "minecraft:omdzr",
            Self::Simple => "minecraft:omsimple",
            Self::SimpleTop => "minecraft:omsimplet",
            Self::Wing => "minecraft:omwr",
            Self::Penthouse => "minecraft:ompenthouse",
        }
    }

    /// 由 RoomDefinition 造盒时的 (rw, rh, rd)。
    fn dims(self) -> (i32, i32, i32) {
        match self {
            Self::Entry | Self::Simple | Self::SimpleTop => (1, 1, 1),
            Self::Core => (2, 2, 2),
            Self::DoubleX => (2, 1, 1),
            Self::DoubleXY => (2, 2, 1),
            Self::DoubleY => (1, 2, 1),
            Self::DoubleYZ => (1, 2, 2),
            Self::DoubleZ => (1, 1, 2),
            _ => (1, 1, 1),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Fitter {
    DoubleXY,
    DoubleYZ,
    DoubleZ,
    DoubleX,
    DoubleY,
    SimpleTop,
    Simple,
}

const FITTERS: [Fitter; 7] = [
    Fitter::DoubleXY,
    Fitter::DoubleYZ,
    Fitter::DoubleZ,
    Fitter::DoubleX,
    Fitter::DoubleY,
    Fitter::SimpleTop,
    Fitter::Simple,
];

/// 取 `conn = rooms[idx].connections[d3]` 对应节点的 claimed（短路上文已保证非空）。
fn conn_claimed(rooms: &[RoomDef], idx: i32, d3: usize) -> bool {
    let n = rooms[idx as usize].connections[d3];
    n >= 0 && rooms[n as usize].claimed
}

fn fitter_fits(fitter: Fitter, rooms: &[RoomDef], idx: i32) -> bool {
    let d = &rooms[idx as usize];
    let op = d.has_opening;
    match fitter {
        Fitter::DoubleXY => {
            if op[D_EAST]
                && !conn_claimed(rooms, idx, D_EAST)
                && op[D_UP]
                && !conn_claimed(rooms, idx, D_UP)
            {
                let east = d.connections[D_EAST];
                east >= 0
                    && rooms[east as usize].has_opening[D_UP]
                    && !conn_claimed(rooms, east, D_UP)
            } else {
                false
            }
        }
        Fitter::DoubleYZ => {
            if op[D_NORTH]
                && !conn_claimed(rooms, idx, D_NORTH)
                && op[D_UP]
                && !conn_claimed(rooms, idx, D_UP)
            {
                let north = d.connections[D_NORTH];
                north >= 0
                    && rooms[north as usize].has_opening[D_UP]
                    && !conn_claimed(rooms, north, D_UP)
            } else {
                false
            }
        }
        Fitter::DoubleZ => op[D_NORTH] && !conn_claimed(rooms, idx, D_NORTH),
        Fitter::DoubleX => op[D_EAST] && !conn_claimed(rooms, idx, D_EAST),
        Fitter::DoubleY => op[D_UP] && !conn_claimed(rooms, idx, D_UP),
        Fitter::SimpleTop => {
            !op[D_WEST] && !op[D_EAST] && !op[D_NORTH] && !op[D_SOUTH] && !op[D_UP]
        }
        Fitter::Simple => true,
    }
}

/// 执行 fitter 的 `create`：改 claimed、返回 (类型, 用于造盒的房间 index)。
/// `FitSimpleRoom` 消耗一次 `nextInt(3)`。
fn fitter_create(
    fitter: Fitter,
    rooms: &mut [RoomDef],
    idx: i32,
    random: &WorldgenRandom,
) -> (RoomKind, i32) {
    match fitter {
        Fitter::DoubleXY => {
            rooms[idx as usize].claimed = true;
            let east = rooms[idx as usize].connections[D_EAST];
            let up = rooms[idx as usize].connections[D_UP];
            rooms[east as usize].claimed = true;
            rooms[up as usize].claimed = true;
            let east_up = rooms[east as usize].connections[D_UP];
            rooms[east_up as usize].claimed = true;
            (RoomKind::DoubleXY, idx)
        }
        Fitter::DoubleYZ => {
            rooms[idx as usize].claimed = true;
            let north = rooms[idx as usize].connections[D_NORTH];
            let up = rooms[idx as usize].connections[D_UP];
            rooms[north as usize].claimed = true;
            rooms[up as usize].claimed = true;
            let north_up = rooms[north as usize].connections[D_UP];
            rooms[north_up as usize].claimed = true;
            (RoomKind::DoubleYZ, idx)
        }
        Fitter::DoubleZ => {
            let d = &rooms[idx as usize];
            let source = if !d.has_opening[D_NORTH] || conn_claimed(rooms, idx, D_NORTH) {
                d.connections[D_SOUTH]
            } else {
                idx
            };
            rooms[source as usize].claimed = true;
            let north = rooms[source as usize].connections[D_NORTH];
            rooms[north as usize].claimed = true;
            (RoomKind::DoubleZ, source)
        }
        Fitter::DoubleX => {
            rooms[idx as usize].claimed = true;
            let east = rooms[idx as usize].connections[D_EAST];
            rooms[east as usize].claimed = true;
            (RoomKind::DoubleX, idx)
        }
        Fitter::DoubleY => {
            rooms[idx as usize].claimed = true;
            let up = rooms[idx as usize].connections[D_UP];
            rooms[up as usize].claimed = true;
            (RoomKind::DoubleY, idx)
        }
        Fitter::SimpleTop => {
            rooms[idx as usize].claimed = true;
            (RoomKind::SimpleTop, idx)
        }
        Fitter::Simple => {
            rooms[idx as usize].claimed = true;
            random.next_int(3); // OceanMonumentSimpleRoom 构造器
            (RoomKind::Simple, idx)
        }
    }
}

/// `generateRoomGraph` 的房间网格 + 扫描循环，返回打乱后的房间 index 列表
/// （末尾追加 roof/left/right）。
fn generate_room_graph(rooms: &mut [RoomDef], random: &WorldgenRandom) -> (Vec<i32>, i32) {
    // 三层网格建节点。
    for x in 0..5 {
        for z in 0..4 {
            rooms[room_index(x, 0, z) as usize].present = true;
        }
    }
    for x in 0..5 {
        for z in 0..4 {
            rooms[room_index(x, 1, z) as usize].present = true;
        }
    }
    for x in 1..4 {
        for z in 0..2 {
            rooms[room_index(x, 2, z) as usize].present = true;
        }
    }

    // 相互连接。
    for x in 0..5 {
        for z in 0..5 {
            for y in 0..3 {
                let pos = room_index(x, y, z);
                if !rooms[pos as usize].present {
                    continue;
                }
                for (d3, &(sx, sy, sz)) in STEPS.iter().enumerate() {
                    let (nx, ny, nz) = (x + sx, y + sy, z + sz);
                    if (0..5).contains(&nx) && (0..5).contains(&nz) && (0..3).contains(&ny) {
                        let np = room_index(nx, ny, nz);
                        if rooms[np as usize].present {
                            let set_dir = if sz != 0 { d3 ^ 1 } else { d3 };
                            set_connection(rooms, pos, set_dir, np);
                        }
                    }
                }
            }
        }
    }

    // 三个特殊房间。
    rooms[1003].present = true;
    rooms[1001].present = true;
    rooms[1002].present = true;
    set_connection(rooms, room_index(2, 2, 0), D_UP, 1003);
    set_connection(rooms, room_index(0, 1, 0), D_SOUTH, 1001);
    set_connection(rooms, room_index(4, 1, 0), D_SOUTH, 1002);
    rooms[1003].claimed = true;
    rooms[1001].claimed = true;
    rooms[1002].claimed = true;

    // sourceRoom.isSource，coreRoom 选择及 claimed 标记。
    rooms[room_index(2, 0, 0) as usize].is_source = true;
    let core = room_index(random.next_int(4), 0, 2);
    rooms[core as usize].claimed = true;
    let e = rooms[core as usize].connections[D_EAST];
    let n = rooms[core as usize].connections[D_NORTH];
    let u = rooms[core as usize].connections[D_UP];
    let en = rooms[e as usize].connections[D_NORTH];
    let eu = rooms[e as usize].connections[D_UP];
    let nu = rooms[n as usize].connections[D_UP];
    let enu = rooms[en as usize].connections[D_UP];
    rooms[e as usize].claimed = true;
    rooms[n as usize].claimed = true;
    rooms[en as usize].claimed = true;
    rooms[u as usize].claimed = true;
    rooms[eu as usize].claimed = true;
    rooms[nu as usize].claimed = true;
    rooms[enu as usize].claimed = true;

    // roomDefs = 非空格子节点（升序），各自 updateOpenings。
    let mut room_defs: Vec<i32> = (0..75)
        .filter(|&p| rooms[p as usize].present)
        .collect::<Vec<_>>();
    for &p in &room_defs {
        rooms[p as usize].update_openings();
    }
    rooms[1003].update_openings();

    // Util.shuffle(roomDefs, random)：for i = size; i > 1; i-- 交换 (i-1, nextInt(i))。
    let size = room_defs.len();
    for i in (2..=size).rev() {
        let s = random.next_int(i as i32) as usize;
        room_defs.swap(i - 1, s);
    }

    // 扫描循环：每节点最多 5 次尝试，每次 nextInt(6)，改开口 + findSource。
    let mut scan_index = 1;
    for &def in &room_defs {
        let mut close_count = 0;
        let mut attempt_count = 0;
        while close_count < 2 && attempt_count < 5 {
            attempt_count += 1;
            let f = random.next_int(6) as usize;
            let opens = rooms[def as usize].has_opening[f];
            if opens {
                let of = f ^ 1;
                rooms[def as usize].has_opening[f] = false;
                let neighbor = rooms[def as usize].connections[f];
                rooms[neighbor as usize].has_opening[of] = false;
                let s1 = scan_index;
                scan_index += 1;
                let r1 = find_source(rooms, def, s1);
                let keep = if r1 {
                    let s2 = scan_index;
                    scan_index += 1;
                    find_source(rooms, neighbor, s2)
                } else {
                    false
                };
                if keep {
                    close_count += 1;
                } else {
                    rooms[def as usize].has_opening[f] = true;
                    rooms[neighbor as usize].has_opening[of] = true;
                }
            }
        }
    }

    room_defs.push(1003);
    room_defs.push(1001);
    room_defs.push(1002);
    (room_defs, core)
}

/// `OceanMonumentPiece.makeBoundingBox(orientation, roomDef, w,h,d)` 的局部盒。
fn room_box(dir: HDir, index: i32, rw: i32, rh: i32, rd: i32) -> Box {
    let rx = index % 5;
    let rz = index / 5 % 5;
    let ry = index / 25;
    let mut b = if dir.axis_z() {
        new_box(0, 0, 0, rw * 8 - 1, rh * 4 - 1, rd * 8 - 1)
    } else {
        new_box(0, 0, 0, rd * 8 - 1, rh * 4 - 1, rw * 8 - 1)
    };
    let (dx, dy, dz) = match dir {
        HDir::North => (rx * 8, ry * 4, -(rz + rd) * 8 + 1),
        HDir::South => (rx * 8, ry * 4, rz * 8),
        HDir::West => (-(rz + rd) * 8 + 1, ry * 4, rx * 8),
        HDir::East => (rz * 8, ry * 4, rx * 8),
    };
    translate(&mut b, dx, dy, dz);
    b
}

/// `StructurePiece.getWorldPos(x, y, z)`。
fn world_pos(bb: &Box, dir: HDir, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
    let wx = match dir {
        HDir::North | HDir::South => bb[0] + x,
        HDir::West => bb[3] - z,
        HDir::East => bb[0] + z,
    };
    let wy = y + bb[1];
    let wz = match dir {
        HDir::North => bb[5] - z,
        HDir::South => bb[2] + z,
        HDir::West | HDir::East => bb[2] + x,
    };
    (wx, wy, wz)
}

fn box_from_corners(a: (i32, i32, i32), b: (i32, i32, i32)) -> Box {
    new_box(
        a.0.min(b.0),
        a.1.min(b.1),
        a.2.min(b.2),
        a.0.max(b.0),
        a.1.max(b.1),
        a.2.max(b.2),
    )
}

/// 造一个子件的 `Piece`（genDepth 一律 1，朝向 = direction，无子类存档数据）。
fn child_piece(kind: RoomKind, bb: Box, dir: HDir) -> Piece {
    Piece::oriented(kind.id(), bb, 1, dir)
}

/// 生成海底神殿的结构件列表：第 0 项是 MonumentBuilding，其余是 childPieces
/// （入口、核心、各房间、两翼、阁楼），包围盒已含 `getWorldPos` 平移。
///
/// Build the ocean monument piece list: index 0 is the MonumentBuilding, the rest
/// are childPieces (entry, core, fitted rooms, wings, penthouse), with boxes
/// already translated via `getWorldPos`.
pub fn generate(seed: i64, chunk_x: i32, chunk_z: i32) -> Vec<Piece> {
    let random = make_random(seed, chunk_x, chunk_z);
    // createTopPiece：Direction.Plane.HORIZONTAL.getRandomDirection(random)。
    let dir = HDir::random(&random);
    let west = chunk_x * 16 - 29;
    let north = chunk_z * 16 - 29;
    // MonumentBuilding 包围盒：width==depth==58，两轴结果相同。
    let bb = new_box(west, 39, north, west + 57, 39 + 22, north + 57);

    let mut rooms: Vec<RoomDef> = (0..1004).map(|_| RoomDef::empty()).collect();
    let (room_defs, core) = generate_room_graph(&mut rooms, &random);

    // 构造器：sourceRoom.claimed = true（generateRoomGraph 之后）。
    let source = room_index(2, 0, 0);
    rooms[source as usize].claimed = true;

    let mut children: Vec<Piece> = Vec::new();
    // 入口与核心（genDepth 1）。
    children.push(child_piece(
        RoomKind::Entry,
        room_box(dir, source, 1, 1, 1),
        dir,
    ));
    let (cw, ch, cd) = RoomKind::Core.dims();
    children.push(child_piece(
        RoomKind::Core,
        room_box(dir, core, cw, ch, cd),
        dir,
    ));

    // 七个 fitter 逐个尝试。
    for &def in &room_defs {
        if rooms[def as usize].claimed || def >= 75 {
            continue;
        }
        for &fitter in &FITTERS {
            if fitter_fits(fitter, &rooms, def) {
                let (kind, box_idx) = fitter_create(fitter, &mut rooms, def, &random);
                let (rw, rh, rd) = kind.dims();
                children.push(child_piece(kind, room_box(dir, box_idx, rw, rh, rd), dir));
                break;
            }
        }
    }

    // offset = getWorldPos(9, 0, 22)；平移已有子件。
    let off = world_pos(&bb, dir, 9, 0, 22);
    for p in children.iter_mut() {
        translate(&mut p.bb, off.0, off.1, off.2);
    }

    // 两翼与阁楼：包围盒已是世界坐标，不再平移。
    let left = box_from_corners(world_pos(&bb, dir, 1, 1, 1), world_pos(&bb, dir, 23, 8, 21));
    let right = box_from_corners(
        world_pos(&bb, dir, 34, 1, 1),
        world_pos(&bb, dir, 56, 8, 21),
    );
    let penthouse = box_from_corners(
        world_pos(&bb, dir, 22, 13, 22),
        world_pos(&bb, dir, 35, 17, 35),
    );
    // wingRandom = random.nextInt()（无界 nextInt = next(32)），仅影响未存档的 mainDesign。
    random.next_int_unbounded();
    children.push(child_piece(RoomKind::Wing, left, dir));
    children.push(child_piece(RoomKind::Wing, right, dir));
    children.push(child_piece(RoomKind::Penthouse, penthouse, dir));

    let mut result = Vec::with_capacity(children.len() + 1);
    result.push(Piece::oriented(RoomKind::Building.id(), bb, 0, dir));
    result.extend(children);
    result
}
