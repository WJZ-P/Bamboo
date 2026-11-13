//! 世界生成用的随机源：`Xoroshiro128PlusPlus`、`XoroshiroRandomSource`、`RandomSupport` 的种子混合、
//! 两种 `PositionalRandomFactory` 和 `WorldgenRandom` 的种子设置，逐位移植。
//!
//! 噪声的构造完全取决于从随机源取数的顺序和次数，所以这里的每一步都照官方来。
//! `LegacyRandomSource` 已经在 `mc-bamboo-blocks` 里（`LegacyRandom`），这里直接用它。

use std::cell::Cell;

use mc_bamboo_blocks::LegacyRandom;
use md5::{Digest, Md5};

/// `RandomSupport.GOLDEN_RATIO_64`。
pub const GOLDEN_RATIO_64: i64 = -7_046_029_254_386_353_131;
/// `RandomSupport.SILVER_RATIO_64`。
pub const SILVER_RATIO_64: i64 = 7_640_891_576_956_012_809;
/// `XoroshiroRandomSource.FLOAT_UNIT`，也就是 2 的 -24 次方。
const FLOAT_UNIT: f32 = 5.960_464_5e-8;
/// `XoroshiroRandomSource.DOUBLE_UNIT`。官方写的是 float 字面量，换成 double 正好是 2 的 -53 次方。
const DOUBLE_UNIT: f64 = 1.110_223e-16_f32 as f64;

/// `RandomSupport.mixStafford13`。
pub fn mix_stafford13(value: i64) -> i64 {
    let mut z = value;
    z = (z ^ ((z as u64) >> 30) as i64).wrapping_mul(-4_658_895_280_553_007_687);
    z = (z ^ ((z as u64) >> 27) as i64).wrapping_mul(-7_723_592_293_110_705_685);
    z ^ ((z as u64) >> 31) as i64
}

/// `RandomSupport.Seed128bit`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seed128 {
    pub lo: i64,
    pub hi: i64,
}

impl Seed128 {
    pub fn xor(self, lo: i64, hi: i64) -> Self {
        Self {
            lo: self.lo ^ lo,
            hi: self.hi ^ hi,
        }
    }

    pub fn mixed(self) -> Self {
        Self {
            lo: mix_stafford13(self.lo),
            hi: mix_stafford13(self.hi),
        }
    }
}

/// `RandomSupport.upgradeSeedTo128bitUnmixed`。
pub fn upgrade_seed_to_128bit_unmixed(legacy_seed: i64) -> Seed128 {
    let lo = legacy_seed ^ SILVER_RATIO_64;
    Seed128 {
        lo,
        hi: lo.wrapping_add(GOLDEN_RATIO_64),
    }
}

/// `RandomSupport.upgradeSeedTo128bit`。
pub fn upgrade_seed_to_128bit(legacy_seed: i64) -> Seed128 {
    upgrade_seed_to_128bit_unmixed(legacy_seed).mixed()
}

/// `RandomSupport.seedFromHashOf`：UTF-8 的 MD5，前后各八个字节按大端读成两个 long。
pub fn seed_from_hash_of(input: &str) -> Seed128 {
    let digest = Md5::digest(input.as_bytes());
    let mut lo = [0u8; 8];
    let mut hi = [0u8; 8];
    lo.copy_from_slice(&digest[..8]);
    hi.copy_from_slice(&digest[8..]);
    Seed128 {
        lo: i64::from_be_bytes(lo),
        hi: i64::from_be_bytes(hi),
    }
}

/// `String.hashCode()`：UTF-16 代码单元的 31 进制多项式，按 int 溢出。
pub fn java_string_hash(text: &str) -> i32 {
    let mut hash: i32 = 0;
    for unit in text.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    hash
}

/// `Mth.getSeed(x, y, z)`：按位置派生种子。`x * 3129871` 在官方是 int 乘法，会溢出。
pub fn positional_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut seed = i64::from(x.wrapping_mul(3_129_871))
        ^ i64::from(z).wrapping_mul(116_129_781)
        ^ i64::from(y);
    seed = seed
        .wrapping_mul(seed)
        .wrapping_mul(42_317_861)
        .wrapping_add(seed.wrapping_mul(11));
    seed >> 16
}

