//! 战利品函数，对应官方的 `LootItemFunction` 体系。
//!
//! 函数只记录「要往哪个组件写什么」：组件值原样保存成 JSON，真正的组件编码
//! 由协议那边的编解码器负责。需要附魔等级的地方走 [`EnchantmentLevels`]，
//! 测试里可以换成固定表。
//!
//! 需要世界数据才能算的函数（结构定位、命令文本解析）会在求值时返回
//! [`LootError::Unsupported`]，不会悄悄按「什么都不做」处理。

use std::sync::OnceLock;

use mc_bamboo_blocks::random::RandomSource;
use mc_bamboo_data::{items, loot};
use serde_json::{json, Map, Value};

use crate::condition::LootCondition;
use crate::context::{EntityTarget, LootContext};
use crate::json::{
    array, f32_value, field, i32_value, object, opt_bool, required, string, type_name,
};
use crate::names::{self, Registry};
use crate::number::{parse_float_provider, parse_int_provider, FloatProvider, IntProvider};
use crate::predicate::ItemPredicate;
use crate::stack::{
    LootStack, ADDITIONAL_TRADE_COST, BLOCK_STATE, CUSTOM_NAME, INSTRUMENT, ITEM_NAME,
    OMINOUS_BOTTLE_AMPLIFIER, POTION_CONTENTS, SUSPICIOUS_STEW_EFFECTS,
};
use crate::{LootError, Result};

/// 嵌套函数（`sequence`、`filtered`）的最大深度，防止坏数据无限展开。
const MAX_DEPTH: usize = 32;

/// 已注册但本实现没有做的函数。遇到它们报不支持，而不是当成未知类型。
const UNSUPPORTED_FUNCTIONS: &[&str] = &[
    "minecraft:set_custom_data",
    "minecraft:set_attributes",
    "minecraft:copy_name",
    "minecraft:set_contents",
    "minecraft:modify_contents",
    "minecraft:set_loot_table",
    "minecraft:set_lore",
    "minecraft:fill_player_head",
    "minecraft:copy_custom_data",
    "minecraft:set_banner_pattern",
    "minecraft:set_random_dyes",
    "minecraft:set_random_potion",
    "minecraft:set_fireworks",
    "minecraft:set_firework_explosion",
    "minecraft:set_book_cover",
    "minecraft:set_written_book_pages",
    "minecraft:set_writable_book_pages",
    "minecraft:toggle_tooltips",
    "minecraft:set_custom_model_data",
];

/// 一个战利品函数：可选条件和函数体。
#[derive(Debug)]
pub struct LootFunction {
    condition: Option<LootCondition>,
    body: FunctionBody,
}

#[derive(Debug)]
enum FunctionBody {
    Sequence(Vec<LootFunction>),
    SetCount {
        count: IntProvider,
        add: bool,
    },
    SetItem(i32),
    /// 组件补丁：`None` 表示移除这个组件。
    SetComponents(Vec<(String, Option<Value>)>),
    ApplyBonus {
        enchantment: i32,
        formula: BonusFormula,
    },
    CopyComponents {
        source: ComponentSource,
        include: Option<Vec<String>>,
        exclude: Vec<String>,
    },
    CopyState {
        properties: Vec<String>,
    },
    Discard,
    EnchantRandomly {
        options: Option<Vec<i32>>,
        only_compatible: bool,
        include_additional_cost: bool,
    },
    EnchantWithLevels {
        levels: IntProvider,
        options: Option<Vec<i32>>,
        include_additional_cost: bool,
    },
    EnchantedCountIncrease {
        enchantment: i32,
        count: FloatProvider,
        limit: i32,
    },
    ExplosionDecay,
    Filtered {
        filter: ItemPredicate,
        on_pass: Option<Box<LootFunction>>,
        on_fail: Option<Box<LootFunction>>,
    },
    Smelt {
        use_input_count: bool,
    },
    LimitCount {
        min: Option<IntProvider>,
        max: Option<IntProvider>,
    },
    SetDamage {
        damage: FloatProvider,
        add: bool,
    },
    SetEnchantments {
        enchantments: Vec<(i32, IntProvider)>,
        add: bool,
    },
    SetInstrument(Vec<i32>),
    SetName {
        name: Option<Value>,
        target: &'static str,
        entity: Option<EntityTarget>,
    },
    SetOminousBottleAmplifier(IntProvider),
    SetPotion(i32),
    SetStewEffect(Vec<(i32, IntProvider)>),
    /// 本实现没有这个函数需要的数据。
    Unsupported(String),
}

