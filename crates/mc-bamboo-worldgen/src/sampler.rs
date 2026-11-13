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
    volume::Volume,
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

/// `SamplerContext.CacheCell`：`cache` 用的那一格。
///
/// 单点求值记一个位置和值（`valueKey`/`value`），成批求值记最后一块体积和它的缓冲。
/// 单点求值没命中位置时还会去缓冲里找：位置正好落在上一块体积的格点上，就直接读成批算出来的值。
/// 两条路径的结果最后一位可能不同，所以「上一次成批算过哪块体积」会影响之后单点求值的结果，
/// 区块生成要和官方逐位一致就得照搬这个行为。
#[derive(Debug, Clone)]
struct CacheCell {
    value_key: i64,
    value: f32,
    volume: Option<Volume>,
    buffer: Vec<f32>,
}

impl CacheCell {
    const fn empty() -> Self {
        Self {
            value_key: 0,
            value: f32::NAN,
            volume: None,
            buffer: Vec::new(),
        }
    }
}

/// `SamplerContext`：一次求值的上下文，`cache` 的格子和临时缓冲都在这里。
///
/// 官方有两种：`EMPTY_UNCACHED`（`sampleBlockValueUncached` 和对拍用，`cache` 不记忆）和
/// `enableCaches()` 的（区块生成用，每个区块一个）。前者是 [`Context::uncached`]，后者是
/// [`Context::new`]。
#[derive(Debug, Default)]
pub struct Context {
    caching: bool,
    cells: Vec<CacheCell>,
    /// 用完的临时缓冲，下次接着用，省掉反复分配。
    pool: Vec<Vec<f32>>,
}

impl Context {
    /// 开了缓存的上下文，和官方 `SamplerContext.builder().enableCaches()` 一样。
    pub fn new(cache_count: usize) -> Self {
        Self {
            caching: true,
            cells: vec![CacheCell::empty(); cache_count],
            pool: Vec::new(),
        }
    }

    /// 不记忆的上下文，官方的 `SamplerContext.EMPTY_UNCACHED`。
    pub fn uncached() -> Self {
        Self::default()
    }

    /// `SamplerContext.getCacheCell`：格子不够就长出来，没开缓存时没有格子。
    fn cell(&mut self, id: usize) -> Option<&mut CacheCell> {
        if !self.caching {
            return None;
        }
        if id >= self.cells.len() {
            self.cells.resize(id + 1, CacheCell::empty());
        }
        Some(&mut self.cells[id])
    }

    /// 清掉记忆，换一个种子或者重新开始时用。
    pub fn reset(&mut self) {
        for cell in &mut self.cells {
            let buffer = std::mem::take(&mut cell.buffer);
            if buffer.capacity() > 0 {
                self.pool.push(buffer);
            }
            *cell = CacheCell::empty();
        }
    }

    /// `acquireBuffer(volume)`：一块够大的临时缓冲。内容不保证，调用方自己填满。
    pub fn acquire(&mut self, len: usize) -> Vec<f32> {
        let mut buffer = self.pool.pop().unwrap_or_default();
        buffer.clear();
        buffer.resize(len, 0.0);
        buffer
    }

    /// 还回一块临时缓冲。
    pub fn release(&mut self, buffer: Vec<f32>) {
        self.pool.push(buffer);
    }
}

