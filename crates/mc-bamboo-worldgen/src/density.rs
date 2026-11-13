//! 官方 `levelgen/densityfunction` 下的密度函数树：按官方 codec 解析，算范围和定义域。
//!
//! 这里只有「函数」这一层，和官方的 `DensityFunction` 对应；求值的那一层在 [`crate::sampler`]。
//! 范围（`range()`）不是文档：官方编译 `min`、`max` 和 `pow` 时用它决定短路和整支丢弃，
//! 所以要和官方逐位相同。
//!
//! 注册表里的引用（JSON 里的字符串）在解析时就展开成同一个节点，
//! 和官方编译前先跑一遍 `DfRewriteRule.INLINE_REFERENCE` 的结果一样。

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use serde_json::Value;

use crate::{
    interval::Interval,
    math::{clamp, cube, java_max, java_min, java_round, sqrt_f32},
    noise::{BlendedNoiseParameters, NormalNoise, NormalNoiseParameters, Normalization},
    spline::{Multipoint, Spline, SplineCoordinate},
};

/// `DensityFunction.AXIS_X`。
pub const AXIS_X: u8 = 1;
/// `DensityFunction.AXIS_Y`。
pub const AXIS_Y: u8 = 2;
/// `DensityFunction.AXIS_Z`。
pub const AXIS_Z: u8 = 4;
/// `DensityFunction.ALL_AXES`。
pub const ALL_AXES: u8 = 7;

/// 官方 `Direction.Axis`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    /// `DensityFunction.axesFrom(axis)`。
    pub fn bit(self) -> u8 {
        match self {
            Self::X => AXIS_X,
            Self::Y => AXIS_Y,
            Self::Z => AXIS_Z,
        }
    }

    /// `Direction.Axis.choose(x, y, z)`。
    pub fn choose(self, x: i32, y: i32, z: i32) -> i32 {
        match self {
            Self::X => x,
            Self::Y => y,
            Self::Z => z,
        }
    }

    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "x" => Ok(Self::X),
            "y" => Ok(Self::Y),
            "z" => Ok(Self::Z),
            _ => Err(format!("unknown axis {name}")),
        }
    }
}

/// 官方 `TilingMode`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TilingMode {
    ClampToEdge,
    Repeat,
    MirroredRepeat,
}

impl TilingMode {
    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "clamp_to_edge" => Ok(Self::ClampToEdge),
            "repeat" => Ok(Self::Repeat),
            "mirrored_repeat" => Ok(Self::MirroredRepeat),
            _ => Err(format!("unknown tiling mode {name}")),
        }
    }
}

/// 官方 `DistanceMetric`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceMetric {
    Euclidean,
    EuclideanSquared,
    Manhattan,
    Chebyshev,
}

impl DistanceMetric {
    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "euclidean" => Ok(Self::Euclidean),
            "euclidean_squared" => Ok(Self::EuclideanSquared),
            "manhattan" => Ok(Self::Manhattan),
            "chebyshev" => Ok(Self::Chebyshev),
            _ => Err(format!("unknown distance metric {name}")),
        }
    }

    /// `DistanceMetric.compute(deltaX, deltaY, deltaZ)`：欧氏距离按 `Mth.length` 先按 float 求和再开方。
    pub fn compute(self, dx: f32, dy: f32, dz: f32) -> f32 {
        match self {
            Self::Euclidean => sqrt_f32(dx * dx + dy * dy + dz * dz),
            Self::EuclideanSquared => dx * dx + dy * dy + dz * dz,
            Self::Manhattan => dx.abs() + dy.abs() + dz.abs(),
            Self::Chebyshev => java_max(java_max(dx.abs(), dy.abs()), dz.abs()),
        }
    }
}

/// `UnaryFunction.Type`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Abs,
    Square,
    Cube,
    Sqrt,
    HalfNegative,
    QuarterNegative,
    Reciprocal,
    Negate,
    Squeeze,
    Log,
    Sign,
}

/// `RoundFunction.Type`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundOp {
    Floor,
    Round,
    Ceil,
    Truncate,
}

impl RoundOp {
    /// `RoundFunction.roundToInteger`。`Math.round` 不是「四舍六入」，按 JDK 的位运算来。
    pub fn apply(self, input: f32) -> f32 {
        match self {
            Self::Floor => f64::from(input).floor() as f32,
            Self::Round => java_round(input) as f32,
            Self::Ceil => f64::from(input).ceil() as f32,
            Self::Truncate => {
                if input > 0.0 {
                    f64::from(input).floor() as f32
                } else {
                    f64::from(input).ceil() as f32
                }
            }
        }
    }
}

/// `BinaryFunction.Type`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
}

/// `UnaryFunction.LeakyReLUSampler.apply`。
pub fn leaky_relu(negative_factor: f32, input: f32) -> f32 {
    if input > 0.0 {
        input
    } else {
        input * negative_factor
    }
}

/// `UnaryFunction.SqueezeSampler.apply`。
pub fn squeeze(input: f32) -> f32 {
    let clamped = clamp(input, -1.0, 1.0);
    clamped / 2.0 - cube(clamped) / 24.0
}

/// 一个密度函数节点，带着算好的范围和定义域。
#[derive(Debug)]
pub struct Function {
    pub kind: Kind,
    range: Interval,
    axes: u8,
    has_interpolated: bool,
}

/// 共享的密度函数节点。注册表里的引用指向同一个 [`Function`]。
pub type Ref = Arc<Function>;

impl Function {
    pub fn range(&self) -> Interval {
        self.range
    }

    pub fn axes(&self) -> u8 {
        self.axes
    }

