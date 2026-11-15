//! 官方 `net.minecraft.world.level.block.Rotation` 的移植。
//!
//! 四个旋转值、`getRandom` / `getShuffled` 的取数顺序、`StructureTemplate.transform`
//! （mirror=NONE、pivot=ZERO）的位置变换、`getSize` 和 `getBoundingBox`。
//!
//! Port of vanilla `Rotation`: the four values, `getRandom` / `getShuffled` draw order,
//! `StructureTemplate.transform` (mirror NONE, pivot ZERO), `getSize`, `getBoundingBox`.

use crate::{
    random::WorldgenRandom,
    structure::{BoundingBox, template::Direction},
};

/// `Rotation` 的四个值，序号就是 `getIndex()` / `ordinal()`。
///
/// The four `Rotation` values, in `getIndex()` / `ordinal()` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    None,
    Clockwise90,
    Clockwise180,
    Counterclockwise90,
}

impl Rotation {
    pub const ALL: [Self; 4] = [
        Self::None,
        Self::Clockwise90,
        Self::Clockwise180,
        Self::Counterclockwise90,
    ];

    pub fn from_index(index: i32) -> Self {
        Self::ALL[(index as usize) % Self::ALL.len()]
    }

    pub const fn index(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Clockwise90 => 1,
            Self::Clockwise180 => 2,
            Self::Counterclockwise90 => 3,
        }
    }

    /// `Rotation.getRandom(random)` = `Util.getRandom(values(), random)`。
    pub fn get_random(random: &WorldgenRandom) -> Self {
        Self::from_index(random.next_int(Self::ALL.len() as i32))
    }

    /// `Rotation.getShuffled(random)` = `Util.shuffledCopy(values(), random)`。
    pub fn get_shuffled(random: &WorldgenRandom) -> Vec<Self> {
        let mut values = Self::ALL.to_vec();
        shuffle(&mut values, random);
        values
    }

    /// `StructureTemplate.transform(pos, NONE, rotation, ZERO)`。
    pub const fn transform(self, pos: (i32, i32, i32)) -> (i32, i32, i32) {
        let (x, y, z) = pos;
        match self {
            Self::None => (x, y, z),
            Self::Clockwise90 => (-z, y, x),
            Self::Clockwise180 => (-x, y, -z),
            Self::Counterclockwise90 => (z, y, -x),
        }
    }

    /// `JigsawBlock.rotate` / `OctahedralGroup.rotate(FrontAndTop)`：
    /// 绕 Y 轴旋转时水平方向按 clock-wise/opposite/counter-clock-wise 变，Y 方向不变。
    ///
    /// `JigsawBlock.rotate` / `OctahedralGroup.rotate(FrontAndTop)`: for Y-axis
    /// rotations horizontal directions rotate clock-wise / opposite / counter-clock-wise,
    /// vertical directions are unchanged.
    pub fn rotate_direction(self, direction: Direction) -> Direction {
        if !direction.axis_is_horizontal() {
            return direction;
        }
        match self {
            Self::None => direction,
            Self::Clockwise90 => direction.clock_wise(),
            Self::Clockwise180 => direction.opposite(),
            Self::Counterclockwise90 => direction.counter_clock_wise(),
        }
    }

    /// `StructureTemplate.getSize(rotation)`。
    pub const fn size(self, size: (i32, i32, i32)) -> (i32, i32, i32) {
        let (sx, sy, sz) = size;
        match self {
            Self::Clockwise90 | Self::Counterclockwise90 => (sz, sy, sx),
            _ => (sx, sy, sz),
        }
    }

    /// `StructureTemplate.getBoundingBox(settings(rotation), position)`。
    ///
    /// `transform` 两个角点，`BoundingBox.fromCorners` 取 min/max，再 `.move(position)`。
    pub fn bounding_box(self, position: (i32, i32, i32), size: (i32, i32, i32)) -> BoundingBox {
        let (sx, sy, sz) = size;
        let first = self.transform((0, 0, 0));
        let second = self.transform((sx - 1, sy - 1, sz - 1));
        BoundingBox {
            min_x: first.0.min(second.0) + position.0,
            min_y: first.1.min(second.1) + position.1,
            min_z: first.2.min(second.2) + position.2,
            max_x: first.0.max(second.0) + position.0,
            max_y: first.1.max(second.1) + position.1,
            max_z: first.2.max(second.2) + position.2,
        }
    }
}

/// `Util.shuffle(list, random)`：从尾部往前的 Fisher-Yates。
///
/// `Util.shuffle(list, random)`: Fisher-Yates from the end.
pub fn shuffle<T>(items: &mut [T], random: &WorldgenRandom) {
    for i in (2..=items.len()).rev() {
        let swap_to = random.next_int(i as i32) as usize;
        items.swap(i - 1, swap_to);
    }
}

/// `Util.shuffledCopy(list, random)`：复制后洗牌。
///
/// `Util.shuffledCopy(list, random)`: copy then shuffle.
pub fn shuffled_copy<T: Clone>(items: &[T], random: &WorldgenRandom) -> Vec<T> {
    let mut copy = items.to_vec();
    shuffle(&mut copy, random);
    copy
}
