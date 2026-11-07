//! 轴对齐的盒子，对应官方的 `AABB`。

use crate::{Axis, BlockPos};

/// 比这更小的重叠不算重叠。官方合并形状的坐标时用的是同一个阈值。
const EPSILON: f64 = 1.0e-7;

/// 一个轴对齐的盒子。放在 `mc-bamboo-core` 是因为生成的形状表和实体碰撞盒都要用它。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl Aabb {
    pub const fn new(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Self {
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    /// 平移后的盒子。
    pub const fn moved(self, dx: f64, dy: f64, dz: f64) -> Self {
        Self::new(
            self.min_x + dx,
            self.min_y + dy,
            self.min_z + dz,
            self.max_x + dx,
            self.max_y + dy,
            self.max_z + dz,
        )
    }

    /// 把相对方块的盒子平移到世界坐标。
    pub fn at(self, pos: BlockPos) -> Self {
        self.moved(f64::from(pos.x), f64::from(pos.y), f64::from(pos.z))
    }

    /// 这个盒子在 `axis` 上的下界。
    pub const fn min(self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.min_x,
            Axis::Y => self.min_y,
            Axis::Z => self.min_z,
        }
    }

    /// 这个盒子在 `axis` 上的上界。
    pub const fn max(self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.max_x,
            Axis::Y => self.max_y,
            Axis::Z => self.max_z,
        }
    }

    /// 和 `other` 有正体积的重叠。只碰到表面不算，放置判定“挡路”时就是这个含义。
    pub fn overlaps(self, other: Self) -> bool {
        self.min_x + EPSILON < other.max_x
            && other.min_x + EPSILON < self.max_x
            && self.min_y + EPSILON < other.max_y
            && other.min_y + EPSILON < self.max_y
            && self.min_z + EPSILON < other.max_z
            && other.min_z + EPSILON < self.max_z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: Aabb = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

    #[test]
    fn moved_boxes_follow_the_position() {
        let moved = UNIT.at(BlockPos::new(2, -3, 4));
        assert_eq!(moved, Aabb::new(2.0, -3.0, 4.0, 3.0, -2.0, 5.0));
        assert_eq!(moved.min(Axis::Y), -3.0);
        assert_eq!(moved.max(Axis::Z), 5.0);
    }

    #[test]
    fn touching_boxes_do_not_overlap() {
        assert!(!UNIT.overlaps(UNIT.moved(1.0, 0.0, 0.0)));
        assert!(UNIT.overlaps(UNIT.moved(0.5, 0.5, 0.5)));
        assert!(!UNIT.overlaps(UNIT.moved(1.0 - 1.0e-9, 0.0, 0.0)));
    }
}
