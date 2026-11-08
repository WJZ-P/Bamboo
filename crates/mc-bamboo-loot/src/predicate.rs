//! 官方 `net.minecraft.advancements.predicates` 下战利品表会用到的谓词。
//!
//! 字段名、默认值和求值顺序都照官方 codec 和 `matches` 写。世界相关的部分走
//! [`LevelView`]，调用方没提供数据时返回 [`LootError::Unsupported`]，不猜默认值。
//!
//! 本实现还没有的部分（结构、流体、NBT、生物效果、玩家和槽位谓词）在解析时记下
//! 名字，求值到那里报不支持。

use std::collections::BTreeMap;

use mc_bamboo_blocks::BlockState;
use serde_json::{Map, Value};

use crate::context::{DamageSourceView, EntityView, LevelView, LootContext, Position};
use crate::json::{
    field, i32_value, invalid, object, opt_bool, required, string,
};
use crate::names::{self, Registry};
use crate::stack::LootStack;
use crate::{LootError, Result};

fn unsupported(what: impl Into<String>) -> LootError {
    LootError::Unsupported(what.into())
}

/// `MinMaxBounds.Ints`。`min` 和 `max` 都可以缺。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IntBounds {
    pub min: Option<i32>,
    pub max: Option<i32>,
}

impl IntBounds {
    pub const ANY: Self = Self {
        min: None,
        max: None,
    };

    pub fn exactly(value: i32) -> Self {
        Self {
            min: Some(value),
            max: Some(value),
        }
    }

    /// 官方 codec 接受单个数字或者 `{min, max}`，上下界颠倒时数据非法。
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let bounds = match value {
            Value::Number(_) => return Ok(Self::exactly(i32_value(value, what)?)),
            Value::Object(map) => Self {
                min: field(map, "min")
                    .map(|value| i32_value(value, what))
                    .transpose()?,
                max: field(map, "max")
                    .map(|value| i32_value(value, what))
                    .transpose()?,
            },
            _ => return Err(invalid(format!("{what} must be an int or an int range"))),
        };
        if let (Some(min), Some(max)) = (bounds.min, bounds.max) {
            if min > max {
                return Err(invalid(format!("{what} has swapped bounds")));
            }
        }
        Ok(bounds)
    }

    pub fn matches(&self, value: i32) -> bool {
        self.min.is_none_or(|min| min <= value) && self.max.is_none_or(|max| value <= max)
    }

    pub fn is_any(&self) -> bool {
        self.min.is_none() && self.max.is_none()
    }
}

/// `MinMaxBounds.Doubles`。平方上下界在构造时算好，和官方一样。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DoubleBounds {
    pub min: Option<f64>,
    pub max: Option<f64>,
    min_sqr: Option<f64>,
    max_sqr: Option<f64>,
}

impl DoubleBounds {
    pub const ANY: Self = Self {
        min: None,
        max: None,
        min_sqr: None,
        max_sqr: None,
    };

    pub fn new(min: Option<f64>, max: Option<f64>) -> Self {
        Self {
            min,
            max,
            min_sqr: min.map(|value| value * value),
            max_sqr: max.map(|value| value * value),
        }
    }

    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let bounds = match value {
            Value::Number(_) => {
                let value = value
                    .as_f64()
                    .ok_or_else(|| invalid(format!("{what} must be a double")))?;
                return Ok(Self::new(Some(value), Some(value)));
            }
            Value::Object(map) => Self::new(
                field(map, "min")
                    .map(|value| {
                        value
                            .as_f64()
                            .ok_or_else(|| invalid(format!("{what} must be a double")))
                    })
                    .transpose()?,
                field(map, "max")
                    .map(|value| {
                        value
                            .as_f64()
                            .ok_or_else(|| invalid(format!("{what} must be a double")))
                    })
                    .transpose()?,
            ),
            _ => {
                return Err(invalid(format!(
                    "{what} must be a double or a double range"
                )));
            }
        };
        if let (Some(min), Some(max)) = (bounds.min, bounds.max) {
            if min > max {
                return Err(invalid(format!("{what} has swapped bounds")));
            }
        }
        Ok(bounds)
    }

    pub fn matches(&self, value: f64) -> bool {
        self.min.is_none_or(|min| min <= value) && self.max.is_none_or(|max| value <= max)
    }

    pub fn matches_sqr(&self, value_sqr: f64) -> bool {
        self.min_sqr
            .is_none_or(|min| min <= value_sqr)
            && self.max_sqr.is_none_or(|max| value_sqr <= max)
    }
}

/// 一个附魔谓词：`enchantments` 是附魔集合，`level` 是等级范围。
#[derive(Debug, Clone)]
pub struct EnchantmentPredicate {
    pub enchantments: Option<Vec<i32>>,
    pub level: IntBounds,
}

impl EnchantmentPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        Ok(Self {
            enchantments: names::opt_holder_set(map, "enchantments", Registry::Enchantment, what)?,
            level: match field(map, "levels") {
                Some(value) => IntBounds::parse(value, what)?,
                None => IntBounds::ANY,
            },
        })
    }

    /// `EnchantmentPredicate.containedIn`。`levels` 只放等级大于 0 的附魔。
    pub fn contained_in(&self, levels: &BTreeMap<i32, i32>) -> bool {
        if let Some(enchantments) = &self.enchantments {
            return enchantments.iter().any(|enchantment| {
                levels.get(enchantment).is_some_and(|level| {
                    *level != 0 && (self.level.is_any() || self.level.matches(*level))
                })
            });
        }
        if !self.level.is_any() {
            return levels
                .values()
                .any(|level| self.level.matches(*level));
        }
        !levels.is_empty()
    }
}

