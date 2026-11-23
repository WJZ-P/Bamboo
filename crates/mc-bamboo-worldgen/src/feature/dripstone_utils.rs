//! 滴水石家族共用的工具：`FloatProvider`、官方 `Mth` 的几个函数、`Column.scan`、
//! `SpeleothemUtils` 的判定与放置，以及带属性的 `pointed_dripstone` 方块状态。
//!
//! Dripstone-family shared utilities: `FloatProvider`, a few official `Mth` functions,
//! `Column.scan`, `SpeleothemUtils` predicates/placement, and property-bearing
//! `pointed_dripstone` block states.

use mc_bamboo_data::{
    block_id_of_state, block_tag, blocks, facts, registries, state_facts, state_fluid,
};
use serde_json::Value;

use crate::{math, random::WorldgenRandom};

use super::{state::apply_property, world::FeatureWorld};

/// `FloatProvider`：本轮滴水石只会用到常量、uniform、clamped_normal 三种。
///
/// `FloatProvider`: only constant / uniform / clamped_normal are used by dripstone.
#[derive(Debug, Clone, Copy)]
pub enum FloatProvider {
    Constant(f32),
    Uniform {
        min_inclusive: f32,
        max_exclusive: f32,
    },
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min: f32,
        max: f32,
    },
}

impl FloatProvider {
    pub fn parse(value: &Value) -> Result<Self, String> {
        // 普通数字是 `ConstantFloat`，不消费随机数。
        // A plain number is `ConstantFloat` and consumes no randomness.
        if let Some(number) = value.as_f64() {
            return Ok(Self::Constant(number as f32));
        }
        let object = value
            .as_object()
            .ok_or("float provider must be a number or object")?;
        let kind = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or("float provider has no type")?;
        let float = |name: &str| -> Result<f32, String> {
            object
                .get(name)
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .ok_or_else(|| format!("float provider {kind} has no {name}"))
        };
        Ok(match kind {
            "minecraft:uniform" => Self::Uniform {
                min_inclusive: float("min_inclusive")?,
                max_exclusive: float("max_exclusive")?,
            },
            "minecraft:clamped_normal" => Self::ClampedNormal {
                mean: float("mean")?,
                deviation: float("deviation")?,
                min: float("min")?,
                max: float("max")?,
            },
            _ => return Err(format!("unsupported float provider {kind}")),
        })
    }

    pub fn sample(self, random: &WorldgenRandom) -> f32 {
        match self {
            Self::Constant(value) => value,
            Self::Uniform {
                min_inclusive,
                max_exclusive,
            } => {
                // `Mth.randomBetween`：nextFloat() * (max - min) + min。
                // `Mth.randomBetween`: nextFloat() * (max - min) + min.
                random.next_float() * (max_exclusive - min_inclusive) + min_inclusive
            }
            Self::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => {
                // `Mth.normal`：mean + (float)nextGaussian() * deviation，再 clamp。
                // `Mth.normal`: mean + (float)nextGaussian() * deviation, then clamp.
                let normal = mean + (random.next_gaussian() as f32) * deviation;
                math::clamp(normal, min, max)
            }
        }
    }
}

/// `Mth.sin(double)` / `cos(double)`：官方 65536 项查找表，全进程只留一份。
/// `Mth.sin(double)` / `cos(double)`: the official 65536-entry table, one copy per process.
pub(crate) fn mth_sin(value: f64) -> f32 {
    mc_bamboo_core::mth::sin(value)
}

pub(crate) fn mth_cos(value: f64) -> f32 {
    mc_bamboo_core::mth::cos(value)
}

/// `Mth.clampedMap(double/float, ...)`：inverseLerp 后 clampedLerp。
/// `Mth.clampedMap(double/float, ...)`: inverseLerp then clampedLerp.
pub(crate) fn clamped_map_f32(
    value: f32,
    from_min: f32,
    from_max: f32,
    to_min: f32,
    to_max: f32,
) -> f32 {
    let factor = (value - from_min) / (from_max - from_min);
    if factor < 0.0 {
        to_min
    } else if factor > 1.0 {
        to_max
    } else {
        to_min + factor * (to_max - to_min)
    }
}

