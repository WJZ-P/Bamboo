//! `BlockStateProvider`：官方 `BlockStateProvider.DIRECT_CODEC` 是
//! `Codec.xor(BlockState.FULL_CODEC, TYPED_CODEC)`。裸方块状态（`{"id":...}`）是
//! `SimpleStateProvider`；带 `type` 的对象按类型分派：`simple`、`rule_based`（DiskFeature
//! 用 `getOptionalState`，规则都不中时回退）、`weighted`、`randomized_int`。
//!
//! `BlockStateProvider`: official `DIRECT_CODEC` is
//! `xor(BlockState.FULL_CODEC, TYPED_CODEC)`. A bare block state is a `SimpleStateProvider`;
//! typed objects dispatch to `simple`, `rule_based` (used by `DiskFeature.getOptionalState`),
//! `weighted` and `randomized_int`. Holder references and unported typed kinds return `None`.

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    block_predicate::BlockPredicate,
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
}

impl StateProvider {
    /// 解析 `to_place` / `state_provider`；未移植的形态返回 `None`。
    ///
    /// Parse `to_place` / `state_provider`; return `None` for unported shapes.
    pub fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let Some(kind) = object.get("type").and_then(Value::as_str) else {
            // 裸方块状态没有 `type`，由 DIRECT_CODEC 的 xor 包成 SimpleStateProvider。
            // A bare block state has no `type` and is wrapped into SimpleStateProvider.
            return Some(Self::Simple(parse_state(value).ok()?));
        };
        Some(match kind {
            "minecraft:simple" => Self::Simple(parse_state(object.get("state")?).ok()?),
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
                        let rule = rule.as_object()?;
                        let predicate = BlockPredicate::parse(rule.get("if_true")?);
                        let then = Self::parse(rule.get("then")?)?;
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
                source: Box::new(Self::parse(object.get("source")?)?),
                property: object.get("property")?.as_str()?.to_owned(),
                values: IntProvider::parse(object.get("values")?).ok()?,
            },
            // 注册表引用（字符串）和其余 typed provider 未移植。
            // Holder references and the other typed providers are unported.
            _ => return None,
        })
    }

    /// 是否全部子提供者 / 谓词都已移植。
    ///
    /// Whether every nested provider / predicate is ported.
    pub fn supported(&self) -> bool {
        match self {
            Self::Simple(_) | Self::Weighted { .. } | Self::RandomizedInt { .. } => true,
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

    /// 官方 `BlockStateProvider.getState(level, random, pos)`；`rule_based` 需要世界和
    /// 位置，这里返回 `None`（它由 DiskFeature 走 [`Self::get_optional_state`]）。
    ///
    /// Official `BlockStateProvider.getState(level, random, pos)`; `rule_based` needs the
    /// world and pos, so it returns `None` here (DiskFeature uses `get_optional_state`).
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
            Self::RuleBased { .. } => None,
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
            Self::Weighted { .. } | Self::RandomizedInt { .. } => self.get_state(random),
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
    }
}