/// `BlockPos.asLong`：x、z 各 26 位，y 12 位，官方 `cache` 的单点键就是它。
pub fn block_pos_as_long(x: i32, y: i32, z: i32) -> i64 {
    const XZ_MASK: i64 = (1 << 26) - 1;
    const Y_MASK: i64 = (1 << 12) - 1;
    ((i64::from(x) & XZ_MASK) << 38) | (i64::from(y) & Y_MASK) | ((i64::from(z) & XZ_MASK) << 12)
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
    /// 按 `path` 选官方的 `sampleValue` 或者 `sampleVolume`。
    ///
    /// [`Path::Volume`] 是在只有这一个点的体积上调 `sampleVolume`，和对拍里官方的做法一样。
    pub fn sample(&self, context: &mut Context, path: Path, x: i32, y: i32, z: i32) -> f32 {
        match path {
            Path::Point => self.sample_value(context, x, y, z),
            Path::Volume => {
                let mut out = [0.0_f32];
                self.sample_volume(context, &mut out, &Volume::point(x, y, z));
                out[0]
            }
        }
    }

    /// 官方的 `sampleValue`：单点求值。
    pub fn sample_value(&self, context: &mut Context, x: i32, y: i32, z: i32) -> f32 {
        match self {
            Self::Constant(value) => *value,
            Self::Noise {
                noise,
                xz_scale,
                y_scale,
            } => noise.get(
                f64::from(x) * xz_scale,
                f64::from(y) * y_scale,
                f64::from(z) * xz_scale,
            ),
            Self::ShiftedXz {
                shift_x,
                shift_z,
                noise,
                xz_scale,
                y_scale,
            } => {
                let noise_x =
                    f64::from(x) * xz_scale + f64::from(shift_x.sample_value(context, x, y, z));
                let noise_y = f64::from(y) * y_scale;
                let noise_z =
                    f64::from(z) * xz_scale + f64::from(shift_z.sample_value(context, x, y, z));
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
                    f64::from(x) * xz_scale + f64::from(shift_x.sample_value(context, x, y, z));
                let noise_y =
                    f64::from(y) * y_scale + f64::from(shift_y.sample_value(context, x, y, z));
                let noise_z =
                    f64::from(z) * xz_scale + f64::from(shift_z.sample_value(context, x, y, z));
                noise.get(noise_x, noise_y, noise_z)
            }
            Self::ShiftB { noise } => {
                noise.get(f64::from(z) * 0.25, f64::from(x) * 0.25, 0.0) * 4.0
            }
            Self::EndIsland { noise } => (end_island_height(noise, x / 8, z / 8) - 8.0) / 128.0,
            Self::DistanceToPoint { point, metric } => metric.compute(
                point[0].wrapping_sub(x) as f32,
                point[1].wrapping_sub(y) as f32,
                point[2].wrapping_sub(z) as f32,
            ),
            Self::GradientClamped { axis, .. }
            | Self::GradientRepeat { axis, .. }
            | Self::GradientMirroredRepeat { axis, .. } => self.gradient(axis.choose(x, y, z)),
            Self::Unary { op, factor, input } => {
                apply_unary(*op, *factor, input.sample_value(context, x, y, z))
            }
            Self::RoundInteger { op, input } => op.apply(input.sample_value(context, x, y, z)),
            Self::Round {
                op,
                input,
                multiple,
            } => {
                let value = input.sample_value(context, x, y, z);
                let multiple = multiple.sample_value(context, x, y, z);
                round_to_multiple(*op, value, multiple)
            }
            Self::Add { left, right } => {
                left.sample_value(context, x, y, z) + right.sample_value(context, x, y, z)
            }
            Self::ConstAdd { left, right } => left.sample_value(context, x, y, z) + right,
            Self::Sub { left, right } => {
                left.sample_value(context, x, y, z) - right.sample_value(context, x, y, z)
            }
            Self::ConstSub { left, right } => left - right.sample_value(context, x, y, z),
            Self::Mul { left, right } => {
                let value = left.sample_value(context, x, y, z);
                // 单点求值时左边是 0 就不看右边。
                if value == 0.0 {
                    0.0
                } else {
                    value * right.sample_value(context, x, y, z)
                }
            }
            Self::ConstMul { left, right } => left.sample_value(context, x, y, z) * right,
            Self::Div { left, right } => {
                let value = left.sample_value(context, x, y, z);
                if value == 0.0 {
                    0.0
                } else {
                    value / right.sample_value(context, x, y, z)
                }
            }
            Self::ConstDiv { left, right } => left / right.sample_value(context, x, y, z),
            Self::Min {
                left,
                right,
                right_min,
            } => {
                let value = left.sample_value(context, x, y, z);
                if value <= *right_min {
                    value
                } else {
                    density::min(value, right.sample_value(context, x, y, z))
                }
            }
            Self::ConstMin { left, right } => {
                density::min(left.sample_value(context, x, y, z), *right)
            }
            Self::Max {
                left,
                right,
                right_max,
            } => {
                let value = left.sample_value(context, x, y, z);
                if value >= *right_max {
                    value
                } else {
                    density::max(value, right.sample_value(context, x, y, z))
                }
            }
            Self::ConstMax { left, right } => {
                density::max(left.sample_value(context, x, y, z), *right)
            }
            Self::PowConstBase { base, exponent } => {
                base.powf(f64::from(exponent.sample_value(context, x, y, z))) as f32
            }
            Self::PowConstExponent { base, exponent } => {
                f64::from(base.sample_value(context, x, y, z)).powf(*exponent) as f32
            }
            Self::Pow { base, exponent } => {
                let base = f64::from(base.sample_value(context, x, y, z));
                let exponent = f64::from(exponent.sample_value(context, x, y, z));
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
                    let value = coordinates[*index].sample_value(context, x, y, z);
                    cached[*index] = value;
                    value
                })
            }
            Self::LerpConstFirst {
                alpha,
                first,
                second,
            } => {
                let alpha = alpha.sample_value(context, x, y, z);
                if alpha == 0.0 {
                    *first
                } else if alpha == 1.0 {
                    second.sample_value(context, x, y, z)
                } else {
                    lerp(alpha, *first, second.sample_value(context, x, y, z))
                }
            }
            Self::LerpConstSecond {
                alpha,
                first,
                second,
            } => {
                let alpha = alpha.sample_value(context, x, y, z);
                if alpha == 0.0 {
                    first.sample_value(context, x, y, z)
                } else if alpha == 1.0 {
                    *second
                } else {
                    lerp(alpha, first.sample_value(context, x, y, z), *second)
                }
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                let alpha = alpha.sample_value(context, x, y, z);
                if alpha == 0.0 {
                    first.sample_value(context, x, y, z)
                } else if alpha == 1.0 {
                    second.sample_value(context, x, y, z)
                } else {
                    lerp(
                        alpha,
                        first.sample_value(context, x, y, z),
                        second.sample_value(context, x, y, z),
                    )
                }
            }
            Self::Clamp { input, min, max } => {
                clamp(input.sample_value(context, x, y, z), *min, *max)
            }
            Self::RangeChoiceConst {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let value = input.sample_value(context, x, y, z);
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
                let value = input.sample_value(context, x, y, z);
                if value >= *min_inclusive && value < *max_exclusive {
                    when_in_range.sample_value(context, x, y, z)
                } else {
                    when_out_of_range.sample_value(context, x, y, z)
                }
            }
            Self::IntervalSelectSingle {
                input,
                threshold,
                if_below,
                if_above,
            } => {
                let value = input.sample_value(context, x, y, z);
                if value < *threshold {
                    if_below.sample_value(context, x, y, z)
                } else {
                    if_above.sample_value(context, x, y, z)
                }
            }
            Self::IntervalSelect {
                input,
                thresholds,
                samplers,
            } => {
                let value = input.sample_value(context, x, y, z);
                samplers[select_interval(thresholds, samplers.len(), value)]
                    .sample_value(context, x, y, z)
            }
            Self::Cached { id, input } => sample_value_cached(context, *id, input, x, y, z),
            Self::BlendDensity(input) => input.sample_value(context, x, y, z),
            Self::Interpolated {
                input,
                cell_size_xz,
                cell_size_y,
            } => {
                let x_in_cell = java_floor_mod(x, *cell_size_xz);
                let y_in_cell = java_floor_mod(y, *cell_size_y);
                let z_in_cell = java_floor_mod(z, *cell_size_xz);
                if x_in_cell == 0 && y_in_cell == 0 && z_in_cell == 0 {
                    return input.sample_value(context, x, y, z);
                }
                // 官方在这里对 2×2×2 的格角调 sampleVolume，所以内层走成批路径。
                let corners = Volume::new(
                    (2, 2, 2),
                    (
                        x.wrapping_sub(x_in_cell),
                        y.wrapping_sub(y_in_cell),
                        z.wrapping_sub(z_in_cell),
                    ),
                    (*cell_size_xz, *cell_size_y, *cell_size_xz),
                );
                let mut buffer = context.acquire(corners.len());
                input.sample_volume(context, &mut buffer, &corners);
                let at = |dx: i32, dy: i32, dz: i32| buffer[corners.index(dx, dy, dz)];
                let value = lerp3(
                    (
                        x_in_cell as f32 / *cell_size_xz as f32,
                        y_in_cell as f32 / *cell_size_y as f32,
                        z_in_cell as f32 / *cell_size_xz as f32,
                    ),
                    [
                        at(0, 0, 0),
                        at(1, 0, 0),
                        at(0, 1, 0),
                        at(1, 1, 0),
                        at(0, 0, 1),
                        at(1, 0, 1),
                        at(0, 1, 1),
                        at(1, 1, 1),
                    ],
                );
                context.release(buffer);
                value
            }
            Self::SliceX { input, x: fixed } => input.sample_value(context, *fixed, y, z),
            Self::SliceY { input, y: fixed } => input.sample_value(context, x, *fixed, z),
            Self::SliceZ { input, z: fixed } => input.sample_value(context, x, y, *fixed),
            Self::SliceXz {
                input,
                x: fixed_x,
                z: fixed_z,
            } => input.sample_value(context, *fixed_x, y, *fixed_z),
            Self::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                let upper = upper_bound.sample_value(context, x, y, z);
                find_surface_from(density, context, x, z, upper, *lower_bound, *cell_height) as f32
            }
        }
    }

    /// 三种 `GradientSampler.compute(coordinate)`。
    fn gradient(&self, coordinate: i32) -> f32 {
        match self {
            Self::GradientClamped {
                from_coordinate,
                min_coordinate,
                max_coordinate,
                from_value,
                coordinate_factor,
                ..
            } => {
                let relative = coordinate
                    .clamp(*min_coordinate, *max_coordinate)
                    .wrapping_sub(*from_coordinate);
                from_value + relative as f32 * coordinate_factor
            }
            Self::GradientRepeat {
                from_coordinate,
                coordinate_range,
                from_value,
                coordinate_factor,
                ..
            } => {
                let relative = coordinate.wrapping_sub(*from_coordinate);
                from_value + java_floor_mod(relative, *coordinate_range) as f32 * coordinate_factor
            }
            Self::GradientMirroredRepeat {
                from_coordinate,
                coordinate_range,
                from_value,
                coordinate_factor,
                ..
            } => {
                let relative = coordinate.wrapping_sub(*from_coordinate);
                let tile = java_floor_div(relative, *coordinate_range);
                let local = relative.wrapping_sub(tile.wrapping_mul(*coordinate_range));
                if tile & 1 == 0 {
                    from_value + local as f32 * coordinate_factor
                } else {
                    from_value + coordinate_range.wrapping_sub(local) as f32 * coordinate_factor
                }
            }
            _ => unreachable!("只给三种梯度采样器用"),
        }
    }

    /// 官方的 `sampleVolume`：在一块体积上成批求值，结果写进 `out`（长度是 `volume.len()`）。
    ///
    /// 每个分支照官方对应采样器的 `sampleVolume` 写，包括子采样器的求值顺序：
    /// `cache` 记的是最后一块体积，顺序不同会让之后的单点求值读到不同的值。
    pub fn sample_volume(&self, context: &mut Context, out: &mut [f32], volume: &Volume) {
        match self {
            Self::Constant(value) => out.fill(*value),
            Self::Noise {
                noise,
                xz_scale,
                y_scale,
            } => {
                out.fill(0.0);
                noise_add_to_volume(noise, out, volume, *xz_scale, *y_scale, 1.0);
            }
            Self::ShiftedXz {
                shift_x,
                shift_z,
                noise,
                xz_scale,
                y_scale,
            } => {
                shift_x.sample_volume(context, out, volume);
                let mut shift_z_values = context.acquire(out.len());
                shift_z.sample_volume(context, &mut shift_z_values, volume);
                let mut index = 0;
                for z in 0..volume.size_z {
                    let base_z = f64::from(volume.block_z(z)) * xz_scale;
                    for x in 0..volume.size_x {
                        let base_x = f64::from(volume.block_x(x)) * xz_scale;
                        for y in 0..volume.size_y {
                            let noise_x = base_x + f64::from(out[index]);
                            let noise_y = f64::from(volume.block_y(y)) * y_scale;
                            let noise_z = base_z + f64::from(shift_z_values[index]);
                            out[index] = noise.get(noise_x, noise_y, noise_z);
                            index += 1;
                        }
                    }
                }
                context.release(shift_z_values);
            }
            Self::ShiftedXyz {
                shift_x,
                shift_y,
                shift_z,
                noise,
                xz_scale,
                y_scale,
            } => {
                shift_x.sample_volume(context, out, volume);
                let mut shift_y_values = context.acquire(out.len());
                shift_y.sample_volume(context, &mut shift_y_values, volume);
                let mut shift_z_values = context.acquire(out.len());
                shift_z.sample_volume(context, &mut shift_z_values, volume);
                let mut index = 0;
                for z in 0..volume.size_z {
                    let base_z = f64::from(volume.block_z(z)) * xz_scale;
                    for x in 0..volume.size_x {
                        let base_x = f64::from(volume.block_x(x)) * xz_scale;
                        for y in 0..volume.size_y {
                            let noise_x = base_x + f64::from(out[index]);
                            let noise_y = f64::from(volume.block_y(y)) * y_scale
                                + f64::from(shift_y_values[index]);
                            let noise_z = base_z + f64::from(shift_z_values[index]);
                            out[index] = noise.get(noise_x, noise_y, noise_z);
                            index += 1;
                        }
                    }
                }
                context.release(shift_z_values);
                context.release(shift_y_values);
            }
            Self::ShiftB { noise } => {
                // 官方把体积转置过来：x 走原来的 z，y 走原来的 x，z 固定是 0。
                let transposed = Volume::new(
                    (volume.size_z, volume.size_x, 1),
                    (volume.min_z, volume.min_x, 0),
                    (volume.step_z, volume.step_x, 1),
                );
                let mut values = context.acquire(transposed.len());
                values.fill(0.0);
                noise_add_to_volume(noise, &mut values, &transposed, 0.25, 0.25, 4.0);
                for z in 0..volume.size_z {
                    for x in 0..volume.size_x {
                        let value = values[transposed.index(z, x, 0)];
                        set_column(out, volume, x, z, value);
                    }
                }
                context.release(values);
            }
            Self::EndIsland { .. } => {
                for z in 0..volume.size_z {
                    let block_z = volume.block_z(z);
                    for x in 0..volume.size_x {
                        let value = self.sample_value(context, volume.block_x(x), 0, block_z);
                        set_column(out, volume, x, z, value);
                    }
                }
            }
            Self::DistanceToPoint { .. } => self.sample_volume_naive(context, out, volume),
            Self::GradientClamped { axis, .. }
            | Self::GradientRepeat { axis, .. }
            | Self::GradientMirroredRepeat { axis, .. } => match axis {
                Axis::X => {
                    for x in 0..volume.size_x {
                        let value = self.gradient(volume.block_x(x));
                        for z in 0..volume.size_z {
                            set_column(out, volume, x, z, value);
                        }
                    }
                }
                Axis::Y => {
                    for y in 0..volume.size_y {
                        let value = self.gradient(volume.block_y(y));
                        for z in 0..volume.size_z {
                            for x in 0..volume.size_x {
                                out[volume.index(x, y, z)] = value;
                            }
                        }
                    }
                }
                Axis::Z => {
                    let layer = (volume.size_x * volume.size_y) as usize;
                    for z in 0..volume.size_z {
                        let value = self.gradient(volume.block_z(z));
                        let start = volume.index(0, 0, z);
                        out[start..start + layer].fill(value);
                    }
                }
            },
            Self::Unary { op, factor, input } => {
                input.sample_volume(context, out, volume);
                for value in out.iter_mut() {
                    *value = apply_unary(*op, *factor, *value);
                }
            }
            Self::RoundInteger { op, input } => {
                input.sample_volume(context, out, volume);
                for value in out.iter_mut() {
                    *value = op.apply(*value);
                }
            }
            Self::Round {
                op,
                input,
                multiple,
            } => {
                input.sample_volume(context, out, volume);
                let multiples = self.child_volume(context, multiple, volume);
                for (value, multiple) in out.iter_mut().zip(&multiples) {
                    *value = round_to_multiple(*op, *value, *multiple);
                }
                context.release(multiples);
            }
            Self::Add { left, right } => {
                left.sample_volume(context, out, volume);
                let right = self.child_volume(context, right, volume);
                for (value, other) in out.iter_mut().zip(&right) {
                    *value += *other;
                }
                context.release(right);
            }
            Self::ConstAdd { left, right } => {
                left.sample_volume(context, out, volume);
                for value in out.iter_mut() {
                    *value += *right;
                }
            }
            Self::Sub { left, right } => {
                left.sample_volume(context, out, volume);
                let right = self.child_volume(context, right, volume);
                for (value, other) in out.iter_mut().zip(&right) {
                    *value += -*other;
                }
                context.release(right);
            }
            Self::ConstSub { left, right } => {
                right.sample_volume(context, out, volume);
                for value in out.iter_mut() {
                    *value = *left - *value;
                }
            }
            Self::Mul { left, right } => {
                left.sample_volume(context, out, volume);
                let right = self.child_volume(context, right, volume);
                for (value, other) in out.iter_mut().zip(&right) {
                    *value *= *other;
                }
                context.release(right);
            }
            Self::ConstMul { left, right } => {
                left.sample_volume(context, out, volume);
                for value in out.iter_mut() {
                    *value *= *right;
                }
            }
            Self::Div { left, right } => {
                left.sample_volume(context, out, volume);
                let right = self.child_volume(context, right, volume);
                for (value, other) in out.iter_mut().zip(&right) {
                    *value /= *other;
                }
                context.release(right);
            }
            Self::ConstDiv { left, right } => {
                right.sample_volume(context, out, volume);
                for value in out.iter_mut() {
                    *value = *left / *value;
                }
            }
            Self::Min { left, right, .. } => {
                left.sample_volume(context, out, volume);
                let right = self.child_volume(context, right, volume);
                for (value, other) in out.iter_mut().zip(&right) {
                    if *other < *value {
                        *value = *other;
                    }
                }
                context.release(right);
            }
            Self::ConstMin { left, right } => {
                left.sample_volume(context, out, volume);
                for value in out.iter_mut() {
                    if *right < *value {
                        *value = *right;
                    }
                }
            }
            Self::Max { left, right, .. } => {
                left.sample_volume(context, out, volume);
                let right = self.child_volume(context, right, volume);
                for (value, other) in out.iter_mut().zip(&right) {
                    if *other > *value {
                        *value = *other;
                    }
                }
                context.release(right);
            }
            Self::ConstMax { left, right } => {
                left.sample_volume(context, out, volume);
                for value in out.iter_mut() {
                    if *right > *value {
                        *value = *right;
                    }
                }
            }
            Self::PowConstBase { base, exponent } => {
                exponent.sample_volume(context, out, volume);
                for value in out.iter_mut() {
                    *value = base.powf(f64::from(*value)) as f32;
                }
            }
            Self::PowConstExponent { base, exponent } => {
                base.sample_volume(context, out, volume);
                for value in out.iter_mut() {
                    *value = f64::from(*value).powf(*exponent) as f32;
                }
            }
            Self::Pow { base, exponent } => {
                base.sample_volume(context, out, volume);
                let exponents = self.child_volume(context, exponent, volume);
                for (value, exponent) in out.iter_mut().zip(&exponents) {
                    *value = f64::from(*value).powf(f64::from(*exponent)) as f32;
                }
                context.release(exponents);
            }
            Self::Spline {
                spline,
                coordinates,
            } => {
                // `BufferSplineInput`：坐标第一次被用到时才在整块体积上求，之后按下标读。
                let mut buffers: Vec<Option<Vec<f32>>> = vec![None; coordinates.len()];
                for (index, value) in out.iter_mut().enumerate() {
                    *value = spline.sample(&mut |coordinate: &usize| {
                        if buffers[*coordinate].is_none() {
                            let mut buffer = context.acquire(volume.len());
                            coordinates[*coordinate].sample_volume(context, &mut buffer, volume);
                            buffers[*coordinate] = Some(buffer);
                        }
                        buffers[*coordinate]
                            .as_ref()
                            .map_or(0.0, |buffer| buffer[index])
                    });
                }
                for buffer in buffers.into_iter().flatten() {
                    context.release(buffer);
                }
            }
            Self::LerpConstFirst {
                alpha,
                first,
                second,
            } => {
                alpha.sample_volume(context, out, volume);
                let seconds = self.child_volume(context, second, volume);
                for (value, second) in out.iter_mut().zip(&seconds) {
                    let alpha = *value;
                    *value = if alpha == 0.0 {
                        *first
                    } else if alpha == 1.0 {
                        *second
                    } else {
                        lerp(alpha, *first, *second)
                    };
                }
                context.release(seconds);
            }
            Self::LerpConstSecond {
                alpha,
                first,
                second,
            } => {
                alpha.sample_volume(context, out, volume);
                let firsts = self.child_volume(context, first, volume);
                for (value, first) in out.iter_mut().zip(&firsts) {
                    let alpha = *value;
                    *value = if alpha == 0.0 {
                        *first
                    } else if alpha == 1.0 {
                        *second
                    } else {
                        lerp(alpha, *first, *second)
                    };
                }
                context.release(firsts);
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                alpha.sample_volume(context, out, volume);
                let firsts = self.child_volume(context, first, volume);
                let seconds = self.child_volume(context, second, volume);
                for ((value, first), second) in out.iter_mut().zip(&firsts).zip(&seconds) {
                    let alpha = *value;
                    *value = if alpha == 0.0 {
                        *first
                    } else if alpha == 1.0 {
                        *second
                    } else {
                        lerp(alpha, *first, *second)
                    };
                }
                context.release(seconds);
                context.release(firsts);
            }
            Self::Clamp { input, min, max } => {
                input.sample_volume(context, out, volume);
                for value in out.iter_mut() {
                    *value = clamp(*value, *min, *max);
                }
            }
            Self::RangeChoiceConst {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                input.sample_volume(context, out, volume);
                for value in out.iter_mut() {
                    *value = if *value >= *min_inclusive && *value < *max_exclusive {
                        *when_in_range
                    } else {
                        *when_out_of_range
                    };
                }
            }
            Self::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                // 官方先把「在范围里」那一支整块算进输出，再算输入和另一支。
                when_in_range.sample_volume(context, out, volume);
                let inputs = self.child_volume(context, input, volume);
                let outside = self.child_volume(context, when_out_of_range, volume);
                for ((value, input), outside) in out.iter_mut().zip(&inputs).zip(&outside) {
                    if !(*input >= *min_inclusive && *input < *max_exclusive) {
                        *value = *outside;
                    }
                }
                context.release(outside);
                context.release(inputs);
            }
            Self::IntervalSelectSingle {
                input,
                threshold,
                if_below,
                if_above,
            } => {
                input.sample_volume(context, out, volume);
                let below = self.child_volume(context, if_below, volume);
                let above = self.child_volume(context, if_above, volume);
                for (index, value) in out.iter_mut().enumerate() {
                    *value = if *value < *threshold {
                        below[index]
                    } else {
                        above[index]
                    };
                }
                context.release(above);
                context.release(below);
            }
            Self::IntervalSelect {
                input,
                thresholds,
                samplers,
            } => {
                input.sample_volume(context, out, volume);
                let mut buffers = Vec::with_capacity(samplers.len());
                for sampler in samplers {
                    buffers.push(self.child_volume(context, sampler, volume));
                }
                for (index, value) in out.iter_mut().enumerate() {
                    *value = buffers[select_interval(thresholds, samplers.len(), *value)][index];
                }
                for buffer in buffers {
                    context.release(buffer);
                }
            }
            Self::Cached { id, input } => sample_volume_cached(context, *id, input, out, volume),
            // 上下文里没有 Blender，官方原样返回输入。
            Self::BlendDensity(input) => input.sample_volume(context, out, volume),
            Self::Interpolated {
                input,
                cell_size_xz,
                cell_size_y,
            } => sample_interpolated(context, input, *cell_size_xz, *cell_size_y, out, volume),
            Self::SliceX { input, x: fixed } => {
                if volume.size_x == 1 && volume.min_x == *fixed {
                    return input.sample_volume(context, out, volume);
                }
                let slice = Volume::new(
                    (1, volume.size_y, volume.size_z),
                    (*fixed, volume.min_y, volume.min_z),
                    (volume.step_x, volume.step_y, volume.step_z),
                );
                let values = self.child_volume(context, input, &slice);
                let mut index = 0;
                for z in 0..volume.size_z {
                    for _ in 0..volume.size_x {
                        for y in 0..volume.size_y {
                            out[index] = values[slice.index(0, y, z)];
                            index += 1;
                        }
                    }
                }
                context.release(values);
            }
            Self::SliceY { input, y: fixed } => {
                if volume.size_y == 1 && volume.min_y == *fixed {
                    return input.sample_volume(context, out, volume);
                }
                let slice = Volume::new(
                    (volume.size_x, 1, volume.size_z),
                    (volume.min_x, *fixed, volume.min_z),
                    (volume.step_x, volume.step_y, volume.step_z),
                );
                let values = self.child_volume(context, input, &slice);
                for z in 0..volume.size_z {
                    for x in 0..volume.size_x {
                        set_column(out, volume, x, z, values[slice.index(x, 0, z)]);
                    }
                }
                context.release(values);
            }
            Self::SliceZ { input, z: fixed } => {
                if volume.size_z == 1 && volume.min_z == *fixed {
                    return input.sample_volume(context, out, volume);
                }
                let slice = Volume::new(
                    (volume.size_x, volume.size_y, 1),
                    (volume.min_x, volume.min_y, *fixed),
                    (volume.step_x, volume.step_y, volume.step_z),
                );
                let values = self.child_volume(context, input, &slice);
                let mut index = 0;
                for _ in 0..volume.size_z {
                    for x in 0..volume.size_x {
                        for y in 0..volume.size_y {
                            out[index] = values[slice.index(x, y, 0)];
                            index += 1;
                        }
                    }
                }
                context.release(values);
            }
            Self::SliceXz {
                input,
                x: fixed_x,
                z: fixed_z,
            } => {
                if volume.size_x == 1
                    && volume.size_z == 1
                    && volume.min_x == *fixed_x
                    && volume.min_z == *fixed_z
                {
                    return input.sample_volume(context, out, volume);
                }
                let slice = Volume::new(
                    (1, volume.size_y, 1),
                    (*fixed_x, volume.min_y, *fixed_z),
                    (volume.step_x, volume.step_y, volume.step_z),
                );
                let values = self.child_volume(context, input, &slice);
                for y in 0..volume.size_y {
                    let value = values[slice.index(0, y, 0)];
                    let mut index = volume.index(0, y, 0);
                    for _ in 0..volume.size_z {
                        for _ in 0..volume.size_x {
                            out[index] = value;
                            index += volume.size_y as usize;
                        }
                    }
                }
                context.release(values);
            }
            Self::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                // 官方要求 sizeY 是 1，外面套着的 `SliceY` 保证了这一点；万一不是，退回逐点求值。
                if volume.size_y != 1 {
                    return self.sample_volume_naive(context, out, volume);
                }
                upper_bound.sample_volume(context, out, volume);
                let mut index = 0;
                for z in 0..volume.size_z {
                    let block_z = volume.block_z(z);
                    for x in 0..volume.size_x {
                        let block_x = volume.block_x(x);
                        out[index] = find_surface_from(
                            density,
                            context,
                            block_x,
                            block_z,
                            out[index],
                            *lower_bound,
                            *cell_height,
                        ) as f32;
                        index += 1;
                    }
                }
            }
        }
    }

    /// `DensitySampler.sampleVolumeNaive`：逐点调 `sampleValue`。
    fn sample_volume_naive(&self, context: &mut Context, out: &mut [f32], volume: &Volume) {
        let mut index = 0;
        for z in 0..volume.size_z {
            let block_z = volume.block_z(z);
            for x in 0..volume.size_x {
                let block_x = volume.block_x(x);
                for y in 0..volume.size_y {
                    out[index] = self.sample_value(context, block_x, volume.block_y(y), block_z);
                    index += 1;
                }
            }
        }
    }

    /// 借一块缓冲，把 `child` 在这块体积上的值算进去。用完要 [`Context::release`]。
    fn child_volume(&self, context: &mut Context, child: &Sampler, volume: &Volume) -> Vec<f32> {
        let mut buffer = context.acquire(volume.len());
        child.sample_volume(context, &mut buffer, volume);
        buffer
    }
}

