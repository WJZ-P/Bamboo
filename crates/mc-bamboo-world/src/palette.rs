//! 调色板容器：区块段存储方块和生物群系的方式。
//!
//! 一个区块段有 4096 个方块，但种类通常很少。所以先建一张“值表”，
//! 每个位置只存表内下标，下标再紧密打包进 64 位整数里。种类太多时才退回到直接存全局 ID。

use mc_bamboo_data::{blocks, registries::biome};

/// 一个区块段的方块数量。
pub const BLOCKS_PER_SECTION: usize = 4096;
/// 一个区块段的生物群系数量，每 4×4×4 个方块共用一个。
pub const BIOMES_PER_SECTION: usize = 64;

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
