//! 官方 `net.minecraft.util.Interval`：密度函数的取值范围。
//!
//! 它不只是文档：编译 `min` 和 `max` 时官方用范围决定短路的阈值，两个范围不相交时还会整支丢掉，
//! 所以范围算错会改变求值结果。这里逐条照官方实现，包括 NaN 区间（官方的 `NaI`）这个特殊值。

use crate::math::{java_max, java_min, square};

/// 官方的 `Interval`。`NaI` 是官方那个单独的实例，表示“没有有效范围”。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Interval {
    /// `Interval.NaI`。
    NaI,
    Range {
        min: f32,
        max: f32,
    },
}

impl Interval {
    /// `Interval.INFINITE`。
    pub const INFINITE: Self = Self::Range {
        min: f32::NEG_INFINITY,
        max: f32::INFINITY,
    };

    /// `Interval.of(min, max)`。官方在越界和 NaN 时抛异常，这里 panic：合法的数据不会走到。
    ///
    /// 官方先看 `max < min`，但 NaN 比不出大小，所以两种输入都会落到 NaN 那条；
    /// 这里把 NaN 的检查提到前面，哪条先触发和官方一样。
    pub fn of(min: f32, max: f32) -> Self {
        assert!(
            !min.is_nan() && !max.is_nan(),
            "bounds cannot include NaN [{min}; {max}]: use Interval::NaI explicitly"
        );
        assert!(max >= min, "max ({max}) < min ({min})");
        Self::Range { min, max }
    }

    /// `Interval.ofSymmetric(range)`。
    pub fn of_symmetric(range: f32) -> Self {
        Self::of(-range, range)
    }

    /// `Interval.ofExact(value)`。
    pub fn of_exact(value: f32) -> Self {
        Self::of(value, value)
    }

    pub fn is_nai(self) -> bool {
        matches!(self, Self::NaI)
    }

    pub fn min(self) -> f32 {
        match self {
            Self::NaI => f32::NAN,
            Self::Range { min, .. } => min,
        }
    }

    pub fn max(self) -> f32 {
        match self {
            Self::NaI => f32::NAN,
            Self::Range { max, .. } => max,
        }
    }

    pub fn contains(self, value: f32) -> bool {
        value >= self.min() && value <= self.max()
    }

    /// `Interval.encapsulating(List<Interval>)`。
    pub fn encapsulating_all(intervals: &[Self]) -> Self {
        assert!(!intervals.is_empty(), "at least one interval required");
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for interval in intervals {
            if !interval.is_nai() {
                min = java_min(interval.min(), min);
                max = java_max(interval.max(), max);
            }
        }
        if max < min {
            Self::NaI
        } else {
            Self::of(min, max)
        }
    }

    /// `Interval.encapsulating(float, float)`。
    pub fn encapsulating_values(first: f32, second: f32) -> Self {
        if first.is_nan() && second.is_nan() {
            Self::NaI
        } else if first.is_nan() {
            Self::of_exact(second)
        } else if second.is_nan() {
            Self::of_exact(first)
        } else {
            Self::of(java_min(first, second), java_max(first, second))
        }
    }

    /// 私有的 `encapsulating(Interval, float)`。
    fn encapsulating_value(self, second: f32) -> Self {
        if second.is_nan() {
            self
        } else if self.is_nai() {
            Self::of_exact(second)
        } else {
            Self::of(java_min(self.min(), second), java_max(self.max(), second))
        }
    }

    /// `Interval.add`。名字带 `_of` 是为了和 `std::ops` 的那几个方法区分开。
    pub fn add_of(left: Self, right: Self) -> Self {
        let min = left.min() + right.min();
        let max = left.max() + right.max();
        if min.is_nan() || max.is_nan() {
            Self::NaI
        } else {
            Self::of(min, max)
        }
    }

    /// `Interval.sub`。
    pub fn sub_of(left: Self, right: Self) -> Self {
        let min = left.min() - right.max();
        let max = left.max() - right.min();
        if min.is_nan() || max.is_nan() {
            Self::NaI
        } else {
            Self::of(min, max)
        }
    }

    /// `Interval.mul`。
    pub fn mul_of(left: Self, right: Self) -> Self {
        if left.is_nai() || right.is_nai() {
            return Self::NaI;
        }
        let min_min = mul_bound(left.min(), right.min());
        let min_max = mul_bound(left.min(), right.max());
        let max_min = mul_bound(left.max(), right.min());
        let max_max = mul_bound(left.max(), right.max());
        Self::of(
            java_min(java_min(min_min, min_max), java_min(max_min, max_max)),
            java_max(java_max(min_min, min_max), java_max(max_min, max_max)),
        )
    }

