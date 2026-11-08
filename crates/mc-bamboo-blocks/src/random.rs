//! 官方 `ServerLevel` 的随机源：`LegacyRandomSource` 和 `Level.randValue`，逐位移植。
//!
//! 种子相同时取出来的数和官方完全一样，oracle 靠这个和官方代码比较结果带随机的题。
//! 官方方块代码取数的次数和顺序也要照抄，少取一次，后面的结果就全错开了。

use std::cell::Cell;

use mc_bamboo_core::{BlockPos, Direction};

/// `LegacyRandomSource.MULTIPLIER`，和 `java.util.Random` 相同。
const MULTIPLIER: i64 = 0x5_DEEC_E66D;
const INCREMENT: i64 = 11;
/// 种子只有低 48 位。
const MODULUS_MASK: i64 = (1 << 48) - 1;
/// `BitRandomSource.FLOAT_MULTIPLIER`，也就是 2 的 -24 次方。
const FLOAT_MULTIPLIER: f32 = 5.960_464_5e-8;
/// `BitRandomSource.DOUBLE_MULTIPLIER`。官方写的是 float 字面量 `1.110223E-16F`，换成 double 正好是 2 的 -53 次方。
const DOUBLE_MULTIPLIER: f64 = 1.110_223e-16_f32 as f64;
/// `Level.getBlockRandomPos` 的线性同余的增量。
const RAND_VALUE_INCREMENT: i32 = 1_013_904_223;

/// `LegacyRandomSource`：48 位的线性同余，和 `java.util.Random` 是同一个算法。
///
/// 官方的方块代码在只读的地方也取随机数，比如放下海带时的 `age`，所以状态放在 `Cell` 里，取数只要共享引用。
#[derive(Debug, Clone)]
pub struct LegacyRandom {
    seed: Cell<i64>,
    /// `MarsagliaPolarGaussian` 一次算出两个数，第二个留到下一次。
    next_gaussian: Cell<Option<f64>>,
}

impl LegacyRandom {
    pub fn new(seed: i64) -> Self {
        let random = Self {
            seed: Cell::new(0),
            next_gaussian: Cell::new(None),
        };
        random.set_seed(seed);
        random
    }

    /// `setSeed`。
    pub fn set_seed(&self, seed: i64) {
        self.seed.set((seed ^ MULTIPLIER) & MODULUS_MASK);
        self.next_gaussian.set(None);
    }

    /// `next(bits)`：线性同余走一步，取高 `bits` 位。
    ///
    /// 官方 `WorldgenRandom` 包着这个源时直接调用它，所以这里是公开的。
    pub fn next(&self, bits: u32) -> i32 {
        let seed = self
            .seed
            .get()
            .wrapping_mul(MULTIPLIER)
            .wrapping_add(INCREMENT)
            & MODULUS_MASK;
        self.seed.set(seed);
        (seed >> (48 - bits)) as i32
    }

    /// `nextInt()`。
    pub fn next_int_unbounded(&self) -> i32 {
        self.next(32)
    }

    /// `nextInt(bound)`：`[0, bound)` 里的整数。`bound` 不是正数时和官方一样 panic。
    pub fn next_int(&self, bound: i32) -> i32 {
        assert!(bound > 0, "Bound must be positive");
        if bound & (bound - 1) == 0 {
            return ((i64::from(bound) * i64::from(self.next(31))) >> 31) as i32;
        }
        loop {
            let sample = self.next(31);
            let modulo = sample % bound;
            // 官方靠 int 溢出变成负数来丢掉分布不均匀的尾巴。
            if sample.wrapping_sub(modulo).wrapping_add(bound - 1) >= 0 {
                return modulo;
            }
        }
    }

    /// `nextInt(origin, bound)`：`[origin, bound)` 里的整数。
    pub fn next_int_between(&self, origin: i32, bound: i32) -> i32 {
        assert!(origin < bound, "bound - origin is non positive");
        origin + self.next_int(bound - origin)
    }

    /// `nextIntBetweenInclusive(min, max)`，也是 `Mth.randomBetweenInclusive`。
    pub fn next_int_between_inclusive(&self, min: i32, max: i32) -> i32 {
        self.next_int(max - min + 1) + min
    }

