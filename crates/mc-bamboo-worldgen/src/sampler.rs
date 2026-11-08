//! 官方 `DensitySampler`：把密度函数编译成采样器再求值。
//!
//! 官方每个采样器都有两个方法，这里合成一个，用 [`Path`] 选：
//!
//! * [`Path::Point`] 是 `sampleValue`，单点求值；
//! * [`Path::Volume`] 是 `sampleVolume` 在一个方块坐标上的结果。
//!
//! 两条路径不是同一串浮点运算：`mul` 的 `左边是 0 就直接给 0`、`max` 用 `Math.max` 还是
//! `右 > 左 ? 右 : 左`、噪声的梯度点积怎么结合，这几处两边不同，最后一位会差。
//! `interpolated` 在单点求值时读的是输入的 `sampleVolume`，所以两条都要有。
//!
//! 编译的决定（`min`/`max` 的短路阈值、常量边的特化、`pow` 的特例）和官方
//! `BinaryFunction.compileSampler` 这些地方逐条对应，因为它们会改变结果。

use std::{collections::HashMap, sync::Arc};

use mc_bamboo_blocks::LegacyRandom;

use crate::{
    density::{
        self, Axis, BinaryOp, DistanceMetric, Kind, Ref, RoundOp, TilingMode, UnaryOp, leaky_relu,
        squeeze,
    },
    math::{
        clamp, cube, floor_f32, java_floor_div, java_floor_mod, java_signum, lerp, lerp3, sqrt_f32,
        square,
    },
    noise::{BLENDED_NOISE_SEED, Noise, SimplexNoise},
    random::{PositionalFactory, Random},
};

/// 官方 `nether/temperature` 噪声的 ID，它走老的下界 fbm。
const NETHER_TEMPERATURE: &str = "minecraft:nether/temperature";
/// 官方 `nether/vegetation` 噪声的 ID。
const NETHER_VEGETATION: &str = "minecraft:nether/vegetation";

/// 求值路径：官方的 `sampleValue` 还是 `sampleVolume`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    Point,
    Volume,
}

/// `SamplerContext` 里 `cache` 用的那一格。
///
/// 两条路径各记一格。官方的 `CacheCell` 也是分开的两组字段（`valueKey`/`value` 是单点，
/// `volume`/`buffer` 是成批），不能混用：两条路径的结果最后一位可能不同。
#[derive(Debug, Clone, Copy)]
struct CacheCell {
    point: Option<((i32, i32, i32), f32)>,
    volume: Option<((i32, i32, i32), f32)>,
}

/// 一次求值的上下文：只有 `cache` 的记忆。
///
/// 官方的键是 `BlockPos.asLong`，坐标超出 ±33554432 或者 y 超出 ±2048 时会撞车；
/// 这里用三个坐标本身当键，撞不了。官方不带缓存的那条路径（`sampleBlockValueUncached`）
/// 算出来的也是这个值，`cache` 本来就是纯函数的记忆。
#[derive(Debug, Default)]
pub struct Context {
    cells: Vec<CacheCell>,
}

/// 空的那一格。
const EMPTY_CELL: CacheCell = CacheCell {
    point: None,
    volume: None,
};

impl Context {
    pub fn new(cache_count: usize) -> Self {
        Self {
            cells: vec![EMPTY_CELL; cache_count],
        }
    }

    /// `SamplerContext.getCacheCell`：格子不够就长出来。
    fn cell(&mut self, id: usize) -> &mut CacheCell {
        if id >= self.cells.len() {
            self.cells.resize(id + 1, EMPTY_CELL);
        }
        &mut self.cells[id]
    }

    /// 清掉记忆，换一个种子或者重新开始时用。
    pub fn reset(&mut self) {
        self.cells.fill(EMPTY_CELL);
    }
}

