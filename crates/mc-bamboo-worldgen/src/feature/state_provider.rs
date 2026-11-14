//! `BlockStateProvider`：`SimpleBlockFeature.to_place` 的已移植形态。
//!
//! 内联 bare state 等价于 `SimpleStateProvider`（不消费随机数）；typed provider 里
//! `weighted` 用 `WeightedList.getRandomOrThrow`、`randomized_int` 用 `IntProvider.sample`
//! 改整数属性、`noise` / `dual_noise` / `noise_threshold` 用 `NormalNoise` 按坐标选、
//! `rule_based` 按 `BlockPredicate` 顺序试规则、都没中时用 fallback。
//! 字符串是 `BLOCK_STATE_PROVIDER` 注册表引用（生成器已导出），解析时展开成对应配置。
//! `rotated` 暂不支持。
//!
//! Ported `BlockStateProvider` shapes for `SimpleBlockFeature.to_place`: inline bare state,
//! weighted / randomized_int / noise / dual_noise / noise_threshold / rule_based typed
//! providers, and holder references into the exported `BLOCK_STATE_PROVIDER` registry.
//! `rotated` stays unsupported.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    block_predicate::BlockPredicate,
    noise_state::{NoiseStateProvider, Pos, holder_registry},
    provider::IntProvider,
    state::{parse_state, with_property},
    world::FeatureWorld,
};

/// 一个已解析的 `BlockStateProvider`。
pub enum StateProvider {
    /// `SimpleStateProvider`：固定状态，不消费随机数。
    Simple(u32),
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
    /// `NoiseProvider` / `DualNoiseProvider` / `NoiseThresholdProvider`（按坐标用噪声）。
    Noise(Box<NoiseStateProvider>),
    /// `RuleBasedStateProvider`：按 `BlockPredicate` 顺序试，第一个命中且给出状态的就用它。
    RuleBased {
        fallback: Option<Box<StateProvider>>,
        rules: Vec<(BlockPredicate, StateProvider)>,
    },
}

