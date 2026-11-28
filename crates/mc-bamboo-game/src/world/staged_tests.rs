//! 端到端：`normal` 世界从 Director 的 `Staged` 流水线 + 计算池交付真正的地形区块。
//!
//! `director/tests.rs` 用假生成器测过 `PipelineKind::Staged` 的状态机，
//! `worldgen.rs` / `gen-world` 测过纯函数；这一条把两者接起来，确认计算池线程上
//! 调 `OverworldGenerator`、世界线程按回执交付的盒子里是噪声地形而不是超平坦模板，
//! 并且生成期光照（`light` 阶段算出的天空光 + 方块光、`isLightOn=true`）真的落进
//! 交付盒子，1、2、4、8 线程逐格相同。
//!
//! End-to-end: a `normal` world delivers real terrain through the Director's
//! `Staged` pipeline + compute pool. `director/tests.rs` covers the state
//! machine and `worldgen.rs`/`gen-world` cover the pure functions; this wires
//! them together and checks the delivered box is noise terrain, that the
//! generation-time sky + block light (`isLightOn=true`) is baked in, and that
//! 1-, 2-, 4- and 8-thread results are cell-by-cell identical.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
use mc_bamboo_data::blocks;
use mc_bamboo_storage::{
    anvil::{AnvilError, RegionCompression, RegionFile},
    chunk_io::ChunkIo,
    world_dir::WorldDir,
};
use mc_bamboo_world::{
    MIN_Y, WORLD_HEIGHT,
    director::TicketOp,
    ticket::{FULL_CHUNK_LEVEL, TicketType},
};

use crate::{
    world::{World, WorldConfig},
    world_options::{LevelType, WorldGenConfig},
};

/// 单个计算任务毫无进展的停滞上限：只要池子还在不断吐回执就不算超时，
/// 避免机器满载时按总墙钟超时误判。
///
/// Stall timeout when no single compute reply arrives: as long as the pool
/// keeps delivering replies the test does not time out under heavy CPU load.
const COMPUTE_STALL_TIMEOUT: Duration = Duration::from_secs(180);

/// 每个等待阶段的防挂死底线：待办队列非空不代表真正推进了。
///
/// Final watchdog for each wait phase: a nonempty backlog does not prove progress.
const PIPELINE_WATCHDOG: Duration = Duration::from_secs(900);

/// 池子里有在途任务时阻塞等至少一条回执，避免主线程和岛线程空转抢占计算线程。
///
/// While compute jobs are in flight, block until at least one pool reply
/// arrives so the test and island threads do not busy-spin and starve workers.
fn wait_for_compute_progress(
    world: &mut World,
    last_progress: &mut Instant,
    deadline: Instant,
    context: &str,
) {
    while world.master.host.compute_in_pool() > 0 {
        check_pipeline_deadline(deadline, context);
        if world
            .master
            .host
            .wait_for_compute_reply(Duration::from_millis(100))
        {
            *last_progress = Instant::now();
            return;
        }
        assert!(
            last_progress.elapsed() < COMPUTE_STALL_TIMEOUT,
            "{context}: no compute progress for {COMPUTE_STALL_TIMEOUT:?}"
        );
    }
}

/// 每个等待阶段的绝对防挂死底线：超过就失败，而不是无限等下去。
///
/// Absolute per-phase hang guard: fail past the deadline instead of looping forever.
fn check_pipeline_deadline(deadline: Instant, context: &str) {
    assert!(
        Instant::now() < deadline,
        "{context}: pipeline watchdog expired after {PIPELINE_WATCHDOG:?}"
    );
}

/// 区块平面是否仍有待交付 / 待处理的计算工作。
///
/// Whether the chunk plane still has pending compute or delivery work.
fn has_pending_work(world: &World) -> bool {
    let (deliveries, reclaims) = world.islands.mailbox.backlog();
    world.master.host.compute_in_flight() > 0
        || world.master.host.posted_len() > 0
        || deliveries > 0
        || reclaims > 0
}