#[derive(Debug, Clone, Copy)]
enum BonusFormula {
    Binomial { extra: i32, probability: f32 },
    OreDrops,
    Uniform { bonus_multiplier: i32 },
}

#[derive(Debug, Clone, Copy)]
enum ComponentSource {
    /// `block_entity`：方块实体收集出来的组件。
    BlockEntity,
    /// `this`、`attacker` 这些实体或物品堆来源，本实现拿不到组件映射。
    Entity,
}

impl LootFunction {
    /// 解析一个函数。字符串是按名字引用的函数，数组是内联的 `sequence`。
    pub fn parse(value: &Value) -> Result<Self> {
        Self::parse_at(value, 0)
    }

    fn parse_at(value: &Value, depth: usize) -> Result<Self> {
        if depth > MAX_DEPTH {
            return Err(LootError::Invalid(
                "function nests too deeply".to_owned(),
            ));
        }
        match value {
            Value::Array(items) => {
                let functions = items
                    .iter()
                    .map(|item| Self::parse_at(item, depth + 1))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Self {
                    condition: None,
                    body: FunctionBody::Sequence(functions),
                })
            }
            Value::String(name) => Ok(Self {
                condition: None,
                body: FunctionBody::Unsupported(format!(
                    "named function `{name}` is not resolved by this crate"
                )),
            }),
            Value::Object(map) => Self::parse_object(map, depth),
            _ => Err(LootError::Invalid(
                "function must be an object, an array or a name".to_owned(),
            )),
        }
    }

    fn parse_object(map: &Map<String, Value>, depth: usize) -> Result<Self> {
        let kind = type_name(map, "function")?;
        let condition = match field(map, "condition") {
            Some(value) => Some(LootCondition::parse(value)?),
            None => None,
        };
        let body = match kind {
            "minecraft:sequence" => FunctionBody::Sequence(
                array(required(map, "functions", kind)?, "functions")?
                    .iter()
                    .map(|value| Self::parse_at(value, depth + 1))
                    .collect::<Result<Vec<_>>>()?,
            ),
            "minecraft:set_count" => FunctionBody::SetCount {
                count: parse_int_provider(required(map, "count", kind)?, "count")?,
                add: opt_bool(map, "add", kind, false)?,
            },
            "minecraft:set_item" => FunctionBody::SetItem(names::single_registry_id(
                required(map, "item", kind)?,
                Registry::Item,
                "item",
            )?),
            "minecraft:set_components" => FunctionBody::SetComponents(parse_component_patch(
                required(map, "components", kind)?,
            )?),
            "minecraft:apply_bonus" => FunctionBody::ApplyBonus {
                enchantment: names::single_registry_id(
                    required(map, "enchantment", kind)?,
                    Registry::Enchantment,
                    "enchantment",
                )?,
                formula: parse_bonus_formula(map, kind)?,
            },
            "minecraft:copy_components" => FunctionBody::CopyComponents {
                source: parse_component_source(required(map, "source", kind)?)?,
                include: super::json::opt_string_list(map, "include", "include")?,
                exclude: super::json::opt_string_list(map, "exclude", "exclude")?.unwrap_or_default(),
            },
            "minecraft:copy_state" => {
                let block = names::single_registry_id(
                    required(map, "block", kind)?,
                    Registry::Block,
                    "block",
                )?;
                let declared = super::json::string_list(required(map, "properties", kind)?, "properties")?;
                let block = &mc_bamboo_data::blocks::BLOCKS[block as usize];
                let properties = declared
                    .into_iter()
                    .filter(|name| block.properties.iter().any(|property| property.name == *name))
                    .collect();
                FunctionBody::CopyState { properties }
            }
            "minecraft:discard" => FunctionBody::Discard,
            "minecraft:enchant_randomly" => FunctionBody::EnchantRandomly {
                options: names::opt_holder_set(map, "options", Registry::Enchantment, "options")?,
                only_compatible: opt_bool(map, "only_compatible", kind, true)?,
                include_additional_cost: opt_bool(map, "include_additional_cost_component", kind, false)?,
            },
            "minecraft:enchant_with_levels" => FunctionBody::EnchantWithLevels {
                levels: parse_int_provider(required(map, "levels", kind)?, "levels")?,
                options: names::opt_holder_set(map, "options", Registry::Enchantment, "options")?,
                include_additional_cost: opt_bool(map, "include_additional_cost_component", kind, false)?,
            },
            "minecraft:enchanted_count_increase" => FunctionBody::EnchantedCountIncrease {
                enchantment: names::single_registry_id(
                    required(map, "enchantment", kind)?,
                    Registry::Enchantment,
                    "enchantment",
                )?,
                count: parse_float_provider(required(map, "count", kind)?, "count")?,
                limit: super::json::opt_i32(map, "limit", "limit", 0)?,
            },
            "minecraft:explosion_decay" => FunctionBody::ExplosionDecay,
            "minecraft:filtered" => FunctionBody::Filtered {
                filter: ItemPredicate::parse(required(map, "item_filter", kind)?, "item_filter")?,
                on_pass: parse_optional_function(map, "on_pass", depth)?,
                on_fail: parse_optional_function(map, "on_fail", depth)?,
            },
            "minecraft:furnace_smelt" => FunctionBody::Smelt {
                use_input_count: opt_bool(map, "use_input_count", kind, true)?,
            },
            "minecraft:limit_count" => {
                let limit = object(required(map, "limit", kind)?, "limit")?;
                FunctionBody::LimitCount {
                    min: match field(limit, "min") {
                        Some(value) => Some(parse_int_provider(value, "limit min")?),
                        None => None,
                    },
                    max: match field(limit, "max") {
                        Some(value) => Some(parse_int_provider(value, "limit max")?),
                        None => None,
                    },
                }
            }
            "minecraft:set_damage" => FunctionBody::SetDamage {
                damage: parse_float_provider(required(map, "damage", kind)?, "damage")?,
                add: opt_bool(map, "add", kind, false)?,
            },
            "minecraft:set_enchantments" => {
                let mut enchantments = Vec::new();
                if let Some(value) = field(map, "enchantments") {
                    for (name, provider) in object(value, "enchantments")? {
                        let enchantment = names::enchantment_id(name).ok_or_else(|| {
                            LootError::Invalid(format!("unknown enchantment `{name}` in enchantments"))
                        })?;
                        enchantments.push((enchantment, parse_int_provider(provider, "enchantment level")?));
                    }
                }
                FunctionBody::SetEnchantments {
                    enchantments,
                    add: opt_bool(map, "add", kind, false)?,
                }
            }
            "minecraft:set_instrument" => FunctionBody::SetInstrument(names::holder_set(
                required(map, "options", kind)?,
                Registry::Instrument,
                "options",
            )?),
            "minecraft:set_name" => {
                let target = match super::json::opt_string(map, "target", "target")? {
                    Some("custom_name") | None => CUSTOM_NAME,
                    Some("item_name") => ITEM_NAME,
                    Some(other) => {
                        return Err(LootError::Invalid(format!(
                            "unknown set_name target `{other}`"
                        )))
                    }
                };
                let entity = match super::json::opt_string(map, "entity", "entity")? {
                    Some(name) => Some(EntityTarget::from_name(name).ok_or_else(|| {
                        LootError::Invalid(format!("unknown set_name entity target `{name}`"))
                    })?),
                    None => None,
                };
                FunctionBody::SetName {
                    name: field(map, "name").cloned(),
                    target,
                    entity,
                }
            }
            "minecraft:set_ominous_bottle_amplifier" => {
                FunctionBody::SetOminousBottleAmplifier(parse_int_provider(
                    required(map, "amplifier", kind)?,
                    "amplifier",
                )?)
            }
            "minecraft:set_potion" => FunctionBody::SetPotion(names::single_registry_id(
                required(map, "id", kind)?,
                Registry::Potion,
                "id",
            )?),
            "minecraft:set_stew_effect" => {
                let mut effects = Vec::new();
                for value in super::json::opt_array(map, "effects", "effects")?.unwrap_or_default() {
                    let entry = object(value, "stew effect")?;
                    effects.push((
                        names::single_registry_id(
                            required(entry, "type", "stew effect")?,
                            Registry::MobEffect,
                            "type",
                        )?,
                        parse_int_provider(
                            required(entry, "duration", "stew effect")?,
                            "duration",
                        )?,
                    ));
                }
                FunctionBody::SetStewEffect(effects)
            }
            // 需要世界里的结构或地图数据，本实现暂时算不了。
            "minecraft:exploration_map" => FunctionBody::Unsupported(
                "exploration_map needs structure generation data".to_owned(),
            ),
            other if UNSUPPORTED_FUNCTIONS.contains(&other) => {
                FunctionBody::Unsupported(format!("function `{other}` is not implemented"))
            }
            other => {
                return Err(LootError::Invalid(format!(
                    "unknown loot function type `{other}`"
                )))
            }
        };
        Ok(Self { condition, body })
    }

    /// 按官方 `LootItemConditionalFunction.apply` 的顺序：条件不成立就原样返回。
    pub fn apply(
        &self,
        stack: LootStack,
        context: &LootContext<'_, '_, '_>,
    ) -> Result<LootStack> {
        if let Some(condition) = &self.condition {
            if !condition.test(context)? {
                return Ok(stack);
            }
        }
        self.run(stack, context)
    }

    fn run(&self, mut stack: LootStack, context: &LootContext<'_, '_, '_>) -> Result<LootStack> {
        match &self.body {
            FunctionBody::Sequence(functions) => {
                for function in functions {
                    stack = function.apply(stack, context)?;
                }
                Ok(stack)
            }
            FunctionBody::SetCount { count, add } => {
                let base = if *add { stack.count } else { 0 };
                stack.set_count(base.wrapping_add(count.get(context)?));
                Ok(stack)
            }
            FunctionBody::SetItem(item) => Ok(stack.transmute_copy(*item)),
            FunctionBody::SetComponents(patch) => {
                for (name, value) in patch {
                    match value {
                        Some(value) => stack.set_component(name, value.clone()),
                        None => stack.remove_component(name),
                    }
                }
                Ok(stack)
            }
            FunctionBody::ApplyBonus {
                enchantment,
                formula,
            } => {
                let Some(tool) = context.params.tool.as_ref() else {
                    return Ok(stack);
                };
                let level = tool.item_enchantment_level(*enchantment);
                apply_bonus(&mut stack, *formula, level, context.random());
                Ok(stack)
            }
            FunctionBody::CopyComponents {
                source,
                include,
                exclude,
            } => {
                copy_components(&mut stack, *source, include.as_deref(), exclude, context)?;
                Ok(stack)
            }
            FunctionBody::CopyState { properties } => {
                copy_state(&mut stack, properties, context);
                Ok(stack)
            }
            FunctionBody::Discard => Ok(LootStack::empty()),
            FunctionBody::EnchantRandomly {
                options,
                only_compatible,
                include_additional_cost,
            } => Ok(enchant_randomly(
                stack,
                options.as_deref(),
                *only_compatible,
                *include_additional_cost,
                context,
            )),
            FunctionBody::EnchantWithLevels {
                levels,
                options,
                include_additional_cost,
            } => {
                let cost = levels.get(context)?;
                let mut result = enchant_with_levels(stack, cost, options.as_deref(), context.random());
                if *include_additional_cost
                    && context.params.additional_cost_component_allowed.is_some()
                    && !result.is_empty()
                    && cost > 0
                {
                    result.set_component(ADDITIONAL_TRADE_COST, Value::from(cost));
                }
                Ok(result)
            }
            FunctionBody::EnchantedCountIncrease {
                enchantment,
                count,
                limit,
            } => {
                let Some(killer) = context.params.attacking_entity.as_ref() else {
                    return Ok(stack);
                };
                if !killer.is_living {
                    return Ok(stack);
                }
                let level = crate::context::entity_enchantment_level(killer, *enchantment);
                if level == 0 {
                    return Ok(stack);
                }
                let addition = level as f32 * count.get(context)?;
                stack.grow(addition.round() as i32);
                if *limit > 0 {
                    stack.limit_size(*limit);
                }
                Ok(stack)
            }
            FunctionBody::ExplosionDecay => {
                if let Some(radius) = context.params.explosion_radius {
                    let probability = 1.0_f32 / radius;
                    let mut result = 0;
                    for _ in 0..stack.count {
                        if context.random().next_float() <= probability {
                            result += 1;
                        }
                    }
                    stack.set_count(result);
                }
                Ok(stack)
            }
            FunctionBody::Filtered {
                filter,
                on_pass,
                on_fail,
            } => {
                let function = if filter.matches(&stack)? {
                    on_pass
                } else {
                    on_fail
                };
                match function {
                    Some(function) => function.apply(stack, context),
                    None => Ok(stack),
                }
            }
            FunctionBody::Smelt { use_input_count } => Ok(smelt(stack, *use_input_count)),
            FunctionBody::LimitCount { min, max } => {
                let input = stack.count;
                let count = match (min, max) {
                    (None, None) => input,
                    (None, Some(max)) => max.get(context)?.min(input),
                    (Some(min), None) => min.get(context)?.max(input),
                    (Some(min), Some(max)) => {
                        let min = min.get(context)?;
                        let max = max.get(context)?;
                        input.clamp(min, max)
                    }
                };
                stack.set_count(count);
                Ok(stack)
            }
            FunctionBody::SetDamage { damage, add } => {
                if stack.is_damageable() {
                    let max_damage = stack.max_damage();
                    let base = if *add {
                        1.0 - stack.damage_value() as f32 / max_damage as f32
                    } else {
                        0.0
                    };
                    let pct = 1.0 - (damage.get(context)? + base).clamp(0.0, 1.0);
                    stack.set_damage_value((pct * max_damage as f32).floor() as i32);
                }
                Ok(stack)
            }
            FunctionBody::SetEnchantments { enchantments, add } => {
                if stack.item == items::BOOK {
                    stack = stack.transmute_copy(items::ENCHANTED_BOOK);
                }
                let component = stack.enchantment_component();
                let mut levels = stack.enchantment_levels(component);
                for (enchantment, provider) in enchantments {
                    let Some(name) = names::enchantment_name(*enchantment) else {
                        continue;
                    };
                    let current = levels.get(name).copied().unwrap_or(0);
                    let value = provider.get(context)?;
                    let level = if *add {
                        current.wrapping_add(value).clamp(0, 255)
                    } else {
                        value.clamp(0, 255)
                    };
                    if level <= 0 {
                        levels.remove(name);
                    } else {
                        levels.insert(name.to_owned(), level);
                    }
                }
                stack.set_enchantment_levels(component, levels);
                Ok(stack)
            }
            FunctionBody::SetInstrument(options) => {
                if !options.is_empty() {
                    let index = context.random().next_int(options.len() as i32) as usize;
                    if let Some(name) = names::instrument_name(options[index]) {
                        stack.set_component(INSTRUMENT, Value::from(name));
                    }
                }
                Ok(stack)
            }
            FunctionBody::SetName {
                name,
                target,
                entity,
            } => {
                let Some(name) = name else {
                    return Ok(stack);
                };
                if entity.is_some() {
                    return Err(LootError::Unsupported(
                        "set_name with an entity resolver needs command text resolution".to_owned(),
                    ));
                }
                stack.set_component(target, name.clone());
                Ok(stack)
            }
            FunctionBody::SetOminousBottleAmplifier(amplifier) => {
                let value = amplifier.get(context)?.clamp(0, 4);
                if value == 0 {
                    stack.remove_component(OMINOUS_BOTTLE_AMPLIFIER);
                } else {
                    stack.set_component(OMINOUS_BOTTLE_AMPLIFIER, Value::from(value));
                }
                Ok(stack)
            }
            FunctionBody::SetPotion(potion) => {
                let Some(name) = names::potion_name(*potion) else {
                    return Ok(stack);
                };
                let mut contents = stack
                    .component(POTION_CONTENTS)
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                contents.insert("potion".to_owned(), Value::from(name));
                stack.set_component(POTION_CONTENTS, Value::Object(contents));
                Ok(stack)
            }
            FunctionBody::SetStewEffect(effects) => {
                if stack.item == items::SUSPICIOUS_STEW && !effects.is_empty() {
                    let index = context.random().next_int(effects.len() as i32) as usize;
                    let (effect, duration) = &effects[index];
                    let mut duration = duration.get(context)?;
                    let Some(effect_name) = names::mob_effect_name(*effect) else {
                        return Ok(stack);
                    };
                    if !loot::mob_effect_is_instantaneous(effect_name) {
                        duration *= 20;
                    }
                    let mut list = stack
                        .component(SUSPICIOUS_STEW_EFFECTS)
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    list.push(json!({"id": effect_name, "duration": duration}));
                    stack.set_component(SUSPICIOUS_STEW_EFFECTS, Value::Array(list));
                }
                Ok(stack)
            }
            FunctionBody::Unsupported(message) => Err(LootError::Unsupported(message.clone())),
        }
    }
}

