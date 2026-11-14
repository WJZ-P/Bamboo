//! `WoodlandMansionStructure` / `WoodlandMansionPieces` 的结构件布局（`MansionGrid` +
//! `MansionPiecePlacer.createMansion`）。
//!
//! 逐行照官方 26.3 翻译：网格生成、走廊/房间识别、三楼、外墙遍历、屋顶、三类房间名
//! 随机、门朝向随机，以及每个 `WoodlandMansionPiece` 的模板名、位置、rotation、mirror。
//! `BB` 用 `structure::template::StructureTemplate` 读导出模板的真实尺寸，照
//! `getBoundingBox(placeSettings, position)` 的 mirror/rotation 变换算出，因此 BB 也逐项比。
//! 不含 `postProcess`（放置那一步）。
//!
//! Layout of `WoodlandMansionStructure` / `WoodlandMansionPieces` (`MansionGrid` +
//! `MansionPiecePlacer.createMansion`). Translated line by line from vanilla 26.3: grid
//! generation, corridor/room identification, third floor, wall traversal, roof, the three
//! room-collection randoms, door-direction random, and each piece's template name,
//! position, rotation and mirror. `BB` is computed from the exported template's real
//! size via `structure::template::StructureTemplate`, mirroring
//! `getBoundingBox(placeSettings, position)`, so BB is compared too. No `postProcess`.

pub mod grid;
pub mod placer;

use std::collections::HashMap;

use mc_bamboo_blocks::LegacyRandom;

use crate::{
    random::{Random, WorldgenRandom},
    structure::{pieces::Piece, template::StructureTemplate},
};

use super::HDir;

/// 官方 `net.minecraft.core.Direction` 的水平四方向 + 竖直两方向。
/// `get3DDataValue()`：DOWN=0 UP=1 NORTH=2 SOUTH=3 WEST=4 EAST=5。
///
/// Vanilla `net.minecraft.core.Direction` values; ordinal is `get3DDataValue()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Dir {
    /// `getStepX/Y/Z()`。
    pub fn step(self) -> (i32, i32, i32) {
        match self {
            Self::Down => (0, -1, 0),
            Self::Up => (0, 1, 0),
            Self::North => (0, 0, -1),
            Self::South => (0, 0, 1),
            Self::West => (-1, 0, 0),
            Self::East => (1, 0, 0),
        }
    }

    /// `get2DDataValue()`（竖直方向为 -1，本布局里用不到）。
    pub fn to_2d(self) -> i32 {
        match self {
            Self::South => 0,
            Self::West => 1,
            Self::North => 2,
            Self::East => 3,
            _ => -1,
        }
    }

    /// `Direction.from2DDataValue(data)`：`BY_2D_DATA = [SOUTH, WEST, NORTH, EAST]`。
    pub fn from_2d(data: i32) -> Self {
        match data.rem_euclid(4) {
            0 => Self::South,
            1 => Self::West,
            2 => Self::North,
            _ => Self::East,
        }
    }

    /// `getOpposite()`。
    pub fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }

    /// `getClockWise()`（仅水平方向有效）。
    pub fn clock_wise(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::South => Self::West,
            Self::West => Self::North,
            Self::East => Self::South,
            _ => self,
        }
    }

    /// `getCounterClockWise()`（仅水平方向有效）。
    pub fn counter_clock_wise(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::South => Self::East,
            Self::West => Self::South,
            Self::East => Self::North,
            _ => self,
        }
    }
}

/// `Direction.Plane.HORIZONTAL` 的迭代顺序 `[NORTH, EAST, SOUTH, WEST]`。
///
/// `Direction.Plane.HORIZONTAL` iteration order `[NORTH, EAST, SOUTH, WEST]`.
pub const HORIZONTAL: [Dir; 4] = [Dir::North, Dir::East, Dir::South, Dir::West];

/// 官方 `Rotation`，ordinal 就是 `getIndex()`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rot {
    None,
    Cw90,
    Cw180,
    Ccw90,
}

impl Rot {
    pub fn ordinal(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Cw90 => 1,
            Self::Cw180 => 2,
            Self::Ccw90 => 3,
        }
    }

    /// `Rotation.getRandom(random)` = `values()[nextInt(4)]`。
    pub fn random(random: &WorldgenRandom) -> Self {
        match random.next_int(4) {
            0 => Self::None,
            1 => Self::Cw90,
            2 => Self::Cw180,
            _ => Self::Ccw90,
        }
    }

    /// `getRotated(rot)` = `(this + rot) mod 4`。
    pub fn rotated(self, other: Self) -> Self {
        match (self.ordinal() + other.ordinal()) % 4 {
            0 => Self::None,
            1 => Self::Cw90,
            2 => Self::Cw180,
            _ => Self::Ccw90,
        }
    }

    /// `Rotation.rotate(Direction)`（Y 轴方向不变）。
    pub fn rotate_dir(self, dir: Dir) -> Dir {
        if matches!(dir, Dir::Up | Dir::Down) {
            return dir;
        }
        match self {
            Self::Cw90 => dir.clock_wise(),
            Self::Cw180 => dir.opposite(),
            Self::Ccw90 => dir.counter_clock_wise(),
            Self::None => dir,
        }
    }

    /// `Rotation.LEGACY_CODEC` 的 `Enum::toString` 值（大写枚举名）。
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Cw90 => "CLOCKWISE_90",
            Self::Cw180 => "CLOCKWISE_180",
            Self::Ccw90 => "COUNTERCLOCKWISE_90",
        }
    }
}

