//! 数字提供器，对应官方的 `ContextIntProvider`、`ContextFloatProvider` 和
//! `LevelBasedValue`。
//!
//! 官方在 `getInt` / `getFloat` 里把算术异常吞成 0，`getIntUnsafe` 才往外抛。
//! 这里用 [`NumberError`] 区分「算术错误」和「本实现不支持」，前者按官方吞掉，
//! 后者照常作为 [`LootError`] 返回。
//!
//! 取随机数的顺序和官方一致：聚合按 `inputs` 顺序、离散按 `cases` 顺序。

use std::sync::OnceLock;

use mc_bamboo_data::loot;
use serde_json::Value;

use crate::condition::LootCondition;
use crate::context::LootContext;
use crate::json::{
    array, f32_value, field, i32_value, invalid, object, opt_i32, required, type_name,
};
use crate::{LootError, Result};

/// 算术错误，对应官方的 `ArithmeticException`。
enum NumberError {
    Arithmetic,
    Loot(LootError),
}

impl From<LootError> for NumberError {
    fn from(error: LootError) -> Self {
        Self::Loot(error)
    }
}

type NumResult<T> = std::result::Result<T, NumberError>;

fn arithmetic() -> NumberError {
    NumberError::Arithmetic
}

/// 整数提供器。
#[derive(Debug, Clone)]
pub enum IntProvider {
    Constant(i32),
    Uniform(Box<Self>, Box<Self>),
    Binomial { n: Box<Self>, p: Box<FloatProvider> },
    Conditional {
        /// box 起来是为了断开和 [`LootCondition`] 的循环包含。
        condition: Box<LootCondition>,
        on_true: Box<Self>,
        on_false: Box<Self>,
    },
    Dispatcher {
        cases: Vec<(Box<LootCondition>, Self)>,
        default: Box<Self>,
    },
    WeightedList(Vec<(Self, i32)>),
    Aggregate(AggregateOp, Vec<Self>),
    Unary(UnaryOp, Box<Self>),
    Binary(BinaryOp, Box<Self>, Box<Self>),
    Power(Box<Self>, Box<Self>),
    FromFloat(Box<FloatProvider>),
    /// 依赖积分板、命令存储或环境属性，本实现没有这些数据。
    Unsupported(&'static str),
}

/// 浮点提供器。
#[derive(Debug, Clone)]
pub enum FloatProvider {
    Constant(f32),
    Uniform(Box<Self>, Box<Self>),
    Conditional {
        /// box 起来是为了断开和 [`LootCondition`] 的循环包含。
        condition: Box<LootCondition>,
        on_true: Box<Self>,
        on_false: Box<Self>,
    },
    Dispatcher {
        cases: Vec<(Box<LootCondition>, Self)>,
        default: Box<Self>,
    },
    WeightedList(Vec<(Self, i32)>),
    Aggregate(AggregateOp, Vec<Self>),
    Unary(UnaryOp, Box<Self>),
    Binary(BinaryOp, Box<Self>, Box<Self>),
    Power(Box<Self>, Box<Self>),
    EnchantmentLevel(LevelBasedValue),
    FromInt(Box<IntProvider>),
    Unsupported(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateOp {
    Sum,
    Average,
    Product,
    Min,
    Max,
    Length,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Abs,
    Negate,
    Floor,
    Ceil,
    Round,
    Truncate,
    SquareRoot,
    Sin,
    Cos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Sub,
    Mul,
    Div,
    Mod,
    FloorDiv,
    FloorMod,
}

/// 附魔数值的等级公式，对应官方 `LevelBasedValue`。
#[derive(Debug, Clone)]
pub enum LevelBasedValue {
    Constant(f32),
    Linear {
        base: f32,
        per_level_above_first: f32,
    },
    Lookup {
        values: Vec<f32>,
        fallback: Box<Self>,
    },
    Clamped {
        value: Box<Self>,
        min: f32,
        max: f32,
    },
    Exponent {
        base: Box<Self>,
        power: Box<Self>,
    },
    Fraction {
        numerator: Box<Self>,
        denominator: Box<Self>,
    },
    LevelsSquared {
        added: f32,
    },
    Unsupported(&'static str),
}

impl IntProvider {
    /// `ContextIntProvider.getInt`：算术错误变成 0，和官方一样。
    pub fn get(&self, context: &LootContext<'_, '_, '_>) -> Result<i32> {
        match self.get_unsafe(context) {
            Ok(value) => Ok(value),
            Err(NumberError::Arithmetic) => Ok(0),
            Err(NumberError::Loot(error)) => Err(error),
        }
    }

    fn get_unsafe(&self, context: &LootContext<'_, '_, '_>) -> NumResult<i32> {
        match self {
            Self::Constant(value) => Ok(*value),
            Self::Uniform(min, max) => {
                let min = min.get_unsafe(context)?;
                let max = max.get_unsafe(context)?;
                Ok(context.random().mth_next_int(min, max))
            }
            Self::Binomial { n, p } => {
                let count = n.get_unsafe(context)?;
                let probability = p.get_float_or_throw(context)?;
                let random = context.random();
                let mut result = 0;
                for _ in 0..count {
                    if random.next_float() < probability {
                        result += 1;
                    }
                }
                Ok(result)
            }
            Self::Conditional {
                condition,
                on_true,
                on_false,
            } => {
                let value = if condition.test(context)? {
                    on_true
                } else {
                    on_false
                };
                value.get_unsafe(context)
            }
            Self::Dispatcher { cases, default } => {
                for (condition, value) in cases {
                    if condition.test(context)? {
                        return value.get_unsafe(context);
                    }
                }
                default.get_unsafe(context)
            }
            Self::WeightedList(values) => {
                let index = pick_weighted(
                    context,
                    values.iter().map(|(_, weight)| *weight).collect(),
                )?;
                values[index].0.get_unsafe(context)
            }
            Self::Aggregate(op, inputs) => {
                let mut value: i64 = match op {
                    AggregateOp::Product => 1,
                    AggregateOp::Min => i64::from(i32::MAX),
                    AggregateOp::Max => i64::from(i32::MIN),
                    _ => 0,
                };
                let mut count: i64 = 0;
                for input in inputs {
                    let item = i64::from(input.get_unsafe(context)?);
                    match op {
                        AggregateOp::Sum | AggregateOp::Average => value = value.wrapping_add(item),
                        AggregateOp::Product => value = value.wrapping_mul(item),
                        AggregateOp::Min => value = value.min(item),
                        AggregateOp::Max => value = value.max(item),
                        AggregateOp::Length => {}
                    }
                    count += 1;
                }
                let result = match op {
                    AggregateOp::Average => {
                        if count == 0 {
                            return Err(arithmetic());
                        }
                        value / count
                    }
                    _ => value,
                };
                long_to_int(result)
            }
            Self::Unary(op, input) => {
                let value = input.get_unsafe(context)?;
                match op {
                    UnaryOp::Abs => value.checked_abs().ok_or_else(arithmetic),
                    UnaryOp::Negate => value.checked_neg().ok_or_else(arithmetic),
                    // 取整和三角函数只在浮点提供器里出现。
                    _ => Err(arithmetic()),
                }
            }
            Self::Binary(op, left, right) => {
                let left = left.get_unsafe(context)?;
                let right = right.get_unsafe(context)?;
                binary_int(*op, left, right)
            }
            Self::Power(base, exponent) => {
                let base = base.get_unsafe(context)?;
                let exponent = exponent.get_unsafe(context)?;
                if base == 0 && exponent == 0 {
                    return Err(arithmetic());
                }
                pow_exact(base, exponent)
            }
            Self::FromFloat(input) => {
                let value = input.get_float_unsafe(context)?;
                float_to_int(value)
            }
            Self::Unsupported(what) => Err(NumberError::Loot(LootError::Unsupported(
                (*what).to_owned(),
            ))),
        }
    }
}

impl FloatProvider {
    /// `ContextFloatProvider.getFloat`：非有限值和算术错误都变成 0。
    pub fn get(&self, context: &LootContext<'_, '_, '_>) -> Result<f32> {
        match self.get_float_unsafe(context) {
            Ok(value) if value.is_finite() => Ok(value),
            Ok(_) | Err(NumberError::Arithmetic) => Ok(0.0),
            Err(NumberError::Loot(error)) => Err(error),
        }
    }

    /// `ContextFloatProvider.getFloatOrThrow`。
    fn get_float_or_throw(&self, context: &LootContext<'_, '_, '_>) -> NumResult<f32> {
        let value = self.get_float_unsafe(context)?;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(arithmetic())
        }
    }

    fn get_float_unsafe(&self, context: &LootContext<'_, '_, '_>) -> NumResult<f32> {
        match self {
            Self::Constant(value) => Ok(*value),
            Self::Uniform(min, max) => {
                let min = min.get_float_unsafe(context)?;
                let max = max.get_float_unsafe(context)?;
                Ok(context.random().mth_next_float(min, max))
            }
            Self::Conditional {
                condition,
                on_true,
                on_false,
            } => {
                let value = if condition.test(context)? {
                    on_true
                } else {
                    on_false
                };
                value.get_float_unsafe(context)
            }
            Self::Dispatcher { cases, default } => {
                for (condition, value) in cases {
                    if condition.test(context)? {
                        return value.get_float_unsafe(context);
                    }
                }
                default.get_float_unsafe(context)
            }
            Self::WeightedList(values) => {
                let index = pick_weighted(
                    context,
                    values.iter().map(|(_, weight)| *weight).collect(),
                )?;
                values[index].0.get_float_unsafe(context)
            }
            Self::Aggregate(op, inputs) => {
                let mut value = match op {
                    AggregateOp::Product => 1.0,
                    AggregateOp::Min => f32::MAX,
                    AggregateOp::Max => -f32::MAX,
                    _ => 0.0,
                };
                let mut count = 0_i32;
                for input in inputs {
                    let item = input.get_float_unsafe(context)?;
                    match op {
                        AggregateOp::Sum | AggregateOp::Average => value += item,
                        AggregateOp::Product => value *= item,
                        AggregateOp::Min => value = java_min(value, item),
                        AggregateOp::Max => value = java_max(value, item),
                        AggregateOp::Length => value += item * item,
                    }
                    count += 1;
                }
                match op {
                    AggregateOp::Average => Ok(value / count as f32),
                    AggregateOp::Length => Ok(mth_sqrt(value)),
                    _ => Ok(value),
                }
            }
            Self::Unary(op, input) => {
                let value = input.get_float_unsafe(context)?;
                Ok(match op {
                    UnaryOp::Abs => value.abs(),
                    UnaryOp::Negate => -value,
                    UnaryOp::Floor => value.floor(),
                    UnaryOp::Ceil => mth_ceil(value),
                    UnaryOp::Round => java_round(value) as f32,
                    UnaryOp::Truncate => {
                        if value > 0.0 {
                            value.floor()
                        } else {
                            value.ceil()
                        }
                    }
                    UnaryOp::SquareRoot => mth_sqrt(value),
                    UnaryOp::Sin => mth_sin(f64::from(value)),
                    UnaryOp::Cos => mth_cos(f64::from(value)),
                })
            }
            Self::Binary(op, left, right) => {
                let left = left.get_float_unsafe(context)?;
                let right = right.get_float_unsafe(context)?;
                Ok(match op {
                    BinaryOp::Sub => left - right,
                    BinaryOp::Mul => left * right,
                    BinaryOp::Div => left / right,
                    BinaryOp::Mod => {
                        if right == 0.0 {
                            f32::NAN
                        } else {
                            left % right
                        }
                    }
                    // 整数专有的操作不会出现在浮点数据里。
                    BinaryOp::FloorDiv | BinaryOp::FloorMod => {
                        return Err(arithmetic());
                    }
                })
            }
            Self::Power(base, exponent) => {
                let base = base.get_float_unsafe(context)?;
                let exponent = exponent.get_float_unsafe(context)?;
                if base == 0.0 && exponent == 0.0 {
                    Ok(f32::NAN)
                } else {
                    Ok(f64::powf(f64::from(base), f64::from(exponent)) as f32)
                }
            }
            Self::EnchantmentLevel(value) => Ok(value.calculate(context.enchantment_level())),
            Self::FromInt(input) => Ok(input.get_unsafe(context)? as f32),
            Self::Unsupported(what) => Err(NumberError::Loot(LootError::Unsupported(
                (*what).to_owned(),
            ))),
        }
    }
}

impl LevelBasedValue {
    pub fn calculate(&self, level: i32) -> f32 {
        match self {
            Self::Constant(value) => *value,
            Self::Linear {
                base,
                per_level_above_first,
            } => *base + *per_level_above_first * (level - 1) as f32,
            Self::Lookup { values, fallback } => {
                // 官方直接取 `level - 1`，等级不合法时数据本身就有问题。
                usize::try_from(level - 1)
                    .ok()
                    .filter(|index| *index < values.len())
                    .map(|index| values[index])
                    .unwrap_or_else(|| fallback.calculate(level))
            }
            Self::Clamped { value, min, max } => {
                let value = value.calculate(level);
                if value < *min {
                    *min
                } else {
                    value.min(*max)
                }
            }
            Self::Exponent { base, power } => {
                f64::powf(f64::from(base.calculate(level)), f64::from(power.calculate(level)))
                    as f32
            }
            Self::Fraction {
                numerator,
                denominator,
            } => {
                let denominator = denominator.calculate(level);
                if denominator == 0.0 {
                    0.0
                } else {
                    numerator.calculate(level) / denominator
                }
            }
            Self::LevelsSquared { added } => (level.wrapping_mul(level) as f32) + *added,
            Self::Unsupported(_) => f32::NAN,
        }
    }
}

/// 按权重取一个下标。官方 `WeightedList.getRandomOrThrow` 从总权重里取一次数，
/// 然后按顺序减，第一个减成负数的是结果。
fn pick_weighted(
    context: &LootContext<'_, '_, '_>,
    weights: Vec<i32>,
) -> NumResult<usize> {
    let total: i32 = weights.iter().fold(0_i32, |sum, weight| sum.wrapping_add(*weight));
    if total <= 0 {
        return Err(NumberError::Loot(LootError::Invalid(
            "weighted list has no positive total weight".to_owned(),
        )));
    }
    let mut index = context.random().next_int(total);
    for (position, weight) in weights.iter().enumerate() {
        index -= *weight;
        if index < 0 {
            return Ok(position);
        }
    }
    Err(NumberError::Loot(LootError::Invalid(
        "weighted list selection fell through".to_owned(),
    )))
}

fn long_to_int(value: i64) -> NumResult<i32> {
    i32::try_from(value).map_err(|_| arithmetic())
}

fn float_to_int(value: f32) -> NumResult<i32> {
    if !value.is_finite() {
        return Err(arithmetic());
    }
    long_to_int(value as i64)
}

/// `Math.floorMod`：除数为 0 抛异常，`MIN / -1` 得到 0。
fn floor_mod(left: i32, right: i32) -> NumResult<i32> {
    if right == 0 {
        return Err(arithmetic());
    }
    if right == -1 {
        return Ok(0);
    }
    Ok(left.rem_euclid(right))
}

/// `Math.floorDivExact`：除数为 0 或 `MIN / -1` 抛异常。
fn floor_div(left: i32, right: i32) -> NumResult<i32> {
    if right == 0 {
        return Err(arithmetic());
    }
    if left == i32::MIN && right == -1 {
        return Err(arithmetic());
    }
    Ok(left.div_euclid(right))
}

fn binary_int(op: BinaryOp, left: i32, right: i32) -> NumResult<i32> {
    match op {
        BinaryOp::Sub => left.checked_sub(right).ok_or_else(arithmetic),
        BinaryOp::Mul => left.checked_mul(right).ok_or_else(arithmetic),
        BinaryOp::Div => {
            if right == 0 || (left == i32::MIN && right == -1) {
                Err(arithmetic())
            } else {
                Ok(left / right)
            }
        }
        BinaryOp::Mod => {
            if right == 0 {
                Err(arithmetic())
            } else {
                Ok(left.wrapping_rem(right))
            }
        }
        BinaryOp::FloorDiv => floor_div(left, right),
        BinaryOp::FloorMod => floor_mod(left, right),
    }
}

/// `Math.powExact`：负指数抛异常，中间乘法按官方源码用普通乘法（会回绕），
/// 最后一次乘法才检查溢出。
fn pow_exact(value: i32, exponent: i32) -> NumResult<i32> {
    if exponent < 0 {
        return Err(arithmetic());
    }
    if exponent == 0 {
        return Ok(1);
    }
    if value == 0 || value == 1 {
        return Ok(value);
    }
    if value == -1 {
        return Ok(if exponent & 1 == 0 { 1 } else { -1 });
    }
    let mut base = value;
    let mut power = exponent;
    let mut product = 1_i32;
    while power > 1 {
        if power & 1 != 0 {
            product = product.wrapping_mul(base);
        }
        base = base.checked_mul(base).ok_or_else(arithmetic)?;
        power = (power as u32 >> 1) as i32;
    }
    product.checked_mul(base).ok_or_else(arithmetic)
}

// 官方 `Mth` 的浮点辅助，NaN 和零的符号按 Java 的规则处理。

fn java_max(left: f32, right: f32) -> f32 {
    if left.is_nan() || right.is_nan() {
        return f32::NAN;
    }
    if left == 0.0 && right == 0.0 {
        // Java 的 `Math.max` 在 -0.0 和 +0.0 里返回 +0.0。
        return 0.0;
    }
    if left >= right {
        left
    } else {
        right
    }
}

fn java_min(left: f32, right: f32) -> f32 {
    if left.is_nan() || right.is_nan() {
        return f32::NAN;
    }
    if left == 0.0 && right == 0.0 {
        return -0.0;
    }
    if left <= right {
        left
    } else {
        right
    }
}

fn mth_ceil(value: f32) -> f32 {
    value.ceil() as i32 as f32
}

/// `Math.round(float)`：加 0.5 后向下取整，超出 int 时饱和。
fn java_round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}

fn mth_sqrt(value: f32) -> f32 {
    value.sqrt()
}

/// 官方 `Mth.sin(double)` 用的 65536 项查表。
fn sin_table() -> &'static [f32] {
    static TABLE: OnceLock<Vec<f32>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..65_536)
            .map(|index| (f64::from(index) / 10_430.378_350_470_453).sin() as f32)
            .collect()
    })
}