fn parse_optional_function(
    map: &Map<String, Value>,
    name: &str,
    depth: usize,
) -> Result<Option<Box<LootFunction>>> {
    match field(map, name) {
        Some(value) => Ok(Some(Box::new(LootFunction::parse_at(value, depth + 1)?))),
        None => Ok(None),
    }
}

fn parse_component_patch(value: &Value) -> Result<Vec<(String, Option<Value>)>> {
    let map = object(value, "components")?;
    let mut patch = Vec::with_capacity(map.len());
    for (name, value) in map {
        if let Some(removed) = name.strip_prefix('!') {
            patch.push((removed.to_owned(), None));
        } else if value.is_null() {
            patch.push((name.clone(), None));
        } else {
            patch.push((name.clone(), Some(value.clone())));
        }
    }
    Ok(patch)
}

fn parse_bonus_formula(map: &Map<String, Value>, what: &str) -> Result<BonusFormula> {
    let formula = string(required(map, "formula", what)?, "formula")?;
    // 公式参数在 `parameters` 里，`ore_drops` 没有参数。
    let parameters = field(map, "parameters");
    let parameter = |name: &str| -> Result<&Value> {
        match parameters {
            Some(parameters) => required(object(parameters, "parameters")?, name, "parameters"),
            None => Err(LootError::Invalid(format!(
                "apply_bonus formula `{formula}` is missing `parameters.{name}`"
            ))),
        }
    };
    match formula {
        "minecraft:binomial_with_bonus_count" => Ok(BonusFormula::Binomial {
            extra: i32_value(parameter("extra")?, "extra")?,
            probability: f32_value(parameter("probability")?, "probability")?,
        }),
        "minecraft:ore_drops" => Ok(BonusFormula::OreDrops),
        "minecraft:uniform_bonus_count" => Ok(BonusFormula::Uniform {
            bonus_multiplier: i32_value(parameter("bonusMultiplier")?, "bonusMultiplier")?,
        }),
        other => Err(LootError::Invalid(format!(
            "unknown apply_bonus formula `{other}`"
        ))),
    }
}

