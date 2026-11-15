//! `BlockStateProvider`：官方 `BlockStateProvider.DIRECT_CODEC` 是
//! `Codec.xor(BlockState.FULL_CODEC, TYPED_CODEC)`。裸方块状态（`{"id":...}`）是
//! `SimpleStateProvider`；带 `type` 的对象按类型分派：`simple`、`rule_based`（DiskFeature
//! 用 `getOptionalState`，规则都不中时回退）、`weighted`、`randomized_int`、`random_block`。
//! 字符串形式是 `Holder<BlockStateProvider>`，按名字查 `block_state_provider` 注册表。
//!
//! `BlockStateProvider`: official `DIRECT_CODEC` is
//! `xor(BlockState.FULL_CODEC, TYPED_CODEC)`. A bare block state is a `SimpleStateProvider`;
//! typed objects dispatch to `simple`, `rule_based` (used by `DiskFeature.getOptionalState`),
//! `weighted`, `randomized_int` and `random_block`. A string is a
//! `Holder<BlockStateProvider>` resolved by name against the `block_state_provider` registry.
//! Unported typed kinds return `None`.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    block_predicate::BlockPredicate,
    noise_state::NoiseStateProvider,
    provider::IntProvider,
    state::{parse_state, with_property},
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// 一个已解析的 `BlockStateProvider`。
///
/// A parsed `BlockStateProvider`.
#[derive(Debug, Clone)]
pub enum StateProvider {
    /// `SimpleStateProvider`：固定状态，不消费随机数。
    Simple(u32),
    /// `RuleBasedStateProvider`：按顺序试规则，都不中再用 fallback。
    RuleBased {
        fallback: Option<Box<StateProvider>>,
        rules: Vec<(BlockPredicate, StateProvider)>,
    },
    /// `WeightedStateProvider`：`(weight, state)` 列表。
    Weighted {
        entries: Vec<(i32, u32)>,
        total: i32,
    },
    /// `RandomizedIntStateProvider`：source + 整数属性名 + 值提供者。
    RandomizedInt {
        source: Box<StateProvider>,
        property: String,
        values: IntProvider,
    },
    /// `RandomBlockProvider`：从方块集合里等概率取一个，用它的默认状态。
    ///
    /// `RandomBlockProvider`: pick one block uniformly and use its default state.
    RandomBlock(Vec<u32>),
    /// `NoiseProvider` / `DualNoiseProvider` / `NoiseThresholdProvider`（按坐标用噪声）。
    ///
    /// The three noise-based providers, which pick a block from a noise sample at `pos`.
    Noise(Box<NoiseStateProvider>),
    /// `RotatedBlockProvider`：先采样内层，再把方向相关属性改掉。
    ///
    /// `RotatedBlockProvider`: sample the inner provider, then overwrite the
    /// direction-dependent properties.
    Rotated {
        source: Box<StateProvider>,
        direction: Option<&'static str>,
    },
}

impl StateProvider {
    /// 解析 `to_place`；未移植的形态返回 `None`。字符串 holder 引用按「没有注册表」
    /// 处理，保持旧行为。
    ///
    /// Parse `to_place`; return `None` for unported shapes. A string holder reference is
    /// treated as "no registry available", keeping the old behaviour.
    pub fn parse(value: &Value) -> Option<Self> {
        Self::parse_with(value, &|_| None)
    }

