//! 判死的测试（岛的实施计划 5.6）。
//!
//! 卡死用测试专用的 `World::stall_island(岛, 1, u64::MAX)`（闸永远不开，主控不等它），
//! 「卡了多久」用 `World::age_island` 把线程池里的开跑时刻往前拨，不靠睡眠。只有最后的
//! 烟雾测试用真的卡死的线程、真的截止时间和很短的判死时限。
//!
//! Tests for declaring an island dead (island plan 5.6). A stuck island is
//! the test-only `World::stall_island(island, 1, u64::MAX)` (the gate never
//! opens and the master does not wait for it); "how long it has been stuck"
//! is `World::age_island`, which moves the start of its tick back in the
//! pool, never a sleep. Only the smoke test at the end uses a really stuck
//! thread, a real deadline and a very short timeout.

use std::time::{Duration, Instant};

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::packets::play::clientbound;
use mc_bamboo_world::director::{HOME_REGION, Owner, RegionId, TicketOp};

use super::{
    island_crossing_tests::{Act, act, first_of},
    island_death::DeathCause,
    island_hybrid_tests::{
        block_on_disk, bounded, clients_of, command, console, count_marker, island_of_group,
        prepare, scene, tick_within,
    },
    island_split_tests::{Scene, at, set_up},
    tests::{Frame, Harness},
};
use crate::island::IslandTestHook;

/// `/save-all` 成功的反馈。
const SAVED: &str = "commands.save.success";

/// 一句只在死岛的断开原因里出现的话。
const DEAD_TEXT: &str = "stopped responding";

/// 判死时限再多一秒：拨这么久，下一次判定就超时。
fn overdue(scene: &Scene) -> Duration {
    scene.server.world.master.island_dead_timeout + Duration::from_secs(1)
}

/// 每名玩家各自收到的帧，按 `scene.clients` 的顺序。
fn take_all(scene: &Scene, into: &mut [Vec<Frame>]) {
    for (index, (_, client)) in scene.clients.iter().enumerate() {
        into[index].extend(client.take_frames());
    }
}

/// 改一个方块并记成要存（和玩家改的一样会被存盘名单拍到）。
fn edit(scene: &mut Scene, pos: BlockPos, state: u32) {
    let chunk = ChunkPos::new(pos.x.div_euclid(16), pos.z.div_euclid(16));
    let world = &mut scene.server.world;
    let island = world.islands.island_of_chunk(chunk).expect("有岛");
    let region = &mut world.islands.get_mut(island).expect("停放着").region;
    region.chunks.set_block(pos, state);
    region.touch(pos);
    region.chunks.finish_light_updates();
}

/// 现在加载着 `pos` 的岛上那一格的方块。
fn loaded_block(scene: &Scene, pos: BlockPos) -> Option<u32> {
    let chunk = ChunkPos::new(pos.x.div_euclid(16), pos.z.div_euclid(16));
    let world = &scene.server.world;
    let island = world.islands.island_of_chunk(chunk)?;
    world.islands.get(island)?.region.chunks.block(pos)
}