/// 起一个 normal 世界、强制加载原点区块，等它交付，再确认里面是真正的噪声地形。
///
/// Start a normal world, force-load the origin chunk, wait for delivery, and
/// check the delivered column is real noise terrain.
#[test]
fn staged_pipeline_delivers_generated_terrain_through_the_pool() {
    let (_inbox, receiver) = crate::inbound::channel();
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
        // 避免机器极忙时默认 30 s 的岛判死时限误杀测试里的家岛。
        // Avoid tripping the default 30 s dead-island timeout on a busy host.
        island_dead_timeout: Duration::from_secs(300),
        ..WorldConfig::default()
    };
    let mut world = World::new(config, receiver);
    let origin = ChunkPos::new(0, 0);
    world.apply_ticket(TicketOp::Forced {
        pos: origin,
        forced: true,
    });

    // 交付每刻有预算，池子回执下一刻才收；池子在算时阻塞等回执，不空转抢 CPU。
    // Delivery has a per-tick budget and pool replies arrive a tick later;
    // wait on pool replies while workers run instead of busy-spinning.
    let mut last_progress = Instant::now();
    let deadline = last_progress + PIPELINE_WATCHDOG;
    let mut idle_ticks = 0_u32;
    loop {
        let _ = world.tick();
        if world.islands.region.chunks.is_loaded(origin) {
            break;
        }
        let context = "staged pipeline waiting for origin chunk";
        check_pipeline_deadline(deadline, context);
        if world.master.host.compute_in_pool() > 0 {
            idle_ticks = 0;
            wait_for_compute_progress(&mut world, &mut last_progress, deadline, context);
        } else if has_pending_work(&world) {
            last_progress = Instant::now();
            idle_ticks = 0;
        } else {
            idle_ticks += 1;
            assert!(
                idle_ticks < 20,
                "staged pipeline went idle before delivering the origin chunk"
            );
        }
    }

    // 生成的盒子 `untouched=false`，超平坦模板是 `true`：确认交付的不是模板。
    // Generated boxes are `untouched=false`; the superflat template is `true`.
    assert!(
        !world.islands.region.chunks.is_pristine(origin),
        "delivered chunk is still the pristine superflat template"
    );
    // y=-60 在超平坦里是草方块上方的空气，在噪声地形里是地下的实心方块。
    // y=-60 is air above grass in superflat, but solid underground in noise terrain.
    let deep = world
        .islands
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
    generated_cells_in_batches(seed, threads, &[positions])
}

fn generated_cells_in_batches(
    seed: i64,
    threads: usize,
    batches: &[&[ChunkPos]],
) -> BTreeMap<ChunkPos, ChunkCells> {
    let (mut world, _inbox) = staged_world(seed, threads);
    for positions in batches {
        assert!(
            positions.iter().all(|&pos| !world.is_chunk_loaded(pos)),
            "each batch must request new FULL columns"
        );
        load_full_batch(&mut world, threads, positions);
    }
    collect_generated_cells(
        &mut world,
        batches.iter().flat_map(|batch| batch.iter().copied()),
    )
}

fn staged_world(seed: i64, threads: usize) -> (World, crate::inbound::InboundSender) {
    let (inbox, receiver) = crate::inbound::channel();
    let config = WorldConfig {
        view_distance: 3,
        world_gen: WorldGenConfig {
            seed,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        },
        chunk_worker_threads: threads,
        island_dead_timeout: Duration::from_secs(300),
        ..WorldConfig::default()
    };
    let mut world = World::new(config, receiver);
    disable_random_ticks(&mut world);
    (world, inbox)
}

fn load_full_batch(world: &mut World, threads: usize, positions: &[ChunkPos]) {
    // 在 FULL 档取生成期的逐格基准；升入 BLOCK_TICKING 会消费完工表。
    // Compare the assembled FULL output before promotion consumes its generation marks.
    world.load_chunks_unpaced(positions, FULL_CHUNK_LEVEL);

    let mut last_progress = Instant::now();
    let deadline = last_progress + PIPELINE_WATCHDOG;
    let context = format!("normal-world generation (threads={threads})");
    let mut quiet = 0;
    let mut idle_unloaded_ticks = 0_u32;
    loop {
        let _ = world.tick();
        let all_loaded = positions
            .iter()
            .all(|pos| world.islands.region.chunks.is_loaded(*pos));
        let idle = world.master.host.compute_in_flight() == 0;
        check_pipeline_deadline(deadline, &context);
        if all_loaded && idle {
            quiet += 1;
            // 池子空了以后再跑几刻，让待办任务和世界线程的增量合并都落地。
            // A few extra ticks after the pool drains so pending tasks and
            // world-thread delta merges all settle.
            if quiet >= 4 {
                break;
            }
            continue;
        }
        quiet = 0;
        if world.master.host.compute_in_pool() > 0 {
            idle_unloaded_ticks = 0;
            wait_for_compute_progress(world, &mut last_progress, deadline, &context);
        } else if world
            .master
            .host
            .io()
            .is_some_and(|io| io.in_flight_requests() > 0)
        {
            world.master.host.flush_io();
            idle_unloaded_ticks = 0;
        } else if has_pending_work(world) {
            last_progress = Instant::now();
            idle_unloaded_ticks = 0;
        } else {
            idle_unloaded_ticks += 1;
            assert!(
                idle_unloaded_ticks < 40,
                "normal-world generation went idle before delivering all chunks (threads={threads})"
            );
        }
    }
}

