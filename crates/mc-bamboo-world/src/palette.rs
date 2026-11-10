//! 调色板容器：区块段存储方块和生物群系的方式。
//!
//! 一个区块段有 4096 个方块，但种类通常很少。所以先建一张“值表”，
//! 每个位置只存表内下标，下标再紧密打包进 64 位整数里。种类太多时才退回到直接存全局 ID。

use mc_bamboo_data::{blocks, registries::biome};

/// 一个区块段的方块数量。
pub const BLOCKS_PER_SECTION: usize = 4096;
/// 一个区块段的生物群系数量，每 4×4×4 个方块共用一个。
pub const BIOMES_PER_SECTION: usize = 64;

/// 磁盘上的调色板和打包数据不能转成内存里的容器。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PackedError {
    #[error("palette must not be empty")]
    EmptyPalette,
    #[error("palette has too many entries for the container")]
    TooManyEntries,
    #[error("packed data has {actual} words, expected {expected}")]
    WrongDataLength { actual: usize, expected: usize },
    #[error("packed index {index} is outside a palette of {size} entries")]
    IndexOutOfRange { index: u32, size: usize },
}

/// 不同内容使用不同的位数规则，这些数字和官方的 `Strategy` 一致。
#[derive(Debug)]
struct Rules {
    entries: usize,
    min_indirect_bits: u8,
    max_indirect_bits: u8,
    global_bits: u8,
}

const fn bits_for_count(count: u32) -> u8 {
    if count <= 1 {
        0
    } else {
        (u32::BITS - (count - 1).leading_zeros()) as u8
    }
}

/// 磁盘上的位数：官方 `Strategy.getConfigurationForPaletteSize(n).bitsInStorage()`。
///
/// 和网络形式不同，超过局部调色板上限时官方用 `Configuration.Global`，但它的
/// `bitsInStorage` 仍是 `ceil(log2(n))`，不是内存里全局 ID 的位数。
pub fn disk_bits(palette_len: usize, biomes: bool) -> u8 {
    if palette_len <= 1 {
        return 0;
    }
    let bits = bits_for_count(palette_len as u32);
    if biomes {
        bits
    } else {
        bits.max(BLOCK_RULES.min_indirect_bits)
    }
}

/// 磁盘上的 long 数组长度。位数为 0（单值调色板）时是 0。
pub fn disk_data_len(entries: usize, bits: u8) -> usize {
    if bits == 0 {
        0
    } else {
        entries.div_ceil(64 / bits as usize)
    }
}

/// 从磁盘位数里读出一个下标。调用方保证 `bits > 0`、`data` 长度和 `index` 范围都合法。
pub fn read_disk_index(data: &[u64], bits: u8, index: usize) -> u32 {
    let per_word = 64 / bits as usize;
    let shift = (index % per_word) * bits as usize;
    let mask = (1u64 << bits) - 1;
    ((data[index / per_word] >> shift) & mask) as u32
}

/// 把下标按磁盘位数写进 long 数组。调用方保证长度和范围合法。
pub fn write_disk_index(data: &mut [u64], bits: u8, index: usize, value: u32) {
    write_packed(data, bits, index, value);
}

/// 磁盘上的 long 数组里最多能存多少个下标，供解码器检查越界。
pub fn packed_capacity(data_len: usize, bits: u8) -> usize {
    if bits == 0 {
        1
    } else {
        data_len.saturating_mul(64 / bits as usize)
    }
}

static BLOCK_RULES: Rules = Rules {
    entries: BLOCKS_PER_SECTION,
    min_indirect_bits: 4,
    max_indirect_bits: 8,
    global_bits: bits_for_count(blocks::TOTAL_STATES),
};

static BIOME_RULES: Rules = Rules {
    entries: BIOMES_PER_SECTION,
    min_indirect_bits: 1,
    max_indirect_bits: 3,
    global_bits: bits_for_count(biome::TOTAL),
};

impl Rules {
    /// 存放 `palette_len` 种值需要的位数。
    fn bits_for_palette(&self, palette_len: usize) -> u8 {
        match bits_for_count(palette_len as u32) {
            0 => 0,
            bits if bits <= self.max_indirect_bits => bits.max(self.min_indirect_bits),
            _ => self.global_bits,
        }
    }

