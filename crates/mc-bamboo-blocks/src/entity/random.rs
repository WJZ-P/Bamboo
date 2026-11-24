//! 实体和爆炸用的随机数，逐位复刻官方的 `java.util.Random`（`LegacyRandomSource`）。
//!
//! 官方每个实体有自己的随机源，世界还有一个（`level.getRandom()`），种子来自
//! `RandomSupport.generateUniqueSeed()`，每次开服都不同。这个世界没有真的随机源，
//! 用固定种子的 [`JavaRandom`] 代替，同一个动作每次都得到相同的结果。

/// `java.util.Random` 的乘法常数，也是 `LegacyRandomSource.MULTIPLIER`。
const MULTIPLIER: u64 = 0x5_DEEC_E66D;
/// `java.util.Random` 的加法常数。
const ADDEND: u64 = 0xB;
/// 内部状态只有 48 位。
const MASK: u64 = (1 << 48) - 1;
/// `BitRandomSource.FLOAT_MULTIPLIER`，2 的 -24 次方。
const FLOAT_MULTIPLIER: f32 = 5.960_464_5e-8;
/// `BitRandomSource.DOUBLE_MULTIPLIER`。官方那个 float 字面量换成 double 正好是 2 的 -53 次方。
const DOUBLE_MULTIPLIER: f64 = 1.110_223e-16_f32 as f64;

/// 和 `java.util.Random` 完全一样的线性同余生成器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaRandom {
    seed: u64,
    /// `MarsagliaPolarGaussian.nextNextGaussian`，没有缓存时是 `None`。
    ///
    /// The cached second Gaussian of `MarsagliaPolarGaussian`, `None` when empty.
    next_gaussian: Option<u64>,
}

impl Default for JavaRandom {
    fn default() -> Self {
        Self::new(0)
    }
}

impl JavaRandom {
    /// `new Random(seed)`。
    pub fn new(seed: i64) -> Self {
        Self {
            seed: (seed as u64 ^ MULTIPLIER) & MASK,
            next_gaussian: None,
        }
    }

    /// 内部的 48 位状态，也就是官方 `LegacyRandomSource.seed` 的值。对拍时比较用。
    ///
    /// The internal 48-bit state, the value of vanilla `LegacyRandomSource.seed`.
    pub const fn state(&self) -> i64 {
        self.seed as i64
    }

    /// 直接设成官方 `LegacyRandomSource.seed` 的值。对拍把官方构造完之后的
    /// 随机源状态搬进 Bamboo 的实体时用。
    ///
    /// Sets the internal state from vanilla's `LegacyRandomSource.seed`. Used
    /// by oracles that carry an official entity's post-construction state over.
    pub const fn from_state(state: i64) -> Self {
        Self {
            seed: (state as u64) & MASK,
            next_gaussian: None,
        }
    }

    /// 有没有缓存着的第二个高斯数，官方的 `haveNextNextGaussian`。
    pub const fn has_cached_gaussian(&self) -> bool {
        self.next_gaussian.is_some()
    }

    /// `nextBoolean()`：`next(1) != 0`。
    pub fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    /// `MarsagliaPolarGaussian.nextGaussian`。官方用 `Math.log` 和 `Math.sqrt`。
    pub fn next_gaussian(&mut self) -> f64 {
        if let Some(bits) = self.next_gaussian.take() {
            return f64::from_bits(bits);
        }
        loop {
            let first = 2.0 * self.next_double() - 1.0;
            let second = 2.0 * self.next_double() - 1.0;
            let square = first * first + second * second;
            if square >= 1.0 || square == 0.0 {
                continue;
            }
            let multiplier = (-2.0 * square.ln() / square).sqrt();
            self.next_gaussian = Some((second * multiplier).to_bits());
            return first * multiplier;
        }
    }

    /// `triangle(mode, deviation)`：第一个数是被减数。
    pub fn triangle(&mut self, mode: f64, deviation: f64) -> f64 {
        let first = self.next_double();
        mode + deviation * (first - self.next_double())
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

    /// `nextFloat()`。官方乘的是 `5.9604645E-8F`，不是先做除法。
    ///
    /// Official multiplies by `5.9604645E-8F` instead of dividing.
    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 * FLOAT_MULTIPLIER
    }

    /// `nextDouble()`。
    pub fn next_double(&mut self) -> f64 {
        let high = i64::from(self.next(26)) << 27;
        let low = i64::from(self.next(27));
        (high + low) as f64 * DOUBLE_MULTIPLIER
    }

    /// `Collections.shuffle(list, random)`：从后往前换，`int` 的 `nextInt(bound)` 取下标。
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for index in (1..items.len()).rev() {
            let swap_with = self.next_int_bounded(index as i32 + 1) as usize;
            items.swap(index, swap_with);
        }
    }
}

/// 把实体的 `JavaRandom`（`&mut self` 接口）适配成 `RandomSource`（`&self` 接口）。
/// 官方有些生成路径传入的是实体自己的 `getRandom()`，例如陷阱骑士的
/// `enchantItemFromProvider`；适配器让这些调用复用同一份 48 位状态。
///
/// Adapts an entity's `JavaRandom` to the `RandomSource` trait so vanilla paths
/// that pass `getRandom()` share the same 48-bit state.
pub struct EntityRandom(pub std::cell::RefCell<JavaRandom>);

impl crate::random::RandomSource for EntityRandom {
    fn next_int_unbounded(&self) -> i32 {
        self.0.borrow_mut().next_int()
    }
    fn next_int(&self, bound: i32) -> i32 {
        self.0.borrow_mut().next_int_bounded(bound)
    }
    fn next_long(&self) -> i64 {
        self.0.borrow_mut().next_long()
    }
    fn next_boolean(&self) -> bool {
        self.0.borrow_mut().next_boolean()
    }
    fn next_float(&self) -> f32 {
        self.0.borrow_mut().next_float()
    }
    fn next_double(&self) -> f64 {
        self.0.borrow_mut().next_double()
    }
    fn next_gaussian(&self) -> f64 {
        self.0.borrow_mut().next_gaussian()
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
        assert_eq!(random.next_int_bounded(1000), 883);
    }

    #[test]
    fn floats_match_java() {
        // 先取 `nextInt()`、`nextInt(100)`、`nextInt(1)`，再取这两个。
        // Draw `nextInt()`, `nextInt(100)` and `nextInt(1)` first, then these two.
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

        // 铁砧摔裂用的就是种子 1 的第一颗 `nextFloat`。
        // Anvil cracking uses the first `nextFloat` from seed 1.
        assert_eq!(JavaRandom::new(1).next_float().to_bits(), 1_060_838_101);
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
