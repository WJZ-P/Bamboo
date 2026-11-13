//! 官方 `BiomeManager`：用世界种子混淆过的 zoom 种子，对 quart 格做八角插值。
//!
//! 地表规则的 `biomeGetter`（`BiomeManager::getBiome`）和雕刻器选群系都走这里。
//! 不启动服务端、不读存档，只用 [`MultiNoiseBiomeSource`] 在 quart 格上直接问。
//!
//! 对拍里两边都用「全部群系」做 `possibleBiomes`，正式接进游戏时要换成周围
//! 3×3 区块群系调色板的并集（那是 `BiomeCondition` 的静态优化，不影响逐格判定）。

use mc_bamboo_data::registries::{biome, fluid};
use sha2::{Digest, Sha256};

use crate::{
    biome::MultiNoiseBiomeSource,
    biome_temperature::BiomeClimate,
    climate::{Sampler, TargetPoint},
    math::{java_floor_div, quart_to_block},
    sampler::{Context, Sampler as DensitySampler},
};

/// `BiomeManager.obfuscateSeed(seed)`：SHA-256 摘要的前八字节按大端读成 long。
///
/// Guava 的 `hashLong` 用小端写八个 byte，`asLong` 再按小端读回来，所以结果是
/// `digest[0] | digest[1]<<8 | ...`，和这里一致。
pub fn obfuscate_seed(seed: i64) -> i64 {
    let digest = Sha256::digest(seed.to_le_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    let mut value = 0_i64;
    for (index, byte) in bytes.iter().enumerate() {
        value |= i64::from(*byte) << (index * 8);
    }
    value
}

/// `BiomeManager` 的纯计算部分。
pub struct BiomeManager<'a> {
    source: &'a MultiNoiseBiomeSource,
    seed: i64,
}

/// 一次地表/雕刻扫描里的气候参数缓存。
///
/// `BiomeManager` 每次问群系都要在空的 `SamplerContext` 上重算六条气候密度函数，
/// 而地表逐列扫描和雕刻器 17×17 邻域会在同一批 quart 格上反复问。气候采样是纯函数
/// （不记忆的上下文、不取随机数），按 quart 坐标记下来即可。
///
/// **只缓存气候参数，不缓存群系**：查群系要过 `ParameterList` 的 R 树，而 R 树带着
/// `lastResult` 当平局时的初始候选，跳过一次查询就会改变后续查询的平局结果。
/// 所以每次仍照原来的顺序查表，只把前面那六次密度求值省掉。
#[derive(Debug)]
pub struct ClimateCache {
    /// 缓存的 quart 范围：`(min_x, min_y, min_z, 宽, 高, 深)`。
    shape: Option<(i32, i32, i32, i32, i32, i32)>,
    cells: Vec<Option<TargetPoint>>,
}

impl ClimateCache {
    /// 按一次扫描里会被问到的 quart 范围建表；范围外的坐标直接走原路径、不缓存。
    pub fn new(min_x: i32, min_y: i32, min_z: i32, size_x: i32, size_y: i32, size_z: i32) -> Self {
        let len = (size_x.max(0) as usize)
            .saturating_mul(size_y.max(0) as usize)
            .saturating_mul(size_z.max(0) as usize);
        Self {
            shape: (len > 0).then_some((min_x, min_y, min_z, size_x, size_y, size_z)),
            cells: vec![None; len.min(1 << 20)],
        }
    }

    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let (min_x, min_y, min_z, size_x, size_y, size_z) = self.shape?;
        let dx = x.wrapping_sub(min_x);
        let dy = y.wrapping_sub(min_y);
        let dz = z.wrapping_sub(min_z);
        if dx < 0 || dy < 0 || dz < 0 || dx >= size_x || dy >= size_y || dz >= size_z {
            return None;
        }
        Some((dx as usize * size_y as usize + dy as usize) * size_z as usize + dz as usize)
    }

    /// 命中就用记下的气候参数，没命中就按原顺序算一次。
    fn climate(
        &mut self,
        samplers: &crate::settings::Samplers,
        context: &mut Context,
        x: i32,
        y: i32,
        z: i32,
    ) -> TargetPoint {
        let Some(slot) = self.index(x, y, z) else {
            return climate_at(samplers, context, x, y, z);
        };
        if let Some(value) = self.cells[slot] {
            return value;
        }
        let value = climate_at(samplers, context, x, y, z);
        self.cells[slot] = Some(value);
        value
    }
}

