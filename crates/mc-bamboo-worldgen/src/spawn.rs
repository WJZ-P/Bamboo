//! 官方 `NoiseBasedChunkGenerator.getOrigin` 的出生区块搜索。
//!
//! 26.3 里 `NoiseGeneratorSettings.spawnTarget` 非空时，`getOrigin(randomState)` 走
//! `NoiseSpawnFinder.findSpawnPosition(spawnTarget, randomState.samplersWithContext(...))`：
//! 先算 (0,0) 的适应度，再按两条半径递增的螺旋各扫一遍，取适应度最小的方块位置，
//! 最后 `ChunkPos.containing(...)` 得到出生区块。`SpawnTargetPoint.sampleFitness` 对
//! 每个气候密度函数取样、`Climate.quantizeCoord` 量化，累加到参数区间距离的平方。
//!
//! spawnTarget 用的五个函数正好是路由的 temperature / vegetation / continents /
//! erosion / ridges 输出（`OverworldBiomeBuilder.spawnTarget` 和
//! `NoiseRouterData.overworld` 用同一批 `DensityFunction`），所以这里复用
//! `SeededRouter::climate` 一次算出六个量化气候值，避免重复取样。
//!
//! Fresh-world spawn chunk search for vanilla `NoiseBasedChunkGenerator.getOrigin`.
//! When `NoiseGeneratorSettings.spawnTarget` is non-empty, `getOrigin(randomState)`
//! runs `NoiseSpawnFinder.findSpawnPosition(spawnTarget, samplers)`: compute the
//! (0,0) fitness, then two radius-increasing spirals, keeping the block position
//! with the smallest fitness, and finally `ChunkPos.containing(...)`.
//! `SpawnTargetPoint.sampleFitness` samples each climate density function, runs
//! `Climate.quantizeCoord`, and sums the squared distance to a parameter range.
//! The five spawn-target functions are exactly the router's temperature /
//! vegetation / continents / erosion / ridges outputs (both
//! `OverworldBiomeBuilder.spawnTarget` and `NoiseRouterData.overworld` use the
//! same `DensityFunction`s), so we reuse `SeededRouter::climate` to compute the
//! six quantized climate values once per candidate.

use serde_json::{Map, Value};

use crate::{
    Worldgen,
    climate::{TargetPoint, quantize},
    settings::SeededRouter,
};

/// `NoiseSpawnFinder.MAX_RADIUS`；也用来做距离偏置 `Mth.square(2048L)`。
/// `NoiseSpawnFinder.MAX_RADIUS`; also the distance bias `Mth.square(2048L)`.
const MAX_RADIUS: i64 = 2048;
/// `Mth.square(2048L)`。
/// `Mth.square(2048L)`.
const DISTANCE_SCALE: i64 = MAX_RADIUS * MAX_RADIUS;

/// `Climate.TargetPoint` 六个字段按 `[temperature, humidity, continentalness,
/// erosion, depth, weirdness]` 排成数组，方便按参数下标取值。
/// The six `Climate.TargetPoint` fields as an array, for indexing by parameter.
fn point_values(point: TargetPoint) -> [i64; 6] {
    [
        point.temperature,
        point.humidity,
        point.continentalness,
        point.erosion,
        point.depth,
        point.weirdness,
    ]
}

/// spawn_target 里密度函数名字 → `point_values` 的下标。
/// A spawn_target density function name → index into `point_values`.
fn param_index(name: &str) -> Option<usize> {
    match name.rsplit('/').next()? {
        "temperature" => Some(0),
        "vegetation" => Some(1),
        "continents" => Some(2),
        "erosion" => Some(3),
        "ridges" => Some(5),
        _ => None,
    }
}

/// 一个 `SpawnTargetPoint` 里的 `Climate.Parameter` 区间（定点数）。
/// One `Climate.Parameter` interval (quantized) of a `SpawnTargetPoint`.
#[derive(Clone, Copy)]
struct Target {
    index: usize,
    min: i64,
    max: i64,
}

/// `Climate.Parameter.distance(target)`。
/// `Climate.Parameter.distance(target)`.
fn distance(param: Target, target: i64) -> i64 {
    let above = target - param.max;
    let below = param.min - target;
    if above > 0 { above } else { below.max(0) }
}

/// `NoiseSpawnFinder.Result`：位置 + 适应度。
/// `NoiseSpawnFinder.Result`: location + fitness.
#[derive(Clone, Copy)]
struct Candidate {
    x: i32,
    z: i32,
    fitness: i64,
}