/// `Xoroshiro128PlusPlus`。
#[derive(Debug, Clone)]
pub struct Xoroshiro128PlusPlus {
    state: Cell<(i64, i64)>,
}

impl Xoroshiro128PlusPlus {
    pub fn new(seed: Seed128) -> Self {
        let state = if seed.lo | seed.hi == 0 {
            (GOLDEN_RATIO_64, SILVER_RATIO_64)
        } else {
            (seed.lo, seed.hi)
        };
        Self {
            state: Cell::new(state),
        }
    }

    pub fn next_long(&self) -> i64 {
        let (s0, mut s1) = self.state.get();
        let result = s0.wrapping_add(s1).rotate_left(17).wrapping_add(s0);
        s1 ^= s0;
        self.state
            .set((s0.rotate_left(49) ^ s1 ^ (s1 << 21), s1.rotate_left(28)));
        result
    }
}

/// `XoroshiroRandomSource`。
#[derive(Debug, Clone)]
pub struct XoroshiroRandom {
    generator: Xoroshiro128PlusPlus,
    /// `MarsagliaPolarGaussian` 一次算两个数，第二个留到下一次。
    next_gaussian: Cell<Option<f64>>,
}

impl XoroshiroRandom {
    /// `XoroshiroRandomSource(long seed)`：先把 64 位种子升级成 128 位。
    pub fn new(seed: i64) -> Self {
        Self::from_seed128(upgrade_seed_to_128bit(seed))
    }

    pub fn from_seed128(seed: Seed128) -> Self {
        Self {
            generator: Xoroshiro128PlusPlus::new(seed),
            next_gaussian: Cell::new(None),
        }
    }

    pub fn from_parts(lo: i64, hi: i64) -> Self {
        Self::from_seed128(Seed128 { lo, hi })
    }

    pub fn set_seed(&self, seed: i64) {
        let upgraded = upgrade_seed_to_128bit(seed);
        let state = if upgraded.lo | upgraded.hi == 0 {
            (GOLDEN_RATIO_64, SILVER_RATIO_64)
        } else {
            (upgraded.lo, upgraded.hi)
        };
        self.generator.state.set(state);
        self.next_gaussian.set(None);
    }

    pub fn next_long(&self) -> i64 {
        self.generator.next_long()
    }

    pub fn next_int_unbounded(&self) -> i32 {
        self.generator.next_long() as i32
    }

    /// `nextInt(bound)`：官方用 32 位的 Lemire 无偏取模。
    pub fn next_int(&self, bound: i32) -> i32 {
        assert!(bound > 0, "Bound must be positive");
        let bound_long = i64::from(bound);
        let mut random_bits = i64::from(self.next_int_unbounded() as u32);
        let mut multiplied = random_bits.wrapping_mul(bound_long);
        let mut fractional = multiplied & 0xFFFF_FFFF;
        if fractional < bound_long {
            let unbiased_start = i64::from((!(bound as u32)).wrapping_add(1) % (bound as u32));
            while fractional < unbiased_start {
                random_bits = i64::from(self.next_int_unbounded() as u32);
                multiplied = random_bits.wrapping_mul(bound_long);
                fractional = multiplied & 0xFFFF_FFFF;
            }
        }
        (multiplied >> 32) as i32
    }

    pub fn next_boolean(&self) -> bool {
        self.generator.next_long() & 1 != 0
    }

    pub fn next_float(&self) -> f32 {
        self.next_bits(24) as f32 * FLOAT_UNIT
    }

    pub fn next_double(&self) -> f64 {
        self.next_bits(53) as f64 * DOUBLE_UNIT
    }

    /// `nextGaussian()`，官方的 `MarsagliaPolarGaussian`。
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

    pub fn consume_count(&self, rounds: i32) {
        for _ in 0..rounds {
            self.generator.next_long();
        }
    }

    pub fn fork(&self) -> Self {
        Self::from_parts(self.generator.next_long(), self.generator.next_long())
    }

    pub fn fork_positional(&self) -> PositionalFactory {
        PositionalFactory::Xoroshiro {
            lo: self.generator.next_long(),
            hi: self.generator.next_long(),
        }
    }

