//! 战利品条件，对应官方的 `LootItemCondition` 体系。
//!
//! 条件在解析时就固定下来，引用别的条件（字符串）会就地展开，和官方注册表
//! 引用等价。求值的短路顺序、随机数取数次数都照官方源码，种子相同时逐位一致。
//!
//! 依赖积分板、环境属性这类本实现没有的数据的条件会在解析时记成
//! [`LootCondition::Unsupported`]，求值到那里报错，而不是悄悄按 false 算。

use std::collections::BTreeMap;

use mc_bamboo_data::loot;
use serde_json::Value;

use crate::context::{EntityTarget, LootContext};
use crate::json::{array, f32_value, field, object, required, string, type_name};
use crate::names::{self, Registry};
use crate::number::{parse_float_provider, parse_int_provider, parse_level_based_value};
use crate::predicate::{BlockPredicate, DamageSourcePredicate, EntityPredicate, ItemPredicate};
use crate::{LootError, Result};
use crate::number::{FloatProvider, IntProvider, LevelBasedValue};

/// 条件里引用别的条件或提供器时的最大展开深度，防止数据循环引用。
const MAX_DEPTH: usize = 32;

/// 一个战利品条件。
#[derive(Debug, Clone)]
pub enum LootCondition {
    /// `all_of`：按顺序求值，遇到 false 立刻停。
    AllOf(Vec<Self>),
    /// `any_of`：按顺序求值，遇到 true 立刻停。
    AnyOf(Vec<Self>),
    Inverted(Box<Self>),
    /// `table_bonus`：工具上附魔等级决定概率。
    BonusLevelTable { enchantment: i32, chances: Vec<f32> },
    DamageSource(Option<DamageSourcePredicate>),
    EnchantmentActive { active: bool },
    /// `entity_scores`：需要积分板，求值时报不支持。
    EntityScores {
        scores: BTreeMap<String, IntRangePredicate>,
        target: EntityTarget,
    },
    EntityProperties {
        predicate: Option<EntityPredicate>,
        target: EntityTarget,
    },
    KilledByPlayer,
    RandomChance(FloatProvider),
    RandomChanceWithEnchantedBonus {
        unenchanted_chance: f32,
        enchanted_chance: LevelBasedValue,
        enchantment: i32,
    },
    Explosion,
    LocationCheck {
        predicate: Option<crate::predicate::LocationPredicate>,
        offset: (i32, i32, i32),
    },
    MatchBlock(BlockPredicate),
    MatchTool(Option<ItemPredicate>),
    TimeCheck {
        clock: String,
        period: Option<i64>,
        value: IntRangePredicate,
    },
    IntValueCheck {
        value: IntProvider,
        range: IntRangePredicate,
    },
    FloatValueCheck {
        value: FloatProvider,
        range: FloatRangePredicate,
    },
    WeatherCheck {
        raining: Option<bool>,
        thundering: Option<bool>,
    },
    /// 本实现没有这个条件需要的数据，名字留着方便报错。
    Unsupported(String),
}

impl LootCondition {
    /// 解析一个条件。字符串是按名字引用的条件，会展开成内联条件。
    pub fn parse(value: &Value) -> Result<Self> {
        Self::parse_at(value, "condition", 0)
    }

