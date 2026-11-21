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

use crate::{chunk_generator::FlatGenerator, island::IslandDrive, world::tests::Harness};

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
/// 池子有在途任务时阻塞等一条回执再推进，避免空转抢 CPU。
///
/// Tick until the pool and mailboxes are empty, then run a few more ticks to
/// settle follow-up effects. Waits on a pool reply while jobs are in flight so
/// busy-spinning does not starve worker threads.
fn settle(server: &mut Harness) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        server.tick();
        let (deliveries, reclaims, _) = server.director_mail();
        if server.world.master.host.compute_in_flight() == 0 && deliveries == 0 && reclaims == 0 {
            break;
        }
        assert!(Instant::now() < deadline, "区块平面 60 秒内没有安定下来");
        if server.world.master.host.compute_in_pool() > 0 {
            server
                .world
                .master
                .host
                .wait_for_compute_reply(Duration::from_millis(50));
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
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
        server.world.islands.region.chunks.is_loaded(pos.chunk()),
        "改方块之前区块要已经交付：{pos:?}"
    );
    server.set_block(pos, state);
    server.world.islands.region.touch(pos);
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
fn run_scenario(threads: usize, drive: IslandDrive) -> BTreeMap<(String, i32, i32), Vec<u8>> {
    let tmp = TempWorld::new(&format!("scenario-{threads}-{drive:?}"));
    let mut server = Harness::new_loaded_with_save_threads_and_drive(2, &tmp.0, threads, drive);
    server.set_reclaim_delay(1);
    assert_eq!(
        server.world.master.host.compute_in_flight(),
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
    server.world.islands.region.chunks.finish_light_updates();
    settle(&mut server);

    // 2. 卸载左边那块：序列化在池子里做，改过的写盘、原样的丢掉。
    let (gone, kept): (Vec<ChunkPos>, Vec<ChunkPos>) =
        first.iter().copied().partition(|pos| pos.x < 0);
    force(&mut server, &gone, false);
    settle(&mut server);
    for pos in &gone {
        assert!(
            !server.world.islands.region.chunks.is_loaded(*pos),
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
    server.world.islands.region.chunks.finish_light_updates();
    settle(&mut server);

    // 4. 关服：等池子清空，全部写盘。
    server.shut_down();
    let [decoded, generated, saved] = server.world.master.host.pooled_kinds;
    if threads > 0 {
        assert!(
            decoded > 0 && generated > 0 && saved > 0,
            "池子要真的跑过解析、生成、序列化：{decoded} / {generated} / {saved}"
        );
    } else {
        assert_eq!(
            server.world.master.host.pooled_kinds, [0; 3],
            "内联执行器不经过池子"
        );
    }
    assert_eq!(
        server.world.master.host.compute_in_flight(),
        0,
        "关服时池子已排空"
    );
    drop(server);
    disk_contents(&tmp.0)
}

/// 核心验收：内联执行器和 4 线程池各跑一遍同样的场景，盘上内容逐字节相同。
/// 交付时刻可以不同（见模块文档），内容不能不同。齐步和分离两种驱动各跑一遍，
/// 两种驱动之间也必须逐字节相同（分离只是交付晚一刻）。
///
/// Inline vs. a 4-thread pool, under both the lockstep and the detached drive:
/// the bytes on disk must match pairwise and across drives (detached only
/// delivers one tick later).
#[test]
fn inline_and_pool_produce_identical_worlds() {
    let mut worlds = Vec::new();
    for drive in [IslandDrive::Lockstep, IslandDrive::Detached] {
        for threads in [0, 4] {
            worlds.push(((drive, threads), run_scenario(threads, drive)));
        }
    }
    let ((_, _), reference) = &worlds[0];
    assert!(!reference.is_empty(), "场景要写出区块");
    for ((drive, threads), world) in &worlds[1..] {
        assert_eq!(
            reference.keys().collect::<Vec<_>>(),
            world.keys().collect::<Vec<_>>(),
            "{drive:?} / {threads} 线程：盘上的区块集合要和齐步内联相同"
        );
        for (key, bytes) in reference {
            assert!(
                world.get(key) == Some(bytes),
                "{drive:?} / {threads} 线程：区块 {key:?} 的字节和齐步内联不同"
            );
        }
    }
}

/// 测试用带闸生成器：统计生成落在世界线程还是后台线程，并可在闸门关闭时阻塞
/// 后台线程，用来在不依赖墙钟快慢的前提下证明「世界线程不会在 tick 内等生成完成」。
///
/// Gated test generator: counts calls on the world thread vs. background pool
/// threads, and blocks pool threads while the gate is closed so tests can prove
/// the world thread does not wait for generation without relying on wall-clock timing.
struct GatedGenerator {
    inner: FlatGenerator,
    world_thread: std::thread::ThreadId,
    gate: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    on_world_thread: Arc<std::sync::atomic::AtomicUsize>,
    off_thread_started: Arc<std::sync::atomic::AtomicUsize>,
    off_thread_finished: Arc<std::sync::atomic::AtomicUsize>,
    gate_timed_out: Arc<std::sync::atomic::AtomicBool>,
}

impl GatedGenerator {
    fn new(world_thread: std::thread::ThreadId, initially_open: bool) -> (Arc<Self>, GateHandle) {
        let gate = Arc::new((
            std::sync::Mutex::new(initially_open),
            std::sync::Condvar::new(),
        ));
        let on_world_thread = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let off_thread_started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let off_thread_finished = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let gate_timed_out = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let generator = Arc::new(Self {
            inner: FlatGenerator::default(),
            world_thread,
            gate: Arc::clone(&gate),
            on_world_thread: Arc::clone(&on_world_thread),
            off_thread_started: Arc::clone(&off_thread_started),
            off_thread_finished: Arc::clone(&off_thread_finished),
            gate_timed_out: Arc::clone(&gate_timed_out),
        });
        let handle = GateHandle {
            gate,
            on_world_thread,
            off_thread_started,
            off_thread_finished,
            gate_timed_out,
        };
        (generator, handle)
    }
}

struct GateHandle {
    gate: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    on_world_thread: Arc<std::sync::atomic::AtomicUsize>,
    off_thread_started: Arc<std::sync::atomic::AtomicUsize>,
    off_thread_finished: Arc<std::sync::atomic::AtomicUsize>,
    gate_timed_out: Arc<std::sync::atomic::AtomicBool>,
}

impl GateHandle {
    fn open(&self) {
        let (lock, cvar) = &*self.gate;
        let mut open = lock.lock().unwrap_or_else(|e| e.into_inner());
        *open = true;
        cvar.notify_all();
    }
}

impl Drop for GateHandle {
    fn drop(&mut self) {
        // 哪怕测试提前 panic 也要开闸，免得计算池线程在析构 join 时卡死。
        // Always open the gate on unwind so pool drop/join never hangs.
        self.open();
    }
}

impl crate::chunk_generator::ChunkGenerator for GatedGenerator {
    fn generate(
        &self,
        stage: mc_bamboo_world::ticket::ChunkStatus,
        pos: ChunkPos,
        center: Option<Box<crate::region::RegionColumn>>,
        center_snapshot: crate::chunk_generator::CenterSnapshot,
        neighbors: crate::chunk_generator::NeighborView<'_>,
    ) -> crate::worldgen_features::GenerateOutput {
        use std::sync::atomic::Ordering;
        if std::thread::current().id() == self.world_thread {
            self.on_world_thread.fetch_add(1, Ordering::SeqCst);
            // 不卡死世界线程，立刻返回让断言直接报出在世界线程上跑了。
            // Never deadlock the world thread; return so assertions fail immediately.
            return self
                .inner
                .generate(stage, pos, center, center_snapshot, neighbors);
        }
        self.off_thread_started.fetch_add(1, Ordering::SeqCst);
        let (lock, cvar) = &*self.gate;
        let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let (_guard, wait) = cvar
            .wait_timeout_while(guard, Duration::from_secs(60), |open| !*open)
            .unwrap_or_else(|e| e.into_inner());
        if wait.timed_out() {
            self.gate_timed_out.store(true, Ordering::SeqCst);
        }
        drop(_guard);
        self.off_thread_finished.fetch_add(1, Ordering::SeqCst);
        self.inner
            .generate(stage, pos, center, center_snapshot, neighbors)
    }
}

/// 慢生成不阻塞世界线程：内联时生成都在世界线程上同步完成；用池子时生成全在后台线程
/// 上做，而且 `server.tick()` 在后台生成被闸门挡住、尚未完成时就能直接返回。
///
/// A slow generator runs off the world thread: inline finishes every generation
/// synchronously on the world thread during `tick()`; with a pool, generation
/// runs only on worker threads and `tick()` returns while the workers are still
/// blocked behind a closed gate.
#[test]
fn a_slow_generator_runs_off_the_world_thread() {
    use std::sync::atomic::Ordering;

    let positions: Vec<ChunkPos> = (10..15)
        .flat_map(|x| (10..15).map(move |z| ChunkPos::new(x, z)))
        .collect();
    let world_thread = std::thread::current().id();

    // 1. 内联执行器：所有生成同步跑在世界线程上。
    // 1. Inline executor: all generation runs synchronously on the world thread.
    {
        let mut server = Harness::new_loaded_with_threads(2, 0);
        let (generator, handle) = GatedGenerator::new(world_thread, true);
        server.world.master.host.set_generator(generator);
        force(&mut server, &positions, true);
        server.tick();
        assert!(
            handle.on_world_thread.load(Ordering::SeqCst) >= positions.len(),
            "内联执行器应在世界线程这一刻里做完全部生成"
        );
        assert_eq!(
            handle.off_thread_started.load(Ordering::SeqCst),
            0,
            "内联执行器不该切到后台线程"
        );
        settle(&mut server);
        for pos in &positions {
            assert!(
                server.world.islands.region.chunks.is_loaded(*pos),
                "内联执行器：区块最终都要交付"
            );
        }
    }

    // 2. 计算池：闸门关着时 `tick()` 照常返回，生成只在后台线程开始并在开闸后完成。
    // 2. Compute pool: `tick()` returns while the gate is closed; generation only
    //    starts on worker threads and completes once the gate opens.
    {
        let mut server = Harness::new_loaded_with_threads(2, 4);
        let (generator, handle) = GatedGenerator::new(world_thread, false);
        server.world.master.host.set_generator(generator);
        force(&mut server, &positions, true);
        server.tick();

        assert_eq!(
            handle.on_world_thread.load(Ordering::SeqCst),
            0,
            "用池子时不该在世界线程上跑生成"
        );
        assert!(
            !handle.gate_timed_out.load(Ordering::SeqCst),
            "世界线程这一刻不该阻塞等待后台生成完成"
        );
        assert_eq!(
            handle.off_thread_finished.load(Ordering::SeqCst),
            0,
            "闸门还没开，后台生成不该已经完成"
        );
        assert!(
            server.world.master.host.compute_in_flight() > 0,
            "生成任务应在池子里在途"
        );

        // 等至少一条池子线程真的进到生成器里，证明后台正在独立跑生成。
        // Wait until at least one worker actually enters `generate` off-thread.
        let deadline = Instant::now() + Duration::from_secs(60);
        while handle.off_thread_started.load(Ordering::SeqCst) == 0 {
            assert!(
                Instant::now() < deadline,
                "计算池线程 60 秒内没有开始跑生成任务"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            handle.on_world_thread.load(Ordering::SeqCst),
            0,
            "后台开跑期间世界线程仍未执行任何生成"
        );

        handle.open();
        settle(&mut server);
        assert_eq!(handle.on_world_thread.load(Ordering::SeqCst), 0);
        assert!(!handle.gate_timed_out.load(Ordering::SeqCst));
        assert!(
            handle.off_thread_finished.load(Ordering::SeqCst) >= positions.len(),
            "开闸后全部生成都应在后台线程上完成"
        );
        for pos in &positions {
            assert!(
                server.world.islands.region.chunks.is_loaded(*pos),
                "计算池：区块最终都要交付"
            );
        }
    }
}

/// 池子里同时最多放 `线程数 × POOL_JOBS_PER_THREAD` 个任务，其余留在宿主的待办里；
/// 撤票时待办里的任务直接丢掉，不会在池子里排着。
///
/// The pool holds at most `threads * POOL_JOBS_PER_THREAD` jobs while the rest
/// stay in the host backlog; dropping tickets cancels the backlog immediately.
#[test]
fn the_pool_holds_a_bounded_number_of_jobs_and_cancels_the_backlog() {
    use std::sync::atomic::Ordering;

    use crate::director_host::POOL_JOBS_PER_THREAD;

    let threads = 2;
    let cap = threads * POOL_JOBS_PER_THREAD;
    let mut server = Harness::new_loaded_with_threads(2, threads);
    // 用闸门卡住进池子的任务，不靠固定睡眠，确保撤票前任务不会跑完。
    // Block pooled jobs behind a gate instead of a fixed sleep so nothing races
    // ticket cancellation.
    let (generator, handle) = GatedGenerator::new(std::thread::current().id(), false);
    server.world.master.host.set_generator(generator);
    let positions: Vec<ChunkPos> = (40..46)
        .flat_map(|x| (40..46).map(move |z| ChunkPos::new(x, z)))
        .collect();
    force(&mut server, &positions, true);
    server.tick();
    assert!(
        server.world.master.host.compute_in_pool() <= cap,
        "池子里最多 {cap} 个：{}",
        server.world.master.host.compute_in_pool()
    );
    let queued = server.world.master.host.compute_backlog();
    assert!(queued > 0, "超出上限的任务留在待办里");

    // 撤掉全部票：待办里的任务当场取消，只剩池子里那几个在跑。
    force(&mut server, &positions, false);
    assert_eq!(
        server.world.master.host.compute_backlog(),
        0,
        "撤票后待办清空"
    );
    handle.open();
    settle(&mut server);
    assert_eq!(server.world.master.host.compute_in_flight(), 0);
    let ran = handle.off_thread_finished.load(Ordering::SeqCst);
    assert!(
        ran <= cap * 2,
        "撤票后待办里的 {queued} 个生成不该再跑：实际跑了 {ran} 个"
    );
}
