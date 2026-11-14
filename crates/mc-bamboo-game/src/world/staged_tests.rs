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
    sync::mpsc,
    time::{Duration, Instant},
};

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::blocks;
use mc_bamboo_world::director::TicketOp;

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
