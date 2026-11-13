//! 测 `Generator::terrain` 三个阶段的单区块耗时。
//! 用 `cargo run --release -p mc-bamboo-worldgen --example chunk_timing` 跑。
//!
//! 每个种子先生成一次让指令缓存热起来，再对若干区块计时，报中位数。

use std::{hint::black_box, time::Instant};

use mc_bamboo_worldgen::{
    Worldgen,
    chunk::{Generator, Stage},
};

fn main() {
    let worldgen = Worldgen::load().expect("能读数据");
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
    let stages = [Stage::Noise, Stage::Surface, Stage::Carvers];
    let mut all = Vec::new();
    for seed in seeds {
        let generator = Generator::new(&worldgen, seed).expect("能建生成器");
        // 预热：编译采样器、填一次格子缓存。
        let _ = black_box(generator.terrain(0, 0, Stage::Carvers)).expect("能生成");
        let mut times = Vec::new();
        for stage in stages {
            for (x, z) in chunks {
                let start = Instant::now();
                let layers = black_box(generator.terrain(x, z, stage)).expect("能生成");
                times.push((stage, start.elapsed().as_secs_f64() * 1000.0));
                black_box(layers.len());
            }
        }
        for stage in stages {
            let mut values: Vec<f64> = times
                .iter()
                .filter(|(measured, _)| *measured == stage)
                .map(|(_, value)| *value)
                .collect();
            values.sort_by(f64::total_cmp);
            let median = values[values.len() / 2];
            println!("seed {seed:>20} {stage:?}: median {median:.2} ms/chunk");
        }
        all.extend(times);
    }
    for stage in stages {
        let mut values: Vec<f64> = all
            .iter()
            .filter(|(measured, _)| *measured == stage)
            .map(|(_, value)| *value)
            .collect();
        values.sort_by(f64::total_cmp);
        let median = values[values.len() / 2];
        println!(
            "{stage:?}: all {} chunks median {:.2} ms, min {:.2} ms, max {:.2} ms",
            values.len(),
            median,
            values[0],
            values[values.len() - 1]
        );
    }
}
