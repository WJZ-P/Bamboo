//! `BlockStateProvider`：地物取要放方块状态的统一入口。
//!
//! 官方 `BlockStateProvider.DIRECT_CODEC` 是 `Codec.xor(BlockState.FULL_CODEC, TYPED_CODEC)`：
//! 一个裸方块状态（`{"id":...}`）就是 `SimpleStateProvider`，带 `type` 的对象按类型分派。
//! 26.3 的 `SimpleBlockFeature.to_place` 是 `Holder<BlockStateProvider>`：内联 bare state
//! 等价于 `SimpleStateProvider`（`getState` 不消费随机数），holder 字符串引用的是未导出的
//! `BLOCK_STATE_PROVIDER` 注册表，暂不支持。`DiskFeature` 等调 `getOptionalState`，
//! `RuleBasedStateProvider` 会返回 `null` 表示不放；`weighted` 用
//! `WeightedList.getRandomOrThrow`，`randomized_int` 先采样 source 再用 `IntProvider.sample`
//! 改一个整数属性。noise / dual_noise / noise_threshold / rotated 暂不支持。
//!
//! Unified entry point for the block states a feature places. `DIRECT_CODEC` is
//! `xor(BlockState.FULL_CODEC, TYPED_CODEC)`: a bare state is a `SimpleStateProvider`, a typed
//! object dispatches by `type`. `SimpleBlockFeature.to_place` is a `Holder<BlockStateProvider>`;
//! an inline bare state equals `SimpleStateProvider` (no random consumed), holder strings
//! reference the unexported `BLOCK_STATE_PROVIDER` registry (unsupported). `DiskFeature` and
//! friends call `getOptionalState`, where `RuleBasedStateProvider` may return `null`;
//! `weighted` uses `WeightedList.getRandomOrThrow` and `randomized_int` samples its source
//! before overwriting an int property with `IntProvider.sample`. The remaining typed
//! providers (noise / dual_noise / noise_threshold / rotated) stay unsupported.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    block_predicate::BlockPredicate,
    provider::IntProvider,
    state::{parse_state, with_property},
    world::FeatureWorld,
};

/// 已解析的方块状态提供者。
#[derive(Debug, Clone)]
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
    /// `rule_based`：按顺序试规则，都不中再用 fallback。
    RuleBased {
        fallback: Option<Box<StateProvider>>,
        rules: Vec<(BlockPredicate, StateProvider)>,
    },
    /// 本轮未移植的类型。
    Unsupported(String),
}

