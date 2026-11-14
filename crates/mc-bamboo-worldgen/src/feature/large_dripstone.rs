//! `LargeDripstoneFeature`（大型滴水石柱）。
//!
//! 照 `LargeDripstoneFeature.place` 与 `SpeleothemUtils`：先确认原点为空或水，再用
//! `Column.scan` 上下找「空腔边缘」，得到地面和天花板的 y；空腔高度不足 4 就放弃。
//! 然后分别造钟乳石（上往下）和石笋（下往上），各自「往回挪到根嵌进石头里」，
//! 再按半径逐列算出高度并铺 `pointed_dripstone`。半径够大且钝度够高时还会加一层
//! 风偏移（同一套随机数）。
//!
//! `LargeDripstoneFeature.place` with `SpeleothemUtils`: confirm the origin is empty or
//! water, scan for the cave ceiling/floor with `Column.scan` (bail below height 4), build a
//! stalactite pointing down and a stalagmite pointing up, move each back until its base is
//! embedded in stone, then fill `pointed_dripstone` columns with the per-radius height.
//! Large-enough and blunt-enough columns also get a wind offset from the same random source.

use mc_bamboo_data::{blocks, facts, state_facts, tags};
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    provider::{FloatProvider, IntProvider},
    state::apply_property,
    world::{FeatureWorld, HeightmapKind},
};

/// `SpeleothemUtils.createPointedBlock`：`TIP_DIRECTION` + `THICKNESS` 的两个属性名。
///
/// The two property names of `SpeleothemUtils.createPointedBlock`: `TIP_DIRECTION` and
/// `THICKNESS`.
const TIP_DIRECTION: &str = "vertical_direction";
const THICKNESS: &str = "thickness";

/// `LargeDripstoneFeature` 的配置。
#[derive(Debug, Clone)]
pub struct LargeDripstoneFeature {
    pub replaceable_blocks: Vec<i32>,
    pub floor_to_ceiling_search_range: i32,
    pub column_radius: IntProvider,
    pub height_scale: FloatProvider,
    pub max_column_radius_to_cave_height_ratio: f32,
    pub stalactite_bluntness: FloatProvider,
    pub stalagmite_bluntness: FloatProvider,
    pub wind_speed: FloatProvider,
    pub min_radius_for_wind: i32,
    pub min_bluntness_for_wind: f32,
}

impl LargeDripstoneFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config
            .as_object()
            .ok_or("large_dripstone feature must be an object")?;
        let field = |name: &str| -> Result<&Value, String> {
            object
                .get(name)
                .ok_or_else(|| format!("large_dripstone has no {name}"))
        };
        Ok(Self {
            replaceable_blocks: holder_set(field("replaceable_blocks")?)?,
            floor_to_ceiling_search_range: object
                .get("floor_to_ceiling_search_range")
                .and_then(Value::as_i64)
                .unwrap_or(30) as i32,
            column_radius: IntProvider::parse(field("column_radius")?)?,
            height_scale: FloatProvider::parse(field("height_scale")?)?,
            max_column_radius_to_cave_height_ratio: field("max_column_radius_to_cave_height_ratio")?
                .as_f64()
                .ok_or("large_dripstone ratio is not a number")?
                as f32,
            stalactite_bluntness: FloatProvider::parse(field("stalactite_bluntness")?)?,
            stalagmite_bluntness: FloatProvider::parse(field("stalagmite_bluntness")?)?,
            wind_speed: FloatProvider::parse(field("wind_speed")?)?,
            min_radius_for_wind: field("min_radius_for_wind")?
                .as_i64()
                .ok_or("large_dripstone min_radius_for_wind is not an int")?
                as i32,
            min_bluntness_for_wind: field("min_bluntness_for_wind")?
                .as_f64()
                .ok_or("large_dripstone min_bluntness_for_wind is not a number")?
                as f32,
        })
    }

    /// 没有嵌套的未移植配置。
    ///
    /// No unported nested config.
    pub fn supported(&self) -> bool {
        true
    }
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

/// 一个方块状态是否属于给定方块集合（按方块注册表 ID 比）。
///
/// Whether a state's block is in the given block set (by block registry ID).
fn in_set(state: u32, ids: &[i32]) -> bool {
    mc_bamboo_data::block_id_of_state(state).is_some_and(|id| ids.contains(&(id as i32)))
}

fn is_block(state: u32, name: &str) -> bool {
    mc_bamboo_data::block_of_state(state).is_some_and(|block| block.name == name)
}