    /// `nextBits(bits)`：取 long 的高 `bits` 位。
    fn next_bits(&self, bits: u32) -> i64 {
        ((self.generator.next_long() as u64) >> (64 - bits)) as i64
    }
}

/// 官方 `WorldgenRandom.Algorithm`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Legacy,
    Xoroshiro,
}

impl Algorithm {
    pub fn new_instance(self, seed: i64) -> Random {
        match self {
            Self::Legacy => Random::Legacy(LegacyRandom::new(seed)),
            Self::Xoroshiro => Random::Xoroshiro(XoroshiroRandom::new(seed)),
        }
    }
}

/// 官方 `RandomSource` 的两种实现。
#[derive(Debug, Clone)]
pub enum Random {
    Legacy(LegacyRandom),
    Xoroshiro(XoroshiroRandom),
}

impl Random {
    pub fn set_seed(&self, seed: i64) {
        match self {
            Self::Legacy(random) => random.set_seed(seed),
            Self::Xoroshiro(random) => random.set_seed(seed),
        }
    }

    pub fn next_int_unbounded(&self) -> i32 {
        match self {
            Self::Legacy(random) => random.next_int_unbounded(),
            Self::Xoroshiro(random) => random.next_int_unbounded(),
        }
    }

    pub fn next_int(&self, bound: i32) -> i32 {
        match self {
            Self::Legacy(random) => random.next_int(bound),
            Self::Xoroshiro(random) => random.next_int(bound),
        }
    }

    pub fn next_long(&self) -> i64 {
        match self {
            Self::Legacy(random) => random.next_long(),
            Self::Xoroshiro(random) => random.next_long(),
        }
    }

    pub fn next_boolean(&self) -> bool {
        match self {
            Self::Legacy(random) => random.next_boolean(),
            Self::Xoroshiro(random) => random.next_boolean(),
        }
    }

    pub fn next_float(&self) -> f32 {
        match self {
            Self::Legacy(random) => random.next_float(),
            Self::Xoroshiro(random) => random.next_float(),
        }
    }

    pub fn next_double(&self) -> f64 {
        match self {
            Self::Legacy(random) => random.next_double(),
            Self::Xoroshiro(random) => random.next_double(),
        }
    }

    pub fn next_gaussian(&self) -> f64 {
        match self {
            Self::Legacy(random) => random.next_gaussian(),
            Self::Xoroshiro(random) => random.next_gaussian(),
        }
    }

    pub fn consume_count(&self, rounds: i32) {
        match self {
            Self::Legacy(random) => random.consume_count(rounds),
            Self::Xoroshiro(random) => random.consume_count(rounds),
        }
    }

    pub fn fork(&self) -> Self {
        match self {
            Self::Legacy(random) => Self::Legacy(random.fork()),
            Self::Xoroshiro(random) => Self::Xoroshiro(random.fork()),
        }
    }

    pub fn fork_positional(&self) -> PositionalFactory {
        match self {
            Self::Legacy(random) => PositionalFactory::Legacy {
                seed: random.next_long(),
            },
            Self::Xoroshiro(random) => random.fork_positional(),
        }
    }

    /// `BitRandomSource.next(bits)`：只有 `LegacyRandomSource` 有，
    /// `WorldgenRandom` 包着别的源时自己从 `nextLong` 取高位。
    fn next(&self, bits: u32) -> i32 {
        match self {
            Self::Legacy(random) => random.next(bits),
            Self::Xoroshiro(random) => ((random.next_long() as u64) >> (64 - bits)) as i32,
        }
    }
}

/// `PositionalRandomFactory` 的两种实现。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionalFactory {
    /// `LegacyRandomSource.LegacyPositionalRandomFactory`。
    Legacy { seed: i64 },
    /// `XoroshiroRandomSource.XoroshiroPositionalRandomFactory`。
    Xoroshiro { lo: i64, hi: i64 },
}