/// 官方的 `DensitySampler`，一个变体对应官方一个采样器类。
#[derive(Debug)]
pub enum Sampler {
    /// `ConstantFunction.Sampler`，也包括 `blend_alpha`、`blend_offset` 和 `beardifier`
    /// 在没有上下文字段时的那个后备值。
    Constant(f32),
    /// `NoiseFunction.Sampler`。
    Noise {
        noise: Arc<Noise>,
        xz_scale: f64,
        y_scale: f64,
    },
    /// `NoiseFunction.ShiftedXzSampler`。
    ShiftedXz {
        shift_x: Box<Sampler>,
        shift_z: Box<Sampler>,
        noise: Arc<Noise>,
        xz_scale: f64,
        y_scale: f64,
    },
    /// `NoiseFunction.ShiftedXyzSampler`。
    ShiftedXyz {
        shift_x: Box<Sampler>,
        shift_y: Box<Sampler>,
        shift_z: Box<Sampler>,
        noise: Arc<Noise>,
        xz_scale: f64,
        y_scale: f64,
    },
    /// `ShiftNoiseFunction.ShiftB` 里那个匿名采样器。
    ShiftB {
        noise: Arc<Noise>,
    },
    /// `EndIslandFunction.Sampler`。
    EndIsland {
        noise: Arc<SimplexNoise>,
    },
    /// `DistanceToPointFunction.Sampler`。
    DistanceToPoint {
        point: [i32; 3],
        metric: DistanceMetric,
    },
    /// `GradientFunction.ClampedSampler`。
    GradientClamped {
        axis: Axis,
        from_coordinate: i32,
        min_coordinate: i32,
        max_coordinate: i32,
        from_value: f32,
        coordinate_factor: f32,
    },
    /// `GradientFunction.RepeatSampler`。
    GradientRepeat {
        axis: Axis,
        from_coordinate: i32,
        coordinate_range: i32,
        from_value: f32,
        coordinate_factor: f32,
    },
    /// `GradientFunction.MirroredRepeatSampler`。
    GradientMirroredRepeat {
        axis: Axis,
        from_coordinate: i32,
        coordinate_range: i32,
        from_value: f32,
        coordinate_factor: f32,
    },
    /// `UnaryFunction` 的那一串采样器，`LeakyReLUSampler` 的系数放在 `factor` 里。
    Unary {
        op: UnaryOp,
        factor: f32,
        input: Box<Sampler>,
    },
    /// `RoundFunction.IntegerMultipleSampler`。
    RoundInteger {
        op: RoundOp,
        input: Box<Sampler>,
    },
    /// `RoundFunction.Sampler`。
    Round {
        op: RoundOp,
        input: Box<Sampler>,
        multiple: Box<Sampler>,
    },
    /// `BinaryFunction.AddSampler` 等等，常量那几个单列。
    Add {
        left: Box<Sampler>,
        right: Box<Sampler>,
    },
    ConstAdd {
        left: Box<Sampler>,
        right: f32,
    },
    Sub {
        left: Box<Sampler>,
        right: Box<Sampler>,
    },
    ConstSub {
        left: f32,
        right: Box<Sampler>,
    },
    Mul {
        left: Box<Sampler>,
        right: Box<Sampler>,
    },
    ConstMul {
        left: Box<Sampler>,
        right: f32,
    },
    Div {
        left: Box<Sampler>,
        right: Box<Sampler>,
    },
    ConstDiv {
        left: f32,
        right: Box<Sampler>,
    },
    /// `BinaryFunction.MinSampler`：`right_min` 是右边范围的下界，用来短路。
    Min {
        left: Box<Sampler>,
        right: Box<Sampler>,
        right_min: f32,
    },
    ConstMin {
        left: Box<Sampler>,
        right: f32,
    },
    /// `BinaryFunction.MaxSampler`。
    Max {
        left: Box<Sampler>,
        right: Box<Sampler>,
        right_max: f32,
    },
    ConstMax {
        left: Box<Sampler>,
        right: f32,
    },
    /// `PowFunction.ConstBaseSampler`。
    PowConstBase {
        base: f64,
        exponent: Box<Sampler>,
    },
    /// `PowFunction.ConstExponentSampler`。
    PowConstExponent {
        base: Box<Sampler>,
        exponent: f64,
    },
    /// `PowFunction.Sampler`。
    Pow {
        base: Box<Sampler>,
        exponent: Box<Sampler>,
    },
    /// `SplineFunction.Sampler`：坐标按下标缓存，和官方的 `PointSplineInput` 一样。
    Spline {
        spline: crate::spline::Spline<usize>,
        coordinates: Vec<Sampler>,
    },
    /// `LerpFunction.ConstFirstSampler`。
    LerpConstFirst {
        alpha: Box<Sampler>,
        first: f32,
        second: Box<Sampler>,
    },
    /// `LerpFunction.ConstSecondSampler`。
    LerpConstSecond {
        alpha: Box<Sampler>,
        first: Box<Sampler>,
        second: f32,
    },
    /// `LerpFunction.Sampler`。
    Lerp {
        alpha: Box<Sampler>,
        first: Box<Sampler>,
        second: Box<Sampler>,
    },
    /// `ClampFunction.Sampler`。
    Clamp {
        input: Box<Sampler>,
        min: f32,
        max: f32,
    },
    /// `RangeChoiceFunction.ConstSampler`。
    RangeChoiceConst {
        input: Box<Sampler>,
        min_inclusive: f32,
        max_exclusive: f32,
        when_in_range: f32,
        when_out_of_range: f32,
    },
    /// `RangeChoiceFunction.Sampler`。
    RangeChoice {
        input: Box<Sampler>,
        min_inclusive: f32,
        max_exclusive: f32,
        when_in_range: Box<Sampler>,
        when_out_of_range: Box<Sampler>,
    },
    /// `IntervalSelectFunction.SingleThresholdSampler`。
    IntervalSelectSingle {
        input: Box<Sampler>,
        threshold: f32,
        if_below: Box<Sampler>,
        if_above: Box<Sampler>,
    },
    /// `IntervalSelectFunction.Sampler`。
    IntervalSelect {
        input: Box<Sampler>,
        thresholds: Vec<f32>,
        samplers: Vec<Sampler>,
    },
    /// `CachingDensitySampler`。
    Cached {
        id: usize,
        input: Arc<Sampler>,
    },
    /// `BlendDensityFunction.Sampler`：上下文里没有 `Blender`，原样返回。
    BlendDensity(Box<Sampler>),
    /// `InterpolatedFunction.Sampler`。
    Interpolated {
        input: Box<Sampler>,
        cell_size_xz: i32,
        cell_size_y: i32,
    },
    /// `SliceFunction.XSampler`。
    SliceX {
        input: Box<Sampler>,
        x: i32,
    },
    /// `SliceFunction.YSampler`。
    SliceY {
        input: Box<Sampler>,
        y: i32,
    },
    /// `SliceFunction.ZSampler`。
    SliceZ {
        input: Box<Sampler>,
        z: i32,
    },
    /// `SliceFunction.XzSampler`。
    SliceXz {
        input: Box<Sampler>,
        x: i32,
        z: i32,
    },
    /// `FindTopSurfaceFunction.Sampler`，外面还套着一层 `SliceFunction.YSampler`。
    FindTopSurface {
        density: Box<Sampler>,
        upper_bound: Box<Sampler>,
        lower_bound: i32,
        cell_height: i32,
    },
}

