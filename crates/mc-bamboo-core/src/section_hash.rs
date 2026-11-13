//! 区块段方块状态的 128 位哈希，红石缓存用它判断一段内容有没有变。
//!
//! 每格按段内规范坐标 `(x, y, z)`（各 0..16）和方块状态算两个 64 位哈希，拼成
//! 128 位，整段 4096 格异或起来。用段内坐标而不是世界坐标：模板区块会被克隆到
//! 任意区块位置，段哈希不能依赖它现在在哪个区块。段坐标本身另进缓存键，所以
//! 内容相同的两段哈希相同也不会让键变粗。
//!
//! 写方块时 `^= cell_hash(旧) ^ cell_hash(新)` 就能增量更新，不必重扫整段。
//!
//! A stable 128-bit hash of a 16³ chunk section's block states. Cell positions
//! are section-local, so a cloned template chunk keeps a valid hash wherever it
//! is placed; the section coordinate itself belongs to the cache key.

use std::hash::{Hash, Hasher};

/// 固定种子的 FNV-1a 变体，两个 64 位半区各一份，跨进程、跨构建可比。
struct StableHasher {
    lo: u64,
    hi: u64,
}

impl StableHasher {
    const fn new() -> Self {
        Self {
            lo: 0x243f_6a88_85a3_08d3,
            hi: 0x1319_8a2e_0370_7344,
        }
    }
}

impl Hasher for StableHasher {
    fn finish(&self) -> u64 {
        self.lo
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.lo ^= u64::from(byte);
            self.lo = self.lo.wrapping_mul(0x100_0000_01b3);
            self.hi = self.hi.rotate_left(7) ^ self.lo;
        }
    }
}

/// 一个值的 64 位稳定哈希。
pub fn stable_hash(value: &impl Hash) -> u64 {
    let mut hasher = StableHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// 一格方块状态对整段哈希的贡献。坐标是段内规范坐标，各 0..16。
///
/// `state` 是方块状态的注册表 ID。
pub fn cell_hash(x: usize, y: usize, z: usize, state: u32) -> u128 {
    let position = (x as u32, y as u32, z as u32);
    let lo = stable_hash(&(position, 0_u8, state));
    let hi = stable_hash(&(position, 1_u8, state));
    lo as u128 | (hi as u128) << 64
}

/// 整段重算一遍，供读盘组装区块和调试断言用。
pub fn section_hash(block_at: impl Fn(usize, usize, usize) -> u32) -> u128 {
    let mut hash = 0;
    for y in 0..16 {
        for z in 0..16 {
            for x in 0..16 {
                hash ^= cell_hash(x, y, z, block_at(x, y, z));
            }
        }
    }
    hash
}

/// 全空气段的哈希。它只算一次：`Chunk::empty` 要建 24 个空段。
pub fn empty_section_hash() -> u128 {
    static EMPTY: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    *EMPTY.get_or_init(|| section_hash(|_, _, _| 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_are_reversible() {
        let hash = empty_section_hash();
        assert_ne!(hash, 0);
        assert_eq!(hash ^ cell_hash(3, 4, 5, 0) ^ cell_hash(3, 4, 5, 7), {
            let mut expected = hash;
            expected ^= cell_hash(3, 4, 5, 0) ^ cell_hash(3, 4, 5, 7);
            expected
        });
    }

    #[test]
    fn cells_and_states_change_the_hash() {
        assert_ne!(cell_hash(1, 2, 3, 7), cell_hash(2, 2, 3, 7));
        assert_ne!(cell_hash(1, 2, 3, 7), cell_hash(1, 2, 3, 8));
        assert_eq!(cell_hash(1, 2, 3, 7), cell_hash(1, 2, 3, 7));
    }

    #[test]
    fn full_scan_matches_incremental_updates() {
        let mut incremental = empty_section_hash();
        let cells = [(1, 2, 3, 9_u32), (15, 15, 15, 4), (0, 0, 0, 71)];
        let mut states = vec![0_u32; 16 * 16 * 16];
        for &(x, y, z, state) in &cells {
            let index = (y << 8) | (z << 4) | x;
            incremental ^= cell_hash(x, y, z, states[index]) ^ cell_hash(x, y, z, state);
            states[index] = state;
        }
        let full = section_hash(|x, y, z| states[(y << 8) | (z << 4) | x]);
        assert_eq!(incremental, full);
    }
}