pub(crate) fn clamped_map_f64(
    value: f64,
    from_min: f64,
    from_max: f64,
    to_min: f64,
    to_max: f64,
) -> f64 {
    let factor = (value - from_min) / (from_max - from_min);
    if factor < 0.0 {
        to_min
    } else if factor > 1.0 {
        to_max
    } else {
        to_min + factor * (to_max - to_min)
    }
}

/// `Mth.clamp(int, int, int)` = `Math.min(Math.max(v, min), max)`。
/// `Mth.clamp(int, int, int)` = `Math.min(Math.max(v, min), max)`.
pub(crate) fn clamp_i32(value: i32, min: i32, max: i32) -> i32 {
    value.max(min).min(max)
}

/// `SpeleothemUtils.getSpeleothemHeight`：按官方的双精度表达式，`cutoff` 是死变量不用。
/// `SpeleothemUtils.getSpeleothemHeight`: the official double expression; `cutoff` is unused.
pub(crate) fn get_speleothem_height(mut xz: f64, radius: f64, scale: f64, bluntness: f64) -> f64 {
    if xz < bluntness {
        xz = bluntness;
    }
    let r = xz / radius * 0.384;
    let part1 = 0.75 * r.powf(1.3333333333333333);
    let part2 = r.powf(0.6666666666666666);
    let part3 = 0.3333333333333333 * r.ln();
    let mut height = scale * (part1 - part2 - part3);
    // `Math.max(height, 0.0)`；本轮 r > 0，不会出现 NaN。
    // `Math.max(height, 0.0)`; r > 0 so no NaN arises this round.
    if height < 0.0 {
        height = 0.0;
    }
    height / 0.384 * radius
}

/// `Direction.values()` 的顺序：DOWN, UP, NORTH, SOUTH, WEST, EAST。
/// `Direction.values()` order: DOWN, UP, NORTH, SOUTH, WEST, EAST.
pub const DIRECTION_OFFSETS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

/// `Direction.Plane.HORIZONTAL`：NORTH, EAST, SOUTH, WEST（注意不是 values() 顺序）。
/// `Direction.Plane.HORIZONTAL`: NORTH, EAST, SOUTH, WEST (not the values() order).
pub const HORIZONTAL: [usize; 4] = [2, 5, 3, 4];

/// 方块状态判定的公共构件。
/// Shared block-state predicates.
pub fn is_air(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0
}

/// `BlockState.is(Block)`：按方块注册表 ID 比。
/// `BlockState.is(Block)`: compare block registry IDs.
pub fn same_block(state: u32, other_state: u32) -> bool {
    block_id_of_state(state)
        .zip(block_id_of_state(other_state))
        .is_some_and(|(a, b)| a == b)
}

pub fn is_empty_or_water(state: u32) -> bool {
    is_air(state) || same_block(state, blocks::WATER)
}

pub fn is_empty_or_water_or_lava(state: u32) -> bool {
    is_empty_or_water(state) || same_block(state, blocks::LAVA)
}

pub fn is_neither_empty_nor_water(state: u32) -> bool {
    !is_air(state) && !same_block(state, blocks::WATER)
}

/// `state.is(HolderSet<Block>)`，标签名带命名空间（如 `minecraft:base_stone_overworld`）。
/// `state.is(HolderSet<Block>)`; tag name includes the namespace.
pub fn in_block_tag(state: u32, tag: &str) -> bool {
    block_id_of_state(state)
        .is_some_and(|id| block_tag(tag).is_some_and(|ids| ids.contains(&(id as i32))))
}