impl Sampler {
    /// 官方的 `sampleValue` / `sampleVolume`，看 `path`。
    pub fn sample(&self, context: &mut Context, path: Path, x: i32, y: i32, z: i32) -> f32 {
        match self {
            Self::Constant(value) => *value,
            Self::Noise {
                noise,
                xz_scale,
                y_scale,
            } => match path {
                Path::Point => noise.get(
                    f64::from(x) * xz_scale,
                    f64::from(y) * y_scale,
                    f64::from(z) * xz_scale,
                ),
                // 官方先把缓冲清成 0 再 addToVolume，振幅是 1。
                Path::Volume => noise.add_to_point((x, y, z), *xz_scale, *y_scale, 1.0),
            },
            Self::ShiftedXz {
                shift_x,
                shift_z,
                noise,
                xz_scale,
                y_scale,
            } => {
                let noise_x =
                    f64::from(x) * xz_scale + f64::from(shift_x.sample(context, path, x, y, z));
                let noise_y = f64::from(y) * y_scale;
                let noise_z =
                    f64::from(z) * xz_scale + f64::from(shift_z.sample(context, path, x, y, z));
                noise.get(noise_x, noise_y, noise_z)
            }
            Self::ShiftedXyz {
                shift_x,
                shift_y,
                shift_z,
                noise,
                xz_scale,
                y_scale,
            } => {
                let noise_x =
                    f64::from(x) * xz_scale + f64::from(shift_x.sample(context, path, x, y, z));
                let noise_y =
                    f64::from(y) * y_scale + f64::from(shift_y.sample(context, path, x, y, z));
                let noise_z =
                    f64::from(z) * xz_scale + f64::from(shift_z.sample(context, path, x, y, z));
                noise.get(noise_x, noise_y, noise_z)
            }
            Self::ShiftB { noise } => match path {
                Path::Point => noise.get(f64::from(z) * 0.25, f64::from(x) * 0.25, 0.0) * 4.0,
                // 官方把体积转置过来，x 走原来的 z，y 走原来的 x，z 固定是 0，振幅 4 在噪声里面乘。
                Path::Volume => noise.add_to_point((z, x, 0), 0.25, 0.25, 4.0),
            },
            Self::EndIsland { noise } => (end_island_height(noise, x / 8, z / 8) - 8.0) / 128.0,
            Self::DistanceToPoint { point, metric } => metric.compute(
                point[0].wrapping_sub(x) as f32,
                point[1].wrapping_sub(y) as f32,
                point[2].wrapping_sub(z) as f32,
            ),
            Self::GradientClamped {
                axis,
                from_coordinate,
                min_coordinate,
                max_coordinate,
                from_value,
                coordinate_factor,
            } => {
                let coordinate = axis.choose(x, y, z);
                let relative = coordinate
                    .clamp(*min_coordinate, *max_coordinate)
                    .wrapping_sub(*from_coordinate);
                from_value + relative as f32 * coordinate_factor
            }
            Self::GradientRepeat {
                axis,
                from_coordinate,
                coordinate_range,
                from_value,
                coordinate_factor,
            } => {
                let relative = axis.choose(x, y, z).wrapping_sub(*from_coordinate);
                from_value + java_floor_mod(relative, *coordinate_range) as f32 * coordinate_factor
            }
            Self::GradientMirroredRepeat {
                axis,
                from_coordinate,
                coordinate_range,
                from_value,
                coordinate_factor,
            } => {
                let relative = axis.choose(x, y, z).wrapping_sub(*from_coordinate);
                let tile = java_floor_div(relative, *coordinate_range);
                let local = relative.wrapping_sub(tile.wrapping_mul(*coordinate_range));
                if tile & 1 == 0 {
                    from_value + local as f32 * coordinate_factor
                } else {
                    from_value + coordinate_range.wrapping_sub(local) as f32 * coordinate_factor
                }
            }
            Self::Unary { op, factor, input } => {
                let value = input.sample(context, path, x, y, z);
                apply_unary(*op, *factor, value)
            }
            Self::RoundInteger { op, input } => op.apply(input.sample(context, path, x, y, z)),
            Self::Round {
                op,
                input,
                multiple,
            } => {
                let value = input.sample(context, path, x, y, z);
                let multiple = multiple.sample(context, path, x, y, z);
                if multiple == 0.0 {
                    value
                } else {
                    op.apply(value / multiple) * multiple
                }
            }
            Self::Add { left, right } => {
                left.sample(context, path, x, y, z) + right.sample(context, path, x, y, z)
            }
            Self::ConstAdd { left, right } => left.sample(context, path, x, y, z) + right,
            Self::Sub { left, right } => {
                left.sample(context, path, x, y, z) - right.sample(context, path, x, y, z)
            }
            Self::ConstSub { left, right } => left - right.sample(context, path, x, y, z),
            Self::Mul { left, right } => {
                let value = left.sample(context, path, x, y, z);
                // 单点求值时左边是 0 就不看右边，成批求值时照乘。
                if path == Path::Point && value == 0.0 {
                    0.0
                } else {
                    value * right.sample(context, path, x, y, z)
                }
            }
            Self::ConstMul { left, right } => left.sample(context, path, x, y, z) * right,
            Self::Div { left, right } => {
                let value = left.sample(context, path, x, y, z);
                if path == Path::Point && value == 0.0 {
                    0.0
                } else {
                    value / right.sample(context, path, x, y, z)
                }
            }
            Self::ConstDiv { left, right } => left / right.sample(context, path, x, y, z),
            Self::Min {
                left,
                right,
                right_min,
            } => {
                let value = left.sample(context, path, x, y, z);
                match path {
                    Path::Point => {
                        if value <= *right_min {
                            value
                        } else {
                            density::min(value, right.sample(context, path, x, y, z))
                        }
                    }
                    Path::Volume => {
                        let other = right.sample(context, path, x, y, z);
                        if other < value { other } else { value }
                    }
                }
            }
            Self::ConstMin { left, right } => {
                let value = left.sample(context, path, x, y, z);
                match path {
                    Path::Point => density::min(value, *right),
                    Path::Volume => {
                        if *right < value {
                            *right
                        } else {
                            value
                        }
                    }
                }
            }
            Self::Max {
                left,
                right,
                right_max,
            } => {
                let value = left.sample(context, path, x, y, z);
                match path {
                    Path::Point => {
                        if value >= *right_max {
                            value
                        } else {
                            density::max(value, right.sample(context, path, x, y, z))
                        }
                    }
                    Path::Volume => {
                        let other = right.sample(context, path, x, y, z);
                        if other > value { other } else { value }
                    }
                }
            }
            Self::ConstMax { left, right } => {
                let value = left.sample(context, path, x, y, z);
                match path {
                    Path::Point => density::max(value, *right),
                    Path::Volume => {
                        if *right > value {
                            *right
                        } else {
                            value
                        }
                    }
                }
            }
            Self::PowConstBase { base, exponent } => {
                let exponent = f64::from(exponent.sample(context, path, x, y, z));
                base.powf(exponent) as f32
            }
            Self::PowConstExponent { base, exponent } => {
                let base = f64::from(base.sample(context, path, x, y, z));
                base.powf(*exponent) as f32
            }
            Self::Pow { base, exponent } => {
                let base = f64::from(base.sample(context, path, x, y, z));
                let exponent = f64::from(exponent.sample(context, path, x, y, z));
                base.powf(exponent) as f32
            }
            Self::Spline {
                spline,
                coordinates,
            } => {
                let mut cached = vec![f32::NAN; coordinates.len()];
                spline.sample(&mut |index: &usize| {
                    if !cached[*index].is_nan() {
                        return cached[*index];
                    }
                    let value = coordinates[*index].sample(context, path, x, y, z);
                    cached[*index] = value;
                    value
                })
            }
            Self::LerpConstFirst {
                alpha,
                first,
                second,
            } => {
                let alpha = alpha.sample(context, path, x, y, z);
                if alpha == 0.0 {
                    *first
                } else if alpha == 1.0 {
                    second.sample(context, path, x, y, z)
                } else {
                    lerp(alpha, *first, second.sample(context, path, x, y, z))
                }
            }
            Self::LerpConstSecond {
                alpha,
                first,
                second,
            } => {
                let alpha = alpha.sample(context, path, x, y, z);
                if alpha == 0.0 {
                    first.sample(context, path, x, y, z)
                } else if alpha == 1.0 {
                    *second
                } else {
                    lerp(alpha, first.sample(context, path, x, y, z), *second)
                }
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                let alpha = alpha.sample(context, path, x, y, z);
                if alpha == 0.0 {
                    first.sample(context, path, x, y, z)
                } else if alpha == 1.0 {
                    second.sample(context, path, x, y, z)
                } else {
                    lerp(
                        alpha,
                        first.sample(context, path, x, y, z),
                        second.sample(context, path, x, y, z),
                    )
                }
            }
            Self::Clamp { input, min, max } => {
                clamp(input.sample(context, path, x, y, z), *min, *max)
            }
            Self::RangeChoiceConst {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let value = input.sample(context, path, x, y, z);
                if value >= *min_inclusive && value < *max_exclusive {
                    *when_in_range
                } else {
                    *when_out_of_range
                }
            }
            Self::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let value = input.sample(context, path, x, y, z);
                if value >= *min_inclusive && value < *max_exclusive {
                    when_in_range.sample(context, path, x, y, z)
                } else {
                    when_out_of_range.sample(context, path, x, y, z)
                }
            }
            Self::IntervalSelectSingle {
                input,
                threshold,
                if_below,
                if_above,
            } => {
                let value = input.sample(context, path, x, y, z);
                if value < *threshold {
                    if_below.sample(context, path, x, y, z)
                } else {
                    if_above.sample(context, path, x, y, z)
                }
            }
            Self::IntervalSelect {
                input,
                thresholds,
                samplers,
            } => {
                let value = input.sample(context, path, x, y, z);
                let index = thresholds
                    .iter()
                    .position(|threshold| value < *threshold)
                    .unwrap_or(samplers.len() - 1);
                samplers[index].sample(context, path, x, y, z)
            }
            Self::Cached { id, input } => {
                let key = (x, y, z);
                let cell = *context.cell(*id);
                let remembered = match path {
                    Path::Point => cell.point,
                    Path::Volume => cell.volume,
                };
                if let Some((cached_key, value)) = remembered
                    && cached_key == key
                    && !value.is_nan()
                {
                    return value;
                }
                let value = input.sample(context, path, x, y, z);
                let cell = context.cell(*id);
                match path {
                    Path::Point => cell.point = Some((key, value)),
                    Path::Volume => cell.volume = Some((key, value)),
                }
                value
            }
            Self::BlendDensity(input) => input.sample(context, path, x, y, z),
            Self::Interpolated {
                input,
                cell_size_xz,
                cell_size_y,
            } => {
                let x_in_cell = java_floor_mod(x, *cell_size_xz);
                let y_in_cell = java_floor_mod(y, *cell_size_y);
                let z_in_cell = java_floor_mod(z, *cell_size_xz);
                if x_in_cell == 0 && y_in_cell == 0 && z_in_cell == 0 {
                    return input.sample(context, path, x, y, z);
                }
                let base = (
                    x.wrapping_sub(x_in_cell),
                    y.wrapping_sub(y_in_cell),
                    z.wrapping_sub(z_in_cell),
                );
                // 官方在这里对 2×2×2 的格子调 sampleVolume，所以内层走成批路径。
                let mut corner = |dx: i32, dy: i32, dz: i32| {
                    input.sample(
                        context,
                        Path::Volume,
                        base.0.wrapping_add(dx.wrapping_mul(*cell_size_xz)),
                        base.1.wrapping_add(dy.wrapping_mul(*cell_size_y)),
                        base.2.wrapping_add(dz.wrapping_mul(*cell_size_xz)),
                    )
                };
                let values = [
                    corner(0, 0, 0),
                    corner(1, 0, 0),
                    corner(0, 1, 0),
                    corner(1, 1, 0),
                    corner(0, 0, 1),
                    corner(1, 0, 1),
                    corner(0, 1, 1),
                    corner(1, 1, 1),
                ];
                lerp3(
                    (
                        x_in_cell as f32 / *cell_size_xz as f32,
                        y_in_cell as f32 / *cell_size_y as f32,
                        z_in_cell as f32 / *cell_size_xz as f32,
                    ),
                    values,
                )
            }
            Self::SliceX { input, x: fixed } => input.sample(context, path, *fixed, y, z),
            Self::SliceY { input, y: fixed } => input.sample(context, path, x, *fixed, z),
            Self::SliceZ { input, z: fixed } => input.sample(context, path, x, y, *fixed),
            Self::SliceXz {
                input,
                x: fixed_x,
                z: fixed_z,
            } => input.sample(context, path, *fixed_x, y, *fixed_z),
            Self::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                let upper = upper_bound.sample(context, path, x, y, z);
                // 官方的这一层不管哪条路径，探测密度时都走单点求值。
                find_surface_from(density, context, x, z, upper, *lower_bound, *cell_height)
            }
        }
    }
}

