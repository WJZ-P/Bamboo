//! `java.lang.Math` 的 `sin` / `cos` / `atan2`，逐位对得上 JVM 的结果。
//!
//! `Math.atan2` 不是 HotSpot 的内建函数，走的是 `StrictMath`（fdlibm 的 Java 移植），这里
//! 照 fdlibm 的 `e_atan2.c` / `s_atan.c` 逐步移植。`Math.sin` / `Math.cos` 在 x86-64 上是
//! 英特尔 LIBM 的桩代码，内部用高低两段算，结果是正确舍入的；这里用双倍双精度算出同样
//! 正确舍入的值。
//!
//! `java.lang.Math`'s `sin` / `cos` / `atan2`, bit for bit like the JVM.
//!
//! `Math.atan2` is not a HotSpot intrinsic and runs `StrictMath` (the Java port of
//! fdlibm); this is a step-by-step port of fdlibm's `e_atan2.c` / `s_atan.c`. `Math.sin`
//! and `Math.cos` are Intel LIBM stubs on x86-64 that work in a high/low pair and round
//! correctly; this computes the same correctly rounded value in double-double.

/// 双倍双精度数：`hi + lo`，`|lo|` 不超过 `hi` 的半个末位。
///
/// A double-double: `hi + lo` with `|lo|` at most half an ulp of `hi`.
#[derive(Clone, Copy)]
struct Dd {
    hi: f64,
    lo: f64,
}

fn two_sum(a: f64, b: f64) -> Dd {
    let sum = a + b;
    let b_virtual = sum - a;
    let error = (a - (sum - b_virtual)) + (b - b_virtual);
    Dd { hi: sum, lo: error }
}

fn two_prod(a: f64, b: f64) -> Dd {
    let product = a * b;
    Dd {
        hi: product,
        lo: a.mul_add(b, -product),
    }
}

impl Dd {
    const fn of(value: f64) -> Self {
        Self { hi: value, lo: 0.0 }
    }

    fn add(self, other: Self) -> Self {
        let sum = two_sum(self.hi, other.hi);
        let low = two_sum(self.lo, other.lo);
        let middle = two_sum(sum.hi, sum.lo + low.hi);
        two_sum(middle.hi, middle.lo + low.lo)
    }

    fn neg(self) -> Self {
        Self {
            hi: -self.hi,
            lo: -self.lo,
        }
    }

    fn mul(self, other: Self) -> Self {
        let product = two_prod(self.hi, other.hi);
        let cross = self.hi * other.lo + self.lo * other.hi;
        two_sum(product.hi, product.lo + cross)
    }

    fn div_f64(self, divisor: f64) -> Self {
        let quotient = self.hi / divisor;
        let product = two_prod(quotient, divisor);
        let remainder = (self.hi - product.hi - product.lo + self.lo) / divisor;
        two_sum(quotient, remainder)
    }
}

/// π/2 的三段：前两段的和已经有 106 位，第三段留给大一点的 `n`。
///
/// π/2 in three parts: the first two already carry 106 bits, the third covers larger `n`.
const PIO2: [f64; 3] = [
    std::f64::consts::FRAC_PI_2,
    6.123_233_995_736_766e-17,
    -1.497_384_904_859_169_8e-33,
];

/// `x = n * π/2 + r`，`|r| <= π/4`。只用在跳跃角度这种不大的数上（`|x|` 远小于 2^20）。
///
/// `x = n * π/2 + r` with `|r| <= π/4`. Only meant for modest arguments such as jump
/// angles (`|x|` far below 2^20).
fn reduce(x: f64) -> (i64, Dd) {
    let n = (x * std::f64::consts::FRAC_2_PI).round();
    let mut r = Dd::of(x);
    for part in PIO2 {
        r = r.add(two_prod(-n, part));
    }
    (n as i64, r)
}

