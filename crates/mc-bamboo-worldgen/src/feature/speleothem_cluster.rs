//! `SpeleothemClusterFeature`（滴水石簇 / 硫磺尖簇）。
//!
//! 照 `SpeleothemClusterFeature.place` 与 `placeColumn`：
//! 先在原点用 `Column.scan` 上下找空腔边缘；对半径范围内的每一列，按「离边缘的距离」
//! 算一个长柱概率，再按 `wetness` 决定是否在底部挖一格水面，按 `density` 和高度偏置
//! 决定钟乳石 / 石笋的高度，最后把柱底换成 `base_block` 并铺 `pointed_block`。
//! `Column.withFloor` 会把地板抬高 / 压低一格，`mergeTips` 决定尖端是否是 `tip_merge`。
//!
//! `SpeleothemClusterFeature.place` and `placeColumn`: scan the cavity edges at the origin
//! with `Column.scan`, then for every column in the radius compute a speleothem chance from
//! its distance to the cluster edge, optionally carve a one-block pool from `wetness`, pick
//! stalactite/stalagmite heights from `density` and the height bias, and finally replace the
//! column base with `base_block` and lay `pointed_block`. `Column.withFloor` shifts the
//! floor by one and `mergeTips` selects the `tip_merge` tip.

use mc_bamboo_data::{facts, state_facts, state_fluid, tags};
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    provider::{FloatProvider, IntProvider},
    state::{apply_property, parse_state},
    world::FeatureWorld,
};

/// `SpeleothemUtils.createPointedBlock` 的两个属性名。
///
/// The two property names of `SpeleothemUtils.createPointedBlock`.
const TIP_DIRECTION: &str = "vertical_direction";
const THICKNESS: &str = "thickness";

/// `SpeleothemClusterFeature` 的配置。
#[derive(Debug, Clone)]
pub struct SpeleothemClusterFeature {
    pub base_block: u32,
    pub pointed_block: u32,
    pub replaceable_blocks: Vec<i32>,
    pub floor_to_ceiling_search_range: i32,
    pub height: IntProvider,
    pub radius: IntProvider,
    pub max_stalagmite_stalactite_height_diff: i32,
    pub height_deviation: i32,
    pub speleothem_block_layer_thickness: IntProvider,
    pub density: FloatProvider,
    pub wetness: FloatProvider,
    pub chance_of_speleothem_at_max_distance_from_center: f32,
    pub max_distance_from_edge_affecting_chance_of_speleothem: i32,
    pub max_distance_from_center_affecting_height_bias: i32,
}

impl SpeleothemClusterFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config
            .as_object()
            .ok_or("speleothem_cluster feature must be an object")?;
        let field = |name: &str| -> Result<&Value, String> {
            object
                .get(name)
                .ok_or_else(|| format!("speleothem_cluster has no {name}"))
        };
        Ok(Self {
            base_block: parse_state(field("base_block")?)?,
            pointed_block: parse_state(field("pointed_block")?)?,
            replaceable_blocks: holder_set(field("replaceable_blocks")?)?,
            floor_to_ceiling_search_range: int(field("floor_to_ceiling_search_range")?)?,
            height: IntProvider::parse(field("height")?)?,
            radius: IntProvider::parse(field("radius")?)?,
            max_stalagmite_stalactite_height_diff: int(field(
                "max_stalagmite_stalactite_height_diff",
            )?)?,
            height_deviation: int(field("height_deviation")?)?,
            speleothem_block_layer_thickness: IntProvider::parse(field(
                "speleothem_block_layer_thickness",
            )?)?,
            density: FloatProvider::parse(field("density")?)?,
            wetness: FloatProvider::parse(field("wetness")?)?,
            chance_of_speleothem_at_max_distance_from_center: number(field(
                "chance_of_speleothem_at_max_distance_from_center",
            )?)?,
            max_distance_from_edge_affecting_chance_of_speleothem: int(field(
                "max_distance_from_edge_affecting_chance_of_speleothem",
            )?)?,
            max_distance_from_center_affecting_height_bias: int(field(
                "max_distance_from_center_affecting_height_bias",
            )?)?,
        })
    }

    /// 没有嵌套的未移植配置。
    ///
    /// No unported nested config.
    pub fn supported(&self) -> bool {
        true
    }
}

fn int(value: &Value) -> Result<i32, String> {
    value
        .as_i64()
        .map(|value| value as i32)
        .ok_or_else(|| "expected an int".to_owned())
}

fn number(value: &Value) -> Result<f32, String> {
    value
        .as_f64()
        .map(|value| value as f32)
        .ok_or_else(|| "expected a number".to_owned())
}