/// `DensityBuffer.setRange(index(x, 0, z), sizeY, value)`：一整列填同一个值。
fn set_column(out: &mut [f32], volume: &Volume, x: i32, z: i32, value: f32) {
    let start = volume.index(x, 0, z);
    out[start..start + volume.size_y as usize].fill(value);
}

/// `Noise.addToVolume`：每个点加上这个噪声的贡献，体积顺序是 z、x、y。
fn noise_add_to_volume(
    noise: &Noise,
    out: &mut [f32],
    volume: &Volume,
    xz_scale: f64,
    y_scale: f64,
    amplitude: f32,
) {
    let mut index = 0;
    for z in 0..volume.size_z {
        let block_z = volume.block_z(z);
        for x in 0..volume.size_x {
            let block_x = volume.block_x(x);
            for y in 0..volume.size_y {
                out[index] += noise.add_to_point(
                    (block_x, volume.block_y(y), block_z),
                    xz_scale,
                    y_scale,
                    amplitude,
                );
                index += 1;
            }
        }
    }
}

/// `RoundFunction.Sampler.apply(value, multiple)`。
fn round_to_multiple(op: RoundOp, value: f32, multiple: f32) -> f32 {
    if multiple == 0.0 {
        value
    } else {
        op.apply(value / multiple) * multiple
    }
}

