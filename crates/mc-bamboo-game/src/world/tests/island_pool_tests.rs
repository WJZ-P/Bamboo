//! 岛的 tick 线程池（岛的实施计划第 6 节第 7 步）：分离驱动下岛真的在 tick 线程上跑，
//! 玩家收到的字节和线程数无关；岛 panic 时走世界线程的崩溃路径；岛卡住时关服不死等；
//! `/stop` 和 `/save-all` 的栅栏带上最后一刻岛上的改动。会卡死的测试都带超时。
//!
//! The island tick-thread pool (island plan, section 6, step 7): the detached
//! island really runs on a tick thread, what players receive does not depend
//! on the thread count, an island panic takes the world thread's crash path,
//! shutdown never waits forever for a stuck island, and the `/stop` and
//! `/save-all` barriers include the island's last tick. Every test that could
//! hang runs under a timeout.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use mc_bamboo_blocks::ticks::TickPriority;
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::blocks;

use super::tests::{D6TempDir, Harness};
use crate::{
    inbound::{Inbound, PlayerId},
    island::{IslandDrive, IslandTestHook},
    island_pool::ISLAND_THREAD_STACK,
    profiler::{Metric, Series},
    save_session::SaveCommand,
};

/// 每个测试最多跑这么久。
const LIMIT: Duration = Duration::from_secs(120);

/// 在一条大栈的线程上跑 `body`，最多等 `LIMIT`：超时算失败（不让卡死的测试挂住整套），
/// `body` 自己 panic 就原样抛出来。
fn with_timeout<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    let (done, result) = mpsc::channel();
    let handle = thread::Builder::new()
        .stack_size(ISLAND_THREAD_STACK)
        .spawn(move || {
            let _ = done.send(body());
        })
        .unwrap();
    match result.recv_timeout(LIMIT) {
        Ok(value) => value,
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("测试超过 {LIMIT:?}，可能卡死了"),
        Err(mpsc::RecvTimeoutError::Disconnected) => match handle.join() {
            Err(payload) => std::panic::resume_unwind(payload),
            Ok(()) => unreachable!("线程正常结束一定发过结果"),
        },
    }
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default()
}

/// 分离驱动下岛的一刻在 `bamboo-island-0` 上跑，不在主控（测试）线程上；每刻都记下
/// 主控、岛和交接三个耗时。齐步不起线程池，这三项都不记。
///
/// Under the detached drive the island's tick runs on `bamboo-island-0`, not
/// on the master (test) thread, and each tick records the master, island and
/// hand-over times. Lockstep starts no pool and records none of them.
#[test]
fn the_detached_island_ticks_on_its_own_thread() {
    with_timeout(|| {
        let mut server = Harness::new_loaded_with_drive(2, IslandDrive::Detached);
        let _client = server.join("Kitten");
        assert_eq!(server.world.master.pool.as_ref().unwrap().threads(), 1);
        let seen = Arc::new(Mutex::new(None));
        server.world.islands.test_hook = Some(IslandTestHook::RecordThread(Arc::clone(&seen)));
        for _ in 0..5 {
            server.tick();
        }
        assert_eq!(seen.lock().unwrap().as_deref(), Some("bamboo-island-0"));
        assert_ne!(thread::current().name(), Some("bamboo-island-0"));
        let profiler = &server.world.islands.region.profiler;
        for metric in [
            Metric::MasterBusy,
            Metric::IslandBusy,
            Metric::IslandHandoff,
        ] {
            assert_eq!(profiler.histogram(Series::Metric(metric)).count(), 5);
        }

        let mut server = Harness::new_loaded_with_drive(2, IslandDrive::Lockstep);
        assert!(server.world.master.pool.is_none());
        let seen = Arc::new(Mutex::new(None));
        server.world.islands.test_hook = Some(IslandTestHook::RecordThread(Arc::clone(&seen)));
        server.tick();
        assert_eq!(*seen.lock().unwrap(), None, "齐步不走 tick 线程的入口");
        let profiler = &server.world.islands.region.profiler;
        assert_eq!(
            profiler
                .histogram(Series::Metric(Metric::IslandHandoff))
                .count(),
            0
        );
    });
}

