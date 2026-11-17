//! 端到端：`normal` 世界从 Director 的 `Staged` 流水线 + 计算池交付真正的地形区块。
//!
//! `director/tests.rs` 用假生成器测过 `PipelineKind::Staged` 的状态机，
//! `worldgen.rs` / `gen-world` 测过纯函数；这一条把两者接起来，确认计算池线程上
//! 调 `OverworldGenerator`、世界线程按回执交付的盒子里是噪声地形而不是超平坦模板，
//! 并且生成期光照（`light` 阶段算出的天空光 + 方块光、`isLightOn=true`）真的落进
//! 交付盒子，1 线程和 4 线程逐格相同。
//!
//! End-to-end: a `normal` world delivers real terrain through the Director's
//! `Staged` pipeline + compute pool. `director/tests.rs` covers the state
//! machine and `worldgen.rs`/`gen-world` cover the pure functions; this wires
//! them together and checks the delivered box is noise terrain, that the
//! generation-time sky + block light (`isLightOn=true`) is baked in, and that
//! 1-thread and 4-thread results are cell-by-cell identical.

use std::{
    collections::BTreeMap,
    sync::mpsc,
    time::{Duration, Instant},
};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
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
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        let _ = world.tick();
        if world.island.region.chunks.is_loaded(origin) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "staged pipeline did not deliver the origin chunk in 600 s"
        );
    }

    // 生成的盒子 `untouched=false`，超平坦模板是 `true`：确认交付的不是模板。
    // Generated boxes are `untouched=false`; the superflat template is `true`.
    assert!(
        !world.island.region.chunks.is_pristine(origin),
        "delivered chunk is still the pristine superflat template"
    );
    // y=-60 在超平坦里是草方块上方的空气，在噪声地形里是地下的实心方块。
    // y=-60 is air above grass in superflat, but solid underground in noise terrain.
    let deep = world
        .island
        .region
        .chunks
        .block(BlockPos::new(0, -60, 0))
        .expect("origin chunk is loaded");
    assert_ne!(deep, blocks::AIR, "origin chunk has no terrain below y=-60");
}

/// 一片区块在指定线程数下、完整生成后逐格的方块和两层光照。
///
/// 强加载这些区块，跑到全部交付、计算池排空、增量合并完毕，再把每格方块、天空光、
/// 方块光读出来，以及这一柱的 `isLightOn`。交付前 `finish_light_updates` 会把光照
/// 跑到底，读到的就是生成期烘焙进去的稳定值。
///
/// Per-cell blocks, sky + block light, and `isLightOn` for a set of chunks after
/// full generation with the given thread count. Reads the settled light that
/// `finish_light_updates` publishes, i.e. the generation-time baked values.
struct ChunkCells {
    blocks: Vec<u32>,
    sky: Vec<u8>,
    block: Vec<u8>,
    light_on: bool,
}

fn generated_cells(threads: usize, positions: &[ChunkPos]) -> BTreeMap<ChunkPos, ChunkCells> {
    generated_cells_with_seed(2026, threads, positions)
}

fn generated_cells_with_seed(
    seed: i64,
    threads: usize,
    positions: &[ChunkPos],
) -> BTreeMap<ChunkPos, ChunkCells> {
    let (_inbox, receiver) = mpsc::channel();
    let config = WorldConfig {
        view_distance: 3,
        world_gen: WorldGenConfig {
            seed,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        },
        chunk_worker_threads: threads,
        ..WorldConfig::default()
    };
    let mut world = World::new(config, receiver);
    // 这里比的是生成结果。区块交付后世界还在 tick，随机刻会让草蔓延到树下的泥土上、
    // 让够不到原木的树叶腐烂，而两次运行经过的 tick 数不同。关掉随机刻再比。
    // This compares generation output. The world keeps ticking after delivery, and
    // random ticks spread grass onto the dirt under trees and decay leaves out of
    // reach of a log, while the two runs see different numbers of ticks. Turn random
    // ticks off for the comparison.
    world.island.region.block_rules.random_tick_speed = 0;
    for &pos in positions {
        world.apply_ticket(TicketOp::Forced { pos, forced: true });
    }

    let deadline = Instant::now() + Duration::from_secs(900);
    let mut quiet = 0;
    loop {
        let _ = world.tick();
        let all_loaded = positions
            .iter()
            .all(|pos| world.island.region.chunks.is_loaded(*pos));
        let idle = world.master.host.compute_in_flight() == 0;
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
            "normal-world generation did not settle in 900 s (threads={threads})"
        );
    }

    let capacity = WORLD_HEIGHT as usize * 256;
    let mut out = BTreeMap::new();
    for &pos in positions {
        let light_on = world
            .island
            .region
            .chunks
            .column_clone(pos)
            .map(|column| column.light_on())
            .unwrap_or(false);
        let mut cells = ChunkCells {
            blocks: Vec::with_capacity(capacity),
            sky: Vec::with_capacity(capacity),
            block: Vec::with_capacity(capacity),
            light_on,
        };
        for y in MIN_Y..MIN_Y + WORLD_HEIGHT {
            for x in 0..16 {
                for z in 0..16 {
                    let at = BlockPos::new(pos.x * 16 + x, y, pos.z * 16 + z);
                    cells
                        .blocks
                        .push(world.island.region.chunks.block(at).unwrap_or(blocks::AIR));
                    cells
                        .sky
                        .push(world.island.region.chunks.light_value(LightLayer::Sky, at));
                    cells.block.push(
                        world
                            .island
                            .region
                            .chunks
                            .light_value(LightLayer::Block, at),
                    );
                }
            }
        }
        out.insert(pos, cells);
    }
    out
}