/// 官方 `Mirror`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mir {
    None,
    LeftRight,
    FrontBack,
}

impl Mir {
    /// `Mirror.LEGACY_CODEC` 的 `Enum::toString` 值。
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::LeftRight => "LEFT_RIGHT",
            Self::FrontBack => "FRONT_BACK",
        }
    }
}

/// `BlockPos` 的三元组 `(x, y, z)`。
pub type Pos = (i32, i32, i32);

/// `BlockPos.relative(direction, n)`。
pub fn relative(p: Pos, dir: Dir, n: i32) -> Pos {
    let (sx, sy, sz) = dir.step();
    (p.0 + sx * n, p.1 + sy * n, p.2 + sz * n)
}

/// `BlockPos.above(n)`。
pub fn above(p: Pos, n: i32) -> Pos {
    (p.0, p.1 + n, p.2)
}

/// `BlockPos.offset(dx, dy, dz)`。
pub fn offset(p: Pos, dx: i32, dy: i32, dz: i32) -> Pos {
    (p.0 + dx, p.1 + dy, p.2 + dz)
}

/// `BlockPos.rotate(Rotation)`。
pub fn rotate_pos(p: Pos, rot: Rot) -> Pos {
    match rot {
        Rot::Cw90 => (-p.2, p.1, p.0),
        Rot::Cw180 => (-p.0, p.1, -p.2),
        Rot::Ccw90 => (p.2, p.1, -p.0),
        Rot::None => p,
    }
}

/// `WoodlandMansionPieces.SimpleGrid`。越界返回 `value_if_outside`。
#[derive(Debug, Clone)]
pub struct SimpleGrid {
    pub width: i32,
    pub height: i32,
    pub value_if_outside: i32,
    pub cells: Vec<i32>,
}

impl SimpleGrid {
    pub fn new(width: i32, height: i32, value_if_outside: i32) -> Self {
        Self {
            width,
            height,
            value_if_outside,
            cells: vec![0; (width * height) as usize],
        }
    }

    pub fn get(&self, x: i32, y: i32) -> i32 {
        if x >= 0 && x < self.width && y >= 0 && y < self.height {
            self.cells[(y * self.width + x) as usize]
        } else {
            self.value_if_outside
        }
    }

    /// 单点 `set` 只写合法坐标。
    pub fn set(&mut self, x: i32, y: i32, value: i32) {
        if x >= 0 && x < self.width && y >= 0 && y < self.height {
            self.cells[(y * self.width + x) as usize] = value;
        }
    }

    /// 矩形 `set(x0, y0, x1, y1, value)`（含端点，逐格调单点 set）。
    pub fn set_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, value: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, value);
            }
        }
    }

    /// `setif(x, y, ifValue, value)`。
    pub fn set_if(&mut self, x: i32, y: i32, if_value: i32, value: i32) {
        if self.get(x, y) == if_value {
            self.set(x, y, value);
        }
    }

    /// `edgesTo(x, y, ifValue)`。
    pub fn edges_to(&self, x: i32, y: i32, if_value: i32) -> bool {
        self.get(x - 1, y) == if_value
            || self.get(x + 1, y) == if_value
            || self.get(x, y + 1) == if_value
            || self.get(x, y - 1) == if_value
    }
}

/// `MansionGrid.isHouse`：值为 1/2/3/4。
pub fn is_house(grid: &SimpleGrid, x: i32, y: i32) -> bool {
    matches!(grid.get(x, y), 1..=4)
}

/// 一条 `WoodlandMansionPiece` 的布局数据（模板名 / 位置 / rotation / mirror）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MansionPiece {
    pub template: String,
    pub pos: Pos,
    pub rot: Rot,
    pub mir: Mir,
}

