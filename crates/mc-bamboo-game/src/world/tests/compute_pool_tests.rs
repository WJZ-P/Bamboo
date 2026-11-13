//! 区块计算池：内联执行器和多线程池跑同一个场景，最终的世界内容逐字节相同。
//!
//! 池子的结果在下一刻开头才收回，哪个结果落在哪一刻取决于线程调度，所以两边
//! **交付给世界的时刻可以不同**；场景里每一步都先等区块平面安定下来（池子空、
//! 信箱空）再改方块，改动落在同一份内容上，最后盘上的字节必须完全一样。
//!
//! Inline executor vs. compute pool on the same scenario: delivery ticks may
//! differ (pool results arrive on a later tick, depending on scheduling), but
//! once each step settles the final on-disk bytes must be identical.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::blocks;
use mc_bamboo_storage::{
    anvil::{RegionCompression, RegionFile},
    world_dir::WorldDir,
};
use mc_bamboo_world::director::TicketOp;

use crate::{chunk_generator::FlatGenerator, world::tests::Harness};

struct TempWorld(std::path::PathBuf);

impl TempWorld {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "bamboo-pool-{tag}-{}-{:?}-{}",
            std::process::id(),
            std::thread::current().id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时世界目录");
        Self(path)
    }
}

impl Drop for TempWorld {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 跑刻直到池子里没有在途任务、信箱也空了，再多跑几刻让后续效果落地。
fn settle(server: &mut Harness) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        server.tick();
        let (deliveries, reclaims, _) = server.director_mail();
        if server.world.host.compute_in_flight() == 0 && deliveries == 0 && reclaims == 0 {
            break;
        }
        assert!(Instant::now() < deadline, "区块平面 30 秒内没有安定下来");
        std::thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..3 {
        server.tick();
    }
}

fn force(server: &mut Harness, positions: &[ChunkPos], forced: bool) {
    for &pos in positions {
        server.apply_ticket_for_test(TicketOp::Forced { pos, forced });
    }
}

fn edit(server: &mut Harness, pos: BlockPos, state: u32) {
    assert!(
        server.world.region.chunks.is_loaded(pos.chunk()),
        "改方块之前区块要已经交付：{pos:?}"
    );
    server.set_block(pos, state);
    server.world.region.touch(pos);
}

/// 盘上每个区块的两份 NBT（解压后的字节），按 (目录, 位置) 排好。
fn disk_contents(root: &std::path::Path) -> BTreeMap<(String, i32, i32), Vec<u8>> {
    let dir = WorldDir::new(root);
    let mut out = BTreeMap::new();
    for (name, folder) in [
        ("region", dir.region_dir()),
        ("entities", dir.entities_dir()),
    ] {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let parts: Vec<&str> = file_name.split('.').collect();
            let [_, rx, rz, "mca"] = parts.as_slice() else {
                continue;
            };
            let (Ok(rx), Ok(rz)) = (rx.parse::<i32>(), rz.parse::<i32>()) else {
                continue;
            };
            let mut file = RegionFile::open(entry.path(), &folder, RegionCompression::Zlib)
                .expect("区域文件能打开");
            for dx in 0..32 {
                for dz in 0..32 {
                    let pos = ChunkPos::new(rx * 32 + dx, rz * 32 + dz);
                    if let Some(bytes) = file.read(pos, 32 << 20).expect("区块能读") {
                        out.insert((name.to_owned(), pos.x, pos.z), bytes);
                    }
                }
            }
        }
    }
    out
}

/// 一个覆盖生成、解析、序列化、停放、重新加载的场景，返回关服后盘上的内容。
fn run_scenario(threads: usize) -> BTreeMap<(String, i32, i32), Vec<u8>> {
    let tmp = TempWorld::new(&format!("scenario-{threads}"));
    let mut server = Harness::new_loaded_with_save_and_threads(2, &tmp.0, threads);
    server.set_reclaim_delay(1);
    assert_eq!(
        server.world.host.compute_in_flight(),
        0,
        "刚开局没有在途任务"
    );

    // 1. 生成：相隔很远的两块 3×3，改其中几格。强制票的等级会向外传两圈
    //    （33 仍是完工档），两块离得远，卸掉一块不会被另一块的票留住。
    let first: Vec<ChunkPos> = [-12, 12]
        .into_iter()
        .flat_map(|cx| (cx - 1..=cx + 1).flat_map(|x| (-1..=1).map(move |z| ChunkPos::new(x, z))))
        .collect();
    force(&mut server, &first, true);
    settle(&mut server);
    for (index, pos) in first.iter().enumerate() {
        if index % 3 == 0 {
            let block = BlockPos::new(pos.x * 16 + 3, -60, pos.z * 16 + 5);
            edit(&mut server, block, blocks::GLOWSTONE);
        }
    }
    server.world.region.chunks.finish_light_updates();
    settle(&mut server);

    // 2. 卸载左边那块：序列化在池子里做，改过的写盘、原样的丢掉。
    let (gone, kept): (Vec<ChunkPos>, Vec<ChunkPos>) =
        first.iter().copied().partition(|pos| pos.x < 0);
    force(&mut server, &gone, false);
    settle(&mut server);
    for pos in &gone {
        assert!(
            !server.world.region.chunks.is_loaded(*pos),
            "没票的区块要被收回：{pos:?}"
        );
    }

    // 3. 再加载回来：走停放表或盘上字节（解析在池子里做），再改一轮。
    force(&mut server, &gone, true);
    settle(&mut server);
    for pos in &gone {
        let block = BlockPos::new(pos.x * 16 + 7, -59, pos.z * 16 + 7);
        edit(&mut server, block, blocks::DIAMOND_BLOCK);
    }
    for pos in &kept {
        let block = BlockPos::new(pos.x * 16 + 1, -59, pos.z * 16 + 1);
        edit(&mut server, block, blocks::GOLD_BLOCK);
    }
    server.world.region.chunks.finish_light_updates();
    settle(&mut server);

    // 4. 关服：等池子清空，全部写盘。
    server.shut_down();
    let [decoded, generated, saved] = server.world.host.pooled_kinds;
    if threads > 0 {
        assert!(
            decoded > 0 && generated > 0 && saved > 0,
            "池子要真的跑过解析、生成、序列化：{decoded} / {generated} / {saved}"
        );
    } else {
        assert_eq!(
            server.world.host.pooled_kinds, [0; 3],
            "内联执行器不经过池子"
        );
    }
    assert_eq!(server.world.host.compute_in_flight(), 0, "关服时池子已排空");
    drop(server);
    disk_contents(&tmp.0)
}