    /// 带注册表查找的解析：字符串形式是 `Holder<BlockStateProvider>`，按名字查
    /// `block_state_provider` 注册表后再递归解析。
    ///
    /// Parse with a registry lookup: the string form is a `Holder<BlockStateProvider>`,
    /// resolved by name against the `block_state_provider` registry and parsed recursively.
    pub fn parse_with<'a>(
        value: &'a Value,
        lookup: &dyn Fn(&str) -> Option<&'a Value>,
    ) -> Option<Self> {
        let object = match value {
            Value::String(name) => return Self::parse_with(lookup(name)?, lookup),
            Value::Object(object) => object,
            _ => return None,
        };
        let Some(kind) = object.get("type").and_then(Value::as_str) else {
            // 裸方块状态没有 `type`，由 DIRECT_CODEC 的 xor 包成 SimpleStateProvider。
            // A bare block state has no `type` and is wrapped into SimpleStateProvider.
            return Some(Self::Simple(parse_state(value).ok()?));
        };
        Some(match kind {
            "minecraft:simple" => Self::Simple(parse_state(object.get("state")?).ok()?),
            "minecraft:rule_based" => {
                let fallback = match object.get("fallback") {
                    Some(value) => Some(Box::new(Self::parse_with(value, lookup)?)),
                    None => None,
                };
                let rules = object
                    .get("rules")?
                    .as_array()?
                    .iter()
                    .map(|rule| {
                        let rule = rule.as_object()?;
                        let predicate = BlockPredicate::parse(rule.get("if_true")?);
                        let then = Self::parse_with(rule.get("then")?, lookup)?;
                        Some((predicate, then))
                    })
                    .collect::<Option<Vec<_>>>()?;
                Self::RuleBased { fallback, rules }
            }
            "minecraft:weighted" => {
                let entries = object
                    .get("entries")?
                    .as_array()?
                    .iter()
                    .map(|entry| {
                        let weight = entry.get("weight")?.as_i64()? as i32;
                        Some((weight, parse_state(entry.get("data")?).ok()?))
                    })
                    .collect::<Option<Vec<_>>>()?;
                let total: i32 = entries.iter().map(|(weight, _)| *weight).sum();
                Self::Weighted { entries, total }
            }
            "minecraft:randomized_int" => Self::RandomizedInt {
                source: Box::new(Self::parse_with(object.get("source")?, lookup)?),
                property: object.get("property")?.as_str()?.to_owned(),
                values: IntProvider::parse(object.get("values")?).ok()?,
            },
            // `RandomBlockProvider`：`blocks` 是 `HolderSet<Block>`，按默认状态取。
            // `RandomBlockProvider`: `blocks` is a `HolderSet<Block>`; use defaults.
            "minecraft:random_block" => {
                Self::RandomBlock(parse_block_states(object.get("blocks")?)?)
            }
            // 三个 noise provider 的 `noise` / `slow_noise` 是内联参数（不是注册表引用），
            // 所以直接按值解析，不需要 `lookup`。
            // The three noise providers carry inline `NormalNoise` parameters, so they
            // parse by value and need no `lookup`.
            "minecraft:noise" | "minecraft:dual_noise" | "minecraft:noise_threshold" => {
                Self::Noise(Box::new(NoiseStateProvider::parse(value)?))
            }
            // `RotatedBlockProvider.CODEC`：`state` + 可选 `direction`。
            // `RotatedBlockProvider.CODEC`: `state` plus an optional `direction`.
            "minecraft:rotated" => Self::Rotated {
                source: Box::new(Self::parse_with(object.get("state")?, lookup)?),
                direction: match object.get("direction") {
                    Some(value) => Some(parse_direction(value.as_str()?)?),
                    None => None,
                },
            },
            _ => return None,
        })
    }

    /// 是否全部子提供者 / 谓词都已移植。
    ///
    /// Whether every nested provider / predicate is ported.
    pub fn supported(&self) -> bool {
        match self {
            Self::Simple(_) | Self::Weighted { .. } | Self::RandomBlock(_) | Self::Noise(_) => true,
            Self::Rotated { source, .. } => source.supported(),
            Self::RandomizedInt { source, .. } => source.supported(),
            Self::RuleBased { fallback, rules } => {
                fallback.as_deref().is_none_or(StateProvider::supported)
                    && rules.iter().all(|(predicate, provider)| {
                        predicate.is_supported() && provider.supported()
                    })
            }
        }
    }

    /// 所有可能放出的状态（用于检查存活判定是否都已移植）；整数属性值不改变方块类。
    ///
    /// Every state this provider may produce (for the survival-check gate); integer
    /// property values do not change the block class.
    pub fn possible_states(&self) -> Vec<u32> {
        match self {
            Self::Simple(state) => vec![*state],
            Self::Weighted { entries, .. } => entries.iter().map(|(_, state)| *state).collect(),
            Self::RandomizedInt { source, .. } => source.possible_states(),
            Self::RandomBlock(states) => states.clone(),
            Self::Noise(provider) => provider.possible_states(),
            // 方向属性不改变方块类，内层 provider 的状态就够了。
            // The direction properties do not change the block class.
            Self::Rotated { source, .. } => source.possible_states(),
            Self::RuleBased { fallback, rules } => {
                let mut result = Vec::new();
                for (_, provider) in rules {
                    result.extend(provider.possible_states());
                }
                if let Some(fallback) = fallback {
                    result.extend(fallback.possible_states());
                }
                result
            }
        }
    }

    /// 官方 `BlockStateProvider.getState(level, random, pos)` 在「不需要世界」时的形态。
    /// `rule_based` 要读世界和位置，这里返回 `None`（它走 [`Self::get_optional_state`]）。
    ///
    /// The world-free form of `BlockStateProvider.getState(level, random, pos)`.
    /// `rule_based` needs the world and position, so it returns `None` here and goes
    /// through [`Self::get_optional_state`] instead.
    pub fn get_state(&self, random: &WorldgenRandom) -> Option<u32> {
        match self {
            Self::Simple(state) => Some(*state),
            // `WeightedList.getRandomOrThrow`：nextInt(totalWeight) + 累计权重轮盘。
            // `WeightedList.getRandomOrThrow`: nextInt(totalWeight) + cumulative weights.
            Self::Weighted { entries, total } => {
                if *total == 0 {
                    return None;
                }
                let mut selection = random.next_int(*total);
                for (weight, state) in entries {
                    selection -= *weight;
                    if selection < 0 {
                        return Some(*state);
                    }
                }
                None
            }
            // 先采样 source（可能消费随机数），再 `values.sample` 改整数属性。
            // Sample the source (may consume random), then overwrite the int property.
            Self::RandomizedInt {
                source,
                property,
                values,
            } => {
                let base = source.get_state(random)?;
                let value = values.sample(random);
                with_property(base, property, &value.to_string())
            }
            // `HolderSet.getRandomElement(random)`：`nextInt(size)` 等概率取一个。
            // `HolderSet.getRandomElement(random)`: one `nextInt(size)` uniform pick.
            Self::RandomBlock(states) => {
                if states.is_empty() {
                    return None;
                }
                let index = random.next_int(states.len() as i32) as usize;
                states.get(index).copied()
            }
            // `Rotated` 要先定 `direction`（可能消费随机数）再采样，走 `get_optional_state`。
            // `Rotated` resolves `direction` first (possibly consuming random) and then
            // samples, so it goes through `get_optional_state`.
            // `Noise` 的 `getState` 不读世界，直接按坐标采样。
            // `Noise.getState` needs no world; it samples at `pos` only through
            // `get_optional_state`, so the world-free form returns `None`.
            Self::Noise(_) => None,
            Self::Rotated { .. } | Self::RuleBased { .. } => None,
        }
    }

    /// `BlockStateProvider.getOptionalState(level, random, pos)`；`None` 表示不放。
    /// `rule_based` 按顺序试 `if_true`，命中则取对应 `then`，都不中再用 fallback。
    ///
    /// `getOptionalState(level, random, pos)`; `None` means place nothing. `rule_based`
    /// tries each `if_true` in order, then falls back.
    pub fn get_optional_state(
        &self,
        world: &FeatureWorld<'_>,
        random: &WorldgenRandom,
        pos: Pos,
    ) -> Option<u32> {
        match self {
            Self::Simple(state) => Some(*state),
            Self::Weighted { .. } | Self::RandomizedInt { .. } | Self::RandomBlock(_) => {
                self.get_state(random)
            }
            Self::Noise(provider) => provider.get_state(random, pos),
            // 官方先把 `direction` 定下来（缺省时 `Direction.getRandom(random)` 消费一次
            // `nextInt(6)`），再采样内层 provider，最后依次尝试 `axis` / `facing` /
            // `horizontal_facing` 三个属性（有该属性才改）。
            // The official code resolves `direction` first (consuming one `nextInt(6)` when
            // absent), samples the inner provider, then tries `axis`, `facing` and
            // `horizontal_facing` in that order, setting only properties the state has.
            Self::Rotated { source, direction } => {
                let direction = match direction {
                    Some(direction) => *direction,
                    None => DIRECTIONS[random.next_int(6) as usize],
                };
                let base = source.get_optional_state(world, random, pos)?;
                let state = with_property(base, "axis", axis_of(direction)).unwrap_or(base);
                let state = with_property(state, "facing", direction).unwrap_or(state);
                Some(if is_horizontal(direction) {
                    with_property(state, "horizontal_facing", direction).unwrap_or(state)
                } else {
                    state
                })
            }
            Self::RuleBased { fallback, rules } => {
                for (predicate, provider) in rules {
                    if predicate.test(world, pos)
                        && let Some(state) = provider.get_optional_state(world, random, pos)
                    {
                        return Some(state);
                    }
                }
                fallback
                    .as_deref()
                    .and_then(|provider| provider.get_optional_state(world, random, pos))
            }
        }
    }
}

