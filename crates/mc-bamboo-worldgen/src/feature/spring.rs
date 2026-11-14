//! `SpringFeature`：在洞穴岩壁里放一格流体。
//!
//! `place` 先检查上方、下方、当前格和四周的 `valid_blocks` / 空气计数，全对上了才
//! `setBlock(origin, state.createLegacyBlock(), 2)` 并 `scheduleTick`。这里只关心方块，
//! `scheduleTick` 不改方块、不消费随机数，所以对拍时忽略。
//!
//! SpringFeature places a fluid block in cave walls. `place` checks the block above,
//! below, at the origin and around it against `valid_blocks` / air counts, then writes
//! `state.createLegacyBlock()` with flags 2. Scheduled ticks don't change blocks or
//! consume randomness, so the oracle ignores them.

use mc_bamboo_data::{block_by_name, block_id_of_state, facts, state_facts};
use serde_json::Value;

use super::{state::apply_property, world::FeatureWorld};

/// `SpringFeature` 的配置；`state` 已经是 `createLegacyBlock()` 之后的状态 ID。
///
/// SpringFeature config; `state` is already the `createLegacyBlock()` state ID.
#[derive(Debug, Clone)]
pub struct SpringFeature {
    pub state: u32,
    pub requires_block_below: bool,
    pub rock_count: i32,
    pub hole_count: i32,
    pub valid_blocks: Vec<usize>,
}

/// `FluidState.createLegacyBlock()`：water/lava 源流体带 `falling=true` 时 level 为 0。
///
/// `FluidState.createLegacyBlock()`: source water/lava with `falling=true` maps to level 0.
fn fluid_legacy_block(value: &Value) -> Result<u32, String> {
    let object = value
        .as_object()
        .ok_or("spring fluid state must be an object")?;
    let id = object
        .get("id")
        .or_else(|| object.get("Name"))
        .and_then(Value::as_str)
        .ok_or("spring fluid state has no id")?;
    let falling = object
        .get("properties")
        .or_else(|| object.get("Properties"))
        .and_then(|p| p.get("falling"))
        .and_then(Value::as_str)
        == Some("true");
    // getLegacyLevel = isSource ? 0 : 8 - min(amount,8) + (falling ? 8 : 0)。
    // `minecraft:water`/`minecraft:lava` 是源流体；这里用到的配置都是源流体。
    // getLegacyLevel; the spring configs in this version all use source fluids.
    let source = matches!(id, "minecraft:water" | "minecraft:lava");
    let level = if source {
        0
    } else {
        let amount = object
            .get("properties")
            .or_else(|| object.get("Properties"))
            .and_then(|p| p.get("level"))
            .and_then(Value::as_i64)
            .unwrap_or(8) as i32;
        8 - amount.min(8) + if falling { 8 } else { 0 }
    };
    let block = match id {
        "minecraft:water" | "minecraft:flowing_water" => "minecraft:water",
        "minecraft:lava" | "minecraft:flowing_lava" => "minecraft:lava",
        other => return Err(format!("unsupported spring fluid {other}")),
    };
    let base = block_by_name(block)
        .ok_or_else(|| format!("unknown block {block}"))?
        .default_state;
    if level == 0 {
        Ok(base)
    } else {
        apply_property(base, "level", &level.to_string())
            .ok_or_else(|| format!("cannot set {block} level={level}"))
    }
}

fn block_set(value: &Value) -> Result<Vec<usize>, String> {
    let names: Vec<&str> = match value {
        Value::String(name) => vec![name.as_str()],
        Value::Array(entries) => entries
            .iter()
            .map(|e| {
                e.as_str()
                    .ok_or_else(|| "valid_blocks entry is not a string".to_owned())
            })
            .collect::<Result<Vec<_>, String>>()?,
        _ => return Err("valid_blocks is neither a string nor an array".to_owned()),
    };
    names
        .iter()
        .map(|name| {
            block_by_name(name)
                .and_then(|block| block_id_of_state(block.first_state))
                .ok_or_else(|| format!("unknown valid block {name}"))
        })
        .collect()
}

impl SpringFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config
            .as_object()
            .ok_or("spring feature must be an object")?;
        Ok(Self {
            state: fluid_legacy_block(object.get("state").ok_or("spring has no state")?)?,
            requires_block_below: object
                .get("requires_block_below")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            rock_count: object
                .get("rock_count")
                .and_then(Value::as_i64)
                .map(|n| n as i32)
                .unwrap_or(4),
            hole_count: object
                .get("hole_count")
                .and_then(Value::as_i64)
                .map(|n| n as i32)
                .unwrap_or(1),
            valid_blocks: block_set(
                object
                    .get("valid_blocks")
                    .ok_or("spring has no valid_blocks")?,
            )?,
        })
    }
}

/// `state.is(validBlocks)`：按方块注册表 ID 判断。
///
/// `state.is(validBlocks)`: compares block registry IDs.
fn is_valid_block(state: u32, valid: &[usize]) -> bool {
    block_id_of_state(state).is_some_and(|id| valid.contains(&id))
}

/// `SpringFeature.place`。
///
/// `SpringFeature.place`.
pub fn place(
    spring: &SpringFeature,
    world: &mut FeatureWorld<'_>,
    origin: (i32, i32, i32),
) -> bool {
    let (x, y, z) = origin;
    let above = world.get_block(x, y + 1, z);
    if !is_valid_block(above, &spring.valid_blocks) {
        return false;
    }
    if spring.requires_block_below {
        let below = world.get_block(x, y - 1, z);
        if !is_valid_block(below, &spring.valid_blocks) {
            return false;
        }
    }
    let current = world.get_block(x, y, z);
    let current_air = state_facts(current) & facts::AIR != 0;
    if !current_air && !is_valid_block(current, &spring.valid_blocks) {
        return false;
    }
    // west, east, north, south, below（官方顺序）。
    // west, east, north, south, below (official order).
    let neighbours = [
        (x - 1, y, z),
        (x + 1, y, z),
        (x, y, z - 1),
        (x, y, z + 1),
        (x, y - 1, z),
    ];
    let mut rock = 0;
    let mut hole = 0;
    for (nx, ny, nz) in neighbours {
        let state = world.get_block(nx, ny, nz);
        if is_valid_block(state, &spring.valid_blocks) {
            rock += 1;
        }
        if state_facts(state) & facts::AIR != 0 {
            hole += 1;
        }
    }
    if rock != spring.rock_count || hole != spring.hole_count {
        return false;
    }
    world.set_block_state(x, y, z, spring.state);
    true
}