    /// 这棵子树里有没有 `interpolated`。
    ///
    /// 有的话，成批那条路径在一个坐标上不是纯函数（官方按整格插值，用到了体积的边界），
    /// 单点求值仍然和官方一致，但不能拿 1×1×1 的 `sampleVolume` 来对。
    pub fn has_interpolated(&self) -> bool {
        self.has_interpolated
    }

    /// 这个节点是不是官方的 `ConstantFunction`，官方编译时按它选采样器。
    pub fn constant(&self) -> Option<f32> {
        match self.kind {
            Kind::Constant(value) => Some(value),
            _ => None,
        }
    }
}

impl SplineCoordinate for Ref {
    fn range(&self) -> Interval {
        Function::range(self)
    }
}

/// `noise` 和 `shift*` 密度函数指向的那一条噪声参数。
#[derive(Debug, Clone)]
pub struct NoiseHolder {
    pub id: String,
    pub noise: Arc<NormalNoise>,
}

/// 官方全部密度函数类型。
#[derive(Debug)]
pub enum Kind {
    /// `ConstantFunction`。
    Constant(f32),
    /// `SimpleDensityFunction.BLEND_ALPHA`。
    BlendAlpha,
    /// `SimpleDensityFunction.BLEND_OFFSET`。
    BlendOffset,
    /// `SimpleDensityFunction.BEARDIFIER`。
    Beardifier,
    /// `NoiseFunction`。
    Noise {
        noise: NoiseHolder,
        xz_scale: f64,
        y_scale: f64,
        shift_x: Ref,
        shift_y: Ref,
        shift_z: Ref,
    },
    /// `EndIslandFunction`。
    EndOuterIslands,
    /// `DistanceToPointFunction`。
    DistanceToPoint {
        point: [i32; 3],
        metric: DistanceMetric,
    },
    /// `GradientFunction`。
    Gradient {
        axis: Axis,
        tiling: TilingMode,
        from_coordinate: i32,
        to_coordinate: i32,
        from_value: f32,
        to_value: f32,
    },
    /// `ShiftNoiseFunction.ShiftA`。
    ShiftA(NoiseHolder),
    /// `ShiftNoiseFunction.ShiftB`。
    ShiftB(NoiseHolder),
    /// `ShiftNoiseFunction.Shift`。
    Shift(NoiseHolder),
    /// `UnaryFunction`。
    Unary { op: UnaryOp, input: Ref },
    /// `RoundFunction`。
    Round {
        op: RoundOp,
        input: Ref,
        multiple: Ref,
    },
    /// `BinaryFunction`。
    Binary { op: BinaryOp, left: Ref, right: Ref },
    /// `PowFunction`。
    Pow { base: Ref, exponent: Ref },
    /// `SplineFunction`。
    Spline(Spline<Ref>),
    /// `LerpFunction`。
    Lerp { alpha: Ref, first: Ref, second: Ref },
    /// `ClampFunction`。
    Clamp { input: Ref, min: f32, max: f32 },
    /// `RangeChoiceFunction`。
    RangeChoice {
        input: Ref,
        min_inclusive: f32,
        max_exclusive: f32,
        when_in_range: Ref,
        when_out_of_range: Ref,
    },
    /// `IntervalSelectFunction`。
    IntervalSelect {
        input: Ref,
        thresholds: Vec<f32>,
        functions: Vec<Ref>,
    },
    /// `CacheFunction`。
    Cache(Ref),
    /// `BlendDensityFunction`。
    BlendDensity(Ref),
    /// `InterpolatedFunction`。
    Interpolated {
        input: Ref,
        cell_size_xz: i32,
        cell_size_y: i32,
    },
    /// `SliceFunction`。
    Slice {
        axis: Axis,
        coordinate: i32,
        input: Ref,
    },
    /// `FindTopSurfaceFunction`。
    FindTopSurface {
        density: Ref,
        upper_bound: Ref,
        lower_bound: i32,
        cell_height: i32,
    },
    /// `BlendedNoise`，JSON 里叫 `old_blended_noise`。
    OldBlendedNoise(BlendedNoiseParameters),
}

impl Kind {
    /// `DensityFunction.range()`。
    fn range(&self) -> Interval {
        match self {
            Self::Constant(value) => Interval::of_exact(*value),
            Self::BlendAlpha => Interval::of(0.0, 1.0),
            // `Blender.OFFSET_KEY` 没有范围，`Beardifier.RANGE` 也是无穷。
            Self::BlendOffset | Self::Beardifier => Interval::INFINITE,
            Self::Noise { noise, .. } => noise.noise.range(),
            Self::EndOuterIslands => Interval::of(-0.843_75, 0.5625),
            Self::DistanceToPoint { .. } => Interval::of(0.0, f32::INFINITY),
            Self::Gradient {
                from_value,
                to_value,
                ..
            } => Interval::encapsulating_values(*from_value, *to_value),
            Self::ShiftA(noise) | Self::ShiftB(noise) | Self::Shift(noise) => {
                Interval::mul_of(noise.noise.range(), Interval::of_exact(4.0))
            }
            Self::Unary { op, input } => unary_range(*op, input.range()),
            Self::Round {
                op,
                input,
                multiple,
            } => {
                let multiple_range = multiple.range();
                let op = *op;
                Interval::mul_of(
                    Interval::map_monotonic(
                        Interval::div_of(input.range(), multiple_range),
                        |value| op.apply(value),
                    ),
                    multiple_range,
                )
            }
            Self::Binary { op, left, right } => binary_range(*op, left.range(), right.range()),
            Self::Pow { base, exponent } => Interval::pow(base.range(), exponent.range()),
            Self::Spline(spline) => spline.range(),
            Self::Lerp {
                alpha,
                first,
                second,
            } => Interval::lerp(alpha.range(), first.range(), second.range()),
            Self::Clamp { input, min, max } => Interval::clamp(input.range(), *min, *max),
            Self::RangeChoice {
                when_in_range,
                when_out_of_range,
                ..
            } => Interval::encapsulating_all(&[when_in_range.range(), when_out_of_range.range()]),
            Self::IntervalSelect { functions, .. } => {
                let ranges: Vec<Interval> =
                    functions.iter().map(|function| function.range()).collect();
                Interval::encapsulating_all(&ranges)
            }
            Self::Cache(input)
            | Self::BlendDensity(input)
            | Self::Interpolated { input, .. }
            | Self::Slice { input, .. } => input.range(),
            Self::FindTopSurface {
                upper_bound,
                lower_bound,
                ..
            } => {
                let lower = *lower_bound as f32;
                Interval::of(lower, java_max(lower, upper_bound.range().max()))
            }
            Self::OldBlendedNoise(parameters) => parameters.range(),
        }
    }

