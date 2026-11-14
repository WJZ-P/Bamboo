//! `SpeleothemClusterFeature`（`minecraft:speleothem_cluster`）：`dripstone_cluster` 和
//! `sulfur_spike_cluster` 的洞穴簇。
//!
//! `place` 先取 height/wetness/density/xRadius/zRadius，再逐列 `placeColumn`：
//! Column.scan、可选水池、钟乳/石笋高度（clamped_normal）、合并判定、growSpeleothem。
//! 随机消费顺序（含所有短路分支）必须逐位照官方。
//!
//! `SpeleothemClusterFeature` (`minecraft:speleothem_cluster`): the `dripstone_cluster`
//! and `sulfur_spike_cluster` cave clusters. `place` draws height/wetness/density/xRadius/
//! zRadius, then runs `placeColumn` per column: Column.scan, optional pool, stalactite /
//! stalagmite heights (clamped_normal), merge decision, growSpeleothem. The random draw
//! order, including every short-circuit branch, must match the official bit for bit.

use mc_bamboo_data::blocks;
use serde_json::Value;

use crate::{math, random::WorldgenRandom};

use super::{
    dripstone_utils::{
        DIRECTION_OFFSETS, FloatProvider, HORIZONTAL, Materials, clamped_map_f32, clamped_map_f64,
        fluid_is_water, grow_speleothem, in_block_tag, is_empty_or_water,
        is_neither_empty_nor_water, replace_blocks_with_base_blocks, same_block, scan_column,
    },
    provider::IntProvider,
    state::parse_state,
    world::FeatureWorld,
};

#[derive(Debug, Clone)]
pub struct SpeleothemClusterFeature {
    base_block: u32,
    pointed_default: u32,
    replaceable: String,
    search_range: i32,
    height: IntProvider,
    radius: IntProvider,
    max_height_diff: i32,
    height_deviation: i32,
    layer_thickness: IntProvider,
    density: FloatProvider,
    wetness: FloatProvider,
    chance_at_max_distance: f32,
    max_distance_edge: i32,
    max_distance_center: i32,
}

impl SpeleothemClusterFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config
            .as_object()
            .ok_or("speleothem_cluster must be an object")?;
        let int = |name: &str| -> Result<i32, String> {
            object
                .get(name)
                .and_then(Value::as_i64)
                .map(|v| v as i32)
                .ok_or_else(|| format!("speleothem_cluster has no {name}"))
        };
        let float = |name: &str| -> Result<f32, String> {
            object
                .get(name)
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .ok_or_else(|| format!("speleothem_cluster has no {name}"))
        };
        Ok(Self {
            base_block: parse_state(object.get("base_block").ok_or("no base_block")?)?,
            pointed_default: parse_state(object.get("pointed_block").ok_or("no pointed_block")?)?,
            replaceable: object
                .get("replaceable_blocks")
                .and_then(Value::as_str)
                .ok_or("no replaceable_blocks")?
                .trim_start_matches('#')
                .to_owned(),
            search_range: int("floor_to_ceiling_search_range")?,
            height: IntProvider::parse(object.get("height").ok_or("no height")?)?,
            radius: IntProvider::parse(object.get("radius").ok_or("no radius")?)?,
            max_height_diff: int("max_stalagmite_stalactite_height_diff")?,
            height_deviation: int("height_deviation")?,
            layer_thickness: IntProvider::parse(
                object
                    .get("speleothem_block_layer_thickness")
                    .ok_or("no layer_thickness")?,
            )?,
            density: FloatProvider::parse(object.get("density").ok_or("no density")?)?,
            wetness: FloatProvider::parse(object.get("wetness").ok_or("no wetness")?)?,
            chance_at_max_distance: float("chance_of_speleothem_at_max_distance_from_center")?,
            max_distance_edge: int("max_distance_from_edge_affecting_chance_of_speleothem")?,
            max_distance_center: int("max_distance_from_center_affecting_height_bias")?,
        })
    }
}