/// `Direction.values()` 的六个方向，顺序即 ordinal。
///
/// The six `Direction.values()` entries in ordinal order.
const DIRECTIONS: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// `Direction.CODEC` 的方向名。
///
/// A name accepted by `Direction.CODEC`.
fn parse_direction(name: &str) -> Option<&'static str> {
    DIRECTIONS.iter().copied().find(|entry| *entry == name)
}

/// `Direction.getAxis().getName()`。
///
/// `Axis.getName()` for `Direction.getAxis()`.
fn axis_of(direction: &str) -> &'static str {
    match direction {
        "up" | "down" => "y",
        "north" | "south" => "z",
        _ => "x",
    }
}

/// `Direction.Axis.isHorizontal()`。
///
/// `Direction.Axis.isHorizontal()`.
fn is_horizontal(direction: &str) -> bool {
    !matches!(direction, "up" | "down")
}

/// `HolderSet<Block>`：`#tag`、单个方块名，或方块名/`{"id":..}` 数组；结果取默认状态。
///
/// `HolderSet<Block>`: a `#tag`, one block name, or an array of names / `{"id":..}`
/// entries; the result holds default states.
fn parse_block_states(value: &Value) -> Option<Vec<u32>> {
    match value {
        Value::String(name) => {
            let mut out = Vec::new();
            push_block_states(name, &mut out)?;
            Some(out)
        }
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                let name = match item {
                    Value::String(name) => name.as_str(),
                    Value::Object(object) => object.get("id")?.as_str()?,
                    _ => return None,
                };
                push_block_states(name, &mut out)?;
            }
            Some(out)
        }
        // `HolderSet` 也可能编码成一个对象（`{"tag": ..}` / `{"contents": [..]}`）。
        // A `HolderSet` may also encode as an object (`{"tag": ..}` / `{"contents": [..]}`).
        Value::Object(object) => {
            if let Some(tag) = object.get("tag").and_then(Value::as_str) {
                let ids = mc_bamboo_data::block_tag(tag)?;
                let mut out = Vec::with_capacity(ids.len());
                for id in ids {
                    let block = mc_bamboo_data::blocks::BLOCKS.get(*id as usize)?;
                    out.push(block.default_state);
                }
                return Some(out);
            }
            parse_block_states(object.get("contents")?)
        }
        _ => None,
    }
}