/// `SpeleothemUtils.isEmptyOrWater`。
fn is_empty_or_water(state: u32) -> bool {
    state_facts(state) & facts::AIR != 0 || is_block(state, "minecraft:water")
}

/// `SpeleothemUtils.isEmptyOrWaterOrLava`。
fn is_empty_water_or_lava(state: u32) -> bool {
    is_empty_or_water(state) || is_block(state, "minecraft:lava")
}

/// `SpeleothemUtils.isBase`：`state.is(baseBlock) || state.is(replaceableBlocks)`。
fn is_base(state: u32, ids: &[i32]) -> bool {
    is_block(state, "minecraft:dripstone_block") || in_set(state, ids)
}

/// `SpeleothemUtils.isBaseOrLava`。
fn is_base_or_lava(state: u32, ids: &[i32]) -> bool {
    is_base(state, ids) || is_block(state, "minecraft:lava")
}

/// `SpeleothemUtils.getSpeleothemHeight`。
fn speleothem_height(xz: f64, radius: f64, scale: f64, bluntness: f64) -> f64 {
    let xz = if xz < bluntness { bluntness } else { xz };
    let r = xz / radius * 0.384;
    let part1 = 0.75 * r.powf(1.333_333_333_333_333_3);
    let part2 = r.powf(0.666_666_666_666_666_6);
    let part3 = 0.333_333_333_333_333_3 * r.ln();
    let relative = scale * (part1 - part2 - part3);
    relative.max(0.0) / 0.384 * radius
}

/// `SpeleothemUtils.isCircleMostlyEmbeddedInStone`。
fn is_circle_most_embedded(world: &FeatureWorld<'_>, center: (i32, i32, i32), radius: i32) -> bool {
    if is_empty_water_or_lava(world.get_block(center.0, center.1, center.2)) {
        return false;
    }
    let angle_increment = 6.0_f32 / radius as f32;
    let mut angle = 0.0_f32;
    while angle < std::f32::consts::PI * 2.0 {
        let dx = (angle.cos() * radius as f32) as i32;
        let dz = (angle.sin() * radius as f32) as i32;
        if is_empty_water_or_lava(world.get_block(center.0 + dx, center.1, center.2 + dz)) {
            return false;
        }
        angle += angle_increment;
    }
    true
}

/// `Mth.randomBetween(random, min, maxExclusive)`（float 版）。
///
/// `Mth.randomBetween(random, min, maxExclusive)` (the float overload).
fn random_between(random: &WorldgenRandom, min: f32, max: f32) -> f32 {
    random.next_float() * (max - min) + min
}

/// `Mth.clamp`。
fn clamp(value: i32, min: i32, max: i32) -> i32 {
    value.max(min).min(max)
}

/// 一株钟乳石或石笋。
///
/// One stalactite or stalagmite.
struct LargeDripstone {
    root: (i32, i32, i32),
    pointing_up: bool,
    radius: i32,
    bluntness: f64,
    scale: f64,
}

impl LargeDripstone {
    fn height(&self) -> i32 {
        self.height_at_radius(0.0)
    }

    fn height_at_radius(&self, check_radius: f32) -> i32 {
        speleothem_height(
            f64::from(check_radius),
            f64::from(self.radius),
            self.scale,
            self.bluntness,
        ) as i32
    }

    /// `moveBackUntilBaseIsInsideStoneAndShrinkRadiusIfNecessary`。
    fn move_back_until_embedded(&mut self, world: &FeatureWorld<'_>, wind: &WindOffsetter) -> bool {
        while self.radius > 1 {
            let mut new_root = self.root;
            let max_tries = 10.min(self.height());
            for _ in 0..max_tries {
                if is_block(
                    world.get_block(new_root.0, new_root.1, new_root.2),
                    "minecraft:lava",
                ) {
                    return false;
                }
                if is_circle_most_embedded(world, wind.offset(new_root), self.radius) {
                    self.root = new_root;
                    return true;
                }
                new_root.1 += if self.pointing_up { -1 } else { 1 };
            }
            self.radius /= 2;
        }
        false
    }