/// 一个没有区块信件的场景（无限超平坦：进服、走动、掉落物、下落方块、TNT）：
/// 玩家收到的字节和岛的线程数无关，分离驱动也和齐步逐位相同。分离和齐步唯一的
/// 差别是区块信件晚一刻，这里没有信件，所以一个字节都不能差。
///
/// A scenario without chunk mail (infinite flat: join, walk, items, falling
/// blocks, TNT): the bytes players receive do not depend on the island thread
/// count, and detached equals lockstep bit for bit, since the only difference
/// between them is chunk mail arriving one tick later.
#[test]
fn players_receive_the_same_bytes_for_any_island_thread_count() {
    fn scenario(threads: usize) -> Vec<u8> {
        use mc_bamboo_blocks::{BlockState, entity::Entity};
        use mc_bamboo_protocol::{item::ItemStack, packet::play::PlayRequest};

        let mut server = Harness::new_with_island_threads(2, false, threads);
        let client = server.join_and_settle("Kitten");
        let floor = BlockPos::new(8, -61, 8);
        for dx in 0..4 {
            server.set_block(floor.offset(dx, 0, 0), blocks::STONE);
        }
        for dx in 0..3 {
            server.spawn_entity(Entity::ground_item(
                Vec3::new(8.5 + f64::from(dx) * 0.2, -59.5, 8.5),
                ItemStack::new(1, 1),
            ));
        }
        server.spawn_entity(Entity::falling_block(
            BlockPos::new(10, -55, 8),
            BlockState(blocks::SAND),
        ));
        server.spawn_entity(Entity::primed_tnt(Vec3::new(12.5, -58.5, 8.5), 0.25));
        let mut bytes = client
            .take_frames()
            .into_iter()
            .flat_map(|frame| frame.body)
            .collect::<Vec<u8>>();
        for tick in 0..40 {
            if tick % 4 == 0 {
                server.request(
                    &client,
                    PlayRequest::MovePlayer {
                        position: Some(Vec3::new(0.5 + f64::from(tick) * 0.1, -60.0, 0.5)),
                        rotation: None,
                        on_ground: true,
                        horizontal_collision: false,
                    },
                );
            }
            server.tick();
            for frame in client.take_frames() {
                mc_bamboo_protocol::codec::write_varint(&mut bytes, frame.id);
                bytes.extend_from_slice(&frame.body);
            }
        }
        bytes
    }
    with_timeout(|| {
        let lockstep = scenario(0);
        assert!(!lockstep.is_empty(), "场景要真的发包");
        assert_eq!(scenario(1), lockstep, "分离一条线程");
        assert_eq!(scenario(3), lockstep, "分离三条线程");
    });
}

/// 岛在 tick 线程上 panic：主控发现它，在世界线程上原样抛出（走服务端原来的
/// 「世界线程 panic」处理），不会静默挂起；岛交回了主控手里。
///
/// An island panic on its tick thread is found by the master and re-raised on
/// the world thread (the server's existing world-thread panic path), never a
/// silent hang; the island is back in the master's hands.
#[test]
fn an_island_panic_reaches_the_world_thread() {
    let (message, home) = with_timeout(|| {
        let mut server = Harness::new_loaded_with_drive(2, IslandDrive::Detached);
        server.tick();
        server.world.islands.test_hook = Some(IslandTestHook::Panic);
        let outcome = catch_unwind(AssertUnwindSafe(|| server.world.tick()));
        let message = outcome.err().map(|payload| panic_text(payload.as_ref()));
        (message, server.world.islands.is_home())
    });
    assert_eq!(message.as_deref(), Some("岛的测试 panic"));
    assert!(home);
}

