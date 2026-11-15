//! `SnowAndFreezeFeature`（`freeze_top_layer`）：给最顶层铺雪、把水面冻成冰。
//!
//! 照 `SnowAndFreezeFeature.place`：对 16×16 的每一列取 `MOTION_BLOCKING` 高度，
//! 用 `Biome.shouldFreeze` 判定把顶面下方那格冻成冰，再用 `Biome.shouldSnow` 判定
//! 在顶面铺雪、并把下方带 `snowy` 属性的方块改成 `snowy=true`。
//!
//! 群系那两段在官方是 `Biome.shouldFreeze` / `shouldSnow`：
//! - `warmEnoughToRain` = `getTemperature(pos, seaLevel) >= 0.15`；
//! - `getTemperature` = `temperatureModifier.modifyTemperature(pos, baseTemperature)`，
//!   高于 `seaLevel + 17` 再按 `TEMPERATURE_NOISE` 线性衰减；
//! - `getPrecipitationAt`：`!hasPrecipitation` 时 `NONE`，否则按 `coldEnoughToSnow`
//!   取 `SNOW` / `RAIN`。
//!
//! features 阶段还没有生成期光照，`LevelReader.getBrightness(BLOCK, pos)` 恒为 0
//! （`< 10` 的条件总是成立），所以这里不查亮度。
//!
//! `SnowAndFreezeFeature.place`: for each of the 16×16 columns read the
//! `MOTION_BLOCKING` height, freeze the cell below it when `Biome.shouldFreeze` says so,
//! then lay a snow layer on top and flip `snowy=true` on the block below when
//! `Biome.shouldSnow` agrees.
//!
//! Generation-time light does not exist during the features stage, so
//! `LevelReader.getBrightness(BLOCK, pos)` is always 0 (the `< 10` test always passes);
//! this port therefore never queries brightness.

use std::sync::OnceLock;

use mc_bamboo_data::{BIOME_CLIMATES, blocks, facts, state_facts, tags};
use serde_json::Value;

use crate::{
    noise::{Noise, NoiseStack, SimplexNoise},
    random::Random,
};

use super::{
    state::apply_property,
    world::{FeatureWorld, HeightmapKind},
};

/// `Biome.TEMPERATURE_NOISE`：`new SimplexNoise(new WorldgenRandom(new LegacyRandomSource(1234L)), true)`。
///
/// `Biome.TEMPERATURE_NOISE`.
fn temperature_noise() -> &'static Noise {
    static NOISE: OnceLock<Noise> = OnceLock::new();
    NOISE.get_or_init(|| {
        Noise::Simplex(SimplexNoise::new(
            &Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(1234)),
            true,
        ))
    })
}

/// `Biome.FROZEN_TEMPERATURE_NOISE`：三层 simplex，频率 1 / 0.5 / 0.25，振幅
/// 0.14285715 / 0.2857143 / 0.5714286（同一个 `WorldgenRandom(Legacy(3456L))`）。
///
/// `Biome.FROZEN_TEMPERATURE_NOISE`: three simplex layers sharing one
/// `WorldgenRandom(Legacy(3456L))`.
fn frozen_temperature_noise() -> &'static Noise {
    static NOISE: OnceLock<Noise> = OnceLock::new();
    NOISE.get_or_init(|| {
        let random = Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(3456));
        let mut builder = NoiseStack::builder();
        for (frequency, amplitude) in [
            (1.0, 0.142_857_15_f32),
            (0.5, 0.285_714_3_f32),
            (0.25, 0.571_428_6_f32),
        ] {
            builder.add(
                Noise::Simplex(SimplexNoise::new(&random, true)),
                frequency,
                amplitude,
            );
        }
        Noise::Stack(builder.build())
    })
}

/// `SnowAndFreezeFeature` 没有配置字段（`MapCodec.unit`）。
///
/// `SnowAndFreezeFeature` has no config fields (`MapCodec.unit`).
#[derive(Debug, Clone, Copy)]
pub struct FreezeTopLayerFeature;

impl FreezeTopLayerFeature {
    pub fn parse(_config: &Value) -> Result<Self, String> {
        Ok(Self)
    }

    /// 没有嵌套配置，恒可对拍。
    ///
    /// No nested config, always comparable.
    pub fn supported(&self) -> bool {
        true
    }
}

/// `Biome.getBaseTemperature()`。
fn base_temperature(biome: u32) -> f32 {
    BIOME_CLIMATES
        .get(biome as usize)
        .map_or(0.5, |climate| climate.temperature)
}

/// `Biome.hasPrecipitation()`。
fn has_precipitation(biome: u32) -> bool {
    BIOME_CLIMATES
        .get(biome as usize)
        .is_some_and(|climate| climate.has_precipitation)
}