    /// `Interval.reciprocal`。
    pub fn reciprocal(input: Self) -> Self {
        if input.is_nai() || (input.min() == 0.0 && input.max() == 0.0) {
            return Self::NaI;
        }
        if !input.contains(0.0) {
            Self::of(1.0 / input.max(), 1.0 / input.min())
        } else if input.max() == 0.0 {
            Self::of(f32::NEG_INFINITY, 1.0 / input.min())
        } else if input.min() == 0.0 {
            Self::of(1.0 / input.max(), f32::INFINITY)
        } else {
            Self::INFINITE
        }
    }

    /// `Interval.div`。
    pub fn div_of(left: Self, right: Self) -> Self {
        Self::mul_of(left, Self::reciprocal(right))
    }

    /// `Interval.min`。
    pub fn min_of(left: Self, right: Self) -> Self {
        if left.is_nai() || right.is_nai() {
            Self::NaI
        } else {
            Self::of(
                java_min(left.min(), right.min()),
                java_min(left.max(), right.max()),
            )
        }
    }

    /// `Interval.max`。
    pub fn max_of(left: Self, right: Self) -> Self {
        if left.is_nai() || right.is_nai() {
            Self::NaI
        } else {
            Self::of(
                java_max(left.min(), right.min()),
                java_max(left.max(), right.max()),
            )
        }
    }

    /// `Interval.clamp`。
    pub fn clamp(input: Self, min: f32, max: f32) -> Self {
        assert!(min <= max, "min ({min}) > max ({max})");
        if input.is_nai() {
            Self::NaI
        } else if input.min() >= max {
            Self::of(max, max)
        } else if input.max() <= min {
            Self::of(min, min)
        } else {
            Self::of(java_max(input.min(), min), java_min(input.max(), max))
        }
    }

    /// `Interval.abs`。
    pub fn abs(input: Self) -> Self {
        if input.is_nai() {
            return Self::NaI;
        }
        let max = java_max(input.min().abs(), input.max().abs());
        if input.contains(0.0) {
            Self::of(0.0, max)
        } else {
            Self::of(java_min(input.min().abs(), input.max().abs()), max)
        }
    }

    /// `Interval.square`。
    pub fn square_of(input: Self) -> Self {
        if input.is_nai() {
            return Self::NaI;
        }
        let max = java_max(square(input.min()), square(input.max()));
        if input.contains(0.0) {
            Self::of(0.0, max)
        } else {
            Self::of(java_min(square(input.min()), square(input.max())), max)
        }
    }

    /// `Interval.pow`。
    pub fn pow(base: Self, exponent: Self) -> Self {
        if base.is_nai() || exponent.is_nai() {
            return Self::NaI;
        }
        if base.min() == base.max() {
            return pow_scalar_base(base.min(), exponent);
        }
        let mut result = Self::encapsulating_all(&[
            pow_scalar_base(base.min(), exponent),
            pow_scalar_base(base.max(), exponent),
        ]);
        if base.contains(0.0) {
            if base.max() > 0.0 {
                result = Self::encapsulating_all(&[result, pow_scalar_base(0.0, exponent)]);
            }
            if base.min() < 0.0 {
                result = Self::encapsulating_all(&[result, pow_scalar_base(-0.0, exponent)]);
            }
        }
        result
    }

    /// `Interval.log`。
    pub fn log(input: Self) -> Self {
        if input.max() < 0.0 {
            return Self::NaI;
        }
        let clipped = Self::max_of(input, Self::of_exact(0.0));
        Self::map_monotonic(clipped, |x| f64::from(x).ln() as f32)
    }

    /// `Interval.mapMonotonic`。
    pub fn map_monotonic(input: Self, op: impl Fn(f32) -> f32) -> Self {
        if input.is_nai() {
            return Self::NaI;
        }
        let mapped_min = op(input.min());
        let mapped_max = op(input.max());
        assert!(
            !mapped_min.is_nan() && !mapped_max.is_nan(),
            "monotonic operator should not produce NaN"
        );
        Self::of(
            java_min(mapped_min, mapped_max),
            java_max(mapped_min, mapped_max),
        )
    }

    /// `Interval.lerp(Interval, Interval, Interval)`。
    pub fn lerp(alpha: Self, first: Self, second: Self) -> Self {
        if alpha.is_nai() || first.is_nai() || second.is_nai() {
            return Self::NaI;
        }
        Self::encapsulating_all(&[
            Self::lerp_bounds(alpha, first.min(), second.min()),
            Self::lerp_bounds(alpha, first.max(), second.min()),
            Self::lerp_bounds(alpha, first.min(), second.max()),
            Self::lerp_bounds(alpha, first.max(), second.max()),
        ])
    }