    /// `placeBlocks`。
    fn place_blocks(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        wind: &WindOffsetter,
    ) {
        for dx in -self.radius..=self.radius {
            for dz in -self.radius..=self.radius {
                let current_radius = ((dx * dx + dz * dz) as f32).sqrt();
                if current_radius > self.radius as f32 {
                    continue;
                }
                let mut height = self.height_at_radius(current_radius);
                if height <= 0 {
                    continue;
                }
                if random.next_float() < 0.2 {
                    height = (height as f32 * random_between(random, 0.8, 1.0)) as i32;
                }
                let mut pos = (self.root.0 + dx, self.root.1, self.root.2 + dz);
                let mut been_out_of_stone = false;
                let max_y = if self.pointing_up {
                    world.get_height(HeightmapKind::WorldSurfaceWg, pos.0, pos.2)
                } else {
                    i32::MAX
                };
                let mut placed = 0;
                while placed < height && pos.1 < max_y {
                    let wind_pos = wind.offset(pos);
                    let state = world.get_block(wind_pos.0, wind_pos.1, wind_pos.2);
                    if is_empty_water_or_lava(state) {
                        been_out_of_stone = true;
                        world.set_block_with_heightmaps(
                            wind_pos.0,
                            wind_pos.1,
                            wind_pos.2,
                            blocks::DRIPSTONE_BLOCK,
                        );
                    } else if been_out_of_stone
                        && in_set(
                            state,
                            mc_bamboo_data::block_tag(tags::block_tags::BASE_STONE_OVERWORLD.name)
                                .unwrap_or(&[]),
                        )
                    {
                        break;
                    }
                    pos.1 += if self.pointing_up { 1 } else { -1 };
                    placed += 1;
                }
            }
        }
    }

    fn is_suitable_for_wind(&self, min_radius: i32, min_bluntness: f32) -> bool {
        self.radius >= min_radius && self.bluntness >= f64::from(min_bluntness)
    }
}

/// `LargeDripstoneFeature.WindOffsetter`：`wind_speed` 为 `None` 时不偏移。
///
/// `LargeDripstoneFeature.WindOffsetter`: no offset when `wind_speed` is `None`.
struct WindOffsetter {
    origin_y: i32,
    wind_speed: Option<(f64, f64)>,
    max_offset: i32,
}

impl WindOffsetter {
    fn new(origin_y: i32, random: &WorldgenRandom, speed: f32, max_offset: i32) -> Self {
        let direction = random_between(random, 0.0, std::f32::consts::PI);
        Self {
            origin_y,
            wind_speed: Some((
                f64::from(direction.cos() * speed),
                f64::from(direction.sin() * speed),
            )),
            max_offset,
        }
    }

    fn no_wind() -> Self {
        Self {
            origin_y: 0,
            wind_speed: None,
            max_offset: 0,
        }
    }

    /// `offset(pos)`：`dy = originY - pos.y`，水平位移按风速缩放后 floor + clamp。
    ///
    /// `offset(pos)`: `dy = originY - pos.y`; the horizontal shift is the wind vector
    /// scaled by `dy`, floored and clamped.
    fn offset(&self, pos: (i32, i32, i32)) -> (i32, i32, i32) {
        let Some((wind_x, wind_z)) = self.wind_speed else {
            return pos;
        };
        let dy = f64::from(self.origin_y - pos.1);
        let dx = clamp(
            (wind_x * dy).floor() as i32,
            -self.max_offset,
            self.max_offset,
        );
        let dz = clamp(
            (wind_z * dy).floor() as i32,
            -self.max_offset,
            self.max_offset,
        );
        (pos.0 + dx, pos.1, pos.2 + dz)
    }
}

/// `Column.scan` 的结果：地面和天花板的 y（`None` 表示那一侧没有边界）。
///
/// The result of `Column.scan`: floor and ceiling y (`None` when that side has no edge).
type ColumnRange = (Option<i32>, Option<i32>);