/// `FluidState.is(FluidTags.WATER)`：流体是 water 或 flowing_water。
/// `FluidState.is(FluidTags.WATER)`: fluid is water or flowing_water.
pub fn fluid_is_water(state: u32) -> bool {
    let fluid = state_fluid(state).fluid;
    fluid == registries::fluid::WATER || fluid == registries::fluid::FLOWING_WATER
}

/// `LevelReader.isWaterAt(pos)`：读这一格方块状态的流体。
/// `LevelReader.isWaterAt(pos)`: read the fluid of the block state at pos.
pub fn is_water_at(world: &FeatureWorld<'_>, x: i32, y: i32, z: i32) -> bool {
    fluid_is_water(world.get_block(x, y, z))
}

/// `SpeleothemUtils.isBase`：`state.is(baseBlock) || state.is(replaceableBlocks)`。
/// `SpeleothemUtils.isBase`: `state.is(baseBlock) || state.is(replaceableBlocks)`.
pub fn is_base(state: u32, base_block_state: u32, replaceable: &str) -> bool {
    same_block(state, base_block_state) || in_block_tag(state, replaceable)
}

pub fn is_base_or_lava(state: u32, base_block_state: u32, replaceable: &str) -> bool {
    is_base(state, base_block_state, replaceable) || same_block(state, blocks::LAVA)
}

/// `SpeleothemUtils.isCircleMostlyEmbeddedInStone`。
/// `SpeleothemUtils.isCircleMostlyEmbeddedInStone`.
pub fn is_circle_mostly_embedded_in_stone(
    world: &FeatureWorld<'_>,
    center: (i32, i32, i32),
    radius: i32,
) -> bool {
    let (cx, cy, cz) = center;
    if is_empty_or_water_or_lava(world.get_block(cx, cy, cz)) {
        return false;
    }
    let angle_increment = 6.0_f32 / radius as f32;
    let limit = (std::f64::consts::PI * 2.0) as f32;
    let mut angle = 0.0_f32;
    while angle < limit {
        let dx = (mth_cos(f64::from(angle)) * radius as f32) as i32;
        let dz = (mth_sin(f64::from(angle)) * radius as f32) as i32;
        if is_empty_or_water_or_lava(world.get_block(cx + dx, cy, cz + dz)) {
            return false;
        }
        angle += angle_increment;
    }
    true
}

/// `Column.scan` 的结果：`create(floor, ceiling)` 得到的 Range/Ray/Line。
/// `Column.scan` result: the Range/Ray/Line from `create(floor, ceiling)`.
#[derive(Debug, Clone, Copy)]
pub struct Column {
    pub floor: Option<i32>,
    pub ceiling: Option<i32>,
}

impl Column {
    /// 是否 `Column.Range`（两端都在）。
    /// Whether this is a `Column.Range` (both ends present).
    pub fn is_range(self) -> bool {
        self.floor.is_some() && self.ceiling.is_some()
    }

    /// `Range.height() = ceiling - floor - 1`；Ray/Line 返回 None。
    /// `Range.height() = ceiling - floor - 1`; None for Ray/Line.
    pub fn height(self) -> Option<i32> {
        Some(self.ceiling? - self.floor? - 1)
    }

    /// `Column.withFloor(OptionalInt.of(newFloor))`。
    /// `Column.withFloor(OptionalInt.of(newFloor))`.
    pub fn with_floor(self, floor: i32) -> Self {
        Self {
            floor: Some(floor),
            ceiling: self.ceiling,
        }
    }
}

fn scan_direction(
    world: &FeatureWorld<'_>,
    pos: (i32, i32, i32),
    search_range: i32,
    inside: &impl Fn(u32) -> bool,
    edge: &impl Fn(u32) -> bool,
    dir: i32,
) -> Option<i32> {
    let (x, start_y, z) = pos;
    let mut y = start_y;
    let mut i = 1;
    while i < search_range && inside(world.get_block(x, y, z)) {
        y += dir;
        i += 1;
    }
    if edge(world.get_block(x, y, z)) {
        Some(y)
    } else {
        None
    }
}