/// `getChanceOfStalagmiteOrStalactite`：float 版 `clampedMap`，结果拓宽成 double。
/// `getChanceOfStalagmiteOrStalactite`: float `clampedMap`, widened to double.
fn chance_of_speleothem(
    feature: &SpeleothemClusterFeature,
    x_radius: i32,
    z_radius: i32,
    dx: i32,
    dz: i32,
) -> f64 {
    let x_distance = x_radius - dx.abs();
    let z_distance = z_radius - dz.abs();
    let distance = x_distance.min(z_distance);
    f64::from(clamped_map_f32(
        distance as f32,
        0.0,
        feature.max_distance_edge as f32,
        feature.chance_at_max_distance,
        1.0,
    ))
}

fn can_be_adjacent_to_water(world: &FeatureWorld<'_>, pos: (i32, i32, i32)) -> bool {
    let state = world.get_block(pos.0, pos.1, pos.2);
    in_block_tag(state, "minecraft:base_stone_overworld") || fluid_is_water(state)
}

/// `canPlacePool`。
/// `canPlacePool`.
fn can_place_pool(
    world: &FeatureWorld<'_>,
    feature: &SpeleothemClusterFeature,
    pos: (i32, i32, i32),
) -> bool {
    let (x, y, z) = pos;
    let state = world.get_block(x, y, z);
    if same_block(state, blocks::WATER)
        || same_block(state, feature.base_block)
        || same_block(state, feature.pointed_default)
    {
        return false;
    }
    if fluid_is_water(world.get_block(x, y + 1, z)) {
        return false;
    }
    for direction in HORIZONTAL {
        let (dx, dy, dz) = DIRECTION_OFFSETS[direction];
        if !can_be_adjacent_to_water(world, (x + dx, y + dy, z + dz)) {
            return false;
        }
    }
    can_be_adjacent_to_water(world, (x, y - 1, z))
}

fn is_lava(world: &FeatureWorld<'_>, pos: (i32, i32, i32)) -> bool {
    same_block(world.get_block(pos.0, pos.1, pos.2), blocks::LAVA)
}

/// `getSpeleothemHeight`：先 `nextFloat() > density` 短路，再 clampedMap(double)，
/// 最后 clamped_normal 取整。
/// `getSpeleothemHeight`: short-circuit on `nextFloat() > density`, then double
/// `clampedMap`, then a clamped-normal sample cast to int.
fn speleothem_height(
    feature: &SpeleothemClusterFeature,
    random: &WorldgenRandom,
    dx: i32,
    dz: i32,
    density: f32,
    max_height: i32,
) -> i32 {
    if random.next_float() > density {
        return 0;
    }
    let distance = dx.abs() + dz.abs();
    let height_mean = clamped_map_f64(
        distance as f64,
        0.0,
        feature.max_distance_center as f64,
        max_height as f64 / 2.0,
        0.0,
    ) as f32;
    let normal = height_mean + (random.next_gaussian() as f32) * (feature.height_deviation as f32);
    math::clamp(normal, 0.0_f32, max_height as f32) as i32
}

/// `placeColumn` 的每列参数，单独打包避免一次传太多参数。
/// Per-column parameters for `placeColumn`, bundled to keep the argument list short.
#[derive(Debug, Clone, Copy)]
struct ColumnParams {
    dx: i32,
    dz: i32,
    wetness: f32,
    chance: f64,
    cluster_height: i32,
    density: f32,
}