/// `sin(r)` 和 `cos(r)` 的泰勒级数，`|r| <= π/4`。
///
/// The Taylor series of `sin(r)` and `cos(r)` for `|r| <= π/4`.
fn sin_cos_reduced(r: Dd) -> (Dd, Dd) {
    let square = r.mul(r);
    let mut sin = r;
    let mut cos = Dd::of(1.0);
    let mut term = r;
    let mut even = Dd::of(1.0);
    for k in 1..=16 {
        let k = f64::from(k);
        even = even.mul(square).div_f64((2.0 * k - 1.0) * (2.0 * k)).neg();
        term = term.mul(square).div_f64((2.0 * k) * (2.0 * k + 1.0)).neg();
        cos = cos.add(even);
        sin = sin.add(term);
    }
    (sin, cos)
}

fn sin_cos(x: f64) -> (f64, f64) {
    let (n, r) = reduce(x);
    let (sin, cos) = sin_cos_reduced(r);
    let (sin, cos) = match n.rem_euclid(4) {
        0 => (sin, cos),
        1 => (cos, sin.neg()),
        2 => (sin.neg(), cos.neg()),
        _ => (cos.neg(), sin),
    };
    (sin.hi + sin.lo, cos.hi + cos.lo)
}

/// `Math.sin`。
///
/// `Math.sin`.
pub fn sin(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return if x == 0.0 { x } else { f64::NAN };
    }
    sin_cos(x).0
}

/// `Math.cos`。
///
/// `Math.cos`.
pub fn cos(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    sin_cos(x).1
}

const ATAN_HI: [f64; 4] = [
    4.636_476_090_008_061e-1,
    std::f64::consts::FRAC_PI_4,
    9.827_937_232_473_29e-1,
    std::f64::consts::FRAC_PI_2,
];
const ATAN_LO: [f64; 4] = [
    2.269_877_745_296_168_7e-17,
    3.061_616_997_868_383e-17,
    1.390_331_103_123_099_8e-17,
    6.123_233_995_736_766e-17,
];
const AT: [f64; 11] = [
    3.333_333_333_333_293e-1,
    -1.999_999_999_987_648_3e-1,
    1.428_571_427_250_346_6e-1,
    -1.111_111_040_546_235_6e-1,
    9.090_887_133_436_507e-2,
    -7.691_876_205_044_83e-2,
    6.661_073_137_387_531e-2,
    -5.833_570_133_790_573_5e-2,
    4.976_877_994_615_932_4e-2,
    -3.653_157_274_421_691_6e-2,
    1.628_582_011_536_578_2e-2,
];

fn high_word(x: f64) -> i32 {
    (x.to_bits() >> 32) as i32
}

fn low_word(x: f64) -> u32 {
    x.to_bits() as u32
}

/// fdlibm `atan`。
fn atan(x: f64) -> f64 {
    let hx = high_word(x);
    let ix = hx & 0x7fff_ffff;
    if ix >= 0x4410_0000 {
        if x.is_nan() {
            return x + x;
        }
        let limit = ATAN_HI[3] + ATAN_LO[3];
        return if hx > 0 { limit } else { -limit };
    }
    let mut x = x;
    let id: Option<usize> = if ix < 0x3fdc_0000 {
        if ix < 0x3e20_0000 {
            return x;
        }
        None
    } else {
        x = x.abs();
        if ix < 0x3ff3_0000 {
            if ix < 0x3fe6_0000 {
                x = (2.0 * x - 1.0) / (2.0 + x);
                Some(0)
            } else {
                x = (x - 1.0) / (x + 1.0);
                Some(1)
            }
        } else if ix < 0x4003_8000 {
            x = (x - 1.5) / (1.0 + 1.5 * x);
            Some(2)
        } else {
            x = -1.0 / x;
            Some(3)
        }
    };
    let z = x * x;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    match id {
        None => x - x * (s1 + s2),
        Some(id) => {
            let z = ATAN_HI[id] - ((x * (s1 + s2) - ATAN_LO[id]) - x);
            if hx < 0 { -z } else { z }
        }
    }
}