    fn is_global(&self, bits: u8) -> bool {
        bits > self.max_indirect_bits
    }
}

/// 一个 64 位整数能放下多少个 `bits` 位的值。值不会跨越整数边界。
const fn values_per_word(bits: u8) -> usize {
    64 / bits as usize
}

const fn packed_len(entries: usize, bits: u8) -> usize {
    if bits == 0 {
        0
    } else {
        entries.div_ceil(values_per_word(bits))
    }
}

/// 把 `values` 里的每个值按 `bits` 位打包。
pub fn pack_values(values: impl ExactSizeIterator<Item = u32>, bits: u8) -> Vec<u64> {
    let mut data = vec![0; packed_len(values.len(), bits)];
    for (index, value) in values.enumerate() {
        write_packed(&mut data, bits, index, value);
    }
    data
}

fn read_packed(data: &[u64], bits: u8, index: usize) -> u32 {
    let per_word = values_per_word(bits);
    let shift = (index % per_word) * bits as usize;
    let mask = (1u64 << bits) - 1;
    ((data[index / per_word] >> shift) & mask) as u32
}

fn write_packed(data: &mut [u64], bits: u8, index: usize, value: u32) {
    let per_word = values_per_word(bits);
    let shift = (index % per_word) * bits as usize;
    let mask = (1u64 << bits) - 1;
    let word = &mut data[index / per_word];
    *word = (*word & !(mask << shift)) | (u64::from(value) << shift);
}

/// 容器当前使用的调色板。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteView<'a> {
    /// 只有一种值。
    Single(u32),
    /// 值表，打包数据里存的是表内下标。
    Indirect(&'a [u32]),
    /// 打包数据里直接存全局 ID。
    Global,
}

/// 调色板容器。
#[derive(Debug, Clone)]
pub struct PalettedContainer {
    rules: &'static Rules,
    bits: u8,
    /// 位数为 0 时只有一个元素，切换到全局 ID 后为空。
    palette: Vec<u32>,
    data: Vec<u64>,
}

impl PalettedContainer {
    /// 创建一个填满 `value` 的方块容器。
    pub fn blocks(value: u32) -> Self {
        Self::filled(&BLOCK_RULES, value)
    }

    /// 创建一个填满 `value` 的生物群系容器。
    pub fn biomes(value: u32) -> Self {
        Self::filled(&BIOME_RULES, value)
    }

    fn filled(rules: &'static Rules, value: u32) -> Self {
        Self {
            rules,
            bits: 0,
            palette: vec![value],
            data: Vec::new(),
        }
    }

    /// 从磁盘上的「调色板 + 下标数组」构造容器，输入一律当成不可信数据检查。
    ///
    /// `entries` 是方块 4096 / 生物群系 64，`bits` 由 [`disk_bits`] 按调色板长度算出。
    /// 磁盘位数不超过内存局部上限时，long 数组原样用；超过时容器切换成全局 ID 形式，
    /// 内存表示重新打包，读出来的值和磁盘一致。
    fn from_disk(
        rules: &'static Rules,
        palette: Vec<u32>,
        data: Vec<u64>,
    ) -> Result<Self, PackedError> {
        if palette.is_empty() {
            return Err(PackedError::EmptyPalette);
        }
        if palette.len() > rules.entries {
            return Err(PackedError::TooManyEntries);
        }
        let disk_bits = disk_bits(palette.len(), rules.entries == BIOMES_PER_SECTION);
        let expected = packed_len(rules.entries, disk_bits);
        if data.len() != expected {
            return Err(PackedError::WrongDataLength {
                actual: data.len(),
                expected,
            });
        }
        if disk_bits == 0 {
            return Ok(Self {
                rules,
                bits: 0,
                palette,
                data,
            });
        }
        // 高位多余的值会让下标数组读出来落到调色板之外，先查一遍再决定怎么存。
        let entries = rules.entries;
        for index in 0..entries {
            let stored = read_disk_index(&data, disk_bits, index);
            // 官方磁盘形式里存的永远是调色板下标，即使配置是 `Configuration.Global`
            // （它的 `bitsInStorage` 是请求的位数，打包内容仍是 HashMapPalette 的下标）。
            if stored as usize >= palette.len() {
                return Err(PackedError::IndexOutOfRange {
                    index: stored,
                    size: palette.len(),
                });
            }
        }

        let memory_bits = rules.bits_for_palette(palette.len());
        if memory_bits == disk_bits {
            return Ok(Self {
                rules,
                bits: disk_bits,
                palette,
                data,
            });
        }
        // 只可能发生在磁盘位数超过内存里的局部上限时：内存改用全局 ID。
        debug_assert!(rules.is_global(memory_bits));
        let mut repacked = vec![0; packed_len(entries, memory_bits)];
        for index in 0..entries {
            let stored = read_disk_index(&data, disk_bits, index);
            let value = palette[stored as usize];
            write_packed(&mut repacked, memory_bits, index, value);
        }
        Ok(Self {
            rules,
            bits: memory_bits,
            palette: Vec::new(),
            data: repacked,
        })
    }