fn parse_component_source(value: &Value) -> Result<ComponentSource> {
    let name = string(value, "source")?;
    match name {
        "block_entity" => Ok(ComponentSource::BlockEntity),
        "this" | "attacker" | "direct_attacker" | "attacking_player" | "target_entity"
        | "interacting_entity" | "tool" => Ok(ComponentSource::Entity),
        other => Err(LootError::Invalid(format!(
            "unknown copy_components source `{other}`"
        ))),
    }
}

/// `ApplyBonusCount.run`：工具上附魔等级决定额外数量。
fn apply_bonus(stack: &mut LootStack, formula: BonusFormula, level: i32, random: &dyn RandomSource) {
    match formula {
        BonusFormula::Binomial { extra, probability } => {
            let mut count = stack.count;
            for _ in 0..level.wrapping_add(extra).max(0) {
                if random.next_float() < probability {
                    count += 1;
                }
            }
            stack.set_count(count);
        }
        BonusFormula::OreDrops => {
            if level > 0 {
                let bonus = random.next_int(level + 2) - 1;
                let bonus = bonus.max(0);
                stack.set_count(stack.count.wrapping_mul(bonus + 1));
            }
        }
        BonusFormula::Uniform { bonus_multiplier } => {
            let bound = bonus_multiplier.wrapping_mul(level).wrapping_add(1);
            if bound > 0 {
                stack.set_count(stack.count.wrapping_add(random.next_int(bound)));
            }
        }
    }
}

