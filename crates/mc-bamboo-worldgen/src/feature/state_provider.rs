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
    /// `RandomBlockProvider`：从方块集合里等概率取一个，用它的默认状态。
    ///
    /// `RandomBlockProvider`: pick one block uniformly and use its default state.
    RandomBlock(Vec<u32>),
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
        match value {
            Value::String(name) => Self::parse_with(lookup(name)?, lookup),
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
                        source: Box::new(Self::parse_with(object.get("source")?, lookup)?),
                        property: object.get("property")?.as_str()?.to_owned(),
                        values: IntProvider::parse(object.get("values")?).ok()?,
                    }),
                    // `RandomBlockProvider`：`blocks` 是 `HolderSet<Block>`，按默认状态取。
                    // `RandomBlockProvider`: `blocks` is a `HolderSet<Block>`; use defaults.
                    "minecraft:random_block" => Some(Self::RandomBlock(parse_block_states(
                        object.get("blocks")?,
                    )?)),
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
            Self::RandomBlock(states) => states.clone(),
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
            // `HolderSet.getRandomElement(random)`：`nextInt(size)` 等概率取一个。
            // `HolderSet.getRandomElement(random)`: one `nextInt(size)` uniform pick.
            Self::RandomBlock(states) => {
                if states.is_empty() {
                    return None;
                }
                let index = random.next_int(states.len() as i32) as usize;
                states.get(index).copied()
            }
        }
    }
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
}
