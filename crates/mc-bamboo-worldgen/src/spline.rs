//! 官方 `net.minecraft.util.CubicSpline`：三次样条，`spline` 密度函数用它。
//!
//! 坐标的类型是参数：解析出来的样条上坐标是密度函数（算范围要用），
//! 编译之后换成坐标采样器的下标（求值时按下标缓存，和官方的 `SamplerCoordinate` 一样）。

use crate::{
    interval::Interval,
    math::{binary_search, java_max, java_min, lerp},
};

/// 官方的 `CubicSpline`。
#[derive(Debug, Clone, PartialEq)]
pub enum Spline<C> {
    /// `CubicSpline.Constant`。
    Constant(f32),
    /// `CubicSpline.Multipoint`。
    Multipoint(Multipoint<C>),
}

/// `CubicSpline.Multipoint`：一个坐标和一串（位置、值、导数）。
#[derive(Debug, Clone, PartialEq)]
pub struct Multipoint<C> {
    pub coordinate: C,
    pub locations: Vec<f32>,
    pub values: Vec<Spline<C>>,
    pub derivatives: Vec<f32>,
}

impl<C> Spline<C> {
    /// `CubicSpline.mapCoordinates`。
    pub fn map_coordinates<D>(&self, mapper: &mut impl FnMut(&C) -> D) -> Spline<D> {
        match self {
            Self::Constant(value) => Spline::Constant(*value),
            Self::Multipoint(multipoint) => Spline::Multipoint(Multipoint {
                coordinate: mapper(&multipoint.coordinate),
                locations: multipoint.locations.clone(),
                values: multipoint
                    .values
                    .iter()
                    .map(|value| value.map_coordinates(mapper))
                    .collect(),
                derivatives: multipoint.derivatives.clone(),
            }),
        }
    }

    /// `CubicSpline.forEachCoordinate`。
    pub fn for_each_coordinate(&self, visit: &mut impl FnMut(&C)) {
        if let Self::Multipoint(multipoint) = self {
            visit(&multipoint.coordinate);
            for value in &multipoint.values {
                value.for_each_coordinate(visit);
            }
        }
    }

    /// `CubicSpline.sample(spline, coordinate)`。
    pub fn sample(&self, eval: &mut impl FnMut(&C) -> f32) -> f32 {
        match self {
            Self::Constant(value) => *value,
            Self::Multipoint(multipoint) => multipoint.sample(eval),
        }
    }
}

impl<C> Multipoint<C> {
    /// `CubicSpline.Multipoint.sample`。
    fn sample(&self, eval: &mut impl FnMut(&C) -> f32) -> f32 {
        let input = eval(&self.coordinate);
        let start = find_interval_start(&self.locations, input);
        let last_index = self.locations.len() as i32 - 1;
        if start < 0 {
            let value = self.values[0].sample(eval);
            return linear_extend(input, &self.locations, value, &self.derivatives, 0);
        }
        if start == last_index {
            let index = last_index as usize;
            let value = self.values[index].sample(eval);
            return linear_extend(input, &self.locations, value, &self.derivatives, index);
        }
        let index = start as usize;
        let x1 = self.locations[index];
        let x2 = self.locations[index + 1];
        let t = (input - x1) / (x2 - x1);
        let d1 = self.derivatives[index];
        let d2 = self.derivatives[index + 1];
        let y1 = self.values[index].sample(eval);
        let y2 = self.values[index + 1].sample(eval);
        let a = d1 * (x2 - x1) - (y2 - y1);
        let b = -d2 * (x2 - x1) + (y2 - y1);
        lerp(t, y1, y2) + t * (1.0 - t) * lerp(t, a, b)
    }
}

/// `CubicSpline.Multipoint.findIntervalStart`。
fn find_interval_start(locations: &[f32], input: f32) -> i32 {
    binary_search(0, locations.len() as i32, |index| {
        input < locations[index as usize]
    }) - 1
}

/// `CubicSpline.Multipoint.linearExtend`。
fn linear_extend(
    input: f32,
    locations: &[f32],
    value: f32,
    derivatives: &[f32],
    index: usize,
) -> f32 {
    let derivative = derivatives[index];
    if derivative == 0.0 {
        value
    } else {
        value + derivative * (input - locations[index])
    }
}

/// 能给出范围的坐标，算 `range()` 时要用。
pub trait SplineCoordinate {
    fn range(&self) -> Interval;
}

impl<C: SplineCoordinate> Spline<C> {
    /// `CubicSpline.range()`。
    pub fn range(&self) -> Interval {
        match self {
            Self::Constant(value) => Interval::of_exact(*value),
            Self::Multipoint(multipoint) => multipoint.range(),
        }
    }
}