/// 组件谓词里能单独判断的一项。
#[derive(Debug, Clone)]
pub enum PartialComponent {
    /// `minecraft:enchantments` / `minecraft:stored_enchantments`。
    Enchantments {
        stored: bool,
        predicates: Vec<EnchantmentPredicate>,
    },
    /// `minecraft:damage`：耐久和已损失耐久。
    Damage {
        durability: IntBounds,
        damage: IntBounds,
    },
    /// 组件存在即可。
    Present(String),
    /// 本实现还没实现的谓词类型。
    Unsupported(String),
}

/// `DataComponentMatchers`：精确组件相等，加上若干部分谓词。
#[derive(Debug, Clone, Default)]
pub struct ComponentMatchers {
    pub exact: BTreeMap<String, Value>,
    pub partial: Vec<PartialComponent>,
}

impl ComponentMatchers {
    pub fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.partial.is_empty()
    }

    /// 从 `DataComponentMatchers` 的字段解析：`components` 是精确表，`predicates` 是部分谓词。
    pub fn parse_fields(object: &Map<String, Value>, what: &str) -> Result<Self> {
        let exact = match field(object, "components") {
            Some(value) => crate::json::object(value, what)?
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
            None => BTreeMap::new(),
        };
        let partial = match field(object, "predicates") {
            Some(value) => Self::parse_partial_map(value, what)?,
            None => Vec::new(),
        };
        Ok(Self { exact, partial })
    }

    /// 实体谓词的 `minecraft:components` 是精确表本身。
    pub fn parse_exact(value: &Value, what: &str) -> Result<Self> {
        Ok(Self {
            exact: object(value, what)?
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
            partial: Vec::new(),
        })
    }

    /// 实体谓词的 `minecraft:predicates` 是部分谓词表本身。
    pub fn parse_partial(value: &Value, what: &str) -> Result<Self> {
        Ok(Self {
            exact: BTreeMap::new(),
            partial: Self::parse_partial_map(value, what)?,
        })
    }

    fn parse_partial_map(value: &Value, what: &str) -> Result<Vec<PartialComponent>> {
        object(value, what)?
            .iter()
            .map(|(name, value)| Self::parse_partial_one(name, value, what))
            .collect()
    }

    fn parse_partial_one(name: &str, value: &Value, what: &str) -> Result<PartialComponent> {
        match name {
            "minecraft:enchantments" => Ok(PartialComponent::Enchantments {
                stored: false,
                predicates: Self::parse_enchantment_predicates(value, what)?,
            }),
            "minecraft:stored_enchantments" => Ok(PartialComponent::Enchantments {
                stored: true,
                predicates: Self::parse_enchantment_predicates(value, what)?,
            }),
            "minecraft:damage" => {
                let map = object(value, what)?;
                Ok(PartialComponent::Damage {
                    durability: match field(map, "durability") {
                        Some(value) => IntBounds::parse(value, what)?,
                        None => IntBounds::ANY,
                    },
                    damage: match field(map, "damage") {
                        Some(value) => IntBounds::parse(value, what)?,
                        None => IntBounds::ANY,
                    },
                })
            }
            other => {
                // `AnyValue` 类型（比如 `minecraft:map_id`）的 codec 只接受空对象。
                if value.as_object().is_some_and(Map::is_empty) {
                    Ok(PartialComponent::Present(other.to_owned()))
                } else {
                    Ok(PartialComponent::Unsupported(other.to_owned()))
                }
            }
        }
    }

    fn parse_enchantment_predicates(
        value: &Value,
        what: &str,
    ) -> Result<Vec<EnchantmentPredicate>> {
        super::json::array(value, what)?
            .iter()
            .map(|value| EnchantmentPredicate::parse(value, what))
            .collect()
    }

    pub fn matches_stack(&self, stack: &LootStack) -> Result<bool> {
        self.matches_components(&stack.components)
    }

    /// 对一组组件求值。精确组件按 JSON 值比较。
    pub fn matches_components(&self, components: &BTreeMap<String, Value>) -> Result<bool> {
        for (name, expected) in &self.exact {
            if components.get(name) != Some(expected) {
                return Ok(false);
            }
        }
        for partial in &self.partial {
            let matched = match partial {
                PartialComponent::Enchantments { stored, predicates } => {
                    let component = if *stored {
                        "minecraft:stored_enchantments"
                    } else {
                        "minecraft:enchantments"
                    };
                    let levels = enchantment_levels(components.get(component));
                    predicates
                        .iter()
                        .all(|predicate| predicate.contained_in(&levels))
                }
                PartialComponent::Damage { durability, damage } => {
                    let Some(damage_value) = components
                        .get("minecraft:damage")
                        .and_then(Value::as_i64)
                        .and_then(|value| i32::try_from(value).ok())
                    else {
                        return Ok(false);
                    };
                    let max_damage = components
                        .get("minecraft:max_damage")
                        .and_then(Value::as_i64)
                        .and_then(|value| i32::try_from(value).ok())
                        .unwrap_or(0);
                    durability.matches(max_damage - damage_value) && damage.matches(damage_value)
                }
                PartialComponent::Present(name) => components.contains_key(name),
                PartialComponent::Unsupported(name) => {
                    return Err(unsupported(format!("component predicate `{name}`")));
                }
            };
            if !matched {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// 从 `minecraft:enchantments` 组件里读出附魔等级表，等级为 0 的不记。
fn enchantment_levels(component: Option<&Value>) -> BTreeMap<i32, i32> {
    let Some(map) = component.and_then(Value::as_object) else {
        return BTreeMap::new();
    };
    let mut levels = BTreeMap::new();
    for (name, level) in map {
        let Some(id) = names::enchantment_id(name) else {
            continue;
        };
        let Some(level) = level
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .filter(|level| *level != 0)
        else {
            continue;
        };
        levels.insert(id, level);
    }
    levels
}

/// `StatePropertiesPredicate`：属性名到精确值或范围。
#[derive(Debug, Clone, Default)]
pub struct StatePropertiesPredicate {
    properties: Vec<(String, ValueMatcher)>,
}

#[derive(Debug, Clone)]
enum ValueMatcher {
    Exact(String),
    Ranged {
        min: Option<String>,
        max: Option<String>,
    },
}

impl StatePropertiesPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        let mut properties = Vec::with_capacity(map.len());
        for (name, value) in map {
            let matcher = match value {
                Value::String(text) => ValueMatcher::Exact(text.clone()),
                Value::Object(object) => ValueMatcher::Ranged {
                    min: field(object, "min")
                        .map(|value| string(value, what).map(str::to_owned))
                        .transpose()?,
                    max: field(object, "max")
                        .map(|value| string(value, what).map(str::to_owned))
                        .transpose()?,
                },
                _ => {
                    return Err(invalid(format!(
                        "{what} property `{name}` must be a string or a range"
                    )));
                }
            };
            properties.push((name.clone(), matcher));
        }
        Ok(Self { properties })
    }

    pub fn matches(&self, state: BlockState) -> bool {
        self.properties.iter().all(|(name, matcher)| match matcher {
            ValueMatcher::Exact(expected) => state.property_state(name) == Some(expected.as_str()),
            ValueMatcher::Ranged { min, max } => {
                let Some(actual) = state.property_state(name) else {
                    return false;
                };
                let compare = |other: &str| compare_property(state, name, actual, other);
                if let Some(min) = min {
                    if compare(min).is_none_or(|ordering| ordering.is_lt()) {
                        return false;
                    }
                }
                if let Some(max) = max {
                    if compare(max).is_none_or(|ordering| ordering.is_gt()) {
                        return false;
                    }
                }
                true
            }
        })
    }
}

/// 属性值比较。数值按数值，布尔按 `false < true`，其余按方块状态里的声明顺序，
/// 对应官方 `Comparable` 的 `compareTo`。
fn compare_property(
    state: BlockState,
    name: &str,
    actual: &str,
    other: &str,
) -> Option<std::cmp::Ordering> {
    if let (Ok(left), Ok(right)) = (actual.parse::<i32>(), other.parse::<i32>()) {
        return Some(left.cmp(&right));
    }
    if let (Ok(left), Ok(right)) = (actual.parse::<bool>(), other.parse::<bool>()) {
        return Some(left.cmp(&right));
    }
    let values = state.property_values(name)?;
    let left = values.iter().position(|value| *value == actual)?;
    let right = values.iter().position(|value| *value == other)?;
    Some(left.cmp(&right))
}

/// `ItemPredicate`。
#[derive(Debug, Clone, Default)]
pub struct ItemPredicate {
    pub items: Option<Vec<i32>>,
    pub count: IntBounds,
    pub components: ComponentMatchers,
}

impl ItemPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        Ok(Self {
            items: names::opt_holder_set(map, "items", Registry::Item, what)?,
            count: match field(map, "count") {
                Some(value) => IntBounds::parse(value, what)?,
                None => IntBounds::ANY,
            },
            components: ComponentMatchers::parse_fields(map, what)?,
        })
    }

    pub fn matches(&self, stack: &LootStack) -> Result<bool> {
        if let Some(items) = &self.items {
            if !items.contains(&stack.item) {
                return Ok(false);
            }
        }
        if !self.count.matches(stack.count) {
            return Ok(false);
        }
        self.components.matches_stack(stack)
    }
}