impl StateProvider {
    /// 解析 `to_place` / `state_provider`；未移植的形态解析成 [`Self::Unsupported`]。
    ///
    /// Parse `to_place` / `state_provider`; unported shapes become [`Self::Unsupported`].
    pub fn parse(value: &Value) -> Result<Self, String> {
        let Some(object) = value.as_object() else {
            // 字符串是 `Holder<BlockStateProvider>` 注册表引用（未导出），不支持。
            // A string is a holder reference to the unexported registry: unsupported.
            return Ok(Self::Unsupported("holder reference".to_owned()));
        };
        let Some(kind) = object.get("type").and_then(Value::as_str) else {
            // 裸方块状态没有 `type`：`DIRECT_CODEC` 的 xor 把它包成 simple。
            // A bare block state has no `type`; the xor codec wraps it as simple.
            return Ok(Self::Simple(parse_state(value)?));
        };
        Ok(match kind {
            "minecraft:simple" => Self::Simple(parse_state(
                object.get("state").ok_or("simple provider has no state")?,
            )?),
            "minecraft:weighted" => {
                let entries = object
                    .get("entries")
                    .and_then(Value::as_array)
                    .ok_or("weighted provider has no entries")?
                    .iter()
                    .map(|entry| {
                        let weight = entry
                            .get("weight")
                            .and_then(Value::as_i64)
                            .ok_or("weighted entry has no weight")?
                            as i32;
                        let state =
                            parse_state(entry.get("data").ok_or("weighted entry has no data")?)?;
                        Ok((weight, state))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let total: i32 = entries.iter().map(|(weight, _)| *weight).sum();
                Self::Weighted { entries, total }
            }
            "minecraft:randomized_int" => Self::RandomizedInt {
                source: Box::new(Self::parse(
                    object.get("source").ok_or("randomized_int has no source")?,
                )?),
                property: object
                    .get("property")
                    .and_then(Value::as_str)
                    .ok_or("randomized_int has no property")?
                    .to_owned(),
                values: IntProvider::parse(
                    object.get("values").ok_or("randomized_int has no values")?,
                )?,
            },
            "minecraft:rule_based" => {
                let fallback = object
                    .get("fallback")
                    .map(|value| Self::parse(value).map(Box::new))
                    .transpose()?;
                let rules = object
                    .get("rules")
                    .and_then(Value::as_array)
                    .ok_or("rule_based has no rules")?
                    .iter()
                    .map(|rule| {
                        let object = rule.as_object().ok_or("rule_based rule is not an object")?;
                        let predicate = BlockPredicate::parse(
                            object
                                .get("if_true")
                                .ok_or("rule_based rule has no if_true")?,
                        );
                        let then =
                            Self::parse(object.get("then").ok_or("rule_based rule has no then")?)?;
                        Ok((predicate, then))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Self::RuleBased { fallback, rules }
            }
            other => Self::Unsupported(other.to_owned()),
        })
    }

    /// 是否全部子提供者/谓词都已移植。
    ///
    /// Whether every nested provider and predicate is ported.
    pub fn supported(&self) -> bool {
        match self {
            Self::Simple(_) => true,
            Self::Weighted { .. } => true,
            Self::RandomizedInt { source, .. } => source.supported(),
            Self::RuleBased { fallback, rules } => {
                fallback.as_deref().is_none_or(StateProvider::supported)
                    && rules.iter().all(|(predicate, provider)| {
                        predicate.is_supported() && provider.supported()
                    })
            }
            Self::Unsupported(_) => false,
        }
    }

    /// 所有可能放出的状态（用于检查存活判定是否都已移植）；属性值变化不改变方块类。
    ///
    /// Every state this provider may produce (for the survival-check gate); changing an
    /// integer property does not change the block class.
    pub fn possible_states(&self) -> Vec<u32> {
        match self {
            Self::Simple(state) => vec![*state],
            Self::Weighted { entries, .. } => entries.iter().map(|(_, state)| *state).collect(),
            Self::RandomizedInt { source, .. } => source.possible_states(),
            Self::RuleBased { fallback, rules } => {
                let mut states: Vec<u32> = rules
                    .iter()
                    .flat_map(|(_, provider)| provider.possible_states())
                    .collect();
                if let Some(fallback) = fallback {
                    states.extend(fallback.possible_states());
                }
                states
            }
            Self::Unsupported(_) => Vec::new(),
        }
    }

    /// `BlockStateProvider.getOptionalState(level, random, pos)`；`None` 表示不放。
    ///
    /// Official `BlockStateProvider.getOptionalState(level, random, pos)`; `None` means
    /// nothing is placed.
    pub fn get_optional_state(
        &self,
        world: &FeatureWorld<'_>,
        random: &WorldgenRandom,
        pos: (i32, i32, i32),
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
            // 按顺序试规则，第一个谓词为真的分支自己决定放什么（也可能返回 None）。
            // Try the rules in order; the first matching branch decides, possibly `None`.
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
            Self::Unsupported(_) => None,
        }
    }

    /// 官方 `BlockStateProvider.getState(level, random, pos)`；未移植的形态返回 `None`。
    ///
    /// Official `BlockStateProvider.getState(level, random, pos)`; `None` when unported.
    pub fn get_state(
        &self,
        world: &FeatureWorld<'_>,
        random: &WorldgenRandom,
        pos: (i32, i32, i32),
    ) -> Option<u32> {
        self.get_optional_state(world, random, pos)
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
            Ok(StateProvider::Simple(_))
        ));
        let holder: Value = json!("minecraft:flower_plain");
        assert!(!StateProvider::parse(&holder).unwrap().supported());
        let noise: Value = json!({"type": "minecraft:noise", "seed": 1});
        assert!(!StateProvider::parse(&noise).unwrap().supported());
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
        assert!(provider.supported());
        assert_eq!(provider.possible_states().len(), 2);
        let randomized: Value = json!({
            "property": "pickles",
            "source": {"id": "minecraft:sea_pickle", "properties": {"pickles": "1", "waterlogged": "true"}},
            "type": "minecraft:randomized_int",
            "values": {"min_inclusive": 1, "max_inclusive": 4, "type": "minecraft:uniform"},
        });
        assert!(matches!(
            StateProvider::parse(&randomized),
            Ok(StateProvider::RandomizedInt { .. })
        ));
    }

    #[test]
    fn parses_rule_based_with_nested_predicate() {
        let rule_based: Value = json!({
            "type": "minecraft:rule_based",
            "rules": [
                {
                    "if_true": {"type": "minecraft:matching_block_tag", "tag": "minecraft:air"},
                    "then": {"id": "minecraft:magma_block"},
                },
            ],
            "fallback": {"id": "minecraft:obsidian"},
        });
        let provider = StateProvider::parse(&rule_based).unwrap();
        assert!(provider.supported());
        assert_eq!(provider.possible_states().len(), 2);
    }
}
