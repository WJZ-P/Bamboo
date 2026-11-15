//! `BlockPredicate`（`BlockPredicateFilter` 用）的解析与求值。
//!
//! `StateTestingPredicate.test(level, origin)` 读 `origin + offset` 那一格；组合谓词
//! （all_of / any_of / not）把 origin 原样传给子谓词，由子谓词自己加 offset。
//! 未移植的谓词类型解析成 `Unsupported`，使整个 placed feature 标 unsupported 跳过。
//!
//! `StateTestingPredicate.test(level, origin)` reads the block at `origin + offset`;
//! combining predicates pass `origin` through and let children apply their own offset.
//! Unported predicate kinds become `Unsupported` so the placed feature is skipped.

use serde_json::Value;

use mc_bamboo_data::{
    block_by_name, block_id_of_state, block_tag, facts, registries, state_facts, state_fluid, tags,
};

use super::{state::parse_state, survival, world::FeatureWorld};

type Pos = (i32, i32, i32);

/// 一个方块谓词。
#[derive(Debug, Clone)]
pub enum BlockPredicate {
    MatchingBlockTag {
        offset: [i32; 3],
        tag: String,
    },
    MatchingBlocks {
        offset: [i32; 3],
        blocks: Vec<i32>,
    },
    AllOf(Vec<BlockPredicate>),
    AnyOf(Vec<BlockPredicate>),
    Not(Box<BlockPredicate>),
    Replaceable {
        offset: [i32; 3],
    },
    Solid {
        offset: [i32; 3],
    },
    True,
    /// `InsideWorldBoundsPredicate`：`origin + offset` 的 y 在建造高度内。
    ///
    /// `InsideWorldBoundsPredicate`: the y of `origin + offset` is inside the build height.
    InsideWorldBounds {
        offset: [i32; 3],
    },
    /// `WouldSurvivePredicate`：`state.canSurvive(level, origin+offset)`。
    WouldSurvive {
        offset: [i32; 3],
        state: u32,
    },
    /// `HasSturdyFacePredicate`：那一格在指定方向的 `isFaceSturdy(FULL)`。
    HasSturdyFace {
        offset: [i32; 3],
        direction: u32,
    },
    /// `MatchingFluidsPredicate`：那一格的流体在给定 holderset 里。
    MatchingFluids {
        offset: [i32; 3],
        fluids: Vec<i32>,
    },
    /// 未移植的谓词类型（含存活判定未移植的 would_survive）。
    ///
    /// Unported predicate kind (including would_survive with an unported state).
    Unsupported,
}

impl BlockPredicate {
    pub fn parse(value: &Value) -> Self {
        let Some(object) = value.as_object() else {
            return Self::Unsupported;
        };
        let Some(kind) = object.get("type").and_then(Value::as_str) else {
            return Self::Unsupported;
        };
        let offset = parse_offset(object.get("offset"));
        match kind {
            "minecraft:matching_block_tag" => {
                let Some(tag) = object.get("tag").and_then(Value::as_str) else {
                    return Self::Unsupported;
                };
                Self::MatchingBlockTag {
                    offset,
                    tag: tag.to_owned(),
                }
            }
            "minecraft:matching_blocks" => {
                let Some(blocks) = parse_blocks(object.get("blocks")) else {
                    return Self::Unsupported;
                };
                Self::MatchingBlocks { offset, blocks }
            }
            "minecraft:all_of" => Self::AllOf(parse_list(object.get("predicates"))),
            "minecraft:any_of" => Self::AnyOf(parse_list(object.get("predicates"))),
            "minecraft:not" => Self::Not(Box::new(
                object
                    .get("predicate")
                    .map(Self::parse)
                    .unwrap_or(Self::Unsupported),
            )),
            "minecraft:replaceable" => Self::Replaceable { offset },
            "minecraft:solid" => Self::Solid { offset },
            "minecraft:true" => Self::True,
            "minecraft:inside_world_bounds" => Self::InsideWorldBounds { offset },
            // `state.canSurvive`；存活判定未移植的方块仍标 unsupported。
            // `state.canSurvive`; a state whose survival check is unported stays unsupported.
            "minecraft:would_survive" => {
                let state = object.get("state").and_then(|v| parse_state(v).ok());
                match state {
                    Some(state) if survival::supports(state) => {
                        Self::WouldSurvive { offset, state }
                    }
                    _ => Self::Unsupported,
                }
            }
            "minecraft:has_sturdy_face" => {
                let Some(direction) = object
                    .get("direction")
                    .and_then(Value::as_str)
                    .and_then(parse_direction)
                else {
                    return Self::Unsupported;
                };
                Self::HasSturdyFace { offset, direction }
            }
            "minecraft:matching_fluids" => {
                let Some(fluids) = parse_fluids(object.get("fluids")) else {
                    return Self::Unsupported;
                };
                Self::MatchingFluids { offset, fluids }
            }
            _ => Self::Unsupported,
        }
    }

