//! `StrongholdStructure` / `StrongholdPieces` 的结构件布局。
//!
//! 移植 `generatePieces` 的重试循环（每次 `setLargeFeatureSeed(seed+tries, x, z)`）、
//! `resetPieces` / `updatePieceWeight` 的加权表、`generatePieceFromSmallDoor` 的
//! 加权选择、`generateAndAddPiece` 的深度/距离门、`pendingChildren` 的随机出队，
//! 以及每个子类 `createPiece` / `addChildren` 的包围盒和构造期随机。最后
//! `moveBelowSeaLevel`。不含 `postProcess`。
//!
//! Layout of `StrongholdStructure` / `StrongholdPieces`: the retry loop, weighted
//! piece table, `generatePieceFromSmallDoor`, `generateAndAddPiece`, random-driven
//! `pendingChildren` worklist, each subclass's `createPiece` / `addChildren`, and
//! the final `moveBelowSeaLevel`. No `postProcess`.

use crate::random::WorldgenRandom;

use super::{
    Box, Builder, HDir, Piece, block_at, intersects, make_random, new_box, x_span, z_span,
};

const SEA_LEVEL: i32 = 63;
const MIN_Y: i32 = -64;
const BELOW_SEA_LEVEL_OFFSET: i32 = 10;
const MAX_DEPTH: i32 = 50;
const MAX_DISTANCE: i32 = 112;
/// 重试上限：官方没有显式上限（一直重试到有传送门房间），这里设一个足够大的安全网，
/// Java 对拍用同样的值，正常种子远在此之前就命中。
const MAX_TRIES: i64 = 128;

/// 结构件类型；同时用作 `PieceWeight` 的身份（下标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Straight,
    PrisonHall,
    LeftTurn,
    RightTurn,
    RoomCrossing,
    StraightStairsDown,
    StairsDown,
    FiveCrossing,
    ChestCorridor,
    Library,
    PortalRoom,
    FillerCorridor,
    Start,
}

impl Kind {
    /// `BuiltInRegistries.STRUCTURE_PIECE` 注册名（短 id 已 lowercase）。
    fn id(self) -> &'static str {
        match self {
            Self::Straight => "minecraft:shs",
            Self::PrisonHall => "minecraft:shph",
            Self::LeftTurn => "minecraft:shlt",
            Self::RightTurn => "minecraft:shrt",
            Self::RoomCrossing => "minecraft:shrc",
            Self::StraightStairsDown => "minecraft:shssd",
            Self::StairsDown => "minecraft:shsd",
            Self::FiveCrossing => "minecraft:sh5c",
            Self::ChestCorridor => "minecraft:shcc",
            Self::Library => "minecraft:shli",
            Self::PortalRoom => "minecraft:shpr",
            Self::FillerCorridor => "minecraft:shfc",
            Self::Start => "minecraft:shstart",
        }
    }
}

/// `PieceWeight` 的一条（含下标身份）。Library / PortalRoom 的 `doPlace` 额外要求
/// `depth > 4` / `depth > 5`。
#[derive(Debug, Clone)]
struct PwEntry {
    id: usize,
    kind: Kind,
    weight: i32,
    max: i32,
    place_count: i32,
    special: Special,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Special {
    None,
    Library,
    PortalRoom,
}

impl PwEntry {
    fn do_place(&self, depth: i32) -> bool {
        let base = self.max == 0 || self.place_count < self.max;
        base && match self.special {
            Special::None => true,
            Special::Library => depth > 4,
            Special::PortalRoom => depth > 5,
        }
    }

