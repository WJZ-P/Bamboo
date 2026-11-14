//! `BlockStateProvider`：`SimpleBlockFeature.to_place` 的三种已移植形态。
//!
//! 26.3 的 `to_place` 是 `Holder<BlockStateProvider>`。内联 bare state 等价于
//! `SimpleStateProvider`（`getState` 不消费随机数）；typed provider 里 `weighted` 用
//! `WeightedList.getRandomOrThrow`，`randomized_int` 先采样 source 再用 `IntProvider.sample`
//! 改一个整数属性。holder 字符串引用的是未导出的 `BLOCK_STATE_PROVIDER` 注册表，暂不支持。
//! 其余 typed provider（noise / dual_noise / noise_threshold / rotated）也暂不支持。
//!
//! `BlockStateProvider` shapes ported for `SimpleBlockFeature.to_place`. An inline bare state
//! is `SimpleStateProvider` (no random consumed); `weighted` uses
//! `WeightedList.getRandomOrThrow`, `randomized_int` samples its source then overwrites an
//! integer property with `IntProvider.sample`. Holder references and other typed providers
//! (noise / dual_noise / noise_threshold / rotated) stay unsupported.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    provider::IntProvider,
    state::{parse_state, with_property},
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
}

impl StateProvider {
    /// 解析 `to_place`；未移植的形态返回 `None`。
    ///
    /// Parse `to_place`; return `None` for unported shapes.
    pub fn parse(value: &Value) -> Option<Self> {
        match value {
            // 字符串是 `Holder<BlockStateProvider>` 注册表引用（未导出），不支持。
            // A string is a holder reference to the unexported registry: unsupported.
            Value::String(_) => None,
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
        }
    }

    /// 官方 `BlockStateProvider.getState(level, random, pos)`，返回状态 ID。
    ///
    /// Official `BlockStateProvider.getState(level, random, pos)`; returns a state ID.
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
        let holder: Value = json!("minecraft:flower_plain");
        assert!(StateProvider::parse(&holder).is_none());
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