/// `BlockPredicate`。
#[derive(Debug, Clone, Default)]
pub struct BlockPredicate {
    pub blocks: Option<Vec<i32>>,
    pub properties: Option<StatePropertiesPredicate>,
    /// NBT 谓词本实现不支持，只记下有这个字段。
    pub has_nbt: bool,
    pub components: ComponentMatchers,
}

impl BlockPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        Ok(Self {
            blocks: names::opt_holder_set(map, "blocks", Registry::Block, what)?,
            properties: match field(map, "state") {
                Some(value) => Some(StatePropertiesPredicate::parse(value, what)?),
                None => None,
            },
            has_nbt: field(map, "nbt").is_some(),
            components: ComponentMatchers::parse_fields(map, what)?,
        })
    }

    pub fn matches_state(&self, state: BlockState) -> bool {
        if let Some(blocks) = &self.blocks {
            if !blocks.contains(&(state.block_id() as i32)) {
                return false;
            }
        }
        self.properties
            .as_ref()
            .is_none_or(|properties| properties.matches(state))
    }

    /// 谓词是否要读方块实体。
    pub fn will_match_block_entity(&self) -> bool {
        self.has_nbt || !self.components.is_empty()
    }

    /// 对应官方 `BlockPredicate.matchesBlockEntity(LevelReader, BlockEntity)`。
    pub fn matches_block_entity(
        &self,
        block_entity: Option<&crate::context::BlockEntityView>,
    ) -> Result<bool> {
        if self.has_nbt {
            return Err(unsupported("block predicate nbt"));
        }
        if self.components.is_empty() {
            return Ok(true);
        }
        match block_entity {
            Some(block_entity) => self.components.matches_components(&block_entity.components),
            None => Ok(false),
        }
    }

    /// 对应官方 `BlockPredicate.matches(ServerLevel, BlockPos)`。
    pub fn matches(
        &self,
        context: &LootContext<'_, '_, '_>,
        x: i32,
        y: i32,
        z: i32,
    ) -> Result<bool> {
        let level = context.level("block predicate")?;
        match level.is_loaded(x, y, z) {
            Some(true) => {}
            Some(false) => return Ok(false),
            None => {
                return Err(unsupported(
                    "block predicate needs LevelView::is_loaded",
                ));
            }
        }
        let state = level
            .block_state(x, y, z)
            .ok_or_else(|| unsupported("block predicate needs LevelView::block_state"))?;
        if !self.matches_state(state) {
            return Ok(false);
        }
        if self.will_match_block_entity() {
            // 官方先判方块实体在不在，不在直接 false，不会去看 NBT。
            let Some(components) = level.block_entity_components(x, y, z)? else {
                return Ok(false);
            };
            if self.has_nbt {
                return Err(unsupported("block predicate nbt"));
            }
            if !self.components.matches_components(&components)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// `LightPredicate`：`{"light": {min, max}}`。
#[derive(Debug, Clone, Copy, Default)]
pub struct LightPredicate {
    pub composite: IntBounds,
}

impl LightPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        Ok(Self {
            composite: match field(map, "light") {
                Some(value) => IntBounds::parse(value, what)?,
                None => IntBounds::ANY,
            },
        })
    }

    pub fn matches(&self, level: &dyn LevelView, x: i32, y: i32, z: i32) -> Result<bool> {
        match level.is_loaded(x, y, z) {
            Some(true) => {}
            Some(false) => return Ok(false),
            None => return Err(unsupported("light predicate needs LevelView::is_loaded")),
        }
        let light = level
            .light(x, y, z)
            .ok_or_else(|| unsupported("light predicate needs LevelView::light"))?;
        Ok(self.composite.matches(light))
    }
}