fn assert_same_lit_cells(want: &ChunkCells, have: &ChunkCells) {
    assert!(want.light_on && have.light_on, "column is not lit");
    assert!(
        want.blocks.contains(&blocks::DEEPSLATE),
        "missing normal terrain"
    );
    assert!(want.blocks == have.blocks, "blocks differ");
    assert!(want.sky == have.sky, "sky light differs");
    assert!(want.block == have.block, "block light differs");
}

#[test]
fn promoted_corrupt_column_regenerates_like_a_missing_column() {
    let seed = 42;
    let first = ChunkPos::new(14, 6);
    let corrupt = ChunkPos::new(15, 6);
    let expected = generated_cells_in_batches(seed, 0, &[&[first], &[corrupt]]);
    for threads in [0, 4] {
        let temp = super::tests::D6TempDir::new("corrupt-staged");
        let dir = WorldDir::new(&temp.0);
        dir.create_layout().unwrap();
        let mut file = RegionFile::open(
            dir.region_dir().join("r.0.0.mca"),
            dir.region_dir(),
            RegionCompression::Zlib,
        )
        .unwrap();
        file.write(corrupt, Some(b"invalid NBT"), 1).unwrap();
        file.sync().unwrap();
        drop(file);
        let (mut world, _inbox) = staged_world(seed, threads);
        let io = ChunkIo::open(&dir).unwrap();
        assert!(io.has_chunk(corrupt));
        world.master.host.set_io(io);
        load_full_batch(&mut world, threads, &[first]);
        assert!(!world.is_chunk_loaded(corrupt));
        load_full_batch(&mut world, threads, &[corrupt]);
        let actual = collect_generated_cells(&mut world, [corrupt].into_iter());
        assert_same_lit_cells(&expected[&corrupt], &actual[&corrupt]);
        println!("corrupt promoted column regenerated (workers={threads})");
    }
}

#[test]
fn promoted_missing_external_column_regenerates_like_a_missing_column() {
    compare_failed_external_column(false);
}

#[test]
fn promoted_corrupt_external_stream_regenerates_like_a_missing_column() {
    compare_failed_external_column(true);
}

fn compare_failed_external_column(corrupt_stream: bool) {
    let seed = 42;
    let threads = 4;
    let first = ChunkPos::new(14, 6);
    let missing = ChunkPos::new(15, 6);
    let expected = generated_cells_with_seed(seed, 0, &[first, missing]);
    let temp = super::tests::D6TempDir::new("missing-external-staged");
    let dir = WorldDir::new(&temp.0);
    dir.create_layout().unwrap();
    let region = dir.region_dir();
    let compression = if corrupt_stream {
        RegionCompression::Zlib
    } else {
        RegionCompression::None
    };
    let mut file = RegionFile::open(region.join("r.0.0.mca"), &region, compression).unwrap();
    let payload = if corrupt_stream {
        let mut state = 42_u64;
        (0..1 << 20)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    } else {
        vec![0; 1 << 20]
    };
    file.write(missing, Some(&payload), 1).unwrap();
    file.sync().unwrap();
    let external = region.join("c.15.6.mcc");
    assert!(external.is_file());
    if corrupt_stream {
        std::fs::write(&external, b"not a zlib stream").unwrap();
        assert!(matches!(
            file.read(missing, 2 << 20),
            Err(AnvilError::Io(_))
        ));
    } else {
        std::fs::rename(&external, region.join("fixture.mcc.offline")).unwrap();
        assert!(matches!(
            file.read(missing, 2 << 20),
            Err(AnvilError::MissingExternal)
        ));
    }
    assert!(file.has_chunk(missing));
    drop(file);

    let (mut world, _inbox) = staged_world(seed, threads);
    let io = ChunkIo::open(&dir).unwrap();
    assert!(io.has_chunk(missing), "the scan must see the Anvil stub");
    world.master.host.set_io(io);
    load_full_batch(&mut world, threads, &[first]);
    assert!(!world.is_chunk_loaded(missing));
    load_full_batch(&mut world, threads, &[missing]);
    let actual = collect_generated_cells(&mut world, [missing].into_iter());
    assert_same_lit_cells(&expected[&missing], &actual[&missing]);
    println!(
        "external column regenerated, corrupt_stream={corrupt_stream}, cells_per_layer={}",
        WORLD_HEIGHT as usize * 256
    );
}