fn mth_sin(value: f64) -> f32 {
    let index = ((value * 10_430.378_350_470_453) as i64 & 65_535) as usize;
    sin_table()[index]
}

fn mth_cos(value: f64) -> f32 {
    let index = ((value * 10_430.378_350_470_453 + 16_384.0) as i64 & 65_535) as usize;
    sin_table()[index]
}

/// 解析整数提供器。字符串是按名字引用的提供器，数字是常量。
pub fn parse_int_provider(value: &Value, what: &str) -> Result<IntProvider> {
    parse_int_provider_at(value, what, 0)
}

fn parse_int_provider_at(value: &Value, what: &str, depth: usize) -> Result<IntProvider> {
    if depth > 32 {
        return Err(invalid(format!("{what} nests too deeply")));
    }
    if let Some(number) = value.as_i64() {
        return i32::try_from(number)
            .map(IntProvider::Constant)
            .map_err(|_| invalid(format!("{what} is out of int range")));
    }
    if let Some(name) = value.as_str() {
        let raw = loot::context_int_provider(name)
            .ok_or_else(|| invalid(format!("unknown int provider `{name}` in {what}")))?;
        let parsed: Value =
            serde_json::from_str(raw).map_err(|error| invalid(format!("{name}: {error}")))?;
        return parse_int_provider_at(&parsed, what, depth + 1);
    }
    let object = object(value, what)?;
    let kind = type_name(object, what)?;
    match kind {
        "minecraft:constant" => Ok(IntProvider::Constant(required_i32(object, "value", what)?)),
        "minecraft:uniform" => Ok(IntProvider::Uniform(
            Box::new(parse_int_provider_at(
                required(object, "min", what)?,
                "minimum",
                depth + 1,
            )?),
            Box::new(parse_int_provider_at(
                required(object, "max", what)?,
                "maximum",
                depth + 1,
            )?),
        )),
        "minecraft:binomial" => Ok(IntProvider::Binomial {
            n: Box::new(parse_int_provider_at(
                required(object, "n", what)?,
                "n",
                depth + 1,
            )?),
            p: Box::new(parse_float_provider_at(
                required(object, "p", what)?,
                "p",
                depth + 1,
            )?),
        }),
        "minecraft:conditional" => Ok(IntProvider::Conditional {
            condition: Box::new(LootCondition::parse(required(object, "condition", what)?)?),
            on_true: Box::new(parse_int_provider_at(
                required(object, "on_true", what)?,
                "on_true",
                depth + 1,
            )?),
            on_false: Box::new(match field(object, "on_false") {
                Some(value) => parse_int_provider_at(value, "on_false", depth + 1)?,
                None => IntProvider::Constant(0),
            }),
        }),
        "minecraft:number_dispatcher" => {
            let mut cases = Vec::new();
            for case in array(required(object, "cases", what)?, "cases")? {
                let case = crate::json::object(case, "case")?;
                cases.push((
                    Box::new(LootCondition::parse(required(case, "condition", "case")?)?),
                    parse_int_provider_at(
                        required(case, "value", "case")?,
                        "case value",
                        depth + 1,
                    )?,
                ));
            }
            let default = match field(object, "default") {
                Some(value) => parse_int_provider_at(value, "default", depth + 1)?,
                None => IntProvider::Constant(0),
            };
            Ok(IntProvider::Dispatcher {
                cases,
                default: Box::new(default),
            })
        }
        "minecraft:weighted_list" => Ok(IntProvider::WeightedList(parse_weighted(
            required(object, "distribution", what)?,
            |value, what| parse_int_provider_at(value, what, depth + 1),
            what,
        )?)),
        "minecraft:add" => Ok(IntProvider::Aggregate(
            AggregateOp::Sum,
            parse_provider_list(object, what, depth)?,
        )),
        "minecraft:avg" => Ok(IntProvider::Aggregate(
            AggregateOp::Average,
            parse_provider_list(object, what, depth)?,
        )),
        "minecraft:mul" => Ok(IntProvider::Aggregate(
            AggregateOp::Product,
            parse_provider_list(object, what, depth)?,
        )),
        "minecraft:min" => Ok(IntProvider::Aggregate(
            AggregateOp::Min,
            parse_provider_list(object, what, depth)?,
        )),
        "minecraft:max" => Ok(IntProvider::Aggregate(
            AggregateOp::Max,
            parse_provider_list(object, what, depth)?,
        )),
        "minecraft:sub" => Ok(IntProvider::Binary(
            BinaryOp::Sub,
            Box::new(parse_int_provider_at(
                required(object, "left", what)?,
                "left",
                depth + 1,
            )?),
            Box::new(parse_int_provider_at(
                required(object, "right", what)?,
                "right",
                depth + 1,
            )?),
        )),
        "minecraft:div" => Ok(IntProvider::Binary(
            BinaryOp::Div,
            Box::new(parse_int_provider_at(
                required(object, "left", what)?,
                "left",
                depth + 1,
            )?),
            Box::new(parse_int_provider_at(
                required(object, "right", what)?,
                "right",
                depth + 1,
            )?),
        )),
        "minecraft:mod" => Ok(IntProvider::Binary(
            BinaryOp::Mod,
            Box::new(parse_int_provider_at(
                required(object, "left", what)?,
                "left",
                depth + 1,
            )?),
            Box::new(parse_int_provider_at(
                required(object, "right", what)?,
                "right",
                depth + 1,
            )?),
        )),
        "minecraft:floor_div" => Ok(IntProvider::Binary(
            BinaryOp::FloorDiv,
            Box::new(parse_int_provider_at(
                required(object, "left", what)?,
                "left",
                depth + 1,
            )?),
            Box::new(parse_int_provider_at(
                required(object, "right", what)?,
                "right",
                depth + 1,
            )?),
        )),
        "minecraft:floor_mod" => Ok(IntProvider::Binary(
            BinaryOp::FloorMod,
            Box::new(parse_int_provider_at(
                required(object, "left", what)?,
                "left",
                depth + 1,
            )?),
            Box::new(parse_int_provider_at(
                required(object, "right", what)?,
                "right",
                depth + 1,
            )?),
        )),
        "minecraft:abs" => Ok(IntProvider::Unary(
            UnaryOp::Abs,
            Box::new(parse_int_provider_at(
                required(object, "input", what)?,
                "input",
                depth + 1,
            )?),
        )),
        "minecraft:negate" => Ok(IntProvider::Unary(
            UnaryOp::Negate,
            Box::new(parse_int_provider_at(
                required(object, "input", what)?,
                "input",
                depth + 1,
            )?),
        )),
        "minecraft:from_float" => Ok(IntProvider::FromFloat(Box::new(
            parse_float_provider_at(required(object, "input", what)?, "input", depth + 1)?,
        ))),
        "minecraft:pow" => Ok(IntProvider::Power(
            Box::new(parse_int_provider_at(
                required(object, "base", what)?,
                "base",
                depth + 1,
            )?),
            Box::new(parse_int_provider_at(
                required(object, "exponent", what)?,
                "exponent",
                depth + 1,
            )?),
        )),
        "minecraft:score" => Ok(IntProvider::Unsupported("int provider `score`")),
        "minecraft:storage" => Ok(IntProvider::Unsupported("int provider `storage`")),
        "minecraft:environment_attribute" => {
            Ok(IntProvider::Unsupported("int provider `environment_attribute`"))
        }
        other => Err(LootError::Unsupported(format!(
            "int provider `{other}` in {what}"
        ))),
    }
}