    fn parse_at(value: &Value, what: &str, depth: usize) -> Result<Self> {
        if depth > MAX_DEPTH {
            return Err(LootError::Invalid(format!(
                "{what} expands too deeply"
            )));
        }
        if let Some(name) = value.as_str() {
            return parse_reference(name, what, depth + 1);
        }
        let map = object(value, what)?;
        let kind = type_name(map, what)?;
        match kind {
            "minecraft:all_of" => Ok(Self::AllOf(parse_terms(map, what, depth)?)),
            "minecraft:any_of" => Ok(Self::AnyOf(parse_terms(map, what, depth)?)),
            "minecraft:inverted" => Ok(Self::Inverted(Box::new(Self::parse_at(
                required(map, "term", what)?,
                "term",
                depth + 1,
            )?))),
            "minecraft:table_bonus" => Ok(Self::BonusLevelTable {
                enchantment: names::single_registry_id(
                    required(map, "enchantment", what)?,
                    Registry::Enchantment,
                    what,
                )?,
                chances: parse_chances(required(map, "chances", what)?, what)?,
            }),
            "minecraft:damage_source_properties" => Ok(Self::DamageSource(
                field(map, "predicate")
                    .map(|value| DamageSourcePredicate::parse(value, what))
                    .transpose()?,
            )),
            "minecraft:enchantment_active_check" => {
                let active = required(map, "active", what)?
                    .as_bool()
                    .ok_or_else(|| LootError::Invalid(format!("{what} active must be a bool")))?;
                Ok(Self::EnchantmentActive { active })
            }
            "minecraft:entity_scores" => {
                let scores = object(required(map, "scores", what)?, "scores")?
                    .iter()
                    .map(|(name, value)| {
                        Ok((name.clone(), IntRangePredicate::parse(value, "score")?))
                    })
                    .collect::<Result<BTreeMap<_, _>>>()?;
                let target = parse_entity_target(map, what)?;
                Ok(Self::EntityScores { scores, target })
            }
            "minecraft:entity_properties" => Ok(Self::EntityProperties {
                predicate: field(map, "predicate")
                    .map(|value| EntityPredicate::parse(value, what))
                    .transpose()?,
                target: parse_entity_target(map, what)?,
            }),
            "minecraft:killed_by_player" => Ok(Self::KilledByPlayer),
            "minecraft:random_chance" => Ok(Self::RandomChance(parse_float_provider(
                required(map, "chance", what)?,
                "chance",
            )?)),
            "minecraft:random_chance_with_enchanted_bonus" => {
                let unenchanted_chance = f32_value(
                    required(map, "unenchanted_chance", what)?,
                    "unenchanted_chance",
                )?;
                let enchanted_chance = parse_level_based_value(
                    required(map, "enchanted_chance", what)?,
                    "enchanted_chance",
                    depth + 1,
                )?;
                let enchantment = names::single_registry_id(
                    required(map, "enchantment", what)?,
                    Registry::Enchantment,
                    what,
                )?;
                Ok(Self::RandomChanceWithEnchantedBonus {
                    unenchanted_chance,
                    enchanted_chance,
                    enchantment,
                })
            }
            "minecraft:survives_explosion" => Ok(Self::Explosion),
            "minecraft:location_check" => {
                let offset = (
                    off_i32(map, "offsetX", what)?,
                    off_i32(map, "offsetY", what)?,
                    off_i32(map, "offsetZ", what)?,
                );
                Ok(Self::LocationCheck {
                    predicate: field(map, "predicate")
                        .map(|value| crate::predicate::LocationPredicate::parse(value, what))
                        .transpose()?,
                    offset,
                })
            }
            "minecraft:match_block" => Ok(Self::MatchBlock(BlockPredicate::parse(value, what)?)),
            "minecraft:match_tool" => Ok(Self::MatchTool(
                field(map, "predicate")
                    .map(|value| ItemPredicate::parse(value, what))
                    .transpose()?,
            )),
            "minecraft:time_check" => {
                let clock = string(required(map, "clock", what)?, "clock")?.to_owned();
                let period = field(map, "period")
                    .map(|value| {
                        value.as_i64().ok_or_else(|| {
                            LootError::Invalid(format!("{what} period must be a long"))
                        })
                    })
                    .transpose()?;
                Ok(Self::TimeCheck {
                    clock,
                    period,
                    value: IntRangePredicate::parse(
                        required(map, "value", what)?,
                        "time value",
                    )?,
                })
            }
            "minecraft:int_value_check" => Ok(Self::IntValueCheck {
                value: parse_int_provider(required(map, "value", what)?, "value")?,
                range: IntRangePredicate::parse(required(map, "test", what)?, "test")?,
            }),
            "minecraft:float_value_check" => Ok(Self::FloatValueCheck {
                value: parse_float_provider(required(map, "value", what)?, "value")?,
                range: FloatRangePredicate::parse(required(map, "test", what)?, "test")?,
            }),
            "minecraft:weather_check" => Ok(Self::WeatherCheck {
                raining: optional_bool_field(map, "raining", what)?,
                thundering: optional_bool_field(map, "thundering", what)?,
            }),
            // 官方 26.3 的条件类型，本实现没有对应的世界数据。
            "minecraft:environment_attribute_check" => {
                Ok(Self::Unsupported("condition `environment_attribute_check`".to_owned()))
            }
            other => Err(LootError::Unsupported(format!(
                "loot condition `{other}` in {what}"
            ))),
        }
    }

