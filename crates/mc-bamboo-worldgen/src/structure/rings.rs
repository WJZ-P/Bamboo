//! `BiomeSource.findBiomeHorizontal` 和 `ChunkGeneratorStructureState.generateRingPositions`
//! 的移植（要塞 `concentric_rings` 的群系吸附）。
//!
//! 群系判定走 `MultiNoiseBiomeSource.createResolver` 那条单点路径：`Climate.Sampler.sample`
//! 出 `TargetPoint`，再用参数表的 R 树查出群系名。`findBiomeHorizontal` 的搜索顺序、
//! `random.nextInt(found + 1)` 的取数、环几何里 `fork()` 消费的 `nextLong()` 都照官方。
//!
//! Port of `BiomeSource.findBiomeHorizontal` and
//! `ChunkGeneratorStructureState.generateRingPositions` (stronghold biome snapping).
//! Biome lookup uses the single-point `MultiNoiseBiomeSource.createResolver` path:
//! `Climate.Sampler.sample` -> `TargetPoint` -> parameter-list R-tree. The search order,
//! `random.nextInt(found + 1)` draws and the `fork()` `nextLong()` consumption follow vanilla.

use std::collections::HashSet;

use mc_bamboo_blocks::LegacyRandom;
use mc_bamboo_data::BIOME_CLIMATES;

use crate::{
    biome::MultiNoiseBiomeSource, settings::SeededRouter,
    structure::registry::resolve_biome_holders,
};

/// `MultiNoiseBiomeSource.createResolver(climateSampler)` 的单点群系查询。
///
/// `MultiNoiseBiomeSource.createResolver(climateSampler)`: single-point biome lookup.
pub struct BiomeResolver<'a> {
    pub router: &'a mut SeededRouter,
    pub source: &'a MultiNoiseBiomeSource,
}

impl BiomeResolver<'_> {
    /// `BiomeResolver.getNoiseBiome(quartX, quartY, quartZ)`。
    pub fn biome_at(&mut self, quart_x: i32, quart_y: i32, quart_z: i32) -> String {
        let target = self.router.climate(quart_x, quart_y, quart_z);
        self.source.biome(target).to_owned()
    }
}

/// 把 `#minecraft:stronghold_biased_to` 这类群系标签展开成群系名集合。
///
/// Expand a biome tag such as `#minecraft:stronghold_biased_to` into biome names.
pub fn resolve_allowed_biomes(tag: &str) -> Result<HashSet<String>, String> {
    let ids = resolve_biome_holders(&serde_json::Value::String(tag.to_owned()))?;
    Ok(ids
        .iter()
        .filter_map(|&id| {
            BIOME_CLIMATES
                .get(id as usize)
                .map(|biome| biome.name.to_owned())
        })
        .collect())
}

/// `findBiomeHorizontal` 的坐标 / 范围参数，避免函数参数过多。
///
/// Coordinate / range parameters of `findBiomeHorizontal`, to keep the arity small.
#[derive(Debug, Clone, Copy)]
pub struct FindBiomeParams {
    pub origin_x: i32,
    pub origin_y: i32,
    pub origin_z: i32,
    pub search_radius: i32,
    pub skip_steps: i32,
    pub find_closest: bool,
}