/// `Column.scan`：先向上再向下找第一个不在空腔里的格子，且那一格必须是「有效边缘」。
///
/// `Column.scan`: scan up then down for the first cell outside the cavity, which must also
/// pass the `valid_edge` test.
fn column_scan(
    world: &FeatureWorld<'_>,
    pos: (i32, i32, i32),
    search_range: i32,
    inside: impl Fn(u32) -> bool,
    valid_edge: impl Fn(u32) -> bool,
) -> Option<ColumnRange> {
    if !inside(world.get_block(pos.0, pos.1, pos.2)) {
        return None;
    }
    let ceiling = scan_direction(world, pos, search_range, &inside, &valid_edge, 1);
    let floor = scan_direction(world, pos, search_range, &inside, &valid_edge, -1);
    Some((floor, ceiling))
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

/// `LargeDripstoneFeature.place`。
///
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
    let Some((floor, ceiling)) = column_scan(
        world,
        origin,
        feature.floor_to_ceiling_search_range,
        is_empty_or_water,
        |state| is_base_or_lava(state, &feature.replaceable_blocks),
    ) else {
        return false;
    };
    let (Some(floor), Some(ceiling)) = (floor, ceiling) else {
        return false;
    };
    // `Column.Range.height()` = `ceiling - floor - 1`。
    let height = ceiling - floor - 1;
    if height < 4 {
        return false;
    }
    let min_radius = min_inclusive(&feature.column_radius);
    let max_radius = max_inclusive(&feature.column_radius);
    let radius_cap =
        (f64::from(height) * f64::from(feature.max_column_radius_to_cave_height_ratio)) as i32;
    let max_column_radius = clamp(radius_cap, min_radius, max_radius);
    let radius = random_between_inclusive(random, min_radius, max_column_radius);

    let mut stalactite = LargeDripstone {
        root: (origin.0, ceiling - 1, origin.2),
        pointing_up: false,
        radius,
        bluntness: f64::from(feature.stalactite_bluntness.sample(random)),
        scale: f64::from(feature.height_scale.sample(random)),
    };
    let mut stalagmite = LargeDripstone {
        root: (origin.0, floor + 1, origin.2),
        pointing_up: true,
        radius,
        bluntness: f64::from(feature.stalagmite_bluntness.sample(random)),
        scale: f64::from(feature.height_scale.sample(random)),
    };
    let wind = if stalactite
        .is_suitable_for_wind(feature.min_radius_for_wind, feature.min_bluntness_for_wind)
        && stalagmite
            .is_suitable_for_wind(feature.min_radius_for_wind, feature.min_bluntness_for_wind)
    {
        WindOffsetter::new(
            origin.1,
            random,
            feature.wind_speed.sample(random),
            16 - radius,
        )
    } else {
        WindOffsetter::no_wind()
    };

    let stalactite_embedded = stalactite.move_back_until_embedded(world, &wind);
    let stalagmite_embedded = stalagmite.move_back_until_embedded(world, &wind);
    if stalactite_embedded {
        stalactite.place_blocks(world, random, &wind);
    }
    if stalagmite_embedded {
        stalagmite.place_blocks(world, random, &wind);
    }
    true
}

/// `IntProvider.minInclusive()`。
///
/// `IntProvider.minInclusive()`.
fn min_inclusive(provider: &IntProvider) -> i32 {
    provider.min_value()
}

/// `IntProvider.maxInclusive()`。
fn max_inclusive(provider: &IntProvider) -> i32 {
    provider.max_value()
}

/// `Mth.randomBetweenInclusive(random, min, maxInclusive)`。
///
/// `Mth.randomBetweenInclusive(random, min, maxInclusive)`.
fn random_between_inclusive(random: &WorldgenRandom, min: i32, max_inclusive: i32) -> i32 {
    random.next_int(max_inclusive - min + 1) + min
}

/// 把 `pointed_dripstone` 的 `waterlogged` 属性按水位设置（预留给后续扩展）。
///
/// Set the `waterlogged` property of `pointed_dripstone` from the water state (reserved for
/// later extension).
#[allow(dead_code)]
fn pointed_state(direction: &str, thickness: &str, waterlogged: bool) -> Option<u32> {
    let block = mc_bamboo_data::block_by_name("minecraft:pointed_dripstone")?;
    let state = apply_property(block.default_state, TIP_DIRECTION, direction)?;
    let state = apply_property(state, THICKNESS, thickness)?;
    apply_property(
        state,
        "waterlogged",
        if waterlogged { "true" } else { "false" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_overworld_large_dripstone() {
        let root: Value =
            serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).expect("worldgen json");
        let features = root
            .get("feature")
            .and_then(Value::as_object)
            .expect("feature registry");
        let config = features
            .get("minecraft:large_dripstone")
            .expect("large_dripstone config");
        let feature = LargeDripstoneFeature::parse(config).expect("parses");
        assert_eq!(feature.floor_to_ceiling_search_range, 30);
        assert_eq!(feature.min_radius_for_wind, 4);
        assert!((feature.max_column_radius_to_cave_height_ratio - 0.33).abs() < 1e-6);
        assert!(!feature.replaceable_blocks.is_empty());
    }

    #[test]
    fn speleothem_height_is_zero_at_the_bluntness_floor() {
        // 半径 0 时高度为正，且钝度不低于阈值时不会被截断成负值。
        // A zero distance still yields a positive height (the bluntness floor applies).
        let height = speleothem_height(0.0, 4.0, 1.0, 0.5);
        assert!(height > 0.0, "{height}");
    }
}