/// `IntervalSelectFunction.Sampler.selectSamplerIndex`：第一个比值大的阈值，没有就取最后一个。
fn select_interval(thresholds: &[f32], count: usize, value: f32) -> usize {
    thresholds
        .iter()
        .position(|threshold| value < *threshold)
        .unwrap_or(count - 1)
}

/// `SamplerContext.sampleValueCached`。
fn sample_value_cached(
    context: &mut Context,
    id: usize,
    input: &Sampler,
    x: i32,
    y: i32,
    z: i32,
) -> f32 {
    let key = block_pos_as_long(x, y, z);
    let Some(cell) = context.cell(id) else {
        return input.sample_value(context, x, y, z);
    };
    if cell.value_key == key && !cell.value.is_nan() {
        return cell.value;
    }
    if let Some(volume) = cell.volume
        && let Some(index) = volume.index_of_block(x, y, z)
    {
        return cell.buffer[index];
    }
    let value = input.sample_value(context, x, y, z);
    if let Some(cell) = context.cell(id) {
        cell.value_key = key;
        cell.value = value;
    }
    value
}

/// `SamplerContext.sampleVolumeCached`：同一块体积直接抄上一次的缓冲。
fn sample_volume_cached(
    context: &mut Context,
    id: usize,
    input: &Sampler,
    out: &mut [f32],
    volume: &Volume,
) {
    let Some(cell) = context.cell(id) else {
        return input.sample_volume(context, out, volume);
    };
    if cell.volume != Some(*volume) {
        let old = std::mem::take(&mut cell.buffer);
        cell.volume = Some(*volume);
        context.release(old);
        let mut buffer = context.acquire(volume.len());
        input.sample_volume(context, &mut buffer, volume);
        if let Some(cell) = context.cell(id) {
            cell.buffer = buffer;
        }
    }
    if let Some(cell) = context.cell(id) {
        out.copy_from_slice(&cell.buffer);
    }
}