/// `CopyComponentsFunction` 的来源是方块实体时，复制它收集出来的组件。
fn copy_components(
    stack: &mut LootStack,
    source: ComponentSource,
    include: Option<&[String]>,
    exclude: &[String],
    context: &LootContext<'_, '_, '_>,
) -> Result<()> {
    let components = match source {
        ComponentSource::BlockEntity => match context.params.block_entity.as_ref() {
            Some(block_entity) => &block_entity.components,
            None => return Ok(()),
        },
        ComponentSource::Entity => {
            return Err(LootError::Unsupported(
                "copy_components from an entity or item stack is not implemented".to_owned(),
            ))
        }
    };
    for (name, value) in components {
        if exclude.contains(name) {
            continue;
        }
        if include.is_some_and(|include| !include.contains(name)) {
            continue;
        }
        stack.set_component(name, value.clone());
    }
    Ok(())
}

/// `CopyBlockState.run`：把当前方块状态里同名的属性写进 `block_state` 组件。
fn copy_state(
    stack: &mut LootStack,
    properties: &[String],
    context: &LootContext<'_, '_, '_>,
) {
    let Some(state) = context.params.block_state else {
        return;
    };
    let mut values = stack
        .component(BLOCK_STATE)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for property in properties {
        if let Some(value) = state.property_state(property) {
            values.insert(property.clone(), Value::from(value));
        }
    }
    if values.is_empty() {
        stack.remove_component(BLOCK_STATE);
    } else {
        stack.set_component(BLOCK_STATE, Value::Object(values));
    }
}

