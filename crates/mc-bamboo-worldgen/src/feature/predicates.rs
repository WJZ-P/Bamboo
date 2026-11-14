//! `BlockPredicate`：地物配置和 `block_predicate_filter` 修饰器里的方块谓词。
//!
//! 官方 `BlockPredicate.CODEC` 按 `type` 分派（`BlockPredicateType`），`test(LevelAccessor, BlockPos)`
//! 只有两类：`StateTestingPredicate` 先按 `offset` 取那一格的 `BlockState`，其余直接判定原位。
//! 这里只实现本轮要用的几种；没移植的记成 `Unsupported`，让主循环把这块标成跳过。
//!
//! Block predicates from feature configs and the `block_predicate_filter` modifier.
//! The official `BlockPredicate.CODEC` dispatches on `type`; `StateTestingPredicate` offsets
//! the sampled position before testing its block state. Only the types needed this round are
//! implemented; unported ones become `Unsupported` so the main loop skips the chunk.

use mc_bamboo_data::{
    block_by_name, block_id_of_state, block_tag, facts, registries, state_facts, state_fluid,
};
use serde_json::Value;

use super::world::FeatureWorld;

/// 一个候选方块位置。
pub type Pos = (i32, i32, i32);

/// 已解析的方块谓词。
#[derive(Debug, Clone)]
pub enum Predicate {
    /// `matching_blocks`：`state.is(HolderSet<Block>)`，值是方块注册表 ID。
    MatchingBlocks {
        offset: [i32; 3],
        blocks: Vec<usize>,
    },
    /// `matching_block_tag`：`state.is(TagKey<Block>)`。
    MatchingBlockTag { offset: [i32; 3], ids: Vec<i32> },
    /// `matching_fluids`：`state.getFluidState().is(...)`，值是流体注册表 ID。
    MatchingFluids { offset: [i32; 3], fluids: Vec<i32> },
    /// `solid`：`state.isSolid()`。
    Solid { offset: [i32; 3] },
    /// `replaceable`：`state.canBeReplaced()`。
    Replaceable { offset: [i32; 3] },
    /// `true`：恒真。
    True,
    /// `all_of`：全部为真。
    AllOf(Vec<Predicate>),
    /// `any_of`：任一为真。
    AnyOf(Vec<Predicate>),
    /// `not`：取反。
    Not(Box<Predicate>),
    /// 本轮未移植的类型。
    Unsupported(String),
}

/// 解析 `offset`，缺省 `[0,0,0]`（`StateTestingPredicate` 的 `Vec3i.ZERO`）。
fn parse_offset(object: &serde_json::Map<String, Value>) -> Result<[i32; 3], String> {
    let Some(value) = object.get("offset") else {
        return Ok([0, 0, 0]);
    };
    let array = value.as_array().ok_or("predicate offset is not an array")?;
    if array.len() != 3 {
        return Err("predicate offset must have 3 entries".to_owned());
    }
    let mut result = [0; 3];
    for (index, entry) in array.iter().enumerate() {
        result[index] = entry
            .as_i64()
            .ok_or("predicate offset entry is not an int")? as i32;
    }
    Ok(result)
}

/// `blocks` / `fluids` 字段：单个字符串、字符串数组，或 `#minecraft:...` 标签。
fn string_set(value: &Value) -> Result<Vec<String>, String> {
    match value {
        Value::String(text) => Ok(vec![text.clone()]),
        Value::Array(entries) => entries
            .iter()
            .map(|entry| {
                entry
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "holder set entry is not a string".to_owned())
            })
            .collect(),
        _ => Err("holder set is neither a string nor an array".to_owned()),
    }
}

/// 方块注册名 → 方块注册表 ID（`HolderSet<Block>` 的元素是方块，不是状态）。
fn block_id_of_name(name: &str) -> Result<usize, String> {
    block_by_name(name)
        .and_then(|block| block_id_of_state(block.first_state))
        .ok_or_else(|| format!("unknown block {name}"))
}