/// `LocationPredicate`。求值顺序和官方一致。
#[derive(Debug, Clone, Default)]
pub struct LocationPredicate {
    pub x: DoubleBounds,
    pub y: DoubleBounds,
    pub z: DoubleBounds,
    pub biomes: Option<Vec<i32>>,
    /// 结构谓词本实现不支持，只记下有这个字段。
    pub has_structures: bool,
    pub dimension: Option<String>,
    pub smokey: Option<bool>,
    pub light: Option<LightPredicate>,
    pub block: Option<BlockPredicate>,
    /// 流体谓词本实现不支持。
    pub has_fluid: bool,
    pub can_see_sky: Option<bool>,
}

impl LocationPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        let mut predicate = Self::default();
        if let Some(position) = field(map, "position") {
            let position = object(position, what)?;
            predicate.x = match field(position, "x") {
                Some(value) => DoubleBounds::parse(value, what)?,
                None => DoubleBounds::ANY,
            };
            predicate.y = match field(position, "y") {
                Some(value) => DoubleBounds::parse(value, what)?,
                None => DoubleBounds::ANY,
            };
            predicate.z = match field(position, "z") {
                Some(value) => DoubleBounds::parse(value, what)?,
                None => DoubleBounds::ANY,
            };
        }
        predicate.biomes = names::opt_holder_set(map, "biomes", Registry::Biome, what)?;
        predicate.has_structures = field(map, "structures").is_some();
        predicate.dimension = field(map, "dimension")
            .map(|value| string(value, what).map(str::to_owned))
            .transpose()?;
        predicate.smokey = field(map, "smokey")
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| invalid(format!("{what} smokey must be a bool")))
            })
            .transpose()?;
        predicate.light = field(map, "light")
            .map(|value| LightPredicate::parse(value, what))
            .transpose()?;
        predicate.block = field(map, "block")
            .map(|value| BlockPredicate::parse(value, what))
            .transpose()?;
        predicate.has_fluid = field(map, "fluid").is_some();
        predicate.can_see_sky = field(map, "can_see_sky")
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| invalid(format!("{what} can_see_sky must be a bool")))
            })
            .transpose()?;
        Ok(predicate)
    }

    pub fn matches(
        &self,
        context: &LootContext<'_, '_, '_>,
        x: f64,
        y: f64,
        z: f64,
    ) -> Result<bool> {
        if !self.x.matches(x) || !self.y.matches(y) || !self.z.matches(z) {
            return Ok(false);
        }
        let level = context.level("location predicate")?;
        if let Some(dimension) = &self.dimension {
            match level.dimension() {
                Some(actual) if actual == dimension => {}
                Some(_) => return Ok(false),
                None => {
                    return Err(unsupported(
                        "location predicate needs LevelView::dimension",
                    ));
                }
            }
        }
        let (bx, by, bz) = Position::new(x, y, z).block_pos();
        let loaded = match level.is_loaded(bx, by, bz) {
            Some(loaded) => loaded,
            None => {
                return Err(unsupported(
                    "location predicate needs LevelView::is_loaded",
                ));
            }
        };
        if let Some(biomes) = &self.biomes {
            if !loaded {
                return Ok(false);
            }
            let biome = level
                .biome(bx, by, bz)
                .ok_or_else(|| unsupported("location predicate needs LevelView::biome"))?;
            if !biomes.contains(&biome) {
                return Ok(false);
            }
        }
        if self.has_structures {
            return Err(unsupported("location predicate structures"));
        }
        if let Some(smokey) = self.smokey {
            if !loaded {
                return Ok(false);
            }
            let actual = level
                .is_smokey(bx, by, bz)
                .ok_or_else(|| unsupported("location predicate needs LevelView::is_smokey"))?;
            if actual != smokey {
                return Ok(false);
            }
        }
        if let Some(light) = &self.light {
            if !light.matches(level, bx, by, bz)? {
                return Ok(false);
            }
        }
        if let Some(block) = &self.block {
            if !block.matches(context, bx, by, bz)? {
                return Ok(false);
            }
        }
        if self.has_fluid {
            return Err(unsupported("location predicate fluid"));
        }
        if let Some(can_see_sky) = self.can_see_sky {
            let actual = level
                .can_see_sky(bx, by, bz)
                .ok_or_else(|| unsupported("location predicate needs LevelView::can_see_sky"))?;
            if actual != can_see_sky {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// `EntityFlagsPredicate`。
#[derive(Debug, Clone, Copy, Default)]
pub struct EntityFlagsPredicate {
    pub is_on_ground: Option<bool>,
    pub is_on_fire: Option<bool>,
    pub is_sneaking: Option<bool>,
    pub is_sprinting: Option<bool>,
    pub is_swimming: Option<bool>,
    pub is_flying: Option<bool>,
    pub is_baby: Option<bool>,
    pub is_in_water: Option<bool>,
    pub is_fall_flying: Option<bool>,
}

impl EntityFlagsPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        Ok(Self {
            is_on_ground: optional_bool(map, "is_on_ground", what)?,
            is_on_fire: optional_bool(map, "is_on_fire", what)?,
            is_sneaking: optional_bool(map, "is_sneaking", what)?,
            is_sprinting: optional_bool(map, "is_sprinting", what)?,
            is_swimming: optional_bool(map, "is_swimming", what)?,
            is_flying: optional_bool(map, "is_flying", what)?,
            is_baby: optional_bool(map, "is_baby", what)?,
            is_in_water: optional_bool(map, "is_in_water", what)?,
            is_fall_flying: optional_bool(map, "is_fall_flying", what)?,
        })
    }

    pub fn matches(&self, entity: &EntityView) -> bool {
        if self.is_on_ground.is_some_and(|value| value != entity.is_on_ground) {
            return false;
        }
        if self.is_on_fire.is_some_and(|value| value != entity.is_on_fire) {
            return false;
        }
        if self.is_sneaking.is_some_and(|value| value != entity.is_sneaking) {
            return false;
        }
        if self.is_sprinting.is_some_and(|value| value != entity.is_sprinting) {
            return false;
        }
        if self.is_swimming.is_some_and(|value| value != entity.is_swimming) {
            return false;
        }
        if self.is_flying.is_some_and(|value| value != entity.is_flying) {
            return false;
        }
        if self.is_in_water.is_some_and(|value| value != entity.is_in_water) {
            return false;
        }
        // 官方只在实体是 LivingEntity 时比较这两个标记。
        if entity.is_living {
            if self
                .is_fall_flying
                .is_some_and(|value| value != entity.is_fall_flying)
            {
                return false;
            }
            if self.is_baby.is_some_and(|value| value != entity.is_baby) {
                return false;
            }
        }
        true
    }
}

fn optional_bool(object: &Map<String, Value>, name: &str, what: &str) -> Result<Option<bool>> {
    field(object, name)
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| invalid(format!("{what} `{name}` must be a bool")))
        })
        .transpose()
}

