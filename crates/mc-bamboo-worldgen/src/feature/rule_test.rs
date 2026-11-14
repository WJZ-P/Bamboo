//! `RuleTest`：`OreFeature` 的 `targets[].target`。
//!
//! `height_match` 的 `min_inclusive` / `max_inclusive` 就是字面上的世界 y：
//! `HeightMatchTest.test` 字节码直接拿 `pos.getY()` 和这两个 int 比较，
//! 没有相对 `minY` 的变换。数据包里故意写了 `0..2031` 和 `-2032..8`
//! 这样的大范围来区分 tuff 的上/下矿石变体。

use mc_bamboo_data::{block_id_of_state, block_tag, facts, state_facts};
use serde_json::Value;

use crate::random::WorldgenRandom;

/// `RuleTest.test(state, pos, random)`。
#[derive(Debug, Clone)]
pub enum RuleTest {
    AlwaysTrue,
    Tag(String),
    Block(String),
    BlockState(u32),
    Height { min: i32, max: i32 },
    RandomBlock { block: String, probability: f32 },
    AllOf(Vec<RuleTest>),
    AnyOf(Vec<RuleTest>),
    Not(Box<RuleTest>),
}

impl RuleTest {
    pub fn parse(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("rule test must be an object")?;
        let kind = object
            .get("predicate_type")
            .and_then(Value::as_str)
            .ok_or("rule test has no predicate_type")?;
        let nested = |name: &str| -> Result<Vec<RuleTest>, String> {
            object
                .get(name)
                .and_then(Value::as_array)
                .ok_or_else(|| format!("rule test {kind} has no {name}"))?
                .iter()
                .map(RuleTest::parse)
                .collect()
        };
        let string = |name: &str| -> Result<String, String> {
            object
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("rule test {kind} has no {name}"))
        };
        let integer = |name: &str| -> Result<i32, String> {
            object
                .get(name)
                .and_then(Value::as_i64)
                .map(|n| n as i32)
                .ok_or_else(|| format!("rule test {kind} has no {name}"))
        };
        match kind {
            "minecraft:always_true" => Ok(Self::AlwaysTrue),
            "minecraft:tag_match" => Ok(Self::Tag(string("tag")?)),
            "minecraft:block_match" => Ok(Self::Block(string("block")?)),
            "minecraft:blockstate_match" => Ok(Self::BlockState(super::state::parse_state(
                object
                    .get("block_state")
                    .ok_or("blockstate_match has no state")?,
            )?)),
            "minecraft:height_match" => Ok(Self::Height {
                min: integer("min_inclusive")?,
                max: integer("max_inclusive")?,
            }),
            "minecraft:random_block_match" => Ok(Self::RandomBlock {
                block: string("block")?,
                probability: object
                    .get("probability")
                    .and_then(Value::as_f64)
                    .map(|p| p as f32)
                    .ok_or("random_block_match has no probability")?,
            }),
            "minecraft:all_of" => Ok(Self::AllOf(nested("rules")?)),
            "minecraft:any_of" => Ok(Self::AnyOf(nested("rules")?)),
            "minecraft:not" => Ok(Self::Not(Box::new(RuleTest::parse(
                object.get("rule").ok_or("not has no rule")?,
            )?))),
            _ => Err(format!("unsupported rule test {kind}")),
        }
    }

    pub fn test(&self, state: u32, x: i32, y: i32, z: i32, random: &WorldgenRandom) -> bool {
        let _ = (x, z);
        match self {
            Self::AlwaysTrue => true,
            Self::Tag(tag) => {
                let Some(id) = block_id_of_state(state) else {
                    return false;
                };
                block_tag(tag).is_some_and(|ids| ids.contains(&(id as i32)))
            }
            Self::Block(name) => mc_bamboo_data::block_by_name(name)
                .is_some_and(|block| block_id_of_state(state) == Some(block_index(block.name))),
            Self::BlockState(expected) => state == *expected,
            Self::Height { min, max } => (*min..=*max).contains(&y),
            Self::RandomBlock { block, probability } => {
                mc_bamboo_data::block_by_name(block)
                    .is_some_and(|b| block_id_of_state(state) == Some(block_index(b.name)))
                    && random.next_float() < *probability
            }
            Self::AllOf(rules) => rules.iter().all(|r| r.test(state, x, y, z, random)),
            Self::AnyOf(rules) => rules.iter().any(|r| r.test(state, x, y, z, random)),
            Self::Not(rule) => !rule.test(state, x, y, z, random),
        }
    }

    /// 不使用随机数的类型（`OreFeature.canPlaceOre` 只在 target 命中后才取数）。
    pub fn uses_random(&self) -> bool {
        match self {
            Self::RandomBlock { .. } => true,
            Self::AllOf(rules) | Self::AnyOf(rules) => rules.iter().any(Self::uses_random),
            Self::Not(rule) => rule.uses_random(),
            _ => false,
        }
    }
}

fn block_index(name: &str) -> usize {
    mc_bamboo_data::blocks::BLOCKS
        .iter()
        .position(|b| b.name == name)
        .unwrap_or(usize::MAX)
}

/// `Heightmap.NOT_AIR` 之类的判定，给后续 feature 用。
pub fn is_air(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::Algorithm;
    use mc_bamboo_data::blocks;

    #[test]
    fn parses_the_ore_targets() {
        let value: Value = serde_json::from_str(
            r#"{"predicate_type":"minecraft:any_of","rules":[
                {"predicate_type":"minecraft:all_of","rules":[
                  {"predicate_type":"minecraft:tag_match","tag":"minecraft:height_specific_ore_replaceables"},
                  {"min_inclusive":0,"max_inclusive":2031,"predicate_type":"minecraft:height_match"}]},
                {"predicate_type":"minecraft:all_of","rules":[
                  {"predicate_type":"minecraft:not","rule":{"predicate_type":"minecraft:tag_match","tag":"minecraft:height_specific_ore_replaceables"}},
                  {"predicate_type":"minecraft:tag_match","tag":"minecraft:stone_ore_replaceables"}]}]}"#,
        )
        .unwrap();
        let rule = RuleTest::parse(&value).unwrap();
        assert!(!rule.uses_random());
        let random = WorldgenRandom::new(Algorithm::Xoroshiro.new_instance(1));
        // tuff 在 height_specific 标签里，两个 height_match 覆盖 0..2031 和 -2032..8。
        let tuff = mc_bamboo_data::block_by_name("minecraft:tuff")
            .unwrap()
            .default_state;
        assert!(rule.test(tuff, 0, 0, 0, &random));
        assert!(!rule.test(tuff, 0, 2032, 0, &random));
        // 第二个 height_match 是 -2032..8，但也要 tuff 在标签里；这里 tuff 是。
        assert!(rule.test(tuff, 0, 8, 0, &random));
        assert!(!rule.test(tuff, 0, -2033, 0, &random));
        // stone 不在 height_specific 标签里，走 stone_ore_replaceables 分支，与高度无关。
        assert!(rule.test(blocks::STONE, 0, 100, 0, &random));
        assert!(is_air(blocks::AIR));
        assert!(!is_air(blocks::STONE));
    }
}
