//! 端到端：`normal` 世界从 Director 的 `Staged` 流水线 + 计算池交付真正的地形区块。
//!
//! `director/tests.rs` 用假生成器测过 `PipelineKind::Staged` 的状态机，
//! `worldgen.rs` / `gen-world` 测过纯函数；这一条把两者接起来，确认计算池线程上
//! 调 `OverworldGenerator`、世界线程按回执交付的盒子里是噪声地形而不是超平坦模板。
//!
//! End-to-end: a `normal` world delivers real terrain through the Director's
//! `Staged` pipeline + compute pool. `director/tests.rs` covers the state
//! machine with a fake generator and `worldgen.rs`/`gen-world` cover the pure
//! functions; this wires them together and checks the delivered box is noise
//! terrain, not the superflat template.

use std::{
    collections::BTreeMap,
    sync::mpsc,
    time::{Duration, Instant},
};

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::blocks;
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT, director::TicketOp};

use crate::{
    world::{World, WorldConfig},
    world_options::{LevelType, WorldGenConfig},
};

/// 起一个 normal 世界、强制加载原点区块，等它交付，再确认里面是真正的噪声地形。
///
/// Start a normal world, force-load the origin chunk, wait for delivery, and
/// check the delivered column is real noise terrain.
#[test]
fn staged_pipeline_delivers_generated_terrain_through_the_pool() {
    let (_inbox, receiver) = mpsc::channel();
    let config = WorldConfig {
        view_distance: 2,
        world_gen: WorldGenConfig {
            seed: 2026,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        },
        // 显式给 1 条计算线程，走 `ChunkJob` 在池子线程上调 `OverworldGenerator`。
        // One explicit worker: `ChunkJob` calls `OverworldGenerator` on a pool thread.
        chunk_worker_threads: 1,
        ..WorldConfig::default()
    };
    let mut world = World::new(config, receiver);
    let origin = ChunkPos::new(0, 0);
    world.apply_ticket(TicketOp::Forced {
        pos: origin,
        forced: true,
    });

    // 交付每刻有预算，池子回执下一刻才收；循环到区块到位，给足时间。
    // Delivery has a per-tick budget and pool replies arrive a tick later;
    // loop until the chunk lands.
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let _ = world.tick();
        if world.region.chunks.is_loaded(origin) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "staged pipeline did not deliver the origin chunk in 90 s"
        );
    }

    // 生成的盒子 `untouched=false`，超平坦模板是 `true`：确认交付的不是模板。
    // Generated boxes are `untouched=false`; the superflat template is `true`.
    assert!(
        !world.region.chunks.is_pristine(origin),
        "delivered chunk is still the pristine superflat template"
    );
    // y=-60 在超平坦里是草方块上方的空气，在噪声地形里是地下的实心方块。
    // y=-60 is air above grass in superflat, but solid underground in noise terrain.
    let deep = world
        .region
        .chunks
        .block(BlockPos::new(0, -60, 0))
        .expect("origin chunk is loaded");
    assert_ne!(deep, blocks::AIR, "origin chunk has no terrain below y=-60");
}

/// 一片 3×3 区块在指定线程数下、完整生成后逐格的方块状态。
///
/// 强加载这 9 个区块，跑到全部交付、计算池排空、增量合并完毕，再把每格方块
/// 读出来。只比方块（不比光照层），避免运行时光照的交付顺序干扰。
///
/// Block states of a 3×3 area after full generation with the given thread
/// count. Compares blocks only (not light layers) so runtime-light delivery
/// order cannot affect the result.
fn generated_blocks(threads: usize, positions: &[ChunkPos]) -> BTreeMap<ChunkPos, Vec<u32>> {
    let (_inbox, receiver) = mpsc::channel();
    let config = WorldConfig {
        view_distance: 3,
        world_gen: WorldGenConfig {
            seed: 2026,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        },
        chunk_worker_threads: threads,
        ..WorldConfig::default()
    };
    let mut world = World::new(config, receiver);
    for &pos in positions {
        world.apply_ticket(TicketOp::Forced {
            pos,
            forced: true,
        });
    }

    let deadline = Instant::now() + Duration::from_secs(180);
    let mut quiet = 0;
    loop {
        let _ = world.tick();
        let all_loaded = positions
            .iter()
            .all(|pos| world.region.chunks.is_loaded(*pos));
        let idle = world.host.compute_in_flight() == 0;
        if all_loaded && idle {
            quiet += 1;
            // 池子空了以后再跑几刻，让待办任务和世界线程的增量合并都落地。
            // A few extra ticks after the pool drains so pending tasks and
            // world-thread delta merges all settle.
            if quiet >= 4 {
                break;
            }
        } else {
            quiet = 0;
        }
        assert!(
            Instant::now() < deadline,
            "normal-world generation did not settle in 180 s (threads={threads})"
        );
    }

    let mut out = BTreeMap::new();
    for &pos in positions {
        let mut cells = Vec::with_capacity(WORLD_HEIGHT as usize * 256);
        for y in MIN_Y..MIN_Y + WORLD_HEIGHT {
            for x in 0..16 {
                for z in 0..16 {
                    let block = world
                        .region
                        .chunks
                        .block(BlockPos::new(pos.x * 16 + x, y, pos.z * 16 + z))
                        .unwrap_or(blocks::AIR);
                    cells.push(block);
                }
            }
        }
        out.insert(pos, cells);
    }
    out
}

/// 验收：1 线程和 4 线程的计算池生成同一片 normal 世界区块，逐格方块完全相同。
/// `features` 的邻居写增量按 `TaskId` 顺序合并、完工排规范序，结果与线程调度无关。
///
/// Acceptance: 1-thread and 4-thread compute pools produce byte-identical
/// blocks for the same normal-world area. Feature neighbor deltas merge in
/// `TaskId` order and are canonically sorted at completion, so the result does
/// not depend on thread scheduling.
#[test]
fn one_and_four_threads_generate_identical_feature_blocks() {
    let positions: Vec<ChunkPos> = (-1..=1)
        .flat_map(|x| (-1..=1).map(move |z| ChunkPos::new(x, z)))
        .collect();
    let one = generated_blocks(1, &positions);
    let four = generated_blocks(4, &positions);
    assert_eq!(one.len(), 9);
    assert_eq!(one.keys().collect::<Vec<_>>(), four.keys().collect::<Vec<_>>());
    for (pos, blocks_one) in &one {
        let blocks_four = four.get(pos).expect("both runs delivered the same chunks");
        assert_eq!(
            blocks_one, blocks_four,
            "chunk {pos:?} differs between 1 and 4 worker threads"
        );
    }
}
