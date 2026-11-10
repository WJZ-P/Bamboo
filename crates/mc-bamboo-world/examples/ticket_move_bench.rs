//! 玩家跨区块的热路径基准。
//!
//! ```text
//! cargo run --release -p mc-bamboo-world --example ticket_move_bench -- [玩家数] [刻数]
//! ```
//!
//! 默认 200 名玩家、每刻每人跨一次区块边界，报每次移动的平均耗时、每次移动改变了多少个
//! 区块的等级、以及表里的项数。设计稿 5.1 要求"只算两个方形视野的差集"，这个数字能
//! 直接看出有没有退化成重算整个视野。

use std::{env, hint::black_box, time::Instant};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::ticket::TicketTable;

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
        .unwrap_or(200)
        .clamp(1, 100_000);

    let mut table = TicketTable::new();
    // 每名玩家从不同的区块出发，避免所有人的视野完全重叠。
    for id in 0..players {
        table.set_player(id as u64, Some(ChunkPos::new(id * 47, 0)));
    }
    table.run_updates();
    println!(
        "初始：{players} 名玩家，视距 {}，等级表项数 {}",
        table.view_distance(),
        table.level_count()
    );

    let mut moves: u64 = 0;
    let mut changed: u64 = 0;
    let mut max_changed = 0usize;
    let mut set_time = std::time::Duration::ZERO;
    let mut update_time = std::time::Duration::ZERO;
    let start = Instant::now();
    for tick in 0..ticks {
        let t0 = Instant::now();
        for id in 0..players {
            let x = id * 47 + tick;
            table.set_player(id as u64, Some(ChunkPos::new(x, 0)));
        }
        set_time += t0.elapsed();
        let t1 = Instant::now();
        let changes = table.run_updates();
        update_time += t1.elapsed();
        changed += changes.len() as u64;
        max_changed = max_changed.max(changes.len());
        moves += players as u64;
        black_box(changes.first());
    }
    let elapsed = start.elapsed();

    println!("每次移动：{:?}", elapsed / moves as u32);
    println!(
        "每次移动变等级的区块数：平均 {:.1}，最多 {max_changed}",
        changed as f64 / moves as f64
    );
    println!("等级表项数：{}（加载 + 模拟）", table.level_count());
    println!("总耗时：{elapsed:?}，共 {moves} 次移动、{ticks} 刻");
}