/// `StructureTemplate.getZeroPositionWithTransform(zeroPos, mirror, rotation, sizeX, sizeZ)`。
pub fn zero_position_with_transform(
    zero: Pos,
    mir: Mir,
    rot: Rot,
    size_x: i32,
    size_z: i32,
) -> Pos {
    let sx = size_x - 1;
    let sz = size_z - 1;
    let mdx = if mir == Mir::FrontBack { sx } else { 0 };
    let mdz = if mir == Mir::LeftRight { sz } else { 0 };
    match rot {
        Rot::Ccw90 => offset(zero, mdz, 0, sx - mdx),
        Rot::Cw90 => offset(zero, sz - mdz, 0, mdx),
        Rot::Cw180 => offset(zero, sx - mdx, 0, sz - mdz),
        Rot::None => offset(zero, mdx, 0, mdz),
    }
}

/// `StructureTemplate.transform(pos, mirror, rotation, pivot=ZERO)`。
fn transform(p: Pos, mir: Mir, rot: Rot) -> Pos {
    let (x0, y, z0) = p;
    let (x, z) = match mir {
        Mir::LeftRight => (x0, -z0),
        Mir::FrontBack => (-x0, z0),
        Mir::None => (x0, z0),
    };
    match rot {
        Rot::Ccw90 => (z, y, -x),
        Rot::Cw90 => (-z, y, x),
        Rot::Cw180 => (-x, y, -z),
        Rot::None => (x, y, z),
    }
}

/// `StructureTemplate.getBoundingBox(placeSettings, position)`：pivot 为 ZERO。
///
/// `corner1 = transform(ZERO) = ZERO`，`corner2 = transform(size-1)`，
/// `fromCorners` 取 min/max 再 `move(position)`。
fn bounding_box(size: (i32, i32, i32), pos: Pos, rot: Rot, mir: Mir) -> super::Box {
    let delta = (size.0 - 1, size.1 - 1, size.2 - 1);
    let c2 = transform(delta, mir, rot);
    let (x0, y0, z0) = (0_i32.min(c2.0), 0_i32.min(c2.1), 0_i32.min(c2.2));
    let (x1, y1, z1) = (0_i32.max(c2.0), 0_i32.max(c2.1), 0_i32.max(c2.2));
    [
        x0 + pos.0,
        y0 + pos.1,
        z0 + pos.2,
        x1 + pos.0,
        y1 + pos.1,
        z1 + pos.2,
    ]
}

/// 生成林地府邸的结构件列表。`start_y` 由外部喂入（terrain 的
/// `getLowestYIn5by5Box`），rotation 照 `findGenerationPoint` 由
/// `Rotation.getRandom(context.random())` 取数。
///
/// Build the woodland mansion piece list. `start_y` is fed from terrain
/// (`getLowestYIn5by5Box`); rotation is drawn exactly as in `findGenerationPoint`.
pub fn generate(seed: i64, chunk_x: i32, chunk_z: i32, start_y: i32) -> Result<Vec<Piece>, String> {
    let random = WorldgenRandom::new(Random::Legacy(LegacyRandom::new(0)));
    random.set_large_feature_seed(seed, chunk_x, chunk_z);
    // findGenerationPoint 的第一处取数：Rotation.getRandom(context.random())。
    let rotation = Rot::random(&random);
    let origin = (chunk_x * 16 + 7, start_y, chunk_z * 16 + 7);

    let mansion = grid::MansionGrid::new(&random);
    let mut placer = placer::MansionPiecePlacer::new(&random);
    placer.create_mansion(origin, rotation, &mansion);

    // 按模板名缓存尺寸，避免同一模板反复解码 gzip NBT。
    let mut sizes: HashMap<String, (i32, i32, i32)> = HashMap::new();
    let mut pieces = Vec::with_capacity(placer.pieces.len());
    for mp in &placer.pieces {
        let id = format!("minecraft:woodland_mansion/{}", mp.template);
        let size = match sizes.get(&id) {
            Some(size) => *size,
            None => {
                let size = StructureTemplate::load(&id)?.size;
                sizes.insert(id.clone(), size);
                size
            }
        };
        let bb = bounding_box(size, mp.pos, mp.rot, mp.mir);
        // TemplateStructurePiece 覆写 getRotation() 返回 placeSettings.getRotation()，
        // 所以 dump 的 rot 是 piece rotation；getMirror() 未覆写，仍是 setOrientation(NORTH)
        // 得到的 NONE。O=2（NORTH 的 2D 值）、GD=0、base mirror=0。
        // TemplateStructurePiece overrides getRotation() to return
        // placeSettings.getRotation(), so the dumped rot is the piece rotation;
        // getMirror() is not overridden and stays NONE from setOrientation(NORTH).
        let mut piece = Piece::oriented("minecraft:wmp", bb, 0, HDir::North);
        piece.rot = mp.rot.ordinal();
        piece.put_str("Template", &mp.template);
        piece.put_int("TPX", mp.pos.0);
        piece.put_int("TPY", mp.pos.1);
        piece.put_int("TPZ", mp.pos.2);
        piece.put_str("Rot", mp.rot.name());
        piece.put_str("Mi", mp.mir.name());
        pieces.push(piece);
    }
    Ok(pieces)
}
