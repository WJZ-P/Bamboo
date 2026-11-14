//! `LargeDripstoneFeature`（`minecraft:large_dripstone`）：洞穴里的大型钟乳石/石笋。
//!
//! 逐段照 `LargeDripstoneFeature.java`：`Column.scan` 定洞穴上下沿、随机半径与
//! bluntness/scale、可选的风偏移、`moveBackUntilBase...` 收缩半径，最后 `placeBlocks`。
//!
//! `LargeDripstoneFeature` (`minecraft:large_dripstone`): the large stalactites /
//! stalagmites in caves. Follows `LargeDripstoneFeature.java` step by step: `Column.scan`
//! bounds the cave, random radius / bluntness / scale, optional wind offset, radius
//! shrinking via `moveBackUntilBase...`, then `placeBlocks`.

use mc_bamboo_data::blocks;
use serde_json::Value;

use crate::{math, random::WorldgenRandom};

use super::{
    dripstone_utils::{
        FloatProvider, clamp_i32, get_speleothem_height, in_block_tag, is_base_or_lava,
        is_circle_mostly_embedded_in_stone, is_empty_or_water, is_empty_or_water_or_lava, mth_cos,
        mth_sin, scan_column,
    },
    world::{FeatureWorld, HeightmapKind},
};

/// `column_radius` 是 `clamped`，本轮只需要它的 min/max 边界（官方 place 里也只用这两个）。
/// `column_radius` is `clamped`; the official `place` only uses its min/max bounds.
#[derive(Debug, Clone, Copy)]
struct ClampedBounds {
    min_inclusive: i32,
    max_inclusive: i32,
}

#[derive(Debug, Clone)]
pub struct LargeDripstoneFeature {
    replaceable: String,
    search_range: i32,
    column_radius: ClampedBounds,
    height_scale: FloatProvider,
    ratio: f32,
    stalactite_bluntness: FloatProvider,
    stalagmite_bluntness: FloatProvider,
    wind_speed: FloatProvider,
    min_radius_for_wind: i32,
    min_bluntness_for_wind: f32,
}

impl LargeDripstoneFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config
            .as_object()
            .ok_or("large_dripstone must be an object")?;
        let tag = object
            .get("replaceable_blocks")
            .and_then(Value::as_str)
            .ok_or("large_dripstone has no replaceable_blocks")?
            .trim_start_matches('#')
            .to_owned();
        let radius = object
            .get("column_radius")
            .and_then(Value::as_object)
            .ok_or("large_dripstone has no column_radius")?;
        let int = |name: &str| -> Result<i32, String> {
            radius
                .get(name)
                .and_then(Value::as_i64)
                .map(|v| v as i32)
                .ok_or_else(|| format!("column_radius has no {name}"))
        };
        let float = |name: &str| -> Result<f32, String> {
            object
                .get(name)
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .ok_or_else(|| format!("large_dripstone has no {name}"))
        };
        Ok(Self {
            replaceable: tag,
            search_range: object
                .get("floor_to_ceiling_search_range")
                .and_then(Value::as_i64)
                .map(|v| v as i32)
                .unwrap_or(30),
            column_radius: ClampedBounds {
                min_inclusive: int("min_inclusive")?,
                max_inclusive: int("max_inclusive")?,
            },
            height_scale: FloatProvider::parse(
                object
                    .get("height_scale")
                    .ok_or("large_dripstone has no height_scale")?,
            )?,
            ratio: float("max_column_radius_to_cave_height_ratio")?,
            stalactite_bluntness: FloatProvider::parse(
                object
                    .get("stalactite_bluntness")
                    .ok_or("no stalactite_bluntness")?,
            )?,
            stalagmite_bluntness: FloatProvider::parse(
                object
                    .get("stalagmite_bluntness")
                    .ok_or("no stalagmite_bluntness")?,
            )?,
            wind_speed: FloatProvider::parse(
                object
                    .get("wind_speed")
                    .ok_or("large_dripstone has no wind_speed")?,
            )?,
            min_radius_for_wind: object
                .get("min_radius_for_wind")
                .and_then(Value::as_i64)
                .map(|v| v as i32)
                .ok_or("large_dripstone has no min_radius_for_wind")?,
            min_bluntness_for_wind: float("min_bluntness_for_wind")?,
        })
    }
}