fn unload_and_save_full(world: &mut World, pos: ChunkPos) {
    world.apply_ticket(TicketOp::Remove {
        ty: TicketType::PlayerLoading,
        pos,
        level: FULL_CHUNK_LEVEL,
    });
    let deadline = Instant::now() + PIPELINE_WATCHDOG;
    while world.is_chunk_loaded(pos)
        || has_pending_work(world)
        || !world.master.host.save_queue.is_empty()
        || world
            .master
            .host
            .io()
            .is_some_and(|io| io.in_flight_requests() > 0 || !io.has_chunk(pos))
    {
        let _ = world.tick();
        world.master.host.flush_io();
        check_pipeline_deadline(deadline, "waiting for generated chunk reclaim and save");
    }
    world.master.host.flush_io();
}

#[test]
fn generated_full_light_survives_unload_and_disk_reopen() {
    let seed = 42;
    let threads = 4;
    let pos = ChunkPos::new(17, 7);
    let temp = super::tests::D6TempDir::new("staged-reopen");
    let dir = WorldDir::new(&temp.0);
    dir.create_layout().unwrap();
    let (mut world, mut inbox) = staged_world(seed, threads);
    world.master.host.set_io(ChunkIo::open(&dir).unwrap());
    world.master.host.set_reclaim_delay(0);
    load_full_batch(&mut world, threads, &[pos]);
    let original = collect_generated_cells(&mut world, [pos].into_iter());
    let cell = original[&pos]
        .blocks
        .iter()
        .position(|&state| state == blocks::DEEPSLATE)
        .expect("generated deepslate");
    let marker = BlockPos::new(
        pos.x * 16 + (cell % 256 / 16) as i32,
        MIN_Y + (cell / 256) as i32,
        pos.z * 16 + (cell % 16) as i32,
    );
    let markers = [blocks::OBSIDIAN, blocks::COBBLESTONE, blocks::OBSIDIAN];
    for (cycle, state) in markers.into_iter().enumerate() {
        let region = &mut world.islands.region;
        assert!(region.chunks.set_block(marker, state));
        region.touch(marker);
        region.chunks.finish_light_updates();
        let before = collect_generated_cells(&mut world, [pos].into_iter());
        assert_eq!(before[&pos].blocks[cell], state);
        assert!(original[&pos].sky == before[&pos].sky);
        assert!(original[&pos].block == before[&pos].block);
        unload_and_save_full(&mut world, pos);
        drop(world);
        drop(inbox);
        let (mut reopened, reopened_inbox) = staged_world(seed, threads);
        let io = ChunkIo::open(&dir).unwrap();
        assert!(io.has_chunk(pos), "reopen must find the saved column");
        reopened.master.host.set_io(io);
        reopened.master.host.set_reclaim_delay(0);
        load_full_batch(&mut reopened, threads, &[pos]);
        let after = collect_generated_cells(&mut reopened, [pos].into_iter());
        assert_same_lit_cells(&before[&pos], &after[&pos]);
        println!(
            "cycle={}, marker={state}, cells_per_layer={}",
            cycle + 1,
            WORLD_HEIGHT as usize * 256
        );
        world = reopened;
        inbox = reopened_inbox;
    }
}