/// `InterpolatedFunction.Sampler.sampleVolume`。
fn sample_interpolated(
    context: &mut Context,
    input: &Sampler,
    cell_xz: i32,
    cell_y: i32,
    out: &mut [f32],
    volume: &Volume,
) {
    let on_cells = (volume.step_x == cell_xz || volume.size_x == 1)
        && (volume.step_y == cell_y || volume.size_y == 1)
        && (volume.step_z == cell_xz || volume.size_z == 1)
        && java_floor_mod(volume.min_x, cell_xz) == 0
        && java_floor_mod(volume.min_y, cell_y) == 0
        && java_floor_mod(volume.min_z, cell_xz) == 0;
    if on_cells {
        return input.sample_volume(context, out, volume);
    }
    if volume.step_x != 1 || volume.step_y != 1 || volume.step_z != 1 {
        // `sampleWithNonBlockStep`：先在步长为 1 的大体积上插值，再挑出要的点。
        let blocks = Volume::blocks(
            (
                volume.size_x.wrapping_mul(volume.step_x),
                volume.size_y.wrapping_mul(volume.step_y),
                volume.size_z.wrapping_mul(volume.step_z),
            ),
            (volume.min_x, volume.min_y, volume.min_z),
        );
        let mut values = context.acquire(blocks.len());
        sample_interpolated_blocks(context, input, cell_xz, cell_y, &mut values, &blocks);
        for z in 0..volume.size_z {
            for x in 0..volume.size_x {
                for y in 0..volume.size_y {
                    out[volume.index(x, y, z)] = values
                        [blocks.index(x * volume.step_x, y * volume.step_y, z * volume.step_z)];
                }
            }
        }
        context.release(values);
        return;
    }
    sample_interpolated_blocks(context, input, cell_xz, cell_y, out, volume);
}