/// 核心验收：内联执行器和 4 线程池各跑一遍同样的场景，盘上内容逐字节相同。
/// 交付时刻可以不同（见模块文档），内容不能不同。
#[test]
fn inline_and_pool_produce_identical_worlds() {
    let inline = run_scenario(0);
    let pooled = run_scenario(4);
    assert!(!inline.is_empty(), "场景要写出区块");
    assert_eq!(
        inline.keys().collect::<Vec<_>>(),
        pooled.keys().collect::<Vec<_>>(),
        "两边盘上的区块集合要相同"
    );
    for (key, bytes) in &inline {
        assert!(
            pooled.get(key) == Some(bytes),
            "区块 {key:?} 的字节不同：内联和池子算出的内容必须相同"
        );
    }
}

/// 一个故意变慢的生成器（每个区块 5 ms）：内联时整批生成算在这一刻里，
/// 用池子时这一刻只是把任务交出去，生成在后台线程上做。
#[test]
fn a_slow_generator_runs_off_the_world_thread() {
    let delay = Duration::from_millis(5);
    let positions: Vec<ChunkPos> = (10..15)
        .flat_map(|x| (10..15).map(move |z| ChunkPos::new(x, z)))
        .collect();
    let generation_time = delay * positions.len() as u32;

    let mut timings = Vec::new();
    for threads in [0, 4] {
        let mut server = Harness::new_loaded_with_threads(2, threads);
        server
            .world
            .host
            .set_generator(Arc::new(FlatGenerator { delay }));
        let started = Instant::now();
        force(&mut server, &positions, true);
        server.tick();
        timings.push(started.elapsed());
        settle(&mut server);
        for pos in &positions {
            assert!(
                server.world.region.chunks.is_loaded(*pos),
                "线程数 {threads}：区块最终都要交付"
            );
        }
    }
    let (inline, pooled) = (timings[0], timings[1]);
    assert!(
        inline >= generation_time,
        "内联执行器在这一刻里做完全部生成：{inline:?} < {generation_time:?}"
    );
    assert!(
        pooled < generation_time / 2,
        "用池子时世界线程这一刻不该等生成：{pooled:?}（内联 {inline:?}）"
    );
}

/// 数一数生成器真的被调了几次。
struct CountingGenerator {
    inner: FlatGenerator,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

impl crate::chunk_generator::ChunkGenerator for CountingGenerator {
    fn generate(
        &self,
        stage: mc_bamboo_world::ticket::ChunkStatus,
        pos: ChunkPos,
        center: Option<Box<crate::region::RegionColumn>>,
        neighbors: crate::chunk_generator::NeighborView<'_>,
    ) -> Box<crate::region::RegionColumn> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.generate(stage, pos, center, neighbors)
    }
}

/// 池子里同时最多放 `线程数 × POOL_JOBS_PER_THREAD` 个任务，其余留在宿主的待办里；
/// 撤票时待办里的任务直接丢掉，不会在池子里排着。
#[test]
fn the_pool_holds_a_bounded_number_of_jobs_and_cancels_the_backlog() {
    use crate::director_host::POOL_JOBS_PER_THREAD;

    let threads = 2;
    let cap = threads * POOL_JOBS_PER_THREAD;
    let mut server = Harness::new_loaded_with_threads(2, threads);
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    server.world.host.set_generator(Arc::new(CountingGenerator {
        inner: FlatGenerator {
            delay: Duration::from_millis(20),
        },
        calls: Arc::clone(&calls),
    }));
    let positions: Vec<ChunkPos> = (40..46)
        .flat_map(|x| (40..46).map(move |z| ChunkPos::new(x, z)))
        .collect();
    force(&mut server, &positions, true);
    server.tick();
    assert!(
        server.world.host.compute_in_pool() <= cap,
        "池子里最多 {cap} 个：{}",
        server.world.host.compute_in_pool()
    );
    let queued = server.world.host.compute_backlog();
    assert!(queued > 0, "超出上限的任务留在待办里");

    // 撤掉全部票：待办里的任务当场取消，只剩池子里那几个在跑。
    force(&mut server, &positions, false);
    assert_eq!(server.world.host.compute_backlog(), 0, "撤票后待办清空");
    settle(&mut server);
    assert_eq!(server.world.host.compute_in_flight(), 0);
    let ran = calls.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        ran <= cap * 2,
        "撤票后待办里的 {queued} 个生成不该再跑：实际跑了 {ran} 个"
    );
}