impl StateProvider {
    /// 解析 `to_place`；未移植的形态返回 `None`。
    ///
    /// Parse `to_place`; return `None` for unported shapes.
    pub fn parse(value: &Value) -> Option<Self> {
        match value {
            // 字符串展开成 `BLOCK_STATE_PROVIDER` 注册表里对应的配置。
            // A string expands to its `BLOCK_STATE_PROVIDER` registry entry.
            Value::String(name) => Self::parse(holder_registry().get(name.as_str())?),
            Value::Object(object) => {
                let Some(kind) = object.get("type").and_then(Value::as_str) else {
                    return Some(Self::Simple(parse_state(value).ok()?));
                };
                match kind {
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
                        Some(Self::Weighted { entries, total })
                    }
                    "minecraft:randomized_int" => Some(Self::RandomizedInt {
                        source: Box::new(Self::parse(object.get("source")?)?),
                        property: object.get("property")?.as_str()?.to_owned(),
                        values: IntProvider::parse(object.get("values")?).ok()?,
                    }),
                    "minecraft:noise" | "minecraft:dual_noise" | "minecraft:noise_threshold" => {
                        Some(Self::Noise(Box::new(NoiseStateProvider::parse(value)?)))
                    }
                    "minecraft:rule_based" => {
                        let fallback = match object.get("fallback") {
                            Some(value) => Some(Box::new(Self::parse(value)?)),
                            None => None,
                        };
                        let rules = object
                            .get("rules")?
                            .as_array()?
                            .iter()
                            .map(|rule| {
                                let predicate = BlockPredicate::parse(rule.get("if_true")?);
                                let provider = Self::parse(rule.get("then")?)?;
                                Some((predicate, provider))
                            })
                            .collect::<Option<Vec<_>>>()?;
                        Some(Self::RuleBased { fallback, rules })
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// 所有可能放出的状态（用于检查存活判定是否都已移植）；属性值变化不改变方块类。
    ///
    /// Every state this provider may produce (for the survival-check gate); integer
    /// property values do not change the block class.
    pub fn possible_states(&self) -> Vec<u32> {
        match self {
            Self::Simple(state) => vec![*state],
            Self::Weighted { entries, .. } => entries.iter().map(|(_, state)| *state).collect(),
            Self::RandomizedInt { source, .. } => source.possible_states(),
            Self::Noise(provider) => provider.possible_states(),
            // fallback 缺失时官方可能返回 null，`SimpleBlockFeature.place` 直接放弃，
            // 所以只有 fallback 存在时它才是可能状态。
            // A missing fallback makes the official provider return null (the feature gives
            // up), so only a present fallback contributes possible states.
            Self::RuleBased { fallback, rules } => {
                let mut states = Vec::new();
                for (_, provider) in rules {
                    states.extend(provider.possible_states());
                }
                if let Some(fallback) = fallback {
                    states.extend(fallback.possible_states());
                }
                states
            }
        }
    }

    /// 谓词与存活判定是否都已移植；`SimpleBlockFeature.parse` 据此 gate。
    ///
    /// Whether the predicates and survival checks are ported; `SimpleBlockFeature.parse`
    /// gates on this.
    pub fn is_supported(&self) -> bool {
        match self {
            Self::RuleBased { fallback, rules } => {
                rules.iter().all(|(predicate, provider)| {
                    predicate.is_supported() && provider.is_supported()
                }) && fallback
                    .as_ref()
                    .is_none_or(|provider| provider.is_supported())
            }
            Self::RandomizedInt { source, .. } => source.is_supported(),
            _ => true,
        }
    }

    /// 官方 `BlockStateProvider.getOptionalState(level, random, pos)`，返回状态 ID。
    ///
    /// Official `BlockStateProvider.getOptionalState(level, random, pos)`; returns a state ID.
    pub fn get_optional_state(
        &self,
        world: &FeatureWorld<'_>,
        random: &WorldgenRandom,
        pos: Pos,
    ) -> Option<u32> {
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
                let base = source.get_optional_state(world, random, pos)?;
                let value = values.sample(random);
                with_property(base, property, &value.to_string())
            }
            Self::Noise(provider) => provider.get_state(random, pos),
            // 官方 `RuleBasedStateProvider.getOptionalState`：按顺序试规则，第一个谓词为真且
            // 子 provider 给出状态的就返回；都不行时用 fallback（可能为空）。
            // Official `RuleBasedStateProvider.getOptionalState`: try rules in order, return the
            // first whose predicate holds and whose provider yields a state, else the fallback.
            Self::RuleBased { fallback, rules } => {
                for (predicate, provider) in rules {
                    if predicate.test(world, pos)
                        && let Some(state) = provider.get_optional_state(world, random, pos)
                    {
                        return Some(state);
                    }
                }
                fallback
                    .as_ref()
                    .and_then(|provider| provider.get_optional_state(world, random, pos))
            }
        }
    }

    /// 官方 `BlockStateProvider.getState(level, random, pos)`：`getOptionalState` 为 None 时
    /// 只有 `RuleBasedStateProvider` 会退化成当前位置的方块，其余 provider 的 `getState`
    /// 从不为 null。
    ///
    /// Official `BlockStateProvider.getState`: only `RuleBasedStateProvider` degrades to the
    /// current block when `getOptionalState` is null; the other providers never return null.
    pub fn get_state(
        &self,
        world: &FeatureWorld<'_>,
        random: &WorldgenRandom,
        pos: Pos,
    ) -> Option<u32> {
        match self.get_optional_state(world, random, pos) {
            Some(state) => Some(state),
            None => match self {
                Self::RuleBased { .. } => Some(world.get_block(pos.0, pos.1, pos.2)),
                _ => None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_inline_bare_state_and_gates_holder_reference() {
        let bare: Value = json!({"id": "minecraft:poppy"});
        assert!(matches!(
            StateProvider::parse(&bare),
            Some(StateProvider::Simple(_))
        ));
        // flower holder 展开成 noise_threshold provider，现在支持。
        // The flower holder expands to a noise_threshold provider; now supported.
        let holder: Value = json!("minecraft:flower_plain");
        assert!(matches!(
            StateProvider::parse(&holder),
            Some(StateProvider::Noise(_))
        ));
        // rule_based holder 现在也支持。
        // The rule_based holder is now supported as well.
        let rules: Value = json!("minecraft:soil_beneath_tree");
        assert!(matches!(
            StateProvider::parse(&rules),
            Some(StateProvider::RuleBased { .. })
        ));
        // 缺字段的 noise provider 仍不支持。
        // An incomplete noise provider stays unsupported.
        let noise: Value = json!({"type": "minecraft:noise", "seed": 1});
        assert!(StateProvider::parse(&noise).is_none());
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
}