fn collect_generated_cells(
    world: &mut World,
    positions: impl Iterator<Item = ChunkPos>,
) -> BTreeMap<ChunkPos, ChunkCells> {
    let capacity = WORLD_HEIGHT as usize * 256;
    let mut out = BTreeMap::new();
    for pos in positions {
        let light_on = world
            .islands
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
                        .push(world.islands.region.chunks.block(at).unwrap_or(blocks::AIR));
                    cells
                        .sky
                        .push(world.islands.region.chunks.light_value(LightLayer::Sky, at));
                    cells.block.push(
                        world
                            .islands
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

#[test]
fn staged_light_matches_one_shot_when_full_columns_arrive_in_strips() {
    let strips: Vec<Vec<_>> = (14..=17)
        .map(|x| (6..=8).map(|z| ChunkPos::new(x, z)).collect())
        .collect();
    let positions: Vec<_> = strips.iter().flatten().copied().collect();
    let batches: Vec<_> = strips.iter().map(Vec::as_slice).collect();
    let incremental = generated_cells_in_batches(42, 4, &batches);
    let one_shot = generated_cells_with_seed(42, 4, &positions);
    for pos in &positions {
        let a = &one_shot[pos];
        let b = &incremental[pos];
        assert!(a.light_on && b.light_on, "{pos:?}: unlit column");
        assert!(a.blocks.contains(&blocks::DEEPSLATE), "{pos:?}: no terrain");
        if let Some((cell, (&a, &b))) = a
            .blocks
            .iter()
            .zip(&b.blocks)
            .enumerate()
            .find(|(_, (a, b))| a != b)
        {
            let at = BlockPos::new(
                pos.x * 16 + (cell % 256 / 16) as i32,
                MIN_Y + (cell / 256) as i32,
                pos.z * 16 + (cell % 16) as i32,
            );
            panic!("{at:?}: one-shot block {a}, incremental block {b}");
        }
        assert!(a.sky == b.sky, "{pos:?}: sky light differs");
        assert!(a.block == b.block, "{pos:?}: block light differs");
    }
    println!(
        "batches={}, chunks={}, cells_per_layer={}",
        batches.len(),
        positions.len(),
        positions.len() * WORLD_HEIGHT as usize * 256
    );
}

/// 验收：1、2、4、8 线程的计算池生成同一片 normal 世界区块，逐格方块、天空光、
/// 方块光都完全相同。`features` 的邻居写增量按 `TaskId` 顺序追加、完工按来源中心的
/// 固定先后合并，`light` 阶段的生成期光照是从只读快照算出的纯函数，两者都与线程
/// 调度无关。
///
/// Acceptance: 1-, 2-, 4- and 8-thread compute pools produce byte-identical
/// blocks AND sky/block light for the same area. Feature deltas merge in
/// `TaskId` order and are merged in a fixed source order; generation-time light is a pure
/// function of the read-only snapshots, so neither depends on scheduling.
#[test]
fn worker_counts_generate_identical_feature_blocks() {
    let positions: Vec<ChunkPos> = (-1..=1)
        .flat_map(|x| (-1..=1).map(move |z| ChunkPos::new(x, z)))
        .collect();
    let one = generated_cells(1, &positions);
    assert_eq!(one.len(), 9);
    for threads in [2, 4, 8] {
        let other = generated_cells(threads, &positions);
        assert!(
            one.keys().eq(other.keys()),
            "loaded chunks differ with {threads} workers"
        );
        for (pos, a) in &one {
            let b = &other[pos];
            assert_eq!(
                a.blocks, b.blocks,
                "chunk {pos:?} blocks differ with {threads} workers"
            );
            assert_eq!(
                a.sky, b.sky,
                "chunk {pos:?} sky light differs with {threads} workers"
            );
            assert_eq!(
                a.block, b.block,
                "chunk {pos:?} block light differs with {threads} workers"
            );
            assert_eq!(
                a.light_on, b.light_on,
                "chunk {pos:?} isLightOn differs with {threads} workers"
            );
        }
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
/// 方块和纯函数（`build_view` 3×3 + 8 个邻居中心的溢出，按来源固定先后合并）逐格相同，1 线程
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
    let (_inbox, receiver) = crate::inbound::channel();
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
        island_dead_timeout: Duration::from_secs(300),
        ..WorldConfig::default()
    };
    let mut world = World::new(config, receiver);
    disable_random_ticks(&mut world);
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
    let mut last_progress = Instant::now();
    let deadline = last_progress + PIPELINE_WATCHDOG;
    let context = "waiting for mid-pipeline generation state";
    let mut idle_ticks = 0_u32;
    loop {
        let _ = world.tick();
        let [_, deltas, light, _] = world.master.host.generation_leftovers();
        let delivered = positions
            .iter()
            .filter(|pos| world.islands.region.chunks.is_loaded(**pos))
            .count();
        if (!deltas.is_empty() || !light.is_empty()) && delivered < positions.len() {
            break;
        }
        check_pipeline_deadline(deadline, context);
        assert!(
            delivered < positions.len(),
            "everything was delivered before a mid-pipeline state was observed"
        );
        if world.master.host.compute_in_pool() > 0 {
            idle_ticks = 0;
            wait_for_compute_progress(&mut world, &mut last_progress, deadline, context);
        } else if has_pending_work(&world) {
            last_progress = Instant::now();
            idle_ticks = 0;
        } else {
            idle_ticks += 1;
            assert!(idle_ticks < 40, "generation never got going");
        }
    }
    for &pos in &positions {
        world.apply_ticket(TicketOp::Forced { pos, forced: false });
    }

    let mut last_progress = Instant::now();
    let deadline = last_progress + PIPELINE_WATCHDOG;
    let context = "draining compute pool after dropping tickets";
    let mut quiet = 0;
    let mut stalled_ticks = 0_u32;
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
        check_pipeline_deadline(deadline, context);
        if idle {
            quiet += 1;
            if quiet >= 8 {
                break;
            }
            continue;
        }
        quiet = 0;
        if world.master.host.compute_in_pool() > 0 {
            stalled_ticks = 0;
            wait_for_compute_progress(&mut world, &mut last_progress, deadline, context);
        } else if has_pending_work(&world) {
            last_progress = Instant::now();
            stalled_ticks = 0;
        } else {
            stalled_ticks += 1;
            assert!(
                stalled_ticks < 200,
                "world did not settle after dropping tickets: {} in flight, {} records",
                world.master.host.compute_in_flight(),
                world.master.host.director().debug_state().records().len()
            );
        }
    }
    let [partials, deltas, light, structures] = world.master.host.generation_leftovers();
    assert!(
        partials.is_empty() && deltas.is_empty() && light.is_empty() && structures.is_empty(),
        "leftover generation state: partials {partials:?}, feature deltas {deltas:?}, \
         light {light:?}, structures {structures:?}"
    );
}

/// 关掉随机刻：改权威的游戏规则再发布，快照每刻都会从 `Global` 重发。
/// Turn random ticks off on the authoritative rules; the snapshot is republished every tick.
fn disable_random_ticks(world: &mut World) {
    world
        .master
        .global
        .rules_mut()
        .assign(mc_bamboo_data::gamerules::RANDOM_TICK_SPEED, "0");
    world.master.global.publish(&mut world.islands.region);
}

/// 种子 42 的世界：推到 `positions` 全部交付、计算池排空。连同收件箱的发送端一起
/// 交回：发送端一丢，世界就当成服务端停了。
/// A seed-42 world ticked until `positions` are delivered and the pool is idle,
/// returned with the inbox sender: dropping it reads as a server shutdown.
fn structured_world(positions: &[ChunkPos], level: i32) -> (World, crate::inbound::InboundSender) {
    let (inbox, receiver) = crate::inbound::channel();
    let config = WorldConfig {
        view_distance: 3,
        world_gen: WorldGenConfig {
            seed: 42,
            level_type: LevelType::Normal,
            generate_structures: true,
            ..WorldGenConfig::default()
        },
        chunk_worker_threads: 4,
        mail_budget: crate::MailBudget {
            deliveries: 0,
            reclaims: 0,
        },
        ..WorldConfig::default()
    };
    let mut world = World::new(config, receiver);
    world.load_chunks_unpaced(positions, level);
    let deadline = Instant::now() + Duration::from_secs(900);
    let mut quiet = 0;
    while quiet < 4 {
        let _ = world.tick();
        let ready = positions.iter().all(|&pos| world.is_chunk_loaded(pos))
            && world.master.host.compute_in_flight() == 0;
        quiet = if ready { quiet + 1 } else { 0 };
        assert!(
            Instant::now() < deadline,
            "structured chunks were not delivered"
        );
    }
    (world, inbox)
}

/// 结构写下的方块实体跟着九中心组装进交付盒子：沙漠神殿 (-375,-22) 的四个箱子带
/// 战利品表。以前组装只传方块，箱子没有方块实体。
#[test]
fn structure_chests_arrive_with_their_loot_tables() {
    let pos = ChunkPos::new(-375, -22);
    let (world, _inbox) = structured_world(&[pos], mc_bamboo_world::ticket::FULL_CHUNK_LEVEL);
    let root = world
        .chunk_nbt_unpaced(pos)
        .expect("chunk loaded")
        .expect("chunk encodes");
    let mc_bamboo_nbt::Tag::Compound(entries) = &root else {
        panic!("chunk root is not a compound");
    };
    let Some((_, mc_bamboo_nbt::Tag::List(entities))) =
        entries.iter().find(|(key, _)| key == "block_entities")
    else {
        panic!("no block_entities list");
    };
    let chests: Vec<&mc_bamboo_nbt::Tag> = entities
        .iter()
        .filter(|tag| {
            matches!(tag, mc_bamboo_nbt::Tag::Compound(fields) if fields.iter().any(|(key, value)| {
                key == "id" && *value == mc_bamboo_nbt::Tag::String("minecraft:chest".to_owned())
            }))
        })
        .collect();
    assert_eq!(chests.len(), 4, "the desert pyramid has four chests");
    for chest in chests {
        let mc_bamboo_nbt::Tag::Compound(fields) = chest else {
            unreachable!()
        };
        let table = fields.iter().find(|(key, _)| key == "LootTable");
        assert_eq!(
            table.map(|(_, value)| value),
            Some(&mc_bamboo_nbt::Tag::String(
                "minecraft:chests/desert_pyramid".to_owned()
            )),
            "chest {fields:?}"
        );
    }
}

/// 加载等级升进 `BLOCK_TICKING` 时做完工后处理：沼泽小屋 (-17,22) 的栅栏在 FULL 时
/// 还没连，升档后连好，表清空；之前不能发给玩家。
#[test]
fn promotion_to_block_ticking_post_processes_the_swamp_hut() {
    let pos = ChunkPos::new(-17, 22);
    let fence = BlockPos::new(-271, 66, 353);
    // 3×3 都先到 FULL：中心升档时邻居已就位，后处理不等生成。
    // The whole 3×3 reaches FULL first so promotion does not wait on generation.
    let ring: Vec<ChunkPos> = (-1..=1)
        .flat_map(|dz| (-1..=1).map(move |dx| ChunkPos::new(pos.x + dx, pos.z + dz)))
        .collect();
    let (mut world, _inbox) = structured_world(&ring, mc_bamboo_world::ticket::FULL_CHUNK_LEVEL);
    let state = |world: &World| world.chunk_blocks_unpaced(pos).expect("delivered");
    let index = |at: BlockPos| (((at.y - MIN_Y) * 16 + (at.x & 15)) * 16 + (at.z & 15)) as usize;
    assert_eq!(state(&world)[index(fence)], blocks::OAK_FENCE);
    assert!(!world.chunk_ready_to_send_unpaced(pos));
    let (table, _) = world
        .chunk_post_processing_unpaced(pos)
        .expect("table")
        .expect("table encodes");
    assert!(table.iter().map(Vec::len).sum::<usize>() > 0);

    world.load_chunks_unpaced(&[pos], mc_bamboo_world::ticket::BLOCK_TICKING_LEVEL);
    let deadline = Instant::now() + Duration::from_secs(300);
    while !world.chunk_post_processed_unpaced(pos) {
        let _ = world.tick();
        assert!(Instant::now() < deadline, "the hut never became ticking");
    }
    // 只连东边：比缺省（全 false）小 16。
    assert_eq!(state(&world)[index(fence)], blocks::OAK_FENCE - 16);
    assert!(world.chunk_ready_to_send_unpaced(pos));
    let (delivered, reloaded) = world
        .chunk_post_processing_unpaced(pos)
        .expect("table")
        .expect("table encodes");
    assert!(delivered.iter().chain(&reloaded).all(Vec::is_empty));
}