    /// 方块容器的磁盘形式。
    pub(crate) fn from_blocks_disk(palette: Vec<u32>, data: Vec<u64>) -> Result<Self, PackedError> {
        Self::from_disk(&BLOCK_RULES, palette, data)
    }

    /// 生物群系容器的磁盘形式。
    pub(crate) fn from_biomes_disk(palette: Vec<u32>, data: Vec<u64>) -> Result<Self, PackedError> {
        Self::from_disk(&BIOME_RULES, palette, data)
    }

    /// 磁盘形式的调色板：丢掉没用到的项，按第一次出现的顺序重排，并给出重新打包的 long 数组。
    /// 对应官方 `PalettedContainer.pack`。
    pub fn packed_for_disk(&self) -> (Vec<u32>, Vec<u64>) {
        let mut palette = Vec::new();
        let mut stored = Vec::with_capacity(self.rules.entries);
        for index in 0..self.rules.entries {
            let value = self.get(index);
            let position = palette
                .iter()
                .position(|&entry| entry == value)
                .unwrap_or_else(|| {
                    palette.push(value);
                    palette.len() - 1
                });
            stored.push(position as u32);
        }
        let bits = disk_bits(palette.len(), self.rules.entries == BIOMES_PER_SECTION);
        let data = if bits == 0 {
            Vec::new()
        } else {
            pack_values(stored.into_iter(), bits)
        };
        (palette, data)
    }

    /// 每个调色板下标在整段里出现的次数，供 `recalcBlockCounts` 用。
    pub(crate) fn index_counts(&self) -> Vec<u16> {
        let size = match self.palette() {
            PaletteView::Single(_) => 1,
            PaletteView::Indirect(states) => states.len(),
            PaletteView::Global => 0,
        };
        let mut counts = vec![0u16; size];
        if self.bits == 0 || size == 0 {
            return counts;
        }
        let per_word = values_per_word(self.bits);
        let mask = (1u64 << self.bits) - 1;
        for index in 0..self.rules.entries {
            let stored = ((self.data[index / per_word] >> (index % per_word * self.bits as usize))
                & mask) as usize;
            if let Some(count) = counts.get_mut(stored) {
                *count += 1;
            }
        }
        counts
    }

    pub fn bits(&self) -> u8 {
        self.bits
    }

    pub fn data(&self) -> &[u64] {
        &self.data
    }