impl PositionalFactory {
    /// `at(x, y, z)`。
    pub fn at(&self, x: i32, y: i32, z: i32) -> Random {
        let seed = positional_seed(x, y, z);
        match *self {
            Self::Legacy { seed: factory } => Random::Legacy(LegacyRandom::new(seed ^ factory)),
            Self::Xoroshiro { lo, hi } => {
                Random::Xoroshiro(XoroshiroRandom::from_parts(seed ^ lo, hi))
            }
        }
    }

    /// `fromHashOf(name)`：旧算法用 `String.hashCode`，新算法用 MD5。
    pub fn from_hash_of(&self, name: &str) -> Random {
        match *self {
            Self::Legacy { seed } => {
                Random::Legacy(LegacyRandom::new(i64::from(java_string_hash(name)) ^ seed))
            }
            Self::Xoroshiro { lo, hi } => Random::Xoroshiro(XoroshiroRandom::from_seed128(
                seed_from_hash_of(name).xor(lo, hi),
            )),
        }
    }

    /// `fromSeed(seed)`。
    pub fn from_seed(&self, seed: i64) -> Random {
        match *self {
            Self::Legacy { .. } => Random::Legacy(LegacyRandom::new(seed)),
            Self::Xoroshiro { lo, hi } => {
                Random::Xoroshiro(XoroshiroRandom::from_parts(seed ^ lo, seed ^ hi))
            }
        }
    }
}

/// `WorldgenRandom`：套在别的随机源外面，`next(bits)` 数一下取了几次。
///
/// 它继承 `LegacyRandomSource`，所以 `nextLong` 这些方法走的是 `BitRandomSource` 的默认实现，
/// 两次 `next(32)` 拼起来，而不是内层源的 `nextLong`。
#[derive(Debug, Clone)]
pub struct WorldgenRandom {
    source: Random,
    count: Cell<u32>,
}

impl WorldgenRandom {
    pub fn new(source: Random) -> Self {
        Self {
            source,
            count: Cell::new(0),
        }
    }

    /// `getCount()`。
    pub fn count(&self) -> u32 {
        self.count.get()
    }

    pub fn source(&self) -> &Random {
        &self.source
    }

    pub fn set_seed(&self, seed: i64) {
        self.source.set_seed(seed);
    }

    fn next(&self, bits: u32) -> i32 {
        self.count.set(self.count.get() + 1);
        self.source.next(bits)
    }

    /// `nextLong()`：`BitRandomSource` 的默认实现。
    pub fn next_long(&self) -> i64 {
        let upper = i64::from(self.next(32));
        let lower = i64::from(self.next(32));
        (upper << 32).wrapping_add(lower)
    }

    /// `nextInt()`：`BitRandomSource.nextInt()` = `next(32)`。
    pub fn next_int_unbounded(&self) -> i32 {
        self.next(32)
    }

    /// `nextInt(bound)`：`BitRandomSource` 的默认实现。
    ///
    /// `WorldgenRandom` 继承 `LegacyRandomSource`，不管内层包的是 Legacy 还是 Xoroshiro，
    /// 这里都走 `next(bits)` 的默认 31 位无偏取模，不是内层源自己的 Lemire 实现。
    pub fn next_int(&self, bound: i32) -> i32 {
        assert!(bound > 0, "Bound must be positive");
        if bound & (bound - 1) == 0 {
            return ((i64::from(bound) * i64::from(self.next(31))) >> 31) as i32;
        }
        loop {
            let sample = self.next(31);
            let modulo = sample % bound;
            if sample.wrapping_sub(modulo).wrapping_add(bound - 1) >= 0 {
                return modulo;
            }
        }
    }

    /// `Mth.randomBetweenInclusive(random, min, max)`。
    pub fn next_int_between_inclusive(&self, min: i32, max: i32) -> i32 {
        self.next_int(max - min + 1) + min
    }

    /// `nextFloat()`：`BitRandomSource` 的默认实现，`next(24) * 2^-24`。
    pub fn next_float(&self) -> f32 {
        self.next(24) as f32 * 5.960_464_5e-8_f32
    }

    /// `nextDouble()`：`BitRandomSource` 的默认实现，两次 `next` 拼 53 位。
    pub fn next_double(&self) -> f64 {
        let upper = i64::from(self.next(26));
        let lower = i64::from(self.next(27));
        ((upper << 27) + lower) as f64 * 1.110_223_024_625_156_5e-16_f64
    }

