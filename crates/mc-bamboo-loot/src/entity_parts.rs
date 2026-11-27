//! `EntityPredicate` 里读实体自身状态的几个子谓词：效果、NBT、队伍、闪电、玩家，
//! 以及 `LocationPredicate` 用的流体谓词。
//!
//! 字段名、默认值和求值顺序照官方 `MobEffectsPredicate`、`NbtPredicate`、`TeamPredicate`、
//! `LightningBoltPredicate`、`PlayerPredicate`、`FluidPredicate` 的 codec 和 `matches`。
//! 实体侧的数据来自 [`EntityView`]，按玩家查的统计、配方和进度走 [`LevelView`]；
//! 调用方没给数据时报 [`LootError::Unsupported`]，不猜默认值。
//!
//! The entity sub-predicates that read the entity's own state (effects, NBT, team,
//! lightning, player) and the fluid predicate used by `LocationPredicate`. Field names,
//! defaults and evaluation order follow the official codecs and `matches` methods.
//! Missing data is reported as [`LootError::Unsupported`], never guessed.

use mc_bamboo_blocks::BlockState;
use mc_bamboo_data::registries;
use mc_bamboo_nbt::Tag;
use serde_json::{Map, Value};

use crate::context::{EffectView, EntityView, InputView, LevelView, LootContext, Position};
use crate::json::{field, invalid, object, required, string};
use crate::names::{self, Registry};
use crate::predicate::{DoubleBounds, EntityPredicate, IntBounds, StatePropertiesPredicate};
use crate::{LootError, Result};

fn unsupported(what: impl Into<String>) -> LootError {
    LootError::Unsupported(what.into())
}

/// `Identifier.parse`：没写命名空间时补 `minecraft:`。
///
/// `Identifier.parse`: a missing namespace defaults to `minecraft:`.
pub(crate) fn namespaced(name: &str) -> String {
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    }
}

fn opt_bounds(map: &Map<String, Value>, name: &str, what: &str) -> Result<IntBounds> {
    match field(map, name) {
        Some(value) => IntBounds::parse(value, what),
        None => Ok(IntBounds::ANY),
    }
}

fn opt_flag(map: &Map<String, Value>, name: &str, what: &str) -> Result<Option<bool>> {
    field(map, name)
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| invalid(format!("{what} {name} must be a bool")))
        })
        .transpose()
}

/// `MobEffectsPredicate.MobEffectInstancePredicate`。
#[derive(Debug, Clone)]
struct EffectInstancePredicate {
    amplifier: IntBounds,
    duration: IntBounds,
    ambient: Option<bool>,
    visible: Option<bool>,
}

impl EffectInstancePredicate {
    /// 官方 `matches(@Nullable MobEffectInstance)`：没有这个效果直接 false。
    ///
    /// Official `matches(@Nullable MobEffectInstance)`: a missing effect is false.
    fn matches(&self, instance: Option<&EffectView>) -> bool {
        let Some(instance) = instance else {
            return false;
        };
        self.amplifier.matches(instance.amplifier)
            && self.duration.matches(instance.duration)
            && self
                .ambient
                .is_none_or(|ambient| ambient == instance.ambient)
            && self
                .visible
                .is_none_or(|visible| visible == instance.visible)
    }
}

/// `MobEffectsPredicate`：效果 ID 到实例谓词。
///
/// `MobEffectsPredicate`: effect id to instance predicate.
#[derive(Debug, Clone, Default)]
pub struct MobEffectsPredicate {
    effects: Vec<(i32, EffectInstancePredicate)>,
}