/// 岛卡在 tick 线程上时关服：主控收到关服请求后再等 `island_stop_timeout`，然后不等了，
/// 照样把全局状态写盘、放掉世界目录的锁（同一个目录马上能重开，游戏时间接得上）。
/// 没收到关服请求之前一直等（慢的一刻不会被当成卡死）。
///
/// Shutdown with the island stuck on its tick thread: after the request the
/// master waits `island_stop_timeout` more, then stops without it, still
/// writing global state and releasing the world lock (the directory reopens at
/// once with the game time intact). Before a request it keeps waiting.
#[test]
fn shutdown_gives_up_on_a_stuck_island_but_still_saves_global_state() {
    let tmp = D6TempDir::new("stuck-island");
    let root = tmp.0.clone();
    let saved_time = with_timeout(move || {
        let mut server =
            Harness::new_loaded_with_save_threads_and_drive(2, &root, 0, IslandDrive::Detached);
        server.ensure_loaded(&[ChunkPos::new(0, 0)]);
        for _ in 0..30 {
            server.tick();
        }
        server.world.master.island_stop_timeout = Duration::from_millis(300);
        let release = Arc::new(AtomicBool::new(false));
        server.world.islands.test_hook = Some(IslandTestHook::Stall(Arc::clone(&release)));
        let sender = server.inbox_sender();
        let (done, result) = mpsc::channel();
        let ticker = thread::Builder::new()
            .stack_size(ISLAND_THREAD_STACK)
            .spawn(move || {
                let started = Instant::now();
                let flow = server.world.tick();
                let _ = done.send((flow.is_break(), started.elapsed()));
                server
            })
            .unwrap();
        // 慢的一刻：没有关服请求时主控一直等，不会自己放弃。
        // A slow tick: without a shutdown request the master keeps waiting.
        assert!(result.recv_timeout(Duration::from_millis(600)).is_err());
        let requested = Instant::now();
        sender.send(Inbound::Shutdown).unwrap();
        let (broke, _) = result
            .recv_timeout(Duration::from_secs(30))
            .expect("关服不能死等卡住的岛");
        let waited = requested.elapsed();
        let server = ticker.join().unwrap();
        assert!(broke, "关服结束世界循环");
        assert!(waited >= Duration::from_millis(300), "先等够 {waited:?}");
        assert!(!server.world.islands.is_home(), "岛还卡在线程上");
        let time = server.world.master.global.time().0;
        drop(server);
        // 让卡住的线程走完、自己退出；交回的通道已经没人收了。
        // Let the stuck thread finish and exit; nobody receives its hand-back.
        release.store(true, Ordering::Release);
        time
    });
    assert!(saved_time > 0);
    let root = tmp.0.clone();
    let reopened_time = with_timeout(move || {
        let server =
            Harness::new_loaded_with_save_threads_and_drive(2, &root, 0, IslandDrive::Lockstep);
        assert!(server.world.master.startup_error.is_none(), "锁已经放掉");
        server.world.master.global.time().0
    });
    assert_eq!(reopened_time, saved_time, "level.dat 写出去了");
}

/// 一块悬空的沙子下一刻在岛上掉下去（方块变成空气、生成下落实体），同一块区块里另
/// 放一块荧石让它一定是脏的。
fn arm_a_falling_sand(server: &mut Harness) -> (BlockPos, BlockPos) {
    // 玩家站在出生点，模拟距离内的区块才跑方块 tick。
    // A player at spawn: only chunks within simulation distance run block ticks.
    let client = server.join_and_settle("Kitten");
    for _ in 0..20 {
        server.tick();
    }
    client.take_frames();
    let sand = BlockPos::new(3, -55, 3);
    let marker = BlockPos::new(5, -60, 5);
    server.set_block(marker, blocks::GLOWSTONE);
    server.set_block(sand, blocks::SAND);
    server.world.islands.region.block_ticks.schedule(
        sand,
        mc_bamboo_data::block_id_of_state(blocks::SAND).expect("沙子是方块"),
        1,
        TickPriority::Normal,
    );
    (sand, marker)
}

/// 直接从盘上的区域文件读回区块 (0, 0) 里两个位置的方块，不起世界（起世界会接着跑
/// 存下的计划刻，看不出存的是哪一刻）。
///
/// Read two blocks of chunk (0, 0) straight from the region file without
/// starting a world (a world would run the saved scheduled tick and hide which
/// tick was saved).
fn saved_blocks(root: &std::path::Path, at: [BlockPos; 2]) -> [u32; 2] {
    use mc_bamboo_storage::{
        anvil::{RegionCompression, RegionPos, open_region},
        chunk_nbt,
        world_dir::WorldDir,
    };
    let chunk = ChunkPos::new(0, 0);
    let mut region = open_region(
        WorldDir::new(root).region_dir(),
        RegionPos::of(chunk),
        RegionCompression::Zlib,
    )
    .unwrap();
    let bytes = region
        .read(chunk, 64 << 20)
        .unwrap()
        .expect("区块 (0, 0) 存过");
    let decoded = chunk_nbt::decode_bytes(&bytes).unwrap();
    at.map(|pos| {
        decoded
            .column
            .chunk()
            .block((pos.x & 15) as usize, pos.y, (pos.z & 15) as usize)
    })
}