    /// `Interval.lerp(Interval, float, float)`。
    pub fn lerp_bounds(alpha: Self, first: f32, second: f32) -> Self {
        if alpha.is_nai() || first.is_nan() || second.is_nan() {
            return Self::NaI;
        }
        if first.is_finite() && second.is_finite() {
            Self::encapsulating_values(
                lerp_finite_bound(alpha.min(), first, second),
                lerp_finite_bound(alpha.max(), first, second),
            )
        } else {
            lerp_infinite_bounds(alpha, first, second)
        }
    }

    /// `Interval.sign`。
    pub fn sign(input: Self) -> Self {
        if input.is_nai() {
            return Self::NaI;
        }
        if input.min() == input.max() {
            return Self::of_exact(crate::math::java_signum(input.min()));
        }
        if input.contains(0.0) {
            if input.min() == 0.0 {
                Self::of(0.0, 1.0)
            } else if input.max() == 0.0 {
                Self::of(-1.0, 0.0)
            } else {
                Self::of(-1.0, 1.0)
            }
        } else {
            Self::of_exact(if input.min() > 0.0 { 1.0 } else { -1.0 })
        }
    }
}

/// 私有的 `mulBound`：任一边是零就直接给 0，避免无穷乘零变成 NaN。
fn mul_bound(left: f32, right: f32) -> f32 {
    if left != 0.0 && right != 0.0 {
        left * right
    } else {
        0.0
    }
}

fn lerp_finite_bound(alpha: f32, first: f32, second: f32) -> f32 {
    first + mul_bound(alpha, second - first)
}

fn lerp_infinite_bounds(alpha: Interval, first: f32, second: f32) -> Interval {
    if first == second {
        return Interval::of_exact(first);
    }
    let new_min = lerp_infinite_bound(alpha.min(), first, second);
    let new_max = lerp_infinite_bound(alpha.max(), first, second);
    if new_min.is_nan() || new_max.is_nan() {
        Interval::NaI
    } else {
        Interval::encapsulating_values(new_min, new_max)
    }
}

fn lerp_infinite_bound(alpha: f32, first: f32, second: f32) -> f32 {
    let first_part = mul_bound(1.0 - alpha, first);
    let second_part = mul_bound(alpha, second);
    if !first_part.is_infinite() || !second_part.is_infinite() {
        first_part + second_part
    } else if alpha <= 0.0 {
        if second > first {
            f32::NEG_INFINITY
        } else {
            f32::INFINITY
        }
    } else if alpha >= 1.0 {
        if second > first {
            f32::INFINITY
        } else {
            f32::NEG_INFINITY
        }
    } else {
        f32::NAN
    }
}

/// 私有的 `pow(float base, Interval exponent)`。
fn pow_scalar_base(base: f32, exponent: Interval) -> Interval {
    if base.is_nan() || exponent.is_nai() {
        return Interval::NaI;
    }
    if exponent.min() == exponent.max() {
        let value = java_pow(base, exponent.min());
        return if value.is_nan() {
            Interval::NaI
        } else {
            Interval::of_exact(value)
        };
    }
    if base == 0.0 {
        return Interval::mul_of(
            pow_zero_base(exponent),
            Interval::of_exact(1.0_f32.copysign(base)),
        );
    }
    if base == 1.0 {
        return Interval::of_exact(1.0);
    }
    if base > 0.0 {
        pow_positive_base(base, exponent)
    } else {
        pow_negative_base(base, exponent)
    }
}

fn pow_positive_base(base: f32, exponent: Interval) -> Interval {
    if exponent.min().is_finite() && exponent.max().is_finite() {
        Interval::encapsulating_values(
            java_pow(base, exponent.min()),
            java_pow(base, exponent.max()),
        )
    } else {
        pow_infinite_exponent(base, exponent)
    }
}

fn pow_zero_base(exponent: Interval) -> Interval {
    if exponent.contains(0.0) {
        if exponent.max() == 0.0 {
            Interval::of(1.0, f32::INFINITY)
        } else if exponent.min() == 0.0 {
            Interval::of(0.0, 1.0)
        } else {
            Interval::of(0.0, f32::INFINITY)
        }
    } else if exponent.max() < 0.0 {
        Interval::of_exact(f32::INFINITY)
    } else {
        Interval::of_exact(0.0)
    }
}