    /// `DensityFunction.domainAxes()`。
    fn axes(&self) -> u8 {
        match self {
            Self::Constant(_) => 0,
            Self::BlendAlpha
            | Self::BlendOffset
            | Self::EndOuterIslands
            | Self::ShiftA(_)
            | Self::ShiftB(_) => AXIS_X | AXIS_Z,
            Self::Beardifier
            | Self::DistanceToPoint { .. }
            | Self::Shift(_)
            | Self::OldBlendedNoise(_) => ALL_AXES,
            Self::Noise {
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
                ..
            } => {
                let mut axes = ALL_AXES;
                if *y_scale == 0.0 {
                    axes &= !AXIS_Y;
                }
                if *xz_scale == 0.0 {
                    axes &= !(AXIS_X | AXIS_Z);
                }
                axes | shift_x.axes() | shift_y.axes() | shift_z.axes()
            }
            Self::Gradient { axis, .. } => axis.bit(),
            Self::Unary { input, .. }
            | Self::Cache(input)
            | Self::BlendDensity(input)
            | Self::Interpolated { input, .. }
            | Self::Clamp { input, .. } => input.axes(),
            Self::Round {
                input, multiple, ..
            } => input.axes() | multiple.axes(),
            Self::Binary { left, right, .. } => left.axes() | right.axes(),
            Self::Pow { base, exponent } => base.axes() | exponent.axes(),
            Self::Spline(spline) => {
                let mut axes = 0;
                spline.for_each_coordinate(&mut |coordinate: &Ref| axes |= coordinate.axes());
                axes
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => alpha.axes() | first.axes() | second.axes(),
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_of_range,
                ..
            } => input.axes() | when_in_range.axes() | when_out_of_range.axes(),
            Self::IntervalSelect {
                input, functions, ..
            } => functions
                .iter()
                .fold(input.axes(), |axes, function| axes | function.axes()),
            Self::Slice { axis, input, .. } => input.axes() & !axis.bit(),
            Self::FindTopSurface {
                density,
                upper_bound,
                ..
            } => (density.axes() | upper_bound.axes()) & !AXIS_Y,
        }
    }

    /// 这棵子树里有没有 `interpolated`。
    fn has_interpolated(&self) -> bool {
        let mut found = matches!(self, Self::Interpolated { .. });
        self.for_each_child(&mut |child: &Ref| found |= child.has_interpolated());
        found
    }

    /// 遍历直接子节点。
    fn for_each_child(&self, visit: &mut impl FnMut(&Ref)) {
        match self {
            Self::Constant(_)
            | Self::BlendAlpha
            | Self::BlendOffset
            | Self::Beardifier
            | Self::EndOuterIslands
            | Self::DistanceToPoint { .. }
            | Self::Gradient { .. }
            | Self::ShiftA(_)
            | Self::ShiftB(_)
            | Self::Shift(_)
            | Self::OldBlendedNoise(_) => {}
            Self::Noise {
                shift_x,
                shift_y,
                shift_z,
                ..
            } => {
                visit(shift_x);
                visit(shift_y);
                visit(shift_z);
            }
            Self::Unary { input, .. }
            | Self::Cache(input)
            | Self::BlendDensity(input)
            | Self::Interpolated { input, .. }
            | Self::Slice { input, .. }
            | Self::Clamp { input, .. } => visit(input),
            Self::Round {
                input, multiple, ..
            } => {
                visit(input);
                visit(multiple);
            }
            Self::Binary { left, right, .. } => {
                visit(left);
                visit(right);
            }
            Self::Pow { base, exponent } => {
                visit(base);
                visit(exponent);
            }
            Self::Spline(spline) => spline.for_each_coordinate(visit),
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                visit(alpha);
                visit(first);
                visit(second);
            }
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_of_range,
                ..
            } => {
                visit(input);
                visit(when_in_range);
                visit(when_out_of_range);
            }
            Self::IntervalSelect {
                input, functions, ..
            } => {
                visit(input);
                for function in functions {
                    visit(function);
                }
            }
            Self::FindTopSurface {
                density,
                upper_bound,
                ..
            } => {
                visit(density);
                visit(upper_bound);
            }
        }
    }
}