/// `Column.scan(level, pos, searchRange, insideColumn, validEdge)`：先判原点，
/// 再各自朝 UP / DOWN 扫到第一个不满足 inside 的格子，用 validEdge 判定是否成边。
///
/// `Column.scan(...)`: check the origin, then scan UP / DOWN to the first cell failing
/// `inside`, and use `validEdge` to decide whether it is a floor/ceiling.
pub fn scan_column(
    world: &FeatureWorld<'_>,
    pos: (i32, i32, i32),
    search_range: i32,
    inside: impl Fn(u32) -> bool,
    edge: impl Fn(u32) -> bool,
) -> Option<Column> {
    let (x, y, z) = pos;
    if !inside(world.get_block(x, y, z)) {
        return None;
    }
    let ceiling = scan_direction(world, pos, search_range, &inside, &edge, 1);
    let floor = scan_direction(world, pos, search_range, &inside, &edge, -1);
    Some(Column { floor, ceiling })
}

/// `SpeleothemUtils.createPointedBlock`：默认状态上设 `vertical_direction` 和
/// `thickness`；`waterlogged` 由 `growSpeleothem` 按 `isWaterAt` 再设。
///
/// `SpeleothemUtils.createPointedBlock`: set `vertical_direction` and `thickness` on
/// the default state; `waterlogged` is set later by `growSpeleothem` via `isWaterAt`.
pub fn create_pointed_block(pointed_default: u32, vertical: &str, thickness: &str) -> u32 {
    let state =
        apply_property(pointed_default, "vertical_direction", vertical).unwrap_or(pointed_default);
    apply_property(state, "thickness", thickness).unwrap_or(state)
}

/// 放一格 pointed 方块并按 tip 方向移动游标，照 `growSpeleothem` 的 consumer。
/// Place one pointed block and advance the cursor along the tip direction, per
/// the `growSpeleothem` consumer.
fn place_pointed(
    world: &mut FeatureWorld<'_>,
    pos: &mut (i32, i32, i32),
    pointed_default: u32,
    vertical: &str,
    thickness: &str,
    tip_up: bool,
) {
    let mut state = create_pointed_block(pointed_default, vertical, thickness);
    let wet = is_water_at(world, pos.0, pos.1, pos.2);
    state =
        apply_property(state, "waterlogged", if wet { "true" } else { "false" }).unwrap_or(state);
    world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
    pos.1 += if tip_up { 1 } else { -1 };
}

/// `growSpeleothem` / `replaceBlocksWithBaseBlocks` 要用的三个方块材料。
/// The three block materials used by `growSpeleothem` / `replaceBlocksWithBaseBlocks`.
#[derive(Debug, Clone, Copy)]
pub struct Materials<'a> {
    pub base_block: u32,
    pub pointed_default: u32,
    pub replaceable: &'a str,
}

/// `SpeleothemUtils.growSpeleothem` + `buildBaseToTipColumn`。
/// `SpeleothemUtils.growSpeleothem` + `buildBaseToTipColumn`.
pub fn grow_speleothem(
    world: &mut FeatureWorld<'_>,
    start: (i32, i32, i32),
    tip_up: bool,
    height: i32,
    merged_tip: bool,
    mats: Materials<'_>,
) {
    let vertical = if tip_up { "up" } else { "down" };
    // tipDirection.getOpposite()：UP→DOWN(-1)，DOWN→UP(+1)。
    // tipDirection.getOpposite(): UP→DOWN(-1), DOWN→UP(+1).
    let opposite_y = if tip_up { -1 } else { 1 };
    let (sx, sy, sz) = start;
    if !is_base(
        world.get_block(sx, sy + opposite_y, sz),
        mats.base_block,
        mats.replaceable,
    ) {
        return;
    }
    let mut pos = start;
    if height >= 3 {
        place_pointed(
            world,
            &mut pos,
            mats.pointed_default,
            vertical,
            "base",
            tip_up,
        );
        for _ in 0..height - 3 {
            place_pointed(
                world,
                &mut pos,
                mats.pointed_default,
                vertical,
                "middle",
                tip_up,
            );
        }
    }
    if height >= 2 {
        place_pointed(
            world,
            &mut pos,
            mats.pointed_default,
            vertical,
            "frustum",
            tip_up,
        );
    }
    if height >= 1 {
        place_pointed(
            world,
            &mut pos,
            mats.pointed_default,
            vertical,
            if merged_tip { "tip_merge" } else { "tip" },
            tip_up,
        );
    }
}

