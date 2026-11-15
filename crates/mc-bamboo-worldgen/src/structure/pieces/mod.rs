//! 多件式结构的结构件「布局」生成：已移植废弃矿井、要塞、海底神殿；
//! 林地府邸（`MansionGrid` / `createMansion`）待办。
//!
//! 这里只产出结构件列表（类型、包围盒、朝向、生成深度、各结构件的参数）以及它们
//! 的随机源取数顺序，不把方块写进世界（`postProcess`），也不读 `.nbt` 模板内容
//! （林地府邸只用模板名和位置）。每个结构的入口是一个纯函数：给定种子、区块坐标
//! 和少量由外部（terrain 阶段）算出的输入，返回结构件列表。
//!
//! Layout generation for the four code-built multi-piece structures: mineshaft,
//! stronghold, ocean monument and woodland mansion. This yields only the piece
//! list (type, bounding box, orientation, generation depth, per-piece parameters)
//! and reproduces the vanilla random-source draw order; it does not place blocks
//! (`postProcess`) or read `.nbt` template contents (the mansion uses template
//! names and positions only). Each structure exposes one pure function.

pub mod mineshaft;
pub mod mineshaft_place;
pub mod ocean_monument;
pub mod stronghold;
pub mod stronghold_place;
pub mod woodland_mansion;

use std::collections::BTreeMap;

use crate::random::WorldgenRandom;

/// `ChunkPos.getBlockX(offset)` / `getBlockZ(offset)` = `chunk * 16 + offset`。
///
/// Vanilla `ChunkPos.getBlockX(offset)` / `getBlockZ(offset)` = `chunk * 16 + offset`.
pub fn block_at(chunk: i32, offset: i32) -> i32 {
    chunk * 16 + offset
}

/// 官方 `Direction` 的水平四方向。`get2DDataValue()`：SOUTH=0、WEST=1、NORTH=2、EAST=3。
///
/// The four horizontal `Direction` values. `get2DDataValue()`: SOUTH=0, WEST=1, NORTH=2, EAST=3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HDir {
    South,
    West,
    North,
    East,
}

impl HDir {
    /// `Direction.from2DDataValue(data)`。
    pub fn from_2d(data: i32) -> Self {
        match data.rem_euclid(4) {
            0 => Self::South,
            1 => Self::West,
            2 => Self::North,
            _ => Self::East,
        }
    }

    /// `Direction.get2DDataValue()`，写进 NBT 的 `O`。
    pub fn to_2d(self) -> i32 {
        match self {
            Self::South => 0,
            Self::West => 1,
            Self::North => 2,
            Self::East => 3,
        }
    }

    /// `Direction.Plane.HORIZONTAL.faces` = `[NORTH, EAST, SOUTH, WEST]`；
    /// `getRandomDirection` = `faces[random.nextInt(4)]`。
    ///
    /// `Direction.Plane.HORIZONTAL.faces` = `[NORTH, EAST, SOUTH, WEST]`;
    /// `getRandomDirection` = `faces[random.nextInt(4)]`.
    pub fn random(random: &WorldgenRandom) -> Self {
        match random.next_int(4) {
            0 => Self::North,
            1 => Self::East,
            2 => Self::South,
            _ => Self::West,
        }
    }

    /// `Direction.getStepX/Y/Z()`（水平方向 Y 恒为 0）。
    pub fn step(self) -> (i32, i32, i32) {
        match self {
            Self::South => (0, 0, 1),
            Self::West => (-1, 0, 0),
            Self::North => (0, 0, -1),
            Self::East => (1, 0, 0),
        }
    }

    /// `Direction.getOpposite()`（水平方向内）。
    pub fn opposite(self) -> Self {
        match self {
            Self::South => Self::North,
            Self::West => Self::East,
            Self::North => Self::South,
            Self::East => Self::West,
        }
    }

    /// `Direction.Axis == Z`。
    pub fn axis_z(self) -> bool {
        matches!(self, Self::North | Self::South)
    }

    /// `StructurePiece.setOrientation(direction)` 同时按方向设置 `rotation`：
    /// NORTH/SOUTH → NONE(0)，WEST/EAST → CLOCKWISE_90(1)。
    pub fn rotation_ordinal(self) -> i32 {
        match self {
            Self::North | Self::South => 0,
            Self::West | Self::East => 1,
        }
    }