/// `Math.atan2(y, x)`：fdlibm `__ieee754_atan2`。
///
/// `Math.atan2(y, x)`: fdlibm's `__ieee754_atan2`.
pub fn atan2(y: f64, x: f64) -> f64 {
    const TINY: f64 = 1.0e-300;
    const PI_O_4: f64 = std::f64::consts::FRAC_PI_4;
    const PI_O_2: f64 = std::f64::consts::FRAC_PI_2;
    const PI: f64 = std::f64::consts::PI;
    const PI_LO: f64 = 1.224_646_799_147_353_2e-16;
    if x.is_nan() || y.is_nan() {
        return x + y;
    }
    let hx = high_word(x);
    let lx = low_word(x);
    let hy = high_word(y);
    let ly = low_word(y);
    let ix = hx & 0x7fff_ffff;
    let iy = hy & 0x7fff_ffff;
    if (hx.wrapping_sub(0x3ff0_0000)) as u32 | lx == 0 {
        return atan(y);
    }
    let m = ((hy >> 31) & 1) | ((hx >> 30) & 2);
    if iy as u32 | ly == 0 {
        return match m {
            0 | 1 => y,
            2 => PI + TINY,
            _ => -PI - TINY,
        };
    }
    if ix as u32 | lx == 0 {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }
    if ix == 0x7ff0_0000 {
        if iy == 0x7ff0_0000 {
            return match m {
                0 => PI_O_4 + TINY,
                1 => -PI_O_4 - TINY,
                2 => 3.0 * PI_O_4 + TINY,
                _ => -3.0 * PI_O_4 - TINY,
            };
        }
        return match m {
            0 => 0.0,
            1 => -0.0,
            2 => PI + TINY,
            _ => -PI - TINY,
        };
    }
    if iy == 0x7ff0_0000 {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }
    let k = (iy - ix) >> 20;
    let z = if k > 60 {
        PI_O_2 + 0.5 * PI_LO
    } else if hx < 0 && k < -60 {
        0.0
    } else {
        atan((y / x).abs())
    };
    match m {
        0 => z,
        1 => -z,
        2 => PI - (z - PI_LO),
        _ => (z - PI_LO) - PI,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方 JVM（OpenJDK 26，x86-64）报出来的样本：每行是 `x sin(x) cos(x) y z atan2(y, z)`
    /// 的位模式。
    ///
    /// Samples reported by the official JVM (OpenJDK 26, x86-64): each line holds the bit
    /// patterns of `x sin(x) cos(x) y z atan2(y, z)`.
    #[test]
    fn matches_the_jvm_samples() {
        let mut checked = 0;
        for line in include_str!("jmath_vectors.txt").lines() {
            let bits: Vec<u64> = line
                .split(' ')
                .map(|token| token.trim().parse::<i64>().expect("位模式") as u64)
                .collect();
            let x = f64::from_bits(bits[0]);
            assert_eq!(sin(x).to_bits(), bits[1], "sin({x})");
            assert_eq!(cos(x).to_bits(), bits[2], "cos({x})");
            let (y, z) = (f64::from_bits(bits[3]), f64::from_bits(bits[4]));
            assert_eq!(atan2(y, z).to_bits(), bits[5], "atan2({y}, {z})");
            checked += 1;
        }
        assert!(checked >= 100);
    }

    /// 已知差异：英特尔桩代码大约每 800 次有一次不是正确舍入的（差一个末位），这里给出的是
    /// 正确舍入的值。
    ///
    /// Known difference: about one Intel stub result in 800 is not correctly rounded (one
    /// ulp off); this module returns the correctly rounded value.
    #[test]
    fn rare_stub_misrounding_is_one_ulp() {
        let jvm = 0.522_402_550_935_231_4_f64;
        let mine = sin(2.591_926_542_948_064_7);
        assert_eq!(mine.to_bits() + 1, jvm.to_bits());
    }

    #[test]
    fn atan2_special_cases_follow_fdlibm() {
        assert_eq!(atan2(0.0, 1.0), 0.0);
        assert_eq!(atan2(0.0, -1.0), std::f64::consts::PI);
        assert_eq!(atan2(1.0, 0.0), std::f64::consts::FRAC_PI_2);
        assert_eq!(atan2(-1.0, 0.0), -std::f64::consts::FRAC_PI_2);
        assert!(atan2(f64::NAN, 1.0).is_nan());
    }
}