/// `HolderSet<Block>`：`#tag`、方块名或数组，解析成方块注册表 ID。
///
/// `HolderSet<Block>`: a `#tag`, a block name, or an array, as block registry IDs.
fn holder_set(value: &Value) -> Result<Vec<i32>, String> {
    let entries: Vec<String> = match value {
        Value::String(name) => vec![name.clone()],
        Value::Array(items) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "block holder entry is not a string".to_owned())
            })
            .collect::<Result<Vec<_>, String>>()?,
        _ => return Err("block holder set is neither a string nor an array".to_owned()),
    };
    let mut ids = Vec::new();
    for entry in entries {
        match entry.strip_prefix('#') {
            Some(tag) => {
                let ids_in_tag =
                    mc_bamboo_data::block_tag(tag).ok_or_else(|| format!("unknown tag {tag}"))?;
                ids.extend_from_slice(ids_in_tag);
            }
            None => {
                let block = mc_bamboo_data::block_by_name(&entry)
                    .ok_or_else(|| format!("unknown block {entry}"))?;
                let id = mc_bamboo_data::block_id_of_state(block.first_state)
                    .ok_or_else(|| format!("unknown block {entry}"))?;
                ids.push(id as i32);
            }
        }
    }
    Ok(ids)
}

fn in_set(state: u32, ids: &[i32]) -> bool {
    mc_bamboo_data::block_id_of_state(state).is_some_and(|id| ids.contains(&(id as i32)))
}

fn is_block(state: u32, name: &str) -> bool {
    mc_bamboo_data::block_of_state(state).is_some_and(|block| block.name == name)
}

