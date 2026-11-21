//! 真实 Director 流水线的吞吐和阶段耗时测量（不是 `wg-perf` 的纯函数路径）。
//!
//! `cargo run --release -p mc-bamboo-game --example director_pipeline_bench -- \
//!     [--chunks N] [--threads T] [--seed S] [--no-structures] [--cold] [--label NAME]`
//!
//! 起一个 `World::new`（`normal` 世界、`Staged` 流水线、计算池），强加载一块
//! 正方形区块，不睡节拍器地 tick 到全部交付且计算池排空。默认先在远处跑一片同样
//! 大小的区域把计算线程暖起来（线程本地的生成器 / 结构状态只建一次），`--cold` 不暖。量每个交付区块的墙钟、
//! 进程 CPU 时间，以及 `worldgen_profile` 里各阶段（terrain / features / build_view /
//! run_features / light）、地形缓存命中、Director 快照命中、`cached_feature_run` 的
//! 命中与「每个中心被以多少个不同中心调用」。所有数字按一行 CSV 打印，方便同轮
//! 交替 A/B 比较。

use std::{
    env,
    hint::black_box,
    time::{Duration, Instant},
};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_game::{
    MailBudget, World, WorldConfig,
    world_options::{LevelType, WorldGenConfig},
    worldgen::{clear_terrain_cache, terrain_cache_stats},
    worldgen_cache::{self, WorldgenCacheSettings},
    worldgen_features::{clear_feature_cache, feature_cache_usage},
    worldgen_profile,
};

#[derive(Debug)]
struct Options {
    side: usize,
    threads: Vec<usize>,
    seed: i64,
    structures: bool,
    warmup: bool,
    label: String,
    /// 交付完以后再按 50 ms 一刻空转多少秒，量闲置内存。
    /// Seconds to keep ticking at 50 ms after delivery, to measure idle memory.
    idle_secs: u64,
    /// 再强加载紧挨着的一片同样大小的区域，量边缘缓存复用后的吞吐。
    /// Also force the adjacent square, measuring throughput with frontier reuse.
    second: bool,
    /// 世界生成缓存的上限和空闲释放秒数（`--terrain-mb` / `--feature-mb` /
    /// `--cache-idle-secs`）。
    /// Worldgen cache limits and idle-release delay.
    cache: WorldgenCacheSettings,
}

impl Options {
    fn parse() -> Self {
        let mut options = Self {
            side: 5,
            threads: vec![1, 2, 4, 8],
            seed: 2026,
            structures: true,
            warmup: true,
            label: "run".to_owned(),
            idle_secs: 0,
            second: false,
            cache: WorldgenCacheSettings::default(),
        };
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            let mut value = || {
                args.next()
                    .unwrap_or_else(|| panic!("missing value for {arg}"))
            };
            match arg.as_str() {
                "--chunks" => options.side = value().parse().expect("number"),
                "--threads" => {
                    options.threads = value()
                        .split(',')
                        .map(|part| part.parse().expect("number"))
                        .collect();
                }
                "--seed" => options.seed = value().parse().expect("number"),
                "--no-structures" => options.structures = false,
                "--cold" => options.warmup = false,
                "--label" => options.label = value(),
                "--idle-secs" => options.idle_secs = value().parse().expect("number"),
                "--second" => options.second = true,
                "--terrain-mb" => options.cache.terrain_mb = value().parse().expect("number"),
                "--feature-mb" => options.cache.feature_mb = value().parse().expect("number"),
                "--cache-idle-secs" => {
                    options.cache.idle_seconds = value().parse().expect("number");
                }
                "--no-dependency-release" => worldgen_cache::set_dependency_release(false),
                _ => panic!("unknown argument {arg}"),
            }
        }
        options
    }
}

#[path = "support/process_cpu.rs"]
mod process_cpu;
use process_cpu::process_cpu_ms;
#[path = "support/process_memory.rs"]
mod process_memory;
use process_memory::process_memory_mb;

/// 当前线程的累计 CPU 毫秒（不含等待）。
///
/// Accumulated CPU milliseconds of the current thread, excluding waits.
#[cfg(unix)]
fn thread_cpu_ms() -> f64 {
    let mut time: libc::timespec = unsafe { std::mem::zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &raw mut time) } != 0 {
        return 0.0;
    }
    time.tv_sec as f64 * 1000.0 + time.tv_nsec as f64 / 1_000_000.0
}