impl<'a> BiomeManager<'a> {
    pub fn new(source: &'a MultiNoiseBiomeSource, world_seed: i64) -> Self {
        Self {
            source,
            seed: obfuscate_seed(world_seed),
        }
    }

    /// `getBiome(x, y, z)` 的带缓存版本，气候参数走 `cache`。
    ///
    /// 没传缓存时走的路径和 [`Self::biome`] 完全一样。
    pub fn biome_cached(
        &self,
        samplers: &crate::settings::Samplers,
        context: &mut Context,
        x: i32,
        y: i32,
        z: i32,
        cache: Option<&mut ClimateCache>,
    ) -> u32 {
        let quart = self.fiddled_corner(x, y, z);
        let target = match cache {
            Some(cache) => cache.climate(samplers, context, quart.0, quart.1, quart.2),
            None => climate_at(samplers, context, quart.0, quart.1, quart.2),
        };
        biome_id(self.source.biome(target))
    }

    /// `getNoiseBiome(quartX, quartY, quartZ)` 的带缓存版本。
    pub fn noise_biome_cached(
        &self,
        samplers: &crate::settings::Samplers,
        context: &mut Context,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        cache: &mut ClimateCache,
    ) -> u32 {
        let target = cache.climate(samplers, context, quart_x, quart_y, quart_z);
        biome_id(self.source.biome(target))
    }

    /// `getBiome(x, y, z)`：返回群系注册表 ID。
    ///
    /// 官方的 `BiomeResolver` 在这里是 `createUncachedResolver`，对应一个空的、
    /// 不记忆的 `SamplerContext`；世界生成的其它步骤不会改这个上下文。
    pub fn biome(
        &self,
        samplers: &crate::settings::Samplers,
        context: &mut Context,
        x: i32,
        y: i32,
        z: i32,
    ) -> u32 {
        let (cx, cy, cz) = self.fiddled_corner(x, y, z);
        let target = climate_at(samplers, context, cx, cy, cz);
        biome_id(self.source.biome(target))
    }

    /// `BiomeManager.getBiome` 的八角插值：按 `biomeZoomSeed` 选出最近的 quart 角。
    fn fiddled_corner(&self, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
        let quart_x = java_floor_div(x.wrapping_sub(2), 4);
        let quart_y = java_floor_div(y.wrapping_sub(2), 4);
        let quart_z = java_floor_div(z.wrapping_sub(2), 4);
        let fraction_x = ((x.wrapping_sub(2)) & 3) as f64 / 4.0;
        let fraction_y = ((y.wrapping_sub(2)) & 3) as f64 / 4.0;
        let fraction_z = ((z.wrapping_sub(2)) & 3) as f64 / 4.0;

        let mut best = 0_u32;
        let mut best_distance = f64::INFINITY;
        for corner in 0..8 {
            let low_x = corner & 4 == 0;
            let low_y = corner & 2 == 0;
            let low_z = corner & 1 == 0;
            let cx = if low_x { quart_x } else { quart_x + 1 };
            let cy = if low_y { quart_y } else { quart_y + 1 };
            let cz = if low_z { quart_z } else { quart_z + 1 };
            let fx = if low_x { fraction_x } else { fraction_x - 1.0 };
            let fy = if low_y { fraction_y } else { fraction_y - 1.0 };
            let fz = if low_z { fraction_z } else { fraction_z - 1.0 };
            let distance = fiddled_distance(self.seed, cx, cy, cz, fx, fy, fz);
            // 官方是「严格小于」才换：等距时保留第一个角。
            if distance < best_distance {
                best = corner;
                best_distance = distance;
            }
        }

        (
            if best & 4 == 0 { quart_x } else { quart_x + 1 },
            if best & 2 == 0 { quart_y } else { quart_y + 1 },
            if best & 1 == 0 { quart_z } else { quart_z + 1 },
        )
    }