    /// `Mth.nextInt(random, min, max)`：`min` 不小于 `max` 时直接返回 `min`，不取数。
    pub fn mth_next_int(&self, min: i32, max: i32) -> i32 {
        if min >= max {
            min
        } else {
            self.next_int(max - min + 1) + min
        }
    }

    /// `nextLong()`。
    pub fn next_long(&self) -> i64 {
        let upper = i64::from(self.next(32));
        let lower = i64::from(self.next(32));
        (upper << 32).wrapping_add(lower)
    }

    /// `nextBoolean()`。
    pub fn next_boolean(&self) -> bool {
        self.next(1) != 0
    }

    /// `nextFloat()`：`[0, 1)`。
    pub fn next_float(&self) -> f32 {
        self.next(24) as f32 * FLOAT_MULTIPLIER
    }

    /// `nextDouble()`：`[0, 1)`。
    pub fn next_double(&self) -> f64 {
        let upper = i64::from(self.next(26));
        let lower = i64::from(self.next(27));
        ((upper << 27) + lower) as f64 * DOUBLE_MULTIPLIER
    }

    /// `Mth.nextFloat(random, min, max)`：`min` 不小于 `max` 时直接返回 `min`，不取数。
    pub fn mth_next_float(&self, min: f32, max: f32) -> f32 {
        if min >= max {
            min
        } else {
            self.next_float() * (max - min) + min
        }
    }

    /// `triangle(mean, spread)` 的 double 版本。
    pub fn triangle(&self, mean: f64, spread: f64) -> f64 {
        let first = self.next_double();
        mean + spread * (first - self.next_double())
    }

    /// `nextGaussian()`，官方的 `MarsagliaPolarGaussian`。
    ///
    /// 官方用 `Math.log`，Rust 的 `ln` 来自平台的数学库，极少数输入上最后一位可能不同。
    pub fn next_gaussian(&self) -> f64 {
        if let Some(value) = self.next_gaussian.take() {
            return value;
        }
        loop {
            let x = 2.0 * self.next_double() - 1.0;
            let y = 2.0 * self.next_double() - 1.0;
            let radius_squared = x * x + y * y;
            if radius_squared < 1.0 && radius_squared != 0.0 {
                let multiplier = (-2.0 * radius_squared.ln() / radius_squared).sqrt();
                self.next_gaussian.set(Some(y * multiplier));
                return x * multiplier;
            }
        }
    }

    /// `consumeCount(rounds)`：取 `rounds` 次 `nextInt()`，结果不要。
    pub fn consume_count(&self, rounds: i32) {
        for _ in 0..rounds {
            self.next_int_unbounded();
        }
    }

    /// `fork()`：用这个源的下一个 `long` 做种子的新随机源。
    pub fn fork(&self) -> Self {
        Self::new(self.next_long())
    }

    /// `Util.getRandom(array, random)`。
    pub fn pick<T: Copy>(&self, items: &[T]) -> T {
        let len = i32::try_from(items.len()).expect("官方的数组放得进 int");
        items[self.next_int(len) as usize]
    }

    /// `Util.shuffle(list, random)`：Fisher-Yates，从后往前换。
    pub fn shuffle<T>(&self, items: &mut [T]) {
        for size in (2..=items.len()).rev() {
            let len = i32::try_from(size).expect("官方的列表放得进 int");
            let swap_to = self.next_int(len) as usize;
            items.swap(size - 1, swap_to);
        }
    }

    /// `Direction.getRandom(random)`：六个面里随便一个。
    pub fn direction(&self) -> Direction {
        self.pick(&Direction::ALL)
    }

    /// `Direction.Plane.HORIZONTAL.getRandomDirection(random)`。
    pub fn horizontal_direction(&self) -> Direction {
        self.pick(&Direction::HORIZONTAL)
    }

    /// `Direction.Plane.HORIZONTAL.shuffledCopy(random)`。
    pub fn shuffled_horizontal(&self) -> [Direction; 4] {
        let mut directions = Direction::HORIZONTAL;
        self.shuffle(&mut directions);
        directions
    }

    /// `Direction.allShuffled(random)`：六个面洗一遍。
    pub fn shuffled_directions(&self) -> [Direction; 6] {
        let mut directions = Direction::ALL;
        self.shuffle(&mut directions);
        directions
    }