/// 场景本身对：不关服时沙子这一刻就掉下去。
/// The scenario itself: without a stop the sand falls this tick.
#[test]
fn the_armed_sand_falls_in_the_next_tick() {
    for drive in [IslandDrive::Lockstep, IslandDrive::Detached] {
        let (before, after) = with_timeout(move || {
            let mut server = Harness::new_loaded_with_drive(2, drive);
            let (sand, _) = arm_a_falling_sand(&mut server);
            let before = server.world_region().chunks.block(sand);
            server.tick();
            (before, server.world_region().chunks.block(sand))
        });
        assert_eq!(before, Some(blocks::SAND), "{drive:?}");
        assert_eq!(after, Some(blocks::AIR), "{drive:?}");
    }
}

/// `/stop`：关服在岛这一刻交回之后做，存下的是这一刻结束时的世界（沙子已经掉了）。
/// 两种驱动都一样。
///
/// `/stop`: shutdown happens after the island hands this tick back, so the
/// save is the world at the end of the tick (the sand has fallen). Same for
/// both drives.
#[test]
fn stop_saves_the_islands_last_tick() {
    for drive in [IslandDrive::Lockstep, IslandDrive::Detached] {
        let tmp = D6TempDir::new("stop-last-tick");
        let root = tmp.0.clone();
        let (sand, marker) = with_timeout(move || {
            let mut server = Harness::new_loaded_with_save_threads_and_drive(2, &root, 0, drive);
            let armed = arm_a_falling_sand(&mut server);
            // 和 `/stop` 命令一样：收包阶段置上，这一刻末尾关服。
            // Like the `/stop` command: set during the inbound phase, stop at the end of the tick.
            server.world.master.global.admin.stop_requested = true;
            assert!(server.world.tick().is_break());
            armed
        });
        let [at_sand, at_marker] = saved_blocks(&tmp.0, [sand, marker]);
        assert_eq!(at_marker, blocks::GLOWSTONE, "{drive:?}：区块存下来了");
        assert_eq!(at_sand, blocks::AIR, "{drive:?}：存的是沙子掉下去之后");
    }
}

/// 分离驱动的 `/save-all` 等岛这一刻交回、停放之后才过栅栏：快照里有这一刻岛上的改动。
/// 存完不走关服（直接丢掉世界，模拟崩溃），重开看到的就是 `/save-all` 存的那份。
///
/// A detached `/save-all` passes its barrier only after the island hands this
/// tick back and parks, so the snapshot has the tick's island changes. The
/// world is then dropped without shutdown (like a crash) and the reopened
/// world shows what `/save-all` wrote.
#[test]
fn detached_save_all_includes_the_islands_last_tick() {
    let tmp = D6TempDir::new("save-all-last-tick");
    let root = tmp.0.clone();
    let (sand, marker) = with_timeout(move || {
        let mut server =
            Harness::new_loaded_with_save_threads_and_drive(2, &root, 0, IslandDrive::Detached);
        let armed = arm_a_falling_sand(&mut server);
        server
            .world
            .master
            .global
            .save_requests
            .push(SaveCommand::SaveAll {
                flush: true,
                source: PlayerId(0),
            });
        server.tick();
        // 不关服：IO 线程关闭时写完已经交给它的快照，锁在会话丢掉时放开。
        // No shutdown: the IO thread writes what it was given as it closes,
        // and the lock goes with the session.
        drop(server);
        armed
    });
    let [at_sand, at_marker] = saved_blocks(&tmp.0, [sand, marker]);
    assert_eq!(at_marker, blocks::GLOWSTONE, "区块存下来了");
    assert_eq!(at_sand, blocks::AIR, "存的是沙子掉下去之后");
}