/// `LargeDripstoneFeature.WindOffsetter`：风向量按 `originY - y` 缩放后取整偏移。
/// `LargeDripstoneFeature.WindOffsetter`: the wind vector scales by `originY - y`.
#[derive(Debug, Clone, Copy)]
struct Wind {
    active: bool,
    origin_y: i32,
    wx: f64,
    wz: f64,
    max_offset: i32,
}

impl Wind {
    fn new(
        origin_y: i32,
        random: &WorldgenRandom,
        wind_speed: FloatProvider,
        max_offset: i32,
    ) -> Self {
        let speed = wind_speed.sample(random);
        let direction = random.next_float() * std::f32::consts::PI;
        // Vec3 的三个分量是 double；这里先按 float 乘 speed，再拓宽成 double。
        // Vec3 components are double; multiply by speed in float first, then widen.
        Self {
            active: true,
            origin_y,
            wx: f64::from(mth_cos(f64::from(direction)) * speed),
            wz: f64::from(mth_sin(f64::from(direction)) * speed),
            max_offset,
        }
    }

    fn no_wind() -> Self {
        Self {
            active: false,
            origin_y: 0,
            wx: 0.0,
            wz: 0.0,
            max_offset: 0,
        }
    }

    fn offset(&self, pos: (i32, i32, i32)) -> (i32, i32, i32) {
        if !self.active {
            return pos;
        }
        let dy = f64::from(self.origin_y - pos.1);
        let dx = clamp_i32(math::floor(self.wx * dy), -self.max_offset, self.max_offset);
        let dz = clamp_i32(math::floor(self.wz * dy), -self.max_offset, self.max_offset);
        (pos.0 + dx, pos.1, pos.2 + dz)
    }
}

/// `LargeDripstoneFeature.LargeDripstone`：`root` 和 `radius` 会被 `moveBack` 改。
/// `LargeDripstoneFeature.LargeDripstone`: `root` / `radius` are mutated by `moveBack`.
#[derive(Debug, Clone, Copy)]
struct Dripstone {
    root: (i32, i32, i32),
    pointing_up: bool,
    radius: i32,
    bluntness: f64,
    scale: f64,
}

impl Dripstone {
    fn new(
        root: (i32, i32, i32),
        pointing_up: bool,
        random: &WorldgenRandom,
        radius: i32,
        bluntness: FloatProvider,
        scale: FloatProvider,
    ) -> Self {
        Self {
            root,
            pointing_up,
            radius,
            bluntness: f64::from(bluntness.sample(random)),
            scale: f64::from(scale.sample(random)),
        }
    }

    fn height_at(&self, check_radius: f64) -> i32 {
        get_speleothem_height(
            check_radius,
            f64::from(self.radius),
            self.scale,
            self.bluntness,
        ) as i32
    }

    fn suitable_for_wind(&self, min_radius: i32, min_bluntness: f32) -> bool {
        self.radius >= min_radius && self.bluntness >= f64::from(min_bluntness)
    }

    fn move_back(&mut self, world: &FeatureWorld<'_>, wind: &Wind) -> bool {
        while self.radius > 1 {
            let mut new_root = self.root;
            let max_tries = 10.min(self.height_at(0.0));
            for _ in 0..max_tries {
                if same_lava(world, new_root) {
                    return false;
                }
                if is_circle_mostly_embedded_in_stone(world, wind.offset(new_root), self.radius) {
                    self.root = new_root;
                    return true;
                }
                new_root.1 += if self.pointing_up { -1 } else { 1 };
            }
            self.radius /= 2;
        }
        false
    }

