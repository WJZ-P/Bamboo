//! 官方 `DensityVolume`：成批求值的那一块方块网格。
//!
//! 一块体积是 `size` 个点、从 `min` 开始、每步走 `step` 格。缓冲区的下标是
//! `y + sizeY * (x + sizeX * z)`，y 变化最快，和官方 `indexUnchecked` 一样。

use crate::math::java_floor_div;

/// 一块体积。字段相等就是同一块，`cache` 用它判断能不能复用上一次的缓冲。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Volume {
    pub size_x: i32,
    pub size_y: i32,
    pub size_z: i32,
    pub min_x: i32,
    pub min_y: i32,
    pub min_z: i32,
    pub step_x: i32,
    pub step_y: i32,
    pub step_z: i32,
}

impl Volume {
    /// `new DensityVolume(sizeX, sizeY, sizeZ, minX, minY, minZ, stepX, stepY, stepZ)`。
    ///
    /// 官方遇到非正的大小或步长会抛异常；这里的体积都由服务端自己算出来，调用方保证为正。
    #[allow(clippy::too_many_arguments)]
    pub const fn new(size: (i32, i32, i32), min: (i32, i32, i32), step: (i32, i32, i32)) -> Self {
        Self {
            size_x: size.0,
            size_y: size.1,
            size_z: size.2,
            min_x: min.0,
            min_y: min.1,
            min_z: min.2,
            step_x: step.0,
            step_y: step.1,
            step_z: step.2,
        }
    }

    /// 步长都是 1 的体积。
    pub const fn blocks(size: (i32, i32, i32), min: (i32, i32, i32)) -> Self {
        Self::new(size, min, (1, 1, 1))
    }

    /// 只有一个点的体积。
    pub const fn point(x: i32, y: i32, z: i32) -> Self {
        Self::blocks((1, 1, 1), (x, y, z))
    }

    /// `size()`：点数。
    pub fn len(&self) -> usize {
        self.size_x as usize * self.size_y as usize * self.size_z as usize
    }

    /// 体积不会是空的，给 clippy 一个配对的方法。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `indexUnchecked(x, y, z)`，参数是格子下标。
    pub fn index(&self, x: i32, y: i32, z: i32) -> usize {
        (y + (x + z * self.size_x) * self.size_y) as usize
    }

    pub fn block_x(&self, x: i32) -> i32 {
        self.min_x.wrapping_add(x.wrapping_mul(self.step_x))
    }

    pub fn block_y(&self, y: i32) -> i32 {
        self.min_y.wrapping_add(y.wrapping_mul(self.step_y))
    }

    pub fn block_z(&self, z: i32) -> i32 {
        self.min_z.wrapping_add(z.wrapping_mul(self.step_z))
    }

    pub fn max_x(&self) -> i32 {
        self.min_x
            .wrapping_add(self.size_x.wrapping_mul(self.step_x))
            .wrapping_sub(1)
    }

    pub fn max_y(&self) -> i32 {
        self.min_y
            .wrapping_add(self.size_y.wrapping_mul(self.step_y))
            .wrapping_sub(1)
    }

    pub fn max_z(&self) -> i32 {
        self.min_z
            .wrapping_add(self.size_z.wrapping_mul(self.step_z))
            .wrapping_sub(1)
    }

    /// `indexOfBlock(x, y, z)`：方块坐标落在哪个点上，不在格点上给 `None`。
    pub fn index_of_block(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let dx = x.wrapping_sub(self.min_x);
        let dy = y.wrapping_sub(self.min_y);
        let dz = z.wrapping_sub(self.min_z);
        if self.step_x == 1 && self.step_y == 1 && self.step_z == 1 {
            if dx >= 0
                && dy >= 0
                && dz >= 0
                && dx < self.size_x
                && dy < self.size_y
                && dz < self.size_z
            {
                return Some(self.index(dx, dy, dz));
            }
            return None;
        }
        let inside = dx >= 0
            && dy >= 0
            && dz >= 0
            && dx < self.size_x.wrapping_mul(self.step_x)
            && dy < self.size_y.wrapping_mul(self.step_y)
            && dz < self.size_z.wrapping_mul(self.step_z)
            && dx % self.step_x == 0
            && dy % self.step_y == 0
            && dz % self.step_z == 0;
        if !inside {
            return None;
        }
        Some(self.index(
            java_floor_div(dx, self.step_x),
            java_floor_div(dy, self.step_y),
            java_floor_div(dz, self.step_z),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn y_varies_fastest() {
        let volume = Volume::blocks((2, 3, 4), (0, 0, 0));
        assert_eq!(volume.index(0, 1, 0), 1);
        assert_eq!(volume.index(1, 0, 0), 3);
        assert_eq!(volume.index(0, 0, 1), 6);
        assert_eq!(volume.len(), 24);
    }

    #[test]
    fn index_of_block_respects_the_steps() {
        let volume = Volume::new((5, 3, 5), (-16, -64, 32), (4, 8, 4));
        assert_eq!(volume.index_of_block(-16, -64, 32), Some(0));
        assert_eq!(
            volume.index_of_block(-12, -56, 32),
            Some(volume.index(1, 1, 0))
        );
        assert_eq!(volume.index_of_block(-13, -64, 32), None);
        assert_eq!(volume.index_of_block(4, -64, 32), None);
        assert_eq!(volume.max_x(), 3);
        assert_eq!(volume.max_y(), -41);
    }
}