/// 验收：1 线程和 4 线程的计算池生成同一片 normal 世界区块，逐格方块、天空光、
/// 方块光都完全相同。`features` 的邻居写增量按 `TaskId` 顺序合并、完工排规范序，
/// `light` 阶段的生成期光照是从只读快照算出的纯函数，两者都与线程调度无关。
///
/// Acceptance: 1-thread and 4-thread compute pools produce byte-identical
/// blocks AND sky/block light for the same area. Feature deltas merge in
/// `TaskId` order and are canonically sorted; generation-time light is a pure
/// function of the read-only snapshots, so neither depends on scheduling.
#[test]
fn one_and_four_threads_generate_identical_feature_blocks() {
    let positions: Vec<ChunkPos> = (-1..=1)
        .flat_map(|x| (-1..=1).map(move |z| ChunkPos::new(x, z)))
        .collect();
    let one = generated_cells(1, &positions);
    let four = generated_cells(4, &positions);
    assert_eq!(one.len(), 9);
    assert_eq!(
        one.keys().collect::<Vec<_>>(),
        four.keys().collect::<Vec<_>>()
    );
    for (pos, a) in &one {
        let b = four.get(pos).expect("both runs delivered the same chunks");
        assert_eq!(
            a.blocks, b.blocks,
            "chunk {pos:?} blocks differ across threads"
        );
        assert_eq!(
            a.sky, b.sky,
            "chunk {pos:?} sky light differs across threads"
        );
        assert_eq!(
            a.block, b.block,
            "chunk {pos:?} block light differs across threads"
        );
        assert_eq!(
            a.light_on, b.light_on,
            "chunk {pos:?} isLightOn differs across threads"
        );
    }
}

/// 0 / 2 / 8 计算线程生成同一片结构区块，结果逐字节相同。真实 Beardifier 只从
/// 依赖表 `GENERATION[4].requires = [(S,8),(B,1)]` 保证定稿的起点/引用算，
/// 并按 (seed,x,z) 全局缓存，所以任务完成先后不影响结果。
#[test]
fn structure_terrain_is_identical_across_worker_counts() {
    use mc_bamboo_world::director::GENERATION;
    use mc_bamboo_world::ticket::ChunkStatus;
    // 依赖表本身要是官方的两条，否则纯函数缓存也救不了调度顺序。
    assert_eq!(
        GENERATION[4].requires,
        &[(ChunkStatus::StructureStarts, 8), (ChunkStatus::Biomes, 1)]
    );
    let positions: Vec<ChunkPos> = (-1..=1)
        .flat_map(|x| (-1..=1).map(move |z| ChunkPos::new(x, z)))
        .collect();
    let baseline = generated_cells(0, &positions);
    for threads in [2usize, 8] {
        let other = generated_cells(threads, &positions);
        for (pos, cell) in &baseline {
            assert_eq!(
                cell.blocks,
                other.get(pos).unwrap().blocks,
                "chunk {pos:?} differs with {threads} workers"
            );
        }
    }
}

/// 生成期光照真的落进交付盒子：`isLightOn=true`，并且天空光是算过的真值 ——
/// 露天有 15，深地下被遮成 0，而不是模板那种「全 15」。
///
/// Generation-time light is baked into the delivered column: `isLightOn=true`
/// and sky light is real (15 in the open, 0 deep underground), not the all-15
/// template.
#[test]
fn generation_light_is_baked_into_the_delivered_column() {
    let positions = [ChunkPos::new(0, 0)];
    let cells = generated_cells(1, &positions);
    let center = &cells[&ChunkPos::new(0, 0)];
    assert!(
        center.light_on,
        "delivered generated column must have isLightOn=true"
    );
    assert!(
        center.sky.contains(&15),
        "some cell should see open sky (sky light 15)"
    );
    assert!(
        center.sky.contains(&0),
        "some deep cell should be fully dark (sky light 0)"
    );
}