    /// `getNoiseBiome(quartX, quartY, quartZ)`：雕刻器选邻域群系用。
    pub fn noise_biome(
        &self,
        samplers: &crate::settings::Samplers,
        context: &mut Context,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
    ) -> u32 {
        let target = climate_at(samplers, context, quart_x, quart_y, quart_z);
        biome_id(self.source.biome(target))
    }
}

fn climate_at(
    samplers: &crate::settings::Samplers,
    context: &mut Context,
    quart_x: i32,
    quart_y: i32,
    quart_z: i32,
) -> TargetPoint {
    let sampler = Sampler {
        temperature: &samplers.temperature,
        humidity: &samplers.vegetation,
        continentalness: &samplers.continents,
        erosion: &samplers.erosion,
        depth: &samplers.depth,
        weirdness: &samplers.ridges,
    };
    let _ = quart_to_block;
    sampler.sample(context, quart_x, quart_y, quart_z)
}

/// 群系 ID 到名字。越界时按平原算，和 `biome_temperature` 的处理一致。
pub fn biome_name(id: u32) -> &'static str {
    biome::NAMES
        .get(id as usize)
        .copied()
        .unwrap_or("minecraft:plains")
}

/// 名字到注册表 ID。
pub fn biome_id(name: &str) -> u32 {
    biome::NAMES
        .iter()
        .position(|id| *id == name)
        .unwrap_or(biome::PLAINS as usize) as u32
}

/// `Biome.coldEnoughToSnow(pos, seaLevel)`。
pub fn cold_enough_to_snow(id: u32, x: i32, y: i32, z: i32, sea_level: i32) -> bool {
    BiomeClimate::of(id as i32).cold_enough_to_snow(x, y, z, sea_level)
}

/// `Biome.getTemperature(pos, seaLevel)`。
pub fn biome_temperature(id: u32, x: i32, y: i32, z: i32, sea_level: i32) -> f32 {
    BiomeClimate::of(id as i32).temperature_at(x, y, z, sea_level)
}

/// 方块状态的 `FluidState.isEmpty()`，用注册表里的流体 ID 判断。
pub fn fluid_is_empty(state: u32) -> bool {
    mc_bamboo_data::state_fluid(state).fluid == fluid::EMPTY
}

/// 按状态 ID 判断 `BlockState.isAir()`。
pub fn is_air(state: u32) -> bool {
    mc_bamboo_data::state_facts(state) & mc_bamboo_data::facts::AIR != 0
}

/// `BiomeManager.getFiddledDistance`。
fn fiddled_distance(seed: i64, x: i32, y: i32, z: i32, fx: f64, fy: f64, fz: f64) -> f64 {
    let mut value = seed;
    for coordinate in [x, y, z, x, y, z] {
        value = lcg_next(value, i64::from(coordinate));
    }
    let dx = fiddle(value);
    value = lcg_next(value, seed);
    let dy = fiddle(value);
    value = lcg_next(value, seed);
    let dz = fiddle(value);
    (fx + dx) * (fx + dx) + (fy + dy) * (fy + dy) + (fz + dz) * (fz + dz)
}

/// `LinearCongruentialGenerator.next(seed, salt)`。
fn lcg_next(state: i64, salt: i64) -> i64 {
    state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407)
        .wrapping_mul(state)
        .wrapping_add(salt)
}

fn fiddle(value: i64) -> f64 {
    let shifted = ((value as u64) >> 24) as i64;
    ((shifted.rem_euclid(1024) as f64) / 1024.0 - 0.5) * 0.9
}

/// `Climate.Sampler.sample` 的名字重导出，省得调用方再列六个采样器。
pub fn climate_sampler<'a>(samplers: &'a crate::settings::Samplers) -> Sampler<'a> {
    Sampler {
        temperature: &samplers.temperature,
        humidity: &samplers.vegetation,
        continentalness: &samplers.continents,
        erosion: &samplers.erosion,
        depth: &samplers.depth,
        weirdness: &samplers.ridges,
    }
}

/// 让 `DensitySampler` 在这个模块里可见，方便上面的类型别名。
#[allow(dead_code)]
type _DensitySampler = DensitySampler;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obfuscate_seed_matches_guava() {
        // Guava `Hashing.sha256().hashLong(0).asLong()`。
        assert_eq!(obfuscate_seed(0), 0x7A_0B_81_A1_F5_70_55_AF_u64 as i64);
    }
}