/// 从导出的 `noise_settings.<id>.spawn_target` 解析出每个目标点的参数区间。
/// `Climate.Parameter.CODEC` 用 `floatRange` 读 min/max，再 `quantizeCoord`；
/// 这里把 JSON 数先转成 f32 再走 `climate::quantize`，和官方一致。
///
/// Parse each target point's parameter intervals from the exported
/// `noise_settings.<id>.spawn_target`. `Climate.Parameter.CODEC` reads min/max
/// with `floatRange` then `quantizeCoord`; we convert the JSON number to f32
/// first and then `climate::quantize`, matching vanilla.
fn parse_targets(raw: &Map<String, Value>, settings_id: &str) -> Result<Vec<Vec<Target>>, String> {
    let settings = raw
        .get("noise_settings")
        .and_then(Value::as_object)
        .and_then(|m| m.get(settings_id))
        .and_then(Value::as_object)
        .ok_or_else(|| format!("missing noise_settings entry {settings_id}"))?;
    let list = settings
        .get("spawn_target")
        .and_then(Value::as_array)
        .ok_or("missing spawn_target")?;
    let mut result = Vec::with_capacity(list.len());
    for point in list {
        let object = point.as_object().ok_or("bad spawn_target point")?;
        let mut targets = Vec::with_capacity(object.len());
        for (name, range) in object {
            let index =
                param_index(name).ok_or_else(|| format!("unknown spawn target function {name}"))?;
            let bounds = range.as_array().ok_or("bad spawn_target parameter")?;
            if bounds.len() != 2 {
                return Err("spawn_target parameter is not a [min, max] pair".to_owned());
            }
            let min = quantize(bounds[0].as_f64().ok_or("bad parameter min")? as f32);
            let max = quantize(bounds[1].as_f64().ok_or("bad parameter max")? as f32);
            targets.push(Target { index, min, max });
        }
        // `sampleFitness` 遍历 `Map.entrySet()`，但适应度是平方和、对顺序不敏感。
        // `sampleFitness` walks `Map.entrySet()`; the fitness is a sum of squares,
        // so the order does not change the result.
        result.push(targets);
    }
    Ok(result)
}

/// `NoiseSpawnFinder.getSpawnPositionAndFitness(samplers, points, blockX, blockZ)`。
///
/// 官方先把 blockX/blockZ 对齐到 quart 方块（`QuartPos.toBlock(fromBlock(x))`），
/// 再对每个 `SpawnTargetPoint` 取样；`distanceBiasToWorldOrigin` 用的是**绝对**方块坐标。
///
/// `NoiseSpawnFinder.getSpawnPositionAndFitness`. Vanilla snaps blockX/blockZ to
/// quart blocks (`QuartPos.toBlock(fromBlock(x))`) before sampling each point;
/// `distanceBiasToWorldOrigin` uses the **absolute** block coordinates.
fn fitness(router: &mut SeededRouter, targets: &[Vec<Target>], x: i32, z: i32) -> Candidate {
    let values = point_values(router.climate(x >> 2, 0, z >> 2));
    let mut min_fitness = i64::MAX;
    for point in targets {
        let mut fitness = 0_i64;
        for &param in point {
            let d = distance(param, values[param.index]);
            fitness += d * d; // `Mth.square(long)`
        }
        min_fitness = min_fitness.min(fitness);
    }
    let distance_bias = (x as i64) * (x as i64) + (z as i64) * (z as i64);
    Candidate {
        x,
        z,
        fitness: min_fitness * DISTANCE_SCALE + distance_bias,
    }
}

/// `NoiseSpawnFinder.radialSearch`：从 `best` 的位置出发，按螺旋取候选，严格更优就换。
/// `angle` / `radius` 是 float，`Math.sin` / `Math.cos` 收 double；`(int)` 截断。
///
/// `NoiseSpawnFinder.radialSearch`: spiral candidates around `best`, replace on a
/// strictly smaller fitness. `angle` / `radius` are float, `Math.sin` / `Math.cos`
/// take doubles; `(int)` truncates toward zero.
fn radial_search(
    router: &mut SeededRouter,
    targets: &[Vec<Target>],
    max_radius: f32,
    increment: f32,
    best: &mut Candidate,
) {
    // `searchOrigin = this.result.location()` 在每次 radialSearch 开始时固定下来。
    // `searchOrigin = this.result.location()` is fixed at the start of each call.
    let origin_x = best.x;
    let origin_z = best.z;
    let mut angle = 0.0_f32;
    let mut radius = increment;
    while radius <= max_radius {
        let x = origin_x + (f64::sin(angle as f64) * radius as f64) as i32;
        let z = origin_z + (f64::cos(angle as f64) * radius as f64) as i32;
        let candidate = fitness(router, targets, x, z);
        if candidate.fitness < best.fitness {
            *best = candidate;
        }
        angle += increment / radius;
        if (angle as f64) > std::f64::consts::PI * 2.0 {
            angle = 0.0;
            radius += increment;
        }
    }
}