impl Kind {
    /// 换掉全部直接子节点，别的字段原样复制。官方 `rewriteChildren` 的那一半。
    ///
    /// `cache` 的输入不在这里换：官方编译时 `cache` 已经变成了 `PreparedCache`，
    /// 它的 `rewriteChildren` 返回自己。
    pub fn map_children(&self, map: &mut impl FnMut(&Ref) -> Ref) -> Kind {
        match self {
            Self::Constant(value) => Self::Constant(*value),
            Self::BlendAlpha => Self::BlendAlpha,
            Self::BlendOffset => Self::BlendOffset,
            Self::Beardifier => Self::Beardifier,
            Self::EndOuterIslands => Self::EndOuterIslands,
            Self::DistanceToPoint { point, metric } => Self::DistanceToPoint {
                point: *point,
                metric: *metric,
            },
            Self::Gradient {
                axis,
                tiling,
                from_coordinate,
                to_coordinate,
                from_value,
                to_value,
            } => Self::Gradient {
                axis: *axis,
                tiling: *tiling,
                from_coordinate: *from_coordinate,
                to_coordinate: *to_coordinate,
                from_value: *from_value,
                to_value: *to_value,
            },
            Self::ShiftA(noise) => Self::ShiftA(noise.clone()),
            Self::ShiftB(noise) => Self::ShiftB(noise.clone()),
            Self::Shift(noise) => Self::Shift(noise.clone()),
            Self::OldBlendedNoise(parameters) => Self::OldBlendedNoise(*parameters),
            Self::Noise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => Self::Noise {
                noise: noise.clone(),
                xz_scale: *xz_scale,
                y_scale: *y_scale,
                shift_x: map(shift_x),
                shift_y: map(shift_y),
                shift_z: map(shift_z),
            },
            Self::Unary { op, input } => Self::Unary {
                op: *op,
                input: map(input),
            },
            Self::Round {
                op,
                input,
                multiple,
            } => Self::Round {
                op: *op,
                input: map(input),
                multiple: map(multiple),
            },
            Self::Binary { op, left, right } => Self::Binary {
                op: *op,
                left: map(left),
                right: map(right),
            },
            Self::Pow { base, exponent } => Self::Pow {
                base: map(base),
                exponent: map(exponent),
            },
            Self::Spline(spline) => Self::Spline(spline.map_coordinates(map)),
            Self::Lerp {
                alpha,
                first,
                second,
            } => Self::Lerp {
                alpha: map(alpha),
                first: map(first),
                second: map(second),
            },
            Self::Clamp { input, min, max } => Self::Clamp {
                input: map(input),
                min: *min,
                max: *max,
            },
            Self::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => Self::RangeChoice {
                input: map(input),
                min_inclusive: *min_inclusive,
                max_exclusive: *max_exclusive,
                when_in_range: map(when_in_range),
                when_out_of_range: map(when_out_of_range),
            },
            Self::IntervalSelect {
                input,
                thresholds,
                functions,
            } => Self::IntervalSelect {
                input: map(input),
                thresholds: thresholds.clone(),
                functions: functions.iter().map(&mut *map).collect(),
            },
            Self::Cache(input) => Self::Cache(Arc::clone(input)),
            Self::BlendDensity(input) => Self::BlendDensity(map(input)),
            Self::Interpolated {
                input,
                cell_size_xz,
                cell_size_y,
            } => Self::Interpolated {
                input: map(input),
                cell_size_xz: *cell_size_xz,
                cell_size_y: *cell_size_y,
            },
            Self::Slice {
                axis,
                coordinate,
                input,
            } => Self::Slice {
                axis: *axis,
                coordinate: *coordinate,
                input: map(input),
            },
            Self::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => Self::FindTopSurface {
                density: map(density),
                upper_bound: map(upper_bound),
                lower_bound: *lower_bound,
                cell_height: *cell_height,
            },
        }
    }
}

/// `UnaryFunction.range()`。
fn unary_range(op: UnaryOp, input: Interval) -> Interval {
    match op {
        UnaryOp::Abs => Interval::abs(input),
        UnaryOp::Square => Interval::square_of(input),
        UnaryOp::Cube => Interval::map_monotonic(input, cube),
        UnaryOp::Sqrt => Interval::pow(input, Interval::of_exact(0.5)),
        UnaryOp::HalfNegative => Interval::map_monotonic(input, |value| leaky_relu(0.5, value)),
        UnaryOp::QuarterNegative => Interval::map_monotonic(input, |value| leaky_relu(0.25, value)),
        UnaryOp::Reciprocal => Interval::reciprocal(input),
        UnaryOp::Negate => Interval::sub_of(Interval::of_exact(0.0), input),
        UnaryOp::Squeeze => Interval::map_monotonic(input, squeeze),
        UnaryOp::Log => Interval::log(input),
        UnaryOp::Sign => Interval::sign(input),
    }
}

/// `BinaryFunction.range()`。
fn binary_range(op: BinaryOp, left: Interval, right: Interval) -> Interval {
    match op {
        BinaryOp::Add => Interval::add_of(left, right),
        BinaryOp::Sub => Interval::sub_of(left, right),
        BinaryOp::Mul => Interval::mul_of(left, right),
        BinaryOp::Div => Interval::div_of(left, right),
        BinaryOp::Min => Interval::min_of(left, right),
        BinaryOp::Max => Interval::max_of(left, right),
    }
}

/// 从一个 [`Kind`] 造出带范围和定义域的共享节点。
pub fn function(kind: Kind) -> Ref {
    let range = kind.range();
    let axes = kind.axes();
    let has_interpolated = kind.has_interpolated();
    Arc::new(Function {
        kind,
        range,
        axes,
        has_interpolated,
    })
}

/// `worldgen/noise` 注册表：噪声 ID 到参数。
#[derive(Debug, Default)]
pub struct NoiseRegistry {
    entries: HashMap<String, Arc<NormalNoise>>,
}