/// `temperature_modifier` 是 `frozen`。
fn is_frozen_biome(biome: u32) -> bool {
    BIOME_CLIMATES
        .get(biome as usize)
        .is_some_and(|climate| climate.frozen)
}

/// `Biome.BIOME_INFO_NOISE`：`SimplexNoise(new WorldgenRandom(new LegacyRandomSource(2345L)), true)`。
///
/// `Biome.BIOME_INFO_NOISE`.
fn biome_info_noise() -> &'static Noise {
    static NOISE: OnceLock<Noise> = OnceLock::new();
    NOISE.get_or_init(|| {
        Noise::Simplex(SimplexNoise::new(
            &Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(2345)),
            true,
        ))
    })
}

/// `Biome.getTemperature(pos, seaLevel)`。
///
/// `Biome.getTemperature(pos, seaLevel)`.
fn temperature(biome: u32, x: i32, y: i32, z: i32, sea_level: i32) -> f32 {
    // `TemperatureModifier.FROZEN.modifyTemperature`：三层 `FROZEN_TEMPERATURE_NOISE`
    // （×7）加一层 `BIOME_INFO_NOISE`（0.2 频率）够小、且 0.09 频率那层也够小时，
    // 温度被抬到 0.2。
    // `TemperatureModifier.FROZEN.modifyTemperature`: when the large-variation noise
    // (scaled by 7) plus the 0.2-frequency info noise is small enough, and the
    // 0.09-frequency layer is also small, the temperature becomes 0.2.
    let base = base_temperature(biome);
    let adjusted = if is_frozen_biome(biome) {
        let large = frozen_temperature_noise().get2(x as f64 * 0.05, z as f64 * 0.05) * 7.0;
        let edge = biome_info_noise().get2(x as f64 * 0.2, z as f64 * 0.2);
        let patches = f64::from(large) + f64::from(edge);
        if patches < 0.3 {
            let small = biome_info_noise().get2(x as f64 * 0.09, z as f64 * 0.09);
            if small < 0.8 { 0.2_f32 } else { base }
        } else {
            base
        }
    } else {
        base
    };

    // `getHeightAdjustedTemperature`。
    let snow_level = sea_level + 17;
    if y > snow_level {
        let value = temperature_noise().get2(x as f64 / 8.0, z as f64 / 8.0) * 8.0;
        adjusted - (value + (y - snow_level) as f32) * 0.05_f32 / 40.0
    } else {
        adjusted
    }
}

/// `Biome.warmEnoughToRain(pos, seaLevel)`。
fn warm_enough_to_rain(biome: u32, x: i32, y: i32, z: i32, sea_level: i32) -> bool {
    temperature(biome, x, y, z, sea_level) >= 0.15_f32
}

/// `Biome.getPrecipitationAt(pos, seaLevel)`。
///
/// `Biome.getPrecipitationAt(pos, seaLevel)`.
fn precipitation_is_snow(biome: u32, x: i32, y: i32, z: i32, sea_level: i32) -> bool {
    has_precipitation(biome) && !warm_enough_to_rain(biome, x, y, z, sea_level)
}

/// `SnowLayerBlock.canSurvive(state, level, pos)`。
///
/// `SnowLayerBlock.canSurvive(state, level, pos)`.
fn snow_can_survive(world: &FeatureWorld<'_>, pos: (i32, i32, i32)) -> bool {
    let below = (pos.0, pos.1 - 1, pos.2);
    let below_state = world.get_block(below.0, below.1, below.2);
    let block_id = mc_bamboo_data::block_id_of_state(below_state).map(|id| id as i32);
    if block_id.is_some_and(|id| tag_contains(tags::block_tags::CANNOT_SUPPORT_SNOW_LAYER.name, id))
    {
        return false;
    }
    if block_id
        .is_some_and(|id| tag_contains(tags::block_tags::SUPPORT_OVERRIDE_SNOW_LAYER.name, id))
    {
        return true;
    }
    // `Block.isFaceFull(collisionShape, UP) || belowState.is(this) && layers == 8`。
    // `Block.isFaceFull(collisionShape, UP) || belowState.is(this) && layers == 8`.
    let face_full = state_facts(below_state) & (1u64 << facts::STURDY_FULL) != 0;
    let same_snow_full = mc_bamboo_data::block_of_state(below_state)
        .is_some_and(|block| block.name == "minecraft:snow")
        && snow_layers(below_state) == Some(8);
    face_full || same_snow_full
}

/// 方块 ID 是否在给定标签里。
///
/// Whether a block ID is in the named tag.
fn tag_contains(tag: &str, id: i32) -> bool {
    mc_bamboo_data::block_tag(tag).is_some_and(|ids| ids.contains(&id))
}