/// `BiomeSource.findBiomeHorizontal`：从 `originX/originZ` 起在 `searchRadius` 范围内找
/// `allowed` 允许的群系，用蓄水池抽样（`nextInt(found+1) == 0`）随机保留一个。
///
/// `findClosest` 为 false 时 `startRadius = noiseRadius`，所以 while 循环只跑一轮
/// `currentRadius == noiseRadius`；这里保留通用形式，和官方字节码一致。
///
/// `BiomeSource.findBiomeHorizontal`: search for an allowed biome within `searchRadius`,
/// keeping one via reservoir sampling (`nextInt(found+1) == 0`). With `findClosest == false`
/// `startRadius = noiseRadius`, so the loop runs once; the general form matches the bytecode.
pub fn find_biome_horizontal(
    resolver: &mut BiomeResolver<'_>,
    params: FindBiomeParams,
    allowed: &HashSet<String>,
    random: &mut LegacyRandom,
) -> Option<(i32, i32, String)> {
    let noise_center_x = params.origin_x >> 2;
    let noise_center_z = params.origin_z >> 2;
    let noise_radius = params.search_radius >> 2;
    let noise_y = params.origin_y >> 2;
    let mut result: Option<(i32, i32, String)> = None;
    let mut found = 0_i32;
    let mut current_radius = if params.find_closest { 0 } else { noise_radius };
    while current_radius <= noise_radius {
        let mut z = -current_radius;
        while z <= current_radius {
            let z_edge = z.abs() == current_radius;
            let mut x = -current_radius;
            while x <= current_radius {
                if params.find_closest {
                    let x_edge = x.abs() == current_radius;
                    if !x_edge && !z_edge {
                        x += params.skip_steps;
                        continue;
                    }
                }
                let noise_x = noise_center_x + x;
                let noise_z = noise_center_z + z;
                let biome = resolver.biome_at(noise_x, noise_y, noise_z);
                if allowed.contains(&biome) {
                    // `result == null` 时官方短路，不取随机数；这里必须一样。
                    // Vanilla short-circuits when result == null (no draw); mirror that.
                    if result.is_none() || random.next_int(found + 1) == 0 {
                        let position = (noise_x << 2, noise_z << 2, biome.clone());
                        if params.find_closest {
                            return Some(position);
                        }
                        result = Some(position);
                    }
                    found += 1;
                }
                x += params.skip_steps;
            }
            z += params.skip_steps;
        }
        current_radius += params.skip_steps;
    }
    result
}

/// `ChunkGeneratorStructureState.generateRingPositions`：算出 `count` 个要塞候选区块。
///
/// 主随机源是 `RandomSource.create().setSeed(concentricRingsSeed)`（普通世界 =
/// levelSeed，LegacyRandomSource）。每个候选点先算确定性几何 `(initialX, initialZ)`，
/// 再 `random.fork()`（消费一次 `nextLong`）当 `findBiomeHorizontal` 的搜索随机源；
/// 命中群系则把结果坐标换回区块坐标，否则用未吸附的初始区块坐标。
///
/// `ChunkGeneratorStructureState.generateRingPositions`: the `count` stronghold chunks.
/// Main source is `RandomSource.create().setSeed(concentricRingsSeed)` (levelSeed for
/// normal worlds). Each point gets deterministic geometry, then `random.fork()` (one
/// `nextLong`) seeds the biome search; a hit is converted back to chunk coords, a miss
/// keeps the unsnapped initial chunk coords.
pub fn ring_positions(
    seed: i64,
    resolver: &mut BiomeResolver<'_>,
    allowed: &HashSet<String>,
    distance: i32,
    spread: i32,
    count: i32,
) -> Vec<(i32, i32)> {
    rings(seed, (distance, spread, count), |params, search| {
        find_biome_horizontal(resolver, params, allowed, search).map(|(x, z, _)| (x, z))
    })
}

/// 固定群系用零种子，`FixedBiomeSource.findBiomeHorizontal` 直接抽取两个坐标。
///
/// Fixed biomes use ring seed zero and draw two coordinates in FixedBiomeSource.findBiomeHorizontal.
pub(super) fn flat_ring_positions(
    biome: &str,
    allowed: &HashSet<String>,
    dimensions: (i32, i32, i32),
) -> Vec<(i32, i32)> {
    let matches = allowed.contains(biome);
    rings(0, dimensions, |params, search| {
        matches.then(|| {
            let r = params.search_radius;
            (
                params.origin_x - r + search.next_int(r * 2 + 1),
                params.origin_z - r + search.next_int(r * 2 + 1),
            )
        })
    })
}