    /// 是否所有分支都已移植；含 `Unsupported` 时该 placed feature 标 unsupported。
    ///
    /// Whether every branch is ported; a contained `Unsupported` skips the feature.
    pub fn is_supported(&self) -> bool {
        match self {
            Self::Unsupported => false,
            Self::AllOf(list) | Self::AnyOf(list) => list.iter().all(Self::is_supported),
            Self::Not(inner) => inner.is_supported(),
            Self::WouldSurvive { state, .. } => survival::supports(*state),
            _ => true,
        }
    }

    /// `BlockPredicate.test(WorldGenLevel, BlockPos)`，对 3×3 世界视图求值。
    ///
    /// `BlockPredicate.test(WorldGenLevel, BlockPos)` over the 3×3 world view.
    pub fn test(&self, world: &FeatureWorld<'_>, pos: Pos) -> bool {
        match self {
            Self::MatchingBlockTag { offset, tag } => {
                let state = state_at(world, pos, offset);
                block_tag(tag).is_some_and(|ids| ids.contains(&block_id(state)))
            }
            Self::MatchingBlocks { offset, blocks } => {
                let state = state_at(world, pos, offset);
                blocks.contains(&block_id(state))
            }
            Self::AllOf(list) => list.iter().all(|p| p.test(world, pos)),
            Self::AnyOf(list) => list.iter().any(|p| p.test(world, pos)),
            Self::Not(inner) => !inner.test(world, pos),
            Self::Replaceable { offset } => {
                state_facts(state_at(world, pos, offset)) & facts::REPLACEABLE != 0
            }
            Self::Solid { offset } => state_facts(state_at(world, pos, offset)) & facts::SOLID != 0,
            Self::True => true,
            // `level.isInsideBuildHeight(origin.offset(offset))`。
            // `level.isInsideBuildHeight(origin.offset(offset))`.
            Self::InsideWorldBounds { offset } => world.is_inside_build_height(pos.1 + offset[1]),
            // `state.canSurvive(level, origin+offset)`。
            Self::WouldSurvive { offset, state } => survival::can_survive(
                *state,
                world,
                (pos.0 + offset[0], pos.1 + offset[1], pos.2 + offset[2]),
            ),
            // `isFaceSturdy(FULL)`：facts 里按 Direction ordinal 预存了这一位。
            // `isFaceSturdy(FULL)`: facts pre-stores this bit per Direction ordinal.
            Self::HasSturdyFace { offset, direction } => {
                let bits = state_facts(state_at(world, pos, offset));
                bits & (1u64 << (*direction + facts::STURDY_FULL)) != 0
            }
            // `state.getFluidState().is(fluids)`：比流体注册表 ID。
            // `state.getFluidState().is(fluids)`: compare the fluid registry ID.
            Self::MatchingFluids { offset, fluids } => {
                let fluid = state_fluid(state_at(world, pos, offset)).fluid;
                fluids.contains(&fluid)
            }
            // `is_supported` 保证这个分支不会被求值。
            // `is_supported` guarantees this branch is never evaluated.
            Self::Unsupported => false,
        }
    }
}

/// 状态所属方块的注册表 ID；`block_tag` / `matching_blocks` 用的都是这个 ID。
///
/// The block registry ID of a state, the same ID used by block tags.
fn block_id(state: u32) -> i32 {
    block_id_of_state(state).map_or(-1, |id| id as i32)
}

fn state_at(world: &FeatureWorld<'_>, pos: Pos, offset: &[i32; 3]) -> u32 {
    world.get_block(pos.0 + offset[0], pos.1 + offset[1], pos.2 + offset[2])
}

/// `Vec3i.offsetCodec`：`[x, y, z]`，缺省 `[0, 0, 0]`。
fn parse_offset(value: Option<&Value>) -> [i32; 3] {
    let Some(array) = value.and_then(Value::as_array) else {
        return [0, 0, 0];
    };
    if array.len() != 3 {
        return [0, 0, 0];
    }
    let mut result = [0; 3];
    for (index, item) in array.iter().enumerate() {
        result[index] = item.as_i64().unwrap_or(0) as i32;
    }
    result
}

/// `HolderSet<Block>`：单个方块名、`#tag`，或它们的数组。名字解析成方块注册表 ID。
///
/// `HolderSet<Block>`: a block name, a `#tag`, or an array of them, resolved to IDs.
fn parse_blocks(value: Option<&Value>) -> Option<Vec<i32>> {
    let mut result = Vec::new();
    match value? {
        Value::String(name) => push_block(name, &mut result)?,
        Value::Array(items) => {
            for item in items {
                push_block(item.as_str()?, &mut result)?;
            }
        }
        _ => return None,
    }
    Some(result)
}

fn push_block(name: &str, out: &mut Vec<i32>) -> Option<()> {
    if let Some(tag) = name.strip_prefix('#') {
        let ids = block_tag(tag)?;
        out.extend_from_slice(ids);
    } else {
        let block = block_by_name(name)?;
        out.push(block_id_of_state(block.default_state)? as i32);
    }
    Some(())
}

/// `Direction` 的 ordinal（DOWN, UP, NORTH, SOUTH, WEST, EAST）。
fn parse_direction(name: &str) -> Option<u32> {
    Some(match name {
        "down" => 0,
        "up" => 1,
        "north" => 2,
        "south" => 3,
        "west" => 4,
        "east" => 5,
        _ => return None,
    })
}