impl MobEffectsPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        let mut effects = Vec::with_capacity(map.len());
        for (name, value) in map {
            let id =
                names::single_registry_id(&Value::String(name.clone()), Registry::MobEffect, what)?;
            let entry = object(value, what)?;
            effects.push((
                id,
                EffectInstancePredicate {
                    amplifier: opt_bounds(entry, "amplifier", what)?,
                    duration: opt_bounds(entry, "duration", what)?,
                    ambient: opt_flag(entry, "ambient", what)?,
                    visible: opt_flag(entry, "visible", what)?,
                },
            ));
        }
        Ok(Self { effects })
    }

    /// 官方 `matches(Entity)`：不是 `LivingEntity` 直接 false。
    ///
    /// Official `matches(Entity)`: false unless the entity is a `LivingEntity`.
    pub fn matches(&self, entity: &EntityView) -> bool {
        entity.is_living
            && self.effects.iter().all(|(id, predicate)| {
                predicate.matches(entity.effects.iter().find(|effect| effect.effect == *id))
            })
    }
}

/// `NbtPredicate`：一个复合标签，按 `NbtUtils.compareNbt(expected, actual, true)` 比。
///
/// `NbtPredicate`: a compound compared with `NbtUtils.compareNbt(expected, actual, true)`.
#[derive(Debug, Clone)]
pub struct NbtPredicate {
    tag: Tag,
}

impl NbtPredicate {
    /// 官方 `TagParser.LENIENT_CODEC`：字符串当 SNBT 解析，必须是复合标签。
    /// JSON 对象形式要按 `JsonOps` → NBT 的数值规则转换，本实现没做，解析时报错。
    ///
    /// Official `TagParser.LENIENT_CODEC`: a string is parsed as SNBT and must be a
    /// compound. The JSON-object form needs the `JsonOps` to NBT number rules, which are
    /// not implemented, so it is rejected at parse time.
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let text = string(value, what)?;
        match mc_bamboo_nbt::parse_snbt(text) {
            Ok(tag @ Tag::Compound(_)) => Ok(Self { tag }),
            Ok(_) => Err(invalid(format!("{what} nbt must be a compound"))),
            Err(error) => Err(invalid(format!("{what} nbt: {}", error.message))),
        }
    }

    pub fn matches(&self, actual: &Tag) -> bool {
        compare_nbt(&self.tag, actual)
    }
}

/// `NbtUtils.compareNbt(expected, actual, true)`。
///
/// 类型不同直接 false；复合标签只要求期望的键都在并且递归匹配；列表是「期望的每一项
/// 都能在实际列表里找到一项匹配」，空的期望列表只匹配空列表；其余按值相等。
///
/// `NbtUtils.compareNbt(expected, actual, true)`. Different types never match. A compound
/// needs every expected key to be present and to match recursively. A list needs each
/// expected element to match some actual element, and an empty expected list matches only
/// an empty list. Everything else is compared by value.
pub fn compare_nbt(expected: &Tag, actual: &Tag) -> bool {
    match (expected, actual) {
        (Tag::Compound(expected), Tag::Compound(actual)) => {
            actual.len() >= expected.len()
                && expected.iter().all(|(name, value)| {
                    actual
                        .iter()
                        .find(|(key, _)| key == name)
                        .is_some_and(|(_, found)| compare_nbt(value, found))
                })
        }
        (Tag::List(expected), Tag::List(actual)) => {
            if expected.is_empty() {
                return actual.is_empty();
            }
            actual.len() >= expected.len()
                && expected
                    .iter()
                    .all(|value| actual.iter().any(|found| compare_nbt(value, found)))
        }
        // `FloatTag` / `DoubleTag` 是 record，相等按 `Float.compare`：NaN 等于 NaN，
        // 正零不等于负零，所以按位比较。
        // `FloatTag` and `DoubleTag` are records whose equality uses `Float.compare`:
        // NaN equals NaN and +0 differs from -0, hence the bit comparison.
        (Tag::Float(expected), Tag::Float(actual)) => expected.to_bits() == actual.to_bits(),
        (Tag::Double(expected), Tag::Double(actual)) => expected.to_bits() == actual.to_bits(),
        _ => expected == actual,
    }
}

/// `LightningBoltPredicate`。
#[derive(Debug, Clone)]
pub struct LightningBoltPredicate {
    blocks_set_on_fire: IntBounds,
    entity_struck: Option<Box<EntityPredicate>>,
}