fn rings(
    seed: i64,
    (distance, spread, count): (i32, i32, i32),
    mut find: impl FnMut(FindBiomeParams, &mut LegacyRandom) -> Option<(i32, i32)>,
) -> Vec<(i32, i32)> {
    let main = LegacyRandom::new(seed);
    let mut angle = main.next_double() * std::f64::consts::PI * 2.0;
    let mut positions = Vec::with_capacity(count.max(0) as usize);
    let mut circle = 0_i32;
    let mut position_in_circle = 0_i32;
    let mut spread = spread.max(1);
    for i in 0..count {
        let dist = 4.0 * f64::from(distance)
            + f64::from(distance * circle) * 6.0
            + (main.next_double() - 0.5) * f64::from(distance) * 2.5;
        // `Math.round(double)` = `floor(a + 0.5)`，不是 Rust 的 round。
        // `Math.round(double)` = `floor(a + 0.5)`, not Rust's round.
        let initial_x = (angle.cos() * dist + 0.5).floor() as i32;
        let initial_z = (angle.sin() * dist + 0.5).floor() as i32;
        // `random.fork()` 消费一次 nextLong()；即便本轮不跑吸附也要推进主随机源。
        // `random.fork()` consumes one nextLong(); advance the main source regardless.
        let fork_seed = main.next_long();
        let origin_x = (initial_x << 4) + 8;
        let origin_z = (initial_z << 4) + 8;
        let mut search = LegacyRandom::new(fork_seed);
        let params = FindBiomeParams {
            origin_x,
            origin_y: 0,
            origin_z,
            search_radius: 112,
            skip_steps: 1,
            find_closest: false,
        };
        match find(params, &mut search) {
            Some((block_x, block_z)) => positions.push((block_x >> 4, block_z >> 4)),
            None => positions.push((initial_x, initial_z)),
        }
        angle += (std::f64::consts::PI * 2.0) / f64::from(spread);
        position_in_circle += 1;
        if position_in_circle == spread {
            circle += 1;
            position_in_circle = 0;
            spread += 2 * spread / (circle + 1);
            spread = spread.min(count - i);
            angle += main.next_double() * std::f64::consts::PI * 2.0;
        }
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stronghold_tag_resolves_to_many_biomes() {
        let allowed = resolve_allowed_biomes("#minecraft:stronghold_biased_to").unwrap();
        assert!(allowed.len() > 20);
        assert!(allowed.contains("minecraft:plains"));
    }

    #[test]
    fn ring_positions_are_deterministic_and_counted() {
        let worldgen = crate::Worldgen::load().expect("worldgen data loads");
        let allowed = resolve_allowed_biomes("#minecraft:stronghold_biased_to").unwrap();
        let settings = worldgen.settings("minecraft:overworld").unwrap();
        let source = worldgen.preset("minecraft:overworld").unwrap();

        let mut router = SeededRouter::new(settings, 42);
        let mut resolver = BiomeResolver {
            router: &mut router,
            source,
        };
        let first = ring_positions(42, &mut resolver, &allowed, 32, 3, 128);
        assert_eq!(first.len(), 128);

        // 换一个全新的 router 重算，结果必须一致。
        // A fresh router must give the same result.
        source.reset();
        let mut router2 = SeededRouter::new(settings, 42);
        let mut resolver2 = BiomeResolver {
            router: &mut router2,
            source,
        };
        assert_eq!(
            first,
            ring_positions(42, &mut resolver2, &allowed, 32, 3, 128)
        );
    }

    #[test]
    fn find_biome_horizontal_respects_search_radius() {
        let worldgen = crate::Worldgen::load().expect("worldgen data loads");
        let allowed = resolve_allowed_biomes("#minecraft:stronghold_biased_to").unwrap();
        let settings = worldgen.settings("minecraft:overworld").unwrap();
        let source = worldgen.preset("minecraft:overworld").unwrap();
        let mut router = SeededRouter::new(settings, 0);
        let mut resolver = BiomeResolver {
            router: &mut router,
            source,
        };
        let mut random = LegacyRandom::new(123);
        let params = FindBiomeParams {
            origin_x: 0,
            origin_y: 0,
            origin_z: 0,
            search_radius: 112,
            skip_steps: 1,
            find_closest: false,
        };
        let result = find_biome_horizontal(&mut resolver, params, &allowed, &mut random);
        if let Some((x, z, _)) = result {
            assert!(x.abs() <= 112 && z.abs() <= 112, "{x},{z}");
        }
    }
}