impl NoiseRegistry {
    /// 按官方 `NormalNoise.Parameters.CODEC` 解析每一条。
    pub fn load(raw: &serde_json::Map<String, Value>) -> Result<Self, String> {
        let mut entries = HashMap::with_capacity(raw.len());
        for (id, value) in raw {
            let parameters =
                parse_noise_parameters(value).map_err(|error| format!("noise {id}: {error}"))?;
            entries.insert(id.clone(), Arc::new(NormalNoise::new(parameters)));
        }
        Ok(Self { entries })
    }

    pub fn get(&self, id: &str) -> Option<&Arc<NormalNoise>> {
        self.entries.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
}

fn parse_noise_parameters(value: &Value) -> Result<NormalNoiseParameters, String> {
    let object = value.as_object().ok_or("expected an object")?;
    let base_amplitude = optional_f64(object, "base_amplitude")?.unwrap_or(1.0);
    let base_octave = required_i32(object, "base_octave")?;
    let octave_count = optional_i32(object, "octave_count")?.unwrap_or(1);
    let normalize = match object.get("normalize") {
        None | Some(Value::Bool(true)) => Normalization::Enabled,
        Some(Value::Bool(false)) => Normalization::Disabled,
        Some(Value::String(name)) if name == "legacy" => Normalization::Legacy,
        Some(other) => return Err(format!("invalid normalization type: {other}")),
    };
    let amplitude_modifiers = match object.get("amplitude_modifiers") {
        None => Vec::new(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| value.as_f64().ok_or_else(|| "expected a number".to_owned()))
            .collect::<Result<Vec<f64>, String>>()?,
        Some(other) => return Err(format!("amplitude_modifiers must be a list: {other}")),
    };
    if !amplitude_modifiers.is_empty() && amplitude_modifiers.len() as i32 != octave_count {
        return Err(format!(
            "amplitude_modifiers had size {}, but octave_count was {octave_count}",
            amplitude_modifiers.len()
        ));
    }
    Ok(NormalNoiseParameters {
        base_amplitude,
        base_octave,
        octave_count,
        normalize,
        amplitude_modifiers,
    })
}

/// `worldgen/density_function` 注册表：ID 到解析好的节点，引用已经展开。
#[derive(Debug, Default)]
pub struct FunctionRegistry {
    entries: HashMap<String, Ref>,
}

impl FunctionRegistry {
    pub fn load(
        raw: &serde_json::Map<String, Value>,
        noises: &NoiseRegistry,
    ) -> Result<Self, String> {
        let mut entries: HashMap<String, Ref> = HashMap::with_capacity(raw.len());
        let mut resolving = HashSet::new();
        for id in raw.keys() {
            resolve(id, raw, noises, &mut entries, &mut resolving)?;
        }
        Ok(Self { entries })
    }