/// `EnchantRandomlyFunction.run`。
fn enchant_randomly(
    stack: LootStack,
    options: Option<&[i32]>,
    only_compatible: bool,
    include_additional_cost: bool,
    context: &LootContext<'_, '_, '_>,
) -> LootStack {
    let target_is_book = stack.item == items::BOOK;
    let should_check = !target_is_book && only_compatible;
    let candidates: Vec<i32> = match options {
        Some(options) => options
            .iter()
            .copied()
            .filter(|id| {
                !should_check
                    || names::enchantment_def(*id).is_some_and(|def| def.is_supported(stack.item))
            })
            .collect(),
        None => names::enchantment_defs_all()
            .iter()
            .enumerate()
            .filter(|(_, def)| !should_check || def.is_supported(stack.item))
            .map(|(index, _)| index as i32)
            .collect(),
    };
    if candidates.is_empty() {
        return stack;
    }
    let index = context.random().next_int(candidates.len() as i32) as usize;
    let enchantment = candidates[index];
    let Some(def) = names::enchantment_def(enchantment) else {
        return stack;
    };
    let level = context.random().mth_next_int(def.min_level, def.max_level);
    let mut stack = if stack.item == items::BOOK {
        stack.transmute_copy(items::ENCHANTED_BOOK)
    } else {
        stack
    };
    stack.enchant(enchantment, level);
    if include_additional_cost && context.params.additional_cost_component_allowed.is_some() {
        let cost = 2 + context.random().next_int(5 + level * 10) + 3 * level;
        stack.set_component(ADDITIONAL_TRADE_COST, Value::from(cost));
    }
    stack
}

