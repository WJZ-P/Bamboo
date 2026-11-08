//! 战利品池，对应官方的 `LootPool`。
//!
//! 一个池先掷 `rolls` 次，再加 `Mth.floor(bonus_rolls * luck)` 次；每次展开
//! 全部条目，只把权重为正的当成候选，按权重取一个。池的 `modifier` 包在输出
//! 消费者外面，所以它在条目自己的 modifier 之后、表的 modifier 之前生效。

use serde_json::Value;

use crate::condition::LootCondition;
use crate::context::LootContext;
use crate::entry::{Candidate, LootEntry};
use crate::function::{parse_modifier, LootFunction};
use crate::json::{array, field, object, required};
use crate::number::{parse_float_provider, parse_int_provider, FloatProvider, IntProvider};
use crate::stack::LootStack;
use crate::Result;

/// 一个战利品池。
#[derive(Debug)]
pub struct LootPool {
    entries: Vec<LootEntry>,
    condition: Option<LootCondition>,
    modifier: Option<LootFunction>,
    rolls: IntProvider,
    bonus_rolls: FloatProvider,
}

impl LootPool {
    /// 解析一个池。`bonus_rolls` 缺省时官方是常量 0。
    pub fn parse(value: &Value) -> Result<Self> {
        let map = object(value, "loot pool")?;
        let entries = array(required(map, "entries", "loot pool")?, "entries")?
            .iter()
            .map(LootEntry::parse)
            .collect::<Result<Vec<_>>>()?;
        let condition = field(map, "condition")
            .map(LootCondition::parse)
            .transpose()?;
        let modifier = parse_modifier(field(map, "modifier"))?;
        let rolls = parse_int_provider(required(map, "rolls", "loot pool")?, "rolls")?;
        let bonus_rolls = match field(map, "bonus_rolls") {
            Some(value) => parse_float_provider(value, "bonus_rolls")?,
            None => FloatProvider::Constant(0.0),
        };
        Ok(Self {
            entries,
            condition,
            modifier,
            rolls,
            bonus_rolls,
        })
    }

    /// `LootPool.addRandomItems`。`outer` 是外面一层的 `modifier`（表的），
    /// 官方把它包在最外层，所以这里在池自己的 modifier 之后应用。
    pub(crate) fn add_random_items(
        &self,
        context: &LootContext<'_, '_, '_>,
        outer: Option<&LootFunction>,
        output: &mut Vec<LootStack>,
    ) -> Result<()> {
        if let Some(condition) = &self.condition {
            if !condition.test(context)? {
                return Ok(());
            }
        }
        let bonus = self.bonus_rolls.get(context)? * context.luck();
        let count = self.rolls.get(context)? + bonus.floor() as i32;
        for _ in 0..count {
            self.add_random_item(context, outer, output)?;
        }
        Ok(())
    }

    /// `LootPool.addRandomItem`：展开全部条目，按权重挑一个。
    fn add_random_item(
        &self,
        context: &LootContext<'_, '_, '_>,
        outer: Option<&LootFunction>,
        output: &mut Vec<LootStack>,
    ) -> Result<()> {
        let mut candidates = Vec::new();
        for entry in &self.entries {
            entry.expand(context, 0, &mut Vec::new(), &mut candidates)?;
        }
        let valid: Vec<&Candidate<'_>> = candidates.iter().filter(|entry| entry.weight > 0).collect();
        let total = valid
            .iter()
            .fold(0_i32, |sum, entry| sum.wrapping_add(entry.weight));
        if total == 0 || valid.is_empty() {
            return Ok(());
        }
        let chosen = if valid.len() == 1 {
            valid[0]
        } else {
            let mut index = context.random().next_int(total);
            let mut chosen = None;
            for candidate in &valid {
                index -= candidate.weight;
                if index < 0 {
                    chosen = Some(*candidate);
                    break;
                }
            }
            let Some(chosen) = chosen else {
                return Ok(());
            };
            chosen
        };
        for mut stack in chosen.create(context, 0)? {
            if let Some(modifier) = &self.modifier {
                stack = modifier.apply(stack, context)?;
            }
            if let Some(modifier) = outer {
                stack = modifier.apply(stack, context)?;
            }
            output.push(stack);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::context::LootParams;
    use mc_bamboo_blocks::random::LegacyRandom;
    use mc_bamboo_data::items;

    fn two_item_pool() -> LootPool {
        LootPool::parse(&json!({
            "rolls": 1,
            "entries": [
                {"type": "minecraft:item", "name": "minecraft:stone", "weight": 1},
                {"type": "minecraft:item", "name": "minecraft:dirt", "weight": 3},
            ],
        }))
        .unwrap()
    }

    #[test]
    fn weighted_selection_draws_once() {
        let pool = two_item_pool();
        let params = LootParams::default();
        let random = LegacyRandom::new(0);
        let context = LootContext::new(&params, &random);
        let mut output = Vec::new();
        pool.add_random_items(&context, None, &mut output).unwrap();
        assert_eq!(output.len(), 1);

        // 和官方一样先取 [0, 4) 里的下标，再按顺序减权重。
        let twin = LegacyRandom::new(0);
        let index = twin.next_int(4);
        let expected = if index == 0 { items::STONE } else { items::DIRT };
        assert_eq!(output[0].item, expected);
        assert_eq!(random.seed(), twin.seed());
    }

    #[test]
    fn bonus_rolls_add_floor_of_luck() {
        let pool = LootPool::parse(&json!({
            "rolls": 1,
            "bonus_rolls": 2.0,
            "entries": [{"type": "minecraft:item", "name": "minecraft:stone"}],
        }))
        .unwrap();
        let params = LootParams::new(1.0);
        let random = LegacyRandom::new(7);
        let context = LootContext::new(&params, &random);
        let mut output = Vec::new();
        pool.add_random_items(&context, None, &mut output).unwrap();
        // 一个候选时官方直接创建，不取随机数。
        assert_eq!(output.len(), 3);
        assert!(output.iter().all(|stack| stack.item == items::STONE));
        assert_eq!(random.seed(), LegacyRandom::new(7).seed());
    }

    #[test]
    fn a_false_condition_skips_everything() {
        let pool = LootPool::parse(&json!({
            "rolls": 3,
            "condition": {"type": "minecraft:random_chance", "chance": 0.0},
            "entries": [{"type": "minecraft:item", "name": "minecraft:stone"}],
        }))
        .unwrap();
        let params = LootParams::default();
        let random = LegacyRandom::new(1);
        let context = LootContext::new(&params, &random);
        let mut output = Vec::new();
        pool.add_random_items(&context, None, &mut output).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn zero_weight_entries_are_not_candidates() {
        let pool = LootPool::parse(&json!({
            "rolls": 1,
            "entries": [{"type": "minecraft:item", "name": "minecraft:stone", "weight": 0}],
        }))
        .unwrap();
        let params = LootParams::default();
        let random = LegacyRandom::new(2);
        let context = LootContext::new(&params, &random);
        let mut output = Vec::new();
        pool.add_random_items(&context, None, &mut output).unwrap();
        assert!(output.is_empty());
        assert_eq!(random.seed(), LegacyRandom::new(2).seed());
    }
}