impl Predicate {
    pub fn parse(value: &Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or("block predicate must be an object")?;
        let kind = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or("block predicate has no type")?;
        Ok(match kind {
            "minecraft:matching_blocks" => {
                let names = string_set(
                    object
                        .get("blocks")
                        .ok_or("matching_blocks has no blocks")?,
                )?;
                let mut blocks = Vec::with_capacity(names.len());
                for name in names {
                    if let Some(tag) = name.strip_prefix('#') {
                        let ids =
                            block_tag(tag).ok_or_else(|| format!("unknown block tag {tag}"))?;
                        blocks.extend(ids.iter().map(|&id| id as usize));
                    } else {
                        blocks.push(block_id_of_name(&name)?);
                    }
                }
                Self::MatchingBlocks {
                    offset: parse_offset(object)?,
                    blocks,
                }
            }
            "minecraft:matching_block_tag" => {
                let tag = object
                    .get("tag")
                    .and_then(Value::as_str)
                    .ok_or("matching_block_tag has no tag")?
                    .trim_start_matches('#');
                let ids = block_tag(tag)
                    .ok_or_else(|| format!("unknown block tag {tag}"))?
                    .to_vec();
                Self::MatchingBlockTag {
                    offset: parse_offset(object)?,
                    ids,
                }
            }
            "minecraft:matching_fluids" => {
                let names = string_set(
                    object
                        .get("fluids")
                        .ok_or("matching_fluids has no fluids")?,
                )?;
                let mut fluids = Vec::with_capacity(names.len());
                for name in names {
                    fluids.push(
                        registries::fluid::by_name(name.trim_start_matches('#'))
                            .ok_or_else(|| format!("unknown fluid {name}"))?,
                    );
                }
                Self::MatchingFluids {
                    offset: parse_offset(object)?,
                    fluids,
                }
            }
            "minecraft:solid" => Self::Solid {
                offset: parse_offset(object)?,
            },
            "minecraft:replaceable" => Self::Replaceable {
                offset: parse_offset(object)?,
            },
            "minecraft:true" => Self::True,
            "minecraft:all_of" => Self::AllOf(parse_list(
                object.get("predicates").ok_or("all_of has no predicates")?,
            )?),
            "minecraft:any_of" => Self::AnyOf(parse_list(
                object.get("predicates").ok_or("any_of has no predicates")?,
            )?),
            "minecraft:not" => Self::Not(Box::new(Self::parse(
                object.get("predicate").ok_or("not has no predicate")?,
            )?)),
            other => Self::Unsupported(other.to_owned()),
        })
    }

    /// 是否全部子谓词都已移植（决定主循环要不要标 `unsupported`）。
    pub fn supported(&self) -> bool {
        match self {
            Self::Unsupported(_) => false,
            Self::AllOf(list) | Self::AnyOf(list) => list.iter().all(Self::supported),
            Self::Not(inner) => inner.supported(),
            _ => true,
        }
    }

    /// `BlockPredicate.test(level, origin)`。
    pub fn test(&self, world: &FeatureWorld<'_>, origin: Pos) -> bool {
        match self {
            Self::MatchingBlocks { offset, blocks } => {
                let state = state_at(world, origin, *offset);
                block_id_of_state(state).is_some_and(|id| blocks.contains(&id))
            }
            Self::MatchingBlockTag { offset, ids } => {
                let state = state_at(world, origin, *offset);
                block_id_of_state(state).is_some_and(|id| ids.contains(&(id as i32)))
            }
            Self::MatchingFluids { offset, fluids } => {
                let state = state_at(world, origin, *offset);
                fluids.contains(&state_fluid(state).fluid)
            }
            Self::Solid { offset } => {
                state_facts(state_at(world, origin, *offset)) & facts::SOLID != 0
            }
            Self::Replaceable { offset } => {
                state_facts(state_at(world, origin, *offset)) & facts::REPLACEABLE != 0
            }
            Self::True => true,
            Self::AllOf(list) => list.iter().all(|p| p.test(world, origin)),
            Self::AnyOf(list) => list.iter().any(|p| p.test(world, origin)),
            Self::Not(inner) => !inner.test(world, origin),
            Self::Unsupported(_) => false,
        }
    }
}

fn parse_list(value: &Value) -> Result<Vec<Predicate>, String> {
    value
        .as_array()
        .ok_or("combining predicate has no predicates array")?
        .iter()
        .map(Predicate::parse)
        .collect()
}

/// `level.getBlockState(origin.offset(offset))`；范围外按空气。
fn state_at(world: &FeatureWorld<'_>, origin: Pos, offset: [i32; 3]) -> u32 {
    world.get_block(
        origin.0 + offset[0],
        origin.1 + offset[1],
        origin.2 + offset[2],
    )
}