/// `EnchantmentHelper.enchantItem`，随机数的取数顺序完全照官方。
fn enchant_with_levels(
    mut stack: LootStack,
    cost: i32,
    options: Option<&[i32]>,
    random: &dyn RandomSource,
) -> LootStack {
    let enchantments = select_enchantment(&stack, cost, options, random);
    if stack.item == items::BOOK {
        stack = stack.transmute_copy(items::ENCHANTED_BOOK);
    }
    for (enchantment, level) in enchantments {
        stack.enchant(enchantment, level);
    }
    stack
}

/// `EnchantmentHelper.selectEnchantment`。
fn select_enchantment(
    stack: &LootStack,
    mut cost: i32,
    options: Option<&[i32]>,
    random: &dyn RandomSource,
) -> Vec<(i32, i32)> {
    let mut results = Vec::new();
    let Some(enchantable) = stack.item().enchantable else {
        return results;
    };
    cost += 1 + random.next_int(enchantable / 4 + 1) + random.next_int(enchantable / 4 + 1);
    let span = (random.next_float() + random.next_float() - 1.0) * 0.15;
    cost = (cost as f32 + cost as f32 * span).round().clamp(1.0, i32::MAX as f32) as i32;
    let mut candidates = available_enchantments(cost, stack, options);
    if candidates.is_empty() {
        return results;
    }
    if let Some(index) = weighted_random(&candidates, random) {
        results.push(candidates[index]);
    }
    while random.next_int(50) <= cost {
        if let Some((last, _)) = results.last() {
            // `filterCompatibleEnchantments` 把和最后一个结果不兼容的候选取掉。
            candidates.retain(|(enchantment, _)| {
                *last != *enchantment
                    && names::enchantment_def(*last).is_none_or(|def| !def.excludes(*enchantment))
                    && names::enchantment_def(*enchantment)
                        .is_none_or(|other| !other.excludes(*last))
            });
        }
        if candidates.is_empty() {
            break;
        }
        if let Some(index) = weighted_random(&candidates, random) {
            results.push(candidates[index]);
        }
        cost /= 2;
    }
    results
}