/// `InterpolatedFunction.Sampler.sampleWithBlockStep`：输入在格角上成批求，格子里插值。
///
/// xz 方向用 `Mth.lerp`，y 方向是起点加上逐格累加的增量，浮点顺序和官方一样，
/// 所以和单点求值的 `lerp3` 结果最后一位会不同。
fn sample_interpolated_blocks(
    context: &mut Context,
    input: &Sampler,
    cell_xz: i32,
    cell_y: i32,
    out: &mut [f32],
    volume: &Volume,
) {
    let cell_min_x = java_floor_div(volume.min_x, cell_xz);
    let cell_min_y = java_floor_div(volume.min_y, cell_y);
    let cell_min_z = java_floor_div(volume.min_z, cell_xz);
    let count_x = java_floor_div(volume.max_x(), cell_xz) - cell_min_x + 1;
    let count_y = java_floor_div(volume.max_y(), cell_y) - cell_min_y + 1;
    let count_z = java_floor_div(volume.max_z(), cell_xz) - cell_min_z + 1;
    let corner_count = |count: i32, max: i32, cell: i32| {
        if java_floor_mod(max, cell) != 0 {
            count + 1
        } else {
            count
        }
    };
    let corners = Volume::new(
        (
            corner_count(count_x, volume.max_x(), cell_xz),
            corner_count(count_y, volume.max_y(), cell_y),
            corner_count(count_z, volume.max_z(), cell_xz),
        ),
        (
            cell_min_x.wrapping_mul(cell_xz),
            cell_min_y.wrapping_mul(cell_y),
            cell_min_z.wrapping_mul(cell_xz),
        ),
        (cell_xz, cell_y, cell_xz),
    );
    let mut values = context.acquire(corners.len());
    input.sample_volume(context, &mut values, &corners);
    let inverse_xz = 1.0 / cell_xz as f32;
    let inverse_y = 1.0 / cell_y as f32;
    let at = |x: i32, y: i32, z: i32| values[corners.index(x, y, z)];
    for cz in 0..count_z {
        let cz1 = (cz + 1).min(corners.size_z - 1);
        for cx in 0..count_x {
            let cx1 = (cx + 1).min(corners.size_x - 1);
            let mut v000 = at(cx, 0, cz);
            let mut v100 = at(cx1, 0, cz);
            let mut v001 = at(cx, 0, cz1);
            let mut v101 = at(cx1, 0, cz1);
            for cy in 0..count_y {
                let cy1 = (cy + 1).min(corners.size_y - 1);
                let v010 = at(cx, cy1, cz);
                let v110 = at(cx1, cy1, cz);
                let v011 = at(cx, cy1, cz1);
                let v111 = at(cx1, cy1, cz1);
                let cell = CellCorners {
                    lower: [v000, v100, v001, v101],
                    upper: [v010, v110, v011, v111],
                };
                fill_cell(
                    out,
                    volume,
                    &corners,
                    (cx, cy, cz),
                    cell,
                    (cell_xz, cell_y),
                    (inverse_xz, inverse_y),
                );
                v000 = v010;
                v100 = v110;
                v001 = v011;
                v101 = v111;
            }
        }
    }
    context.release(values);
}

/// 一个格子的八个角：`lower` 是下面四个，`upper` 是上面四个，顺序都是 (x0z0, x1z0, x0z1, x1z1)。
#[derive(Clone, Copy)]
struct CellCorners {
    lower: [f32; 4],
    upper: [f32; 4],
}