    fn is_valid(&self) -> bool {
        self.max == 0 || self.place_count < self.max
    }
}

/// `STRONGHOLD_PIECE_WEIGHTS` 的 (kind, weight, max, special)，顺序照官方。
const WEIGHTS: [(Kind, i32, i32, Special); 11] = [
    (Kind::Straight, 40, 0, Special::None),
    (Kind::PrisonHall, 5, 5, Special::None),
    (Kind::LeftTurn, 20, 0, Special::None),
    (Kind::RightTurn, 20, 0, Special::None),
    (Kind::RoomCrossing, 10, 6, Special::None),
    (Kind::StraightStairsDown, 5, 5, Special::None),
    (Kind::StairsDown, 5, 5, Special::None),
    (Kind::FiveCrossing, 5, 4, Special::None),
    (Kind::ChestCorridor, 5, 4, Special::None),
    (Kind::Library, 10, 2, Special::Library),
    (Kind::PortalRoom, 20, 1, Special::PortalRoom),
];

/// 一个结构件的行为数据；`to_piece` 产出对拍用的 `Piece`。
#[derive(Debug, Clone)]
struct SP {
    kind: Kind,
    bb: Box,
    gd: i32,
    dir: HDir,
    entry_door: &'static str,
    left_child: bool,
    right_child: bool,
    left_low: bool,
    left_high: bool,
    right_low: bool,
    right_high: bool,
    is_source: bool,
    extra_type: i32,
    extra_tall: bool,
    extra_steps: i32,
}

impl SP {
    fn new(kind: Kind, bb: Box, gd: i32, dir: HDir) -> Self {
        Self {
            kind,
            bb,
            gd,
            dir,
            entry_door: "OPENING",
            left_child: false,
            right_child: false,
            left_low: false,
            left_high: false,
            right_low: false,
            right_high: false,
            is_source: false,
            extra_type: 0,
            extra_tall: false,
            extra_steps: 0,
        }
    }

    fn to_piece(&self) -> Piece {
        let mut p = Piece::oriented(self.kind.id(), self.bb, self.gd, self.dir);
        p.put_str("EntryDoor", self.entry_door);
        match self.kind {
            Kind::Start => p.put_bool("Source", true),
            Kind::StairsDown => p.put_bool("Source", false),
            Kind::Straight => {
                p.put_bool("Left", self.left_child);
                p.put_bool("Right", self.right_child);
            }
            Kind::RoomCrossing => p.put_int("Type", self.extra_type),
            Kind::FiveCrossing => {
                p.put_bool("leftLow", self.left_low);
                p.put_bool("leftHigh", self.left_high);
                p.put_bool("rightLow", self.right_low);
                p.put_bool("rightHigh", self.right_high);
            }
            Kind::ChestCorridor => p.put_bool("Chest", false),
            Kind::Library => p.put_bool("Tall", self.extra_tall),
            Kind::PortalRoom => p.put_bool("Mob", false),
            Kind::FillerCorridor => p.put_int("Steps", self.extra_steps),
            _ => {}
        }
        p
    }
}

/// `BoundingBox.orientBox` 的水平四方向版。
fn orient_box(
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    off_x: i32,
    off_y: i32,
    off_z: i32,
    width: i32,
    height: i32,
    depth: i32,
    dir: HDir,
) -> Box {
    match dir {
        HDir::South => new_box(
            foot_x + off_x,
            foot_y + off_y,
            foot_z + off_z,
            foot_x + width - 1 + off_x,
            foot_y + height - 1 + off_y,
            foot_z + depth - 1 + off_z,
        ),
        HDir::North => new_box(
            foot_x + off_x,
            foot_y + off_y,
            foot_z - depth + 1 + off_z,
            foot_x + width - 1 + off_x,
            foot_y + height - 1 + off_y,
            foot_z + off_z,
        ),
        HDir::West => new_box(
            foot_x - depth + 1 + off_z,
            foot_y + off_y,
            foot_z + off_x,
            foot_x + off_z,
            foot_y + height - 1 + off_y,
            foot_z + width - 1 + off_x,
        ),
        HDir::East => new_box(
            foot_x + off_z,
            foot_y + off_y,
            foot_z + off_x,
            foot_x + depth - 1 + off_z,
            foot_y + height - 1 + off_y,
            foot_z + width - 1 + off_x,
        ),
    }
}

/// `StructurePiece.makeBoundingBox` 的 (width,height,depth) 在 Z/X 轴上的处理。
fn make_box(x: i32, y: i32, z: i32, dir: HDir, width: i32, height: i32, depth: i32) -> Box {
    if dir.axis_z() {
        new_box(x, y, z, x + width - 1, y + height - 1, z + depth - 1)
    } else {
        new_box(x, y, z, x + depth - 1, y + height - 1, z + width - 1)
    }
}

/// `randomSmallDoor(random)`：`nextInt(5)`；0/1→OPENING、2→WOOD_DOOR、
/// 3→GRATES、4→IRON_DOOR。
fn random_small_door(random: &WorldgenRandom) -> &'static str {
    match random.next_int(5) {
        2 => "WOOD_DOOR",
        3 => "GRATES",
        4 => "IRON_DOOR",
        _ => "OPENING",
    }
}