    /// `StructurePiece.setOrientation(direction)` 同时按方向设置 `mirror`：
    /// NORTH/EAST → NONE(0)，SOUTH/WEST → LEFT_RIGHT(1)。
    pub fn mirror_ordinal(self) -> i32 {
        match self {
            Self::North | Self::East => 0,
            Self::South | Self::West => 1,
        }
    }
}

/// `BoundingBox` 的六个 int：`[minX, minY, minZ, maxX, maxY, maxZ]`。
///
/// Six ints of a `BoundingBox`: `[minX, minY, minZ, maxX, maxY, maxZ]`.
pub type Box = [i32; 6];

/// `BoundingBox(x0, y0, z0, x1, y1, z1)`。
pub fn new_box(x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) -> Box {
    [x0, y0, z0, x1, y1, z1]
}

/// `BoundingBox.move(dx, dy, dz)`。
pub fn translate(b: &mut Box, dx: i32, dy: i32, dz: i32) {
    b[0] += dx;
    b[1] += dy;
    b[2] += dz;
    b[3] += dx;
    b[4] += dy;
    b[5] += dz;
}

/// `getXSpan()` / `getYSpan()` / `getZSpan()` = `max - min + 1`。
pub fn x_span(b: &Box) -> i32 {
    b[3] - b[0] + 1
}
pub fn y_span(b: &Box) -> i32 {
    b[4] - b[1] + 1
}
pub fn z_span(b: &Box) -> i32 {
    b[5] - b[2] + 1
}

/// `BoundingBox.intersects(other)`：三轴都相交。
pub fn intersects(a: &Box, b: &Box) -> bool {
    a[3] >= b[0] && a[0] <= b[3] && a[5] >= b[2] && a[2] <= b[5] && a[4] >= b[1] && a[1] <= b[4]
}

/// `BoundingBox.encapsulatingBoxes` 的并集（至少有一个盒子）。
pub fn union(boxes: &[Box]) -> Box {
    let mut result = [i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN];
    for b in boxes {
        for i in 0..3 {
            result[i] = result[i].min(b[i]);
            result[i + 3] = result[i + 3].max(b[i + 3]);
        }
    }
    result
}

/// `BoundingBox.getCenter()` 的 Y：`minY + (maxY - minY + 1) / 2`。
pub fn center_y(b: &Box) -> i32 {
    b[1] + y_span(b) / 2
}

/// 一条结构件的布局数据。字段刻意比 `start.rs::StructurePiece` 多（rotation / mirror /
/// 子类参数），因为对拍要逐项比到这些字段；和存档用的 `StructurePiece` 保持独立，
/// 由结构放置里程碑再把两者接起来。
///
/// One piece of layout data. Intentionally richer than `start.rs::StructurePiece`
/// (rotation / mirror / subclass params) because the oracle compares every field;
/// kept separate from the save-time `StructurePiece` until the placement milestone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    /// `BuiltInRegistries.STRUCTURE_PIECE.getKey(type)`，例如 `minecraft:msroom`。
    pub id: String,
    pub bb: Box,
    /// NBT `O`：朝向的 2D 值，null 为 -1。
    pub o: i32,
    /// NBT `GD`：genDepth。
    pub gd: i32,
    /// `getRotation().ordinal()`，null 为 -1。
    pub rot: i32,
    /// `getMirror().ordinal()`，null 为 -1。
    pub mirror: i32,
    /// `addAdditionalSaveData` 里除 `id/BB/O/GD` 外的键，按 key 排序；值保留 NBT 类型，
    /// 存档时逐键写出官方类型（布尔是 Byte、`Num` 是 Int、`D` 是 Byte、模板名是 String）。
    pub extras: BTreeMap<String, Extra>,
    /// `MineShaftRoom` 的 `Entrances`：一组包围盒，随 `move` 一起平移。
    /// 其它结构件为空。为空时不写 `Entrances` 键。
    pub entrances: Vec<Box>,
}

/// 一条结构件子类存档字段的值，对应官方 `addAdditionalSaveData` 写的 NBT 类型。
/// 只覆盖这四个多件式结构用到的类型：`putBoolean` → Byte，`putInt` → Int，
/// `store(String 枚举 codec)` → String，`store(BYTE codec)`（`Direction` 的 `D`）→ Byte。
///
/// One subclass save value, matching the NBT type written by vanilla
/// `addAdditionalSaveData`: `putBoolean` → Byte, `putInt` → Int, `store(String enum
/// codec)` → String, and `store(BYTE codec)` (the mineshaft `D`) → Byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Extra {
    Byte(i8),
    Int(i32),
    Str(String),
}

