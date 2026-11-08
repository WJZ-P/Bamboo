//! 量一量光照在世界线程上花多少时间。
//!
//! 第一部分是总账：放一个火把、拆掉一大片屋顶，改方块加上把光照全部算完一共用多久。
//! 第二部分是按 tick 分摊：一个 tick 里改完方块，之后每 tick 只给光照 [`LIGHT_BUDGET`] 的工作量，
//! 量最慢的那个 tick、一共用了几个 tick 和总时间。
//!
//! ```text
//! cargo run --release -p mc-bamboo-world --example light_cost
//! ```

use std::time::{Duration, Instant};

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::blocks;
use mc_bamboo_world::store::ChunkStore;

/// 每项量这么多次，取中位数。
const RUNS: usize = 15;
/// 地面上面第一格。
const GROUND: i32 = -60;
/// 屋顶离地面的高度。
const ROOF_Y: i32 = GROUND + 10;
/// 每 tick 给光照的工作量，和 `mc-bamboo-game` 里的 `LIGHT_BUDGET` 相同。
const LIGHT_BUDGET: usize = 80_000;

fn main() {
    report("create the flat world (template light)", || {
        let start = Instant::now();
        let _store = ChunkStore::flat();
        start.elapsed()
    });
    report("place a torch in the open", || {
        let mut store = ChunkStore::flat();
        timed(&mut store, |store| {
            place(store, 8, GROUND, 8, blocks::TORCH)
        })
    });
    report("remove that torch", || {
        let mut store = ChunkStore::flat();
        place(&mut store, 8, GROUND, 8, blocks::TORCH);
        store.finish_light_updates();
        timed(&mut store, |store| place(store, 8, GROUND, 8, blocks::AIR))
    });
    for size in [32, 64, 128] {
        report(&format!("build a {size}x{size} stone roof"), || {
            let mut store = ChunkStore::flat();
            timed(&mut store, |store| roof(store, size, blocks::STONE))
        });
        report(&format!("remove a {size}x{size} stone roof"), || {
            let mut store = removable_roof(size);
            timed(&mut store, |store| roof(store, size, blocks::AIR))
        });
    }
    report("place a torch under a 64x64 roof", || {
        let mut store = removable_roof(64);
        timed(&mut store, |store| {
            place(store, 32, GROUND, 32, blocks::TORCH)
        })
    });

    report("place 3000 torches", || {
        let mut store = ChunkStore::flat();
        timed(&mut store, |store| torches(store, 3000))
    });

    println!();
    println!("spread over ticks, {LIGHT_BUDGET} units of light work per tick:");
    for size in [64, 128] {
        report_ticks(&format!("build a {size}x{size} stone roof"), || {
            let mut store = ChunkStore::flat();
            ticks(&mut store, |store| roof(store, size, blocks::STONE))
        });
        report_ticks(&format!("remove a {size}x{size} stone roof"), || {
            let mut store = removable_roof(size);
            ticks(&mut store, |store| roof(store, size, blocks::AIR))
        });
    }
    report_ticks("place 3000 torches", || {
        let mut store = ChunkStore::flat();
        ticks(&mut store, |store| torches(store, 3000))
    });
}

/// 改方块，再把光照算完，返回总共用的时间。
fn timed(store: &mut ChunkStore, edit: impl FnOnce(&mut ChunkStore)) -> Duration {
    let start = Instant::now();
    edit(store);
    store.finish_light_updates();
    start.elapsed()
}

/// 第一个 tick 改方块并且算一份光照，之后每 tick 接着算，直到算完。返回每个 tick 用的时间。
fn ticks(store: &mut ChunkStore, edit: impl FnOnce(&mut ChunkStore)) -> Vec<Duration> {
    let start = Instant::now();
    edit(store);
    store.run_light_updates(LIGHT_BUDGET);
    let mut ticks = vec![start.elapsed()];
    while store.has_pending_light_updates() {
        let start = Instant::now();
        store.run_light_updates(LIGHT_BUDGET);
        ticks.push(start.elapsed());
    }
    ticks
}

fn place(store: &mut ChunkStore, x: i32, y: i32, z: i32, state: u32) {
    store.set_block(BlockPos::new(x, y, z), state);
}

/// 地面上隔 5 格一支火把，一行 64 支。一轮 1000 个任务里全是光源，一轮就要几十毫秒。
fn torches(store: &mut ChunkStore, count: i32) {
    for index in 0..count {
        place(store, index % 64 * 5, GROUND, index / 64 * 5, blocks::TORCH);
    }
}

/// 以原点为角的正方形屋顶。
fn roof(store: &mut ChunkStore, size: i32, state: u32) {
    for x in 0..size {
        for z in 0..size {
            place(store, x, ROOF_Y, z, state);
        }
    }
}

/// 一个已经盖好、光照也算完了的屋顶。
fn removable_roof(size: i32) -> ChunkStore {
    let mut store = ChunkStore::flat();
    roof(&mut store, size, blocks::STONE);
    store.finish_light_updates();
    store
}

fn report(name: &str, mut measure: impl FnMut() -> Duration) {
    let mut times: Vec<Duration> = (0..RUNS).map(|_| measure()).collect();
    times.sort_unstable();
    let median = times[RUNS / 2];
    let worst = times[RUNS - 1];
    println!(
        "{name:<40} median {:>9.3} ms   worst {:>9.3} ms",
        millis(median),
        millis(worst)
    );
}

/// 每次的最慢 tick 和总时间各取中位数，tick 数取最多的一次。
fn report_ticks(name: &str, mut measure: impl FnMut() -> Vec<Duration>) {
    let runs: Vec<Vec<Duration>> = (0..RUNS).map(|_| measure()).collect();
    let median = |mut values: Vec<Duration>| {
        values.sort_unstable();
        values[RUNS / 2]
    };
    let slowest = median(
        runs.iter()
            .map(|ticks| ticks.iter().copied().max().unwrap_or_default())
            .collect(),
    );
    let total = median(runs.iter().map(|ticks| ticks.iter().sum()).collect());
    let count = runs.iter().map(Vec::len).max().unwrap_or_default();
    println!(
        "{name:<40} slowest tick {:>8.3} ms   total {:>8.3} ms   ticks {count:>3}",
        millis(slowest),
        millis(total)
    );
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