/// `SnowLayerBlock.LAYERS` 的值。
///
/// The `SnowLayerBlock.LAYERS` value.
fn snow_layers(state: u32) -> Option<i32> {
    let block = mc_bamboo_data::block_of_state(state)?;
    let mut stride = 1_u32;
    for property in block.properties.iter().rev() {
        let index = (state - block.first_state) / stride % property.values.len() as u32;
        if property.name == "layers" {
            return property.values.get(index as usize)?.parse().ok();
        }
        stride *= property.values.len() as u32;
    }
    None
}

/// `SnowAndFreezeFeature.place`。
///
/// `SnowAndFreezeFeature.place`.
pub fn place(
    _feature: &FreezeTopLayerFeature,
    world: &mut FeatureWorld<'_>,
    _random: &crate::random::WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    for dx in 0..16 {
        for dz in 0..16 {
            let x = origin.0 + dx;
            let z = origin.2 + dz;
            let y = world.get_height(HeightmapKind::MotionBlocking, x, z);
            let top = (x, y, z);
            let below = (x, y - 1, z);
            let biome = world.get_biome(top.0, top.1, top.2);
            if should_freeze(world, biome, below) {
                world.set_block_with_heightmaps(below.0, below.1, below.2, blocks::ICE);
            }
            if should_snow(world, biome, top) {
                world.set_block_with_heightmaps(top.0, top.1, top.2, blocks::SNOW);
                let below_state = world.get_block(below.0, below.1, below.2);
                let has_snowy = mc_bamboo_data::block_of_state(below_state)
                    .is_some_and(|block| block.properties.iter().any(|p| p.name == "snowy"));
                if has_snowy && let Some(state) = apply_property(below_state, "snowy", "true") {
                    world.set_block_with_heightmaps(below.0, below.1, below.2, state);
                }
            }
        }
    }
    true
}

/// `Biome.shouldFreeze(level, pos, false)`。
///
/// `Biome.shouldFreeze(level, pos, false)`.
fn should_freeze(world: &FeatureWorld<'_>, biome: u32, pos: (i32, i32, i32)) -> bool {
    if warm_enough_to_rain(biome, pos.0, pos.1, pos.2, world.sea_level) {
        return false;
    }
    // 亮度条件在 features 阶段恒成立（见模块文档）。
    // The brightness test always holds during the features stage (see the module docs).
    if !world.is_inside_build_height(pos.1) {
        return false;
    }
    let state = world.get_block(pos.0, pos.1, pos.2);
    let fluid = mc_bamboo_data::state_fluid(state);
    // `fluidState.is(Fluids.WATER) && blockState.getBlock() instanceof LiquidBlock`。
    // `fluidState.is(Fluids.WATER) && blockState.getBlock() instanceof LiquidBlock`.
    fluid.fluid == mc_bamboo_data::registries::fluid::WATER
        && mc_bamboo_data::block_of_state(state)
            .is_some_and(|block| block.name == "minecraft:water")
}

/// `Biome.shouldSnow(level, pos)`。
///
/// `Biome.shouldSnow(level, pos)`.
fn should_snow(world: &FeatureWorld<'_>, biome: u32, pos: (i32, i32, i32)) -> bool {
    if !precipitation_is_snow(biome, pos.0, pos.1, pos.2, world.sea_level) {
        return false;
    }
    if !world.is_inside_build_height(pos.1) {
        return false;
    }
    let state = world.get_block(pos.0, pos.1, pos.2);
    let is_air_or_snow = state_facts(state) & facts::AIR != 0
        || mc_bamboo_data::block_of_state(state)
            .is_some_and(|block| block.name == "minecraft:snow");
    is_air_or_snow && snow_can_survive(world, pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_top_layer_has_no_config_fields() {
        let config: Value = serde_json::json!({"type": "minecraft:freeze_top_layer"});
        assert!(FreezeTopLayerFeature::parse(&config).is_ok());
    }

    #[test]
    fn warm_biomes_never_snow() {
        // 沙漠温度 2.0，一定 `warmEnoughToRain`。
        // Desert has temperature 2.0, always warm enough to rain.
        let desert = mc_bamboo_data::BIOME_CLIMATES
            .iter()
            .position(|climate| climate.name == "minecraft:desert")
            .expect("desert exists");
        assert!(!precipitation_is_snow(desert as u32, 0, 64, 0, 63));
    }

    #[test]
    fn cold_biomes_precipitate_as_snow() {
        let snowy = mc_bamboo_data::BIOME_CLIMATES
            .iter()
            .position(|climate| climate.name == "minecraft:snowy_plains")
            .expect("snowy plains exists");
        assert!(precipitation_is_snow(snowy as u32, 0, 64, 0, 63));
    }
}