/// `isOkBox(box)` = `box.minY() > 10`。
fn is_ok_box(b: &Box) -> bool {
    b[1] > 10
}

/// `findAndCreatePieceFactory`：按 kind 算盒子、查碰撞、构造（构造期消耗随机）。
/// `boxes` 是调用时 builder 里已有结构件包围盒的快照。
fn create_piece(
    kind: Kind,
    boxes: &[Box],
    random: &WorldgenRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: HDir,
    gd: i32,
) -> Option<SP> {
    let collides = |b: &Box| boxes.iter().any(|e| intersects(e, b));
    let mut sp = SP::new(kind, [0; 6], gd, dir);
    let bb = match kind {
        Kind::Straight => orient_box(foot_x, foot_y, foot_z, -1, -1, 0, 5, 5, 7, dir),
        Kind::PrisonHall => orient_box(foot_x, foot_y, foot_z, -1, -1, 0, 9, 5, 11, dir),
        Kind::LeftTurn | Kind::RightTurn => {
            orient_box(foot_x, foot_y, foot_z, -1, -1, 0, 5, 5, 5, dir)
        }
        Kind::RoomCrossing => orient_box(foot_x, foot_y, foot_z, -4, -1, 0, 11, 7, 11, dir),
        Kind::StraightStairsDown => orient_box(foot_x, foot_y, foot_z, -1, -7, 0, 5, 11, 8, dir),
        Kind::StairsDown => orient_box(foot_x, foot_y, foot_z, -1, -7, 0, 5, 11, 5, dir),
        Kind::FiveCrossing => orient_box(foot_x, foot_y, foot_z, -4, -3, 0, 10, 9, 11, dir),
        Kind::ChestCorridor => orient_box(foot_x, foot_y, foot_z, -1, -1, 0, 5, 5, 7, dir),
        Kind::PortalRoom => orient_box(foot_x, foot_y, foot_z, -4, -1, 0, 11, 8, 16, dir),
        Kind::Library => {
            let tall = orient_box(foot_x, foot_y, foot_z, -4, -1, 0, 14, 11, 15, dir);
            if !is_ok_box(&tall) || collides(&tall) {
                let short = orient_box(foot_x, foot_y, foot_z, -4, -1, 0, 14, 6, 15, dir);
                if !is_ok_box(&short) || collides(&short) {
                    return None;
                }
                sp.extra_tall = false;
                short
            } else {
                sp.extra_tall = true;
                tall
            }
        }
        Kind::FillerCorridor | Kind::Start => unreachable!(),
    };
    if !is_ok_box(&bb) || collides(&bb) {
        return None;
    }
    sp.bb = bb;

    // 构造期随机（仅当盒子有效、确实要构造时才消耗）。
    match kind {
        Kind::Straight => {
            sp.entry_door = random_small_door(random);
            sp.left_child = random.next_int(2) == 0;
            sp.right_child = random.next_int(2) == 0;
        }
        Kind::PrisonHall
        | Kind::LeftTurn
        | Kind::RightTurn
        | Kind::StraightStairsDown
        | Kind::ChestCorridor => {
            sp.entry_door = random_small_door(random);
        }
        Kind::RoomCrossing => {
            sp.entry_door = random_small_door(random);
            sp.extra_type = random.next_int(5);
        }
        Kind::StairsDown => {
            sp.entry_door = random_small_door(random);
            sp.is_source = false;
        }
        Kind::FiveCrossing => {
            sp.entry_door = random_small_door(random);
            sp.left_low = random.next_boolean();
            sp.left_high = random.next_boolean();
            sp.right_low = random.next_boolean();
            sp.right_high = random.next_int(3) > 0;
        }
        Kind::Library => {
            sp.entry_door = random_small_door(random);
            // isTall 已在上面按盒子算出。
        }
        Kind::PortalRoom => {}
        Kind::FillerCorridor | Kind::Start => unreachable!(),
    }
    Some(sp)
}