    /// 求值。短路顺序和官方一致。
    pub fn test(&self, context: &LootContext<'_, '_, '_>) -> Result<bool> {
        match self {
            Self::AllOf(terms) => {
                for term in terms {
                    if !term.test(context)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Self::AnyOf(terms) => {
                for term in terms {
                    if term.test(context)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Self::Inverted(term) => Ok(!term.test(context)?),
            Self::BonusLevelTable {
                enchantment,
                chances,
            } => {
                let level = context
                    .params
                    .tool
                    .as_ref()
                    .map(|tool| tool.item_enchantment_level(*enchantment))
                    .unwrap_or(0);
                let index = usize::try_from(level.min(chances.len() as i32 - 1))
                    .map_err(|_| LootError::Invalid("table_bonus level is negative".to_owned()))?;
                Ok(context.random().next_float() < chances[index])
            }
            Self::DamageSource(predicate) => {
                if context.params.origin.is_none() || context.params.damage_source.is_none() {
                    return Ok(false);
                }
                match predicate {
                    None => Ok(true),
                    Some(predicate) => predicate.matches(
                        context,
                        context.params.origin,
                        context.params.damage_source.as_ref().unwrap(),
                    ),
                }
            }
            Self::EnchantmentActive { active } => {
                Ok(context.params.enchantment_active == Some(*active))
            }
            Self::EntityScores { .. } => Err(LootError::Unsupported(
                "condition `entity_scores` needs a scoreboard".to_owned(),
            )),
            Self::EntityProperties { predicate, target } => {
                let entity = target.get(context.params);
                match predicate {
                    None => Ok(true),
                    Some(predicate) => predicate.matches(context, context.params.origin, entity),
                }
            }
            Self::KilledByPlayer => Ok(context.params.last_damage_player.is_some()),
            Self::RandomChance(chance) => {
                let probability = chance.get(context)?;
                Ok(context.random().next_float() < probability)
            }
            Self::RandomChanceWithEnchantedBonus {
                unenchanted_chance,
                enchanted_chance,
                enchantment,
            } => {
                let level = context
                    .params
                    .attacking_entity
                    .as_ref()
                    .filter(|entity| entity.is_living)
                    .map(|entity| crate::context::entity_enchantment_level(entity, *enchantment))
                    .unwrap_or(0);
                let chance = if level > 0 {
                    enchanted_chance.calculate(level)
                } else {
                    *unenchanted_chance
                };
                Ok(context.random().next_float() < chance)
            }
            Self::Explosion => match context.params.explosion_radius {
                Some(radius) => Ok(context.random().next_float() <= 1.0 / radius),
                None => Ok(true),
            },
            Self::LocationCheck { predicate, offset } => {
                let Some(origin) = context.params.origin else {
                    return Ok(false);
                };
                match predicate {
                    None => Ok(true),
                    Some(predicate) => predicate.matches(
                        context,
                        origin.x + f64::from(offset.0),
                        origin.y + f64::from(offset.1),
                        origin.z + f64::from(offset.2),
                    ),
                }
            }
            Self::MatchBlock(predicate) => {
                let Some(state) = context.params.block_state else {
                    return Ok(false);
                };
                if !predicate.matches_state(state) {
                    return Ok(false);
                }
                predicate.matches_block_entity(context.params.block_entity.as_ref())
            }
            Self::MatchTool(predicate) => {
                let Some(tool) = context.params.tool.as_ref() else {
                    return Ok(false);
                };
                match predicate {
                    None => Ok(true),
                    Some(predicate) => predicate.matches(tool),
                }
            }
            Self::TimeCheck {
                clock,
                period,
                value,
            } => {
                let level = context.level("time check")?;
                let mut time = level.clock_time(clock).ok_or_else(|| {
                    LootError::Unsupported(format!("time check needs clock `{clock}`"))
                })?;
                if let Some(period) = *period {
                    if period == 0 {
                        return Err(LootError::Invalid("time check period is zero".to_owned()));
                    }
                    // Java 的 `%` 在 MIN % -1 时得 0，Rust 这里会溢出。
                    time = if period == -1 { 0 } else { time % period };
                }
                value.test(context, time as i32)
            }
            Self::IntValueCheck { value, range } => {
                let input = value.get(context)?;
                range.test(context, input)
            }
            Self::FloatValueCheck { value, range } => {
                let input = value.get(context)?;
                range.test(context, input)
            }
            Self::WeatherCheck {
                raining,
                thundering,
            } => {
                let level = context.level("weather check")?;
                if let Some(expected) = raining {
                    let actual = level.is_raining().ok_or_else(|| {
                        LootError::Unsupported(
                            "weather check needs LevelView::is_raining".to_owned(),
                        )
                    })?;
                    if actual != *expected {
                        return Ok(false);
                    }
                }
                if let Some(expected) = thundering {
                    let actual = level.is_thundering().ok_or_else(|| {
                        LootError::Unsupported(
                            "weather check needs LevelView::is_thundering".to_owned(),
                        )
                    })?;
                    if actual != *expected {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Self::Unsupported(what) => Err(LootError::Unsupported(what.clone())),
        }
    }
}

/// `IntRangePredicate`：一个提供器（Point）或者上下界（Line）。
#[derive(Debug, Clone)]
pub enum IntRangePredicate {
    Point(IntProvider),
    Line {
        min: Option<IntProvider>,
        max: Option<IntProvider>,
    },
}

impl IntRangePredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        if is_provider_value(value) {
            return Ok(Self::Point(parse_int_provider(value, what)?));
        }
        let map = object(value, what)?;
        Ok(Self::Line {
            min: field(map, "min")
                .map(|value| parse_int_provider(value, "min"))
                .transpose()?,
            max: field(map, "max")
                .map(|value| parse_int_provider(value, "max"))
                .transpose()?,
        })
    }

    /// `input` 是调用方先求好的值。官方在 Line 里先求最小值再求最大值。
    pub fn test(&self, context: &LootContext<'_, '_, '_>, input: i32) -> Result<bool> {
        match self {
            Self::Point(value) => Ok(value.get(context)? == input),
            Self::Line { min, max } => {
                if let Some(min) = min {
                    if input < min.get(context)? {
                        return Ok(false);
                    }
                }
                if let Some(max) = max {
                    if input > max.get(context)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }
}

/// `FloatRangePredicate`。
#[derive(Debug, Clone)]
pub enum FloatRangePredicate {
    Point(FloatProvider),
    Line {
        min: Option<FloatProvider>,
        max: Option<FloatProvider>,
    },
}

impl FloatRangePredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        if is_provider_value(value) {
            return Ok(Self::Point(parse_float_provider(value, what)?));
        }
        let map = object(value, what)?;
        Ok(Self::Line {
            min: field(map, "min")
                .map(|value| parse_float_provider(value, "min"))
                .transpose()?,
            max: field(map, "max")
                .map(|value| parse_float_provider(value, "max"))
                .transpose()?,
        })
    }

    pub fn test(&self, context: &LootContext<'_, '_, '_>, input: f32) -> Result<bool> {
        match self {
            Self::Point(value) => Ok(value.get(context)? == input),
            Self::Line { min, max } => {
                if let Some(min) = min {
                    if input < min.get(context)? {
                        return Ok(false);
                    }
                }
                if let Some(max) = max {
                    if input > max.get(context)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }
}

/// Point 编解码吃的是提供器：数字、名字或者带 `type` 的对象。
fn is_provider_value(value: &Value) -> bool {
    match value {
        Value::Number(_) | Value::String(_) => true,
        Value::Object(map) => map.contains_key("type"),
        _ => false,
    }
}

fn parse_terms(
    map: &serde_json::Map<String, Value>,
    what: &str,
    depth: usize,
) -> Result<Vec<LootCondition>> {
    let raw = required(map, "terms", what)?;
    match raw {
        Value::Array(values) => values
            .iter()
            .map(|value| LootCondition::parse_at(value, "term", depth + 1))
            .collect(),
        Value::String(name) if name.starts_with('#') => Err(LootError::Unsupported(format!(
            "condition tag `{name}` in {what}"
        ))),
        _ => Err(LootError::Invalid(format!("{what} terms must be an array"))),
    }
}

fn parse_chances(value: &Value, what: &str) -> Result<Vec<f32>> {
    let chances: Vec<f32> = array(value, what)?
        .iter()
        .map(|value| f32_value(value, "chance"))
        .collect::<Result<_>>()?;
    if chances.is_empty() {
        return Err(LootError::Invalid(format!("{what} chances must not be empty")));
    }
    Ok(chances)
}

fn parse_entity_target(
    map: &serde_json::Map<String, Value>,
    what: &str,
) -> Result<EntityTarget> {
    let name = string(required(map, "entity", what)?, "entity")?;
    EntityTarget::from_name(name)
        .ok_or_else(|| LootError::Invalid(format!("unknown entity target `{name}` in {what}")))
}

fn off_i32(map: &serde_json::Map<String, Value>, name: &str, what: &str) -> Result<i32> {
    match field(map, name) {
        Some(value) => value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| LootError::Invalid(format!("{what} {name} must be an int"))),
        None => Ok(0),
    }
}

fn optional_bool_field(
    map: &serde_json::Map<String, Value>,
    name: &str,
    what: &str,
) -> Result<Option<bool>> {
    field(map, name)
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| LootError::Invalid(format!("{what} {name} must be a bool")))
        })
        .transpose()
}

/// 按注册表名字展开条件引用。
fn parse_reference(name: &str, what: &str, depth: usize) -> Result<LootCondition> {
    let raw = loot::loot_predicate(name)
        .ok_or_else(|| LootError::Invalid(format!("unknown loot condition `{name}` in {what}")))?;
    let value: Value = serde_json::from_str(raw)
        .map_err(|error| LootError::Invalid(format!("{name}: {error}")))?;
    LootCondition::parse_at(&value, name, depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_blocks::random::LegacyRandom;
    use serde_json::json;

    use crate::context::{LootParams, Position};

    fn context<'a>(params: &'a LootParams<'a>, random: &'a LegacyRandom) -> LootContext<'a, 'a, 'a> {
        LootContext::new(params, random)
    }

    #[test]
    fn all_of_short_circuits_in_order() {
        let condition = LootCondition::parse(&json!({
            "type": "minecraft:any_of",
            "terms": [
                {"type": "minecraft:random_chance", "chance": 0.0},
                {"type": "minecraft:random_chance", "chance": 1.0}
            ]
        }))
        .unwrap();
        let params = LootParams::new(0.0);
        let random = LegacyRandom::new(1);
        assert!(condition.test(&context(&params, &random)).unwrap());
    }

    #[test]
    fn location_check_needs_an_origin() {
        let condition = LootCondition::parse(&json!({
            "type": "minecraft:location_check",
            "predicate": {}
        }))
        .unwrap();
        let params = LootParams::new(0.0);
        let random = LegacyRandom::new(1);
        assert!(!condition.test(&context(&params, &random)).unwrap());
    }

    #[test]
    fn references_expand_to_registered_conditions() {
        // 官方数据包里的 `minecraft:tool/can_shear` 是 `match_tool` 剪刀。
        let condition =
            LootCondition::parse(&json!("minecraft:tool/can_shear")).unwrap();
        let mut params = LootParams::new(0.0);
        let random = LegacyRandom::new(1);
        assert!(!condition.test(&context(&params, &random)).unwrap());
        let shears = names::item_id("minecraft:shears").unwrap();
        params.tool = Some(crate::stack::LootStack::new(shears, 1));
        assert!(condition.test(&context(&params, &random)).unwrap());
    }

    #[test]
    fn explosion_without_radius_always_survives() {
        let condition =
            LootCondition::parse(&json!({"type": "minecraft:survives_explosion"})).unwrap();
        let params = LootParams::new(0.0);
        let random = LegacyRandom::new(1);
        assert!(condition.test(&context(&params, &random)).unwrap());
    }

    #[test]
    fn int_value_check_evaluates_the_provider_first() {
        let condition = LootCondition::parse(&json!({
            "type": "minecraft:int_value_check",
            "value": 5,
            "test": {"min": 3}
        }))
        .unwrap();
        let params = LootParams::new(0.0).with_origin(Position::new(0.0, 0.0, 0.0));
        let random = LegacyRandom::new(1);
        assert!(condition.test(&context(&params, &random)).unwrap());
    }
}