/// `EntityEquipmentPredicate`。
#[derive(Debug, Clone, Default)]
pub struct EntityEquipmentPredicate {
    slots: Vec<(crate::context::EquipmentSlot, ItemPredicate)>,
}

impl EntityEquipmentPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        use crate::context::EquipmentSlot;
        let map = object(value, what)?;
        let mut slots = Vec::new();
        for (name, slot) in [
            ("head", EquipmentSlot::Head),
            ("chest", EquipmentSlot::Chest),
            ("legs", EquipmentSlot::Legs),
            ("feet", EquipmentSlot::Feet),
            ("body", EquipmentSlot::Body),
            ("mainhand", EquipmentSlot::Mainhand),
            ("offhand", EquipmentSlot::Offhand),
        ] {
            if let Some(predicate) = field(map, name) {
                slots.push((slot, ItemPredicate::parse(predicate, what)?));
            }
        }
        Ok(Self { slots })
    }

    pub fn matches(&self, entity: &EntityView) -> Result<bool> {
        if !entity.is_living {
            return Ok(false);
        }
        let empty = LootStack::empty();
        for (slot, predicate) in &self.slots {
            let stack = entity.equipment(*slot).unwrap_or(&empty);
            if !predicate.matches(stack)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// `DistancePredicate`。差值先收窄成 `f32`，和官方一样。
#[derive(Debug, Clone, Copy, Default)]
pub struct DistancePredicate {
    pub x: DoubleBounds,
    pub y: DoubleBounds,
    pub z: DoubleBounds,
    pub horizontal: DoubleBounds,
    pub absolute: DoubleBounds,
}

impl DistancePredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        let bounds = |name: &str| -> Result<DoubleBounds> {
            match field(map, name) {
                Some(value) => DoubleBounds::parse(value, what),
                None => Ok(DoubleBounds::ANY),
            }
        };
        Ok(Self {
            x: bounds("x")?,
            y: bounds("y")?,
            z: bounds("z")?,
            horizontal: bounds("horizontal")?,
            absolute: bounds("absolute")?,
        })
    }

    pub fn matches(&self, from: Position, to: Position) -> bool {
        let xd = (from.x - to.x) as f32;
        let yd = (from.y - to.y) as f32;
        let zd = (from.z - to.z) as f32;
        if !self.x.matches(f64::from(xd.abs()))
            || !self.y.matches(f64::from(yd.abs()))
            || !self.z.matches(f64::from(zd.abs()))
        {
            return false;
        }
        if !self.horizontal.matches_sqr(f64::from(xd * xd + zd * zd)) {
            return false;
        }
        self.absolute
            .matches_sqr(f64::from(xd * xd + yd * yd + zd * zd))
    }
}