impl LightningBoltPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        Ok(Self {
            blocks_set_on_fire: opt_bounds(map, "blocks_set_on_fire", what)?,
            entity_struck: field(map, "entity_struck")
                .map(|value| EntityPredicate::parse(value, what).map(Box::new))
                .transpose()?,
        })
    }

    /// 官方 `matches`：不是 `LightningBolt` 直接 false；`entity_struck` 是被劈中的实体里
    /// 任意一个匹配。
    ///
    /// Official `matches`: false unless the entity is a `LightningBolt`; `entity_struck`
    /// passes when any struck entity matches.
    pub fn matches(
        &self,
        context: &LootContext<'_, '_, '_>,
        position: Option<Position>,
        entity: &EntityView,
    ) -> Result<bool> {
        let Some(bolt) = &entity.lightning else {
            return Ok(false);
        };
        if !self.blocks_set_on_fire.matches(bolt.blocks_set_on_fire) {
            return Ok(false);
        }
        let Some(struck) = &self.entity_struck else {
            return Ok(true);
        };
        for hit in &bolt.hit_entities {
            if struck.matches(context, position, Some(hit))? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// `PlayerPredicate.AdvancementPredicate`：布尔是整个进度完没完成，对象是逐条判据。
///
/// `PlayerPredicate.AdvancementPredicate`: a bool tests the whole advancement, an object
/// tests criteria one by one.
#[derive(Debug, Clone)]
enum AdvancementPredicate {
    Done(bool),
    Criteria(Vec<(String, bool)>),
}

/// `InputPredicate`，七个可选按键。
///
/// `InputPredicate`, seven optional keys.
#[derive(Debug, Clone, Copy, Default)]
struct InputPredicate {
    forward: Option<bool>,
    backward: Option<bool>,
    left: Option<bool>,
    right: Option<bool>,
    jump: Option<bool>,
    sneak: Option<bool>,
    sprint: Option<bool>,
}

impl InputPredicate {
    fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        Ok(Self {
            forward: opt_flag(map, "forward", what)?,
            backward: opt_flag(map, "backward", what)?,
            left: opt_flag(map, "left", what)?,
            right: opt_flag(map, "right", what)?,
            jump: opt_flag(map, "jump", what)?,
            sneak: opt_flag(map, "sneak", what)?,
            sprint: opt_flag(map, "sprint", what)?,
        })
    }

    fn matches(&self, input: InputView) -> bool {
        let same = |wanted: Option<bool>, actual: bool| wanted.is_none_or(|value| value == actual);
        same(self.forward, input.forward)
            && same(self.backward, input.backward)
            && same(self.left, input.left)
            && same(self.right, input.right)
            && same(self.jump, input.jump)
            && same(self.sneak, input.sneak)
            && same(self.sprint, input.sprint)
    }
}

/// `PlayerPredicate`。
#[derive(Debug, Clone)]
pub struct PlayerPredicate {
    level: IntBounds,
    food_level: IntBounds,
    saturation: DoubleBounds,
    /// `None` 是官方的 `GameTypePredicate.ANY`。
    game_modes: Option<Vec<String>>,
    /// （统计类型，统计项，范围）。
    stats: Vec<(String, String, IntBounds)>,
    recipes: Vec<(String, bool)>,
    advancements: Vec<(String, AdvancementPredicate)>,
    looking_at: Option<Box<EntityPredicate>>,
    input: Option<InputPredicate>,
}

const GAME_MODES: [&str; 4] = ["survival", "creative", "adventure", "spectator"];

impl PlayerPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        let (food_level, saturation) = match field(map, "food") {
            Some(food) => {
                let food = object(food, what)?;
                let saturation = match field(food, "saturation") {
                    Some(value) => DoubleBounds::parse(value, what)?,
                    None => DoubleBounds::ANY,
                };
                (opt_bounds(food, "level", what)?, saturation)
            }
            None => (IntBounds::ANY, DoubleBounds::ANY),
        };
        Ok(Self {
            level: opt_bounds(map, "level", what)?,
            food_level,
            saturation,
            game_modes: field(map, "gamemode")
                .map(|value| parse_game_modes(value, what))
                .transpose()?,
            stats: match field(map, "stats") {
                Some(value) => parse_stats(value, what)?,
                None => Vec::new(),
            },
            recipes: match field(map, "recipes") {
                Some(value) => parse_bool_map(value, what)?
                    .into_iter()
                    .map(|(name, state)| (namespaced(&name), state))
                    .collect(),
                None => Vec::new(),
            },
            advancements: match field(map, "advancements") {
                Some(value) => parse_advancements(value, what)?,
                None => Vec::new(),
            },
            looking_at: field(map, "looking_at")
                .map(|value| EntityPredicate::parse(value, what).map(Box::new))
                .transpose()?,
            input: field(map, "input")
                .map(|value| InputPredicate::parse(value, what))
                .transpose()?,
        })
    }

    /// 官方 `matches`，检查顺序：等级、饥饿、游戏模式、统计、配方、进度、注视、按键。
    /// 不是 `ServerPlayer` 直接 false。
    ///
    /// Official `matches`, in its order: level, food, game mode, stats, recipes,
    /// advancements, looking-at, input. False unless the entity is a `ServerPlayer`.
    pub fn matches(&self, context: &LootContext<'_, '_, '_>, entity: &EntityView) -> Result<bool> {
        let Some(player) = &entity.player else {
            return Ok(false);
        };
        if !self.level.matches(player.experience_level)
            || !self.food_level.matches(player.food_level)
            || !self.saturation.matches(f64::from(player.saturation))
        {
            return Ok(false);
        }
        if let Some(modes) = &self.game_modes
            && !modes.iter().any(|mode| mode == player.game_mode)
        {
            return Ok(false);
        }
        if !self.matches_records(context, entity)? {
            return Ok(false);
        }
        if let Some(looking_at) = &self.looking_at {
            let Some(seen) = &player.looking_at else {
                return Err(unsupported("looking_at needs PlayerView::looking_at"));
            };
            // 官方 `lookingAt.matches(player, entity)` 的位置参数是玩家自己的位置。
            // Official `lookingAt.matches(player, entity)` passes the player's own position.
            if !looking_at.matches(context, entity.position, seen.as_deref())? {
                return Ok(false);
            }
        }
        match &self.input {
            None => Ok(true),
            Some(input) => match player.input {
                Some(actual) => Ok(input.matches(actual)),
                None => Err(unsupported("input needs PlayerView::input")),
            },
        }
    }

    /// 统计、配方、进度：都按玩家的积分板名字向世界视图查。
    ///
    /// Stats, recipes and advancements, all queried by the player's scoreboard name.
    fn matches_records(
        &self,
        context: &LootContext<'_, '_, '_>,
        entity: &EntityView,
    ) -> Result<bool> {
        if self.stats.is_empty() && self.recipes.is_empty() && self.advancements.is_empty() {
            return Ok(true);
        }
        let level = context.level("player predicate")?;
        let name = entity
            .scoreboard_name
            .as_deref()
            .ok_or_else(|| unsupported("player predicate needs EntityView::scoreboard_name"))?;
        for (stat_type, stat, range) in &self.stats {
            let value = level
                .player_stat(name, stat_type, stat)
                .ok_or_else(|| unsupported("stats needs LevelView::player_stat"))?;
            if !range.matches(value) {
                return Ok(false);
            }
        }
        for (recipe, wanted) in &self.recipes {
            let has = level
                .player_has_recipe(name, recipe)
                .ok_or_else(|| unsupported("recipes needs LevelView::player_has_recipe"))?;
            if has != *wanted {
                return Ok(false);
            }
        }
        for (id, predicate) in &self.advancements {
            if !advancement_matches(level, name, id, predicate)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

fn advancement_matches(
    level: &dyn LevelView,
    player: &str,
    id: &str,
    predicate: &AdvancementPredicate,
) -> Result<bool> {
    let progress = level
        .player_advancement(player, id)
        .ok_or_else(|| unsupported("advancements needs LevelView::player_advancement"))?;
    // 服务器没有这个进度：官方 `advancement == null` 判 false。
    // The server has no such advancement: vanilla's `advancement == null` is false.
    let Some(progress) = progress else {
        return Ok(false);
    };
    Ok(match predicate {
        AdvancementPredicate::Done(state) => progress.done == *state,
        AdvancementPredicate::Criteria(criteria) => criteria
            .iter()
            .all(|(name, state)| progress.criteria.get(name) == Some(state)),
    })
}

fn parse_game_modes(value: &Value, what: &str) -> Result<Vec<String>> {
    let list = value
        .as_array()
        .ok_or_else(|| invalid(format!("{what} gamemode must be a list")))?;
    list.iter()
        .map(|entry| {
            let name = string(entry, what)?;
            if GAME_MODES.contains(&name) {
                Ok(name.to_owned())
            } else {
                Err(invalid(format!("unknown game mode `{name}` in {what}")))
            }
        })
        .collect()
}

fn parse_stats(value: &Value, what: &str) -> Result<Vec<(String, String, IntBounds)>> {
    let list = value
        .as_array()
        .ok_or_else(|| invalid(format!("{what} stats must be a list")))?;
    list.iter()
        .map(|entry| {
            let entry = object(entry, what)?;
            Ok((
                namespaced(string(required(entry, "type", what)?, what)?),
                namespaced(string(required(entry, "stat", what)?, what)?),
                opt_bounds(entry, "value", what)?,
            ))
        })
        .collect()
}

fn parse_bool_map(value: &Value, what: &str) -> Result<Vec<(String, bool)>> {
    object(value, what)?
        .iter()
        .map(|(name, state)| {
            state
                .as_bool()
                .map(|state| (name.clone(), state))
                .ok_or_else(|| invalid(format!("{what} `{name}` must be a bool")))
        })
        .collect()
}

fn parse_advancements(value: &Value, what: &str) -> Result<Vec<(String, AdvancementPredicate)>> {
    object(value, what)?
        .iter()
        .map(|(id, entry)| {
            let predicate = match entry {
                Value::Bool(state) => AdvancementPredicate::Done(*state),
                other => AdvancementPredicate::Criteria(parse_bool_map(other, what)?),
            };
            Ok((namespaced(id), predicate))
        })
        .collect()
}

/// `FluidPredicate`。
#[derive(Debug, Clone, Default)]
pub struct FluidPredicate {
    fluids: Option<Vec<i32>>,
    properties: Option<StatePropertiesPredicate>,
}

impl FluidPredicate {
    pub fn parse(value: &Value, what: &str) -> Result<Self> {
        let map = object(value, what)?;
        Ok(Self {
            fluids: names::opt_holder_set(map, "fluids", Registry::Fluid, what)?,
            properties: field(map, "state")
                .map(|value| StatePropertiesPredicate::parse(value, what))
                .transpose()?,
        })
    }

    /// 官方 `matches(ServerLevel, BlockPos)`：没加载直接 false，然后流体集合，再属性。
    ///
    /// Official `matches(ServerLevel, BlockPos)`: false when unloaded, then the fluid
    /// set, then the state properties.
    pub fn matches(&self, level: &dyn LevelView, x: i32, y: i32, z: i32) -> Result<bool> {
        match level.is_loaded(x, y, z) {
            Some(true) => {}
            Some(false) => return Ok(false),
            None => return Err(unsupported("fluid predicate needs LevelView::is_loaded")),
        }
        let state = level
            .block_state(x, y, z)
            .ok_or_else(|| unsupported("fluid predicate needs LevelView::block_state"))?;
        Ok(self.matches_state(state))
    }

    fn matches_state(&self, state: BlockState) -> bool {
        let fluid = state.fluid();
        if let Some(fluids) = &self.fluids
            && !fluids.contains(&fluid.fluid())
        {
            return false;
        }
        self.properties
            .as_ref()
            .is_none_or(|properties| properties.matches_values(&fluid_properties(state)))
    }
}

/// `FluidState` 的属性表：空流体没有属性，水源和岩浆源只有 `falling`，流动的两种还有
/// `level`（1–8，就是 `getAmount`）。
///
/// The property table of a `FluidState`. The empty fluid has none; the two sources have
/// only `falling`; the two flowing fluids also have `level` (1-8, which is `getAmount`).
fn fluid_properties(state: BlockState) -> Vec<(&'static str, PropertyValue)> {
    let fluid = state.fluid();
    let mut out = Vec::new();
    if fluid.fluid() == registries::fluid::EMPTY {
        return out;
    }
    out.push(("falling", PropertyValue::Bool(fluid.is_falling())));
    if !fluid.is_source() {
        out.push(("level", PropertyValue::Int(fluid.amount(), 1, 8)));
    }
    out
}

/// 一个属性的当前值。整数带上属性允许的范围，`Property.getValue` 拒绝范围外的文本。
///
/// The current value of one property. An integer carries the range the property allows,
/// because `Property.getValue` rejects text outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PropertyValue {
    Bool(bool),
    Int(i32, i32, i32),
}

impl PropertyValue {
    /// `actual.compareTo(property.getValue(text))`；文本不是这个属性的合法值时返回 `None`。
    ///
    /// `actual.compareTo(property.getValue(text))`, or `None` when the text is not a
    /// valid value of the property.
    pub(crate) fn compare(self, text: &str) -> Option<std::cmp::Ordering> {
        match self {
            Self::Bool(actual) => match text {
                "true" => Some(actual.cmp(&true)),
                "false" => Some(actual.cmp(&false)),
                _ => None,
            },
            Self::Int(actual, min, max) => text
                .parse::<i32>()
                .ok()
                .filter(|value| (min..=max).contains(value))
                .map(|value| actual.cmp(&value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_nbt::parse_snbt;

    fn tag(text: &str) -> Tag {
        parse_snbt(text).expect("valid SNBT")
    }

    #[test]
    fn compare_nbt_follows_the_official_partial_rules() {
        let actual = tag("{a:1,b:[1,2,3],c:{d:1b,e:\"x\"},f:[]}");
        assert!(compare_nbt(&tag("{}"), &actual));
        assert!(compare_nbt(&tag("{a:1}"), &actual));
        assert!(!compare_nbt(&tag("{a:1b}"), &actual), "类型不同不算相等");
        assert!(compare_nbt(&tag("{b:[3,1]}"), &actual), "列表是子集匹配");
        assert!(!compare_nbt(&tag("{b:[4]}"), &actual));
        assert!(!compare_nbt(&tag("{b:[]}"), &actual), "空列表只匹配空列表");
        assert!(compare_nbt(&tag("{f:[]}"), &actual));
        assert!(compare_nbt(&tag("{c:{e:\"x\"}}"), &actual));
        assert!(!compare_nbt(&tag("{missing:1}"), &actual));
    }

    #[test]
    fn effects_need_a_living_entity_and_every_listed_effect() {
        let predicate = MobEffectsPredicate::parse(
            &serde_json::json!({"minecraft:speed": {"amplifier": {"min": 1}, "visible": true}}),
            "test",
        )
        .expect("parses");
        let mut entity = EntityView::new(0);
        entity.effects.push(EffectView {
            effect: registries::mob_effect::SPEED,
            amplifier: 1,
            duration: 600,
            ambient: false,
            visible: true,
        });
        assert!(!predicate.matches(&entity), "非生物直接 false");
        entity.is_living = true;
        assert!(predicate.matches(&entity));
        entity.effects[0].amplifier = 0;
        assert!(!predicate.matches(&entity));
        entity.effects.clear();
        assert!(!predicate.matches(&entity), "没有这个效果是 false");
    }

    #[test]
    fn fluid_properties_follow_the_fluid_state_definition() {
        assert_eq!(
            PropertyValue::Bool(true).compare("true"),
            Some(std::cmp::Ordering::Equal)
        );
        assert_eq!(PropertyValue::Bool(true).compare("yes"), None);
        assert_eq!(
            PropertyValue::Int(3, 1, 8).compare("9"),
            None,
            "范围外的文本不是合法值"
        );
        assert_eq!(
            PropertyValue::Int(3, 1, 8).compare("5"),
            Some(std::cmp::Ordering::Less)
        );
    }
}