/// `Layout`：一次重试里的全部可变状态（不含随机源，随机源单独传）。
struct Layout {
    builder: Builder,
    behaviors: Vec<SP>,
    pending: Vec<usize>,
    current: Vec<PwEntry>,
    imposed: Option<Kind>,
    previous: Option<usize>,
    total_weight: i32,
    portal_found: bool,
    start_min_x: i32,
    start_min_z: i32,
}

impl Layout {
    /// `resetPieces()`：重建加权表、清空 imposed / previous / portal / builder。
    fn new() -> Self {
        let current = WEIGHTS
            .iter()
            .enumerate()
            .map(|(id, &(kind, weight, max, special))| PwEntry {
                id,
                kind,
                weight,
                max,
                place_count: 0,
                special,
            })
            .collect();
        Self {
            builder: Builder::new(),
            behaviors: Vec::new(),
            pending: Vec::new(),
            current,
            imposed: None,
            previous: None,
            total_weight: 0,
            portal_found: false,
            start_min_x: 0,
            start_min_z: 0,
        }
    }

    /// `updatePieceWeight()`：重算 totalWeight，返回是否还有可放置件。
    fn update_piece_weight(&mut self) -> bool {
        let mut has_any = false;
        let mut total = 0;
        for p in &self.current {
            if p.max > 0 && p.place_count < p.max {
                has_any = true;
            }
            total += p.weight;
        }
        self.total_weight = total;
        has_any
    }
}

/// `FillerCorridor.findPieceBox`：找出能塞下的填充走廊盒；`random` 参数官方未用。
fn find_filler_box(boxes: &[Box], foot_x: i32, foot_y: i32, foot_z: i32, dir: HDir) -> Option<Box> {
    let first = orient_box(foot_x, foot_y, foot_z, -1, -1, 0, 5, 5, 4, dir);
    let collision = boxes.iter().find(|b| intersects(b, &first)).copied()?;
    if collision[1] == first[1] {
        for depth in (1..=2).rev() {
            let b = orient_box(foot_x, foot_y, foot_z, -1, -1, 0, 5, 5, depth, dir);
            if !intersects(&collision, &b) {
                return Some(orient_box(
                    foot_x,
                    foot_y,
                    foot_z,
                    -1,
                    -1,
                    0,
                    5,
                    5,
                    depth + 1,
                    dir,
                ));
            }
        }
    }
    None
}

/// `generatePieceFromSmallDoor`：加权选择（从选中的下标起往后逐个尝试，直到成功）。
fn generate_piece_from_small_door(
    layout: &mut Layout,
    random: &WorldgenRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: HDir,
    depth: i32,
) -> Option<SP> {
    if !layout.update_piece_weight() {
        return None;
    }
    let boxes: Vec<Box> = layout.builder.pieces.iter().map(|p| p.bb).collect();

    // imposedPiece 优先尝试，且无论成功与否都清空。
    if let Some(imposed) = layout.imposed.take() {
        if let Some(sp) = create_piece(imposed, &boxes, random, foot_x, foot_y, foot_z, dir, depth)
        {
            return Some(sp);
        }
    }

    for _ in 0..5 {
        let mut weight_selection = random.next_int(layout.total_weight);
        let mut i = 0usize;
        while i < layout.current.len() {
            weight_selection -= layout.current[i].weight;
            if weight_selection < 0 {
                if !layout.current[i].do_place(depth)
                    || Some(layout.current[i].id) == layout.previous
                {
                    break;
                }
                let sp = create_piece(
                    layout.current[i].kind,
                    &boxes,
                    random,
                    foot_x,
                    foot_y,
                    foot_z,
                    dir,
                    depth,
                );
                if let Some(sp) = sp {
                    layout.current[i].place_count += 1;
                    layout.previous = Some(layout.current[i].id);
                    if !layout.current[i].is_valid() {
                        layout.current.remove(i);
                    }
                    return Some(sp);
                }
            }
            i += 1;
        }
    }

    // 5 次失败后的 FillerCorridor 兜底。
    let bb = find_filler_box(&boxes, foot_x, foot_y, foot_z, dir)?;
    if bb[1] <= 1 {
        return None;
    }
    let mut sp = SP::new(Kind::FillerCorridor, bb, depth, dir);
    sp.extra_steps = if dir.axis_z() {
        z_span(&bb)
    } else {
        x_span(&bb)
    };
    Some(sp)
}