    /// `nextBoolean()`：`next(1)`。
    pub fn next_boolean(&self) -> bool {
        self.next(1) != 0
    }

    /// `setDecorationSeed(seed, chunkX, chunkZ)`，返回官方那个 population seed。
    pub fn set_decoration_seed(&self, seed: i64, chunk_x: i32, chunk_z: i32) -> i64 {
        self.set_seed(seed);
        let x_scale = self.next_long() | 1;
        let z_scale = self.next_long() | 1;
        let result = i64::from(chunk_x)
            .wrapping_mul(x_scale)
            .wrapping_add(i64::from(chunk_z).wrapping_mul(z_scale))
            ^ seed;
        self.set_seed(result);
        result
    }

    /// `setFeatureSeed(seed, index, step)`。
    pub fn set_feature_seed(&self, seed: i64, index: i32, step: i32) {
        let result = seed
            .wrapping_add(i64::from(index))
            .wrapping_add(i64::from(10_000 * step));
        self.set_seed(result);
    }

    /// `setLargeFeatureSeed(seed, chunkX, chunkZ)`。
    pub fn set_large_feature_seed(&self, seed: i64, chunk_x: i32, chunk_z: i32) {
        self.set_seed(seed);
        let x_scale = self.next_long();
        let z_scale = self.next_long();
        let result = i64::from(chunk_x).wrapping_mul(x_scale)
            ^ i64::from(chunk_z).wrapping_mul(z_scale)
            ^ seed;
        self.set_seed(result);
    }

    /// `setLargeFeatureWithSalt(seed, x, z, blend)`。
    pub fn set_large_feature_with_salt(&self, seed: i64, x: i32, z: i32, blend: i32) {
        let result = i64::from(x)
            .wrapping_mul(341_873_128_712)
            .wrapping_add(i64::from(z).wrapping_mul(132_897_987_541))
            .wrapping_add(seed)
            .wrapping_add(i64::from(blend));
        self.set_seed(result);
    }
}