/// `UnaryFunction` 的那一串采样器。
fn apply_unary(op: UnaryOp, factor: f32, value: f32) -> f32 {
    match op {
        UnaryOp::Abs => value.abs(),
        UnaryOp::Square => square(value),
        UnaryOp::Cube => cube(value),
        UnaryOp::Sqrt => sqrt_f32(value),
        UnaryOp::HalfNegative | UnaryOp::QuarterNegative => leaky_relu(factor, value),
        UnaryOp::Reciprocal => 1.0 / value,
        UnaryOp::Negate => -value,
        UnaryOp::Squeeze => squeeze(value),
        UnaryOp::Log => f64::from(value).ln() as f32,
        UnaryOp::Sign => java_signum(value),
    }
}

/// `FindTopSurfaceFunction.Sampler.findSurfaceFrom`。
fn find_surface_from(
    density: &Sampler,
    context: &mut Context,
    x: i32,
    z: i32,
    upper_bound: f32,
    lower_bound: i32,
    cell_height: i32,
) -> f32 {
    let top_y = floor_f32(upper_bound / cell_height as f32).wrapping_mul(cell_height);
    if top_y <= lower_bound {
        return lower_bound as f32;
    }
    let mut probe_y = top_y;
    while probe_y >= lower_bound {
        if density.sample(context, Path::Point, x, probe_y, z) > 0.0 {
            return probe_y as f32;
        }
        // 官方是 int 减法。`upper_bound` 是 NaN 或者无穷时 `top_y` 会饱和到边界，
        // 这里用 wrapping 跟着官方溢出，而不是在 debug 下 panic。
        probe_y = probe_y.wrapping_sub(cell_height);
    }
    lower_bound as f32
}

/// `EndIslandFunction.getHeightValue`。
fn end_island_height(noise: &SimplexNoise, section_x: i32, section_z: i32) -> f32 {
    let chunk_x = section_x / 2;
    let chunk_z = section_z / 2;
    let sub_section_x = section_x % 2;
    let sub_section_z = section_z % 2;
    let mut doffs = -100.0_f32;
    for xo in -12..=12_i32 {
        for zo in -12..=12_i32 {
            let total_chunk_x = i64::from(chunk_x.wrapping_add(xo));
            let total_chunk_z = i64::from(chunk_z.wrapping_add(zo));
            if total_chunk_x * total_chunk_x + total_chunk_z * total_chunk_z > 4096
                && noise.get2(total_chunk_x as f64, total_chunk_z as f64) < -0.9
            {
                let island_size = ((total_chunk_x as f32).abs() * 3439.0
                    + (total_chunk_z as f32).abs() * 147.0)
                    % 13.0
                    + 9.0;
                let xd = (sub_section_x - xo * 2) as f32;
                let zd = (sub_section_z - zo * 2) as f32;
                let new_doffs = 100.0 - sqrt_f32(xd * xd + zd * zd) * island_size;
                let new_doffs = clamp(new_doffs, -100.0, 80.0);
                doffs = density::max(doffs, new_doffs);
            }
        }
    }
    doffs
}

