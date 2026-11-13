//! 开/关缓存的方块逻辑阶段耗时。
//!
//! ```bash
//! cargo run --release -p mc-bamboo-hit-rate --bin cache_bench -- [刻数] [轮数]
//! ```
//!
//! 每轮造两份新世界，取最小值；开缓存那一侧包含守卫和录制开销。

use mc_bamboo_blocks::cache::{CacheConfig, RedstoneCache};
use mc_bamboo_hit_rate::machines;
use std::time::{Duration, Instant};

fn run(build: fn() -> mc_bamboo_hit_rate::world::Machine, ticks: i64, cached: bool) -> Duration {
    let mut m = build();
    if cached {
        m.cache = Some(RedstoneCache::new(CacheConfig::immediate()));
    }
    let started = Instant::now();
    for now in 1..=ticks {
        m.step(now, false);
    }
    started.elapsed()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ticks: i64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(4000);
    let rounds: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
    for build in machines::all() {
        let name = build().name;
        let mut plain = Duration::MAX;
        let mut cached = Duration::MAX;
        for _ in 0..rounds {
            plain = plain.min(run(build, ticks, false));
            cached = cached.min(run(build, ticks, true));
        }
        println!(
            "{name}: 关 {:.3} ms / 开 {:.3} ms / {:.2}× ({:.1} → {:.1} ns/tick)",
            plain.as_secs_f64() * 1e3,
            cached.as_secs_f64() * 1e3,
            plain.as_secs_f64() / cached.as_secs_f64(),
            plain.as_secs_f64() * 1e9 / ticks as f64,
            cached.as_secs_f64() * 1e9 / ticks as f64,
        );
    }
}