fn pow_infinite_exponent(base: f32, exponent: Interval) -> Interval {
    if exponent.min().is_infinite() && exponent.max().is_infinite() {
        Interval::of(0.0, f32::INFINITY)
    } else if exponent.min().is_infinite() {
        if base < 1.0 {
            Interval::of(java_pow(base, exponent.max()), f32::INFINITY)
        } else {
            Interval::of(0.0, java_pow(base, exponent.max()))
        }
    } else if base < 1.0 {
        Interval::of(0.0, java_pow(base, exponent.min()))
    } else {
        Interval::of(java_pow(base, exponent.min()), f32::INFINITY)
    }
}

fn pow_negative_base(base: f32, exponent: Interval) -> Interval {
    let exponent_min_int = exponent.min().ceil();
    let exponent_max_int = exponent.max().floor();
    if exponent_max_int < exponent_min_int {
        return Interval::NaI;
    }
    let base_to_min_int = java_pow(base, exponent_min_int);
    let base_to_max_int = java_pow(base, exponent_max_int);
    let mut result = Interval::encapsulating_values(base_to_min_int, base_to_max_int);
    if exponent_min_int.is_infinite() {
        result = result.encapsulating_value(-base_to_min_int);
    } else if exponent_min_int + 1.0 < exponent_max_int {
        result = result.encapsulating_value(java_pow(base, exponent_min_int + 1.0));
    }
    if exponent_max_int.is_infinite() {
        result = result.encapsulating_value(-base_to_max_int);
    } else if exponent_max_int - 1.0 > exponent_min_int {
        result = result.encapsulating_value(java_pow(base, exponent_max_int - 1.0));
    }
    result
}

/// `(float)Math.pow(base, exponent)`：官方先按 double 算再截断。
fn java_pow(base: f32, exponent: f32) -> f32 {
    f64::from(base).powf(f64::from(exponent)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_and_symmetric_ranges() {
        assert_eq!(
            Interval::of_exact(2.0),
            Interval::Range { min: 2.0, max: 2.0 }
        );
        assert_eq!(
            Interval::of_symmetric(2.0),
            Interval::Range {
                min: -2.0,
                max: 2.0
            }
        );
    }

    /// `mulBound` 让零吃掉无穷，而不是给出 NaN。
    #[test]
    fn multiplying_zero_by_infinity_gives_zero() {
        let product = Interval::mul_of(Interval::of_exact(0.0), Interval::INFINITE);
        assert_eq!(product, Interval::of_exact(0.0));
    }

    #[test]
    fn reciprocal_of_a_range_across_zero_is_infinite() {
        assert_eq!(
            Interval::reciprocal(Interval::of(-1.0, 1.0)),
            Interval::INFINITE
        );
        assert_eq!(
            Interval::reciprocal(Interval::of(2.0, 4.0)),
            Interval::of(0.25, 0.5)
        );
        assert!(Interval::reciprocal(Interval::of_exact(0.0)).is_nai());
    }

    #[test]
    fn clamp_collapses_when_the_input_is_outside() {
        assert_eq!(
            Interval::clamp(Interval::of(5.0, 6.0), 0.0, 1.0),
            Interval::of_exact(1.0)
        );
        assert_eq!(
            Interval::clamp(Interval::of(-6.0, -5.0), 0.0, 1.0),
            Interval::of_exact(0.0)
        );
        assert_eq!(
            Interval::clamp(Interval::of(-2.0, 2.0), -1.0, 1.0),
            Interval::of(-1.0, 1.0)
        );
    }

    #[test]
    fn abs_and_square_pin_the_lower_bound_at_zero() {
        assert_eq!(
            Interval::abs(Interval::of(-3.0, 1.0)),
            Interval::of(0.0, 3.0)
        );
        assert_eq!(
            Interval::abs(Interval::of(2.0, 3.0)),
            Interval::of(2.0, 3.0)
        );
        assert_eq!(
            Interval::square_of(Interval::of(-3.0, 1.0)),
            Interval::of(0.0, 9.0)
        );
    }

    #[test]
    fn encapsulating_ignores_nai_entries() {
        let interval =
            Interval::encapsulating_all(&[Interval::NaI, Interval::of(1.0, 2.0), Interval::NaI]);
        assert_eq!(interval, Interval::of(1.0, 2.0));
        assert!(Interval::encapsulating_all(&[Interval::NaI]).is_nai());
    }

    #[test]
    fn sign_of_a_range_across_zero_spans_both() {
        assert_eq!(
            Interval::sign(Interval::of(-1.0, 1.0)),
            Interval::of(-1.0, 1.0)
        );
        assert_eq!(
            Interval::sign(Interval::of(1.0, 2.0)),
            Interval::of_exact(1.0)
        );
        assert_eq!(
            Interval::sign(Interval::of(0.0, 2.0)),
            Interval::of(0.0, 1.0)
        );
    }
}
