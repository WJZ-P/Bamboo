//! 一个区块段这一 tick 改过的格子，官方 `ChunkHolder.changedBlocksPerSection` 的一项。
//!
//! 官方把段内坐标记在 fastutil 的 `ShortOpenHashSet` 里，`section_blocks_update` 里格子的先后
//! 就是这个集合的遍历顺序。这里照它的办法存一遍，顺序和发出去的字节就都和官方一样：开放寻址、
//! 线性探测，坐标 0 不进表单独记；遍历时 0 最先，其余从表尾往表头。

use std::mem;

/// `new ShortOpenHashSet()` 的表长：预计 16 个元素，装载因子 0.75。
const INITIAL_TABLE_LEN: usize = 32;

/// 段内坐标的集合，坐标见 [`super::local_of`]。
#[derive(Debug, Clone)]
pub(super) struct ChangedBlocks {
    /// 长度是 2 的幂，0 是空位。
    table: Vec<u16>,
    /// 坐标 0 是段里 x、y、z 都最小的那一格，和空位撞了，所以单独记。
    has_zero: bool,
    len: usize,
}

impl ChangedBlocks {
    pub(super) fn new() -> Self {
        Self {
            table: vec![0; INITIAL_TABLE_LEN],
            has_zero: false,
            len: 0,
        }
    }

    /// 记下一格，记过的不变。官方的 `ShortOpenHashSet.add`。
    pub(super) fn insert(&mut self, local: u16) {
        let added = if local == 0 {
            !mem::replace(&mut self.has_zero, true)
        } else {
            place(&mut self.table, local)
        };
        if !added {
            return;
        }
        // 官方写的是 `size++ >= maxFill`，比的是加一之前的个数。
        let full = self.len >= max_fill(self.table.len());
        self.len += 1;
        if full {
            self.grow(table_len_for(self.len + 1));
        }
    }

    /// 官方的遍历顺序。
    pub(super) fn iter(&self) -> impl Iterator<Item = u16> + '_ {
        let rest = self.table.iter().rev().copied().filter(|&local| local != 0);
        self.has_zero.then_some(0).into_iter().chain(rest)
    }

    /// 官方的 `rehash`：从旧表的表尾往表头，逐个放进新表。
    fn grow(&mut self, table_len: usize) {
        let old = mem::replace(&mut self.table, vec![0; table_len]);
        for local in old.into_iter().rev().filter(|&local| local != 0) {
            place(&mut self.table, local);
        }
    }
}

/// 从 `mix` 算出的位置往后找空位放下。已经在表里时返回 `false`。
fn place(table: &mut [u16], local: u16) -> bool {
    let mask = table.len() - 1;
    let mut slot = mix(local) & mask;
    while table[slot] != 0 {
        if table[slot] == local {
            return false;
        }
        slot = (slot + 1) & mask;
    }
    table[slot] = local;
    true
}

/// fastutil 的 `HashCommon.mix`：乘以 2^32 除以黄金比，再把高 16 位混进低 16 位。
fn mix(local: u16) -> usize {
    let hash = u32::from(local).wrapping_mul(0x9e37_79b9);
    (hash ^ (hash >> 16)) as usize
}

/// `HashCommon.maxFill`：表里已经有这么多个时，再放一个就要扩表。
fn max_fill(table_len: usize) -> usize {
    (table_len * 3).div_ceil(4).min(table_len - 1)
}

/// `HashCommon.arraySize`：装载因子 0.75 时放得下 `expected` 个的最小表长。
fn table_len_for(expected: usize) -> usize {
    (expected * 4).div_ceil(3).next_power_of_two().max(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 下面的答案都来自官方带的 fastutil 8.5.18：`new ShortOpenHashSet()`，按同样的顺序 `add`，
    // 再用 `iterator()` 读出来。

    fn order(keys: impl IntoIterator<Item = u16>) -> Vec<u16> {
        let mut blocks = ChangedBlocks::new();
        for key in keys {
            blocks.insert(key);
        }
        blocks.iter().collect()
    }

    #[test]
    fn zero_comes_first_and_repeats_are_ignored() {
        assert_eq!(
            order([5, 0, 300, 4095, 17, 37, 5]),
            [0, 37, 300, 5, 17, 4095]
        );
    }

    #[test]
    fn a_collision_moves_the_later_key_one_slot_up() {
        // 两个键的 `mix` 落在同一格，后来的放在下一格，倒着读时先出来。
        assert_eq!(mix(1) & 31, mix(30) & 31);
        assert_eq!(order([1, 30]), [30, 1]);
        // 表尾撞了绕回表头，倒着读时最后出来。
        assert_eq!(mix(33) & 31, 31);
        assert_eq!(mix(66) & 31, 31);
        assert_eq!(order([33, 66]), [33, 66]);
    }

    #[test]
    fn growing_the_table_keeps_the_official_order() {
        // 第 25 个元素进来时表从 32 格扩到 64 格。
        let keys = (0..40).map(|index| (index * 1103 % 4096) as u16);
        assert_eq!(
            order(keys),
            [
                0, 1109, 948, 3154, 161, 3941, 322, 1735, 1896, 2528, 2844, 1580, 3470, 3625, 2367,
                477, 316, 2206, 1741, 3631, 2051, 2999, 6, 2838, 1264, 3309, 3786, 2212, 1103, 954,
                638, 2522, 1419, 632, 2683, 793, 3315, 1425, 3947, 2057,
            ]
        );
    }

    #[test]
    fn a_whole_section_matches_the_official_order() {
        // 按 x、y、z 三层循环改满一段，表要扩 8 次，长到 8192 格。
        let keys = (0..16u16).flat_map(|x| {
            (0..16u16).flat_map(move |y| (0..16u16).map(move |z| (x << 8) | (z << 4) | y))
        });
        let order = order(keys);
        assert_eq!(order.len(), 4096);
        assert_eq!(
            order[..16],
            [
                0, 1715, 3430, 528, 2902, 3958, 1056, 659, 1187, 1584, 3299, 2112, 1318, 4089,
                2243, 2374,
            ]
        );
        // 整个顺序的 FNV-1a 摘要，每个元素按大端的两个字节算，和官方那边用同样的办法算出来的比。
        let digest = order
            .iter()
            .flat_map(|key| key.to_be_bytes())
            .fold(0xcbf2_9ce4_8422_2325u64, |digest, byte| {
                (digest ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            });
        assert_eq!(digest, 0x1a76_052f_543a_32b9);
    }

    #[test]
    fn table_sizes_follow_fastutil() {
        assert_eq!(table_len_for(16), INITIAL_TABLE_LEN);
        assert_eq!(max_fill(32), 24);
        assert_eq!(table_len_for(26), 64);
        assert_eq!(table_len_for(3074), 8192);
        assert_eq!(max_fill(2), 1);
    }
}