/// 一个岛卡死被判死以后世界照常运转（岛的实施计划 5.6.2 的每一条），`come_back` 时
/// 那条线程之后又回来了，它交回的岛被整个丢掉。
///
/// 场景：三组各一个岛，带存档；第 1 组中心挂一张强加载票（判死后要重新加载）。第 1 组
/// 中心的一格先改成金块并存盘，再改成钻石块（存盘以后的改动，判死会丢）。第 1 组的岛
/// 卡死以后：一条控制台命令和一条玩家命令排着等全部岛；家岛和它的合并排着；第 0 组的
/// 聊天攒进它的出站队列。然后把它判死。
///
/// After an island gets stuck and is declared dead the world goes on (each
/// item of island plan 5.6.2); with `come_back` the thread returns later and
/// the island it hands back is discarded whole.
fn run_death(seed: u64, come_back: bool) {
    let root = std::env::temp_dir().join(format!(
        "bamboo-island-death-{}-{seed}-{come_back}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let spot;
    {
        let mut scene = set_up(seed, Harness::new_islands_with_save(2, &root, 4));
        let center = scene.centers[1];
        scene.server.apply_ticket_for_test(TicketOp::Forced {
            pos: center,
            forced: true,
        });
        for _ in 0..6 {
            scene.server.tick();
        }
        prepare(&mut scene);
        let victim = island_of_group(&mut scene, 1);
        assert_ne!(victim, HOME_REGION, "第 1 组不在家岛上");
        assert_eq!(island_of_group(&mut scene, 0), HOME_REGION);
        spot = at(center, 4, -60, 4);
        let (gold, diamond) = (
            mc_bamboo_data::blocks::GOLD_BLOCK,
            mc_bamboo_data::blocks::DIAMOND_BLOCK,
        );
        edit(&mut scene, spot, gold);
        command(&scene, 0, "save-all flush");
        for _ in 0..3 {
            scene.server.tick();
        }
        scene.server.world.master.host.flush_io();
        assert_eq!(block_on_disk(&root, spot), Some(gold), "金块存盘了");
        edit(&mut scene, spot, diamond);
        let victims = clients_of(&scene, 1);
        let victim_ids: Vec<_> = victims.iter().map(|client| client.id).collect();
        let mut frames: Vec<Vec<Frame>> = vec![Vec::new(); scene.clients.len()];
        take_all(&scene, &mut frames);
        frames.iter_mut().for_each(Vec::clear);

        scene.server.world.stall_island(victim, 1, u64::MAX);
        scene.server.tick();
        let world = &mut scene.server.world;
        assert!(!world.islands.is_parked(victim), "卡住了");
        let partition = world.master.host.partition_mut().expect("多岛开着");
        assert!(partition.force_merge(HOME_REGION, victim));
        world.master.host.take_partition_ops();
        world.settle_islands();
        assert!(world.master.host.has_topology(), "和卡死的岛的合并排着");
        console(&scene, "say MARK-HELD-CONSOLE");
        command(&scene, 2, "tellraw @a {\"text\":\"MARK-HELD-CMD\"}");
        act(&mut scene, &Act::Chat(0, "MARK-FAN-OUT"));
        command(&scene, 0, "save-all");
        for _ in 0..3 {
            scene.server.tick();
        }
        take_all(&scene, &mut frames);
        let world = &scene.server.world;
        assert!(world.master.hybrid.held_len() >= 2, "要全部岛的事排着");
        assert!(
            world
                .master
                .hybrid
                .fan_out
                .get(&victim)
                .is_some_and(|queue| !queue.is_empty()),
            "给它的出站投递攒着"
        );
        for got in &frames {
            assert_eq!(count_marker(got, "MARK-HELD-CONSOLE"), 0);
            assert_eq!(count_marker(got, "MARK-HELD-CMD"), 0);
            assert_eq!(count_marker(got, SAVED), 0, "`/save-all` 等着全部岛");
        }
        let threads_before = world.master.pool.as_ref().expect("分离驱动").threads();

        // 判死。
        // The verdict.
        let by = overdue(&scene);
        assert!(scene.server.world.age_island(victim, by), "它在线程上跑着");
        scene.server.tick();
        scene.server.tick();
        take_all(&scene, &mut frames);
        let world = &mut scene.server.world;
        let death = world
            .master
            .hybrid
            .death
            .deaths
            .last()
            .expect("判死了")
            .clone();
        assert_eq!(world.master.hybrid.death.deaths.len(), 1);
        assert_eq!(death.island, victim);
        assert_eq!(death.cause, DeathCause::Timeout);
        assert!(death.running_for.is_some_and(|time| time >= by));
        assert!(death.ticks_owed >= 3, "记下了欠的刻");
        let mut expected_players = victim_ids.clone();
        expected_players.sort_unstable();
        let mut players = death.players.clone();
        players.sort_unstable();
        assert_eq!(players, expected_players, "记下了它上面的玩家");
        assert!(death.chunks.contains(&center), "记下了它拿着的区块");
        // 线程池少了一条、补了一条。
        // The pool lost a thread and started a replacement.
        let pool = world.master.pool.as_ref().expect("分离驱动");
        assert_eq!(pool.threads(), threads_before + 1, "补了一条线程");
        assert_eq!(pool.live_threads(), threads_before, "活着的线程数不变");
        // 等全部岛的事做了，被它挡住的合并做了，它的出站队列丢了。
        // Work waiting for every island ran, the merge it blocked happened,
        // and its outbound queue was dropped.
        assert_eq!(world.master.hybrid.held_len(), 0, "排着的事都做了");
        assert!(!world.master.host.has_topology(), "被它挡住的合并做了");
        assert!(!world.master.hybrid.fan_out.contains_key(&victim));
        assert!(world.master.hybrid.death.fan_out_bytes.is_empty());
        assert!(world.islands.all_parked(), "剩下的岛都停放着");
        // 它的玩家从主控的全部名单里拿掉了。
        // Its players are gone from every list the master keeps.
        for id in &victim_ids {
            assert!(world.islands.island_of_player(*id).is_none());
            assert!(world.master.global.roster_name(*id).is_none(), "名单里没了");
            assert!(!world.master.ticket_centers.contains_key(id), "票撤了");
            assert!(!world.master.transit.contains_key(id));
            assert!(!world.master.hybrid.death.exits.contains_key(id));
        }
        for (index, (group, client)) in scene.clients.iter().enumerate() {
            let got = &frames[index];
            if *group == 1 {
                assert!(client.is_closed(), "死岛上的玩家被断开了");
                let disconnects = super::tests::frames_with_id(got, clientbound::DISCONNECT);
                assert_eq!(disconnects.len(), 1, "收到一个断开包");
                assert_eq!(count_marker(got, DEAD_TEXT), 1, "断开原因说清楚了");
                continue;
            }
            assert!(!client.is_closed());
            assert_eq!(
                count_marker(got, "MARK-HELD-CONSOLE"),
                1,
                "控制台命令做了一次"
            );
            assert_eq!(count_marker(got, "MARK-HELD-CMD"), 1, "玩家命令做了一次");
            if client.id == first_of(&scene, 0).id {
                assert_eq!(count_marker(got, SAVED), 1, "`/save-all` 做了");
            }
            assert_eq!(
                count_marker(got, "multiplayer.player.left"),
                victims.len(),
                "别的玩家看到他们离开"
            );
            assert!(
                !super::tests::frames_with_id(got, clientbound::PLAYER_INFO_REMOVE).is_empty(),
                "Tab 名单移除了他们"
            );
        }
        // 它的区块从上次存盘重新加载（强加载票还要它）：金块在，钻石块丢了。
        // Its chunks reload from the last save (the forced ticket still wants
        // them): the gold block is back, the diamond is lost.
        for _ in 0..40 {
            if loaded_block(&scene, spot).is_some() {
                break;
            }
            scene.server.tick();
        }
        assert_eq!(
            loaded_block(&scene, spot),
            Some(gold),
            "重新加载的是上次存盘"
        );
        let world = &mut scene.server.world;
        assert!(matches!(
            world.master.host.director().owner_at(center),
            Some(Owner::Region(_))
        ));
        world.assert_island_invariants();

        if come_back {
            // 那条线程回来了：交回的岛对不上任何槽位，整个丢掉。
            // The thread comes back: its island matches no slot and is
            // discarded whole.
            scene.server.world.release_stalls_for_test();
            let waited = Instant::now();
            while scene.server.world.master.hybrid.death.zombies == 0 {
                assert!(waited.elapsed() < Duration::from_secs(60), "死岛一直没回来");
                std::thread::sleep(Duration::from_millis(5));
                scene.server.tick();
            }
            for _ in 0..3 {
                scene.server.tick();
            }
            let world = &mut scene.server.world;
            assert_eq!(
                world.master.hybrid.death.deaths.len(),
                1,
                "回来不算第二次死"
            );
            assert!(!world.islands.contains(victim) || world.islands.is_parked(victim));
            for id in &victim_ids {
                assert!(
                    world.islands.island_of_player(*id).is_none(),
                    "它的玩家没被带回来"
                );
            }
            assert_eq!(
                loaded_block(&scene, spot),
                Some(gold),
                "它的钻石块没被带回来"
            );
            let world = &mut scene.server.world;
            world.assert_island_invariants();
            let pool = world.master.pool.as_ref().expect("分离驱动");
            assert_eq!(
                pool.live_threads(),
                threads_before,
                "回来的线程退出了，不再接活"
            );
        }
        // 关服照常（存盘名单里没有死岛的东西）。
        // Shutdown works as usual (nothing of the dead island on the save lists).
        scene.server.world.master.island_stop_timeout = Duration::from_millis(300);
        scene.server.shut_down();
        scene.server.world.release_stalls_for_test();
    }
    assert_eq!(
        block_on_disk(&root, spot),
        Some(mc_bamboo_data::blocks::GOLD_BLOCK),
        "盘上留着上次存盘的金块，死岛的钻石块没写进去"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_stuck_island_is_declared_dead_and_the_world_goes_on() {
    bounded(|| run_death(301, false));
}

#[test]
fn a_dead_island_that_comes_back_is_discarded() {
    bounded(|| run_death(302, true));
}

/// 家岛卡死被判死：槽位里当场换上一个空的家岛（时钟是它没卡住的话现在该有的，和按时的
/// 岛相同），控制台命令照常，控制台 `/stop` 能关服。
///
/// The home island is declared dead: an empty home takes its slot at once
/// (with the clock it would have had, equal to the islands on time), console
/// commands work, and a console `/stop` shuts down.
#[test]
fn a_dead_home_island_is_replaced_by_an_empty_one() {
    bounded(|| {
        let mut scene = scene(311, 4);
        for _ in 0..3 {
            scene.server.tick();
        }
        prepare(&mut scene);
        assert_eq!(island_of_group(&mut scene, 0), HOME_REGION);
        let other = island_of_group(&mut scene, 1);
        scene.server.world.stall_island(HOME_REGION, 1, u64::MAX);
        scene.server.tick();
        assert!(!scene.server.world.islands.is_parked(HOME_REGION));
        console(&scene, "say MARK-HOME-HELD");
        for _ in 0..3 {
            scene.server.tick();
        }
        let mut frames: Vec<Vec<Frame>> = vec![Vec::new(); scene.clients.len()];
        take_all(&scene, &mut frames);
        assert!(
            frames
                .iter()
                .all(|got| count_marker(got, "MARK-HOME-HELD") == 0)
        );
        let by = overdue(&scene);
        assert!(scene.server.world.age_island(HOME_REGION, by));
        scene.server.tick();
        take_all(&scene, &mut frames);
        let world = &scene.server.world;
        let death = world.master.hybrid.death.deaths.last().expect("判死了");
        assert_eq!(death.island, HOME_REGION);
        assert!(world.islands.is_parked(HOME_REGION), "家岛一直在");
        let home = world.islands.get(HOME_REGION).expect("停放着");
        assert!(home.region.players.is_empty(), "换上的家岛是空的");
        assert_eq!(
            home.region.tick.game_time(),
            world
                .islands
                .get(other)
                .expect("停放着")
                .region
                .tick
                .game_time(),
            "换上的家岛的时钟和按时的岛相同"
        );
        for (index, (group, client)) in scene.clients.iter().enumerate() {
            if *group == 0 {
                assert!(client.is_closed());
                assert_eq!(count_marker(&frames[index], DEAD_TEXT), 1);
            } else {
                assert_eq!(count_marker(&frames[index], "MARK-HOME-HELD"), 1);
            }
        }
        scene.server.world.assert_island_invariants();
        console(&scene, "stop");
        let mut stopped = false;
        for _ in 0..3 {
            scene.server.world.master.host.flush_io();
            if scene.server.world.tick().is_break() {
                stopped = true;
                break;
            }
        }
        assert!(stopped, "控制台的 /stop 关了服");
        scene.server.world.release_stalls_for_test();
    });
}

/// 出站队列满了提前判死（岛的实施计划 5.6.5），不丢包：上限以内一直活着，过了上限
/// 下一个判定点判死，原因记成出站队列满。条数和字节数两种上限各跑一遍。聊天每 10 刻
/// 一条（再快会被当成刷屏踢掉）。
///
/// A full outbound queue declares the island dead early (island plan 5.6.5)
/// instead of dropping packets: alive within the limit, dead at the next
/// check past it, with the full queue as the cause. Once with an item limit
/// and once with a byte limit. One chat every 10 ticks (faster is kicked as spam).
fn run_overflow(seed: u64, items: usize, bytes: usize) {
    let mut scene = scene(seed, 4);
    for _ in 0..3 {
        scene.server.tick();
    }
    prepare(&mut scene);
    let victim = island_of_group(&mut scene, 1);
    scene.server.world.master.hybrid.death.fan_out_limit = Some((items, bytes));
    scene.server.world.stall_island(victim, 1, u64::MAX);
    scene.server.tick();
    let sender = first_of(&scene, 0);
    let mut chats = 0;
    let mut tick = 0;
    while scene.server.world.master.hybrid.death.deaths.is_empty() {
        let world = &scene.server.world;
        let queued = world.master.hybrid.fan_out.get(&victim).map_or(0, Vec::len);
        let size = world
            .master
            .hybrid
            .death
            .fan_out_bytes
            .get(&victim)
            .copied()
            .unwrap_or(0);
        assert!(
            queued <= items && size <= bytes,
            "队列过了上限还活着：{queued} 条 {size} 字节"
        );
        assert!(chats < 40, "一直没判死（出站队列 {queued} 条 {size} 字节）");
        if tick % 10 == 0 {
            chats += 1;
            scene.server.request(
                &sender,
                mc_bamboo_protocol::packet::play::PlayRequest::unsigned_chat(format!(
                    "MARK-FLOOD-{chats}"
                )),
            );
        }
        tick += 1;
        scene.server.tick();
    }
    assert!(chats > 1, "上限以内没有判死");
    assert!(
        !first_of(&scene, 0).is_closed(),
        "发聊天的人没被当成刷屏踢掉"
    );
    let world = &mut scene.server.world;
    let death = world.master.hybrid.death.deaths.last().expect("判死了");
    assert_eq!(death.island, victim);
    assert_eq!(death.cause, DeathCause::OutboundFull);
    assert!(!world.master.hybrid.fan_out.contains_key(&victim));
    for client in clients_of(&scene, 1) {
        assert!(client.is_closed());
    }
    scene.server.tick();
    scene.server.world.assert_island_invariants();
    scene.server.world.release_stalls_for_test();
}

#[test]
fn an_overflowing_outbound_queue_declares_the_island_dead_early() {
    bounded(|| {
        run_overflow(321, 4, usize::MAX);
        run_overflow(322, usize::MAX, 600);
    });
}

/// 关服时还没回来的岛按同一套判死（岛的实施计划 5.6.6）：最多等 `island_stop_timeout`，
/// 原因记成关服，它的玩家收到的是关服的断开原因，世界循环照常结束。
///
/// Islands still away at shutdown are declared dead the same way (island
/// plan 5.6.6): after at most `island_stop_timeout`, with shutdown as the
/// cause and the shutdown reason for its players, and the loop ends as usual.
#[test]
fn islands_still_away_at_shutdown_are_declared_dead() {
    bounded(|| {
        let mut scene = scene(331, 4);
        for _ in 0..3 {
            scene.server.tick();
        }
        let victim = island_of_group(&mut scene, 2);
        scene.server.world.master.island_stop_timeout = Duration::from_millis(300);
        scene.server.world.stall_island(victim, 1, u64::MAX);
        scene.server.tick();
        for (_, client) in &scene.clients {
            let _ = client.take_frames();
        }
        scene
            .server
            .inbox_sender()
            .send(crate::inbound::Inbound::Shutdown)
            .expect("世界还在");
        let started = Instant::now();
        scene.server.world.master.host.flush_io();
        assert!(scene.server.world.tick().is_break(), "关服结束世界循环");
        assert!(started.elapsed() < Duration::from_secs(20));
        let world = &scene.server.world;
        let death = world.master.hybrid.death.deaths.last().expect("判死了");
        assert_eq!((death.island, death.cause), (victim, DeathCause::Shutdown));
        assert!(world.islands.all_parked(), "剩下的都停放着");
        for client in clients_of(&scene, 2) {
            assert!(client.is_closed());
            let got = client.take_frames();
            assert_eq!(count_marker(&got, DEAD_TEXT), 0);
            assert_eq!(
                count_marker(&got, super::SERVER_SHUTDOWN),
                1,
                "关服的断开原因"
            );
        }
        scene.server.world.release_stalls_for_test();
    });
}

/// 线程比岛少时，排在卡死的岛后面等线程的岛不算卡死（岛的实施计划 5.6.1）：只判开跑
/// 了超过时限的那一个；它的线程被放弃、补上新线程以后，排着的岛照常跑完。
///
/// With fewer threads than islands, islands queued behind a stuck one are
/// not stuck (island plan 5.6.1): only the one running past the limit dies;
/// once its thread is abandoned and replaced, the queued islands run.
#[test]
fn islands_queued_behind_a_stuck_one_are_not_declared_dead() {
    bounded(|| {
        let mut scene = scene(341, 1);
        for _ in 0..3 {
            scene.server.tick();
        }
        prepare(&mut scene);
        let victim = island_of_group(&mut scene, 1);
        let clock = |scene: &Scene, id: RegionId| {
            scene
                .server
                .world
                .islands
                .get(id)
                .map(|island| island.region.tick.game_time())
        };
        let before = clock(&scene, HOME_REGION).expect("停放着");
        scene.server.world.stall_island(victim, 1, u64::MAX);
        // 家岛先派出（编号小），它要是排在卡死的岛后面就回不来；用截止时间跑。
        // Run with a deadline: anything queued behind the stuck island stays out.
        for _ in 0..3 {
            let _ = tick_within(&mut scene.server, Duration::from_millis(40));
        }
        let by = overdue(&scene);
        assert!(scene.server.world.age_island(victim, by));
        let mut deaths = 0;
        for _ in 0..40 {
            let _ = tick_within(&mut scene.server, Duration::from_millis(200));
            deaths = scene.server.world.master.hybrid.death.deaths.len();
            if deaths > 0 && scene.server.world.islands.all_parked() {
                break;
            }
        }
        assert_eq!(deaths, 1, "只判死了卡住的那一个");
        let world = &scene.server.world;
        assert_eq!(world.master.hybrid.death.deaths[0].island, victim);
        assert!(world.islands.all_parked(), "排着的岛跑完回来了");
        for _ in 0..3 {
            scene.server.tick();
        }
        assert!(
            clock(&scene, HOME_REGION).expect("停放着") > before + 3,
            "家岛照常往前走"
        );
        scene.server.world.assert_island_invariants();
        scene.server.world.release_stalls_for_test();
    });
}

/// 烟雾测试：真的卡死的线程（测试钩子直接卡在闸上，主控不知道它是故意的）、真的截止
/// 时间和很短的判死时限。主控在判死之前照常跑、不等它；到时限判死；排着的控制台命令
/// 做了；之后开闸，死岛回来被丢掉。
///
/// Smoke test: a really stuck thread (the hook waits on a gate the master
/// knows nothing about), a real deadline and a very short timeout. The
/// master keeps going without it until the verdict, the held console
/// command then runs, and once the gate opens the dead island comes back and
/// is discarded.
#[test]
fn a_really_stuck_thread_is_declared_dead() {
    bounded(|| {
        let mut scene = scene(351, 2);
        for _ in 0..3 {
            scene.server.tick();
        }
        prepare(&mut scene);
        let victim = island_of_group(&mut scene, 2);
        let gate = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let world = &mut scene.server.world;
        world.master.island_dead_timeout = Duration::from_millis(300);
        world.islands.get_mut(victim).expect("停放着").test_hook =
            Some(IslandTestHook::Stall(std::sync::Arc::clone(&gate)));
        let _ = tick_within(&mut scene.server, Duration::from_millis(20));
        console(&scene, "say MARK-SMOKE");
        let started = Instant::now();
        while scene.server.world.master.hybrid.death.deaths.is_empty() {
            assert!(started.elapsed() < Duration::from_secs(30), "一直没判死");
            let flow = tick_within(&mut scene.server, Duration::from_millis(20));
            assert!(flow.is_continue());
        }
        assert!(
            started.elapsed() >= Duration::from_millis(250),
            "没到时限就判死了"
        );
        for _ in 0..3 {
            scene.server.tick();
        }
        let mut frames: Vec<Vec<Frame>> = vec![Vec::new(); scene.clients.len()];
        take_all(&scene, &mut frames);
        for (index, (group, client)) in scene.clients.iter().enumerate() {
            if *group == 2 {
                assert!(client.is_closed());
            } else {
                assert_eq!(count_marker(&frames[index], "MARK-SMOKE"), 1);
            }
        }
        gate.store(true, std::sync::atomic::Ordering::Release);
        let waited = Instant::now();
        while scene.server.world.master.hybrid.death.zombies == 0 {
            assert!(waited.elapsed() < Duration::from_secs(30), "死岛没回来");
            std::thread::sleep(Duration::from_millis(5));
            scene.server.tick();
        }
        scene.server.world.assert_island_invariants();
    });
}
