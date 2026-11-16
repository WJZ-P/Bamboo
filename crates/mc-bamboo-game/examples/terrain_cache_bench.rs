//! 量地形缓存的常驻内存、命中率和整块生成墙钟（release 构建）。
//!
//! `cargo run --release -p mc-bamboo-game --example terrain_cache_bench -- \
//!     [--chunks N] [--threads T] [--seed S] [--label NAME]`
//!
//! 每批按「中心区块」跑游戏里同一份 `game_features`：一个中心要 9 个 CARVERS
//! 地形（3×3），相邻中心大量重叠，正好是这块缓存的用途。工作和线上的计算池一样
//! 用 `std::thread::scope` 分给 T 条线程，每条线程各自持有一份 `Generator`
//! （和 `OverworldGenerator` 在池子线程上的情形一致）。结构分支关掉，和
//! `gen-world --through features` 的对拍世界一致；地形是纯函数，关掉结构只影响
//! 有没有 Beardifier，不影响比较口径。
//!
//! 每批前清一次缓存，量完打印一块 CSV 行：区块数、线程数、墙钟中位数 / p95、
//! 吞吐、进程峰值 RSS、缓存条目数与实测堆字节、命中率、跨线程重复计算次数。
//! RSS 峰值用 `getrusage(RUSAGE_SELF).ru_maxrss`（macOS 是字节）读，测量时
//! 不编译、不开别的进程。
//!
//! Measure the terrain cache's resident memory, hit rate and per-chunk wall clock
//! (release build). Each batch runs the game's own `game_features` over a grid of
//! centers; every center needs a 3x3 of CARVERS terrain. Work is spread over T
//! threads like the in-game compute pool. Prints one CSV row per batch with RSS
//! peak, measured cache bytes, hit rate and cross-thread duplicate computations.

use std::{
    env,
    hint::black_box,
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Instant,
};

use mc_bamboo_game::{
    world_options::{LevelType, WorldGenConfig},
    worldgen::{clear_terrain_cache, terrain_cache_stats},
    worldgen_features::game_features,
};

/// 一批的命令行选项。 Command-line options for one batch.
#[derive(Debug)]
struct Options {
    /// 网格边长：总共跑 `chunks^2` 个中心。 Grid side: `chunks^2` centers in all.
    side: usize,
    threads: usize,
    seed: i64,
    label: String,
}

impl Options {
    fn parse() -> Result<Self, String> {
        let mut options = Self {
            side: 32,
            threads: 4,
            seed: 2026,
            label: "run".to_owned(),
        };
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            let mut value = || {
                args.next()
                    .ok_or_else(|| format!("missing value for {arg}"))
            };
            match arg.as_str() {
                "--chunks" => options.side = value()?.parse().map_err(|e| format!("{e}"))?,
                "--threads" => options.threads = value()?.parse().map_err(|e| format!("{e}"))?,
                "--seed" => options.seed = value()?.parse().map_err(|e| format!("{e}"))?,
                "--label" => options.label = value()?,
                _ => {
                    return Err(format!(
                        "usage: terrain_cache_bench [--chunks N] [--threads T] [--seed S] [--label NAME]\nunknown argument {arg}"
                    ));
                }
            }
        }
        if options.side == 0 || options.threads == 0 {
            return Err("--chunks and --threads must be positive".to_owned());
        }
        Ok(options)
    }
}

fn config(seed: i64) -> WorldGenConfig {
    let mut config = WorldGenConfig {
        seed,
        level_type: LevelType::Normal,
        ..WorldGenConfig::default()
    };
    // 和 `gen-world --through features` 一样关掉结构，让 9 宫格的每个格子都走
    // 缓存路径（`Beardifier.EMPTY`）。 Match the features oracle's structure-free world.
    config.generate_structures = false;
    config
}

