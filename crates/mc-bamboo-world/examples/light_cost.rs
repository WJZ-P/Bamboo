//! 量一量光照在世界线程上花多少时间：放一个火把、拆掉一大片屋顶，都是改方块加算完光照的总时间。
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
        store.run_light_updates();
        timed(&mut store, |store| place(store, 8, GROUND, 8, blocks::AIR))
    });
    for size in [32, 64, 128] {
        report(&format!("build a {size}x{size} stone roof"), || {
            let mut store = ChunkStore::flat();
            timed(&mut store, |store| roof(store, size, blocks::STONE))
        });
        report(&format!("remove a {size}x{size} stone roof"), || {
            let mut store = ChunkStore::flat();
            roof(&mut store, size, blocks::STONE);
            store.run_light_updates();
            timed(&mut store, |store| roof(store, size, blocks::AIR))
        });
    }
    report("place a torch under a 64x64 roof", || {
        let mut store = ChunkStore::flat();
        roof(&mut store, 64, blocks::STONE);
        store.run_light_updates();
        timed(&mut store, |store| {
            place(store, 32, GROUND, 32, blocks::TORCH)
        })
    });
}

/// 改方块，再把光照算完，返回总共用的时间。
fn timed(store: &mut ChunkStore, edit: impl FnOnce(&mut ChunkStore)) -> Duration {
    let start = Instant::now();
    edit(store);
    store.run_light_updates();
    start.elapsed()
}

fn place(store: &mut ChunkStore, x: i32, y: i32, z: i32, state: u32) {
    store.set_block(BlockPos::new(x, y, z), state);
}

/// 以原点为角的正方形屋顶。
fn roof(store: &mut ChunkStore, size: i32, state: u32) {
    for x in 0..size {
        for z in 0..size {
            place(store, x, ROOF_Y, z, state);
        }
    }
}

fn report(name: &str, mut measure: impl FnMut() -> Duration) {
    let mut times: Vec<Duration> = (0..RUNS).map(|_| measure()).collect();
    times.sort_unstable();
    let median = times[RUNS / 2];
    let worst = times[RUNS - 1];
    println!(
        "{name:<40} median {:>9.3} ms   worst {:>9.3} ms",
        median.as_secs_f64() * 1000.0,
        worst.as_secs_f64() * 1000.0
    );
}