/// `InterpolatedFunction.Sampler.fillCell`。
fn fill_cell(
    out: &mut [f32],
    volume: &Volume,
    corners: &Volume,
    cell: (i32, i32, i32),
    values: CellCorners,
    size: (i32, i32),
    inverse: (f32, f32),
) {
    let offset_x = corners.block_x(cell.0) - volume.min_x;
    let offset_y = corners.block_y(cell.1) - volume.min_y;
    let offset_z = corners.block_z(cell.2) - volume.min_z;
    let start_x = 0.max(-offset_x);
    let start_y = 0.max(-offset_y);
    let start_z = 0.max(-offset_z);
    let end_x = size.0.min(volume.size_x - offset_x) - 1;
    let end_y = size.1.min(volume.size_y - offset_y) - 1;
    let end_z = size.0.min(volume.size_z - offset_z) - 1;
    let [l00, l10, l01, l11] = values.lower;
    let [u00, u10, u01, u11] = values.upper;
    for local_z in start_z..=end_z {
        let z = offset_z + local_z;
        let t_z = local_z as f32 * inverse.0;
        let lower_x0 = lerp(t_z, l00, l01);
        let upper_x0 = lerp(t_z, u00, u01);
        let lower_x1 = lerp(t_z, l10, l11);
        let upper_x1 = lerp(t_z, u10, u11);
        for local_x in start_x..=end_x {
            let x = offset_x + local_x;
            let t_x = local_x as f32 * inverse.0;
            let lower = lerp(t_x, lower_x0, lower_x1);
            let upper = lerp(t_x, upper_x0, upper_x1);
            let delta = (upper - lower) * inverse.1;
            let mut value = lower + delta * start_y as f32;
            let start = volume.index(x, offset_y + start_y, z);
            let len = (end_y - start_y + 1).max(0) as usize;
            for slot in &mut out[start..start + len] {
                *slot = value;
                value += delta;
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
) -> i32 {
    let top_y = floor_f32(upper_bound / cell_height as f32).wrapping_mul(cell_height);
    if top_y <= lower_bound {
        return lower_bound;
    }
    let mut probe_y = top_y;
    while probe_y >= lower_bound {
        if density.sample_value(context, x, probe_y, z) > 0.0 {
            return probe_y;
        }
        // 官方是 int 减法。`upper_bound` 是 NaN 或者无穷时 `top_y` 会饱和到边界，
        // 这里用 wrapping 跟着官方溢出，而不是在 debug 下 panic。
        probe_y = probe_y.wrapping_sub(cell_height);
    }
    lower_bound
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
    caches: HashMap<String, (usize, Arc<Sampler>)>,
    next_cache_id: usize,
    /// `slice_uniform_axes` 的记忆，键是（节点地址，上层定义域）。
    rewritten: HashMap<(usize, u8), Ref>,
    /// 记忆里用到的原节点，留着它们，地址才不会被别的节点重用。
    keep_alive: Vec<Ref>,
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
            rewritten: HashMap::new(),
            keep_alive: Vec::new(),
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

    /// `DensityFunctionCompiler.getSampler(function)`：先照官方重写一遍，再编译。
    ///
    /// 官方编译前跑 `DfRewriteRule.SLICE_UNIFORM_AXES`，把用不到某个轴的子树包进 `slice`。
    /// 单点求值的值不受影响，但成批求值时体积的形状变了，`cache` 记住的体积也跟着变，
    /// 之后单点求值能不能读到成批的缓冲就不一样了，所以要照做。
    pub fn compile(&mut self, function: &Ref) -> Sampler {
        let rewritten = self.slice_uniform_axes(function, density::ALL_AXES);
        self.compile_node(&rewritten)
    }

    /// `DfRewriteRule.SliceUniformAxes.rewrite`：`parent` 是上一层的定义域。
    ///
    /// 结果按（节点，上层定义域）记下来，同一个共享子树重写后还是同一个节点。
    fn slice_uniform_axes(&mut self, function: &Ref, parent: u8) -> Ref {
        if matches!(function.kind, Kind::Constant(_) | Kind::Gradient { .. }) {
            return Arc::clone(function);
        }
        let key = (Arc::as_ptr(function) as usize, parent);
        if let Some(found) = self.rewritten.get(&key) {
            return Arc::clone(found);
        }
        let axes = function.axes();
        let rewritten = if matches!(function.kind, Kind::Cache(_)) {
            // 官方这时 `cache` 已经是 `PreparedCache`，子树另外重写，自己不动。
            Arc::clone(function)
        } else {
            density::function(
                function
                    .kind
                    .map_children(&mut |child: &Ref| self.slice_uniform_axes(child, axes)),
            )
        };
        let result = if parent == axes {
            rewritten
        } else {
            remove_axes(rewritten, parent & !axes)
        };
        self.rewritten.insert(key, Arc::clone(&result));
        self.keep_alive.push(Arc::clone(function));
        result
    }

    /// 编译一个已经重写过的节点。
    fn compile_node(&mut self, function: &Ref) -> Sampler {
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
                let compiled_x = Box::new(self.compile_node(shift_x));
                let compiled_z = Box::new(self.compile_node(shift_z));
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
                    shift_y: Box::new(self.compile_node(shift_y)),
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
                input: Box::new(self.compile_node(input)),
            },
            Kind::Round {
                op,
                input,
                multiple,
            } => {
                let compiled = Box::new(self.compile_node(input));
                if multiple.constant() == Some(1.0) {
                    Sampler::RoundInteger {
                        op: *op,
                        input: compiled,
                    }
                } else {
                    Sampler::Round {
                        op: *op,
                        input: compiled,
                        multiple: Box::new(self.compile_node(multiple)),
                    }
                }
            }
            Kind::Binary { op, left, right } => self.compile_binary(*op, left, right),
            Kind::Pow { base, exponent } => {
                let compiled_base = self.compile_node(base);
                let compiled_exponent = Box::new(self.compile_node(exponent));
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
                    .map(|coordinate| self.compile_node(coordinate))
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
                let compiled_alpha = Box::new(self.compile_node(alpha));
                let compiled_first = Box::new(self.compile_node(first));
                let compiled_second = Box::new(self.compile_node(second));
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
                input: Box::new(self.compile_node(input)),
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
                let compiled_input = Box::new(self.compile_node(input));
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
                        when_in_range: Box::new(self.compile_node(when_in_range)),
                        when_out_of_range: Box::new(self.compile_node(when_out_of_range)),
                    },
                }
            }
            Kind::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let compiled_input = Box::new(self.compile_node(input));
                if thresholds.len() == 1 {
                    return Sampler::IntervalSelectSingle {
                        input: compiled_input,
                        threshold: thresholds[0],
                        if_below: Box::new(self.compile_node(&functions[0])),
                        if_above: Box::new(
                            self.compile_node(functions.last().expect("至少两个函数")),
                        ),
                    };
                }
                let samplers = functions
                    .iter()
                    .map(|function| self.compile_node(function))
                    .collect();
                Sampler::IntervalSelect {
                    input: compiled_input,
                    thresholds: thresholds.clone(),
                    samplers,
                }
            }
            Kind::Cache(input) => self.compile_cache(input),
            Kind::BlendDensity(input) => Sampler::BlendDensity(Box::new(self.compile_node(input))),
            Kind::Interpolated {
                input,
                cell_size_xz,
                cell_size_y,
            } => Sampler::Interpolated {
                input: Box::new(self.compile_node(input)),
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
                    density: Box::new(self.compile_node(density)),
                    upper_bound: Box::new(self.compile_node(upper_bound)),
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
        let compiled_left = Box::new(self.compile_node(left));
        let compiled_right = Box::new(self.compile_node(right));
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
                    input: Box::new(self.compile_node(inner_input)),
                    x,
                    z,
                };
            }
        }
        let compiled = Box::new(self.compile_node(input));
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
        // 官方按输入的 `equals` 去重，记录类是按结构比的，这里用调试输出当结构键。
        let key = format!("{:?}", input.kind);
        if let Some((id, sampler)) = self.caches.get(&key) {
            return Sampler::Cached {
                id: *id,
                input: Arc::clone(sampler),
            };
        }
        let id = self.next_cache_id;
        self.next_cache_id += 1;
        let rewritten = self.slice_uniform_axes(input, density::ALL_AXES);
        let compiled = Arc::new(self.compile_node(&rewritten));
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