/// 官方 `RandomState` 加 `DensityFunctionCompiler`：给一个种子把密度函数编译成采样器。
#[derive(Debug)]
pub struct Compiler {
    seed: i64,
    use_legacy_random: bool,
    random: PositionalFactory,
    noises: HashMap<String, Arc<Noise>>,
    /// 下界温度和植被的老 fbm，和上面那张表分开：同一个 ID 两条路造出来的噪声不一样。
    legacy_noises: HashMap<String, Arc<Noise>>,
    island_noise: Option<Arc<SimplexNoise>>,
    caches: HashMap<usize, (usize, Arc<Sampler>)>,
    next_cache_id: usize,
}

impl Compiler {
    /// `RandomState.create(noises, seed, settings)`。
    pub fn new(seed: i64, use_legacy_random: bool) -> Self {
        let algorithm = if use_legacy_random {
            crate::random::Algorithm::Legacy
        } else {
            crate::random::Algorithm::Xoroshiro
        };
        Self {
            seed,
            use_legacy_random,
            random: algorithm.new_instance(seed).fork_positional(),
            noises: HashMap::new(),
            legacy_noises: HashMap::new(),
            island_noise: None,
            caches: HashMap::new(),
            next_cache_id: 0,
        }
    }

    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// `RandomState.random`，表面规则和地物以后要用。
    pub fn positional_factory(&self) -> PositionalFactory {
        self.random
    }

    /// `RandomState.getOrCreateRandomFactory(name)`。
    pub fn random_factory(&self, name: &str) -> PositionalFactory {
        self.random.from_hash_of(name).fork_positional()
    }

    /// 编译好之后要开多少个 `cache` 格子。
    pub fn cache_count(&self) -> usize {
        self.next_cache_id
    }