/// 交接的额外耗时：一个空世界（只有一名站着的玩家）每刻几乎不干活，分离和齐步每刻
/// 耗时之差就是交接本身（派出、唤醒、交回、记账）。A-B-A 交替各跑 `TICKS` 刻，报两边
/// 每刻的均值和剖析器记的 `island_handoff`。只用来测量：
/// `cargo test -p mc-bamboo-game --release --lib island_pool_tests::handoff_cost -- --ignored --nocapture`
///
/// The hand-over cost: an empty world (one standing player) does almost no
/// work per tick, so the per-tick difference between detached and lockstep is
/// the hand-over itself. Alternates A-B-A and prints both means and the
/// profiler's `island_handoff`. Measurement only.
#[test]
#[ignore]
fn handoff_cost() {
    const TICKS: usize = 4000;
    fn run(threads: usize) -> (f64, f64, f64) {
        let mut server = Harness::new_with_island_threads(2, false, threads);
        let client = server.join_and_settle("Kitten");
        for _ in 0..200 {
            server.world.tick_unpaced();
        }
        client.take_frames();
        server.world.islands.region.profiler.reset_samples();
        let started = Instant::now();
        for _ in 0..TICKS {
            server.world.tick_unpaced();
        }
        let per_tick = started.elapsed().as_secs_f64() * 1e6 / TICKS as f64;
        client.take_frames();
        let profiler = &server.world.islands.region.profiler;
        let handoff = profiler.histogram(Series::Metric(Metric::IslandHandoff));
        let mean = |h: &crate::profiler::Histogram| {
            if h.count() == 0 {
                0.0
            } else {
                h.sum() as f64 / h.count() as f64 / 1e3
            }
        };
        let p50 = handoff.p50() as f64 / 1e3;
        (per_tick, mean(handoff), p50)
    }
    with_timeout(|| {
        for round in 1..=5 {
            let a1 = run(0);
            let b = run(1);
            let a2 = run(0);
            let lockstep = (a1.0 + a2.0) / 2.0;
            println!(
                "round {round}: lockstep {:.1} / {:.1} us/tick, detached {:.1} us/tick, extra {:.1} us, island_handoff mean {:.1} p50 {:.1} us",
                a1.0,
                a2.0,
                b.0,
                b.0 - lockstep,
                b.1,
                b.2
            );
        }
    });
}

/// 只有一个岛时多岛模式（混合驱动）比多岛关着（分离驱动等唯一的岛）每刻多花多少：已加载
/// 模式、一名站着的玩家，两边都背靠背不睡地跑，差值就是混合驱动自己的记账（分区器、
/// 截止时间、落地和追赶的检查、按岛的交接记录）。A-B-A 交替。只用来测量：
/// `cargo test -p mc-bamboo-game --release --lib island_pool_tests::one_island_hybrid_cost -- --ignored --nocapture`
///
/// How much more a tick costs with multi-island mode on (the hybrid drive)
/// than off (the detached drive waiting for its one island) while there is
/// only one island: loaded mode, one standing player, both run back to back
/// without sleeping, so the difference is the hybrid drive's own
/// bookkeeping. Alternates A-B-A. Measurement only.
#[test]
#[ignore]
fn one_island_hybrid_cost() {
    const TICKS: usize = 4000;
    fn run(islands: bool) -> f64 {
        let mut server = if islands {
            Harness::new_islands(2, 4)
        } else {
            Harness::new_with_island_threads(2, true, 1)
        };
        let client = server.join_and_settle("Kitten");
        for _ in 0..200 {
            server.world.tick_unpaced();
        }
        assert_eq!(server.world.islands.len(), 1, "只有一个岛");
        client.take_frames();
        let started = Instant::now();
        for _ in 0..TICKS {
            server.world.tick_unpaced();
        }
        let per_tick = started.elapsed().as_secs_f64() * 1e6 / TICKS as f64;
        client.take_frames();
        per_tick
    }
    with_timeout(|| {
        for round in 1..=7 {
            let a1 = run(false);
            let b = run(true);
            let a2 = run(false);
            let off = (a1 + a2) / 2.0;
            println!(
                "round {round}: islands off {a1:.1} / {a2:.1} us/tick, on {b:.1} us/tick, extra {:.1} us ({:.3}x)",
                b - off,
                b / off
            );
        }
    });
}