/// `SpeleothemClusterFeature.placeColumn`。
/// `SpeleothemClusterFeature.placeColumn`.
fn place_column(
    feature: &SpeleothemClusterFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    pos: (i32, i32, i32),
    params: ColumnParams,
) {
    let (x, _y, z) = pos;
    let ColumnParams {
        dx,
        dz,
        wetness,
        chance,
        cluster_height,
        density,
    } = params;
    let Some(base_column) = scan_column(
        world,
        pos,
        feature.search_range,
        is_empty_or_water,
        is_neither_empty_nor_water,
    ) else {
        return;
    };
    let ceiling = base_column.ceiling;
    let base_floor = base_column.floor;
    if ceiling.is_none() && base_floor.is_none() {
        return;
    }

    let want_pool = random.next_float() < wetness;
    let mut column = base_column;
    if want_pool
        && let Some(floor_y) = base_floor
        && can_place_pool(world, feature, (x, floor_y, z))
    {
        column = column.with_floor(floor_y - 1);
        world.set_block_state(x, floor_y, z, blocks::WATER);
    }
    let floor = column.floor;

    let want_stalactite = random.next_double() < chance;
    let stalactite_height = if let Some(ceiling_y) = ceiling {
        if want_stalactite && !is_lava(world, (x, ceiling_y, z)) {
            let thickness = feature.layer_thickness.sample(random);
            replace_blocks_with_base_blocks(
                world,
                (x, ceiling_y, z),
                thickness,
                1,
                feature.base_block,
                &feature.replaceable,
            );
            let max_height = match floor {
                Some(floor_y) => cluster_height.min(ceiling_y - floor_y),
                None => cluster_height,
            };
            speleothem_height(feature, random, dx, dz, density, max_height)
        } else {
            0
        }
    } else {
        0
    };

    let want_stalagmite = random.next_double() < chance;
    let stalagmite_height = if let Some(floor_y) = floor {
        if want_stalagmite && !is_lava(world, (x, floor_y, z)) {
            let thickness = feature.layer_thickness.sample(random);
            replace_blocks_with_base_blocks(
                world,
                (x, floor_y, z),
                thickness,
                -1,
                feature.base_block,
                &feature.replaceable,
            );
            if ceiling.is_some() {
                let diff = random
                    .next_int_between_inclusive(-feature.max_height_diff, feature.max_height_diff);
                (stalactite_height + diff).max(0)
            } else {
                speleothem_height(feature, random, dx, dz, density, cluster_height)
            }
        } else {
            0
        }
    } else {
        0
    };

    let (actual_stalactite, actual_stalagmite) =
        if let (Some(ceiling_y), Some(floor_y)) = (ceiling, floor) {
            if ceiling_y - stalactite_height <= floor_y + stalagmite_height {
                let lowest = (ceiling_y - stalactite_height).max(floor_y + 1);
                let highest = (floor_y + stalagmite_height).min(ceiling_y - 1);
                let bottom = random.next_int_between_inclusive(lowest, highest + 1);
                (ceiling_y - bottom, bottom - 1 - floor_y)
            } else {
                (stalactite_height, stalagmite_height)
            }
        } else {
            (stalactite_height, stalagmite_height)
        };

    // `nextBoolean()` 总会被消费，后面几项只是纯比较。
    // `nextBoolean()` is always drawn; the rest are pure comparisons.
    let merge_tips = random.next_boolean()
        && actual_stalactite > 0
        && actual_stalagmite > 0
        && column
            .height()
            .is_some_and(|h| actual_stalactite + actual_stalagmite == h);

    let mats = Materials {
        base_block: feature.base_block,
        pointed_default: feature.pointed_default,
        replaceable: &feature.replaceable,
    };
    if let Some(ceiling_y) = ceiling {
        grow_speleothem(
            world,
            (x, ceiling_y - 1, z),
            false,
            actual_stalactite,
            merge_tips,
            mats,
        );
    }
    if let Some(floor_y) = floor {
        grow_speleothem(
            world,
            (x, floor_y + 1, z),
            true,
            actual_stalagmite,
            merge_tips,
            mats,
        );
    }
}

/// `SpeleothemClusterFeature.place`。
/// `SpeleothemClusterFeature.place`.
pub fn place(
    feature: &SpeleothemClusterFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    if !is_empty_or_water(world.get_block(origin.0, origin.1, origin.2)) {
        return false;
    }
    let height = feature.height.sample(random);
    let wetness = feature.wetness.sample(random);
    let density = feature.density.sample(random);
    let x_radius = feature.radius.sample(random);
    let z_radius = feature.radius.sample(random);

    for dx in -x_radius..=x_radius {
        for dz in -z_radius..=z_radius {
            let params = ColumnParams {
                dx,
                dz,
                wetness,
                chance: chance_of_speleothem(feature, x_radius, z_radius, dx, dz),
                cluster_height: height,
                density,
            };
            let pos = (origin.0 + dx, origin.1, origin.2 + dz);
            place_column(feature, world, random, pos, params);
        }
    }
    true
}
