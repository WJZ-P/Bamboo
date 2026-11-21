//! 地形缓存「调色板压缩」的测量：省多少内存、编码 / 解码各花多久。
//!
//! `cargo run --release -p mc-bamboo-game --example terrain_compact_bench -- \
//!     [--chunks N] [--seed S] [--repeats R]`
//!
//! 量的是地形缓存冷层用的 `worldgen_compact::CompactTerrain`（结论和数字见
//! `docs/performance-optimizations.md` 第 23 节）。
//!
//! 对每个区块核对「解码 == 原始」，然后打印一行 CSV：原始 / 压缩后的堆字节、
//! 编码 / 解码 / 整块 `clone` 的单次耗时中位数。`clone` 是参照：缓存现在命中时
//! 只克隆一个 `Arc`，从冷层取回时要解码出一份完整的方块数组。
//!
//! Measures `worldgen_compact::CompactTerrain`, the terrain cache's cold-tier
//! encoding: memory saved versus encode / decode cost (numbers and decision in
//! section 23 of `performance-optimizations.md`). Every chunk is
//! checked to round-trip, then one CSV row reports raw / compact heap bytes and
//! the median encode / decode / full-clone time. The clone is the reference: a
//! hot hit clones one `Arc` while a cold hit decodes a whole block array.

use std::{env, hint::black_box, time::Instant};

use mc_bamboo_game::{
    world_options::{LevelType, WorldGenConfig},
    worldgen::{generate_chunk, generated_chunk_heap_bytes},
    worldgen_compact::CompactTerrain,
};

fn median_us(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn time_us<T>(repeats: usize, mut run: impl FnMut() -> T) -> f64 {
    let samples = (0..repeats)
        .map(|_| {
            let started = Instant::now();
            black_box(run());
            started.elapsed().as_secs_f64() * 1_000_000.0
        })
        .collect();
    median_us(samples)
}

fn main() {
    let mut side = 4_i32;
    let mut seed = 2026_i64;
    let mut repeats = 31_usize;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .unwrap_or_else(|| panic!("missing value for {arg}"))
        };
        match arg.as_str() {
            "--chunks" => side = value().parse().expect("number"),
            "--seed" => seed = value().parse().expect("number"),
            "--repeats" => repeats = value().parse().expect("number"),
            _ => panic!("unknown argument {arg}"),
        }
    }
    let config = WorldGenConfig {
        seed,
        level_type: LevelType::Normal,
        ..WorldGenConfig::default()
    };
    let (mut raw, mut compact_bytes) = (0_usize, 0_usize);
    let (mut encode_us, mut decode_us, mut clone_us) = (Vec::new(), Vec::new(), Vec::new());
    for x in 0..side {
        for z in 0..side {
            // 隔 7 个区块取一个，覆盖几种地形而不是一整片相邻的。
            // Sample every 7th chunk to cover several terrains, not one patch.
            let chunk = generate_chunk(&config, x * 7, z * 7).expect("terrain");
            let compact = CompactTerrain::encode(&chunk);
            assert_eq!(compact.decode().blocks, chunk.blocks, "round trip");
            raw += generated_chunk_heap_bytes(&chunk);
            compact_bytes += compact.heap_bytes();
            encode_us.push(time_us(repeats, || CompactTerrain::encode(&chunk)));
            decode_us.push(time_us(repeats, || compact.decode()));
            clone_us.push(time_us(repeats, || chunk.clone()));
        }
    }
    let count = (side * side) as usize;
    println!(
        "chunks={count},raw_bytes={},compact_bytes={},ratio={:.2},\
         encode_us={:.1},decode_us={:.1},clone_us={:.1}",
        raw / count,
        compact_bytes / count,
        raw as f64 / compact_bytes as f64,
        median_us(encode_us),
        median_us(decode_us),
        median_us(clone_us),
    );
}