    pub fn get(&self, id: &str) -> Option<&Ref> {
        self.entries.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
}

fn resolve(
    id: &str,
    raw: &serde_json::Map<String, Value>,
    noises: &NoiseRegistry,
    entries: &mut HashMap<String, Ref>,
    resolving: &mut HashSet<String>,
) -> Result<Ref, String> {
    if let Some(found) = entries.get(id) {
        return Ok(Arc::clone(found));
    }
    if !resolving.insert(id.to_owned()) {
        return Err(format!("density function {id} references itself"));
    }
    let value = raw
        .get(id)
        .ok_or_else(|| format!("unknown density function {id}"))?;
    let parsed = parse(value, raw, noises, entries, resolving)
        .map_err(|error| format!("density function {id}: {error}"))?;
    resolving.remove(id);
    entries.insert(id.to_owned(), Arc::clone(&parsed));
    Ok(parsed)
}

/// `DensityFunction.CODEC`：字符串是注册表引用，数字是常量，对象按 `type` 分发。
pub fn parse(
    value: &Value,
    raw: &serde_json::Map<String, Value>,
    noises: &NoiseRegistry,
    entries: &mut HashMap<String, Ref>,
    resolving: &mut HashSet<String>,
) -> Result<Ref, String> {
    match value {
        Value::String(id) => resolve(id, raw, noises, entries, resolving),
        Value::Number(_) => Ok(function(Kind::Constant(as_f32(value)?))),
        Value::Object(object) => {
            let kind = parse_object(object, raw, noises, entries, resolving)?;
            Ok(function(kind))
        }
        other => Err(format!("expected a density function, got {other}")),
    }
}

fn parse_object(
    object: &serde_json::Map<String, Value>,
    raw: &serde_json::Map<String, Value>,
    noises: &NoiseRegistry,
    entries: &mut HashMap<String, Ref>,
    resolving: &mut HashSet<String>,
) -> Result<Kind, String> {
    let kind_name = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or("missing type")?;
    let short = kind_name
        .strip_prefix("minecraft:")
        .ok_or_else(|| format!("unknown namespace in {kind_name}"))?;

    // 用宏而不是闭包：闭包会一直借着 entries 和 resolving，别的分支就用不了它们了。
    macro_rules! child {
        ($name:expr) => {{
            let value = object
                .get($name)
                .ok_or_else(|| format!("missing field {}", $name))?;
            parse(value, raw, noises, entries, resolving)?
        }};
    }
    macro_rules! optional_child {
        ($name:expr, $default:expr) => {
            match object.get($name) {
                None => $default,
                Some(value) => parse(value, raw, noises, entries, resolving)?,
            }
        };
    }

    match short {
        "constant" => Ok(Kind::Constant(required_f32(object, "value")?)),
        "blend_alpha" => Ok(Kind::BlendAlpha),
        "blend_offset" => Ok(Kind::BlendOffset),
        "beardifier" => Ok(Kind::Beardifier),
        "end_outer_islands" => Ok(Kind::EndOuterIslands),
        "noise" => {
            let noise = parse_noise_ref(object, noises)?;
            let xz_scale = required_f64(object, "xz_scale")?;
            let y_scale = required_f64(object, "y_scale")?;
            let shift_x = optional_child!("shift_x", function(Kind::Constant(0.0)));
            let shift_y = optional_child!("shift_y", function(Kind::Constant(0.0)));
            let shift_z = optional_child!("shift_z", function(Kind::Constant(0.0)));
            Ok(Kind::Noise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            })
        }
        "shift_a" => Ok(Kind::ShiftA(parse_noise_ref(object, noises)?)),
        "shift_b" => Ok(Kind::ShiftB(parse_noise_ref(object, noises)?)),
        "shift" => Ok(Kind::Shift(parse_noise_ref(object, noises)?)),
        "distance_to_point" => {
            let point = object
                .get("point")
                .and_then(Value::as_array)
                .ok_or("missing field point")?;
            if point.len() != 3 {
                return Err("point must have three entries".to_owned());
            }
            let metric = DistanceMetric::parse(
                object
                    .get("metric")
                    .and_then(Value::as_str)
                    .ok_or("missing field metric")?,
            )?;
            Ok(Kind::DistanceToPoint {
                point: [as_i32(&point[0])?, as_i32(&point[1])?, as_i32(&point[2])?],
                metric,
            })
        }
        "gradient" => {
            let axis = Axis::parse(
                object
                    .get("axis")
                    .and_then(Value::as_str)
                    .ok_or("missing field axis")?,
            )?;
            let tiling = match object.get("tiling") {
                None => TilingMode::ClampToEdge,
                Some(value) => TilingMode::parse(value.as_str().ok_or("tiling must be a string")?)?,
            };
            let from_coordinate = required_i32(object, "from_coordinate")?;
            let to_coordinate = required_i32(object, "to_coordinate")?;
            if from_coordinate == to_coordinate {
                return Err("from_coordinate cannot be equal to to_coordinate".to_owned());
            }
            Ok(Kind::Gradient {
                axis,
                tiling,
                from_coordinate,
                to_coordinate,
                from_value: required_f32(object, "from_value")?,
                to_value: required_f32(object, "to_value")?,
            })
        }
        "abs" | "square" | "cube" | "sqrt" | "half_negative" | "quarter_negative"
        | "reciprocal" | "negate" | "squeeze" | "log" | "sign" => {
            let op = match short {
                "abs" => UnaryOp::Abs,
                "square" => UnaryOp::Square,
                "cube" => UnaryOp::Cube,
                "sqrt" => UnaryOp::Sqrt,
                "half_negative" => UnaryOp::HalfNegative,
                "quarter_negative" => UnaryOp::QuarterNegative,
                "reciprocal" => UnaryOp::Reciprocal,
                "negate" => UnaryOp::Negate,
                "squeeze" => UnaryOp::Squeeze,
                "log" => UnaryOp::Log,
                _ => UnaryOp::Sign,
            };
            Ok(Kind::Unary {
                op,
                input: child!("input"),
            })
        }
        "floor" | "round" | "ceil" | "truncate" => {
            let op = match short {
                "floor" => RoundOp::Floor,
                "round" => RoundOp::Round,
                "ceil" => RoundOp::Ceil,
                _ => RoundOp::Truncate,
            };
            let input = child!("input");
            let multiple = optional_child!("multiple", function(Kind::Constant(1.0)));
            Ok(Kind::Round {
                op,
                input,
                multiple,
            })
        }
        "add" | "sub" | "mul" | "div" | "min" | "max" => {
            let op = match short {
                "add" => BinaryOp::Add,
                "sub" => BinaryOp::Sub,
                "mul" => BinaryOp::Mul,
                "div" => BinaryOp::Div,
                "min" => BinaryOp::Min,
                _ => BinaryOp::Max,
            };
            Ok(Kind::Binary {
                op,
                left: child!("left"),
                right: child!("right"),
            })
        }
        "pow" => Ok(Kind::Pow {
            base: child!("base"),
            exponent: child!("exponent"),
        }),
        "spline" => {
            let spline = object.get("spline").ok_or("missing field spline")?;
            Ok(Kind::Spline(parse_spline(
                spline, raw, noises, entries, resolving,
            )?))
        }
        "lerp" => Ok(Kind::Lerp {
            alpha: child!("alpha"),
            first: child!("first"),
            second: child!("second"),
        }),
        "clamp" => {
            let input = child!("input");
            let min = required_f32(object, "min")?;
            let max = required_f32(object, "max")?;
            if max < min {
                return Err(format!(
                    "min ({min}) must be less than or equal to max ({max})"
                ));
            }
            Ok(Kind::Clamp { input, min, max })
        }
        "range_choice" => Ok(Kind::RangeChoice {
            input: child!("input"),
            min_inclusive: required_f32(object, "min_inclusive")?,
            max_exclusive: required_f32(object, "max_exclusive")?,
            when_in_range: child!("when_in_range"),
            when_out_of_range: child!("when_out_of_range"),
        }),
        "interval_select" => {
            let input = child!("input");
            let thresholds = object
                .get("thresholds")
                .and_then(Value::as_array)
                .ok_or("missing field thresholds")?
                .iter()
                .map(as_f32)
                .collect::<Result<Vec<f32>, String>>()?;
            let raw_functions = object
                .get("functions")
                .and_then(Value::as_array)
                .ok_or("missing field functions")?;
            let mut functions = Vec::with_capacity(raw_functions.len());
            for value in raw_functions {
                functions.push(parse(value, raw, noises, entries, resolving)?);
            }
            if functions.len() < 2 {
                return Err("interval_select needs at least two functions".to_owned());
            }
            if thresholds.len() + 1 != functions.len() {
                return Err(format!(
                    "expected {} thresholds for {} functions, but got {}",
                    functions.len() - 1,
                    functions.len(),
                    thresholds.len()
                ));
            }
            if thresholds.windows(2).any(|pair| pair[0] > pair[1]) {
                return Err("threshold values must be ordered from smallest to largest".to_owned());
            }
            Ok(Kind::IntervalSelect {
                input,
                thresholds,
                functions,
            })
        }
        "cache" => Ok(Kind::Cache(child!("input"))),
        "blend_density" => Ok(Kind::BlendDensity(child!("input"))),
        "interpolated" => {
            let input = child!("input");
            let cell_size_xz = required_i32(object, "cell_size_xz")?;
            let cell_size_y = required_i32(object, "cell_size_y")?;
            if cell_size_xz <= 0 || cell_size_y <= 0 {
                return Err("cell sizes must be positive".to_owned());
            }
            Ok(Kind::Interpolated {
                input,
                cell_size_xz,
                cell_size_y,
            })
        }
        "slice" => {
            let axis = Axis::parse(
                object
                    .get("axis")
                    .and_then(Value::as_str)
                    .ok_or("missing field axis")?,
            )?;
            let coordinate = required_i32(object, "coordinate")?;
            Ok(Kind::Slice {
                axis,
                coordinate,
                input: child!("input"),
            })
        }
        "find_top_surface" => {
            let density = child!("density");
            let upper_bound = child!("upper_bound");
            let cell_height = required_i32(object, "cell_height")?;
            if cell_height <= 0 {
                return Err("cell_height must be positive".to_owned());
            }
            Ok(Kind::FindTopSurface {
                density,
                upper_bound,
                lower_bound: required_i32(object, "lower_bound")?,
                cell_height,
            })
        }
        "old_blended_noise" => Ok(Kind::OldBlendedNoise(BlendedNoiseParameters {
            xz_scale: required_f64(object, "xz_scale")?,
            y_scale: required_f64(object, "y_scale")?,
            xz_factor: required_f64(object, "xz_factor")?,
            y_factor: required_f64(object, "y_factor")?,
            smear_scale_multiplier: required_f64(object, "smear_scale_multiplier")?,
        })),
        _ => Err(format!("unknown density function type {kind_name}")),
    }
}

/// `CubicSpline.codec`：数字是常数样条，对象是多点样条。
fn parse_spline(
    value: &Value,
    raw: &serde_json::Map<String, Value>,
    noises: &NoiseRegistry,
    entries: &mut HashMap<String, Ref>,
    resolving: &mut HashSet<String>,
) -> Result<Spline<Ref>, String> {
    if value.is_number() {
        return Ok(Spline::Constant(as_f32(value)?));
    }
    let object = value.as_object().ok_or("expected a spline")?;
    let coordinate_value = object.get("coordinate").ok_or("missing field coordinate")?;
    let coordinate = parse(coordinate_value, raw, noises, entries, resolving)?;
    let points = object
        .get("points")
        .and_then(Value::as_array)
        .ok_or("missing field points")?;
    if points.is_empty() {
        return Err("cannot create a multipoint spline with no points".to_owned());
    }
    let mut locations = Vec::with_capacity(points.len());
    let mut values = Vec::with_capacity(points.len());
    let mut derivatives = Vec::with_capacity(points.len());
    for point in points {
        let point = point
            .as_object()
            .ok_or("a spline point must be an object")?;
        locations.push(required_f32(point, "location")?);
        derivatives.push(required_f32(point, "derivative")?);
        let sub = point.get("value").ok_or("missing field value")?;
        values.push(parse_spline(sub, raw, noises, entries, resolving)?);
    }
    Ok(Spline::Multipoint(Multipoint {
        coordinate,
        locations,
        values,
        derivatives,
    }))
}

fn parse_noise_ref(
    object: &serde_json::Map<String, Value>,
    noises: &NoiseRegistry,
) -> Result<NoiseHolder, String> {
    let id = object
        .get("noise")
        .and_then(Value::as_str)
        .ok_or("missing field noise")?;
    let noise = noises
        .get(id)
        .ok_or_else(|| format!("unknown noise {id}"))?;
    Ok(NoiseHolder {
        id: id.to_owned(),
        noise: Arc::clone(noise),
    })
}

fn as_f32(value: &Value) -> Result<f32, String> {
    value
        .as_f64()
        .map(|number| number as f32)
        .ok_or_else(|| format!("expected a number, got {value}"))
}

fn as_i32(value: &Value) -> Result<i32, String> {
    value
        .as_i64()
        .and_then(|number| i32::try_from(number).ok())
        .ok_or_else(|| format!("expected an int, got {value}"))
}

fn required_f32(object: &serde_json::Map<String, Value>, name: &str) -> Result<f32, String> {
    as_f32(
        object
            .get(name)
            .ok_or_else(|| format!("missing field {name}"))?,
    )
}

fn required_f64(object: &serde_json::Map<String, Value>, name: &str) -> Result<f64, String> {
    object
        .get(name)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("missing field {name}"))
}

