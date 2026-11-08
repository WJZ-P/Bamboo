//! 官方 `Mth` 和 `java.lang.Math` 里世界生成要用的几个函数，按 Java 的语义逐位移植。
//!
//! 浮点运算的顺序和宽度决定最后几位，所以这里不化简任何表达式：
//! 官方写 `(float)Math.sqrt(x)` 就先按 double 开方再截断，官方写 `int * float` 就先把整数转成 float。

/// `Mth.floor(double)`，也就是 `(int)Math.floor(v)`：越界时 Java 的窄化转换会饱和，Rust 的 `as` 相同。
pub fn floor(value: f64) -> i32 {
    value.floor() as i32
}

/// `Mth.floor(float)`。
pub fn floor_f32(value: f32) -> i32 {
    value.floor() as i32
}

/// `Mth.clamp(float, float, float)`：官方写的是 `value < min ? min : Math.min(value, max)`。
pub fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else {
        java_min(value, max)
    }
}

/// `Math.min(float, float)`：NaN 传染，`-0.0` 小于 `0.0`。
pub fn java_min(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a == 0.0 && b == 0.0 {
        // 两个零时按符号位选，`Math.min` 返回 -0.0。
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b { a } else { b }
}

/// `Math.max(float, float)`。
pub fn java_max(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { b } else { a };
    }
    if a > b { a } else { b }
}

/// `Math.signum(float)`：`0.0`、`-0.0` 和 NaN 原样返回。
pub fn java_signum(value: f32) -> f32 {
    if value == 0.0 || value.is_nan() {
        value
    } else {
        1.0_f32.copysign(value)
    }
}

/// `Math.round(float)`：返回 int，按 JDK 的位运算实现，不是“四舍五入到偶数”。
pub fn java_round(value: f32) -> i32 {
    const SIGNIFICAND_WIDTH: i32 = 24;
    const EXP_BIAS: i32 = 127;
    const EXP_BIT_MASK: i32 = 0x7F80_0000;
    const SIGNIF_BIT_MASK: i32 = 0x007F_FFFF;

    let bits = value.to_bits() as i32;
    let biased_exp = (bits & EXP_BIT_MASK) >> (SIGNIFICAND_WIDTH - 1);
    let shift = (SIGNIFICAND_WIDTH - 2 + EXP_BIAS) - biased_exp;
    if shift & -32 == 0 {
        let mut significand = (bits & SIGNIF_BIT_MASK) | (SIGNIF_BIT_MASK + 1);
        if bits < 0 {
            significand = -significand;
        }
        ((significand >> shift) + 1) >> 1
    } else {
        value as i32
    }
}

/// `Mth.sqrt(float)`：官方按 double 开方再截断成 float。
pub fn sqrt_f32(value: f32) -> f32 {
    (f64::from(value)).sqrt() as f32
}

/// `Mth.square(float)`。
pub fn square(value: f32) -> f32 {
    value * value
}

/// `Mth.cube(float)`：官方写的是 `x * x * x`，从左往右算。
pub fn cube(value: f32) -> f32 {
    value * value * value
}

/// `Mth.lerp(alpha, p0, p1)`。
pub fn lerp(alpha: f32, p0: f32, p1: f32) -> f32 {
    p0 + alpha * (p1 - p0)
}

/// `Mth.lerp2(alpha1, alpha2, x00, x10, x01, x11)`。
pub fn lerp2(alpha: (f32, f32), values: [f32; 4]) -> f32 {
    lerp(
        alpha.1,
        lerp(alpha.0, values[0], values[1]),
        lerp(alpha.0, values[2], values[3]),
    )
}

/// `Mth.lerp3(alpha1, alpha2, alpha3, x000, x100, x010, x110, x001, x101, x011, x111)`。
pub fn lerp3(alpha: (f32, f32, f32), values: [f32; 8]) -> f32 {
    lerp(
        alpha.2,
        lerp2(
            (alpha.0, alpha.1),
            [values[0], values[1], values[2], values[3]],
        ),
        lerp2(
            (alpha.0, alpha.1),
            [values[4], values[5], values[6], values[7]],
        ),
    )
}