/// 两个状态是否属于同一个方块（官方 `state.is(block)` 的语义）。
///
/// Whether two states belong to the same block (the official `state.is(block)` semantics).
fn same_block(state: u32, other: u32) -> bool {
    match (
        mc_bamboo_data::block_id_of_state(state),
        mc_bamboo_data::block_id_of_state(other),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// `SpeleothemUtils.isEmptyOrWater`。
fn is_empty_or_water(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0 || is_block(state, "minecraft:water")
}

/// `SpeleothemUtils.isNeitherEmptyNorWater`。
fn is_neither_empty_nor_water(state: u32) -> bool {
    !is_empty_or_water(state)
}

/// `Mth.clampedMap(value, min, max, outMin, outMax)`（float 版）。
///
/// `Mth.clampedMap(value, min, max, outMin, outMax)` (the float overload).
fn clamped_map(value: f32, min: f32, max: f32, out_min: f32, out_max: f32) -> f32 {
    let t = ((value - min) / (max - min)).clamp(0.0, 1.0);
    out_min + t * (out_max - out_min)
}

/// 双端都有的 `Column.Range`（本轮只用这一种）。
///
/// The `Column.Range` shape with both ends present (the only shape used this round).
#[derive(Debug, Clone, Copy)]
struct Range {
    floor: i32,
    ceiling: i32,
}

impl Range {
    fn height(self) -> i32 {
        self.ceiling - self.floor - 1
    }
}

/// `Column.scan` 的结果；本轮只关心「上下都有边缘」的 `Range`。
///
/// The result of `Column.scan`; this round only needs the `Range` with both edges.
fn column_scan(
    world: &FeatureWorld<'_>,
    pos: (i32, i32, i32),
    search_range: i32,
    inside: impl Fn(u32) -> bool,
    valid_edge: impl Fn(u32) -> bool,
) -> Option<Range> {
    if !inside(world.get_block(pos.0, pos.1, pos.2)) {
        return None;
    }
    let ceiling = scan_direction(world, pos, search_range, &inside, &valid_edge, 1)?;
    let floor = scan_direction(world, pos, search_range, &inside, &valid_edge, -1)?;
    Some(Range { floor, ceiling })
}

fn scan_direction(
    world: &FeatureWorld<'_>,
    pos: (i32, i32, i32),
    search_range: i32,
    inside: &impl Fn(u32) -> bool,
    valid_edge: &impl Fn(u32) -> bool,
    step: i32,
) -> Option<i32> {
    let mut y = pos.1;
    let mut index = 1;
    while index < search_range && inside(world.get_block(pos.0, y, pos.2)) {
        y += step;
        index += 1;
    }
    if valid_edge(world.get_block(pos.0, y, pos.2)) {
        Some(y)
    } else {
        None
    }
}

/// `SpeleothemUtils.placeBaseBlockIfPossible`。
fn place_base_block_if_possible(
    world: &mut FeatureWorld<'_>,
    pos: (i32, i32, i32),
    base: u32,
    replaceable: &[i32],
) -> bool {
    let state = world.get_block(pos.0, pos.1, pos.2);
    if in_set(state, replaceable) {
        world.set_block_with_heightmaps(pos.0, pos.1, pos.2, base);
        true
    } else {
        false
    }
}

/// `SpeleothemUtils.createPointedBlock`。
///
/// `SpeleothemUtils.createPointedBlock`.
fn create_pointed_block(pointed: u32, direction: &str, thickness: &str) -> Option<u32> {
    let state = apply_property(pointed, TIP_DIRECTION, direction)?;
    apply_property(state, THICKNESS, thickness)
}

/// `SpeleothemUtils.buildBaseToTipColumn` + `growSpeleothem`：从 `start` 沿 `direction`
/// 依次写 base / middle / frustum / tip。
///
/// `SpeleothemUtils.buildBaseToTipColumn` + `growSpeleothem`: write base / middle / frustum /
/// tip from `start` along `direction`.
fn grow_speleothem(
    world: &mut FeatureWorld<'_>,
    start: (i32, i32, i32),
    downward: bool,
    total_length: i32,
    merged_tip: bool,
    base: u32,
    pointed: u32,
    replaceable: &[i32],
) {
    let opposite = if downward { 1 } else { -1 };
    let above = (start.0, start.1 + opposite, start.2);
    let base_state = world.get_block(above.0, above.1, above.2);
    // `SpeleothemUtils.isBase(state, baseBlock, replaceableBlocks)`。
    // `SpeleothemUtils.isBase(state, baseBlock, replaceableBlocks)`.
    if !(same_block(base_state, base) || in_set(base_state, replaceable)) {
        return;
    }
    let direction = if downward { "down" } else { "up" };
    let mut pos = start;
    let step = if downward { -1 } else { 1 };
    let mut emit = |thickness: &str, world: &mut FeatureWorld<'_>| {
        if let Some(mut state) = create_pointed_block(pointed, direction, thickness) {
            // `PointedDripstoneBlock.WATERLOGGED` 按该格的 `isWaterAt` 设置。
            // `PointedDripstoneBlock.WATERLOGGED` follows the cell's `isWaterAt`.
            let waterlogged = state_fluid(world.get_block(pos.0, pos.1, pos.2)).fluid
                != mc_bamboo_data::registries::fluid::EMPTY;
            state = apply_property(
                state,
                "waterlogged",
                if waterlogged { "true" } else { "false" },
            )
            .unwrap_or(state);
            world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
        }
        pos.1 += step;
    };
    if total_length >= 3 {
        emit("base", world);
        for _ in 0..total_length - 3 {
            emit("middle", world);
        }
    }
    if total_length >= 2 {
        emit("frustum", world);
    }
    if total_length >= 1 {
        emit(if merged_tip { "tip_merge" } else { "tip" }, world);
    }
    let _ = base;
}

impl SpeleothemClusterFeature {
    /// `SpeleothemClusterFeature.place`。
    ///
    /// `SpeleothemClusterFeature.place`.
    pub fn place(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin: (i32, i32, i32),
    ) -> bool {
        if !is_empty_or_water(world.get_block(origin.0, origin.1, origin.2)) {
            return false;
        }
        // 取样顺序照官方：height、wetness、density、radius ×2。
        // Draw order per the official code: height, wetness, density, then radius twice.
        let height = self.height.sample(random);
        let wetness = self.wetness.sample(random);
        let density = self.density.sample(random);
        let x_radius = self.radius.sample(random);
        let z_radius = self.radius.sample(random);

        for dx in -x_radius..=x_radius {
            for dz in -z_radius..=z_radius {
                let chance = self.chance_of_stalagmite_or_stalactite(x_radius, z_radius, dx, dz);
                self.place_column(
                    world,
                    random,
                    (origin.0 + dx, origin.1, origin.2 + dz),
                    dx,
                    dz,
                    wetness,
                    chance,
                    height,
                    density,
                );
            }
        }
        true
    }

    /// `placeColumn`。
    ///
    /// `placeColumn`.
    #[allow(clippy::too_many_arguments)]
    fn place_column(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        pos: (i32, i32, i32),
        dx: i32,
        dz: i32,
        chance_of_water: f32,
        chance_of_speleothem: f64,
        cluster_height: i32,
        density: f32,
    ) {
        let Some(base_column) = column_scan(
            world,
            pos,
            self.floor_to_ceiling_search_range,
            is_empty_or_water,
            is_neither_empty_nor_water,
        ) else {
            return;
        };
        let ceiling = Some(base_column.ceiling);
        let base_floor = Some(base_column.floor);
        let want_pool = random.next_float() < chance_of_water;
        let mut column = base_column;
        if want_pool && self.can_place_pool(world, (pos.0, base_floor.unwrap(), pos.2)) {
            let base_floor_y = base_floor.unwrap();
            column.floor = base_floor_y - 1;
            world.set_block_with_heightmaps(
                pos.0,
                base_floor_y,
                pos.2,
                mc_bamboo_data::blocks::WATER,
            );
        }
        let floor = Some(column.floor);
        let want_stalactite = random.next_double() < chance_of_speleothem;
        let stalactite_height = if ceiling.is_some()
            && want_stalactite
            && !self.is_lava(world, (pos.0, ceiling.unwrap(), pos.2))
        {
            let thickness = self.speleothem_block_layer_thickness.sample(random);
            self.replace_blocks_with_base_blocks(
                world,
                (pos.0, ceiling.unwrap(), pos.2),
                thickness,
                1,
            );
            let max_height = if floor.is_some() {
                cluster_height.min(ceiling.unwrap() - floor.unwrap())
            } else {
                cluster_height
            };
            self.get_speleothem_height(random, dx, dz, density, max_height)
        } else {
            0
        };

        let want_stalagmite = random.next_double() < chance_of_speleothem;
        let stalagmite_height = if floor.is_some()
            && want_stalagmite
            && !self.is_lava(world, (pos.0, floor.unwrap(), pos.2))
        {
            let thickness = self.speleothem_block_layer_thickness.sample(random);
            self.replace_blocks_with_base_blocks(
                world,
                (pos.0, floor.unwrap(), pos.2),
                thickness,
                -1,
            );
            if ceiling.is_some() {
                (stalactite_height
                    + random.next_int_between_inclusive(
                        -self.max_stalagmite_stalactite_height_diff,
                        self.max_stalagmite_stalactite_height_diff,
                    ))
                .max(0)
            } else {
                self.get_speleothem_height(random, dx, dz, density, cluster_height)
            }
        } else {
            0
        };

        let (actual_stalactite_height, actual_stalagmite_height) = if ceiling.is_some()
            && floor.is_some()
            && ceiling.unwrap() - stalactite_height <= floor.unwrap() + stalagmite_height
        {
            let floor_y = floor.unwrap();
            let ceiling_y = ceiling.unwrap();
            let lowest_bottom = (ceiling_y - stalactite_height).max(floor_y + 1);
            let highest_top = (floor_y + stalagmite_height).min(ceiling_y - 1);
            let actual_bottom = random.next_int_between_inclusive(lowest_bottom, highest_top + 1);
            let actual_top = actual_bottom - 1;
            (ceiling_y - actual_bottom, actual_top - floor_y)
        } else {
            (stalactite_height, stalagmite_height)
        };

        let merge_tips = random.next_boolean()
            && actual_stalactite_height > 0
            && actual_stalagmite_height > 0
            && actual_stalactite_height + actual_stalagmite_height == column.height();
        if let Some(ceiling_y) = ceiling {
            grow_speleothem(
                world,
                (pos.0, ceiling_y - 1, pos.2),
                true,
                actual_stalactite_height,
                merge_tips,
                self.base_block,
                self.pointed_block,
                &self.replaceable_blocks,
            );
        }
        if let Some(floor_y) = floor {
            grow_speleothem(
                world,
                (pos.0, floor_y + 1, pos.2),
                false,
                actual_stalagmite_height,
                merge_tips,
                self.base_block,
                self.pointed_block,
                &self.replaceable_blocks,
            );
        }
    }

    fn is_lava(&self, world: &FeatureWorld<'_>, pos: (i32, i32, i32)) -> bool {
        is_block(world.get_block(pos.0, pos.1, pos.2), "minecraft:lava")
    }

    /// `getSpeleothemHeight`。
    ///
    /// `getSpeleothemHeight`.
    fn get_speleothem_height(
        &self,
        random: &WorldgenRandom,
        dx: i32,
        dz: i32,
        density: f32,
        max_height: i32,
    ) -> i32 {
        if random.next_float() > density {
            return 0;
        }
        let distance_from_center = dx.abs() + dz.abs();
        let height_mean = clamped_map(
            distance_from_center as f32,
            0.0,
            self.max_distance_from_center_affecting_height_bias as f32,
            max_height as f32 / 2.0,
            0.0,
        );
        // `randomBetweenBiased` = `ClampedNormalFloat.sample(random, mean, deviation, min, max)`。
        // `randomBetweenBiased` = `ClampedNormalFloat.sample(random, mean, deviation, min, max)`.
        let value = height_mean + random.next_gaussian() as f32 * self.height_deviation as f32;
        value.clamp(0.0, max_height as f32) as i32
    }

    /// `canPlacePool`。
    ///
    /// `canPlacePool`.
    fn can_place_pool(&self, world: &FeatureWorld<'_>, pos: (i32, i32, i32)) -> bool {
        let state = world.get_block(pos.0, pos.1, pos.2);
        // `state.is(Blocks.WATER) || state.is(baseBlock.getBlock()) || state.is(pointedBlock.getBlock())`
        // —— 官方按**方块**比，不是按状态比。
        // The official checks compare blocks, not states.
        if is_block(state, "minecraft:water")
            || same_block(state, self.base_block)
            || same_block(state, self.pointed_block)
        {
            return false;
        }
        if state_fluid(world.get_block(pos.0, pos.1 + 1, pos.2)).fluid
            == mc_bamboo_data::registries::fluid::WATER
        {
            return false;
        }
        for direction in [(0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)] {
            let neighbor = (pos.0 + direction.0, pos.1, pos.2 + direction.2);
            if !self.can_be_adjacent_to_water(world, neighbor) {
                return false;
            }
        }
        self.can_be_adjacent_to_water(world, (pos.0, pos.1 - 1, pos.2))
    }

    /// `canBeAdjacentToWater`。
    ///
    /// `canBeAdjacentToWater`.
    fn can_be_adjacent_to_water(&self, world: &FeatureWorld<'_>, pos: (i32, i32, i32)) -> bool {
        let state = world.get_block(pos.0, pos.1, pos.2);
        in_set(
            state,
            mc_bamboo_data::block_tag(tags::block_tags::BASE_STONE_OVERWORLD.name).unwrap_or(&[]),
        ) || state_fluid(state).fluid == mc_bamboo_data::registries::fluid::WATER
    }

    /// `replaceBlocksWithBaseBlocks`。
    ///
    /// `replaceBlocksWithBaseBlocks`.
    fn replace_blocks_with_base_blocks(
        &self,
        world: &mut FeatureWorld<'_>,
        first: (i32, i32, i32),
        max_count: i32,
        step: i32,
    ) {
        let mut pos = first;
        for _ in 0..max_count {
            if !place_base_block_if_possible(world, pos, self.base_block, &self.replaceable_blocks)
            {
                return;
            }
            pos.1 += step;
        }
    }

    /// `getChanceOfStalagmiteOrStalactite`。
    ///
    /// `getChanceOfStalagmiteOrStalactite`.
    fn chance_of_stalagmite_or_stalactite(
        &self,
        x_radius: i32,
        z_radius: i32,
        dx: i32,
        dz: i32,
    ) -> f64 {
        let x_distance_from_edge = x_radius - dx.abs();
        let z_distance_from_edge = z_radius - dz.abs();
        let distance_from_edge = x_distance_from_edge.min(z_distance_from_edge);
        f64::from(clamped_map(
            distance_from_edge as f32,
            0.0,
            self.max_distance_from_edge_affecting_chance_of_speleothem as f32,
            self.chance_of_speleothem_at_max_distance_from_center,
            1.0,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_overworld_clusters() {
        let root: Value =
            serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).expect("worldgen json");
        let features = root
            .get("feature")
            .and_then(Value::as_object)
            .expect("feature registry");
        let mut parsed = 0;
        for name in [
            "minecraft:dripstone_cluster",
            "minecraft:sulfur_spike_cluster",
        ] {
            let Some(config) = features.get(name) else {
                continue;
            };
            let feature = SpeleothemClusterFeature::parse(config)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(feature.supported(), "{name}");
            parsed += 1;
        }
        assert!(parsed > 0, "no cluster config found");
    }

    #[test]
    fn clamped_map_matches_the_linear_interpolation() {
        assert_eq!(clamped_map(0.0, 0.0, 8.0, 4.0, 0.0), 4.0);
        assert_eq!(clamped_map(8.0, 0.0, 8.0, 4.0, 0.0), 0.0);
        assert_eq!(clamped_map(4.0, 0.0, 8.0, 4.0, 0.0), 2.0);
        // 超出范围时夹在端点。
        // Out of range clamps to the endpoints.
        assert_eq!(clamped_map(-1.0, 0.0, 8.0, 4.0, 0.0), 4.0);
        assert_eq!(clamped_map(99.0, 0.0, 8.0, 4.0, 0.0), 0.0);
    }
}
