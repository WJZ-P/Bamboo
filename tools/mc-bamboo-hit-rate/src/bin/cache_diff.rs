//! 九台机器的开/关缓存逐刻差分。
//!
//! ```bash
//! cargo run --release -p mc-bamboo-hit-rate --bin cache_diff -- [刻数]
//! ```
//!
//! 逐刻比较世界状态、对外效果（含顺序和 pitch 的位模式）、发包层的音效取数次数、
//! 世界随机源种子和计划刻队列。任何一项不等就打印机器名、刻号和两份差异。

use mc_bamboo_blocks::cache::{CacheConfig, RedstoneCache};
use mc_bamboo_hit_rate::{machines, world::Machine};

fn main() {
    let ticks: i64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4000);
    let mut total_mismatches = 0;
    for build in machines::all() {
        let name = build().name;
        let mut cached = build();
        cached.cache = Some(RedstoneCache::new(CacheConfig::immediate()));
        let mut plain = build();
        let mut mismatches = 0;
        let mut first = None;
        for now in 1..=ticks {
            cached.step(now, false);
            plain.step(now, false);
            let same = cached.level.snapshot(&cached.ticks) == plain.level.snapshot(&plain.ticks)
                && cached.effects == plain.effects
                && cached.sound_draws == plain.sound_draws
                && cached.level.random.source().seed() == plain.level.random.source().seed();
            if !same {
                mismatches += 1;
                if first.is_none() {
                    first = Some(now);
                }
            }
        }
        let stats = cached.cache.as_ref().unwrap().stats();
        println!(
            "{name}: {mismatches} 刻不一致（首次 {first:?}），触发 {} / 命中 {} / 记录 {} / 拒绝 {} / 淘汰 {} / 停录点 {} / 缓存 {} KiB",
            stats.triggers,
            stats.replays,
            stats.recorded,
            stats.rejected,
            stats.evictions,
            stats.stopped_sites,
            cached.cache.as_ref().unwrap().bytes() / 1024,
        );
        total_mismatches += mismatches;
    }
    println!("合计：{total_mismatches} 刻不一致（应为 0）");
    assert_eq!(total_mismatches, 0, "缓存与解释执行不一致");
    let _ = Machine::compare(machines::repeater_clock, 1);
}