impl Extra {
    /// 转成存档用的 `Tag`，类型和官方逐键一致。
    ///
    /// Convert to a save `Tag` with the exact vanilla type.
    pub fn to_tag(&self) -> mc_bamboo_nbt::Tag {
        match self {
            Self::Byte(v) => mc_bamboo_nbt::Tag::Byte(*v),
            Self::Int(v) => mc_bamboo_nbt::Tag::Int(*v),
            Self::Str(v) => mc_bamboo_nbt::Tag::String(v.clone()),
        }
    }

    /// 对拍用的标准串：数值带 NBT 类型前缀（`B:` / `I:` …），和 oracle 的 `tagTypeName`
    /// 一致；字符串原样。压平数值会让 Byte/Int 写错也发现不了。
    ///
    /// Oracle token: numbers carry an NBT type prefix (`B:` / `I:` ...) matching the
    /// oracle's `tagTypeName`; strings are verbatim. Flattening numbers would hide
    /// Byte-vs-Int mistakes.
    pub fn token(&self) -> String {
        match self {
            Self::Byte(v) => format!("B:{v}"),
            Self::Int(v) => format!("I:{v}"),
            Self::Str(v) => v.clone(),
        }
    }
}

impl Piece {
    /// 没有朝向的基类结构件（rotation / mirror 为 null → -1）。
    pub fn plain(id: &str, bb: Box, gd: i32) -> Self {
        Self {
            id: id.to_owned(),
            bb,
            o: -1,
            gd,
            rot: -1,
            mirror: -1,
            extras: BTreeMap::new(),
            entrances: Vec::new(),
        }
    }

    /// 调过 `setOrientation(direction)` 的结构件：由方向推出 `O/rotation/mirror`。
    pub fn oriented(id: &str, bb: Box, gd: i32, dir: HDir) -> Self {
        Self {
            id: id.to_owned(),
            bb,
            o: dir.to_2d(),
            gd,
            rot: dir.rotation_ordinal(),
            mirror: dir.mirror_ordinal(),
            extras: BTreeMap::new(),
            entrances: Vec::new(),
        }
    }

    pub fn put_int(&mut self, key: &str, value: i32) {
        self.extras.insert(key.to_owned(), Extra::Int(value));
    }

    /// 浮点 extras：按 Java `Float.toString` 的写法（整数也带 `.0`），对拍两侧一致。
    ///
    /// Float extras written the way Java's `Float.toString` does (integral values keep a
    /// `.0`), so both oracle sides agree.
    pub fn put_float(&mut self, key: &str, value: f32) {
        let text = format!("{value}");
        let text = if text.contains('.') || text.contains('e') || text.contains("inf") {
            text
        } else {
            format!("{text}.0")
        };
        self.extras.insert(key.to_owned(), Extra::Str(text));
    }

    pub fn put_bool(&mut self, key: &str, value: bool) {
        self.extras
            .insert(key.to_owned(), Extra::Byte(i8::from(value)));
    }

    /// `tag.store(key, Direction.LEGACY_ID_CODEC_2D, dir)`：`Codec.BYTE`，不是 Int。
    ///
    /// `tag.store(key, Direction.LEGACY_ID_CODEC_2D, dir)`: a `Codec.BYTE`, not an Int.
    pub fn put_byte(&mut self, key: &str, value: i32) {
        self.extras.insert(key.to_owned(), Extra::Byte(value as i8));
    }

    pub fn put_str(&mut self, key: &str, value: &str) {
        self.extras
            .insert(key.to_owned(), Extra::Str(value.to_owned()));
    }