/// `MovementPredicate`。速度是 `getKnownMovement()` 乘 20。
#[derive(Debug, Clone, Copy, Default)]
pub struct MovementPredicate {
    pub x: DoubleBounds,
    pub y: DoubleBounds,
    pub z: DoubleBounds,
    pub speed: DoubleBounds,
    pub horizontal_speed: DoubleBounds,
    pub vertical_speed: DoubleBounds,
    pub fall_distance: DoubleBounds,
}

impl MovementPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        let bounds = |name: &str| -> Result<DoubleBounds> {
            match field(map, name) {
                Some(value) => DoubleBounds::parse(value, what),
                None => Ok(DoubleBounds::ANY),
            }
        };
        Ok(Self {
            x: bounds("x")?,
            y: bounds("y")?,
            z: bounds("z")?,
            speed: bounds("speed")?,
            horizontal_speed: bounds("horizontal_speed")?,
            vertical_speed: bounds("vertical_speed")?,
            fall_distance: bounds("fall_distance")?,
        })
    }

    pub fn matches(&self, x: f64, y: f64, z: f64, fall_distance: f64) -> bool {
        if !self.x.matches(x) || !self.y.matches(y) || !self.z.matches(z) {
            return false;
        }
        let speed_sqr = x * x + y * y + z * z;
        if !self.speed.matches_sqr(speed_sqr) {
            return false;
        }
        let horizontal_sqr = x * x + z * z;
        if !self.horizontal_speed.matches_sqr(horizontal_sqr) {
            return false;
        }
        self.vertical_speed.matches(y.abs()) && self.fall_distance.matches(fall_distance)
    }
}

/// `EntityTagPredicate`：积分板标签。
#[derive(Debug, Clone, Default)]
pub struct EntityTagPredicate {
    pub any_of: Option<Vec<String>>,
    pub all_of: Option<Vec<String>>,
    pub none_of: Option<Vec<String>>,
}

impl EntityTagPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        let list = |name: &str| -> Result<Option<Vec<String>>> {
            field(map, name)
                .map(|value| super::json::string_list(value, what))
                .transpose()
        };
        Ok(Self {
            any_of: list("any_of")?,
            all_of: list("all_of")?,
            none_of: list("none_of")?,
        })
    }

    pub fn matches(&self, tags: &[String]) -> bool {
        let contains = |tag: &str| tags.iter().any(|actual| actual == tag);
        if self.any_of.as_ref().is_some_and(|any_of| {
            !any_of.iter().any(|tag| contains(tag))
        }) {
            return false;
        }
        if self.none_of.as_ref().is_some_and(|none_of| {
            none_of.iter().any(|tag| contains(tag))
        }) {
            return false;
        }
        self.all_of
            .as_ref()
            .is_none_or(|all_of| all_of.iter().all(|tag| contains(tag)))
    }
}

/// `EntityPredicate`：每个 `minecraft:` 子谓词是表里的一项。
#[derive(Debug, Clone, Default)]
pub struct EntityPredicate {
    pub entity_type: Option<Vec<i32>>,
    pub components: ComponentMatchers,
    pub flags: Option<EntityFlagsPredicate>,
    pub equipment: Option<EntityEquipmentPredicate>,
    pub vehicle: Option<Box<EntityPredicate>>,
    pub passenger: Option<Box<EntityPredicate>>,
    pub targeted_entity: Option<Box<EntityPredicate>>,
    pub location: Option<LocationPredicate>,
    pub stepping_on: Option<LocationPredicate>,
    pub movement_affected_by: Option<LocationPredicate>,
    pub distance: Option<DistancePredicate>,
    pub movement: Option<MovementPredicate>,
    pub cube_mob_size: Option<IntBounds>,
    pub fishing_hook_in_open_water: Option<bool>,
    pub sheep_sheared: Option<bool>,
    pub raider: Option<RaiderPredicate>,
    pub entity_tags: Option<EntityTagPredicate>,
    pub periodic_tick: Option<i32>,
    /// 本实现还不支持的子谓词名字。
    pub unsupported: Vec<String>,
}