fn optional_f64(
    object: &serde_json::Map<String, Value>,
    name: &str,
) -> Result<Option<f64>, String> {
    match object.get(name) {
        None => Ok(None),
        Some(value) => value
            .as_f64()
            .map(Some)
            .ok_or_else(|| format!("field {name} must be a number")),
    }
}

fn required_i32(object: &serde_json::Map<String, Value>, name: &str) -> Result<i32, String> {
    as_i32(
        object
            .get(name)
            .ok_or_else(|| format!("missing field {name}"))?,
    )
}

fn optional_i32(
    object: &serde_json::Map<String, Value>,
    name: &str,
) -> Result<Option<i32>, String> {
    match object.get(name) {
        None => Ok(None),
        Some(value) => as_i32(value).map(Some),
    }
}

/// `Math.min(float, float)` 的转发，采样器要用。
pub fn min(a: f32, b: f32) -> f32 {
    java_min(a, b)
}

/// `Math.max(float, float)` 的转发。
pub fn max(a: f32, b: f32) -> f32 {
    java_max(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registries(noise: &str, functions: &str) -> (NoiseRegistry, FunctionRegistry) {
        let noise: Value = serde_json::from_str(noise).expect("合法的 JSON");
        let functions: Value = serde_json::from_str(functions).expect("合法的 JSON");
        let noises = NoiseRegistry::load(noise.as_object().expect("对象")).expect("噪声能解析");
        let functions =
            FunctionRegistry::load(functions.as_object().expect("对象"), &noises).expect("能解析");
        (noises, functions)
    }

    #[test]
    fn bare_numbers_are_constants() {
        let (_, functions) = registries("{}", r#"{"minecraft:zero": 0.0}"#);
        let zero = functions.get("minecraft:zero").expect("有这一条");
        assert_eq!(zero.constant(), Some(0.0));
        assert_eq!(zero.axes(), 0);
    }

    #[test]
    fn references_share_one_node() {
        let (_, functions) = registries(
            "{}",
            r#"{
                "a": 1.0,
                "b": {"type": "minecraft:add", "left": "a", "right": "a"}
            }"#,
        );
        let b = functions.get("b").expect("有这一条");
        let Kind::Binary { left, right, .. } = &b.kind else {
            panic!("b 是二元函数");
        };
        assert!(Arc::ptr_eq(left, right));
        assert_eq!(b.range(), Interval::of_exact(2.0));
    }

    #[test]
    fn self_references_are_rejected() {
        let noise: Value = serde_json::from_str("{}").expect("合法的 JSON");
        let functions: Value =
            serde_json::from_str(r#"{"a": {"type": "minecraft:abs", "input": "a"}}"#)
                .expect("合法的 JSON");
        let noises = NoiseRegistry::load(noise.as_object().expect("对象")).expect("能解析");
        let error = FunctionRegistry::load(functions.as_object().expect("对象"), &noises)
            .expect_err("自引用要报错");
        assert!(error.contains("references itself"), "{error}");
    }

    #[test]
    fn gradient_range_covers_both_ends() {
        let (_, functions) = registries(
            "{}",
            r#"{"g": {
                "type": "minecraft:gradient", "axis": "y",
                "from_coordinate": -64, "to_coordinate": 320,
                "from_value": 1.5, "to_value": -1.5
            }}"#,
        );
        let gradient = functions.get("g").expect("有这一条");
        assert_eq!(gradient.range(), Interval::of(-1.5, 1.5));
        assert_eq!(gradient.axes(), AXIS_Y);
    }

    /// `noise` 的定义域按两个系数去掉对应的轴。
    #[test]
    fn noise_axes_drop_the_zero_scales() {
        let (_, functions) = registries(
            r#"{"minecraft:n": {"base_octave": -3, "octave_count": 1}}"#,
            r#"{
                "flat": {"type": "minecraft:noise", "noise": "minecraft:n",
                    "xz_scale": 1.0, "y_scale": 0.0},
                "column": {"type": "minecraft:noise", "noise": "minecraft:n",
                    "xz_scale": 0.0, "y_scale": 1.0}
            }"#,
        );
        assert_eq!(
            functions.get("flat").expect("有这一条").axes(),
            AXIS_X | AXIS_Z
        );
        assert_eq!(functions.get("column").expect("有这一条").axes(), AXIS_Y);
    }

    #[test]
    fn slice_removes_its_axis() {
        let (_, functions) = registries(
            "{}",
            r#"{"s": {"type": "minecraft:slice", "axis": "y", "coordinate": 0,
                "input": {"type": "minecraft:beardifier"}}}"#,
        );
        assert_eq!(
            functions.get("s").expect("有这一条").axes(),
            AXIS_X | AXIS_Z
        );
    }

    #[test]
    fn unknown_types_are_rejected() {
        let noise: Value = serde_json::from_str("{}").expect("合法的 JSON");
        let functions: Value =
            serde_json::from_str(r#"{"a": {"type": "minecraft:nope"}}"#).expect("合法的 JSON");
        let noises = NoiseRegistry::load(noise.as_object().expect("对象")).expect("能解析");
        let error = FunctionRegistry::load(functions.as_object().expect("对象"), &noises)
            .expect_err("未知类型要报错");
        assert!(error.contains("unknown density function type"), "{error}");
    }

    /// `interval_select` 的阈值个数要比函数个数少一个。
    #[test]
    fn interval_select_checks_the_threshold_count() {
        let noise: Value = serde_json::from_str("{}").expect("合法的 JSON");
        let functions: Value = serde_json::from_str(
            r#"{"a": {"type": "minecraft:interval_select", "input": 0.0,
                "thresholds": [0.0, 1.0], "functions": [1.0, 2.0]}}"#,
        )
        .expect("合法的 JSON");
        let noises = NoiseRegistry::load(noise.as_object().expect("对象")).expect("能解析");
        let error = FunctionRegistry::load(functions.as_object().expect("对象"), &noises)
            .expect_err("阈值个数不对要报错");
        assert!(error.contains("thresholds"), "{error}");
    }
}
