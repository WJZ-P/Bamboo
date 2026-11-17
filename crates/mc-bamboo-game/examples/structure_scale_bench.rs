//! 量「带结构」整块生成在不同线程数下的吞吐、每中心 CPU 时间与地形缓存行为。
//!
//! `cargo run --release -p mc-bamboo-game --example structure_scale_bench -- \
//!     [--chunks N] [--threads T] [--seed S] [--structures] [--label NAME]`
//!
//! 和 `terrain_cache_bench` 同一套口径，但可以打开 `generate_structures`，用于
//! 核对 `structure_starts` / `structure_references` / Beardifier 的缓存效果。
//! 每中心 CPU 时间用进程级 `getrusage(RUSAGE_SELF)` 的增量除以中心数，
//! 不受别的进程抢占墙钟影响。

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
            side: 12,
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
    let config = Arc::new(WorldGenConfig {
        seed: options.seed,
        level_type: LevelType::Normal,
        generate_structures: options.structures,
        ..WorldGenConfig::default()
    });
    // 主线程预热：解析 Worldgen、建 Generator、填一份线程本地状态。
    let _ = black_box(game_features(&config, i32::MAX / 4, i32::MAX / 4, None).unwrap());
    for &threads in &options.threads {
        clear_terrain_cache();
        let next = Arc::new(AtomicUsize::new(0));
        let timings = Arc::new(std::sync::Mutex::new(Vec::with_capacity(total)));
        let ready = Arc::new(Barrier::new(threads + 1));
        let go = Arc::new(Barrier::new(threads + 1));
        let mut start = Instant::now();
        let mut cpu0 = 0.0;
        thread::scope(|scope| {
            for thread_index in 0..threads {
                let config = Arc::clone(&config);
                let next = Arc::clone(&next);
                let timings = Arc::clone(&timings);
                let ready = Arc::clone(&ready);
                let go = Arc::clone(&go);
                let side = options.side;
                scope.spawn(move || {
                    let _ = black_box(
                        game_features(
                            &config,
                            i32::MAX / 4 + thread_index as i32 * 64,
                            i32::MAX / 4,
                            None,
                        )
                        .unwrap(),
                    );
                    ready.wait();
                    go.wait();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= total {
                            break;
                        }
                        let x = (index / side) as i32;
                        let z = (index % side) as i32;
                        let at = Instant::now();
                        let r = black_box(game_features(&config, x, z, None).unwrap());
                        black_box(r.blocks[4][0][0]);
                        timings
                            .lock()
                            .expect("timings")
                            .push(at.elapsed().as_secs_f64() * 1000.0);
                    }
                });
            }
            ready.wait();
            clear_terrain_cache();
            go.wait();
            start = Instant::now();
            cpu0 = process_cpu_ms();
        });
        let wall = start.elapsed().as_secs_f64();
        let cpu_ms = process_cpu_ms() - cpu0;
        let stats = terrain_cache_stats();
        let mut timings = timings.lock().expect("timings").clone();
        timings.sort_by(f64::total_cmp);
        let median = timings[timings.len() / 2];
        let p95 = timings[((timings.len() as f64 * 0.95) as usize).min(timings.len() - 1)];
        println!(
            "{},structures={},threads={},chunks={},wall_s={:.3},throughput={:.2},\
             median_ms={:.3},p95_ms={:.3},cpu_per_center_ms={:.3},\
             hits={},misses={},hit_rate={:.4},cross_thread_duplicates={},entries={},bytes_mb={:.1}",
            options.label,
            options.structures,
            threads,
            total,
            wall,
            total as f64 / wall,
            median,
            p95,
            cpu_ms / total as f64,
            stats.hits,
            stats.misses,
            stats.hits as f64 / (stats.hits + stats.misses).max(1) as f64,
            stats.cross_thread_duplicates,
            stats.entries,
            stats.bytes as f64 / (1024.0 * 1024.0),
        );
    }
}
