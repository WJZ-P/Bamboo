//! 开/关缓存的方块逻辑阶段耗时。
//!
//! ```bash
//! cargo run --release -p mc-bamboo-hit-rate --bin cache_bench -- [刻数] [轮数] [immediate]
//! ```
//!
//! 第三个参数传 `immediate` 时用「第一次就录」的最坏情况配置作参考；默认用正式
//! 配置 `CacheConfig::enabled()`（按触发点算账，只在划算的地方缓存）。
//! 每轮造两份新世界，取最小值；开缓存那一侧包含守卫和录制开销。

use mc_bamboo_blocks::cache::{CacheConfig, CacheStats, RedstoneCache};
use mc_bamboo_hit_rate::machines;
use std::time::{Duration, Instant};

fn run(
    build: fn() -> mc_bamboo_hit_rate::world::Machine,
    ticks: i64,
    config: Option<CacheConfig>,
) -> (Duration, CacheStats) {
    let mut m = build();
    if let Some(config) = config {
        m.cache = Some(RedstoneCache::new(config));
    }
    let started = Instant::now();
    for now in 1..=ticks {
        m.step(now, false);
    }
    (
        started.elapsed(),
        m.cache
            .as_ref()
            .map_or_else(CacheStats::default, |cache| cache.stats()),
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ticks: i64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(4000);
    let rounds: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
    let immediate = args.get(2).is_some_and(|s| s == "immediate");
    let config = if immediate {
        CacheConfig::immediate()
    } else {
        CacheConfig::enabled()
    };
    for build in machines::all() {
        let name = build().name;
        let mut plain = Duration::MAX;
        let mut cached = Duration::MAX;
        let mut stats = CacheStats::default();
        for _ in 0..rounds {
            let (duration, _) = run(build, ticks, None);
            plain = plain.min(duration);
            let (duration, round_stats) = run(build, ticks, Some(config));
            if duration < cached {
                cached = duration;
                stats = round_stats;
            }
        }
        println!(
            "{name}: 关 {:.3} ms / 开 {:.3} ms / {:.2}× ({:.1} → {:.1} ns/tick) 触发 {} / 直达 {} / 命中 {} / 记录 {} / 停录点 {}",
            plain.as_secs_f64() * 1e3,
            cached.as_secs_f64() * 1e3,
            plain.as_secs_f64() / cached.as_secs_f64(),
            plain.as_secs_f64() * 1e9 / ticks as f64,
            cached.as_secs_f64() * 1e9 / ticks as f64,
            stats.triggers,
            stats.bypassed,
            stats.replays,
            stats.recorded,
            stats.stopped_sites,
        );
    }
}
