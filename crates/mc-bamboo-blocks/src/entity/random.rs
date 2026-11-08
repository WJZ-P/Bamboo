//! 实体和爆炸用的随机数，逐位复刻官方的 `java.util.Random`（`LegacyRandomSource`）。
//!
//! 官方每个实体有自己的随机源，世界还有一个（`level.getRandom()`），种子来自
//! `RandomSupport.generateUniqueSeed()`，每次开服都不同。这个世界没有真的随机源，
//! 用固定种子的 [`JavaRandom`] 代替，同一个动作每次都得到相同的结果。

/// `java.util.Random` 的乘法常数。
const MULTIPLIER: u64 = 0x5DE_EECE_66D;
/// `java.util.Random` 的加法常数。
const ADDEND: u64 = 0xB;
/// 内部状态只有 48 位。
const MASK: u64 = (1 << 48) - 1;

/// 和 `java.util.Random` 完全一样的线性同余生成器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaRandom {
    seed: u64,
}

impl JavaRandom {
    /// `new Random(seed)`。
    pub fn new(seed: i64) -> Self {
        Self {
            seed: (seed as u64 ^ MULTIPLIER) & MASK,
        }
    }

    /// `next(bits)`。
    fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND) & MASK;
        (self.seed >> (48 - bits)) as i32
    }

    /// `nextInt()`。
    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    /// `nextInt(bound)`。`bound` 不是正数时和官方一样 panic。
    ///
    /// 官方用 `bits - val + (bound - 1) < 0` 判断剩下的区间够不够，这个判断在 `int` 上溢出，
    /// 所以这里也是 `i32` 的加减。
    pub fn next_int_bounded(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        if bound & (bound - 1) == 0 {
            let bits = i64::from(self.next(31));
            return ((i64::from(bound) * bits) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let value = bits % bound;
            if bits.wrapping_sub(value).wrapping_add(bound - 1) >= 0 {
                return value;
            }
        }
    }

    /// `nextLong()`。
    pub fn next_long(&mut self) -> i64 {
        ((i64::from(self.next(32))) << 32).wrapping_add(i64::from(self.next(32)))
    }

    /// `nextFloat()`。官方的除法在 `float` 上做，24 位分子能精确表示。
    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1 << 24) as f32
    }

    /// `nextDouble()`。
    pub fn next_double(&mut self) -> f64 {
        let high = i64::from(self.next(26)) << 27;
        let low = i64::from(self.next(27));
        (high + low) as f64 / (1u64 << 53) as f64
    }

    /// `Collections.shuffle(list, random)`：从后往前换，`int` 的 `nextInt(bound)` 取下标。
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for index in (1..items.len()).rev() {
            let swap_with = self.next_int_bounded(index as i32 + 1) as usize;
            items.swap(index, swap_with);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这些值来自官方 `java.util.Random`，规则见 `java.util.Random` 的文档。
    #[test]
    fn integers_match_java() {
        let mut random = JavaRandom::new(42);
        assert_eq!(random.next_int(), -1170105035);
        assert_eq!(random.next_int_bounded(100), 63);
        assert_eq!(random.next_int_bounded(1), 0);

        let mut random = JavaRandom::new(0);
        assert_eq!(
            [
                random.next_int_bounded(16),
                random.next_int_bounded(16),
                random.next_int_bounded(16)
            ],
            [11, 13, 3]
        );

        let mut random = JavaRandom::new(-1234567890123);
        assert_eq!(random.next_int(), 7952977);
        assert_eq!(random.next_int_bounded(1000), 610);
    }

    #[test]
    fn floats_match_java() {
        // 先取 `nextInt()`、`nextInt(100)`、`nextInt(1)`，再取这两个。
        let mut random = JavaRandom::new(42);
        let _ = random.next_int();
        let _ = random.next_int_bounded(100);
        let _ = random.next_int_bounded(1);
        assert_eq!(random.next_float().to_bits(), 1_027_890_176);
        assert_eq!(random.next_double().to_bits(), 4_599_233_015_213_898_676);

        let mut random = JavaRandom::new(-1234567890123);
        let _ = random.next_int();
        assert_eq!(random.next_float().to_bits(), 974_312_448);
        assert_eq!(random.next_double().to_bits(), 4_604_076_210_556_129_097);
    }

    #[test]
    fn shuffle_matches_java() {
        // 官方 `Collections.shuffle(list, new Random(42))` 对 0..8 的结果。
        let mut random = JavaRandom::new(42);
        let mut items: Vec<i32> = (0..8).collect();
        random.shuffle(&mut items);
        assert_eq!(items, [2, 6, 3, 1, 4, 0, 7, 5]);
    }
}
