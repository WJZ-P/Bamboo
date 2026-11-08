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

    /// `AABB.expandTowards`：朝 `(dx, dy, dz)` 的方向把盒子拉长，反方向保持不动。
    pub fn expand_towards(self, dx: f64, dy: f64, dz: f64) -> Self {
        Self::new(
            if dx < 0.0 { self.min_x + dx } else { self.min_x },
            if dy < 0.0 { self.min_y + dy } else { self.min_y },
            if dz < 0.0 { self.min_z + dz } else { self.min_z },
            if dx > 0.0 { self.max_x + dx } else { self.max_x },
            if dy > 0.0 { self.max_y + dy } else { self.max_y },
            if dz > 0.0 { self.max_z + dz } else { self.max_z },
        )
    }

    /// `AABB.inflate`：三根轴上各往外撑一点。
    pub const fn inflate(self, dx: f64, dy: f64, dz: f64) -> Self {
        Self::new(
            self.min_x - dx,
            self.min_y - dy,
            self.min_z - dz,
            self.max_x + dx,
            self.max_y + dy,
            self.max_z + dz,
        )
    }

    /// `AABB.deflate`：三根轴上各往里收一点。
    pub const fn deflate(self, dx: f64, dy: f64, dz: f64) -> Self {
        self.inflate(-dx, -dy, -dz)
    }

    /// `AABB.intersects(min, max)`：有正体积的重叠，连 `EPSILON` 都不给。
    ///
    /// 官方这个方法和 [`Aabb::overlaps`] 用的判断不一样：`overlaps` 带容差，这个不带。
    pub const fn intersects(self, min_x: f64, min_y: f64, min_z: f64, max_x: f64, max_y: f64, max_z: f64) -> bool {
        self.min_x < max_x
            && self.max_x > min_x
            && self.min_y < max_y
            && self.max_y > min_y
            && self.min_z < max_z
            && self.max_z > min_z
    }

    /// `AABB.contains(Vec3)`：下界算在里面，上界不算。
    pub fn contains(self, x: f64, y: f64, z: f64) -> bool {
        x >= self.min_x
            && x < self.max_x
            && y >= self.min_y
            && y < self.max_y
            && z >= self.min_z
            && z < self.max_z
    }

    /// `AABB.getCenter`。
    pub const fn center(self) -> crate::Vec3 {
        crate::Vec3::new(
            (self.min_x + self.max_x) / 2.0,
            (self.min_y + self.max_y) / 2.0,
            (self.min_z + self.max_z) / 2.0,
        )
    }

    /// `AABB.getXsize`、`getYsize`、`getZsize`。
    pub const fn size(self, axis: Axis) -> f64 {
        self.max(axis) - self.min(axis)
    }

    /// 线段 `from` 到 `to` 和这个盒子有没有交点，对应官方的 `AABB.clip(...).isPresent()`。
    ///
    /// 线段完全在盒子里时也算碰到；起点和终点重合时只看点在不在盒子里。
    pub fn clip_is_present(self, from: crate::Vec3, to: crate::Vec3) -> bool {
        if self.contains(from.x, from.y, from.z) || self.contains(to.x, to.y, to.z) {
            return true;
        }
        let mut near = 0.0_f64;
        let mut far = 1.0_f64;
        let axes = [
            (from.x, to.x - from.x, self.min_x, self.max_x),
            (from.y, to.y - from.y, self.min_y, self.max_y),
            (from.z, to.z - from.z, self.min_z, self.max_z),
        ];
        for (start, delta, min, max) in axes {
            if delta.abs() < EPSILON {
                if start < min || start >= max {
                    return false;
                }
            } else {
                let first = (min - start) / delta;
                let second = (max - start) / delta;
                let (low, high) = if first < second {
                    (first, second)
                } else {
                    (second, first)
                };
                near = near.max(low);
                far = far.min(high);
                if near > far {
                    return false;
                }
            }
        }
        near < 1.0 && far > 0.0
    }

    /// `AABB.collidedAlongVector`：盒子扫过这个向量时有没有碰到 `parts` 里的任何一个。
    ///
    /// 把 `parts` 按盒子自己的一半尺寸撑大，再看向量的两端在不在里面、线段穿不穿过去。
    pub fn collided_along_vector(self, vector: crate::Vec3, parts: &[Aabb]) -> bool {
        let from = self.center();
        let to = from.add(vector);
        let half = [
            self.size(Axis::X) * 0.5 - EPSILON,
            self.size(Axis::Y) * 0.5 - EPSILON,
            self.size(Axis::Z) * 0.5 - EPSILON,
        ];
        parts.iter().any(|&part| {
            let inflated = part.inflate(half[0], half[1], half[2]);
            inflated.contains(to.x, to.y, to.z)
                || inflated.contains(from.x, from.y, from.z)
                || inflated.clip_is_present(from, to)
        })
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
