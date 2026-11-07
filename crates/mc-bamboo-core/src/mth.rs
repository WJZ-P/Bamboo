//! 官方 `Mth` 里和方块判定有关的数学函数。
//!
//! 官方的 `sin`、`cos` 是查表的，结果和真正的三角函数不同。客户端用同一张表预测放置方向，
//! 服务端不用它的话，边界角度上两边会算出不同的朝向。

use std::sync::LazyLock;

/// 表的长度，一整圈被切成这么多份。
const SIN_ENTRIES: usize = 65536;
const SIN_MASK: i64 = 65535;
/// `cos(x) = sin(x + 四分之一圈)`。
const COS_OFFSET: f64 = 16384.0;
/// 弧度换算成表下标的比例。官方写的是这个字面量，不是 `65536 / 2π` 的计算结果。
const SIN_SCALE: f64 = 10430.378350470453;

static SIN: LazyLock<Box<[f32]>> = LazyLock::new(|| {
    (0..SIN_ENTRIES)
        .map(|index| (index as f64 / SIN_SCALE).sin() as f32)
        .collect()
});

/// `Mth.sin`。
pub fn sin(radians: f64) -> f32 {
    SIN[table_index(radians * SIN_SCALE)]
}

/// `Mth.cos`。
pub fn cos(radians: f64) -> f32 {
    SIN[table_index(radians * SIN_SCALE + COS_OFFSET)]
}

/// Java 把 `double` 转成 `long` 时向零截断，再取低 16 位。Rust 的 `as` 语义相同。
fn table_index(scaled: f64) -> usize {
    ((scaled as i64) & SIN_MASK) as usize
}

/// `Mth.floor`。
pub fn floor(value: f64) -> i32 {
    value.floor() as i32
}

/// `Mth.frac`。
pub fn frac(value: f64) -> f64 {
    value - value.floor()
}

/// `Mth.sign`：0 算 0，其余取符号。
pub fn sign(value: f64) -> i32 {
    if value == 0.0 {
        0
    } else if value > 0.0 {
        1
    } else {
        -1
    }
}

/// `Mth.lerp`。
pub fn lerp(factor: f64, from: f64, to: f64) -> f64 {
    from + factor * (to - from)
}

/// `Mth.clamp`。比较的顺序和官方相同，所以 `NaN` 的结果也相同。
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min { min } else { value.min(max) }
}

/// `Mth.getSeed`：按坐标算的随机种子，植物的渲染偏移靠它。
///
/// 第一个乘法在官方是 `int` 乘法，溢出后才扩展成 `long`，这里要保留这个行为。
pub const fn seed(x: i32, y: i32, z: i32) -> i64 {
    let mixed = (x.wrapping_mul(3129871) as i64) ^ (z as i64).wrapping_mul(116129781) ^ y as i64;
    let scrambled = mixed
        .wrapping_mul(mixed)
        .wrapping_mul(42317861)
        .wrapping_add(mixed.wrapping_mul(11));
    scrambled >> 16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方 `Mth.SIN` 全部 65536 个数的位模式依次做 FNV-1a 的结果。
    const OFFICIAL_TABLE_HASH: u64 = 2435040284621619975;

    fn table_hash() -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for value in SIN.iter() {
            hash = (hash ^ u64::from(value.to_bits())).wrapping_mul(0x0100_0000_01b3);
        }
        hash
    }

    #[test]
    fn sin_table_matches_the_official_one() {
        assert_eq!(table_hash(), OFFICIAL_TABLE_HASH);
    }

    #[test]
    fn table_lookup_is_not_the_exact_sine() {
        // 查表把角度截断到 1/65536 圈，这个角度上和真正的 sin 差了一点。
        let radians = 1.0_f64;
        assert_ne!(f64::from(sin(radians)), radians.sin());
        assert!((f64::from(sin(radians)) - radians.sin()).abs() < 1e-3);
    }

    #[test]
    fn cos_is_a_quarter_turn_ahead_of_sin() {
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(sin(0.0), 0.0);
    }

    #[test]
    fn negative_angles_wrap_around_the_table() {
        assert_eq!(sin(-1.0), -sin(1.0));
    }

    #[test]
    fn seed_follows_the_int_overflow_of_the_first_multiply() {
        assert_eq!(seed(0, 0, 0), 0);
        // 这些数字来自官方 `Mth.getSeed`。
        assert_eq!(seed(1, 0, 1), 60165349410229);
        assert_eq!(seed(-1, 0, -1), 60165349410229);
        assert_eq!(seed(1000000, 64, -3000000), 80424206700872);
        assert_eq!(seed(5, 0, -7), -113315081188586);
        assert_eq!(seed(30000000, 0, 29999999), 20650537332405);
    }

    #[test]
    fn sign_and_frac_follow_mth() {
        assert_eq!(sign(-0.5), -1);
        assert_eq!(sign(0.0), 0);
        assert_eq!(sign(3.0), 1);
        assert_eq!(frac(-0.25), 0.75);
        assert_eq!(floor(-0.25), -1);
    }
}
