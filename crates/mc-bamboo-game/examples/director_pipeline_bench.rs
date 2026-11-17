//! 真实 Director 流水线的吞吐和阶段耗时测量（不是 `wg-perf` 的纯函数路径）。
//!
//! `cargo run --release -p mc-bamboo-game --example director_pipeline_bench -- \
//!     [--chunks N] [--threads T] [--seed S] [--no-structures] [--label NAME]`
//!
//! 起一个 `World::new`（`normal` 世界、`Staged` 流水线、计算池），强加载一块
//! 正方形区块，不睡节拍器地 tick 到全部交付且计算池排空。量每个交付区块的墙钟、
//! 进程 CPU 时间，以及 `worldgen_profile` 里各阶段（terrain / features / build_view /
//! run_features / light）、地形缓存命中、Director 快照命中、`cached_feature_run` 的
//! 命中与「每个中心被以多少个不同中心调用」。所有数字按一行 CSV 打印，方便同轮
//! 交替 A/B 比较。

use std::{
    env,
    hint::black_box,
    sync::mpsc,
    time::{Duration, Instant},
};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_game::{
    MailBudget, World, WorldConfig,
    world_options::{LevelType, WorldGenConfig},
    worldgen::{clear_terrain_cache, terrain_cache_stats},
    worldgen_features::clear_feature_cache,
    worldgen_profile,
};

#[derive(Debug)]
struct Options {
    side: usize,
    threads: Vec<usize>,
    seed: i64,
    structures: bool,
    label: String,
}

impl Options {
    fn parse() -> Self {
        let mut options = Self {
            side: 5,
            threads: vec![1, 2, 4, 8],
            seed: 2026,
            structures: true,
            label: "run".to_owned(),
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
                "--label" => options.label = value(),
                _ => panic!("unknown argument {arg}"),
            }
        }
        options
    }
}

#[repr(C)]
struct RUsage {
    utime: [i64; 2],
    stime: [i64; 2],
    ru_maxrss: i64,
    ixrss: i64,
    idrss: i64,
    isrss: i64,
    minflt: i64,
    majflt: i64,
    nswap: i64,
    inblock: i64,
    oublock: i64,
    msgsnd: i64,
    msgrcv: i64,
    nsignals: i64,
    nvcsw: i64,
    nivcsw: i64,
}
unsafe extern "C" {
    fn getrusage(who: i32, usage: *mut RUsage) -> i32;
}
/// 进程累计 CPU 毫秒（RUSAGE_SELF=0，macOS timeval 是秒/微秒）。
/// Process-wide accumulated CPU milliseconds.
fn process_cpu_ms() -> f64 {
    let mut usage = RUsage {
        utime: [0; 2],
        stime: [0; 2],
        ru_maxrss: 0,
        ixrss: 0,
        idrss: 0,
        isrss: 0,
        minflt: 0,
        majflt: 0,
        nswap: 0,
        inblock: 0,
        oublock: 0,
        msgsnd: 0,
        msgrcv: 0,
        nsignals: 0,
        nvcsw: 0,
        nivcsw: 0,
    };
    let ok = unsafe { getrusage(0, &raw mut usage) };
    if ok != 0 {
        return 0.0;
    }
    (usage.utime[0] + usage.stime[0]) as f64 * 1000.0
        + (usage.utime[1] + usage.stime[1]) as f64 / 1000.0
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
        let (_inbox, receiver) = mpsc::channel();
        let config = WorldConfig {
            view_distance: 4,
            world_gen: WorldGenConfig {
                seed: options.seed,
                level_type: LevelType::Normal,
                generate_structures: options.structures,
                ..WorldGenConfig::default()
            },
            chunk_worker_threads: threads,
            // 测量不限制每刻交付数，交付节奏不成为吞吐瓶颈。
            // No per-tick delivery cap: the pacing is not part of the measurement.
            mail_budget: MailBudget {
                deliveries: 0,
                reclaims: 0,
            },
            ..WorldConfig::default()
        };
        let mut world = World::new(config, receiver);
        world.force_chunks_unpaced(&positions);

        let wall_started = Instant::now();
        let cpu0 = process_cpu_ms();
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
                    break;
                }
            } else {
                quiet = 0;
            }
            assert!(
                Instant::now() < deadline,
                "pipeline did not settle in 1800 s (threads={threads})"
            );
        }
        let wall = wall_started.elapsed().as_secs_f64();
        let cpu_ms = process_cpu_ms() - cpu0;
        let p = worldgen_profile::stats();
        worldgen_profile::disable();
        let cache = terrain_cache_stats();
        let ns = |v: u64| v as f64 / 1_000_000.0;
        println!(
            "{},structures={},threads={},requested={},delivered={},wall_s={:.3},\
             delivered_per_s={:.2},wall_per_delivered_ms={:.3},cpu_per_delivered_ms={:.3},\
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
             entries={},bytes_mb={:.1},cross_thread_duplicates={}",
            options.label,
            options.structures,
            threads,
            total,
            delivered,
            wall,
            delivered as f64 / wall,
            wall * 1000.0 / delivered as f64,
            cpu_ms / delivered as f64,
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
        );
        black_box(world.is_chunk_loaded(positions[0]));
    }
}