    pub fn palette(&self) -> PaletteView<'_> {
        if self.bits == 0 {
            PaletteView::Single(self.palette[0])
        } else if self.rules.is_global(self.bits) {
            PaletteView::Global
        } else {
            PaletteView::Indirect(&self.palette)
        }
    }

    pub fn get(&self, index: usize) -> u32 {
        if self.bits == 0 {
            return self.palette[0];
        }
        let stored = read_packed(&self.data, self.bits, index);
        if self.rules.is_global(self.bits) {
            stored
        } else {
            self.palette[stored as usize]
        }
    }

    pub fn set(&mut self, index: usize, value: u32) {
        if self.bits == 0 && self.palette[0] == value {
            return;
        }
        let stored = self.stored_form(value);
        write_packed(&mut self.data, self.bits, index, stored);
    }

    /// 返回 `value` 在打包数据里的表示，必要时扩展调色板。
    fn stored_form(&mut self, value: u32) -> u32 {
        if self.rules.is_global(self.bits) {
            return value;
        }
        if self.bits != 0
            && let Some(position) = self.palette.iter().position(|&entry| entry == value)
        {
            return position as u32;
        }

        let required_bits = self.rules.bits_for_palette(self.palette.len() + 1);
        if required_bits != self.bits {
            self.repack(required_bits);
        }
        if self.rules.is_global(self.bits) {
            return value;
        }
        self.palette.push(value);
        (self.palette.len() - 1) as u32
    }

    /// 换成新的位数，并重写所有位置。
    fn repack(&mut self, new_bits: u8) {
        let values: Vec<u32> = (0..self.rules.entries)
            .map(|index| self.get(index))
            .collect();
        let becoming_global = self.rules.is_global(new_bits);

        let mut data = vec![0; packed_len(self.rules.entries, new_bits)];
        for (index, &value) in values.iter().enumerate() {
            let stored = if becoming_global {
                value
            } else {
                self.palette
                    .iter()
                    .position(|&entry| entry == value)
                    .expect("调色板里应该已有这个值") as u32
            };
            write_packed(&mut data, new_bits, index, stored);
        }

        self.bits = new_bits;
        self.data = data;
        if becoming_global {
            self.palette.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_bits_like_the_game() {
        assert_eq!(bits_for_count(1), 0);
        assert_eq!(bits_for_count(2), 1);
        assert_eq!(bits_for_count(16), 4);
        assert_eq!(bits_for_count(17), 5);
        assert_eq!(BLOCK_RULES.global_bits, 16);
    }

    #[test]
    fn starts_as_a_single_value() {
        let container = PalettedContainer::blocks(7);
        assert_eq!(container.bits(), 0);
        assert_eq!(container.palette(), PaletteView::Single(7));
        assert!(container.data().is_empty());
        assert_eq!(container.get(4095), 7);
    }

    #[test]
    fn setting_the_same_value_stays_single() {
        let mut container = PalettedContainer::blocks(7);
        container.set(10, 7);
        assert_eq!(container.bits(), 0);
    }

    #[test]
    fn block_palette_jumps_straight_to_four_bits() {
        let mut container = PalettedContainer::blocks(0);
        container.set(1, 9);

        assert_eq!(container.bits(), 4);
        assert_eq!(container.palette(), PaletteView::Indirect(&[0, 9]));
        assert_eq!(container.data().len(), 256);
        assert_eq!(container.get(0), 0);
        assert_eq!(container.get(1), 9);
    }

    #[test]
    fn biome_palette_starts_at_one_bit() {
        let mut container = PalettedContainer::biomes(0);
        container.set(1, 5);

        assert_eq!(container.bits(), 1);
        assert_eq!(container.data().len(), 1);
    }

    #[test]
    fn grows_and_keeps_every_value() {
        let mut container = PalettedContainer::blocks(0);
        for value in 1..=40u32 {
            container.set(value as usize * 3, value);
        }

        assert_eq!(container.bits(), 6);
        for value in 1..=40u32 {
            assert_eq!(container.get(value as usize * 3), value);
        }
        assert_eq!(container.get(1), 0);
    }

    #[test]
    fn switches_to_global_ids_past_the_palette_limit() {
        let mut container = PalettedContainer::blocks(0);
        for value in 1..=300u32 {
            container.set(value as usize, value + 1000);
        }

        assert_eq!(container.bits(), BLOCK_RULES.global_bits);
        assert_eq!(container.palette(), PaletteView::Global);
        assert_eq!(container.get(0), 0);
        for value in 1..=300u32 {
            assert_eq!(container.get(value as usize), value + 1000);
        }
    }

    #[test]
    fn packs_values_from_the_low_bits_without_crossing_words() {
        assert_eq!(pack_values([1u32, 2, 3].into_iter(), 4), [0x321]);

        // 9 位时一个整数只放 7 个值，第 8 个值从下一个整数开始。
        let packed = pack_values(std::iter::repeat_n(1, 8), 9);
        assert_eq!(packed.len(), 2);
        assert_eq!(packed[1], 1);
    }
}