impl<C: SplineCoordinate> Multipoint<C> {
    /// `CubicSpline.Multipoint.range()`：两端按导数线性外推，中间每一段取三次曲线的保守界。
    fn range(&self) -> Interval {
        let last_index = self.locations.len() - 1;
        let mut min_value = f32::INFINITY;
        let mut max_value = f32::NEG_INFINITY;
        let input_range = self.coordinate.range();
        if input_range.is_nai() {
            return input_range;
        }

        if input_range.min() < self.locations[0] {
            let first_range = self.values[0].range();
            let edge1 = linear_extend(
                input_range.min(),
                &self.locations,
                first_range.min(),
                &self.derivatives,
                0,
            );
            let edge2 = linear_extend(
                input_range.min(),
                &self.locations,
                first_range.max(),
                &self.derivatives,
                0,
            );
            min_value = java_min(min_value, java_min(edge1, edge2));
            max_value = java_max(max_value, java_max(edge1, edge2));
        }

        if input_range.max() > self.locations[last_index] {
            let last_range = self.values[last_index].range();
            let edge1 = linear_extend(
                input_range.max(),
                &self.locations,
                last_range.min(),
                &self.derivatives,
                last_index,
            );
            let edge2 = linear_extend(
                input_range.max(),
                &self.locations,
                last_range.max(),
                &self.derivatives,
                last_index,
            );
            min_value = java_min(min_value, java_min(edge1, edge2));
            max_value = java_max(max_value, java_max(edge1, edge2));
        }

        let value_ranges: Vec<Interval> = self.values.iter().map(Spline::range).collect();
        for range in &value_ranges {
            min_value = java_min(min_value, range.min());
            max_value = java_max(max_value, range.max());
        }

        for index in 0..last_index {
            let x1 = self.locations[index];
            let x2 = self.locations[index + 1];
            let x_diff = x2 - x1;
            let (min1, max1) = (value_ranges[index].min(), value_ranges[index].max());
            let (min2, max2) = (value_ranges[index + 1].min(), value_ranges[index + 1].max());
            let d1 = self.derivatives[index];
            let d2 = self.derivatives[index + 1];
            if d1 != 0.0 || d2 != 0.0 {
                let p1 = d1 * x_diff;
                let p2 = d2 * x_diff;
                let min_lerp1 = java_min(min1, min2);
                let max_lerp1 = java_max(max1, max2);
                let min_a = p1 - max2 + min1;
                let max_a = p1 - min2 + max1;
                let min_b = -p2 + min2 - max1;
                let max_b = -p2 + max2 - min1;
                let min_lerp2 = java_min(min_a, min_b);
                let max_lerp2 = java_max(max_a, max_b);
                min_value = java_min(min_value, min_lerp1 + 0.25 * min_lerp2);
                max_value = java_max(max_value, max_lerp1 + 0.25 * max_lerp2);
            }
        }

        Interval::of(min_value, max_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 只有一个常数坐标的样条：范围就是那个常数。
    struct Fixed(Interval);

    impl SplineCoordinate for Fixed {
        fn range(&self) -> Interval {
            self.0
        }
    }

    fn two_point(d1: f32, d2: f32) -> Spline<Fixed> {
        Spline::Multipoint(Multipoint {
            coordinate: Fixed(Interval::of(-1.0, 1.0)),
            locations: vec![-1.0, 1.0],
            values: vec![Spline::Constant(0.0), Spline::Constant(1.0)],
            derivatives: vec![d1, d2],
        })
    }

    #[test]
    fn samples_the_end_points_exactly() {
        let spline = two_point(0.0, 0.0);
        assert_eq!(spline.sample(&mut |_| -1.0), 0.0);
        assert_eq!(spline.sample(&mut |_| 1.0), 1.0);
        assert_eq!(spline.sample(&mut |_| 0.0), 0.5);
    }

    /// 导数是 0 时两端是水平的，不再外推。
    #[test]
    fn zero_derivatives_do_not_extend() {
        let spline = two_point(0.0, 0.0);
        assert_eq!(spline.sample(&mut |_| -5.0), 0.0);
        assert_eq!(spline.sample(&mut |_| 5.0), 1.0);
    }

    #[test]
    fn non_zero_derivatives_extend_linearly() {
        let spline = two_point(2.0, 3.0);
        assert_eq!(spline.sample(&mut |_| -2.0), 0.0 + 2.0 * (-2.0 - -1.0));
        assert_eq!(spline.sample(&mut |_| 2.0), 1.0 + 3.0 * (2.0 - 1.0));
    }

    #[test]
    fn range_covers_the_values_and_the_extensions() {
        let flat = two_point(0.0, 0.0).range();
        assert_eq!(flat, Interval::of(0.0, 1.0));
        // 有导数时范围要宽一些。
        let sloped = two_point(2.0, 3.0).range();
        assert!(sloped.min() <= 0.0 && sloped.max() >= 1.0);
    }

    #[test]
    fn map_coordinates_keeps_the_shape() {
        let spline = two_point(1.0, 1.0);
        let mapped = spline.map_coordinates(&mut |_| 7_usize);
        let Spline::Multipoint(multipoint) = &mapped else {
            panic!("还是多点样条");
        };
        assert_eq!(multipoint.coordinate, 7);
        assert_eq!(multipoint.locations, [-1.0, 1.0]);
    }
}