fn parse_provider_list(
    object: &serde_json::Map<String, Value>,
    what: &str,
    depth: usize,
) -> Result<Vec<IntProvider>> {
    let inputs = required(object, "inputs", what)?;
    let values: Vec<&Value> = match inputs {
        Value::Array(values) => values.iter().collect(),
        other => vec![other],
    };
    values
        .iter()
        .map(|value| parse_int_provider_at(value, "inputs", depth + 1))
        .collect()
}

/// 解析浮点提供器。
pub fn parse_float_provider(value: &Value, what: &str) -> Result<FloatProvider> {
    parse_float_provider_at(value, what, 0)
}

fn parse_float_provider_at(value: &Value, what: &str, depth: usize) -> Result<FloatProvider> {
    if depth > 32 {
        return Err(invalid(format!("{what} nests too deeply")));
    }
    if value.is_number() {
        return Ok(FloatProvider::Constant(f32_value(value, what)?));
    }
    if let Some(name) = value.as_str() {
        let raw = loot::context_float_provider(name)
            .ok_or_else(|| invalid(format!("unknown float provider `{name}` in {what}")))?;
        let parsed: Value =
            serde_json::from_str(raw).map_err(|error| invalid(format!("{name}: {error}")))?;
        return parse_float_provider_at(&parsed, what, depth + 1);
    }
    let object = object(value, what)?;
    let kind = type_name(object, what)?;
    match kind {
        "minecraft:constant" => Ok(FloatProvider::Constant(required_f32(
            object, "value", what,
        )?)),
        "minecraft:uniform" => Ok(FloatProvider::Uniform(
            Box::new(parse_float_provider_at(
                required(object, "min", what)?,
                "minimum",
                depth + 1,
            )?),
            Box::new(parse_float_provider_at(
                required(object, "max", what)?,
                "maximum",
                depth + 1,
            )?),
        )),
        "minecraft:conditional" => Ok(FloatProvider::Conditional {
            condition: Box::new(LootCondition::parse(required(object, "condition", what)?)?),
            on_true: Box::new(parse_float_provider_at(
                required(object, "on_true", what)?,
                "on_true",
                depth + 1,
            )?),
            on_false: Box::new(match field(object, "on_false") {
                Some(value) => parse_float_provider_at(value, "on_false", depth + 1)?,
                None => FloatProvider::Constant(0.0),
            }),
        }),
        "minecraft:number_dispatcher" => {
            let mut cases = Vec::new();
            for case in array(required(object, "cases", what)?, "cases")? {
                let case = crate::json::object(case, "case")?;
                cases.push((
                    Box::new(LootCondition::parse(required(case, "condition", "case")?)?),
                    parse_float_provider_at(
                        required(case, "value", "case")?,
                        "case value",
                        depth + 1,
                    )?,
                ));
            }
            let default = match field(object, "default") {
                Some(value) => parse_float_provider_at(value, "default", depth + 1)?,
                None => FloatProvider::Constant(0.0),
            };
            Ok(FloatProvider::Dispatcher {
                cases,
                default: Box::new(default),
            })
        }
        "minecraft:weighted_list" => Ok(FloatProvider::WeightedList(parse_weighted(
            required(object, "distribution", what)?,
            |value, what| parse_float_provider_at(value, what, depth + 1),
            what,
        )?)),
        "minecraft:add" => Ok(FloatProvider::Aggregate(
            AggregateOp::Sum,
            parse_float_provider_list(object, what, depth)?,
        )),
        "minecraft:avg" => Ok(FloatProvider::Aggregate(
            AggregateOp::Average,
            parse_float_provider_list(object, what, depth)?,
        )),
        "minecraft:mul" => Ok(FloatProvider::Aggregate(
            AggregateOp::Product,
            parse_float_provider_list(object, what, depth)?,
        )),
        "minecraft:min" => Ok(FloatProvider::Aggregate(
            AggregateOp::Min,
            parse_float_provider_list(object, what, depth)?,
        )),
        "minecraft:max" => Ok(FloatProvider::Aggregate(
            AggregateOp::Max,
            parse_float_provider_list(object, what, depth)?,
        )),
        "minecraft:length" => Ok(FloatProvider::Aggregate(
            AggregateOp::Length,
            parse_float_provider_list(object, what, depth)?,
        )),
        "minecraft:sub" => Ok(FloatProvider::Binary(
            BinaryOp::Sub,
            Box::new(parse_float_provider_at(
                required(object, "left", what)?,
                "left",
                depth + 1,
            )?),
            Box::new(parse_float_provider_at(
                required(object, "right", what)?,
                "right",
                depth + 1,
            )?),
        )),
        "minecraft:div" => Ok(FloatProvider::Binary(
            BinaryOp::Div,
            Box::new(parse_float_provider_at(
                required(object, "left", what)?,
                "left",
                depth + 1,
            )?),
            Box::new(parse_float_provider_at(
                required(object, "right", what)?,
                "right",
                depth + 1,
            )?),
        )),
        "minecraft:mod" => Ok(FloatProvider::Binary(
            BinaryOp::Mod,
            Box::new(parse_float_provider_at(
                required(object, "left", what)?,
                "left",
                depth + 1,
            )?),
            Box::new(parse_float_provider_at(
                required(object, "right", what)?,
                "right",
                depth + 1,
            )?),
        )),
        "minecraft:abs" => unary_float(object, UnaryOp::Abs, what, depth),
        "minecraft:negate" => unary_float(object, UnaryOp::Negate, what, depth),
        "minecraft:floor" => unary_float(object, UnaryOp::Floor, what, depth),
        "minecraft:ceil" => unary_float(object, UnaryOp::Ceil, what, depth),
        "minecraft:round" => unary_float(object, UnaryOp::Round, what, depth),
        "minecraft:truncate" => unary_float(object, UnaryOp::Truncate, what, depth),
        "minecraft:sqrt" => unary_float(object, UnaryOp::SquareRoot, what, depth),
        "minecraft:sin" => unary_float(object, UnaryOp::Sin, what, depth),
        "minecraft:cos" => unary_float(object, UnaryOp::Cos, what, depth),
        "minecraft:from_int" => Ok(FloatProvider::FromInt(Box::new(
            parse_int_provider_at(required(object, "input", what)?, "input", depth + 1)?,
        ))),
        "minecraft:pow" => Ok(FloatProvider::Power(
            Box::new(parse_float_provider_at(
                required(object, "base", what)?,
                "base",
                depth + 1,
            )?),
            Box::new(parse_float_provider_at(
                required(object, "exponent", what)?,
                "exponent",
                depth + 1,
            )?),
        )),
        "minecraft:enchantment_level" => Ok(FloatProvider::EnchantmentLevel(
            parse_level_based_value(required(object, "amount", what)?, "amount", depth + 1)?,
        )),
        "minecraft:storage" => Ok(FloatProvider::Unsupported("float provider `storage`")),
        "minecraft:environment_attribute" => Ok(FloatProvider::Unsupported(
            "float provider `environment_attribute`",
        )),
        other => Err(LootError::Unsupported(format!(
            "float provider `{other}` in {what}"
        ))),
    }
}

