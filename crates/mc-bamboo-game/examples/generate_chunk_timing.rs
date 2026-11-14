//! 测游戏里那份 `worldgen::generate_chunk` 的耗时（release 构建）。
//!
//! `cargo run --release -p mc-bamboo-game --example generate_chunk_timing`。
//! 分别量：只算 BIOMES（`generate_biomes`）、到 CARVERS（`generate_chunk`，
//! 内部 = BIOMES + doFill → buildSurface → generateCarvers）、以及把结果组装成
//! 可交付 `RegionColumn`（`build_region_column`：PalettedContainer 重打包 +
//! 四张高度图重算）。每种先预热一次，再对若干区块取中位数。
//!
//! Measure the in-game `worldgen::generate_chunk` (release build). Reports the
//! BIOMES-only stage, the full CARVERS stage, and the RegionColumn assembly,
//! medians over several chunks after a warmup.

use std::{hint::black_box, time::Instant};

use mc_bamboo_game::{
    world_options::{LevelType, WorldGenConfig},
    worldgen::{build_region_column, generate_biomes, generate_chunk},
};

fn main() {
    let seeds = [0_i64, 1, -1, 1234567890123456789, -8765432109876543210];
    let chunks = [
        (0, 0),
        (1, 0),
        (100, 100),
        (-200, 375),
        (8, -13),
        (3, 5),
        (17, -9),
    ];
    let mut biomes = Vec::new();
    let mut carvers = Vec::new();
    let mut assembly = Vec::new();
    for seed in seeds {
        let config = WorldGenConfig {
            seed,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        // 预热：解析 Worldgen、建 Generator、填一次缓存。
        // Warmup: parse Worldgen, build the Generator, fill caches once.
        let warm = black_box(generate_chunk(&config, 0, 0)).expect("warmup chunk");
        black_box(build_region_column(&warm)).expect("warmup assembly");
        for &(x, z) in &chunks {
            let start = Instant::now();
            let b = black_box(generate_biomes(&config, x, z)).expect("biomes");
            biomes.push(start.elapsed().as_secs_f64() * 1000.0);
            black_box(b.blocks.len());

            let start = Instant::now();
            let g = black_box(generate_chunk(&config, x, z)).expect("chunk");
            carvers.push(start.elapsed().as_secs_f64() * 1000.0);

            let start = Instant::now();
            let column = black_box(build_region_column(&g)).expect("assembly");
            assembly.push(start.elapsed().as_secs_f64() * 1000.0);
            black_box(column.parts().column.chunk().sections().len());
        }
    }
    let carvers_median = median(&carvers);
    let assembly_median = median(&assembly);
    report("biomes-only", &biomes);
    report("to-carvers", &carvers);
    report("assembly", &assembly);
    println!(
        "to-carvers + assembly: {:.2} ms/chunk (medians added)",
        carvers_median + assembly_median
    );
}

fn report(label: &str, values: &[f64]) {
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    println!(
        "{label:>12}: {} chunks median {:.2} ms, min {:.2} ms, max {:.2} ms",
        values.len(),
        median(values),
        min,
        max
    );
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}
