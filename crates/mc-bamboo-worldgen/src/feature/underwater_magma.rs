//! `UnderwaterMagmaFeature`：在水柱底部附近、被实心方块包住的位置铺岩浆块。
//!
//! `place` 先 `Column.scan` 找水柱的 floor（向下第一个非水方块），再以它为中心取
//! (2r+1)³ 的盒子，按 `BlockPos.betweenClosedStream` 顺序对每个位置取一次
//! `nextFloat()`；通过概率和 `isValidPlacement` 的位置写 `magma_block`。
//!
//! UnderwaterMagmaFeature places magma blocks near the floor of a water column,
//! enclosed by solid neighbours. It scans for the floor with `Column.scan`, takes
//! a (2r+1)³ box, draws one `nextFloat()` per position in `betweenClosedStream`
//! order, and writes `magma_block` for positions passing the probability and
//! `isValidPlacement` checks.

use mc_bamboo_data::{block_by_name, block_id_of_state, blocks, facts, state_facts};
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::world::FeatureWorld;

/// `UnderwaterMagmaFeature` 的配置。
///
/// Configuration of `UnderwaterMagmaFeature`.
#[derive(Debug, Clone)]
pub struct UnderwaterMagmaFeature {
    pub floor_search_range: i32,
    pub placement_radius_around_floor: i32,
    pub placement_probability_per_valid_position: f32,
}

impl UnderwaterMagmaFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config
            .as_object()
            .ok_or("underwater_magma must be an object")?;
        Ok(Self {
            floor_search_range: object
                .get("floor_search_range")
                .and_then(Value::as_i64)
                .ok_or("underwater_magma has no floor_search_range")?
                as i32,
            placement_radius_around_floor: object
                .get("placement_radius_around_floor")
                .and_then(Value::as_i64)
                .ok_or("underwater_magma has no placement_radius_around_floor")?
                as i32,
            placement_probability_per_valid_position: object
                .get("placement_probability_per_valid_position")
                .and_then(Value::as_f64)
                .ok_or("underwater_magma has no placement_probability_per_valid_position")?
                as f32,
        })
    }
}

/// `state.is(Blocks.WATER)`：按方块注册表 ID 判断，不是流体状态。
///
/// `state.is(Blocks.WATER)`: compares block registry IDs, not the fluid state.
fn is_water_block(state: u32) -> bool {
    block_by_name("minecraft:water")
        .and_then(|block| block_id_of_state(block.first_state))
        .is_some_and(|water| block_id_of_state(state) == Some(water))
}

/// `isWaterOrAir(state)`：`state.is(Blocks.WATER) || state.isAir()`。
///
/// `isWaterOrAir(state)`: `state.is(Blocks.WATER) || state.isAir()`.
fn is_water_or_air(state: u32) -> bool {
    is_water_block(state) || state_facts(state) & facts::AIR != 0
}

/// `Column.scanDirection`：从 start_y 沿 direction 走，直到不是水或走满 searchRange-1 步；
/// 最后那一格不是水才返回它的 y。
///
/// `Column.scanDirection`: walk from start_y along direction until the block is
/// not water or searchRange-1 steps are used; return that y only if it is not water.
fn scan_direction(
    world: &FeatureWorld<'_>,
    x: i32,
    start_y: i32,
    z: i32,
    search_range: i32,
    direction: i32,
) -> Option<i32> {
    let mut y = start_y;
    let mut i = 1;
    while i < search_range && is_water_block(world.get_block(x, y, z)) {
        y += direction;
        i += 1;
    }
    if is_water_block(world.get_block(x, y, z)) {
        None
    } else {
        Some(y)
    }
}

/// `getFloorY` = `Column.scan(...).getFloor()`：向下扫描的 floor。
///
/// `getFloorY` = `Column.scan(...).getFloor()`: the floor of the downward scan.
fn get_floor_y(
    world: &FeatureWorld<'_>,
    origin: (i32, i32, i32),
    search_range: i32,
) -> Option<i32> {
    let (x, y, z) = origin;
    // Column.scan 先确认原点本身是水。
    // Column.scan first checks that the origin itself is water.
    if !is_water_block(world.get_block(x, y, z)) {
        return None;
    }
    let _ = scan_direction(world, x, y, z, search_range, 1);
    scan_direction(world, x, y, z, search_range, -1)
}

/// `isVisibleFromOutside(level, pos, coveredDirection)`：全遮挡的实心立方体返回 false。
///
/// `isVisibleFromOutside`: a fully-occluding solid cube returns false.
pub fn is_visible_from_outside(world: &FeatureWorld<'_>, x: i32, y: i32, z: i32) -> bool {
    let bits = state_facts(world.get_block(x, y, z));
    bits & facts::CAN_OCCLUDE == 0 || bits & facts::SOLID_RENDER == 0
}

/// `isValidPlacement`：pos 不是水/空气，下方是实心，四个水平邻居都是实心。
///
/// `isValidPlacement`: pos is not water/air, below is solid, all four horizontal
/// neighbours are solid.
fn is_valid_placement(world: &FeatureWorld<'_>, x: i32, y: i32, z: i32) -> bool {
    if is_water_or_air(world.get_block(x, y, z)) {
        return false;
    }
    if is_visible_from_outside(world, x, y - 1, z) {
        return false;
    }
    for (dx, dz) in [(0, -1), (0, 1), (-1, 0), (1, 0)] {
        if is_visible_from_outside(world, x + dx, y, z + dz) {
            return false;
        }
    }
    true
}

/// `UnderwaterMagmaFeature.place`。
///
/// `UnderwaterMagmaFeature.place`.
pub fn place(
    feature: &UnderwaterMagmaFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    let (ox, oy, oz) = origin;
    let Some(floor_y) = get_floor_y(world, origin, feature.floor_search_range) else {
        return false;
    };
    let r = feature.placement_radius_around_floor;
    let magma = blocks::MAGMA_BLOCK;
    let mut placed = false;
    // betweenClosedStream(bounds)：x 变化最快，其次 y，z 最慢。
    // betweenClosedStream(bounds): x fastest, then y, z slowest.
    for z in (oz - r)..=(oz + r) {
        for y in (floor_y - r)..=(floor_y + r) {
            for x in (ox - r)..=(ox + r) {
                // 流式的第一个 filter 对每个位置都取一次 nextFloat()。
                // The first stream filter draws one nextFloat() per position.
                if random.next_float() < feature.placement_probability_per_valid_position
                    && is_valid_placement(world, x, y, z)
                {
                    world.set_block_with_heightmaps(x, y, z, magma);
                    placed = true;
                }
            }
        }
    }
    let _ = oy;
    placed
}