fn push_block_states(name: &str, out: &mut Vec<u32>) -> Option<()> {
    if let Some(tag) = name.strip_prefix('#') {
        for id in mc_bamboo_data::block_tag(tag)? {
            let block = mc_bamboo_data::blocks::BLOCKS.get(*id as usize)?;
            out.push(block.default_state);
        }
    } else {
        out.push(mc_bamboo_data::block_by_name(name)?.default_state);
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::Algorithm;
    use serde_json::json;

    fn random() -> WorldgenRandom {
        WorldgenRandom::new(Algorithm::Xoroshiro.new_instance(42))
    }

    #[test]
    fn parses_inline_bare_state_and_gates_holder_reference() {
        let bare: Value = json!({"id": "minecraft:poppy"});
        assert!(matches!(
            StateProvider::parse(&bare),
            Some(StateProvider::Simple(_))
        ));
        let holder: Value = json!("minecraft:flower_plain");
        assert!(StateProvider::parse(&holder).is_none());
        let noise: Value = json!({"type": "minecraft:noise", "seed": 1});
        assert!(StateProvider::parse(&noise).is_none());
    }

    #[test]
    fn holder_string_is_resolved_through_the_lookup() {
        // 字符串 holder 在有注册表时解析成被引用的 provider。
        // A string holder parses to the referenced provider when the lookup knows it.
        let referenced: Value = json!({"id": "minecraft:poppy"});
        let holder: Value = json!("minecraft:test_provider");
        let provider = StateProvider::parse_with(&holder, &|name| {
            (name == "minecraft:test_provider").then_some(&referenced)
        })
        .unwrap();
        assert_eq!(provider.possible_states().len(), 1);
    }

    #[test]
    fn parses_random_block_from_a_tag() {
        // `coral/block_decoration` uses `{"blocks": "#minecraft:corals", "type":
        // "random_block"}`.
        let value: Value = serde_json::json!({
            "blocks": "#minecraft:corals",
            "type": "minecraft:random_block",
        });
        let provider = StateProvider::parse(&value).unwrap();
        let states = provider.possible_states();
        assert!(!states.is_empty());
        assert!(states.iter().all(|&state| state != 0));
        // 同一个种子源上，每次取一个状态并各消费一次 nextInt。
        // Each call picks one state and consumes exactly one nextInt.
        let a = random();
        let b = random();
        for _ in 0..50 {
            let expected = b.next_int(states.len() as i32) as usize;
            assert_eq!(provider.get_state(&a), Some(states[expected]));
        }
    }

    #[test]
    fn random_block_rejects_unknown_blocks() {
        let value: Value = serde_json::json!({
            "blocks": ["minecraft:not_a_block"],
            "type": "minecraft:random_block",
        });
        assert!(StateProvider::parse(&value).is_none());
    }

    #[test]
    fn parses_weighted_and_randomized_int() {
        let weighted: Value = json!({
            "entries": [
                {"data": "minecraft:poppy", "weight": 2},
                {"data": "minecraft:dandelion", "weight": 1},
            ],
            "type": "minecraft:weighted",
        });
        let provider = StateProvider::parse(&weighted).unwrap();
        assert_eq!(provider.possible_states().len(), 2);
        let randomized: Value = json!({
            "property": "pickles",
            "source": {"id": "minecraft:sea_pickle", "properties": {"pickles": "1", "waterlogged": "true"}},
            "type": "minecraft:randomized_int",
            "values": {"min_inclusive": 1, "max_inclusive": 4, "type": "minecraft:uniform"},
        });
        assert!(matches!(
            StateProvider::parse(&randomized),
            Some(StateProvider::RandomizedInt { .. })
        ));
    }

    #[test]
    fn parses_rule_based_disk_provider() {
        let sand: Value = json!({
            "fallback": {"id": "minecraft:sand"},
            "rules": [{"if_true": {"blocks": "minecraft:air", "offset": [0,-1,0], "type": "minecraft:matching_blocks"},
                       "then": {"id": "minecraft:sandstone"}}],
            "type": "minecraft:rule_based",
        });
        let provider = StateProvider::parse(&sand).unwrap();
        assert!(provider.supported());
        assert_eq!(provider.possible_states().len(), 2);
        // `rule_based` 需要世界和位置，无位置版本返回 `None`。
        // `rule_based` needs the world and position, so the world-free form returns `None`.
        assert!(provider.get_state(&random()).is_none());
    }
}