fn unary_float(
    object: &serde_json::Map<String, Value>,
    op: UnaryOp,
    what: &str,
    depth: usize,
) -> Result<FloatProvider> {
    Ok(FloatProvider::Unary(
        op,
        Box::new(parse_float_provider_at(
            required(object, "input", what)?,
            "input",
            depth + 1,
        )?),
    ))
}

fn parse_float_provider_list(
    object: &serde_json::Map<String, Value>,
    what: &str,
    depth: usize,
) -> Result<Vec<FloatProvider>> {
    let inputs = required(object, "inputs", what)?;
    let values: Vec<&Value> = match inputs {
        Value::Array(values) => values.iter().collect(),
        other => vec![other],
    };
    values
        .iter()
        .map(|value| parse_float_provider_at(value, "inputs", depth + 1))
        .collect()
}

/// `minecraft:weighted_list` 的 `distribution`：每项是 `{data, weight}`。
fn parse_weighted<T>(
    value: &Value,
    parse: impl Fn(&Value, &str) -> Result<T>,
    what: &str,
) -> Result<Vec<(T, i32)>> {
    array(value, what)?
        .iter()
        .map(|entry| {
            let entry = object(entry, what)?;
            let data = parse(required(entry, "data", what)?, "data")?;
            Ok((data, opt_i32(entry, "weight", what, 1)?))
        })
        .collect()
}