/// 一格的墙钟（毫秒）。 One center's wall clock in milliseconds.
fn one_center(config: &WorldGenConfig, x: i32, z: i32) -> f64 {
    let start = Instant::now();
    let result = black_box(game_features(config, x, z, None)).expect("game features failed");
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    black_box(result.blocks[4][0][0]);
    elapsed
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let index = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

/// 进程峰值常驻内存（字节）。macOS 的 `ru_maxrss` 单位就是字节。
/// Peak resident set size in bytes; macOS `ru_maxrss` is already in bytes.
fn peak_rss_bytes() -> u64 {
    #[repr(C)]
    struct RUsage {
        ru_utime: [i64; 2],
        ru_stime: [i64; 2],
        ru_maxrss: i64,
        ru_ixrss: i64,
        ru_idrss: i64,
        ru_isrss: i64,
        ru_minflt: i64,
        ru_majflt: i64,
        ru_nswap: i64,
        ru_inblock: i64,
        ru_oublock: i64,
        ru_msgsnd: i64,
        ru_msgrcv: i64,
        ru_nsignals: i64,
        ru_nvcsw: i64,
        ru_nivcsw: i64,
    }
    unsafe extern "C" {
        fn getrusage(who: i32, usage: *mut RUsage) -> i32;
    }
    let mut usage = RUsage {
        ru_utime: [0; 2],
        ru_stime: [0; 2],
        ru_maxrss: 0,
        ru_ixrss: 0,
        ru_idrss: 0,
        ru_isrss: 0,
        ru_minflt: 0,
        ru_majflt: 0,
        ru_nswap: 0,
        ru_inblock: 0,
        ru_oublock: 0,
        ru_msgsnd: 0,
        ru_msgrcv: 0,
        ru_nsignals: 0,
        ru_nvcsw: 0,
        ru_nivcsw: 0,
    };
    // RUSAGE_SELF = 0. 只读进程自己的计数，失败就报 0，不让基准 panic。
    // RUSAGE_SELF = 0; read only our own counters and report 0 on failure.
    let ok = unsafe { getrusage(0, &raw mut usage) };
    if ok == 0 {
        usage.ru_maxrss.max(0) as u64
    } else {
        0
    }
}

fn main() {
    let options = Options::parse().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    let config = Arc::new(config(options.seed));
    let total = options.side * options.side;
    // 先在主线程预热一次：解析 Worldgen、建 Generator、填一份线程本地状态。
    // Warm up on the main thread: parse Worldgen, build the Generator.
    let warm = one_center(&config, i32::MAX / 4, i32::MAX / 4);
    clear_terrain_cache();

    let timings = Arc::new(std::sync::Mutex::new(Vec::with_capacity(total)));
    let next = Arc::new(AtomicUsize::new(0));
    // 每条工作线程先各跑一个（不在计时范围里的）区块，把线程本地的 `Generator`
    // 和 `Worldgen` 解析好，再一起进计时段；否则第一块要花 2 s 建生成器，p95 会被
    // 这组冷启动主导。这几块也进缓存，但不计入命中率（下面清一次计数再开跑）。
    // Each worker generates one untimed chunk first so its thread-local
    // `Generator` / parsed `Worldgen` is ready before the timed section.
    let ready = Arc::new(Barrier::new(options.threads + 1));
    let mut start = Instant::now();
    thread::scope(|scope| {
        for _ in 0..options.threads {
            let config = Arc::clone(&config);
            let timings = Arc::clone(&timings);
            let next = Arc::clone(&next);
            let ready = Arc::clone(&ready);
            let side = options.side;
            scope.spawn(move || {
                let _ = black_box(one_center(&config, i32::MAX / 4, i32::MAX / 4));
                ready.wait();
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= total {
                        break;
                    }
                    let x = (index / side) as i32;
                    let z = (index % side) as i32;
                    let ms = one_center(&config, x, z);
                    timings.lock().expect("timings lock").push(ms);
                }
            });
        }
        ready.wait();
        // 清掉工作线程冷启动的缓存和计数，只量下面这批；所有线程都在 barrier 上等着。
        // Clear the warmup cache/counters once every worker is parked on the barrier.
        clear_terrain_cache();
        start = Instant::now();
    });
    let wall = start.elapsed().as_secs_f64();
    let stats = terrain_cache_stats();
    let mut timings = timings.lock().expect("timings lock").clone();
    timings.sort_by(f64::total_cmp);
    let hit_rate = if stats.hits + stats.misses == 0 {
        0.0
    } else {
        stats.hits as f64 / (stats.hits + stats.misses) as f64
    };
    // 一行 CSV，方便多轮交替量后取中位数。 One CSV row per alternating run.
    println!(
        "{},threads={},chunks={},warm_ms={:.2},wall_s={:.3},throughput={:.2},\
         median_ms={:.3},p95_ms={:.3},rss_peak_mb={:.1},entries={},bytes_mb={:.1},\
         peak_bytes_mb={:.1},hits={},misses={},hit_rate={:.4},clears={},evictions={},\
         cross_thread_duplicates={}",
        options.label,
        options.threads,
        total,
        warm,
        wall,
        total as f64 / wall,
        percentile(&timings, 0.5),
        percentile(&timings, 0.95),
        peak_rss_bytes() as f64 / (1024.0 * 1024.0),
        stats.entries,
        stats.bytes as f64 / (1024.0 * 1024.0),
        stats.peak_bytes as f64 / (1024.0 * 1024.0),
        stats.hits,
        stats.misses,
        hit_rate,
        stats.clears,
        stats.evictions,
        stats.cross_thread_duplicates,
    );
}