/// `RaiderPredicate`。
#[derive(Debug, Clone, Copy)]
pub struct RaiderPredicate {
    pub has_raid: bool,
    pub is_captain: bool,
}

impl RaiderPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        Ok(Self {
            has_raid: opt_bool(map, "has_raid", what, false)?,
            is_captain: opt_bool(map, "is_captain", what, false)?,
        })
    }
}

impl EntityPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        let mut predicate = Self::default();
        for (name, value) in map {
            match name.as_str() {
                "minecraft:entity_type" => {
                    predicate.entity_type =
                        Some(names::holder_set(value, Registry::EntityType, what)?);
                }
                "minecraft:components" => {
                    predicate.components = ComponentMatchers::parse_exact(value, what)?;
                }
                "minecraft:predicates" => {
                    predicate.components = ComponentMatchers::parse_partial(value, what)?;
                }
                "minecraft:flags" => {
                    predicate.flags = Some(EntityFlagsPredicate::parse(value, what)?);
                }
                "minecraft:equipment" => {
                    predicate.equipment = Some(EntityEquipmentPredicate::parse(value, what)?);
                }
                "minecraft:vehicle" => {
                    predicate.vehicle = Some(Box::new(Self::parse(value, what)?));
                }
                "minecraft:passenger" => {
                    predicate.passenger = Some(Box::new(Self::parse(value, what)?));
                }
                "minecraft:targeted_entity" => {
                    predicate.targeted_entity = Some(Box::new(Self::parse(value, what)?));
                }
                "minecraft:location" => {
                    predicate.location = Some(LocationPredicate::parse(value, what)?);
                }
                "minecraft:stepping_on" => {
                    predicate.stepping_on = Some(LocationPredicate::parse(value, what)?);
                }
                "minecraft:movement_affected_by" => {
                    predicate.movement_affected_by = Some(LocationPredicate::parse(value, what)?);
                }
                "minecraft:distance" => {
                    predicate.distance = Some(DistancePredicate::parse(value, what)?);
                }
                "minecraft:movement" => {
                    predicate.movement = Some(MovementPredicate::parse(value, what)?);
                }
                "minecraft:entity_tags" => {
                    predicate.entity_tags = Some(EntityTagPredicate::parse(value, what)?);
                }
                "minecraft:periodic_tick" => {
                    predicate.periodic_tick = Some(i32_value(value, what)?);
                }
                "minecraft:type_specific/cube_mob" => {
                    let map = object(value, what)?;
                    predicate.cube_mob_size = Some(match field(map, "size") {
                        Some(value) => IntBounds::parse(value, what)?,
                        None => IntBounds::ANY,
                    });
                }
                "minecraft:type_specific/sheep" => {
                    let map = object(value, what)?;
                    predicate.sheep_sheared = optional_bool(map, "sheared", what)?;
                }
                "minecraft:type_specific/fishing_hook" => {
                    let map = object(value, what)?;
                    predicate.fishing_hook_in_open_water =
                        optional_bool(map, "in_open_water", what)?;
                }
                "minecraft:type_specific/raider" => {
                    predicate.raider = Some(RaiderPredicate::parse(value, what)?);
                }
                other => predicate.unsupported.push(other.to_owned()),
            }
        }
        Ok(predicate)
    }

    /// 官方 `EntityPredicate.matches(ServerLevel, Vec3, Entity)`：实体为空直接 false。
    pub fn matches(
        &self,
        context: &LootContext<'_, '_, '_>,
        position: Option<Position>,
        entity: Option<&EntityView>,
    ) -> Result<bool> {
        let Some(entity) = entity else {
            return Ok(false);
        };
        if let Some(types) = &self.entity_type {
            if !types.contains(&entity.entity_type) {
                return Ok(false);
            }
        }
        if !self.components.matches_components(&entity.components)? {
            return Ok(false);
        }
        if let Some(flags) = &self.flags {
            if !flags.matches(entity) {
                return Ok(false);
            }
        }
        if let Some(equipment) = &self.equipment {
            if !equipment.matches(entity)? {
                return Ok(false);
            }
        }
        if let Some(vehicle) = &self.vehicle {
            if !vehicle.matches(context, position, entity.vehicle.as_deref())? {
                return Ok(false);
            }
        }
        if let Some(passenger) = &self.passenger {
            // 官方遍历所有乘客，任一匹配就通过；本实现只建模第一个乘客。
            let matches = match entity.passenger.as_deref() {
                Some(passenger_entity) => passenger.matches(context, position, Some(passenger_entity))?,
                None => false,
            };
            if !matches {
                return Ok(false);
            }
        }
        if let Some(targeted) = &self.targeted_entity {
            if !targeted.matches(context, position, entity.targeted_entity.as_deref())? {
                return Ok(false);
            }
        }
        if let Some(location) = &self.location {
            let (x, y, z) = entity_position(entity)?;
            if !location.matches(context, x, y, z)? {
                return Ok(false);
            }
        }
        if let Some(stepping_on) = &self.stepping_on {
            if !entity.is_on_ground {
                return Ok(false);
            }
            let Some((x, y, z)) = entity.on_pos else {
                return Err(unsupported("stepping_on needs EntityView::on_pos"));
            };
            if !stepping_on.matches(
                context,
                f64::from(x) + 0.5,
                f64::from(y) + 0.5,
                f64::from(z) + 0.5,
            )? {
                return Ok(false);
            }
        }
        if let Some(movement_affected_by) = &self.movement_affected_by {
            let Some((x, y, z)) = entity.movement_affected_by_pos else {
                return Err(unsupported(
                    "movement_affected_by needs EntityView::movement_affected_by_pos",
                ));
            };
            if !movement_affected_by.matches(
                context,
                f64::from(x) + 0.5,
                f64::from(y) + 0.5,
                f64::from(z) + 0.5,
            )? {
                return Ok(false);
            }
        }
        if let Some(distance) = &self.distance {
            let Some(from) = position else {
                return Err(unsupported("distance needs the origin parameter"));
            };
            if !distance.matches(from, entity_position_object(entity)?) {
                return Ok(false);
            }
        }
        if let Some(movement) = &self.movement {
            let Some(velocity) = entity.movement else {
                return Err(unsupported("movement needs EntityView::movement"));
            };
            let Some(fall_distance) = entity.fall_distance else {
                return Err(unsupported(
                    "movement needs EntityView::fall_distance",
                ));
            };
            if !movement.matches(
                velocity.x * 20.0,
                velocity.y * 20.0,
                velocity.z * 20.0,
                fall_distance,
            ) {
                return Ok(false);
            }
        }
        if let Some(size) = &self.cube_mob_size {
            let Some(actual) = entity.cube_mob_size else {
                return Ok(false);
            };
            if !size.matches(actual) {
                return Ok(false);
            }
        }
        if let Some(in_open_water) = self.fishing_hook_in_open_water {
            if !entity.is_fishing_hook || entity.in_open_water != in_open_water {
                return Ok(false);
            }
        }
        if let Some(sheared) = self.sheep_sheared {
            if !entity.is_sheep || entity.sheep_sheared != sheared {
                return Ok(false);
            }
        }
        if let Some(raider) = &self.raider {
            if !entity.is_raider
                || entity.has_raid != raider.has_raid
                || entity.is_captain != raider.is_captain
            {
                return Ok(false);
            }
        }
        if let Some(tags) = &self.entity_tags {
            if !tags.matches(&entity.scoreboard_tags) {
                return Ok(false);
            }
        }
        if let Some(periodic_tick) = self.periodic_tick {
            let Some(tick_count) = entity.tick_count else {
                return Err(unsupported("periodic_tick needs EntityView::tick_count"));
            };
            if tick_count % periodic_tick != 0 {
                return Ok(false);
            }
        }
        if !self.unsupported.is_empty() {
            return Err(unsupported(format!(
                "entity predicate part(s) {}",
                self.unsupported.join(", ")
            )));
        }
        Ok(true)
    }
}