/// 解析 `LevelBasedValue`，普通数字是常量。
pub fn parse_level_based_value(
    value: &Value,
    what: &str,
    depth: usize,
) -> Result<LevelBasedValue> {
    if value.is_number() {
        return Ok(LevelBasedValue::Constant(f32_value(value, what)?));
    }
    let object = object(value, what)?;
    let kind = type_name(object, what)?;
    match kind {
        "minecraft:constant" => Ok(LevelBasedValue::Constant(required_f32(
            object, "value", what,
        )?)),
        "minecraft:linear" => Ok(LevelBasedValue::Linear {
            base: required_f32(object, "base", what)?,
            per_level_above_first: required_f32(object, "per_level_above_first", what)?,
        }),
        "minecraft:lookup" => Ok(LevelBasedValue::Lookup {
            values: array(required(object, "values", what)?, "values")?
                .iter()
                .map(|value| f32_value(value, "values"))
                .collect::<Result<Vec<_>>>()?,
            fallback: Box::new(parse_level_based_value(
                required(object, "fallback", what)?,
                "fallback",
                depth + 1,
            )?),
        }),
        "minecraft:clamped" => Ok(LevelBasedValue::Clamped {
            value: Box::new(parse_level_based_value(
                required(object, "value", what)?,
                "value",
                depth + 1,
            )?),
            min: required_f32(object, "min", what)?,
            max: required_f32(object, "max", what)?,
        }),
        "minecraft:exponent" => Ok(LevelBasedValue::Exponent {
            base: Box::new(parse_level_based_value(
                required(object, "base", what)?,
                "base",
                depth + 1,
            )?),
            power: Box::new(parse_level_based_value(
                required(object, "power", what)?,
                "power",
                depth + 1,
            )?),
        }),
        "minecraft:fraction" => Ok(LevelBasedValue::Fraction {
            numerator: Box::new(parse_level_based_value(
                required(object, "numerator", what)?,
                "numerator",
                depth + 1,
            )?),
            denominator: Box::new(parse_level_based_value(
                required(object, "denominator", what)?,
                "denominator",
                depth + 1,
            )?),
        }),
        "minecraft:levels_squared" => Ok(LevelBasedValue::LevelsSquared {
            added: required_f32(object, "added", what)?,
        }),
        other => Err(LootError::Unsupported(format!(
            "level based value `{other}` in {what}"
        ))),
    }
}