/// `EnchantmentHelper.getAvailableEnchantmentResults`。
fn available_enchantments(
    cost: i32,
    stack: &LootStack,
    options: Option<&[i32]>,
) -> Vec<(i32, i32)> {
    let is_book = stack.item == items::BOOK;
    let mut results = Vec::new();
    let ids: Vec<i32> = match options {
        Some(options) => options.to_vec(),
        None => (0..names::enchantment_defs_all().len() as i32).collect(),
    };
    for id in ids {
        let Some(def) = names::enchantment_def(id) else {
            continue;
        };
        if !def.is_primary(stack.item) && !is_book {
            continue;
        }
        for level in (def.min_level..=def.max_level).rev() {
            if cost >= def.min_cost_at(level) && cost <= def.max_cost_at(level) {
                results.push((id, level));
                break;
            }
        }
    }
    results
}

/// `WeightedRandom.getRandomItem`：总权重为 0 时返回 `None`。
fn weighted_random(items: &[(i32, i32)], random: &dyn RandomSource) -> Option<usize> {
    let total: i32 = items
        .iter()
        .map(|(enchantment, _)| {
            names::enchantment_def(*enchantment)
                .map(|def| def.weight)
                .unwrap_or(1)
        })
        .fold(0, i32::wrapping_add);
    if total == 0 {
        return None;
    }
    let mut selection = random.next_int(total);
    for (index, (enchantment, _)) in items.iter().enumerate() {
        let weight = names::enchantment_def(*enchantment)
            .map(|def| def.weight)
            .unwrap_or(1);
        selection = selection.wrapping_sub(weight);
        if selection < 0 {
            return Some(index);
        }
    }
    None
}

/// `SmeltItemFunction.run`：按输入找熔炉配方，结果的数量按输入数量放大。
fn smelt(stack: LootStack, use_input_count: bool) -> LootStack {
    if stack.is_empty() {
        return stack;
    }
    let Some(recipe) = smelting_recipes()
        .iter()
        .find(|recipe| recipe.matches(stack.item))
    else {
        return stack;
    };
    let mut result = LootStack::new(recipe.result, recipe.count);
    let new_count = (if use_input_count { stack.count } else { 1 }).wrapping_mul(recipe.count);
    result.set_count(new_count.min(result.max_stack_size()));
    result
}

struct SmeltingRecipe {
    ingredients: Vec<Ingredient>,
    result: i32,
    count: i32,
}

enum Ingredient {
    Item(i32),
    Tag(&'static [i32]),
}

impl SmeltingRecipe {
    fn matches(&self, item: i32) -> bool {
        self.ingredients.iter().any(|ingredient| match ingredient {
            Ingredient::Item(id) => *id == item,
            Ingredient::Tag(ids) => ids.contains(&item),
        })
    }
}

fn smelting_recipes() -> &'static Vec<SmeltingRecipe> {
    static RECIPES: OnceLock<Vec<SmeltingRecipe>> = OnceLock::new();
    RECIPES.get_or_init(|| {
        loot::SMELTING_RECIPES
            .iter()
            .map(|(name, raw)| parse_smelting_recipe(name, raw).expect("生成器导出的熔炉配方必须能解析"))
            .collect()
    })
}

fn parse_smelting_recipe(name: &str, raw: &str) -> Result<SmeltingRecipe> {
    let value: Value = serde_json::from_str(raw).map_err(|error| {
        LootError::Invalid(format!("smelting recipe `{name}`: {error}"))
    })?;
    let map = object(&value, name)?;
    let mut ingredients = Vec::new();
    for raw in super::json::string_or_list(required(map, "ingredient", name)?, "ingredient")? {
        let (name, is_tag) = super::json::holder_name(raw);
        if is_tag {
            let ids = names::tag_ids("minecraft:item", name).ok_or_else(|| {
                LootError::Invalid(format!("unknown item tag `{raw}` in smelting recipe"))
            })?;
            ingredients.push(Ingredient::Tag(ids));
        } else {
            let item = names::item_id(name).ok_or_else(|| {
                LootError::Invalid(format!("unknown item `{raw}` in smelting recipe"))
            })?;
            ingredients.push(Ingredient::Item(item));
        }
    }
    let result = object(required(map, "result", name)?, "result")?;
    let result_item = names::single_registry_id(required(result, "id", "result")?, Registry::Item, "result")?;
    let count = super::json::opt_i32(result, "count", "result", 1)?;
    Ok(SmeltingRecipe {
        ingredients,
        result: result_item,
        count,
    })
}

/// 供 `entry`、`pool` 解析 `modifier` 字段。
pub(crate) fn parse_modifier(value: Option<&Value>) -> Result<Option<LootFunction>> {
    value.map(LootFunction::parse).transpose()
}
