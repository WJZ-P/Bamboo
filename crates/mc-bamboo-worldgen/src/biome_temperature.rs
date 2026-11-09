//! 官方 `Biome` 的气候温度：按位置修过的温度、够不够暖得下雨、够不够冷得下雪。
//!
//! `Biome.getHeightAdjustedTemperature` 用三份和世界种子无关的静态噪声，见模块里的常量。
//! 数据包的群系基础温度和 `temperature_modifier` 在 `mc-bamboo-data` 的 `biomes.rs` 里。
//! 世界线程和方块 oracle 都用这一份实现。

use std::sync::OnceLock;

use mc_bamboo_blocks::LegacyRandom;
use mc_bamboo_data::{BIOME_CLIMATES, registries::biome};

use crate::{
    noise::{Noise, NoiseStack, SimplexNoise},
    random::Random,
};

/// 官方 `Biome.ClimateSettings` 里天气要用的三项。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiomeClimate {
    pub has_precipitation: bool,
    pub temperature: f32,
    /// `temperature_modifier` 是 `frozen`。
    pub frozen: bool,
}

impl Default for BiomeClimate {
    fn default() -> Self {
        Self::of(biome::PLAINS)
    }
}

impl BiomeClimate {
    /// 按 `minecraft:worldgen/biome` 注册表 ID 查气候。越界时按平原算。
    pub fn of(id: i32) -> Self {
        usize::try_from(id)
            .ok()
            .and_then(|index| BIOME_CLIMATES.get(index))
            .map_or_else(Self::default, |generated| Self {
                has_precipitation: generated.has_precipitation,
                temperature: generated.temperature,
                frozen: generated.frozen,
            })
    }

    /// `Biome.getTemperature(pos, seaLevel)`，没有温度缓存（缓存只影响速度）。
    pub fn temperature_at(self, x: i32, y: i32, z: i32, sea_level: i32) -> f32 {
        let noises = static_noises();
        let base = if self.frozen {
            frozen_modifier(noises, x, z, self.temperature)
        } else {
            self.temperature
        };
        // `Biome.getHeightAdjustedTemperature` 的 `seaLevel + 17`。
        let threshold = sea_level + 17;
        if y <= threshold {
            return base;
        }
        // 官方先把 int 坐标提成 float、除以 8，再把 float 提成 double 传给噪声。
        let noise = noises
            .temperature
            .get2(f64::from(x as f32 / 8.0), f64::from(z as f32 / 8.0))
            * 8.0;
        base - (y as f32 + noise - threshold as f32) * 0.05 / 40.0
    }

    /// `Biome.warmEnoughToRain(pos, seaLevel)`。
    pub fn warm_enough_to_rain(self, x: i32, y: i32, z: i32, sea_level: i32) -> bool {
        self.temperature_at(x, y, z, sea_level) >= 0.15
    }

    /// `Biome.coldEnoughToSnow(pos, seaLevel)`。
    pub fn cold_enough_to_snow(self, x: i32, y: i32, z: i32, sea_level: i32) -> bool {
        !self.warm_enough_to_rain(x, y, z, sea_level)
    }
}

/// 官方 `Biome.Precipitation`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precipitation {
    None,
    Rain,
    Snow,
}

/// `Biome.getPrecipitationAt(pos, seaLevel)`。
pub fn precipitation_at(
    climate: BiomeClimate,
    x: i32,
    y: i32,
    z: i32,
    sea_level: i32,
) -> Precipitation {
    if !climate.has_precipitation {
        Precipitation::None
    } else if climate.cold_enough_to_snow(x, y, z, sea_level) {
        Precipitation::Snow
    } else {
        Precipitation::Rain
    }
}

/// 三份静态噪声：`TEMPERATURE_NOISE`、`FROZEN_TEMPERATURE_NOISE` 和 `BIOME_INFO_NOISE`。
struct StaticNoises {
    temperature: SimplexNoise,
    frozen: Noise,
    biome_info: SimplexNoise,
}

fn static_noises() -> &'static StaticNoises {
    static NOISES: OnceLock<StaticNoises> = OnceLock::new();
    NOISES.get_or_init(|| {
        // `Biome` 的静态初始化：三份噪声都用 `WorldgenRandom(LegacyRandomSource)` 洗置换表，
        // 种子是硬编码的，和世界种子无关。`WorldgenRandom` 取数和内层旧源一致。
        let temperature_random = Random::Legacy(LegacyRandom::new(1234));
        let temperature = SimplexNoise::new(&temperature_random, true);

        let frozen_random = Random::Legacy(LegacyRandom::new(3456));
        let mut stack = NoiseStack::builder();
        for &(frequency, amplitude) in &[
            (1.0_f64, 0.14285715_f32),
            (0.5, 0.2857143),
            (0.25, 0.5714286),
        ] {
            stack.add(
                Noise::Simplex(SimplexNoise::new(&frozen_random, true)),
                frequency,
                amplitude,
            );
        }

        let info_random = Random::Legacy(LegacyRandom::new(2345));
        let biome_info = SimplexNoise::new(&info_random, true);
        StaticNoises {
            temperature,
            frozen: Noise::Stack(stack.build()),
            biome_info,
        }
    })
}

/// `TemperatureModifier.FROZEN.modifyTemperature`：两处噪声都偏小时温度变成 0.2。
fn frozen_modifier(noises: &StaticNoises, x: i32, z: i32, base: f32) -> f32 {
    let xf = f64::from(x);
    let zf = f64::from(z);
    let first = f64::from(noises.frozen.get2(xf * 0.05, zf * 0.05)) * 7.0;
    let sum = first + f64::from(noises.biome_info.get2(xf * 0.2, zf * 0.2));
    if sum < 0.3 && f64::from(noises.biome_info.get2(xf * 0.09, zf * 0.09)) < 0.8 {
        0.2
    } else {
        base
    }
}