/// `HolderSet<Fluid>`：单个流体名、`#tag`，或它们的数组，解析成流体注册表 ID。
///
/// `HolderSet<Fluid>`: a fluid name, `#tag`, or an array of them, resolved to IDs.
fn parse_fluids(value: Option<&Value>) -> Option<Vec<i32>> {
    let mut out = Vec::new();
    match value? {
        Value::String(name) => push_fluid(name, &mut out)?,
        Value::Array(items) => {
            for item in items {
                push_fluid(item.as_str()?, &mut out)?;
            }
        }
        _ => return None,
    }
    Some(out)
}

fn push_fluid(name: &str, out: &mut Vec<i32>) -> Option<()> {
    if let Some(tag_name) = name.strip_prefix('#') {
        let group = tags::TAG_GROUPS.get(tags::fluid_tags::GROUP)?;
        let tag = group.tags.iter().find(|t| t.name == tag_name)?;
        out.extend_from_slice(tag.ids);
    } else {
        out.push(registries::fluid::by_name(name)?);
    }
    Some(())
}

fn parse_list(value: Option<&Value>) -> Vec<BlockPredicate> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().map(BlockPredicate::parse).collect())
        .unwrap_or_else(|| vec![BlockPredicate::Unsupported])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_air_tag_and_block_set_with_offset() {
        let air: Value = json!({"type": "minecraft:matching_block_tag", "tag": "minecraft:air"});
        assert!(matches!(
            BlockPredicate::parse(&air),
            BlockPredicate::MatchingBlockTag { .. }
        ));
        let below: Value = json!({
            "type": "minecraft:matching_blocks",
            "blocks": "minecraft:grass_block",
            "offset": [0, -1, 0],
        });
        assert!(matches!(
            BlockPredicate::parse(&below),
            BlockPredicate::MatchingBlocks {
                offset: [0, -1, 0],
                ..
            }
        ));
    }

    #[test]
    fn unported_kind_marks_the_predicate_unsupported() {
        // would_survive 的 state 若存活判定未移植（苍白苔藓地毯要邻接判定），仍标 unsupported。
        // A would_survive with an unported survival check (pale moss carpet needs adjacency)
        // stays unsupported.
        let would: Value =
            json!({"type": "minecraft:would_survive", "state": "minecraft:pale_moss_carpet"});
        assert!(!BlockPredicate::parse(&would).is_supported());
        // all_of 里夹一个未移植的，整体也要 unsupported。
        // An unported child makes the whole all_of unsupported.
        let all: Value = json!({
            "type": "minecraft:all_of",
            "predicates": [
                {"type": "minecraft:matching_block_tag", "tag": "minecraft:air"},
                {"type": "minecraft:would_survive", "state": "minecraft:pale_moss_carpet"},
            ],
        });
        assert!(!BlockPredicate::parse(&all).is_supported());
    }

    #[test]
    fn cactus_and_sugar_cane_would_survive_are_supported() {
        for name in ["minecraft:cactus", "minecraft:sugar_cane"] {
            let would: Value = json!({"type": "minecraft:would_survive", "state": name});
            assert!(BlockPredicate::parse(&would).is_supported(), "{name}");
        }
    }

    #[test]
    fn ports_would_survive_sturdy_face_and_matching_fluids() {
        let would: Value =
            json!({"type": "minecraft:would_survive", "state": "minecraft:oak_sapling"});
        assert!(matches!(
            BlockPredicate::parse(&would),
            BlockPredicate::WouldSurvive { .. }
        ));
        assert!(BlockPredicate::parse(&would).is_supported());
        let sturdy: Value = json!({
            "type": "minecraft:has_sturdy_face",
            "direction": "up",
            "offset": [0, -1, 0],
        });
        assert!(matches!(
            BlockPredicate::parse(&sturdy),
            BlockPredicate::HasSturdyFace { direction: 1, .. }
        ));
        assert!(BlockPredicate::parse(&sturdy).is_supported());
        let fluids: Value = json!({
            "type": "minecraft:matching_fluids",
            "fluids": ["minecraft:water", "minecraft:flowing_water"],
            "offset": [1, -1, 0],
        });
        assert!(matches!(
            BlockPredicate::parse(&fluids),
            BlockPredicate::MatchingFluids { .. }
        ));
        assert!(BlockPredicate::parse(&fluids).is_supported());
        let empty: Value =
            json!({"type": "minecraft:matching_fluids", "fluids": "minecraft:empty"});
        assert!(BlockPredicate::parse(&empty).is_supported());
    }

    #[test]
    fn supported_combinators_stay_supported() {
        let all: Value = json!({
            "type": "minecraft:all_of",
            "predicates": [
                {"type": "minecraft:matching_block_tag", "tag": "minecraft:air"},
                {"type": "minecraft:matching_blocks", "blocks": "minecraft:grass_block", "offset": [0,-1,0]},
            ],
        });
        assert!(BlockPredicate::parse(&all).is_supported());
    }
}