/// `generateAndAddPiece`：深度/距离门 → 造件 → 加入 builder / behaviors / pending。
fn generate_and_add_piece(
    layout: &mut Layout,
    random: &WorldgenRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: HDir,
    depth: i32,
) -> Option<usize> {
    if depth > MAX_DEPTH {
        return None;
    }
    if (foot_x - layout.start_min_x).abs() > MAX_DISTANCE
        || (foot_z - layout.start_min_z).abs() > MAX_DISTANCE
    {
        return None;
    }
    let sp =
        generate_piece_from_small_door(layout, random, foot_x, foot_y, foot_z, dir, depth + 1)?;
    layout.builder.add(sp.to_piece());
    layout.behaviors.push(sp);
    let idx = layout.behaviors.len() - 1;
    layout.pending.push(idx);
    Some(idx)
}

/// `generateSmallDoorChildForward(start, builder, random, xOff, yOff)`。
fn child_forward(layout: &mut Layout, random: &WorldgenRandom, sp: &SP, x_off: i32, y_off: i32) {
    let bb = sp.bb;
    let (fx, fy, fz) = match sp.dir {
        HDir::North => (bb[0] + x_off, bb[1] + y_off, bb[2] - 1),
        HDir::South => (bb[0] + x_off, bb[1] + y_off, bb[5] + 1),
        HDir::West => (bb[0] - 1, bb[1] + y_off, bb[2] + x_off),
        HDir::East => (bb[3] + 1, bb[1] + y_off, bb[2] + x_off),
    };
    generate_and_add_piece(layout, random, fx, fy, fz, sp.dir, sp.gd);
}

/// `generateSmallDoorChildLeft(start, builder, random, yOff, zOff)`。
fn child_left(layout: &mut Layout, random: &WorldgenRandom, sp: &SP, y_off: i32, z_off: i32) {
    let bb = sp.bb;
    let (fx, fy, fz, d) = match sp.dir {
        HDir::North | HDir::South => (bb[0] - 1, bb[1] + y_off, bb[2] + z_off, HDir::West),
        HDir::West | HDir::East => (bb[0] + z_off, bb[1] + y_off, bb[2] - 1, HDir::North),
    };
    generate_and_add_piece(layout, random, fx, fy, fz, d, sp.gd);
}

/// `generateSmallDoorChildRight(start, builder, random, yOff, zOff)`。
fn child_right(layout: &mut Layout, random: &WorldgenRandom, sp: &SP, y_off: i32, z_off: i32) {
    let bb = sp.bb;
    let (fx, fy, fz, d) = match sp.dir {
        HDir::North | HDir::South => (bb[3] + 1, bb[1] + y_off, bb[2] + z_off, HDir::East),
        HDir::West | HDir::East => (bb[0] + z_off, bb[1] + y_off, bb[5] + 1, HDir::South),
    };
    generate_and_add_piece(layout, random, fx, fy, fz, d, sp.gd);
}