    /// `DensityFunctionCompiler.getSampler(function)`。
    ///
    /// 官方在编译前还跑一遍 `DfRewriteRule.SLICE_UNIFORM_AXES`，把用不到某个轴的子树
    /// 包进 `slice` 里。那一步只改哪些坐标被传下去，不改结果（`domainAxes` 是保守的），
    /// 单点求值时可以不做，详见 crate 文档。
    pub fn compile(&mut self, function: &Ref) -> Sampler {
        match &function.kind {
            Kind::Constant(value) => Sampler::Constant(*value),
            // 上下文里没有 Blender 和 Beardifier，取官方的后备常量。
            Kind::BlendAlpha => Sampler::Constant(1.0),
            Kind::BlendOffset | Kind::Beardifier => Sampler::Constant(0.0),
            Kind::Noise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                let instance = self.noise_sampler(&noise.id, &noise.noise);
                let is_zero = |function: &Ref| function.constant() == Some(0.0);
                if is_zero(shift_x) && is_zero(shift_y) && is_zero(shift_z) {
                    return Sampler::Noise {
                        noise: instance,
                        xz_scale: *xz_scale,
                        y_scale: *y_scale,
                    };
                }
                let compiled_x = Box::new(self.compile(shift_x));
                let compiled_z = Box::new(self.compile(shift_z));
                if is_zero(shift_y) {
                    return Sampler::ShiftedXz {
                        shift_x: compiled_x,
                        shift_z: compiled_z,
                        noise: instance,
                        xz_scale: *xz_scale,
                        y_scale: *y_scale,
                    };
                }
                Sampler::ShiftedXyz {
                    shift_x: compiled_x,
                    shift_y: Box::new(self.compile(shift_y)),
                    shift_z: compiled_z,
                    noise: instance,
                    xz_scale: *xz_scale,
                    y_scale: *y_scale,
                }
            }
            Kind::ShiftA(noise) => Sampler::ConstMul {
                left: Box::new(Sampler::Noise {
                    noise: self.noise_sampler(&noise.id, &noise.noise),
                    xz_scale: 0.25,
                    y_scale: 0.0,
                }),
                right: 4.0,
            },
            Kind::Shift(noise) => Sampler::ConstMul {
                left: Box::new(Sampler::Noise {
                    noise: self.noise_sampler(&noise.id, &noise.noise),
                    xz_scale: 0.25,
                    y_scale: 0.25,
                }),
                right: 4.0,
            },
            Kind::ShiftB(noise) => Sampler::ShiftB {
                noise: self.noise_sampler(&noise.id, &noise.noise),
            },
            Kind::EndOuterIslands => Sampler::EndIsland {
                noise: self.end_island_noise(),
            },
            Kind::DistanceToPoint { point, metric } => Sampler::DistanceToPoint {
                point: *point,
                metric: *metric,
            },
            Kind::Gradient {
                axis,
                tiling,
                from_coordinate,
                to_coordinate,
                from_value,
                to_value,
            } => {
                let coordinate_range = to_coordinate.wrapping_sub(*from_coordinate);
                let coordinate_factor = (to_value - from_value) / coordinate_range as f32;
                match tiling {
                    TilingMode::ClampToEdge => Sampler::GradientClamped {
                        axis: *axis,
                        from_coordinate: *from_coordinate,
                        min_coordinate: (*from_coordinate).min(*to_coordinate),
                        max_coordinate: (*from_coordinate).max(*to_coordinate),
                        from_value: *from_value,
                        coordinate_factor,
                    },
                    TilingMode::Repeat => Sampler::GradientRepeat {
                        axis: *axis,
                        from_coordinate: *from_coordinate,
                        coordinate_range,
                        from_value: *from_value,
                        coordinate_factor,
                    },
                    TilingMode::MirroredRepeat => Sampler::GradientMirroredRepeat {
                        axis: *axis,
                        from_coordinate: *from_coordinate,
                        coordinate_range,
                        from_value: *from_value,
                        coordinate_factor,
                    },
                }
            }
            Kind::Unary { op, input } => Sampler::Unary {
                op: *op,
                factor: match op {
                    UnaryOp::HalfNegative => 0.5,
                    UnaryOp::QuarterNegative => 0.25,
                    _ => 0.0,
                },
                input: Box::new(self.compile(input)),
            },
            Kind::Round {
                op,
                input,
                multiple,
            } => {
                let compiled = Box::new(self.compile(input));
                if multiple.constant() == Some(1.0) {
                    Sampler::RoundInteger {
                        op: *op,
                        input: compiled,
                    }
                } else {
                    Sampler::Round {
                        op: *op,
                        input: compiled,
                        multiple: Box::new(self.compile(multiple)),
                    }
                }
            }
            Kind::Binary { op, left, right } => self.compile_binary(*op, left, right),
            Kind::Pow { base, exponent } => {
                let compiled_base = self.compile(base);
                let compiled_exponent = Box::new(self.compile(exponent));
                match (base.constant(), exponent.constant()) {
                    (Some(value), _) => Sampler::PowConstBase {
                        base: f64::from(value),
                        exponent: compiled_exponent,
                    },
                    (None, Some(value)) => compile_const_exponent(compiled_base, value),
                    (None, None) => Sampler::Pow {
                        base: Box::new(compiled_base),
                        exponent: compiled_exponent,
                    },
                }
            }
            Kind::Spline(spline) => {
                let mut coordinates: Vec<Ref> = Vec::new();
                let indexed = spline.map_coordinates(&mut |coordinate: &Ref| match coordinates
                    .iter()
                    .position(|found| Arc::ptr_eq(found, coordinate))
                {
                    Some(index) => index,
                    None => {
                        coordinates.push(Arc::clone(coordinate));
                        coordinates.len() - 1
                    }
                });
                let compiled = coordinates
                    .iter()
                    .map(|coordinate| self.compile(coordinate))
                    .collect();
                Sampler::Spline {
                    spline: indexed,
                    coordinates: compiled,
                }
            }
            Kind::Lerp {
                alpha,
                first,
                second,
            } => {
                let compiled_alpha = Box::new(self.compile(alpha));
                let compiled_first = Box::new(self.compile(first));
                let compiled_second = Box::new(self.compile(second));
                match (first.constant(), second.constant()) {
                    (Some(value), _) => Sampler::LerpConstFirst {
                        alpha: compiled_alpha,
                        first: value,
                        second: compiled_second,
                    },
                    (None, Some(value)) => Sampler::LerpConstSecond {
                        alpha: compiled_alpha,
                        first: compiled_first,
                        second: value,
                    },
                    (None, None) => Sampler::Lerp {
                        alpha: compiled_alpha,
                        first: compiled_first,
                        second: compiled_second,
                    },
                }
            }
            Kind::Clamp { input, min, max } => Sampler::Clamp {
                input: Box::new(self.compile(input)),
                min: *min,
                max: *max,
            },
            Kind::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let compiled_input = Box::new(self.compile(input));
                match (when_in_range.constant(), when_out_of_range.constant()) {
                    (Some(inside), Some(outside)) => Sampler::RangeChoiceConst {
                        input: compiled_input,
                        min_inclusive: *min_inclusive,
                        max_exclusive: *max_exclusive,
                        when_in_range: inside,
                        when_out_of_range: outside,
                    },
                    _ => Sampler::RangeChoice {
                        input: compiled_input,
                        min_inclusive: *min_inclusive,
                        max_exclusive: *max_exclusive,
                        when_in_range: Box::new(self.compile(when_in_range)),
                        when_out_of_range: Box::new(self.compile(when_out_of_range)),
                    },
                }
            }
            Kind::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let compiled_input = Box::new(self.compile(input));
                if thresholds.len() == 1 {
                    return Sampler::IntervalSelectSingle {
                        input: compiled_input,
                        threshold: thresholds[0],
                        if_below: Box::new(self.compile(&functions[0])),
                        if_above: Box::new(self.compile(functions.last().expect("至少两个函数"))),
                    };
                }
                let samplers = functions
                    .iter()
                    .map(|function| self.compile(function))
                    .collect();
                Sampler::IntervalSelect {
                    input: compiled_input,
                    thresholds: thresholds.clone(),
                    samplers,
                }
            }
            Kind::Cache(input) => self.compile_cache(input),
            Kind::BlendDensity(input) => Sampler::BlendDensity(Box::new(self.compile(input))),
            Kind::Interpolated {
                input,
                cell_size_xz,
                cell_size_y,
            } => Sampler::Interpolated {
                input: Box::new(self.compile(input)),
                cell_size_xz: *cell_size_xz,
                cell_size_y: *cell_size_y,
            },
            Kind::Slice {
                axis,
                coordinate,
                input,
            } => self.compile_slice(*axis, *coordinate, input),
            Kind::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => Sampler::SliceY {
                input: Box::new(Sampler::FindTopSurface {
                    density: Box::new(self.compile(density)),
                    upper_bound: Box::new(self.compile(upper_bound)),
                    lower_bound: *lower_bound,
                    cell_height: *cell_height,
                }),
                y: 0,
            },
            Kind::OldBlendedNoise(parameters) => {
                let random = self.create_random(BLENDED_NOISE_SEED);
                let fbms = parameters.create_fbm_set(&random);
                let (xz, y, main_xz, main_y) = parameters.multipliers();
                let min_limit = Sampler::Noise {
                    noise: Arc::new(Noise::Stack(fbms.min_limit)),
                    xz_scale: xz,
                    y_scale: y,
                };
                let max_limit = Sampler::Noise {
                    noise: Arc::new(Noise::Stack(fbms.max_limit)),
                    xz_scale: xz,
                    y_scale: y,
                };
                let main = Sampler::Noise {
                    noise: Arc::new(Noise::Stack(fbms.main)),
                    xz_scale: main_xz,
                    y_scale: main_y,
                };
                let choice = Sampler::Clamp {
                    input: Box::new(Sampler::ConstAdd {
                        left: Box::new(main),
                        right: 0.5,
                    }),
                    min: 0.0,
                    max: 1.0,
                };
                Sampler::Lerp {
                    alpha: Box::new(choice),
                    first: Box::new(min_limit),
                    second: Box::new(max_limit),
                }
            }
        }
    }

    /// `BinaryFunction.compileSampler`。官方先把两边都编译出来，再按类型和范围挑采样器。
    fn compile_binary(&mut self, op: BinaryOp, left: &Ref, right: &Ref) -> Sampler {
        let compiled_left = Box::new(self.compile(left));
        let compiled_right = Box::new(self.compile(right));
        match op {
            BinaryOp::Add => match (left.constant(), right.constant()) {
                (Some(value), _) => Sampler::ConstAdd {
                    left: compiled_right,
                    right: value,
                },
                (None, Some(value)) => Sampler::ConstAdd {
                    left: compiled_left,
                    right: value,
                },
                (None, None) => Sampler::Add {
                    left: compiled_left,
                    right: compiled_right,
                },
            },
            BinaryOp::Sub => match (left.constant(), right.constant()) {
                (Some(value), _) => Sampler::ConstSub {
                    left: value,
                    right: compiled_right,
                },
                // 官方在这里转成加一个负数。
                (None, Some(value)) => Sampler::ConstAdd {
                    left: compiled_left,
                    right: -value,
                },
                (None, None) => Sampler::Sub {
                    left: compiled_left,
                    right: compiled_right,
                },
            },
            BinaryOp::Mul => match (left.constant(), right.constant()) {
                (Some(value), _) => Sampler::ConstMul {
                    left: compiled_right,
                    right: value,
                },
                (None, Some(value)) => Sampler::ConstMul {
                    left: compiled_left,
                    right: value,
                },
                (None, None) => Sampler::Mul {
                    left: compiled_left,
                    right: compiled_right,
                },
            },
            BinaryOp::Div => match (left.constant(), right.constant()) {
                (Some(value), _) => Sampler::ConstDiv {
                    left: value,
                    right: compiled_right,
                },
                // 官方在这里转成乘倒数。
                (None, Some(value)) => Sampler::ConstMul {
                    left: compiled_left,
                    right: 1.0 / value,
                },
                (None, None) => Sampler::Div {
                    left: compiled_left,
                    right: compiled_right,
                },
            },
            BinaryOp::Min => {
                let left_range = left.range();
                let right_range = right.range();
                // 两边的范围完全不相交时，官方整支丢掉另一边。
                if left_range.max() < right_range.min() {
                    return *compiled_left;
                }
                if right_range.max() < left_range.min() {
                    return *compiled_right;
                }
                match (left.constant(), right.constant()) {
                    (Some(value), _) => Sampler::ConstMin {
                        left: compiled_right,
                        right: value,
                    },
                    (None, Some(value)) => Sampler::ConstMin {
                        left: compiled_left,
                        right: value,
                    },
                    (None, None) => Sampler::Min {
                        left: compiled_left,
                        right: compiled_right,
                        right_min: right_range.min(),
                    },
                }
            }
            BinaryOp::Max => {
                let left_range = left.range();
                let right_range = right.range();
                if left_range.min() > right_range.max() {
                    return *compiled_left;
                }
                if right_range.min() > left_range.max() {
                    return *compiled_right;
                }
                match (left.constant(), right.constant()) {
                    (Some(value), _) => Sampler::ConstMax {
                        left: compiled_right,
                        right: value,
                    },
                    (None, Some(value)) => Sampler::ConstMax {
                        left: compiled_left,
                        right: value,
                    },
                    (None, None) => Sampler::Max {
                        left: compiled_left,
                        right: compiled_right,
                        right_max: right_range.max(),
                    },
                }
            }
        }
    }

    /// `SliceFunction.compileSampler`：x 套 z（或者反过来）时合成一个采样器。
    fn compile_slice(&mut self, axis: Axis, coordinate: i32, input: &Ref) -> Sampler {
        if let Kind::Slice {
            axis: inner_axis,
            coordinate: inner_coordinate,
            input: inner_input,
        } = &input.kind
        {
            let crossed = (axis == Axis::X && *inner_axis == Axis::Z)
                || (axis == Axis::Z && *inner_axis == Axis::X);
            if crossed {
                let (x, z) = if axis == Axis::X {
                    (coordinate, *inner_coordinate)
                } else {
                    (*inner_coordinate, coordinate)
                };
                return Sampler::SliceXz {
                    input: Box::new(self.compile(inner_input)),
                    x,
                    z,
                };
            }
        }
        let compiled = Box::new(self.compile(input));
        match axis {
            Axis::X => Sampler::SliceX {
                input: compiled,
                x: coordinate,
            },
            Axis::Y => Sampler::SliceY {
                input: compiled,
                y: coordinate,
            },
            Axis::Z => Sampler::SliceZ {
                input: compiled,
                z: coordinate,
            },
        }
    }

    /// `DensityFunctionCompiler.reuseOrPrepareCache`：同一个输入共用一个格子。
    fn compile_cache(&mut self, input: &Ref) -> Sampler {
        let key = Arc::as_ptr(input) as usize;
        if let Some((id, sampler)) = self.caches.get(&key) {
            return Sampler::Cached {
                id: *id,
                input: Arc::clone(sampler),
            };
        }
        let id = self.next_cache_id;
        self.next_cache_id += 1;
        let compiled = Arc::new(self.compile(input));
        self.caches.insert(key, (id, Arc::clone(&compiled)));
        Sampler::Cached {
            id,
            input: compiled,
        }
    }

    /// `DensityFunction.CompileContext.createNoiseSampler`。
    ///
    /// 只有这条路把下界的温度和植被噪声换成老的 fbm；`RandomState.getOrCreateNoise`
    /// 不管这两条，所以两边要分开，见 [`Self::get_or_create_noise`]。
    fn noise_sampler(
        &mut self,
        id: &str,
        parameters: &Arc<crate::noise::NormalNoise>,
    ) -> Arc<Noise> {
        let seed_offset = match id {
            NETHER_TEMPERATURE => 0,
            NETHER_VEGETATION => 1,
            _ => return self.get_or_create_noise(id, parameters),
        };
        if let Some(found) = self.legacy_noises.get(id) {
            return Arc::clone(found);
        }
        let random = self.legacy_instance(seed_offset);
        let instance = Arc::new(parameters.create_for_legacy_nether_biome(&random));
        self.legacy_noises
            .insert(id.to_owned(), Arc::clone(&instance));
        instance
    }

    /// `RandomState.getOrCreateNoise(key)`：一律走 `NormalNoise.create`。
    pub fn get_or_create_noise(
        &mut self,
        id: &str,
        parameters: &Arc<crate::noise::NormalNoise>,
    ) -> Arc<Noise> {
        if let Some(found) = self.noises.get(id) {
            return Arc::clone(found);
        }
        let instance = Arc::new(parameters.create(&self.random.from_hash_of(id)));
        self.noises.insert(id.to_owned(), Arc::clone(&instance));
        instance
    }

    /// `DensityFunction.CompileContext.createRandom`。
    fn create_random(&self, seed_name: &str) -> Random {
        if self.use_legacy_random && seed_name == BLENDED_NOISE_SEED {
            self.legacy_instance(0)
        } else {
            self.random.from_hash_of(seed_name)
        }
    }

    /// `newLegacyInstance(seedOffset)`。
    fn legacy_instance(&self, seed_offset: i64) -> Random {
        Random::Legacy(LegacyRandom::new(self.seed.wrapping_add(seed_offset)))
    }

    /// `EndIslandFunction.compileSampler`：先跳掉 17292 个数，再造一个不带偏移的单纯噪声。
    fn end_island_noise(&mut self) -> Arc<SimplexNoise> {
        if let Some(found) = &self.island_noise {
            return Arc::clone(found);
        }
        let random = Random::Legacy(LegacyRandom::new(self.seed));
        random.consume_count(17_292);
        let noise = Arc::new(SimplexNoise::new(&random, true));
        self.island_noise = Some(Arc::clone(&noise));
        noise
    }
}

