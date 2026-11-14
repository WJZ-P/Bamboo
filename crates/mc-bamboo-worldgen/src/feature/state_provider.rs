//! `BlockStateProvider`：`DiskFeature` 等通过它取要放的方块状态。
//!
//! 官方 `BlockStateProvider.DIRECT_CODEC` 是 `Codec.xor(BlockState.FULL_CODEC, TYPED_CODEC)`：
//! 一个裸方块状态（`{"id":...}`）就是 `SimpleStateProvider`，带 `type` 的对象按类型分派。
//! `DiskFeature` 调 `getOptionalState`，`RuleBasedStateProvider` 会返回 `null` 表示不放。
//!
//! Block state providers. `DIRECT_CODEC` is `xor(BlockState.FULL_CODEC, TYPED_CODEC)`: a bare
//! block state is a `SimpleStateProvider`; a typed object dispatches by `type`. `DiskFeature`
//! calls `getOptionalState`, and `RuleBasedStateProvider` can return `null` (place nothing).

use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{predicates::Predicate, state::parse_state, world::FeatureWorld};

/// 已解析的方块状态提供者。
#[derive(Debug, Clone)]
pub enum StateProvider {
    /// `simple`：固定状态。
    Simple(u32),
    /// `rule_based`：按顺序试规则，都不中再用 fallback。
    RuleBased {
        fallback: Option<Box<StateProvider>>,
        rules: Vec<(Predicate, StateProvider)>,
    },
    /// 本轮未移植的类型。
    Unsupported(String),
}

impl StateProvider {
    pub fn parse(value: &Value) -> Result<Self, String> {
        // 裸方块状态没有 `type`；`SimpleStateProvider.CODEC` 之外由 DIRECT_CODEC 的 xor 包成 simple。
        if value.get("type").is_none() {
            return Ok(Self::Simple(parse_state(value)?));
        }
        let object = value
            .as_object()
            .ok_or("state provider must be an object")?;
        let kind = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or("state provider has no type")?;
        Ok(match kind {
            "minecraft:simple" => Self::Simple(parse_state(
                object.get("state").ok_or("simple provider has no state")?,
            )?),
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
                        let predicate = Predicate::parse(
                            object
                                .get("if_true")
                                .ok_or("rule_based rule has no if_true")?,
                        )?;
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
    pub fn supported(&self) -> bool {
        match self {
            Self::Simple(_) => true,
            Self::RuleBased { fallback, rules } => {
                fallback.as_deref().is_none_or(StateProvider::supported)
                    && rules
                        .iter()
                        .all(|(predicate, provider)| predicate.supported() && provider.supported())
            }
            Self::Unsupported(_) => false,
        }
    }

    /// `BlockStateProvider.getOptionalState(level, random, pos)`；`None` 表示不放。
    pub fn get_optional_state(
        &self,
        world: &FeatureWorld<'_>,
        random: &WorldgenRandom,
        pos: (i32, i32, i32),
    ) -> Option<u32> {
        let _ = random;
        match self {
            Self::Simple(state) => Some(*state),
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
}