/// 返回 `NoiseBasedChunkGenerator.getOrigin(randomState)` 的出生区块 `(x, z)`。
///
/// `spawnTarget` 为空时退回基类 `ChunkGenerator.getOrigin` 的 `ChunkPos.ZERO`。
/// 两种半径的螺旋顺序照官方构造器的 `2048/512` 再 `512/32`。
///
/// Return the spawn chunk `(x, z)` from `NoiseBasedChunkGenerator.getOrigin`.
/// An empty `spawnTarget` falls back to the base `ChunkGenerator.getOrigin`
/// (`ChunkPos.ZERO`). The two spirals follow the constructor's `2048/512` then
/// `512/32` order.
pub fn spawn_origin(
    worldgen: &Worldgen,
    settings_id: &str,
    seed: i64,
) -> Result<(i32, i32), String> {
    let targets = parse_targets(worldgen.raw(), settings_id)?;
    if targets.is_empty() {
        return Ok((0, 0));
    }
    let settings = worldgen
        .settings(settings_id)
        .ok_or_else(|| format!("missing noise settings {settings_id}"))?;
    let mut router = SeededRouter::new(settings, seed);
    let mut best = fitness(&mut router, &targets, 0, 0);
    radial_search(&mut router, &targets, 2048.0, 512.0, &mut best);
    radial_search(&mut router, &targets, 512.0, 32.0, &mut best);
    Ok((best.x >> 4, best.z >> 4))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方 `generator.getOrigin(randomState)` 在多个种子、三种噪声设置下的出生区块。
    /// 由 `OriginCheck.java`（临时工具，跑完即删）用官方 `NoiseSpawnFinder` 算出。
    /// Spawn chunks from official `generator.getOrigin(randomState)` across seeds
    /// and the three noise settings, produced by `OriginCheck.java`.
    #[test]
    fn spawn_origin_matches_vanilla_get_origin() {
        let worldgen = Worldgen::load().expect("worldgen data loads");
        let cases: &[(&str, i64, i32, i32)] = &[
            ("minecraft:overworld", 0, -2, 0),
            ("minecraft:overworld", 1, 10, 10),
            ("minecraft:overworld", -1, 31, 24),
            ("minecraft:overworld", 12345, 6, -2),
            ("minecraft:overworld", 999, 0, -1),
            ("minecraft:overworld", -999, 8, -48),
            ("minecraft:overworld", 1234567890123456789, 0, 0),
            ("minecraft:overworld", -8765432109876543210, 0, 0),
            ("minecraft:large_biomes", 0, -7, -65),
            ("minecraft:large_biomes", 1, 57, -78),
            ("minecraft:large_biomes", -1, 1, 0),
            ("minecraft:large_biomes", 12345, 5, -44),
            ("minecraft:large_biomes", 999, 0, -1),
            ("minecraft:large_biomes", -999, -73, 13),
            ("minecraft:large_biomes", 1234567890123456789, 0, 0),
            ("minecraft:large_biomes", -8765432109876543210, 0, 0),
            ("minecraft:amplified", 0, -2, 0),
            ("minecraft:amplified", 1, 10, 10),
            ("minecraft:amplified", -1, 31, 24),
            ("minecraft:amplified", 12345, 6, -2),
            ("minecraft:amplified", 999, 0, -1),
            ("minecraft:amplified", -999, 8, -48),
            ("minecraft:amplified", 1234567890123456789, 0, 0),
            ("minecraft:amplified", -8765432109876543210, 0, 0),
        ];
        for &(id, seed, x, z) in cases {
            let got = spawn_origin(&worldgen, id, seed).expect("origin");
            assert_eq!(got, (x, z), "settings {id} seed {seed}");
        }
    }
}