/// `SliceUniformAxes.removeAxes`：把 `removed` 里还没切掉的轴一层层切掉，x 在最里面，y 在最外面。
fn remove_axes(function: Ref, removed: u8) -> Ref {
    let mut existing = 0;
    let mut cursor = &function;
    while let Kind::Slice { axis, input, .. } = &cursor.kind {
        existing |= axis.bit();
        cursor = input;
    }
    let removed = removed & !existing;
    let mut result = function;
    for axis in [Axis::X, Axis::Z, Axis::Y] {
        if removed & axis.bit() != 0 {
            result = density::function(Kind::Slice {
                axis,
                coordinate: 0,
                input: result,
            });
        }
    }
    result
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

    /// 编译时官方会把用不到某个轴的根包进 `slice`，看里面那一层。
    fn unsliced(sampler: &Sampler) -> &Sampler {
        match sampler {
            Sampler::SliceX { input, .. }
            | Sampler::SliceY { input, .. }
            | Sampler::SliceZ { input, .. }
            | Sampler::SliceXz { input, .. } => unsliced(input),
            other => other,
        }
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
        assert!(matches!(unsliced(&sampler), Sampler::Constant(value) if *value == -5.0));
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
        assert!(matches!(unsliced(&sampler), Sampler::Cached { id: 0, .. }));
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
            unsliced(&reciprocal),
            Sampler::Unary {
                op: UnaryOp::Reciprocal,
                ..
            }
        ));
        assert_eq!(reciprocal.sample(&mut context, Path::Point, 0, 0, 0), 1.0);
    }

    /// 一个 x、y、z 都用到的式子：x + 2y + 3z，各项都是精确的小整数，成批和单点一定相等。
    const XYZ: &str = r#"{"f": {"type": "minecraft:add",
        "left": {"type": "minecraft:gradient", "axis": "x",
            "from_coordinate": -1000, "to_coordinate": 1000,
            "from_value": -1000.0, "to_value": 1000.0},
        "right": {"type": "minecraft:add",
            "left": {"type": "minecraft:mul", "left": 2.0,
                "right": {"type": "minecraft:gradient", "axis": "y",
                    "from_coordinate": -1000, "to_coordinate": 1000,
                    "from_value": -1000.0, "to_value": 1000.0}},
            "right": {"type": "minecraft:mul", "left": 3.0,
                "right": {"type": "minecraft:gradient", "axis": "z",
                    "from_coordinate": -1000, "to_coordinate": 1000,
                    "from_value": -1000.0, "to_value": 1000.0}}}}}"#;

    /// 整块体积的成批结果按 z、x、y 排，每个点和单点求值相同。
    #[test]
    fn volume_layout_matches_point_values() {
        let (sampler, mut context) = compile("{}", XYZ, "f");
        let volume = Volume::new((3, 4, 2), (-5, 10, 7), (2, 3, 1));
        let mut out = vec![0.0; volume.len()];
        sampler.sample_volume(&mut context, &mut out, &volume);
        for z in 0..volume.size_z {
            for x in 0..volume.size_x {
                for y in 0..volume.size_y {
                    let (bx, by, bz) = (volume.block_x(x), volume.block_y(y), volume.block_z(z));
                    assert_eq!(
                        out[volume.index(x, y, z)],
                        sampler.sample_value(&mut context, bx, by, bz)
                    );
                    assert_eq!(out[volume.index(x, y, z)], (bx + 2 * by + 3 * bz) as f32);
                }
            }
        }
    }

    /// `interpolated` 在步长为 1 的体积上：格点就是输入，格子里线性函数插出来还是它自己。
    #[test]
    fn interpolated_volume_fills_whole_cells() {
        let functions = format!(
            r#"{{"g": {}, "i": {{"type": "minecraft:interpolated", "cell_size_xz": 4,
                "cell_size_y": 8, "input": "g"}}}}"#,
            &XYZ[6..XYZ.len() - 1]
        );
        let (sampler, mut context) = compile("{}", &functions, "i");
        // 起点不对齐格子，终点也不对齐，测两边的半格。
        let volume = Volume::blocks((7, 19, 6), (-3, -9, 2));
        let mut out = vec![0.0; volume.len()];
        sampler.sample_volume(&mut context, &mut out, &volume);
        for z in 0..volume.size_z {
            for x in 0..volume.size_x {
                for y in 0..volume.size_y {
                    let (bx, by, bz) = (volume.block_x(x), volume.block_y(y), volume.block_z(z));
                    assert_eq!(out[volume.index(x, y, z)], (bx + 2 * by + 3 * bz) as f32);
                }
            }
        }
        // 格点体积直接交给输入。
        let corners = Volume::new((2, 2, 2), (0, 0, 0), (4, 8, 4));
        let mut out = vec![0.0; corners.len()];
        sampler.sample_volume(&mut context, &mut out, &corners);
        assert_eq!(out[corners.index(1, 1, 1)], (4 + 16 + 12) as f32);
    }

    /// 开了缓存时，单点求值落在上一块成批体积的格点上就读缓冲。
    #[test]
    fn point_lookups_read_the_last_cached_volume() {
        let (sampler, mut context) = compile(
            "{}",
            r#"{"c": {"type": "minecraft:cache",
                "input": {"type": "minecraft:gradient", "axis": "y",
                    "from_coordinate": 0, "to_coordinate": 100,
                    "from_value": 0.0, "to_value": 1.0}}}"#,
            "c",
        );
        let volume = Volume::blocks((1, 4, 1), (0, 0, 0));
        let mut out = vec![0.0; volume.len()];
        sampler.sample_volume(&mut context, &mut out, &volume);
        // 改掉缓冲里的值，单点求值读到的就是改过的值，说明它没有重新算。
        let id = match unsliced(&sampler) {
            Sampler::Cached { id, .. } => *id,
            _ => unreachable!("根是 cache"),
        };
        if let Some(cell) = context.cell(id) {
            cell.buffer[2] = 42.0;
        }
        assert_eq!(sampler.sample_value(&mut context, 0, 2, 0), 42.0);
        // 不在体积里的点照常求值。
        assert_eq!(sampler.sample_value(&mut context, 0, 50, 0), 0.5);
        // 不开缓存时什么都不记。
        let mut uncached = Context::uncached();
        assert_eq!(sampler.sample_value(&mut uncached, 0, 2, 0), 0.02);
    }

    /// `range_choice` 成批时三支都算，按输入选。
    #[test]
    fn range_choice_volume_picks_per_point() {
        let (sampler, mut context) = compile(
            "{}",
            r#"{"r": {"type": "minecraft:range_choice",
                "input": {"type": "minecraft:gradient", "axis": "y",
                    "from_coordinate": 0, "to_coordinate": 10,
                    "from_value": 0.0, "to_value": 10.0},
                "min_inclusive": 3.0, "max_exclusive": 6.0,
                "when_in_range": {"type": "minecraft:gradient", "axis": "x",
                    "from_coordinate": 0, "to_coordinate": 10,
                    "from_value": 0.0, "to_value": 10.0},
                "when_out_of_range": -1.0}}"#,
            "r",
        );
        let volume = Volume::blocks((2, 10, 1), (4, 0, 0));
        let mut out = vec![0.0; volume.len()];
        sampler.sample_volume(&mut context, &mut out, &volume);
        for x in 0..2 {
            for y in 0..10 {
                let expected = if (3..6).contains(&y) {
                    (4 + x) as f32
                } else {
                    -1.0
                };
                assert_eq!(out[volume.index(x, y, 0)], expected);
            }
        }
    }
}