/// 结构附近（种子 42 的 trial_chambers 起点 (17, 7) 周围）真实 Director 流水线交付的
/// 方块和纯函数（`build_view` 3×3 + 8 个邻居中心的溢出，规范序合并）逐格相同，1 线程
/// 和 4 线程的光照也相同。Director 的依赖记录不真正跑邻居的 `terrain`，有没有邻居
/// 快照取决于调度；补格曾读不带 Beardifier 的地形，这里 1/4 线程会差几十格。
///
/// Near structures, the real Director pipeline delivers the same blocks as the
/// pure functions, and 1 vs 4 threads give the same light. Snapshot presence
/// depends on scheduling; a Beardifier-less fallback made threads disagree here.
#[test]
fn director_matches_pure_features_near_structures() {
    let config = WorldGenConfig {
        seed: 42,
        level_type: LevelType::Normal,
        ..WorldGenConfig::default()
    };
    let positions: Vec<ChunkPos> = (16..=18)
        .flat_map(|x| (6..=8).map(move |z| ChunkPos::new(x, z)))
        .collect();
    let one = generated_cells_with_seed(42, 1, &positions);
    let four = generated_cells_with_seed(42, 4, &positions);
    for &pos in &positions {
        let pure = crate::worldgen_features::pure_feature_blocks_for_test(&config, pos.x, pos.z)
            .expect("pure features");
        let pure: Vec<u32> = pure.into_iter().flatten().collect();
        for (threads, cells) in [(1, &one), (4, &four)] {
            let got = &cells[&pos];
            let differing = got.blocks.iter().zip(&pure).filter(|(a, b)| a != b).count();
            assert_eq!(
                differing, 0,
                "chunk {pos:?} with {threads} threads differs from the pure result"
            );
        }
        assert_eq!(one[&pos].sky, four[&pos].sky, "chunk {pos:?} sky light");
        assert_eq!(
            one[&pos].block, four[&pos].block,
            "chunk {pos:?} block light"
        );
    }
}

/// 组装前撤票的区块：生成期每区块状态（`partials`、增量表、光照、结构标签）
/// 最后都要清掉。强加载一片区块，等到有区块停在地物 / 光照之后、还没全部交付时
/// 撤掉所有票，跑到计算池排空、Director 没有记录，宿主里不能剩任何一条。
///
/// Chunks whose tickets go away before assembly must not leave per-chunk
/// generation state behind. Force-load an area, drop every ticket once some
/// chunks are past features / light but not all are delivered, run until the
/// pool drains and the Director has no records, and expect nothing left.
#[test]
fn chunks_cancelled_before_assembly_leave_no_generation_state() {
    let (_inbox, receiver) = mpsc::channel();
    let config = WorldConfig {
        view_distance: 3,
        world_gen: WorldGenConfig {
            // 只在这条测试里用的种子，不和别的测试共用地形 / 地物缓存键。
            // A seed private to this test: no shared terrain / feature cache keys.
            seed: 0x1eaf_c0de,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        },
        chunk_worker_threads: 4,
        ..WorldConfig::default()
    };
    let mut world = World::new(config, receiver);
    world.island.region.block_rules.random_tick_speed = 0;
    world.master.host.set_reclaim_delay(0);
    let positions: Vec<ChunkPos> = (0..5)
        .flat_map(|x| (0..5).map(move |z| ChunkPos::new(x, z)))
        .collect();
    for &pos in &positions {
        world.apply_ticket(TicketOp::Forced { pos, forced: true });
    }

    // 等到有区块的地物增量或光照已经回到世界线程、但还有区块没交付。
    // Wait until some features deltas or light reached the world thread while
    // some chunks are still undelivered.
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        let _ = world.tick();
        let [_, deltas, light, _] = world.master.host.generation_leftovers();
        let delivered = positions
            .iter()
            .filter(|pos| world.island.region.chunks.is_loaded(**pos))
            .count();
        if (!deltas.is_empty() || !light.is_empty()) && delivered < positions.len() {
            break;
        }
        assert!(
            delivered < positions.len(),
            "everything was delivered before a mid-pipeline state was observed"
        );
        assert!(Instant::now() < deadline, "generation never got going");
    }
    for &pos in &positions {
        world.apply_ticket(TicketOp::Forced { pos, forced: false });
    }

    let deadline = Instant::now() + Duration::from_secs(600);
    let mut quiet = 0;
    loop {
        let _ = world.tick();
        let idle = world.master.host.compute_in_flight() == 0
            && world
                .master
                .host
                .director()
                .debug_state()
                .records()
                .is_empty();
        if idle {
            quiet += 1;
            if quiet >= 8 {
                break;
            }
        } else {
            quiet = 0;
        }
        assert!(
            Instant::now() < deadline,
            "world did not settle after dropping tickets: {} in flight, {} records",
            world.master.host.compute_in_flight(),
            world.master.host.director().debug_state().records().len()
        );
    }
    let [partials, deltas, light, structures] = world.master.host.generation_leftovers();
    assert!(
        partials.is_empty() && deltas.is_empty() && light.is_empty() && structures.is_empty(),
        "leftover generation state: partials {partials:?}, feature deltas {deltas:?}, \
         light {light:?}, structures {structures:?}"
    );
}