    fn place_blocks(&self, world: &mut FeatureWorld<'_>, random: &WorldgenRandom, wind: &Wind) {
        for dx in -self.radius..=self.radius {
            for dz in -self.radius..=self.radius {
                let current_radius = math::sqrt_f32((dx * dx + dz * dz) as f32);
                if current_radius > self.radius as f32 {
                    continue;
                }
                let mut height = self.height_at(f64::from(current_radius));
                if height > 0 {
                    // `0.2` 是 double 字面量：nextFloat 提升成 double 再比。
                    // `0.2` is a double literal: nextFloat is promoted to double.
                    if f64::from(random.next_float()) < 0.2_f64 {
                        // `Mth.randomBetween(random, 0.8F, 1.0F)`：差值按 float 算，不能写成 0.2。
                        // `Mth.randomBetween(random, 0.8F, 1.0F)`: the difference is a float
                        // subtraction, not the literal 0.2.
                        let factor = random.next_float() * (1.0_f32 - 0.8_f32) + 0.8_f32;
                        height = ((height as f32) * factor) as i32;
                    }
                    let mut pos = (self.root.0 + dx, self.root.1, self.root.2 + dz);
                    let max_y = if self.pointing_up {
                        world.get_height(HeightmapKind::WorldSurfaceWg, pos.0, pos.2)
                    } else {
                        i32::MAX
                    };
                    let mut has_been_out_of_stone = false;
                    let mut i = 0;
                    while i < height && pos.1 < max_y {
                        let wa = wind.offset(pos);
                        let state = world.get_block(wa.0, wa.1, wa.2);
                        if is_empty_or_water_or_lava(state) {
                            has_been_out_of_stone = true;
                            world.set_block_state(wa.0, wa.1, wa.2, blocks::DRIPSTONE_BLOCK);
                        } else if has_been_out_of_stone
                            && in_block_tag(state, "minecraft:base_stone_overworld")
                        {
                            break;
                        }
                        pos.1 += if self.pointing_up { 1 } else { -1 };
                        i += 1;
                    }
                }
            }
        }
    }
}

fn same_lava(world: &FeatureWorld<'_>, pos: (i32, i32, i32)) -> bool {
    super::dripstone_utils::same_block(world.get_block(pos.0, pos.1, pos.2), blocks::LAVA)
}

/// `LargeDripstoneFeature.place`。
/// `LargeDripstoneFeature.place`.
pub fn place(
    feature: &LargeDripstoneFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    if !is_empty_or_water(world.get_block(origin.0, origin.1, origin.2)) {
        return false;
    }
    let base = blocks::DRIPSTONE_BLOCK;
    let replaceable = feature.replaceable.as_str();
    let Some(column) = scan_column(
        world,
        origin,
        feature.search_range,
        is_empty_or_water,
        |state| is_base_or_lava(state, base, replaceable),
    ) else {
        return false;
    };
    if !column.is_range() {
        return false;
    }
    let Some(column_height) = column.height() else {
        return false;
    };
    if column_height < 4 {
        return false;
    }

    let max_column_radius = clamp_i32(
        (column_height as f32 * feature.ratio) as i32,
        feature.column_radius.min_inclusive,
        feature.column_radius.max_inclusive,
    );
    let radius =
        random.next_int_between_inclusive(feature.column_radius.min_inclusive, max_column_radius);

    let stalactite = Dripstone::new(
        (origin.0, column.ceiling.unwrap() - 1, origin.2),
        false,
        random,
        radius,
        feature.stalactite_bluntness,
        feature.height_scale,
    );
    let stalagmite = Dripstone::new(
        (origin.0, column.floor.unwrap() + 1, origin.2),
        true,
        random,
        radius,
        feature.stalagmite_bluntness,
        feature.height_scale,
    );

    let wind = if stalactite
        .suitable_for_wind(feature.min_radius_for_wind, feature.min_bluntness_for_wind)
        && stalagmite.suitable_for_wind(feature.min_radius_for_wind, feature.min_bluntness_for_wind)
    {
        Wind::new(origin.1, random, feature.wind_speed, 16 - radius)
    } else {
        Wind::no_wind()
    };

    let mut stalactite = stalactite;
    let mut stalagmite = stalagmite;
    let stalactite_base = stalactite.move_back(world, &wind);
    let stalagmite_base = stalagmite.move_back(world, &wind);
    if stalactite_base {
        stalactite.place_blocks(world, random, &wind);
    }
    if stalagmite_base {
        stalagmite.place_blocks(world, random, &wind);
    }
    true
}
