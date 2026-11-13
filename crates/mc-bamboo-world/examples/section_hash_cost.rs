//! 量段哈希给 `Chunk::set_block` 加了多少开销。
//!
//! ```text
//! cargo run --release -p mc-bamboo-world --example section_hash_cost
//! ```
//!
//! 用 commit 历史里没有哈希的版本对比时，把 `Section::set_block` 里那次
//! `block_hash ^=` 注掉即可；两边跑同一份固定随机序列。

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use mc_bamboo_data::blocks;
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT, chunk::Chunk};

const WRITES: usize = 2_000_000;
const RUNS: usize = 5;

fn next(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *seed
}

fn run(write: bool) -> Duration {
    let mut chunk = Chunk::empty(0);
    let mut seed = 0x1234_5678_9abc_def0;
    let start = Instant::now();
    for _ in 0..WRITES {
        let value = next(&mut seed);
        let x = (value >> 16) as usize & 15;
        let z = (value >> 24) as usize & 15;
        let y = MIN_Y + ((value >> 32) as i32).rem_euclid(WORLD_HEIGHT);
        let state = if write {
            (value >> 48) as u32 % 64
        } else {
            blocks::AIR
        };
        chunk.set_block(x, y, z, state);
        black_box(chunk.sections()[0].block_count());
        black_box(chunk.sections()[0].block_hash());
    }
    start.elapsed()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let only_changing = args.first().is_some_and(|a| a == "changing");
    for write in [false, true] {
        if only_changing && !write {
            continue;
        }
        let mut best = Duration::MAX;
        for _ in 0..RUNS {
            best = best.min(run(write));
        }
        println!(
            "{}: {:.3} ms / {WRITES} writes ({:.1} ns/write)",
            if write {
                "changing states"
            } else {
                "writing air"
            },
            best.as_secs_f64() * 1e3,
            best.as_secs_f64() * 1e9 / WRITES as f64,
        );
    }
}