fn required_i32(object: &serde_json::Map<String, Value>, name: &str, what: &str) -> Result<i32> {
    i32_value(required(object, name, what)?, name)
}

fn required_f32(object: &serde_json::Map<String, Value>, name: &str, what: &str) -> Result<f32> {
    f32_value(required(object, name, what)?, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pow_exact_matches_java() {
        assert_eq!(pow_exact(2, 10).ok(), Some(1024));
        assert_eq!(pow_exact(-1, 3).ok(), Some(-1));
        assert!(pow_exact(2, -1).is_err());
        // `Math.powExact` 在指数为 0 时先返回 1，0 的 0 次方也不例外。
        assert_eq!(pow_exact(0, 0).ok(), Some(1));
    }

    #[test]
    fn floor_ops_match_java() {
        assert_eq!(floor_mod(-7, 3).ok(), Some(2));
        assert!(floor_mod(1, 0).is_err());
        assert_eq!(floor_div(-7, 3).ok(), Some(-3));
        assert!(floor_div(i32::MIN, -1).is_err());
    }

    #[test]
    fn level_values_match_official() {
        let linear = LevelBasedValue::Linear {
            base: 10.0,
            per_level_above_first: 5.0,
        };
        assert_eq!(linear.calculate(1), 10.0);
        assert_eq!(linear.calculate(3), 20.0);
        let lookup = LevelBasedValue::Lookup {
            values: vec![1.0, 2.0],
            fallback: Box::new(LevelBasedValue::Constant(9.0)),
        };
        assert_eq!(lookup.calculate(2), 2.0);
        assert_eq!(lookup.calculate(5), 9.0);
    }

    #[test]
    fn plain_numbers_are_constants() {
        let value = serde_json::json!(3);
        match parse_int_provider(&value, "test").unwrap() {
            IntProvider::Constant(3) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
}
