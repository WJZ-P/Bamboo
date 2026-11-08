//! 一个区块段的一层光照，官方的 `DataLayer`。
//!
//! 官方在没写过的时候不分配数组，只记一个默认值。“空”说的就是这种没分配、默认值又是 0 的状态，
//! 不是“每格都是 0”：区块包按它决定这一段发数据还是只在掩码里标一位，所以这里要一模一样地记下来。

use std::borrow::Cow;

/// 一层光照的字节数：4096 格，每格 4 位。
pub const LAYER_BYTES: usize = 2048;
/// 一个 16×16 平面的字节数，`repeatFirstLayer` 复制的就是最底下这一片。
const SLICE_BYTES: usize = 128;

/// 一个区块段的一层光照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataLayer {
    /// 还没分配数组，每一格都是这个值。
    Filled(u8),
    /// 每格 4 位，下标顺序是 y、z、x，同一字节里低 4 位在前。
    Data(Box<[u8; LAYER_BYTES]>),
}

impl DataLayer {
    /// 官方的 `new DataLayer()`。
    pub const EMPTY: Self = Self::Filled(0);

    fn index(x: usize, y: usize, z: usize) -> usize {
        (y << 8) | (z << 4) | x
    }

    /// 读一格，参数是区块段内的坐标。
    pub fn get(&self, x: usize, y: usize, z: usize) -> u8 {
        match self {
            Self::Filled(value) => *value,
            Self::Data(data) => {
                let index = Self::index(x, y, z);
                data[index / 2] >> (index % 2 * 4) & 15
            }
        }
    }

    /// 写一格。第一次写时才分配数组，哪怕写进去的值和默认值一样。
    pub fn set(&mut self, x: usize, y: usize, z: usize, value: u8) {
        if let Self::Filled(filled) = *self {
            *self = Self::Data(Box::new([pack(filled); LAYER_BYTES]));
        }
        let Self::Data(data) = self else {
            unreachable!("上面刚分配了数组");
        };
        let index = Self::index(x, y, z);
        let shift = index % 2 * 4;
        let byte = &mut data[index / 2];
        *byte = *byte & !(15 << shift) | (value & 15) << shift;
    }

    /// `fill`：丢掉数组，每一格都变成 `value`。
    pub fn fill(&mut self, value: u8) {
        *self = Self::Filled(value);
    }

    /// `isEmpty`：没分配数组，默认值是 0。
    pub fn is_empty(&self) -> bool {
        *self == Self::EMPTY
    }

    /// `getData`：发给客户端的 2048 字节。
    pub fn bytes(&self) -> Cow<'_, [u8]> {
        match self {
            Self::Filled(value) => Cow::Owned(vec![pack(*value); LAYER_BYTES]),
            Self::Data(data) => Cow::Borrowed(data.as_slice()),
        }
    }

    /// `SkyLightSectionStorage.repeatFirstLayer`：把最底下一片复制 16 遍。没分配数组时原样复制。
    pub(crate) fn repeat_first_layer(&self) -> Self {
        let Self::Data(data) = self else {
            return self.clone();
        };
        let mut repeated = Box::new([0; LAYER_BYTES]);
        for start in (0..LAYER_BYTES).step_by(SLICE_BYTES) {
            repeated[start..start + SLICE_BYTES].copy_from_slice(&data[..SLICE_BYTES]);
        }
        Self::Data(repeated)
    }
}

/// 一个字节里两格都是 `value`。
const fn pack(value: u8) -> u8 {
    value & 15 | (value & 15) << 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_default_until_written() {
        let layer = DataLayer::Filled(15);
        assert_eq!(layer.get(3, 7, 9), 15);
        assert!(!layer.is_empty());
        assert!(DataLayer::EMPTY.is_empty());
    }

    #[test]
    fn writing_allocates_even_the_same_value() {
        let mut layer = DataLayer::EMPTY;
        layer.set(0, 0, 0, 0);

        assert!(!layer.is_empty());
        assert!(matches!(layer, DataLayer::Data(_)));
        assert!(layer.bytes().iter().all(|&byte| byte == 0));
    }

    #[test]
    fn nibbles_follow_the_official_order() {
        let mut layer = DataLayer::Filled(15);
        layer.set(1, 0, 0, 3);
        layer.set(0, 1, 0, 4);

        let bytes = layer.bytes();
        // x = 1 是第一个字节的高 4 位，y = 1 从第 128 个字节开始。
        assert_eq!(bytes[0], 0x3f);
        assert_eq!(bytes[128], 0xf4);
        assert_eq!(layer.get(1, 0, 0), 3);
        assert_eq!(layer.get(0, 1, 0), 4);
        assert_eq!(layer.get(2, 0, 0), 15);
    }

    #[test]
    fn fill_drops_the_array() {
        let mut layer = DataLayer::EMPTY;
        layer.set(5, 5, 5, 9);
        layer.fill(0);
        assert!(layer.is_empty());
    }

    #[test]
    fn repeat_first_layer_copies_the_bottom_slice() {
        let mut layer = DataLayer::EMPTY;
        layer.set(4, 0, 6, 12);
        layer.set(4, 9, 6, 1);

        let repeated = layer.repeat_first_layer();
        for y in 0..16 {
            assert_eq!(repeated.get(4, y, 6), 12);
        }
        assert_eq!(
            DataLayer::Filled(15).repeat_first_layer(),
            DataLayer::Filled(15)
        );
    }
}