/// 各子类的 `addChildren`（`startPiece` 恒为 StartPiece，故 PortalRoom 能置 portal 标记）。
fn add_children(layout: &mut Layout, random: &WorldgenRandom, sp: &SP) {
    match sp.kind {
        Kind::Start | Kind::StairsDown => {
            if sp.is_source {
                layout.imposed = Some(Kind::FiveCrossing);
            }
            child_forward(layout, random, sp, 1, 1);
        }
        Kind::Straight => {
            child_forward(layout, random, sp, 1, 1);
            if sp.left_child {
                child_left(layout, random, sp, 1, 2);
            }
            if sp.right_child {
                child_right(layout, random, sp, 1, 2);
            }
        }
        Kind::PrisonHall | Kind::ChestCorridor | Kind::StraightStairsDown => {
            child_forward(layout, random, sp, 1, 1);
        }
        Kind::LeftTurn => {
            if sp.dir != HDir::North && sp.dir != HDir::East {
                child_right(layout, random, sp, 1, 1);
            } else {
                child_left(layout, random, sp, 1, 1);
            }
        }
        Kind::RightTurn => {
            if sp.dir != HDir::North && sp.dir != HDir::East {
                child_left(layout, random, sp, 1, 1);
            } else {
                child_right(layout, random, sp, 1, 1);
            }
        }
        Kind::RoomCrossing => {
            child_forward(layout, random, sp, 4, 1);
            child_left(layout, random, sp, 1, 4);
            child_right(layout, random, sp, 1, 4);
        }
        Kind::FiveCrossing => {
            let (mut z_off_a, mut z_off_b) = (3, 5);
            if sp.dir == HDir::West || sp.dir == HDir::North {
                z_off_a = 8 - z_off_a;
                z_off_b = 8 - z_off_b;
            }
            child_forward(layout, random, sp, 5, 1);
            if sp.left_low {
                child_left(layout, random, sp, z_off_a, 1);
            }
            if sp.left_high {
                child_left(layout, random, sp, z_off_b, 7);
            }
            if sp.right_low {
                child_right(layout, random, sp, z_off_a, 1);
            }
            if sp.right_high {
                child_right(layout, random, sp, z_off_b, 7);
            }
        }
        Kind::PortalRoom => layout.portal_found = true,
        // Library 和 FillerCorridor 没有 addChildren 覆写（基类空实现）。
        Kind::Library | Kind::FillerCorridor => {}
    }
}

/// 生成要塞的结构件列表。重试直到命中传送门房间（或达到安全上限）。
/// 返回的结构件包围盒已含 `moveBelowSeaLevel` 的竖直平移。
///
/// Build the stronghold piece list, retrying until a portal room is found (or
/// the safety cap is hit). Returned boxes include the final vertical shift.
pub fn generate(seed: i64, chunk_x: i32, chunk_z: i32) -> Vec<Piece> {
    let mut tries: i64 = 0;
    loop {
        let random = make_random(seed.wrapping_add(tries), chunk_x, chunk_z);
        tries += 1;
        let mut layout = Layout::new();

        // StartPiece：构造期 `getRandomHorizontalDirection(random)` 消耗一次 nextInt(4)。
        let dir = HDir::random(&random);
        let west = block_at(chunk_x, 2);
        let north = block_at(chunk_z, 2);
        let bb = make_box(west, 64, north, dir, 5, 11, 5);
        let start = SP {
            is_source: true,
            ..SP::new(Kind::Start, bb, 0, dir)
        };
        layout.start_min_x = bb[0];
        layout.start_min_z = bb[2];
        layout.builder.add(start.to_piece());
        layout.behaviors.push(start.clone());

        // startRoom.addChildren(startRoom, builder, random)
        add_children(&mut layout, &random, &start);

        // pendingChildren 循环：随机出队，逐个跑 addChildren。
        while !layout.pending.is_empty() {
            let pos = random.next_int(layout.pending.len() as i32) as usize;
            let idx = layout.pending.remove(pos);
            let sp = layout.behaviors[idx].clone();
            add_children(&mut layout, &random, &sp);
        }

        layout
            .builder
            .move_below_sea_level(SEA_LEVEL, MIN_Y, &random, BELOW_SEA_LEVEL_OFFSET);

        if !layout.builder.pieces.is_empty() && layout.portal_found {
            return layout.builder.pieces;
        }
        if tries >= MAX_TRIES {
            return layout.builder.pieces;
        }
    }
}