    /// 取字符串字段（`Extras` 里非字符串返回 `None`）。测试和放置里程碑读模板名用。
    ///
    /// Read a string field (`None` for non-string extras). Used by tests and the
    /// placement milestone to read the template name.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.extras.get(key) {
            Some(Extra::Str(value)) => Some(value.as_str()),
            _ => None,
        }
    }

    /// 对拍用的标准行：`P id bb6 O GD rot mirror extras`。
    /// extras 是按 key 排序的 `k=v`，用逗号连接，空则为 `-`。
    /// `Entrances` 在这时按当前（已平移过的）盒子格式化。
    ///
    /// Canonical oracle line: `P id bb6 O GD rot mirror extras`. Extras are sorted
    /// `k=v` pairs joined by commas, `-` when empty. `Entrances` is formatted here
    /// from the current (already-translated) boxes.
    pub fn canonical_line(&self) -> String {
        let mut extras: BTreeMap<String, String> = self
            .extras
            .iter()
            .map(|(k, v)| (k.clone(), v.token()))
            .collect();
        if !self.entrances.is_empty() {
            let mut parts = Vec::with_capacity(self.entrances.len());
            for b in &self.entrances {
                parts.push(format!(
                    "{}_{}_{}_{}_{}_{}",
                    b[0], b[1], b[2], b[3], b[4], b[5]
                ));
            }
            extras.insert("Entrances".to_owned(), parts.join("/"));
        }
        let extras_token = if extras.is_empty() {
            "-".to_owned()
        } else {
            extras
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        format!(
            "P {} {} {} {} {} {} {} {} {} {} {} {}",
            self.id,
            self.bb[0],
            self.bb[1],
            self.bb[2],
            self.bb[3],
            self.bb[4],
            self.bb[5],
            self.o,
            self.gd,
            self.rot,
            self.mirror,
            extras_token
        )
    }
}

/// `StructurePiecesBuilder` 的等价物：按加入顺序保存的结构件列表，外加碰撞查询和
/// 竖直平移。递归 `addChildren` 过程中碰撞查询针对「当前已加入的全部结构件」。
///
/// Counterpart of `StructurePiecesBuilder`: pieces in insertion order, plus
/// collision queries and vertical translation. Collision queries during recursive
/// `addChildren` see all pieces added so far.
#[derive(Debug, Clone, Default)]
pub struct Builder {
    pub pieces: Vec<Piece>,
}

impl Builder {
    pub fn new() -> Self {
        Self { pieces: Vec::new() }
    }

    pub fn add(&mut self, piece: Piece) {
        self.pieces.push(piece);
    }

    /// `findCollisionPiece(box)`。
    pub fn collides(&self, box_: &Box) -> bool {
        self.pieces.iter().any(|p| intersects(&p.bb, box_))
    }

    /// `findCollisionPiece(box)` 返回第一个相交结构件的包围盒（FillerCorridor 用）。
    ///
    /// `findCollisionPiece(box)` returning the first intersecting piece's box.
    pub fn find_collision(&self, box_: &Box) -> Option<Box> {
        self.pieces
            .iter()
            .find(|p| intersects(&p.bb, box_))
            .map(|p| p.bb)
    }

    /// `getBoundingBox()`：所有结构件包围盒的并集。
    pub fn bounding_box(&self) -> Box {
        let boxes: Vec<Box> = self.pieces.iter().map(|p| p.bb).collect();
        union(&boxes)
    }

    /// `offsetPiecesVertically(dy)`：每个结构件的 `move(0, dy, 0)`，
    /// `MineShaftRoom` 的 `Entrances` 也一起平移。
    pub fn offset_vertically(&mut self, dy: i32) {
        for p in &mut self.pieces {
            translate(&mut p.bb, 0, dy, 0);
            for e in &mut p.entrances {
                translate(e, 0, dy, 0);
            }
        }
    }

    /// `moveBelowSeaLevel(seaLevel, minY, random, offset)` 的平移量计算与执行。
    pub fn move_below_sea_level(
        &mut self,
        sea_level: i32,
        min_y: i32,
        random: &WorldgenRandom,
        offset: i32,
    ) {
        let max_y = sea_level - offset;
        let bb = self.bounding_box();
        let mut y1_pos = y_span(&bb) + min_y + 1;
        if y1_pos < max_y {
            y1_pos += random.next_int(max_y - y1_pos);
        }
        let dy = y1_pos - bb[4];
        self.offset_vertically(dy);
    }
}

/// 造一个 `makeRandom(seed, chunkPos)`：`WorldgenRandom(LegacyRandomSource(0))`
/// 再 `setLargeFeatureSeed(seed, x, z)`。
///
/// Build a `makeRandom(seed, chunkPos)`: `WorldgenRandom(LegacyRandomSource(0))`
/// then `setLargeFeatureSeed(seed, x, z)`.
pub fn make_random(seed: i64, x: i32, z: i32) -> WorldgenRandom {
    use crate::random::Random;
    use mc_bamboo_blocks::LegacyRandom;
    let random = WorldgenRandom::new(Random::Legacy(LegacyRandom::new(0)));
    random.set_large_feature_seed(seed, x, z);
    random
}