#[cfg(windows)]
fn thread_cpu_ms() -> f64 {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};
    process_cpu::filetime_cpu_ms(|created, exited, kernel, user| {
        // 当前线程的伪句柄无需释放。
        // The current thread pseudo-handle does not need closing.
        unsafe { GetThreadTimes(GetCurrentThread(), created, exited, kernel, user) }
    })
}

/// tick 到这批区块全部交付、计算池排空再多跑 4 刻；返回期间交付的区块数。
/// Tick until every position is delivered and the pool drained (+4 ticks).
fn settle(world: &mut World, positions: &[ChunkPos], threads: usize) -> usize {
    let mut delivered = 0usize;
    let mut quiet = 0usize;
    let deadline = Instant::now() + Duration::from_secs(1800);
    loop {
        delivered += world.tick_unpaced();
        let all_loaded = positions.iter().all(|&pos| world.is_chunk_loaded(pos));
        let idle = world.compute_in_flight() == 0;
        if all_loaded && idle {
            quiet += 1;
            if quiet >= 4 {
                return delivered;
            }
        } else {
            quiet = 0;
        }
        assert!(
            Instant::now() < deadline,
            "pipeline did not settle in 1800 s (threads={threads})"
        );
    }
}

fn main() {
    let options = Options::parse();
    let total = options.side * options.side;
    let positions: Vec<ChunkPos> = (0..options.side)
        .flat_map(|x| (0..options.side).map(move |z| ChunkPos::new(x as i32, z as i32)))
        .collect();
    for &threads in &options.threads {
        clear_terrain_cache();
        clear_feature_cache();
        worldgen_profile::reset();
        let (_inbox, receiver) = mc_bamboo_game::inbound_channel();
        let config = WorldConfig {
            view_distance: 4,
            world_gen: WorldGenConfig {
                seed: options.seed,
                level_type: LevelType::Normal,
                generate_structures: options.structures,
                ..WorldGenConfig::default()
            },
            chunk_worker_threads: threads,
            worldgen_cache: options.cache,
            // 测量不限制每刻交付数，交付节奏不成为吞吐瓶颈。
            // No per-tick delivery cap: the pacing is not part of the measurement.
            mail_budget: MailBudget {
                deliveries: 0,
                reclaims: 0,
            },
            ..WorldConfig::default()
        };
        let mut world = World::new(config, receiver);
        if options.warmup {
            // 每条计算线程第一次生成时要建自己的生成器、结构状态和模板表（一次性，
            // 长跑的服务器只付一次）。先在远处跑一片同样大小的区域把线程暖起来，
            // 再清掉全进程缓存，量的就是稳态。`--cold` 保留这笔一次性开销。
            // Each compute thread builds its own generator / structure state /
            // templates on first use (one-off in a long-running server). Warm the
            // pool on a far-away area of the same size, then clear the global
            // caches so the measurement is steady state. `--cold` keeps the cost.
            let far: Vec<ChunkPos> = positions
                .iter()
                .map(|pos| ChunkPos::new(pos.x + 4096, pos.z + 4096))
                .collect();
            world.force_chunks_unpaced(&far);
            settle(&mut world, &far, threads);
            clear_terrain_cache();
            clear_feature_cache();
            worldgen_profile::reset();
        }
        world.force_chunks_unpaced(&positions);

        let wall_started = Instant::now();
        let cpu0 = process_cpu_ms();
        let world_cpu0 = thread_cpu_ms();
        let delivered = settle(&mut world, &positions, threads);
        let wall = wall_started.elapsed().as_secs_f64();
        let cpu_ms = process_cpu_ms() - cpu0;
        let world_cpu_ms = thread_cpu_ms() - world_cpu0;
        let p = worldgen_profile::stats();
        worldgen_profile::disable();
        let cache = terrain_cache_stats();
        let features = feature_cache_usage();
        let (rss_mb, _) = process_memory_mb();
        let second_per_s = if options.second {
            let side = options.side as i32;
            let next: Vec<ChunkPos> = positions
                .iter()
                .map(|pos| ChunkPos::new(pos.x + side, pos.z))
                .collect();
            world.force_chunks_unpaced(&next);
            let started = Instant::now();
            let count = settle(&mut world, &next, threads);
            count as f64 / started.elapsed().as_secs_f64()
        } else {
            0.0
        };
        let idle_until = Instant::now() + Duration::from_secs(options.idle_secs);
        while Instant::now() < idle_until {
            world.tick_unpaced();
            std::thread::sleep(Duration::from_millis(50));
        }
        let (idle_rss_mb, peak_rss_mb) = process_memory_mb();
        let after = terrain_cache_stats();
        let ns = |v: u64| v as f64 / 1_000_000.0;
        println!(
            "{},structures={},warm={},threads={},requested={},delivered={},wall_s={:.3},\
             delivered_per_s={:.2},wall_per_delivered_ms={:.3},cpu_per_delivered_ms={:.3},\
             pool_cpu_per_delivered_ms={:.3},world_cpu_ms={:.1},\
             terrain_ms={:.3},features_ms={:.3},light_ms={:.3},\
             build_view_ms={:.3},run_features_ms={:.3},view_resolve_ms={:.3},\
             terrain_stage_calls={},feature_stage_calls={},light_stage_calls={},\
             cache_hits={},cache_misses={},cache_hit_rate={:.4},\
             snapshot_some={},snapshot_none={},snapshot_rate={:.4},\
             terrain_lookups={},terrain_hits={},terrain_misses={},\
             view_terrain_misses={},build_view_terrain_misses={},\
             feature_run_calls={},feature_run_hits={},feature_run_misses={},feature_run_hit_rate={:.4},\
             distinct_feature_centers={},feature_centers_called_once={},feature_centers_max_calls={},\
             build_view_calls={},build_view_cells={},run_features_calls={},\
             entries={},bytes_mb={:.1},cross_thread_duplicates={},\
             rss_mb={:.0},peak_rss_mb={:.0},idle_rss_mb={:.0},second_per_s={:.2},\
             cache_peak_mb={:.1},idle_entries={},idle_bytes_mb={:.1},\
             dependency_releases={},evictions={},feature_entries={},feature_mb={:.1},\
             idle_releases={},cold_entries={},cold_mb={:.1},cold_hits={}",
            options.label,
            options.structures,
            options.warmup,
            threads,
            total,
            delivered,
            wall,
            delivered as f64 / wall,
            wall * 1000.0 / delivered as f64,
            cpu_ms / delivered as f64,
            (cpu_ms - world_cpu_ms) / delivered as f64,
            world_cpu_ms,
            ns(p.terrain_ns),
            ns(p.features_ns),
            ns(p.light_ns),
            ns(p.build_view_ns),
            ns(p.run_features_ns),
            ns(p.view_resolve_ns),
            p.terrain_calls,
            p.features_calls,
            p.light_calls,
            cache.hits,
            cache.misses,
            cache.hits as f64 / (cache.hits + cache.misses).max(1) as f64,
            p.snapshot_some,
            p.snapshot_none,
            p.snapshot_some as f64 / (p.snapshot_some + p.snapshot_none).max(1) as f64,
            p.terrain_lookups,
            p.terrain_hits,
            p.terrain_misses,
            p.view_terrain_misses,
            p.build_view_terrain_misses,
            p.feature_run_calls,
            p.feature_run_hits,
            p.feature_run_misses,
            p.feature_run_hits as f64 / p.feature_run_calls.max(1) as f64,
            p.distinct_feature_centers,
            p.feature_centers_called_once,
            p.feature_centers_max_calls,
            p.build_view_calls,
            p.build_view_cells,
            p.run_features_calls,
            cache.entries,
            cache.bytes as f64 / (1024.0 * 1024.0),
            cache.cross_thread_duplicates,
            rss_mb,
            peak_rss_mb,
            idle_rss_mb,
            second_per_s,
            after.peak_bytes as f64 / (1024.0 * 1024.0),
            after.entries,
            after.bytes as f64 / (1024.0 * 1024.0),
            cache.dependency_releases,
            cache.evictions,
            features.0,
            features.1 as f64 / (1024.0 * 1024.0),
            worldgen_cache::idle_releases(),
            cache.cold_entries,
            cache.cold_bytes as f64 / (1024.0 * 1024.0),
            cache.cold_hits,
        );
        black_box(world.is_chunk_loaded(positions[0]));
    }
}
