//! `Shapes` 的 AABB 布尔运算，用乘 4 的整数坐标精确复刻。
//!
//! `addPieces` 里出现的所有 AABB 边界都是 0.25 的倍数（整数边界来自
//! `AABB.of(BoundingBox)` 的 `min`、`max+1`，`deflate(0.25)` 再 ±0.25），
//! 所以乘 4 后用 `i64` 表示，半开区间 `[min, max)` 的所有布尔运算都是精确的。
//!
//! `free = aabb \ P`（P 是已占用 box 的并集，含 centerBox / 已放置的 targetBB）。
//! - `joinIsNotEmpty(free, B', ONLY_SECOND)` = `(B' \ aabb) ∪ (B' ∩ P)` 非空
//!   = B' 不完全在 aabb 内，或 B' 与某个已占用 box 相交。
//! - `joinUnoptimized(free, target, ONLY_FIRST)` = `free \ target` = 把 target 加进 P。
//!
//! `MutableObject<VoxelShape>` 在兄弟 `PieceState` 间共享，用 `Rc<RefCell<FreeSpace>>`
//! 表示（见 `tryPlacingChildren`）。
//!
//! Exact `Shapes` AABB booleans in quarter-block integer coordinates. Every AABB
//! boundary in `addPieces` is a multiple of 0.25, so scaling by 4 keeps all
//! half-open `[min, max)` boolean operations exact.

use crate::structure::BoundingBox;

/// 乘 4 的整数坐标 AABB，半开区间 `[min, max)`。
///
/// An AABB in quarter-block integer coordinates, half-open `[min, max)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Box4 {
    pub min: [i64; 3],
    pub max: [i64; 3],
}

impl Box4 {
    /// 直接从 `new AABB(min, max)` 的整数值构造（big aabb）。
    pub fn from_aabb(min: [i32; 3], max: [i32; 3]) -> Self {
        Self {
            min: min.map(i64::from),
            max: max.map(i64::from),
        }
        .scaled()
    }

    /// `AABB.of(BoundingBox)`：`[minX, minY, minZ]` 到 `[maxX+1, maxY+1, maxZ+1]`。
    pub fn from_bounds(box_: BoundingBox) -> Self {
        Self::from_aabb(
            [box_.min_x, box_.min_y, box_.min_z],
            [box_.max_x + 1, box_.max_y + 1, box_.max_z + 1],
        )
    }

    fn scaled(mut self) -> Self {
        for axis in 0..3 {
            self.min[axis] *= 4;
            self.max[axis] *= 4;
        }
        self
    }

    /// `AABB.deflate(0.25)`：每边收缩 0.25（乘 4 后 ±1）。
    pub fn deflate_quarter(&self) -> Self {
        let mut min = self.min;
        let mut max = self.max;
        for axis in 0..3 {
            min[axis] += 1;
            max[axis] -= 1;
        }
        Self { min, max }
    }

    /// 半开区间相交（共享一个面不算相交，这正是 deflate 允许贴边放置的原因）。
    pub fn intersects(&self, other: &Self) -> bool {
        (0..3).all(|axis| self.max[axis] > other.min[axis] && self.min[axis] < other.max[axis])
    }

    /// `self` 是否完全落在 `outer` 内。
    pub fn within(&self, outer: &Self) -> bool {
        (0..3).all(|axis| self.min[axis] >= outer.min[axis] && self.max[axis] <= outer.max[axis])
    }

    /// 裁到 `outer` 内（`joinUnoptimized` 只减去 aabb 内的部分）。
    pub fn clamped(&self, outer: &Self) -> Self {
        let mut min = self.min;
        let mut max = self.max;
        for axis in 0..3 {
            min[axis] = min[axis].max(outer.min[axis]);
            max[axis] = max[axis].min(outer.max[axis]);
        }
        Self { min, max }
    }

    /// 空 box（`clamped` 后可能为空）。
    pub fn is_empty(&self) -> bool {
        (0..3).any(|axis| self.min[axis] >= self.max[axis])
    }
}

/// `MutableObject<VoxelShape>` 的等价物：`aabb \ occupied`。
///
/// Counterpart of `MutableObject<VoxelShape>`: `aabb \ occupied`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeSpace {
    aabb: Box4,
    occupied: Vec<Box4>,
}

impl FreeSpace {
    /// `Shapes.join(create(aabb), create(center), ONLY_FIRST)` = `aabb \ center`。
    pub fn context(aabb: Box4, center: Box4) -> Self {
        Self {
            aabb,
            occupied: vec![center.clamped(&aabb)],
        }
    }

    /// `Shapes.create(AABB.of(sourceBB))`：整个 sourceBB 都自由，初始无已占用。
    pub fn source(source_bb: Box4) -> Self {
        Self {
            aabb: source_bb,
            occupied: Vec::new(),
        }
    }

    /// `Shapes.joinIsNotEmpty(free, probe, ONLY_SECOND)`：`(probe \ free)` 非空。
    pub fn join_is_not_empty(&self, probe: &Box4) -> bool {
        !probe.within(&self.aabb) || self.occupied.iter().any(|placed| probe.intersects(placed))
    }

    /// `Shapes.joinUnoptimized(free, box, ONLY_FIRST)`：把 box 从自由空间减去。
    pub fn subtract(&mut self, box_: Box4) {
        let clamped = box_.clamped(&self.aabb);
        if !clamped.is_empty() {
            self.occupied.push(clamped);
        }
    }

    pub fn aabb(&self) -> &Box4 {
        &self.aabb
    }
}