/// `WorldgenRandom.seedSlimeChunk` 用的种子。官方 `x * x * 4987142` 这几项是 int 乘法，会溢出。
pub fn slime_chunk_seed(x: i32, z: i32, seed: i64, salt: i64) -> i64 {
    seed.wrapping_add(i64::from(x.wrapping_mul(x).wrapping_mul(4_987_142)))
        .wrapping_add(i64::from(x.wrapping_mul(5_947_611)))
        .wrapping_add(i64::from(z.wrapping_mul(z)).wrapping_mul(4_392_871))
        .wrapping_add(i64::from(z.wrapping_mul(389_711)))
        ^ salt
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 期望值来自官方 `RandomSupport` 的常量。
    #[test]
    fn seed_upgrade_matches_official() {
        let unmixed = upgrade_seed_to_128bit_unmixed(0);
        assert_eq!(unmixed.lo, SILVER_RATIO_64);
        assert_eq!(unmixed.hi, SILVER_RATIO_64.wrapping_add(GOLDEN_RATIO_64));
    }

    /// `Xoroshiro128PlusPlus(0, 0)` 要换成两个比例常数。
    #[test]
    fn all_zero_seed_is_replaced() {
        let random = Xoroshiro128PlusPlus::new(Seed128 { lo: 0, hi: 0 });
        assert_eq!(random.state.get(), (GOLDEN_RATIO_64, SILVER_RATIO_64));
    }

    /// MD5 的公开测试向量。
    #[test]
    fn md5_seed_reads_big_endian_halves() {
        // md5("") = d41d8cd98f00b204e9800998ecf8427e
        let seed = seed_from_hash_of("");
        assert_eq!(seed.lo as u64, 0xd41d_8cd9_8f00_b204);
        assert_eq!(seed.hi as u64, 0xe980_0998_ecf8_427e);
    }

    #[test]
    fn java_string_hash_matches_jdk() {
        assert_eq!(java_string_hash(""), 0);
        assert_eq!(java_string_hash("a"), 97);
        assert_eq!(java_string_hash("octave_0"), 1_261_148_513);
        assert_eq!(java_string_hash("octave_-7"), 440_898_201);
        assert_eq!(java_string_hash("minecraft:temperature"), -549_971_161);
        assert_eq!(java_string_hash("minecraft:terrain"), 1_657_813_608);
    }

    #[test]
    fn positional_seed_matches_mth_get_seed() {
        assert_eq!(positional_seed(0, 0, 0), 0);
        assert_eq!(positional_seed(1, 2, 3), -33_674_130_277_896);
        assert_eq!(positional_seed(-1, -2, -3), -33_674_130_395_733);
        assert_eq!(positional_seed(16, -32, 48), 105_146_910_863_167);
    }

    #[test]
    fn bounded_integers_stay_in_range() {
        let random = XoroshiroRandom::new(42);
        for bound in [1, 2, 3, 7, 256, 1000, i32::MAX] {
            let value = random.next_int(bound);
            assert!((0..bound).contains(&value), "bound {bound} gave {value}");
        }
    }

    #[test]
    fn gaussians_come_in_pairs() {
        let random = XoroshiroRandom::new(1);
        let first = random.next_gaussian();
        let state = random.generator.state.get();
        let second = random.next_gaussian();
        assert_ne!(first, second);
        // 第二个数是上一次留下的，不再取数。
        assert_eq!(random.generator.state.get(), state);
    }

    /// 下面这些期望值都是官方代码在这台机器上打出来的。
    #[test]
    fn xoroshiro_matches_official() {
        let random = XoroshiroRandom::new(0);
        assert_eq!(random.next_long(), 3_038_984_756_725_240_190);
        assert_eq!(random.next_long(), -3_694_039_286_755_638_414);
        assert_eq!(random.next_long(), 4_633_751_808_701_151_732);

        let random = XoroshiroRandom::new(42);
        assert_eq!(random.next_int(100), 41);
        assert_eq!(random.next_int(100), 31);
        assert_eq!(random.next_int(100), 85);

        let random = XoroshiroRandom::new(42);
        assert_eq!(random.next_double().to_bits(), 4_604_889_475_255_711_931);
        assert_eq!(random.next_float().to_bits(), 1_053_541_890);
        assert_eq!(random.next_gaussian().to_bits(), 4_603_565_654_530_321_701);
        assert!(random.next_boolean());
    }

    #[test]
    fn seed_mixing_matches_official() {
        let upgraded = upgrade_seed_to_128bit(42);
        assert_eq!(upgraded.lo, 6_720_814_022_939_733_433);
        assert_eq!(upgraded.hi, -2_851_323_883_594_622_011);
        let hashed = seed_from_hash_of("minecraft:temperature");
        assert_eq!(hashed.lo, 6_664_882_324_328_353_151);
        assert_eq!(hashed.hi, -587_597_586_455_377_528);
        assert_eq!(mix_stafford13(1), 6_238_072_747_940_578_789);
    }

    #[test]
    fn positional_factories_match_official() {
        let factory = XoroshiroRandom::new(7).fork_positional();
        assert_eq!(factory.at(1, 2, 3).next_long(), -1_232_936_358_448_939_199);
        assert_eq!(
            factory.from_hash_of("octave_-7").next_long(),
            882_858_770_697_765_102
        );
        assert_eq!(factory.from_seed(99).next_long(), 2_426_626_644_417_679_601);
    }

    #[test]
    fn worldgen_seeds_match_official() {
        let random = || WorldgenRandom::new(Random::Xoroshiro(XoroshiroRandom::new(5)));

        let decoration = random();
        assert_eq!(
            decoration.set_decoration_seed(1234, 7, -9),
            -5_068_086_870_331_793_044
        );
        assert_eq!(decoration.next_long(), -169_777_228_947_430_164);
        assert_eq!(decoration.count(), 6);

        let feature = random();
        feature.set_feature_seed(1234, 3, 2);
        assert_eq!(feature.next_long(), 3_265_680_326_279_950_252);

        let large = random();
        large.set_large_feature_seed(1234, 7, -9);
        assert_eq!(large.next_long(), 6_500_184_741_357_636_233);

        let salted = random();
        salted.set_large_feature_with_salt(1234, 7, -9, 11);
        assert_eq!(salted.next_long(), 3_195_103_775_342_378_780);
    }

    /// `WorldgenRandom.seedSlimeChunk` 给的是种子，官方拿它造一个旧算法的随机源。
    #[test]
    fn slime_chunk_seed_matches_official() {
        let seed = slime_chunk_seed(7, -9, 1234, 987_654_321);
        assert_eq!(LegacyRandom::new(seed).next_int(10), 2);
    }

    /// `WorldgenRandom` 包着 xoroshiro 时，`nextLong` 是两次 `next(32)`，不是内层的 `nextLong`。
    #[test]
    fn worldgen_random_long_is_two_halves() {
        let inner = XoroshiroRandom::new(7);
        let first = inner.next_long();
        let second = inner.next_long();
        let expected = ((first as u64 >> 32) as i64) << 32;
        let expected = expected.wrapping_add(i64::from((second as u64 >> 32) as i32));

        let wrapper = WorldgenRandom::new(Random::Xoroshiro(XoroshiroRandom::new(7)));
        assert_eq!(wrapper.next_long(), expected);
        assert_eq!(wrapper.count(), 2);
    }
}