/// `Mth.smoothstep(x)`。
pub fn smoothstep(x: f32) -> f32 {
    x * x * x * (x * (x * 6.0 - 15.0) + 10.0)
}

/// `Mth.binarySearch(from, to, condition)`。
pub fn binary_search(from: i32, to: i32, condition: impl Fn(i32) -> bool) -> i32 {
    let mut from = from;
    let mut len = to - from;
    while len > 0 {
        let half = len / 2;
        let middle = from + half;
        if condition(middle) {
            len = half;
        } else {
            from = middle + 1;
            len -= half + 1;
        }
    }
    from
}

/// `Math.floorDiv(a, b)`：商往负无穷取整，和 Rust 的 `/` 不同。
pub fn java_floor_div(a: i32, b: i32) -> i32 {
    let quotient = a.wrapping_div(b);
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        quotient - 1
    } else {
        quotient
    }
}

/// `Math.floorMod(a, b)`：余数跟着除数的符号，`b` 是负数时和 `rem_euclid` 不同。
pub fn java_floor_mod(a: i32, b: i32) -> i32 {
    a.wrapping_sub(java_floor_div(a, b).wrapping_mul(b))
}

/// `QuartPos.toBlock(quart)`。
pub fn quart_to_block(quart: i32) -> i32 {
    quart << 2
}

/// `SectionPos.blockToSectionCoord(block)`。
pub fn block_to_section(block: i32) -> i32 {
    block >> 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_matches_java() {
        assert_eq!(java_round(0.5), 1);
        assert_eq!(java_round(-0.5), 0);
        assert_eq!(java_round(1.5), 2);
        assert_eq!(java_round(2.5), 3);
        assert_eq!(java_round(-2.5), -2);
        assert_eq!(java_round(f32::NAN), 0);
        assert_eq!(java_round(f32::INFINITY), i32::MAX);
        assert_eq!(java_round(1e30), i32::MAX);
        assert_eq!(java_round(-1e30), i32::MIN);
    }

    #[test]
    fn signum_keeps_signed_zero() {
        assert_eq!(java_signum(0.0).to_bits(), 0.0_f32.to_bits());
        assert_eq!(java_signum(-0.0).to_bits(), (-0.0_f32).to_bits());
        assert!(java_signum(f32::NAN).is_nan());
        assert_eq!(java_signum(-3.0), -1.0);
        assert_eq!(java_signum(3.0), 1.0);
    }

    #[test]
    fn min_and_max_follow_java_on_zeros() {
        assert_eq!(java_min(0.0, -0.0).to_bits(), (-0.0_f32).to_bits());
        assert_eq!(java_max(0.0, -0.0).to_bits(), 0.0_f32.to_bits());
        assert!(java_min(f32::NAN, 1.0).is_nan());
        assert!(java_max(1.0, f32::NAN).is_nan());
    }

    #[test]
    fn floor_saturates_like_a_java_cast() {
        assert_eq!(floor(-0.5), -1);
        assert_eq!(floor(1e30), i32::MAX);
        assert_eq!(floor(-1e30), i32::MIN);
    }

    #[test]
    fn floor_div_and_mod_follow_java() {
        assert_eq!(java_floor_div(7, 2), 3);
        assert_eq!(java_floor_div(-7, 2), -4);
        assert_eq!(java_floor_div(7, -2), -4);
        assert_eq!(java_floor_mod(-7, 2), 1);
        assert_eq!(java_floor_mod(7, -2), -1);
        assert_eq!(java_floor_mod(-7, -2), -1);
    }

    /// `Mth.binarySearch` 返回第一个让条件成立的下标。
    #[test]
    fn binary_search_finds_the_first_true() {
        let locations = [-1.0_f32, 0.0, 1.0, 2.0];
        let start = |input: f32| binary_search(0, 4, |i| input < locations[i as usize]) - 1;
        assert_eq!(start(-2.0), -1);
        assert_eq!(start(-1.0), 0);
        assert_eq!(start(0.5), 1);
        assert_eq!(start(5.0), 3);
    }
}