    /// 现在的内部种子，测试和 oracle 用它确认两边取数的次数相同。
    pub fn seed(&self) -> i64 {
        self.seed.get()
    }
}

/// 一个世界的随机状态：`Level.random` 和 `Level.randValue`。
#[derive(Debug, Clone)]
pub struct LevelRandom {
    source: LegacyRandom,
    /// `getBlockRandomPos` 用的另一个线性同余，和 `random` 无关。
    rand_value: Cell<i32>,
}

impl LevelRandom {
    /// 官方两个都用每次不同的种子。这里都从 `seed` 派生，种子相同时整个序列相同，oracle 靠这个比较。
    pub fn new(seed: i64) -> Self {
        Self {
            source: LegacyRandom::new(seed),
            rand_value: Cell::new(LegacyRandom::new(!seed).next_int_unbounded()),
        }
    }

    /// `Level.random`。
    pub fn source(&self) -> &LegacyRandom {
        &self.source
    }

    /// `getBlockRandomPos(xo, yo, zo, yMask)`：`origin` 开始的区块段里的一格。
    pub fn block_random_pos(&self, origin: BlockPos, y_mask: i32) -> BlockPos {
        let value = self
            .rand_value
            .get()
            .wrapping_mul(3)
            .wrapping_add(RAND_VALUE_INCREMENT);
        self.rand_value.set(value);
        let bits = value >> 2;
        origin.offset(bits & 15, bits >> 16 & y_mask, bits >> 8 & 15)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 期望值来自 `java.util.Random`，它和 `LegacyRandomSource` 是同一个算法。
    #[test]
    fn integers_match_java() {
        let random = LegacyRandom::new(42);
        assert_eq!(random.next_int_unbounded(), -1_170_105_035);
        assert_eq!(random.next_int(10), 3);
        assert_eq!(random.next_int(16), 10);
        assert_eq!(random.next_long(), 884_324_181_205_335_268);
    }

    #[test]
    fn floats_match_java() {
        let random = LegacyRandom::new(0);
        assert_eq!(random.next_float().to_bits(), 0x3f3b_20b4);
        assert_eq!(random.next_double().to_bits(), 0x3fea_9b2a_21ec_9e5b);
        assert!(random.next_boolean());
    }

    #[test]
    fn gaussians_come_in_pairs() {
        let random = LegacyRandom::new(1);
        let first = random.next_gaussian();
        let seed = random.seed();
        let second = random.next_gaussian();
        assert_ne!(first, second);
        // 第二个数是上一次留下的，不再取数。
        assert_eq!(random.seed(), seed);
    }

    #[test]
    fn bounds_that_are_not_powers_of_two_stay_in_range() {
        let random = LegacyRandom::new(7);
        for bound in [3, 5, 25, 1000, i32::MAX] {
            let value = random.next_int(bound);
            assert!((0..bound).contains(&value));
        }
    }

    #[test]
    fn mth_helpers_skip_the_draw_for_empty_ranges() {
        let random = LegacyRandom::new(3);
        let seed = random.seed();
        assert_eq!(random.mth_next_int(5, 5), 5);
        assert_eq!(random.mth_next_float(1.0, 1.0), 1.0);
        assert_eq!(random.seed(), seed);
        assert!((2..=5).contains(&random.mth_next_int(2, 5)));
    }

    #[test]
    fn shuffles_draw_one_less_than_the_length() {
        let random = LegacyRandom::new(9);
        let twin = LegacyRandom::new(9);
        let mut items = [1, 2, 3, 4];
        random.shuffle(&mut items);
        for bound in [4, 3, 2] {
            twin.next_int(bound);
        }
        assert_eq!(random.seed(), twin.seed());
        let mut sorted = items;
        sorted.sort_unstable();
        assert_eq!(sorted, [1, 2, 3, 4]);
    }

    #[test]
    fn block_random_positions_stay_in_the_section() {
        let random = LevelRandom::new(5);
        let origin = BlockPos::new(-32, 64, 48);
        for _ in 0..100 {
            let pos = random.block_random_pos(origin, 15);
            assert!((0..16).contains(&(pos.x - origin.x)));
            assert!((0..16).contains(&(pos.y - origin.y)));
            assert!((0..16).contains(&(pos.z - origin.z)));
        }
    }
}