/// `WorldgenRandom` 的 `BitRandomSource` 默认方法与官方逐位一致。
///
/// 期望值来自 `RandProbe.java`（`setDecorationSeed(0,0,0)` 再
/// `setFeatureSeed(deco,0,6)` 后的 12 个 `nextDouble`）和
/// `RandProbe2.java` / `OreProbe2.java`（`setFeatureSeed(1234,3,2)` 后的
/// `nextFloat`、`nextInt(16)`、`nextInt(321)` 序列）。
#[cfg(test)]
mod bit_source_tests {
    use super::*;

    #[test]
    fn next_double_and_float_match_official_bits() {
        let doubles: [u64; 12] = [
            0x3fe5176a7c84f4e0,
            0x3fd3ae22ec25f982,
            0x3fe21317d64bb543,
            0x3f9eecb1b3865040,
            0x3fed928a2f45aae2,
            0x3fe2d596e048cffe,
            0x3fe88d1290842d31,
            0x3fe31bd6f7bfcb1b,
            0x3fc7d41e9728d59c,
            0x3fe22d31f3aa9fa3,
            0x3fea431af13bd658,
            0x3fd69e680b06f434,
        ];
        let floats: [u32; 8] = [
            0x3e354818, 0x3eabcbde, 0x3ed271ae, 0x3f29410d, 0x3f5b6976, 0x3f155757, 0x3ee4b004,
            0x3e2925d8,
        ];
        let r = WorldgenRandom::new(Algorithm::Xoroshiro.new_instance(0));
        let deco = r.set_decoration_seed(0, 0, 0);
        r.set_feature_seed(deco, 0, 6);
        for (i, want) in doubles.iter().enumerate() {
            assert_eq!(r.next_double().to_bits(), *want, "nextDouble {i}");
        }
        let r2 = WorldgenRandom::new(Algorithm::Xoroshiro.new_instance(0));
        r2.set_feature_seed(1234, 3, 2);
        for (i, want) in floats.iter().enumerate() {
            assert_eq!(r2.next_float().to_bits(), *want, "nextFloat {i}");
        }
        assert_eq!(r2.count(), 8);

        let r3 = WorldgenRandom::new(Algorithm::Xoroshiro.new_instance(0));
        r3.set_feature_seed(1234, 3, 2);
        let ni16: [i32; 20] = [
            2, 5, 6, 10, 13, 9, 7, 2, 1, 5, 9, 5, 11, 9, 11, 9, 4, 3, 14, 14,
        ];
        let ni321: [i32; 20] = [
            120, 74, 0, 130, 167, 278, 166, 163, 160, 213, 284, 47, 93, 52, 266, 213, 1, 94, 74,
            163,
        ];
        for (i, want) in ni16.iter().enumerate() {
            assert_eq!(r3.next_int(16), *want, "nextInt(16) {i}");
        }
        for (i, want) in ni321.iter().enumerate() {
            assert_eq!(r3.next_int(321), *want, "nextInt(321) {i}");
        }
    }
}