fn entity_position(entity: &EntityView) -> Result<(f64, f64, f64)> {
    let position = entity_position_object(entity)?;
    Ok((position.x, position.y, position.z))
}

fn entity_position_object(entity: &EntityView) -> Result<Position> {
    entity
        .position
        .ok_or_else(|| unsupported("entity predicate needs EntityView::position"))
}

/// `DamageSourcePredicate`。
#[derive(Debug, Clone, Default)]
pub struct DamageSourcePredicate {
    /// 伤害类型标签和期望值。
    pub tags: Vec<(Vec<i32>, bool)>,
    pub direct_entity: Option<EntityPredicate>,
    pub source_entity: Option<EntityPredicate>,
    pub is_direct: Option<bool>,
}

impl DamageSourcePredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        let mut tags = Vec::new();
        if let Some(list) = super::json::opt_array(map, "tags", what)? {
            for entry in list {
                let entry = object(entry, what)?;
                let id = required(entry, "id", what)?;
                let types = names::holder_set(id, Registry::DamageType, what)?;
                let expected = required(entry, "expected", what)?
                    .as_bool()
                    .ok_or_else(|| invalid(format!("{what} expected must be a bool")))?;
                tags.push((types, expected));
            }
        }
        Ok(Self {
            tags,
            direct_entity: field(map, "direct_entity")
                .map(|value| EntityPredicate::parse(value, what))
                .transpose()?,
            source_entity: field(map, "source_entity")
                .map(|value| EntityPredicate::parse(value, what))
                .transpose()?,
            is_direct: optional_bool(map, "is_direct", what)?,
        })
    }

    pub fn matches(
        &self,
        context: &LootContext<'_, '_, '_>,
        position: Option<Position>,
        source: &DamageSourceView,
    ) -> Result<bool> {
        for (types, expected) in &self.tags {
            if types.contains(&source.damage_type) != *expected {
                return Ok(false);
            }
        }
        if let Some(predicate) = &self.direct_entity {
            if !predicate.matches(context, position, source.direct_entity.as_deref())? {
                return Ok(false);
            }
        }
        if let Some(predicate) = &self.source_entity {
            if !predicate.matches(context, position, source.source_entity.as_deref())? {
                return Ok(false);
            }
        }
        if let Some(is_direct) = self.is_direct {
            if source.is_direct != is_direct {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