/// `PowFunction.compileConstExponent`。
fn compile_const_exponent(base: Sampler, exponent: f32) -> Sampler {
    let abs = exponent.abs();
    let special = if abs == 0.5 {
        Sampler::Unary {
            op: UnaryOp::Sqrt,
            factor: 0.0,
            input: Box::new(base),
        }
    } else if abs == 1.0 {
        base
    } else if abs == 2.0 {
        Sampler::Unary {
            op: UnaryOp::Square,
            factor: 0.0,
            input: Box::new(base),
        }
    } else if abs == 3.0 {
        Sampler::Unary {
            op: UnaryOp::Cube,
            factor: 0.0,
            input: Box::new(base),
        }
    } else {
        return Sampler::PowConstExponent {
            base: Box::new(base),
            exponent: f64::from(exponent),
        };
    };
    if exponent >= 0.0 {
        special
    } else {
        Sampler::Unary {
            op: UnaryOp::Reciprocal,
            factor: 0.0,
            input: Box::new(special),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::density::{FunctionRegistry, NoiseRegistry};

    fn compile(noise: &str, functions: &str, id: &str) -> (Sampler, Context) {
        let noise: Value = serde_json::from_str(noise).expect("合法的 JSON");
        let functions: Value = serde_json::from_str(functions).expect("合法的 JSON");
        let noises = NoiseRegistry::load(noise.as_object().expect("对象")).expect("能解析");
        let registry =
            FunctionRegistry::load(functions.as_object().expect("对象"), &noises).expect("能解析");
        let mut compiler = Compiler::new(1, false);
        let sampler = compiler.compile(registry.get(id).expect("有这一条"));
        let context = Context::new(compiler.cache_count());
        (sampler, context)
    }

    #[test]
    fn gradient_clamps_at_both_ends() {
        let (sampler, mut context) = compile(
            "{}",
            r#"{"g": {"type": "minecraft:gradient", "axis": "y",
                "from_coordinate": 0, "to_coordinate": 100,
                "from_value": 0.0, "to_value": 1.0}}"#,
            "g",
        );
        assert_eq!(sampler.sample(&mut context, Path::Point, 0, -50, 0), 0.0);
        assert_eq!(sampler.sample(&mut context, Path::Point, 0, 50, 0), 0.5);
        assert_eq!(sampler.sample(&mut context, Path::Point, 0, 500, 0), 1.0);
    }

    /// 右边是常量时官方特化成 `ConstMulSampler`，两条路径一样，零也照乘出 -0.0。
    #[test]
    fn constant_multipliers_keep_the_sign_of_zero() {
        let (sampler, mut context) = compile(
            "{}",
            r#"{"m": {"type": "minecraft:mul",
                "left": {"type": "minecraft:gradient", "axis": "y",
                    "from_coordinate": 0, "to_coordinate": 1,
                    "from_value": 0.0, "to_value": 1.0},
                "right": -1.0}}"#,
            "m",
        );
        // 右边是常量，官方会特化成 ConstMulSampler，两条路径一样。
        assert_eq!(
            sampler.sample(&mut context, Path::Point, 0, 0, 0).to_bits(),
            (-0.0_f32).to_bits()
        );
    }

    /// 范围完全不相交时，官方把 `min` 整支丢掉。
    #[test]
    fn min_drops_the_branch_it_can_never_pick() {
        let (sampler, mut context) = compile(
            "{}",
            r#"{"m": {"type": "minecraft:min", "left": -5.0, "right": 5.0}}"#,
            "m",
        );
        assert!(matches!(sampler, Sampler::Constant(value) if value == -5.0));
        assert_eq!(sampler.sample(&mut context, Path::Point, 0, 0, 0), -5.0);
    }

    /// `cache` 记住上一次的位置，换了位置就重新算。
    #[test]
    fn cache_remembers_one_position() {
        let (sampler, mut context) = compile(
            "{}",
            r#"{"c": {"type": "minecraft:cache",
                "input": {"type": "minecraft:gradient", "axis": "y",
                    "from_coordinate": 0, "to_coordinate": 100,
                    "from_value": 0.0, "to_value": 1.0}}}"#,
            "c",
        );
        assert!(matches!(sampler, Sampler::Cached { id: 0, .. }));
        assert_eq!(sampler.sample(&mut context, Path::Point, 0, 50, 0), 0.5);
        assert_eq!(sampler.sample(&mut context, Path::Point, 0, 50, 0), 0.5);
        assert_eq!(sampler.sample(&mut context, Path::Point, 0, 100, 0), 1.0);
    }

    /// 同一个输入的两个 `cache` 共用一个格子，和官方 `preparedCaches` 一样。
    #[test]
    fn two_caches_over_the_same_input_share_a_cell() {
        let noise: Value = serde_json::from_str("{}").expect("合法的 JSON");
        let functions: Value = serde_json::from_str(
            r#"{
                "base": {"type": "minecraft:gradient", "axis": "y",
                    "from_coordinate": 0, "to_coordinate": 100,
                    "from_value": 0.0, "to_value": 1.0},
                "a": {"type": "minecraft:cache", "input": "base"},
                "b": {"type": "minecraft:cache", "input": "base"}
            }"#,
        )
        .expect("合法的 JSON");
        let noises = NoiseRegistry::load(noise.as_object().expect("对象")).expect("能解析");
        let registry =
            FunctionRegistry::load(functions.as_object().expect("对象"), &noises).expect("能解析");
        let mut compiler = Compiler::new(1, false);
        let _ = compiler.compile(registry.get("a").expect("有这一条"));
        let _ = compiler.compile(registry.get("b").expect("有这一条"));
        assert_eq!(compiler.cache_count(), 1);
    }

    /// `interpolated` 在格点上直接问输入，不插值。
    #[test]
    fn interpolated_is_exact_on_cell_corners() {
        let (sampler, mut context) = compile(
            "{}",
            r#"{"i": {"type": "minecraft:interpolated", "cell_size_xz": 4, "cell_size_y": 8,
                "input": {"type": "minecraft:gradient", "axis": "y",
                    "from_coordinate": 0, "to_coordinate": 64,
                    "from_value": 0.0, "to_value": 1.0}}}"#,
            "i",
        );
        assert_eq!(sampler.sample(&mut context, Path::Point, 0, 0, 0), 0.0);
        assert_eq!(sampler.sample(&mut context, Path::Point, 0, 32, 0), 0.5);
        // 格子中间按八个角插值，梯度是线性的所以结果一样。
        assert_eq!(sampler.sample(&mut context, Path::Point, 2, 36, 2), 0.5625);
    }

    /// `pow` 的常量指数走特例：0.5 变开方，-1 变倒数。
    #[test]
    fn constant_exponents_use_the_special_cases() {
        let (sqrt, mut context) = compile(
            "{}",
            r#"{"p": {"type": "minecraft:pow", "base": 9.0, "exponent": 0.5}}"#,
            "p",
        );
        // base 是常量，官方先看 base，所以这里是 ConstBase，而不是开方。
        assert_eq!(sqrt.sample(&mut context, Path::Point, 0, 0, 0), 3.0);

        let (reciprocal, mut context) = compile(
            "{}",
            r#"{"p": {"type": "minecraft:pow",
                "base": {"type": "minecraft:gradient", "axis": "y",
                    "from_coordinate": 0, "to_coordinate": 100,
                    "from_value": 1.0, "to_value": 5.0},
                "exponent": -1.0}}"#,
            "p",
        );
        assert!(matches!(
            reciprocal,
            Sampler::Unary {
                op: UnaryOp::Reciprocal,
                ..
            }
        ));
        assert_eq!(reciprocal.sample(&mut context, Path::Point, 0, 0, 0), 1.0);
    }
}
