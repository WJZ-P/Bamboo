//! 轴对齐的盒子，对应官方的 `AABB`。

use crate::{Axis, BlockPos, Vec3};

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

    /// `AABB.inflate(dx, dy, dz)`：三根轴上各往外撑一点。
    ///
    /// `AABB.inflate(dx, dy, dz)`: grow the box outward on each axis.
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

    /// `AABB.deflate(dx, dy, dz)`：三根轴上各往里收一点。
    ///
    /// `AABB.deflate(dx, dy, dz)`: shrink the box inward on each axis.
    pub const fn deflate(self, dx: f64, dy: f64, dz: f64) -> Self {
        self.inflate(-dx, -dy, -dz)
    }

    /// `AABB.intersects(min, max)`：有正体积的重叠，连 `EPSILON` 都不给。
    ///
    /// 和 [`Aabb::intersects`] 是同一种判断，参数是六个坐标而不是另一个盒子。
    ///
    /// `AABB.intersects(minX, minY, minZ, maxX, maxY, maxZ)`. Same test as
    /// [`Aabb::intersects`], taking six coordinates instead of another box.
    pub const fn intersects_coords(
        self,
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> bool {
        self.min_x < max_x
            && self.max_x > min_x
            && self.min_y < max_y
            && self.max_y > min_y
            && self.min_z < max_z
            && self.max_z > min_z
    }

    /// `AABB.contains(Vec3)`：下界算在里面，上界不算。
    ///
    /// `AABB.contains`: the minimum is inside, the maximum is not.
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
    ///
    /// Whether the segment from `from` to `to` hits this box, matching
    /// `AABB.clip(...).isPresent()`. A segment fully inside the box counts;
    /// a zero-length segment only counts when the point is inside.
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
    ///
    /// Inflate `parts` by half this box, then test the segment ends and the segment itself.
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

    /// 每个方向都向外扩大 `value`。官方的 `inflate`。
    pub const fn inflated(self, value: f64) -> Self {
        Self::new(
            self.min_x - value,
            self.min_y - value,
            self.min_z - value,
            self.max_x + value,
            self.max_y + value,
            self.max_z + value,
        )
    }

    /// 每个方向都向内缩小 `value`。官方的 `deflate`。
    pub const fn deflated(self, value: f64) -> Self {
        self.inflated(-value)
    }

    /// 每个方向按给定的距离伸缩，正数向外、负数向内。官方的 `expandTowards`。
    pub fn expanded_towards(self, x: f64, y: f64, z: f64) -> Self {
        let mut result = self;
        if x < 0.0 {
            result.min_x += x;
        } else if x > 0.0 {
            result.max_x += x;
        }
        if y < 0.0 {
            result.min_y += y;
        } else if y > 0.0 {
            result.max_y += y;
        }
        if z < 0.0 {
            result.min_z += z;
        } else if z > 0.0 {
            result.max_z += z;
        }
        result
    }

    /// 每个方向按给定的距离收缩，负数向外、正数向内。官方的 `contract`。
    ///
    /// 注意符号和 [`expanded_towards`](Self::expanded_towards) 相反：`contract(0.0, -1.0, 0.0)`
    /// 把盒子的下界抬高 1。
    pub fn contracted(self, x: f64, y: f64, z: f64) -> Self {
        let mut result = self;
        if x < 0.0 {
            result.min_x -= x;
        } else if x > 0.0 {
            result.max_x -= x;
        }
        if y < 0.0 {
            result.min_y -= y;
        } else if y > 0.0 {
            result.max_y -= y;
        }
        if z < 0.0 {
            result.min_z -= z;
        } else if z > 0.0 {
            result.max_z -= z;
        }
        result
    }

    /// 和 `other` 有重叠，只碰到表面不算。官方的 `intersects`，没有 epsilon。
    pub fn intersects(self, other: Self) -> bool {
        self.min_x < other.max_x
            && self.max_x > other.min_x
            && self.min_y < other.max_y
            && self.max_y > other.min_y
            && self.min_z < other.max_z
            && self.max_z > other.min_z
    }

    /// 这个盒子到 `point` 的平方距离，点在盒子里时是零。官方的 `distanceToSqr`。
    pub fn distance_to_sqr(self, point: Vec3) -> f64 {
        let axis = |min: f64, max: f64, value: f64| (min - value).max(value - max).max(0.0);
        let dx = axis(self.min_x, self.max_x, point.x);
        let dy = axis(self.min_y, self.max_y, point.y);
        let dz = axis(self.min_z, self.max_z, point.z);
        dx * dx + dy * dy + dz * dz
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

    #[test]
    fn expanding_and_contracting_follow_the_official_signs() {
        let box_ = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        assert_eq!(
            box_.expanded_towards(0.5, -0.5, 0.0),
            Aabb::new(0.0, -0.5, 0.0, 1.5, 1.0, 1.0)
        );
        // 负数把下界抬高，正数把上界压低。
        assert_eq!(
            box_.contracted(0.0, -1.0, 0.5),
            Aabb::new(0.0, 1.0, 0.0, 1.0, 1.0, 0.5)
        );
        assert_eq!(
            box_.inflated(0.5),
            Aabb::new(-0.5, -0.5, -0.5, 1.5, 1.5, 1.5)
        );
        assert_eq!(
            box_.deflated(0.25),
            Aabb::new(0.25, 0.25, 0.25, 0.75, 0.75, 0.75)
        );
    }

    #[test]
    fn intersect_needs_a_shared_volume_and_distance_clamps_per_axis() {
        assert!(UNIT.intersects(UNIT.moved(0.5, 0.0, 0.0)));
        // 只碰到表面不算，官方也是严格的小于。
        assert!(!UNIT.intersects(UNIT.moved(1.0, 0.0, 0.0)));
        assert!(!UNIT.intersects(UNIT.moved(1.0 + 1.0e-9, 0.0, 0.0)));
        assert_eq!(UNIT.distance_to_sqr(Vec3::new(0.5, 0.5, 0.5)), 0.0);
        assert_eq!(UNIT.distance_to_sqr(Vec3::new(2.0, 0.5, 0.5)), 1.0);
        assert_eq!(UNIT.distance_to_sqr(Vec3::new(2.0, 2.0, 2.0)), 3.0);
    }
}
