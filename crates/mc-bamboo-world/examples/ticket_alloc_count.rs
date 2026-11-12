//! 数一次 `run_updates` 里发生多少次堆分配。
//!
//! ```text
//! cargo run --release -p mc-bamboo-world --example ticket_alloc_count -- [玩家数] [刻数]
//! ```
//!
//! 用包了一层 `System` 的全局分配器计数；`set_player` 不算在内，只数传播那一段。
//! 这个二进制里的耗时不能当基准数字用（每次分配多一次原子加法）。

use std::{
    alloc::{GlobalAlloc, Layout, System},
    env,
    sync::atomic::{AtomicU64, Ordering},
};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::ticket::TicketTable;

struct CountingAllocator;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static REALLOCS: AtomicU64 = AtomicU64::new(0);
static DEALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        DEALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        REALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(
            new_size.saturating_sub(layout.size()) as u64,
            Ordering::Relaxed,
        );
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    let mut args = env::args().skip(1);
    let players: i32 = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200)
        .clamp(1, 10_000);
    let ticks: i32 = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
        .clamp(1, 100_000);

    let mut table = TicketTable::new();
    for id in 0..players {
        table.set_player(id as u64, Some(ChunkPos::new(id * 47, 0)));
    }
    table.run_updates();

    let mut total_allocs = 0u64;
    let mut total_reallocs = 0u64;
    let mut total_deallocs = 0u64;
    let mut total_bytes = 0u64;
    let mut changed = 0u64;
    for tick in 0..ticks {
        for id in 0..players {
            let x = id * 47 + tick;
            table.set_player(id as u64, Some(ChunkPos::new(x, 0)));
        }
        ALLOCS.store(0, Ordering::Relaxed);
        REALLOCS.store(0, Ordering::Relaxed);
        DEALLOCS.store(0, Ordering::Relaxed);
        BYTES.store(0, Ordering::Relaxed);
        let changes = table.run_updates();
        total_allocs += ALLOCS.load(Ordering::Relaxed);
        total_reallocs += REALLOCS.load(Ordering::Relaxed);
        total_deallocs += DEALLOCS.load(Ordering::Relaxed);
        total_bytes += BYTES.load(Ordering::Relaxed);
        changed += changes.len() as u64;
    }
    let ticks = ticks as f64;
    println!(
        "每次 run_updates（{players} 名玩家同时移动）：alloc {:.1}、realloc {:.1}、dealloc {:.1}、{:.0} KiB",
        total_allocs as f64 / ticks,
        total_reallocs as f64 / ticks,
        total_deallocs as f64 / ticks,
        total_bytes as f64 / ticks / 1024.0
    );
    println!(
        "折到每次移动：alloc {:.2}、变等级区块 {:.1}",
        total_allocs as f64 / ticks / players as f64,
        changed as f64 / ticks / players as f64
    );
}