/// `SpeleothemUtils.placeBaseBlockIfPossible`。
/// `SpeleothemUtils.placeBaseBlockIfPossible`.
pub fn place_base_block_if_possible(
    world: &mut FeatureWorld<'_>,
    pos: (i32, i32, i32),
    base_block_state: u32,
    replaceable: &str,
) -> bool {
    if in_block_tag(world.get_block(pos.0, pos.1, pos.2), replaceable) {
        world.set_block_with_heightmaps(pos.0, pos.1, pos.2, base_block_state);
        true
    } else {
        false
    }
}

/// `SpeleothemClusterFeature.replaceBlocksWithBaseBlocks`。
/// `SpeleothemClusterFeature.replaceBlocksWithBaseBlocks`.
pub fn replace_blocks_with_base_blocks(
    world: &mut FeatureWorld<'_>,
    first: (i32, i32, i32),
    max_count: i32,
    dy: i32,
    base_block_state: u32,
    replaceable: &str,
) {
    let mut pos = first;
    for _ in 0..max_count {
        if !place_base_block_if_possible(world, pos, base_block_state, replaceable) {
            return;
        }
        pos.1 += dy;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::state::{official_name, state_from_official_name};
    use mc_bamboo_data::blocks;

    #[test]
    fn float_provider_parses_the_exported_shapes() {
        let constant: Value = serde_json::from_str("0").unwrap();
        assert!(
            matches!(FloatProvider::parse(&constant).unwrap(), FloatProvider::Constant(v) if v == 0.0)
        );
        let uniform: Value = serde_json::from_str(
            r#"{"type":"minecraft:uniform","min_inclusive":0.3,"max_exclusive":0.7}"#,
        )
        .unwrap();
        assert!(matches!(
            FloatProvider::parse(&uniform).unwrap(),
            FloatProvider::Uniform { .. }
        ));
        let normal: Value = serde_json::from_str(
            r#"{"type":"minecraft:clamped_normal","mean":0.1,"deviation":0.3,"min":0.1,"max":0.9}"#,
        )
        .unwrap();
        assert!(matches!(
            FloatProvider::parse(&normal).unwrap(),
            FloatProvider::ClampedNormal { .. }
        ));
    }

    #[test]
    fn pointed_block_states_round_trip_by_property() {
        // `createPointedBlock` 用的三个属性都要能精确还原成状态 ID。
        // All three properties used by `createPointedBlock` must round-trip to a state ID.
        for direction in ["up", "down"] {
            for thickness in ["tip_merge", "tip", "frustum", "middle", "base"] {
                for waterlogged in ["true", "false"] {
                    let mut state =
                        create_pointed_block(blocks::POINTED_DRIPSTONE, direction, thickness);
                    state = apply_property(state, "waterlogged", waterlogged).unwrap();
                    let name = official_name(state);
                    assert!(
                        name.contains(&format!("vertical_direction={direction}")),
                        "{name}"
                    );
                    assert!(name.contains(&format!("thickness={thickness}")), "{name}");
                    assert!(
                        name.contains(&format!("waterlogged={waterlogged}")),
                        "{name}"
                    );
                    assert_eq!(state_from_official_name(&name).unwrap(), state, "{name}");
                }
            }
        }
    }
}
