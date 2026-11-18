//! 混合驱动的测试（岛的实施计划 5.5，第 10 步第三段）。
//!
//! 场景沿用第 9 步的三组内容（三个岛）。直接调 `World::tick` 时不给截止时间、等全部岛
//! 回来，这就是确定性的「都按时」；过载用测试专用的 `World::stall_island`（某岛第 N 到
//! M 刻不回来，主控不等它、过后开闸等它回来），不靠睡眠和运气。只有最后的烟雾测试用
//! 真的截止时间和真的慢岛。
//!
//! Tests for the hybrid drive (island plan 5.5, step 10 part three). The
//! scene is step 9's three groups (three islands). A direct `World::tick`
//! has no deadline and waits for every island, which is the deterministic
//! "everyone on time"; overload uses the test-only `World::stall_island`
//! (an island stays away for ticks N to M, the master does not wait and
//! opens the gate afterwards), never sleeps or luck. Only the smoke tests at
//! the end use a real deadline and really slow islands.

use std::{
    collections::BTreeMap,
    ops::ControlFlow,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use mc_bamboo_core::{ChunkPos, Vec3};
use mc_bamboo_data::packets::play::clientbound;
use mc_bamboo_protocol::packet::play::PlayRequest;
use mc_bamboo_world::director::{HOME_REGION, RegionId, TicketOp};

use super::{
    island_crossing_tests::{Act, act, first_of, make_op},
    island_hybrid::Breaks,
    island_runtime_tests::align,
    island_split_tests::{
        Scene, fingerprint, furnish_scene_with_sand, region_of_player, rng, set_up, with_stack,
    },
    tests::{Client, Frame, Harness},
};
use crate::{inbound::Inbound, island::IslandTestHook};

/// 每条测试的时间上限。并发的东西出了问题通常表现为卡住，而不是断言失败。
///
/// The time limit for each test: a concurrency bug usually shows as a hang
/// rather than a failed assertion.
pub(super) const LIMIT: Duration = Duration::from_secs(240);

/// 在一条栈够大的线程上跑测试，超过 [`LIMIT`] 就失败。
pub(super) fn bounded(body: impl FnOnce() + Send + 'static) {
    let (done, finished) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        with_stack(body);
        let _ = done.send(());
    });
    match finished.recv_timeout(LIMIT) {
        Ok(()) => worker.join().expect("测试线程"),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            if let Err(payload) = worker.join() {
                std::panic::resume_unwind(payload);
            }
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("测试在 {LIMIT:?} 里没有跑完（卡住了）")
        }
    }
}

/// 一个多岛世界：三组各自一个岛，每组第一名玩家是 OP，聊天时钟钉死，机器摆好（悬空沙子
/// 推到窗口之后，窗口里不生成新实体）。`threads = 0` 是齐步，≥ 1 是混合驱动。
///
/// A multi-island world: one island per group, each group's first player an
/// op, the chat clock pinned and the machines placed (the hanging sand is
/// pushed past the window, so no entity spawns in it). `threads = 0` is
/// lockstep, 1 or more the hybrid drive.
pub(super) fn scene(seed: u64, threads: usize) -> Scene {
    let scene = set_up(seed, Harness::new_islands(2, threads));
    assert_eq!(
        scene.server.world.islands.len(),
        scene.centers.len(),
        "每组一个岛"
    );
    scene
}

/// 两个世界里同一名玩家都收完区块：两边每次各跑一刻，直到两边都没有新批次、邮筒也空。
fn settle_pair(a: &mut Harness, b: &mut Harness, in_a: &Client, in_b: &Client) {
    loop {
        let mut busy = false;
        for (server, client) in [(&mut *a, in_a), (&mut *b, in_b)] {
            server.tick();
            let frames = client.take_frames();
            let finished =
                super::tests::frames_with_id(&frames, clientbound::CHUNK_BATCH_FINISHED).len();
            for _ in 0..finished {
                server.acknowledge_batch(client, 64.0);
            }
            busy |= finished != 0 || server.world.master.host.posted_len() != 0;
        }
        if !busy {
            return;
        }
    }
}

/// 同 `set_up`，但两个世界一步一步一起搭：每名玩家在两边的同一刻进服，等区块时两边跑
/// 同样多的刻。两个世界的驱动不同（分离驱动的交付晚一刻）时，单独搭出来的两份历史长短
/// 不一，玩家身上按「进服以来第几刻」走的节拍（比如每两刻同步一次位置）就会错开；
/// 这样搭出来的两份只差交付的时刻。
///
/// Like `set_up`, but building two worlds step by step together: each player
/// joins both at the same tick and both run the same number of ticks while
/// chunks arrive. With different drives (detached deliveries are a tick
/// later) two separately built histories differ in length, and per-player
/// rhythms counted from the join (such as the two-tick position sync) fall
/// out of step; built this way the worlds differ only in delivery timing.
fn set_up_pair(seed: u64, mut a: Harness, mut b: Harness) -> (Scene, Scene) {
    for server in [&mut a, &mut b] {
        server
            .world
            .master
            .global
            .rules_mut()
            .assign(mc_bamboo_data::gamerules::RANDOM_TICK_SPEED, "0");
        server.world.publish_global();
    }
    let centers = super::island_split_tests::centers_for(seed);
    let mut next = rng(seed ^ 0x5555);
    let (mut clients_a, mut clients_b) = (Vec::new(), Vec::new());
    for (group, &center) in centers.iter().enumerate() {
        let count = 1 + (next() % 2) as usize;
        for index in 0..count {
            let name = format!("g{group}p{index}");
            let (in_a, in_b) = (a.join(&name), b.join(&name));
            for (server, client) in [(&a, &in_a), (&b, &in_b)] {
                server.confirm_teleport(client);
                server.load_player(client);
            }
            settle_pair(&mut a, &mut b, &in_a, &in_b);
            let x = f64::from(center.x * 16) + 8.5 + index as f64 * 3.0;
            let z = f64::from(center.z * 16) + 8.5;
            if (group, index) != (0, 0) {
                a.move_to(&in_a, x, z);
                b.move_to(&in_b, x, z);
            }
            clients_a.push((group, in_a));
            clients_b.push((group, in_b));
        }
    }
    for ((_, in_a), (_, in_b)) in clients_a.iter().zip(&clients_b) {
        settle_pair(&mut a, &mut b, in_a, in_b);
    }
    let scene = |server, clients| Scene {
        server,
        centers: centers.clone(),
        clients,
        hashes: super::island_split_tests::ChunkHashes::new(),
    };
    (scene(a, clients_a), scene(b, clients_b))
}

/// 两个世界对齐以后做的准备（见 [`scene`]）。
pub(super) fn prepare(scene: &mut Scene) {
    for group in 0..scene.centers.len() {
        let client = first_of(scene, group);
        make_op(scene, &client);
    }
    for island in scene.server.world.islands.iter_mut() {
        island.region.clock_millis = Some(1_700_000_000_000);
    }
    // 已经确认过的传送留下的 `sent_at` 是历史（两个世界搭场景用的刻数不同），不再被读；
    // 对齐成现在，免得指纹里带着它。
    // The `sent_at` of an already confirmed teleport is history (the worlds
    // took different tick counts to set up) and is never read again; align it
    // to now so the prints do not carry it.
    for island in scene.server.world.islands.iter_mut() {
        let now = island.region.tick;
        for player in island.region.players.values_mut() {
            assert!(
                player.teleport.awaiting_position.is_none(),
                "传送都确认过了"
            );
            player.teleport.sent_at = now;
        }
    }
    furnish_scene_with_sand(scene, 10_000);
    for (_, client) in &scene.clients {
        let _ = client.take_frames();
    }
}

/// 拿着第 `group` 组的岛（按分区器）。
pub(super) fn island_of_group(scene: &mut Scene, group: usize) -> RegionId {
    let center = scene.centers[group];
    scene
        .server
        .world
        .master
        .host
        .island_of(center)
        .expect("每组的中心都归某个岛")
}

/// 第 `group` 组的内容指纹和它的岛的时钟；那个岛不在主控手里时是 `None`。
///
/// The fingerprint of group `group` and its island's clock, or `None` while
/// that island is away.
pub(super) fn group_state(scene: &mut Scene, group: usize) -> Option<(String, i64)> {
    let id = island_of_group(scene, group);
    let Scene {
        server,
        centers,
        hashes,
        ..
    } = scene;
    if !server.world.islands.is_parked(id) {
        return None;
    }
    let island = server.world.islands.get(id).expect("停放着");
    let print = fingerprint(&island.region, centers, false, hashes);
    Some((print[group].clone(), island.region.tick.game_time()))
}

/// 岛 `id`（停放着）上第 `group` 组的内容指纹。
fn group_print_on(scene: &mut Scene, id: RegionId, group: usize) -> String {
    let Scene {
        server,
        centers,
        hashes,
        ..
    } = scene;
    let island = server.world.islands.get(id).expect("停放着");
    fingerprint(&island.region, centers, false, hashes)[group].clone()
}

/// 两份（内容，时钟）相同；不同时只报第一处不同的那一行，不把整份指纹打出来。
fn assert_same_state(left: Option<(String, i64)>, right: Option<(String, i64)>, what: &str) {
    if left == right {
        return;
    }
    let (Some((left, left_clock)), Some((right, right_clock))) = (left, right) else {
        panic!("{what}：一边的岛不在主控手里");
    };
    assert_eq!(left_clock, right_clock, "{what}：时钟");
    assert_same_print(&left, &right, what);
}

/// 两份内容指纹相同；不同时只报第一处不同的那一行。
fn assert_same_print(left: &str, right: &str, what: &str) {
    if left == right {
        return;
    }
    let mut theirs = right.lines();
    for (index, mine) in left.lines().enumerate() {
        let other = theirs.next();
        if Some(mine) != other {
            panic!("{what}：第 {index} 行不同\n  左：{mine}\n  右：{other:?}");
        }
    }
    panic!(
        "{what}：行数不同（左 {}，右 {}）",
        left.lines().count(),
        right.lines().count()
    );
}

/// 把一行拆成文字和整数（带符号）交替的序列。
fn tokens(line: &str) -> Vec<Result<i128, String>> {
    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<Result<i128, String>> = Vec::new();
    let mut text = String::new();
    let mut index = 0;
    while index < chars.len() {
        let here = chars[index];
        let signed = here == '-'
            && chars.get(index + 1).is_some_and(char::is_ascii_digit)
            && !index
                .checked_sub(1)
                .is_some_and(|before| chars[before].is_alphanumeric() || chars[before] == '_');
        if here.is_ascii_digit() || signed {
            let start = index;
            index += 1;
            while index < chars.len() && chars[index].is_ascii_digit() {
                index += 1;
            }
            let number: String = chars[start..index].iter().collect();
            match number.parse::<i128>() {
                Ok(value) => {
                    out.push(Err(std::mem::take(&mut text)));
                    out.push(Ok(value));
                }
                Err(_) => text.push_str(&number),
            }
        } else {
            text.push(here);
            index += 1;
        }
    }
    out.push(Err(text));
    out
}

/// 两份内容指纹除了绝对时刻以外相同：逐行逐项比，文字必须相同，整数要么相同、要么右边
/// 正好比左边大 `delta`（合并时被吸收方的绝对时刻整体平移了两岛的时钟差）。少一行、
/// 多一行、任何别的数不同都报出来。
///
/// Two prints are equal apart from absolute instants: line by line and
/// token by token, text must match and every integer is either equal or
/// exactly `delta` larger on the right (a merge rebases the absorbed side's
/// instants by the clock difference). A missing or extra line or any other
/// number fails.
fn assert_same_print_shifted(left: &str, right: &str, delta: i128, what: &str) {
    let (mine, theirs): (Vec<&str>, Vec<&str>) = (left.lines().collect(), right.lines().collect());
    assert_eq!(mine.len(), theirs.len(), "{what}：行数");
    let mut shifted = 0;
    for (index, (mine, theirs)) in mine.iter().zip(&theirs).enumerate() {
        let (a, b) = (tokens(mine), tokens(theirs));
        let same = a.len() == b.len()
            && a.iter().zip(&b).all(|pair| match pair {
                (Ok(x), Ok(y)) => {
                    if x != y && y - x == delta {
                        shifted += 1;
                    }
                    x == y || y - x == delta
                }
                (Err(x), Err(y)) => x == y,
                _ => false,
            });
        assert!(
            same,
            "{what}：第 {index} 行不同\n  左：{mine}\n  右：{theirs}"
        );
    }
    assert!(shifted > 0 || delta == 0, "{what}：有绝对时刻被平移了");
}

/// 第 `group` 组的第一名玩家发一条命令（不带斜杠）。和 `act` 不同，不碰任何岛（有岛
/// 不在主控手里时也能用）。
pub(super) fn command(scene: &Scene, group: usize, line: &str) {
    let client = first_of(scene, group);
    scene
        .server
        .request(&client, PlayRequest::unsigned_command(line));
}

/// 控制台命令。
pub(super) fn console(scene: &Scene, line: &str) {
    scene
        .server
        .inbox_sender()
        .send(Inbound::ConsoleCommand(line.to_owned()))
        .expect("世界还在");
}

/// 第 `group` 组的玩家。
pub(super) fn clients_of(scene: &Scene, group: usize) -> Vec<Client> {
    scene
        .clients
        .iter()
        .filter(|(index, _)| *index == group)
        .map(|(_, client)| client.clone())
        .collect()
}

/// 帧里出现了几次 `marker`（聊天和系统消息的正文原样在包体里）。
pub(super) fn count_marker(frames: &[Frame], marker: &str) -> usize {
    let needle = marker.as_bytes();
    frames
        .iter()
        .map(|frame| {
            frame
                .body
                .windows(needle.len())
                .filter(|window| *window == needle)
                .count()
        })
        .sum()
}

/// `markers` 在帧里出现的先后（每个标记第一次出现的帧号）；没出现的不在结果里。
fn marker_order(frames: &[Frame], markers: &[&str]) -> Vec<String> {
    let mut seen: Vec<(usize, String)> = markers
        .iter()
        .filter_map(|marker| {
            frames
                .iter()
                .position(|frame| count_marker(std::slice::from_ref(frame), marker) > 0)
                .map(|index| (index, (*marker).to_owned()))
        })
        .collect();
    seen.sort();
    seen.into_iter().map(|(_, marker)| marker).collect()
}

/// 带截止时间跑一刻（混合驱动在 `World::run` 里的样子）。
///
/// Run one tick with a deadline, as the hybrid drive does inside `World::run`.
pub(super) fn tick_within(server: &mut Harness, window: Duration) -> ControlFlow<()> {
    server.world.master.host.flush_io();
    server.world.master.hybrid.deadline = Some(Instant::now() + window);
    let flow = server.world.tick();
    server.world.master.hybrid.deadline = None;
    flow
}

/// 所有岛都按时回来时，混合驱动（4 条 tick 线程）和齐步逐刻相同：每组的内容、每个岛的
/// 时钟、每名玩家收到的全部字节。窗口里有跨岛的命令、聊天、控制台命令和按名随机序列
/// （不换岛、不进出服：那些会挂票，分离驱动的交付本来就比齐步晚一刻）。
///
/// With every island on time the hybrid drive (4 tick threads) matches
/// lockstep tick by tick: every group's content, every island clock and
/// every player's bytes. The window has cross-island commands, chat, console
/// commands and a named random sequence (no island changes, joins or leaves:
/// those place tickets, and detached deliveries are a tick later than
/// lockstep by design).
#[test]
fn hybrid_with_every_island_on_time_matches_lockstep() {
    bounded(|| {
        for seed in [41_u64, 42] {
            run_on_time(seed, false);
        }
    });
}

/// 改坏确认：混合驱动那份借玩家时一个都不借（`@a` 只到得了发起人自己岛上的人），上面的
/// 对照要报出别的岛上的玩家的状态或者收到的字节不同。
///
/// Break-it check: if the hybrid world borrows nobody (`@a` only reaches the
/// sender's own island), the comparison above reports the other islands'
/// players' state or bytes.
#[test]
fn the_on_time_comparison_catches_a_broken_borrow() {
    bounded(|| {
        let caught = std::panic::catch_unwind(|| run_on_time(41, true));
        let text = panic_text(caught.expect_err("不借玩家没有被抓到"));
        // 借不到别的岛上的人：他们的物品栏（内容指纹里的玩家那一行）或者收到的包就不同。
        // Players of other islands are not reached: their inventory (the
        // player line of the print) or their packets differ.
        assert!(
            text.contains("组的内容和时钟") || text.contains("收到的字节不同"),
            "失败的原因不对：{text}"
        );
    });
}

/// [`hybrid_with_every_island_on_time_matches_lockstep`] 的一次运行；`lend_nothing` 为真时
/// 混合驱动那份借玩家时一个都不借。
fn run_on_time(seed: u64, lend_nothing: bool) {
    {
        {
            let script: Vec<(u64, Act)> = vec![
                (1, Act::Command(0, "gamemode creative @a")),
                (2, Act::Command(1, "give @a minecraft:stone 3")),
                (3, Act::Command(2, "tellraw @a {\"text\":\"hi\"}")),
                (4, Act::Chat(2, "hello everyone")),
                (4, Act::Chat(0, "hello back")),
                (5, Act::Console("say from the console")),
                (6, Act::Command(0, "tp {g1} {c1}")),
                (7, Act::Command(1, "msg {g2} psst")),
                (8, Act::Command(2, "gamemode survival {g0}")),
                (9, Act::Command(0, "time set 1000")),
                (10, Act::Console("give {g2} minecraft:dirt 2")),
                // 要整个世界的命令（计划 4.5）：目标在别的岛的方块、跨岛的复制、读别的岛上
                // 玩家脚下的方块。
                // Whole-world commands (plan 4.5): blocks on another island, a
                // cross-island clone, reading the block under a player elsewhere.
                (11, Act::Command(0, "setblock {c2} minecraft:glowstone")),
                (12, Act::Command(1, "fill {c0} {c0} minecraft:stone")),
                (13, Act::Command(1, "clear @a minecraft:stone")),
                (14, Act::Command(2, "clone {c2} {c2} {c1}")),
                (15, Act::Command(0, "gamemode adventure @a")),
                (16, Act::Chat(0, "bye")),
                (17, Act::Console("gamemode creative {g1}")),
                (18, Act::Command(0, "list")),
                (
                    19,
                    Act::Command(1, "random value 1..1000000 minecraft:shared"),
                ),
                (
                    20,
                    Act::Command(0, "random value 1..1000000 minecraft:shared"),
                ),
                // 命令生成的实体落在别的岛上（取发起人岛的实体种子源，两边对齐过）。
                // Entities spawned by commands onto other islands (from the
                // sender island's seed source, aligned on both sides).
                (21, Act::Command(0, "summon minecraft:cow {c1}")),
                (
                    22,
                    Act::Command(0, "loot spawn {c2} loot minecraft:blocks/diamond_ore"),
                ),
                (23, Act::Console("gamemode survival @a")),
            ];
            let (mut lockstep, mut hybrid) =
                set_up_pair(seed, Harness::new_islands(2, 0), Harness::new_islands(2, 4));
            assert!(hybrid.server.world.hybrid() && !lockstep.server.world.hybrid());
            align(&mut lockstep, &mut hybrid);
            for scene in [&lockstep, &hybrid] {
                assert_eq!(
                    scene.server.world.islands.len(),
                    scene.centers.len(),
                    "seed {seed}：每组一个岛"
                );
            }
            // 两个世界搭场景用的刻数不同（分离驱动的交付晚一刻），岛建出来的时刻不同，
            // 按（世界种子，起源，建岛时刻）派生的随机源也就不同。对照从这里开始：两边在
            // 同一刻把全部岛并成一个、换成同一份随机源、再按组拆开，之后每个岛的随机源
            // 两边相同。
            // The two worlds took different tick counts to set up (detached
            // deliveries are a tick later), so their islands were created at
            // different times and derived different random sources. The
            // comparison starts here: both merge everything, take the same
            // sources and split by group at the same tick.
            let split_at = (
                super::island_alone_tests::regroup(&mut lockstep, true),
                super::island_alone_tests::regroup(&mut hybrid, true),
            );
            assert_eq!(split_at.0, split_at.1, "两边在同一刻拆开");
            prepare(&mut lockstep);
            prepare(&mut hybrid);
            hybrid.server.world.islands.lend_nothing = lend_nothing;
            for tick in 0..26_u64 {
                for (_, step) in script.iter().filter(|(at, _)| *at == tick) {
                    act(&mut lockstep, step);
                    act(&mut hybrid, step);
                }
                lockstep.server.tick();
                hybrid.server.tick();
                let what = format!("seed {seed} tick {tick}");
                assert!(hybrid.server.world.islands.all_parked(), "{what}");
                assert_eq!(hybrid.server.world.master.hybrid.held_len(), 0, "{what}");
                for group in 0..lockstep.centers.len() {
                    assert_same_state(
                        group_state(&mut hybrid, group),
                        group_state(&mut lockstep, group),
                        &format!("{what}：第 {group} 组的内容和时钟"),
                    );
                }
                let mut differing = Vec::new();
                let mut seen = Vec::new();
                for ((_, mine), (_, theirs)) in hybrid.clients.iter().zip(&lockstep.clients) {
                    let (mine_frames, their_frames) = (mine.take_frames(), theirs.take_frames());
                    let ids =
                        |frames: &[Frame]| frames.iter().map(|frame| frame.id).collect::<Vec<_>>();
                    seen.push((mine.id, ids(&mine_frames), ids(&their_frames)));
                    if mine_frames != their_frames {
                        differing.push(mine.id);
                    }
                }
                assert!(
                    differing.is_empty(),
                    "{what}：玩家 {differing:?} 收到的字节不同；这一刻每名玩家收到的包 ID\
                     （混合，齐步）：{seen:?}；排着队的事 {} 件，欠刻 {:?}",
                    hybrid.server.world.master.hybrid.held_len(),
                    hybrid
                        .server
                        .world
                        .islands
                        .ids()
                        .iter()
                        .map(|&id| hybrid.server.world.islands.debt(id))
                        .collect::<Vec<_>>()
                );
                hybrid.server.world.assert_island_invariants();
            }
            // 命令生成的实体真的落在了别的岛上：牛在第 1 组的岛上，掉落物在第 2 组的岛上。
            // The spawned entities really landed on other islands.
            let kinds = |scene: &mut Scene, group: usize| -> (usize, usize) {
                let id = island_of_group(scene, group);
                let island = scene.server.world.islands.get(id).expect("停放着");
                island
                    .region
                    .entities
                    .iter_public()
                    .fold((0, 0), |(mobs, items), public| match public.kind {
                        mc_bamboo_blocks::entity::KindPublic::Mob(_) => (mobs + 1, items),
                        mc_bamboo_blocks::entity::KindPublic::Item { .. } => (mobs, items + 1),
                        _ => (mobs, items),
                    })
            };
            if !lend_nothing {
                assert!(
                    kinds(&mut hybrid, 1).0 >= 1,
                    "seed {seed}：牛在第 1 组的岛上"
                );
                assert!(
                    kinds(&mut hybrid, 2).1 >= 1,
                    "seed {seed}：掉落物在第 2 组的岛上"
                );
            }
        }
    }
}

/// 慢岛那一段的参数：哪一组的岛慢、从第几刻到第几刻不回来。
#[derive(Clone, Copy)]
struct Slow {
    group: usize,
    first: u64,
    last: u64,
}

/// 一个岛慢的时候，别的岛照常前进：对照是同一个世界、没有慢岛。慢岛不在的每一刻，别的
/// 组的内容、时钟和别的玩家收到的字节都和对照相同（窗口里有健康岛上的聊天，慢岛上的人晚
/// 收到）。慢岛回来以后连续追赶：这一刻结束时它的时钟和别的岛相同、内容和对照相同，之后
/// 每一刻连它的玩家收到的字节也和对照相同。返回慢岛的玩家在它回来那一刻收到的帧。
///
/// While one island is slow the others go on: the control is the same world
/// without a slow island. Every tick the slow island is away, the other
/// groups' content and clocks and the other players' bytes match the control
/// (healthy islands chat in the window; the slow island's players get it
/// late). Once back the slow island catches up back to back: at the end of
/// that tick its clock equals the others' and its content the control's, and
/// from then on even its players' bytes match the control tick by tick.
fn run_slow_island(seed: u64, slow: Slow, breaks: Breaks) {
    let mut control = scene(seed, 4);
    let mut lagging = scene(seed, 4);
    align(&mut control, &mut lagging);
    prepare(&mut control);
    prepare(&mut lagging);
    lagging.server.world.master.hybrid.breaks = breaks;
    let victim = island_of_group(&mut lagging, slow.group);
    lagging
        .server
        .world
        .stall_island(victim, slow.first, slow.last);
    let groups = control.centers.len();
    let healthy: Vec<usize> = (0..groups).filter(|&group| group != slow.group).collect();
    let chatter = healthy[0];
    let mut late_frames: Vec<Frame> = Vec::new();
    for tick in 1..=slow.last + 12 {
        if tick == slow.first + 1 || tick == slow.first + 2 {
            let step = Act::Chat(chatter, "while you were slow");
            act(&mut control, &step);
            act(&mut lagging, &step);
        }
        control.server.tick();
        lagging.server.tick();
        let what = format!("seed {seed} 慢的是第 {} 组，tick {tick}", slow.group);
        let away = (slow.first..=slow.last).contains(&tick);
        assert_eq!(
            lagging.server.world.islands.is_parked(victim),
            !away,
            "{what}：慢岛在不在主控手里"
        );
        for &group in &healthy {
            assert_same_state(
                group_state(&mut lagging, group),
                group_state(&mut control, group),
                &format!("{what}：第 {group} 组（健康的岛）的内容和时钟"),
            );
            for (mine, theirs) in clients_of(&lagging, group)
                .iter()
                .zip(&clients_of(&control, group))
            {
                assert_eq!(
                    mine.take_frames(),
                    theirs.take_frames(),
                    "{what}：健康岛上的玩家 {:?} 收到的字节",
                    mine.id
                );
            }
        }
        if away {
            for client in clients_of(&lagging, slow.group) {
                assert!(
                    client.take_frames().is_empty(),
                    "{what}：慢岛不在时它的玩家什么都收不到"
                );
            }
            for client in clients_of(&control, slow.group) {
                late_frames.extend(client.take_frames());
            }
            continue;
        }
        // 慢岛在主控手里：追平了，内容和时钟都和对照相同。
        // The slow island is parked: caught up, content and clock as the control's.
        assert_same_state(
            group_state(&mut lagging, slow.group),
            group_state(&mut control, slow.group),
            &format!("{what}：慢岛的内容和时钟"),
        );
        assert_eq!(
            lagging.server.world.islands.debt(victim),
            0,
            "{what}：不欠刻"
        );
        let mine: Vec<Vec<Frame>> = clients_of(&lagging, slow.group)
            .iter()
            .map(Client::take_frames)
            .collect();
        let theirs: Vec<Vec<Frame>> = clients_of(&control, slow.group)
            .iter()
            .map(Client::take_frames)
            .collect();
        if tick == slow.last + 1 {
            // 回来的这一刻：它的玩家补收到慢的那几刻里的全部聊天，一条不少、一条不多。
            // The tick it returns: its players get every chat of the slow
            // ticks, none missing and none twice.
            for frames in &mine {
                assert_eq!(
                    count_marker(frames, "while you were slow"),
                    2,
                    "{what}：慢岛上的玩家补收到两条聊天"
                );
            }
            assert_eq!(
                count_marker(&late_frames, "while you were slow"),
                2 * mine.len(),
                "{what}：对照里这两条聊天是在慢的那几刻里收到的"
            );
        } else if tick > slow.last + 1 {
            assert_eq!(
                mine, theirs,
                "{what}：追平以后慢岛上的玩家收到的字节也和对照相同"
            );
        }
    }
    lagging.server.world.assert_island_invariants();
}

/// 一个岛慢 8 刻（不是家岛、是家岛各一遍）：别的岛照常前进，慢岛回来以后追平，见
/// [`run_slow_island`]。
///
/// One island is slow for 8 ticks (once a non-home island, once the home
/// island): the others go on and the slow one catches up; see
/// [`run_slow_island`].
#[test]
fn a_slow_island_only_slows_itself_and_catches_up() {
    bounded(|| {
        for (seed, group) in [(51_u64, 1), (52, 0), (53, 2)] {
            run_slow_island(
                seed,
                Slow {
                    group,
                    first: 3,
                    last: 10,
                },
                Breaks::default(),
            );
        }
    });
}

/// 改坏确认：慢岛回来以后不追赶，上面的对照在它回来的那一刻报出时钟不同。
///
/// Break-it check: without catching up, the comparison above reports the
/// clock the tick the slow island returns.
#[test]
fn not_catching_up_is_caught() {
    bounded(|| {
        let slow = Slow {
            group: 1,
            first: 3,
            last: 10,
        };
        run_slow_island(51, slow, Breaks::default());
        let caught = std::panic::catch_unwind(|| {
            run_slow_island(
                51,
                slow,
                Breaks {
                    no_catch_up: true,
                    ..Breaks::default()
                },
            );
        });
        let text = panic_text(caught.expect_err("不追赶没有被抓到"));
        assert!(text.contains("慢岛的内容和时钟"), "失败的原因不对：{text}");
    });
}

/// 改坏确认：给不在的岛的出站投递丢掉 / 抄两份，慢岛上的玩家补收到的聊天条数就不对。
///
/// Break-it check: dropping or doubling the deferred deliveries makes the
/// slow island's players get the wrong number of chat messages.
#[test]
fn losing_or_doubling_deferred_deliveries_is_caught() {
    bounded(|| {
        let slow = Slow {
            group: 1,
            first: 3,
            last: 10,
        };
        for breaks in [
            Breaks {
                drop_fan_out: true,
                ..Breaks::default()
            },
            Breaks {
                fan_out_twice: true,
                ..Breaks::default()
            },
        ] {
            let caught = std::panic::catch_unwind(|| run_slow_island(51, slow, breaks));
            let text = panic_text(caught.expect_err("出站投递改坏了没有被抓到"));
            assert!(
                text.contains("慢岛上的玩家补收到两条聊天"),
                "失败的原因不对：{text}"
            );
        }
    });
}

/// panic 的文字。
fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
        })
        .unwrap_or_default()
}

/// 慢岛欠的刻超过 2 秒（40 刻）就全部丢掉：它回来以后不追赶，时钟从此比别的岛慢这么多；
/// 之后和别的岛合并照样按时钟差平移，机器照常运转。
///
/// More than 2 seconds (40 ticks) owed are dropped: the island does not
/// catch up and its clock stays that far behind; a later merge still rebases
/// by the clock difference and the machines keep running.
#[test]
fn ticks_owed_beyond_two_seconds_are_dropped() {
    bounded(|| {
        let mut scene = scene(54, 4);
        for _ in 0..3 {
            scene.server.tick();
        }
        prepare(&mut scene);
        let victim = island_of_group(&mut scene, 1);
        scene.server.world.stall_island(victim, 1, 45);
        for _ in 0..45 {
            scene.server.tick();
            assert!(!scene.server.world.islands.is_parked(victim));
        }
        scene.server.tick();
        assert!(scene.server.world.islands.is_parked(victim));
        assert_eq!(scene.server.world.islands.debt(victim), 0, "欠的刻丢掉了");
        let (_, healthy) = group_state(&mut scene, 0).expect("停放着");
        let (_, slow) = group_state(&mut scene, 1).expect("停放着");
        // 慢岛在那 45 刻里只跑了一刻，回来的这一刻又跑了一刻。
        // The slow island ran one tick in those 45 and one more on its return.
        assert_eq!(healthy - slow, 44, "时钟慢了丢掉的那 44 刻");
        // 合并：时钟不同的两个岛按时钟差平移（内容不变由下面
        // `merges_involving_a_slow_island_wait_and_still_happen` 逐项核对）。
        // Merge: islands with different clocks are rebased (the content is
        // checked item by item in the deferred-merge test below).
        let world = &mut scene.server.world;
        let into = world.master.host.island_of(scene.centers[0]).expect("有岛");
        let (into, from) = (into.min(victim), into.max(victim));
        let partition = world.master.host.partition_mut().expect("多岛开着");
        assert!(partition.force_merge(into, from));
        world.master.host.take_partition_ops();
        world.settle_islands();
        assert!(!world.islands.contains(from), "并掉了");
        assert!(
            !group_print_on(&mut scene, into, 1).is_empty(),
            "第 1 组现在在吸收方的岛上"
        );
        scene.server.world.assert_island_invariants();
        for _ in 0..5 {
            scene.server.tick();
        }
        assert!(scene.server.world.islands.all_parked());
        scene.server.world.assert_island_invariants();
    });
}

/// 慢岛不在的时候排队的事。返回每名玩家（按进服的先后）收到的标记的先后，以及第 0 组
/// 发命令的那名玩家在慢岛回来之前、之后的位置有没有变。
struct HeldOutcome {
    orders: Vec<Vec<String>>,
    moved_while_held: bool,
    moved_afterwards: bool,
}

const MARKERS: [&str; 6] = [
    "MARK-C1",
    "MARK-P1",
    "MARK-CHAT",
    "MARK-C2",
    "MARK-Q1",
    "MARK-C3",
];

/// 有岛没停放时，要全部岛的事（控制台命令、玩家命令）整件排队，玩家发起的连同他之后的
/// 入站消息一起按原来的先后排；慢岛回来以后按排队的先后做，一件不丢、一件不重。
///
/// While an island is away, work that needs every island (console commands,
/// player commands) is held whole, a player's together with their later
/// inbound messages in order; once the slow island is back it all runs in
/// arrival order, nothing lost and nothing twice.
fn run_held_work(breaks: Breaks) -> HeldOutcome {
    let mut scene = scene(61, 4);
    for _ in 0..3 {
        scene.server.tick();
    }
    prepare(&mut scene);
    scene.server.world.master.hybrid.breaks = breaks;
    let victim = island_of_group(&mut scene, 1);
    scene.server.world.stall_island(victim, 2, 8);
    let sender = first_of(&scene, 0);
    let start = region_of_player(&mut scene.server.world, sender.id).players[&sender.id].position();
    let mut moved_while_held = false;
    let mut frames: Vec<Vec<Frame>> = vec![Vec::new(); scene.clients.len()];
    for tick in 1..=12_u64 {
        match tick {
            3 => {
                console(&scene, "say MARK-C1");
                command(&scene, 0, "tellraw @a {\"text\":\"MARK-P1\"}");
                // 同一名玩家紧跟着的聊天和移动：要排在他的命令后面。
                // The same player's chat and move right after: they queue
                // behind the command.
                act(&mut scene, &Act::Chat(0, "MARK-CHAT"));
                scene.server.request(
                    &sender,
                    PlayRequest::MovePlayer {
                        position: Some(Vec3::new(start.x + 1.0, start.y, start.z)),
                        rotation: None,
                        on_ground: true,
                        horizontal_collision: false,
                    },
                );
            }
            4 => {
                console(&scene, "say MARK-C2");
                command(&scene, 2, "tellraw @a {\"text\":\"MARK-Q1\"}");
            }
            6 => console(&scene, "say MARK-C3"),
            _ => {}
        }
        scene.server.tick();
        let parked = scene.server.world.islands.is_parked(victim);
        for (index, (_, client)) in scene.clients.iter().enumerate() {
            let got = client.take_frames();
            if !parked {
                for marker in MARKERS {
                    assert_eq!(
                        count_marker(&got, marker),
                        0,
                        "tick {tick}：慢岛还没回来，{marker} 不该已经做了"
                    );
                }
            }
            frames[index].extend(got);
        }
        if tick == 8 {
            assert!(
                scene.server.world.master.hybrid.held_len() >= 6,
                "慢岛回来之前这些事都排着"
            );
            let here =
                region_of_player(&mut scene.server.world, sender.id).players[&sender.id].position();
            moved_while_held = here != start;
        }
    }
    assert!(scene.server.world.islands.all_parked());
    assert_eq!(scene.server.world.master.hybrid.held_len(), 0, "都做完了");
    let here = region_of_player(&mut scene.server.world, sender.id).players[&sender.id].position();
    for (index, got) in frames.iter().enumerate() {
        for marker in MARKERS {
            assert_eq!(
                count_marker(got, marker),
                1,
                "第 {index} 名玩家收到 {marker} 的次数"
            );
        }
    }
    HeldOutcome {
        orders: frames
            .iter()
            .map(|got| marker_order(got, &MARKERS))
            .collect(),
        moved_while_held,
        moved_afterwards: here != start,
    }
}

/// 见 [`run_held_work`]：每名玩家（含慢岛上的）都按排队的先后各收到一次；发命令的玩家
/// 之后的移动在慢岛回来之前没有被处理（没有跑到他的命令前面去），回来之后处理了。
///
/// See [`run_held_work`]: every player, the slow island's included, gets each
/// marker once in arrival order; the sender's later move is not handled
/// before the slow island is back (it does not overtake the command) and is
/// handled afterwards.
#[test]
fn work_held_for_a_slow_island_runs_in_order_exactly_once() {
    bounded(|| {
        let outcome = run_held_work(Breaks::default());
        let expected: Vec<String> = MARKERS.iter().map(|marker| (*marker).to_owned()).collect();
        for (index, order) in outcome.orders.iter().enumerate() {
            assert_eq!(order, &expected, "第 {index} 名玩家收到的先后");
        }
        assert!(!outcome.moved_while_held, "命令还排着时后面的移动不能先做");
        assert!(outcome.moved_afterwards, "命令做完以后移动照做");
    });
}

/// 改坏确认：排着队的事倒着做、排进来就丢、命令不等全部岛，上面的测试分别因为先后不对、
/// 少收、慢岛上的玩家没收到而失败。
///
/// Break-it check: running held work backwards, dropping it, or not waiting
/// for every island fails the test above on order, on a missing marker, and
/// on the slow island's players missing it.
#[test]
fn breaking_the_held_queue_is_caught() {
    bounded(|| {
        let caught = std::panic::catch_unwind(|| {
            let outcome = run_held_work(Breaks {
                held_backwards: true,
                ..Breaks::default()
            });
            let expected: Vec<String> = MARKERS.iter().map(|marker| (*marker).to_owned()).collect();
            for order in &outcome.orders {
                assert_eq!(order, &expected, "收到的先后");
            }
        });
        // 倒着做时，同一名玩家后发的聊天抢在他还排着的命令前面做了，或者最后的先后不对。
        // Backwards, the player's later chat runs before their held command,
        // or the final order is wrong.
        let text = panic_text(caught.expect_err("倒着做没有被抓到"));
        assert!(
            text.contains("MARK-CHAT 不该已经做了") || text.contains("收到的先后"),
            "失败的原因不对：{text}"
        );

        let caught = std::panic::catch_unwind(|| {
            run_held_work(Breaks {
                drop_held: true,
                ..Breaks::default()
            })
        });
        // 命令排进来就丢了：同一名玩家后面的聊天前面没有东西挡着，当场就做了；或者到慢岛
        // 回来之前队列里少了东西。
        // The command is dropped as it is queued: nothing holds back the
        // player's later chat, or the queue is short before the island returns.
        let text = panic_text(caught.err().expect("丢掉排着的事没有被抓到"));
        assert!(
            text.contains("MARK-CHAT 不该已经做了") || text.contains("慢岛回来之前这些事都排着"),
            "失败的原因不对：{text}"
        );

        let caught = std::panic::catch_unwind(|| {
            run_held_work(Breaks {
                commands_do_not_wait: true,
                ..Breaks::default()
            })
        });
        let text = panic_text(caught.err().expect("命令不等全部岛没有被抓到"));
        assert!(text.contains("不该已经做了"), "失败的原因不对：{text}");
    });
}

/// 慢岛不在期间发往它的玩家的聊天最终都送达、顺序正确：慢岛上的玩家收到的聊天包（含每名
/// 接收者自己的消息序号）和对照（没有慢岛）里同一名玩家收到的逐字节相同。聊天来自两个
/// 健康的岛，慢岛回来以后它自己的玩家再说一句。
///
/// Chat sent to a slow island's players while it is away all arrives, in
/// order: the chat packets its players receive (each receiver's own message
/// index included) match, byte for byte, what the same players receive in a
/// control without a slow island. The chat comes from two healthy islands,
/// and the slow island's own player speaks once it is back.
fn run_chat_to_a_slow_island(breaks: Breaks) {
    let mut control = scene(71, 4);
    let mut lagging = scene(71, 4);
    align(&mut control, &mut lagging);
    prepare(&mut control);
    prepare(&mut lagging);
    lagging.server.world.master.hybrid.breaks = breaks;
    let victim = island_of_group(&mut lagging, 2);
    lagging.server.world.stall_island(victim, 2, 9);
    let script: Vec<(u64, Act)> = vec![
        (1, Act::Chat(2, "before")),
        (3, Act::Chat(0, "one")),
        (3, Act::Chat(1, "two")),
        (4, Act::Chat(1, "three")),
        (5, Act::Chat(0, "four")),
        (7, Act::Chat(0, "five")),
        (11, Act::Chat(2, "after")),
        (12, Act::Chat(1, "six")),
    ];
    let chats = |frames: Vec<Frame>| -> Vec<Frame> {
        frames
            .into_iter()
            .filter(|frame| frame.id == clientbound::PLAYER_CHAT)
            .collect()
    };
    let mut mine: BTreeMap<usize, Vec<Frame>> = BTreeMap::new();
    let mut theirs: BTreeMap<usize, Vec<Frame>> = BTreeMap::new();
    for tick in 1..=14_u64 {
        for (_, step) in script.iter().filter(|(at, _)| *at == tick) {
            act(&mut control, step);
            act(&mut lagging, step);
        }
        control.server.tick();
        lagging.server.tick();
        for (index, (_, client)) in lagging.clients.iter().enumerate() {
            mine.entry(index)
                .or_default()
                .extend(chats(client.take_frames()));
        }
        for (index, (_, client)) in control.clients.iter().enumerate() {
            theirs
                .entry(index)
                .or_default()
                .extend(chats(client.take_frames()));
        }
    }
    for (index, (group, _)) in lagging.clients.iter().enumerate() {
        assert_eq!(theirs[&index].len(), script.len(), "对照里每条聊天都收到");
        assert_eq!(
            mine[&index], theirs[&index],
            "第 {group} 组的玩家收到的聊天包（内容、顺序、消息序号）"
        );
    }
}

#[test]
fn chat_for_a_slow_island_arrives_in_order() {
    bounded(|| run_chat_to_a_slow_island(Breaks::default()));
}

/// 改坏确认：给不在的岛的出站投递倒着发，慢岛上的玩家收到的聊天包顺序就和对照不同。
///
/// Break-it check: applying the deferred deliveries backwards changes the
/// order of the chat packets the slow island's players receive.
#[test]
fn delivering_deferred_chat_out_of_order_is_caught() {
    bounded(|| {
        let caught = std::panic::catch_unwind(|| {
            run_chat_to_a_slow_island(Breaks {
                reverse_fan_out: true,
                ..Breaks::default()
            });
        });
        let text = panic_text(caught.expect_err("倒着发没有被抓到"));
        assert!(text.contains("收到的聊天包"), "失败的原因不对：{text}");
    });
}

/// 涉及慢岛的拆合推迟、不涉及它的照常，推迟的在它回来以后照样发生。四个岛（三组加一个
/// 只有强加载票的岛）：先登记「慢岛并进家岛」，再登记「另外两个岛合并」。慢岛不在时
/// 后一条照做（它不碰被前一条卷进去的岛），前一条留着；慢岛回来的那一刻前一条照做，
/// 它那一组的内容在合并前后相同。
///
/// Changes involving a slow island wait, others go ahead, and the waiting
/// one still happens once the island is back. Four islands (three groups and
/// one with only a forced ticket): first "the slow island merges into home",
/// then "the other two merge". While the slow island is away the second is
/// applied (it touches no island of the first) and the first stays; the tick
/// the slow island returns the first is applied and its group's content is
/// the same before and after.
fn run_deferred_merge(breaks: Breaks, by_hand: bool) {
    let mut scene = scene(81, 4);
    // 第四个岛：离三组都很远的一张强加载票。
    // A fourth island: a forced ticket far from every group.
    let far = ChunkPos::new(-90, -90);
    scene.server.apply_ticket_for_test(TicketOp::Forced {
        pos: far,
        forced: true,
    });
    for _ in 0..6 {
        scene.server.tick();
    }
    prepare(&mut scene);
    // 先空跑一阵把摆机器带来的光照算完：合并会把攒着的光照一次算完，那样合并前后的
    // 区块内容就不能直接比了。
    // Idle until the light from furnishing has settled: a merge finishes
    // pending light at once, which would make before and after incomparable.
    for _ in 0..12 {
        scene.server.tick();
    }
    scene.server.world.master.hybrid.breaks = breaks;
    let world = &mut scene.server.world;
    assert!(
        world.islands.island_of_chunk(far).is_some(),
        "强加载的区块交付了"
    );
    let ids = world.islands.ids();
    assert_eq!(ids.len(), 4, "四个岛");
    assert_eq!(ids[0], HOME_REGION);
    let (victim, third, fourth) = (ids[1], ids[2], ids[3]);
    let slow_group = (0..scene.centers.len())
        .find(|&group| island_of_group(&mut scene, group) == victim)
        .expect("慢岛拿着一组");
    scene.server.world.stall_island(victim, 1, 5);
    scene.server.tick();
    let world = &mut scene.server.world;
    assert!(!world.islands.is_parked(victim));
    let partition = world.master.host.partition_mut().expect("多岛开着");
    assert!(partition.force_merge(HOME_REGION, victim));
    assert!(partition.force_merge(third, fourth));
    world.master.host.take_partition_ops();
    world.settle_islands();
    assert!(
        !world.islands.contains(fourth) && world.islands.contains(victim),
        "不涉及慢岛的合并照做，涉及它的留着"
    );
    assert!(world.master.host.has_topology(), "涉及慢岛的合并还排着");
    for _ in 0..4 {
        scene.server.tick();
        assert!(scene.server.world.islands.contains(victim), "慢岛还没并掉");
    }
    if by_hand {
        // 只走第 ⓪ 段（开闸、落地），在合并前后各取一次慢岛那一组的指纹：除了绝对时刻
        // 整体平移了两岛的时钟差以外，一项不多、一项不少。
        // Part 0 only (open the gate, land), with the slow group's print
        // taken before and after the merge: apart from absolute instants
        // rebased by the clock difference, nothing is added or missing.
        let _ = scene.server.world.begin_hybrid_tick();
        assert!(scene.server.world.islands.is_parked(victim), "慢岛回来了");
        let clock = |scene: &Scene, id: RegionId| {
            let island = scene.server.world.islands.get(id).expect("停放着");
            i128::from(island.region.tick.game_time())
        };
        let delta = clock(&scene, HOME_REGION) - clock(&scene, victim);
        assert_eq!(delta, 4, "慢岛的时钟落后 4 刻");
        let before = group_print_on(&mut scene, victim, slow_group);
        assert!(!before.is_empty(), "慢岛上有它那一组的东西");
        scene.server.world.settle_islands();
        assert!(
            !scene.server.world.islands.contains(victim),
            "推迟的合并照样发生"
        );
        let after = group_print_on(&mut scene, HOME_REGION, slow_group);
        assert_same_print_shifted(&before, &after, delta, "合并前后慢岛那一组的内容");
    } else {
        // 照常跑慢岛回来的那一刻：它落地以后，这一刻里合并照做。
        // Run the tick the slow island returns as usual: once it has landed
        // the merge is applied within the tick.
        scene.server.tick();
        assert!(
            !scene.server.world.islands.contains(victim),
            "推迟的合并照样发生"
        );
    }
    assert!(!scene.server.world.master.host.has_topology());
    scene.server.world.assert_island_invariants();
    for _ in 0..5 {
        scene.server.tick();
    }
    assert!(scene.server.world.islands.all_parked());
    scene.server.world.assert_island_invariants();
}

#[test]
fn merges_involving_a_slow_island_wait_and_still_happen() {
    bounded(|| {
        run_deferred_merge(Breaks::default(), false);
        run_deferred_merge(Breaks::default(), true);
    });
}

/// 改坏确认：卷进不在的岛的拆合直接丢掉，上面的测试报「推迟的合并照样发生」不成立。
///
/// Break-it check: dropping a change that involves an absent island fails
/// the test above where the deferred merge should have happened.
#[test]
fn forgetting_a_deferred_merge_is_caught() {
    bounded(|| {
        let caught = std::panic::catch_unwind(|| {
            run_deferred_merge(
                Breaks {
                    forget_blocked_topology: true,
                    ..Breaks::default()
                },
                false,
            );
        });
        let text = panic_text(caught.expect_err("丢掉推迟的拆合没有被抓到"));
        assert!(
            text.contains("涉及慢岛的合并还排着") || text.contains("推迟的合并照样发生"),
            "失败的原因不对：{text}"
        );
    });
}

/// 分离驱动下投给还不存在的岛编号的信（只有强加载票、没有玩家的新岛）：岛被建出来、
/// 区块交付。`fixed` 为假时回到修之前的样子（不建岛）。返回那个区块有没有交付。
fn forced_chunk_far_away_is_delivered(fixed: bool) -> bool {
    let mut scene = scene(91, 2);
    scene.server.world.master.hybrid.breaks.no_islands_for_mail = !fixed;
    let far = ChunkPos::new(95, -95);
    scene.server.apply_ticket_for_test(TicketOp::Forced {
        pos: far,
        forced: true,
    });
    for _ in 0..10 {
        scene.server.tick();
    }
    scene.server.world.islands.island_of_chunk(far).is_some()
}

/// 上一任的疑点 3：分离驱动下只有强加载票、没有玩家的新岛现在建得出来；改回去（不给
/// 投给不存在的岛编号的信建岛）区块就永远交付不了。
///
/// The predecessor's open issue 3: under the detached drive a new island
/// with only a forced ticket and no player is now created; without the fix
/// its chunk is never delivered.
#[test]
fn mail_for_an_island_that_does_not_exist_yet_creates_it() {
    bounded(|| {
        assert!(forced_chunk_far_away_is_delivered(true), "区块交付了");
        assert!(
            !forced_chunk_far_away_is_delivered(false),
            "改坏确认：不建岛时区块交付不了"
        );
    });
}

/// 时钟不同的岛之间借玩家：玩家身上按岛时钟记的两样绝对时刻（传送确认的 `sent_at`、
/// 讲台待发格子的 `deliver_at`）在借来的岛上离「现在」的距离和在自己岛上一样，还回去
/// 以后原样。`shift` 为假时故意不平移。返回（借来时两样的相对时刻，原岛上的相对时刻，
/// 还回去以后是否原样）。
fn borrowed_instants(shift: bool) -> ((i64, i64), (i64, i64), bool) {
    let mut scene = scene(95, 0);
    for _ in 0..3 {
        scene.server.tick();
    }
    prepare(&mut scene);
    let client = first_of(&scene, 1);
    let world = &mut scene.server.world;
    world.islands.lend_unshifted = !shift;
    let lender = world.islands.island_of_player(client.id).expect("在岛上");
    let host = world
        .islands
        .ids()
        .into_iter()
        .find(|&id| id != lender)
        .expect("有别的岛");
    // 借出方的时钟落后 7 刻（慢岛丢过刻的样子）。
    // The lender's clock is 7 ticks behind, as after dropped ticks.
    let region = &mut world.islands.get_mut(lender).expect("在表里").region;
    region.rebase_clock(-7);
    let now = region.tick;
    region
        .players
        .get_mut(&client.id)
        .expect("在岛上")
        .teleport
        .sent_at = now.rebased(-3);
    region
        .lecterns
        .insert_pending_for_test(client.id, vec![1], now.plus(4));
    let stamp = |region: &crate::region::Region| -> (i64, i64) {
        let now = region.tick.game_time();
        (
            region.players[&client.id].teleport.sent_at.game_time() - now,
            region
                .lecterns
                .pending_at_for_test(client.id)
                .expect("待发格子在")
                .game_time()
                - now,
        )
    };
    let at_home = stamp(&world.islands.get(lender).expect("在表里").region);
    let borrowed = world.on_master(host, crate::island::Borrow::All, |_, region| stamp(region));
    let back = stamp(&world.islands.get(lender).expect("在表里").region);
    (borrowed, at_home, back == at_home)
}

/// 见 [`borrowed_instants`]：平移以后借来的玩家的两样时刻和在自己岛上一样；不平移就差
/// 两岛的时钟差（改坏确认）。
///
/// See [`borrowed_instants`]: with the rebase a borrowed player's two
/// instants read as on their own island; without it they are off by the
/// clock difference (break-it check).
#[test]
fn borrowing_players_across_different_clocks_rebases_their_instants() {
    bounded(|| {
        let (borrowed, at_home, restored) = borrowed_instants(true);
        assert_eq!(at_home, (-3, 4));
        assert_eq!(borrowed, at_home, "借来以后离现在的距离不变");
        assert!(restored, "还回去以后原样");
        let (borrowed, at_home, restored) = borrowed_instants(false);
        assert_eq!(
            (borrowed.0 - at_home.0, borrowed.1 - at_home.1),
            (-7, -7),
            "改坏确认：不平移时差两岛的时钟差"
        );
        assert!(restored);
    });
}

/// 真的截止时间、一个岛卡死：主控每刻到点就走，别的岛照常前进；关服时等够超时就不等
/// 它，停放着的岛上改过的区块存下来、锁放掉，同目录马上能重开。
///
/// A real deadline with one island stuck: the master moves on at each
/// deadline and the other islands advance; at shutdown it stops waiting past
/// the timeout, the parked islands' edited chunks are saved, the lock is
/// released and the directory reopens at once.
#[test]
fn a_stuck_island_is_left_behind_and_the_others_are_saved() {
    bounded(|| {
        let root =
            std::env::temp_dir().join(format!("bamboo-island-hybrid-stuck-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let centers;
        let stuck_group;
        let release = Arc::new(AtomicBool::new(false));
        {
            let mut scene = set_up(101, Harness::new_islands_with_save(2, &root, 4));
            centers = scene.centers.clone();
            for _ in 0..3 {
                scene.server.tick();
            }
            let victim = island_of_group(&mut scene, 1);
            stuck_group = 1;
            let world = &mut scene.server.world;
            world.master.island_stop_timeout = Duration::from_millis(300);
            for &center in &centers {
                let pos = super::island_split_tests::at(center, 4, -60, 4);
                let island = world.islands.island_of_chunk(center).expect("有岛");
                world
                    .islands
                    .get_mut(island)
                    .expect("在表里")
                    .region
                    .chunks
                    .set_block(pos, mc_bamboo_data::blocks::GOLD_BLOCK);
            }
            world.islands.get_mut(victim).expect("在表里").test_hook =
                Some(IslandTestHook::Stall(Arc::clone(&release)));
            let healthy = island_of_group(&mut scene, 0);
            let clock = |scene: &Scene| {
                scene
                    .server
                    .world
                    .islands
                    .get(healthy)
                    .expect("停放着")
                    .region
                    .tick
                    .game_time()
            };
            let before = clock(&scene);
            let started = Instant::now();
            for _ in 0..6 {
                let flow = tick_within(&mut scene.server, Duration::from_millis(40));
                assert!(flow.is_continue(), "卡死的岛不拦主控");
                assert!(!scene.server.world.islands.is_parked(victim));
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "主控没有等卡死的岛：{:?}",
                started.elapsed()
            );
            assert_eq!(clock(&scene) - before, 6, "别的岛照常跑了 6 刻");
            assert!(
                scene.server.world.islands.debt(victim) >= 5,
                "卡死的岛欠着刻"
            );
            scene.server.inbox_sender().send(Inbound::Shutdown).unwrap();
            let started = Instant::now();
            let flow = tick_within(&mut scene.server, Duration::from_millis(40));
            assert!(flow.is_break(), "关服结束世界循环");
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "关服没有被卡死的岛拖住：{:?}",
                started.elapsed()
            );
            release.store(true, std::sync::atomic::Ordering::Release);
        }
        let mut reopened = Harness::new_loaded_with_save(2, &root);
        for (group, &center) in centers.iter().enumerate() {
            if group == stuck_group {
                continue;
            }
            reopened.ensure_loaded(&[center]);
            let pos = super::island_split_tests::at(center, 4, -60, 4);
            let island = reopened
                .world
                .islands
                .island_of_chunk(center)
                .expect("有岛");
            assert_eq!(
                reopened
                    .world
                    .islands
                    .get(island)
                    .expect("在表里")
                    .region
                    .chunks
                    .block(pos),
                Some(mc_bamboo_data::blocks::GOLD_BLOCK),
                "第 {group} 组（停放着的岛）的改动存下来了"
            );
        }
        drop(reopened);
        let _ = std::fs::remove_dir_all(&root);
    });
}

/// 混合驱动下一个岛 panic、另一个岛卡死：主控把别的岛收回来（卡死的等够超时就不等），
/// 在世界线程上原样抛出，不死锁；按时的岛都交回了。
///
/// Under the hybrid drive one island panics while another is stuck: the
/// master recalls the others (giving up on the stuck one past the timeout)
/// and re-raises on the world thread without deadlocking; the islands that
/// were on time are all back.
#[test]
fn an_island_panic_is_raised_even_with_another_island_stuck() {
    bounded(|| {
        let mut scene = scene(111, 4);
        for _ in 0..3 {
            scene.server.tick();
        }
        let panicking = island_of_group(&mut scene, 1);
        let stuck = island_of_group(&mut scene, 2);
        let release = Arc::new(AtomicBool::new(false));
        let world = &mut scene.server.world;
        world.master.island_stop_timeout = Duration::from_millis(300);
        world.islands.get_mut(panicking).expect("在表里").test_hook = Some(IslandTestHook::Panic);
        world.islands.get_mut(stuck).expect("在表里").test_hook =
            Some(IslandTestHook::Stall(Arc::clone(&release)));
        let started = Instant::now();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = tick_within(&mut scene.server, Duration::from_millis(40));
        }));
        let text = panic_text(outcome.expect_err("panic 要在世界线程上抛出来"));
        assert!(
            text.contains("岛的测试 panic"),
            "抛出的是岛的 panic：{text}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "没有死锁：{:?}",
            started.elapsed()
        );
        let away = scene.server.world.islands.away_ids();
        assert_eq!(away, vec![stuck], "只有卡死的岛没回来");
        release.store(true, std::sync::atomic::Ordering::Release);
    });
}

/// 烟雾测试的一次运行：三组各自一个岛搭好以后，带真的截止时间（每刻只等 8 ms）和真的
/// 慢岛（两个岛每一刻睡 12–30 ms，一定会迟到；其中一个是家岛）跑一阵随机的聊天、玩家
/// 命令（含 `@a` 和跨岛传送）、控制台命令、走动、进服和离开。跑完去掉延迟、不带截止时间
/// 地跑到所有岛都停放、欠刻还清、排着的事做完，再断言全部不变量：每名在线玩家恰好在
/// 一个岛上，谁都没丢，出站队列发完了。
///
/// One smoke run: with one island per group set up, a real deadline (8 ms
/// per tick) and really slow islands (two islands, home among them, sleep 12
/// to 30 ms every tick and are always late) run random chat, player commands
/// (`@a` and cross-island teleports included), console commands, walking,
/// joins and leaves. Afterwards the delays are lifted and ticks without a
/// deadline run until every island is parked, owes nothing and no work is
/// held; then every invariant holds, each online player is on exactly one
/// island, none is lost and the outbound queues are empty.
fn smoke(seed: u64, threads: usize, ticks: u64) {
    let mut scene = scene(seed, threads);
    for _ in 0..3 {
        scene.server.tick();
    }
    prepare(&mut scene);
    let centers = scene.centers.clone();
    let mut next = rng(seed);
    let mut online: Vec<Client> = scene.clients.iter().map(|(_, c)| c.clone()).collect();
    let mut joined = 0;
    for (group, base) in [(0_usize, 12_u64), (1 + (seed as usize % 2), 18)] {
        let id = island_of_group(&mut scene, group);
        let lag = Duration::from_millis(base + next() % 13);
        scene
            .server
            .world
            .islands
            .get_mut(id)
            .expect("停放着")
            .test_hook = Some(IslandTestHook::Lag(lag));
    }
    let server = &mut scene.server;
    // 这次运行真的碰到了什么：有岛没按时回来的刻数、同时存在的岛数、排过队的事、
    // 延后投递过的岛。
    // What this run really hit: ticks with an island late, islands alive at
    // once, work that was held and islands that had deliveries deferred.
    let (mut late_ticks, mut fewest_islands, mut most_held, mut most_deferred) =
        (0, usize::MAX, 0, 0);
    for tick in 0..ticks {
        let pick = |next: &mut dyn FnMut() -> u64, online: &Vec<Client>| {
            online[next() as usize % online.len()].clone()
        };
        match next() % 12 {
            0 if online.len() < 8 => {
                joined += 1;
                online.push(server.join(&format!("h{joined}")));
            }
            1 if online.len() > 3 => {
                let index = next() as usize % online.len();
                let client = online.remove(index);
                server.leave(&client);
            }
            2 | 3 => {
                // 跨岛传送：用命令（玩家的岛可能不在主控手里，测试不直接改它）。
                // A cross-island teleport by command (the player's island may be away).
                let client = pick(&mut next, &online);
                let center = centers[next() as usize % centers.len()];
                let jitter = (next() % 5) as i32;
                server.request(
                    &client,
                    PlayRequest::unsigned_command(format!(
                        "tp @s {} -59 {}",
                        center.x * 16 + 4 + jitter,
                        center.z * 16 + 4 + jitter
                    )),
                );
            }
            4 | 5 => {
                let client = pick(&mut next, &online);
                server.request(&client, PlayRequest::unsigned_chat("smoke"));
            }
            6 => {
                server
                    .inbox_sender()
                    .send(Inbound::ConsoleCommand("say tick".to_owned()))
                    .unwrap();
            }
            7 => {
                let client = pick(&mut next, &online);
                server.request(
                    &client,
                    PlayRequest::unsigned_command("gamemode creative @a"),
                );
            }
            8 => {
                let client = pick(&mut next, &online);
                server.request(
                    &client,
                    PlayRequest::ChangeGameMode {
                        mode: mc_bamboo_protocol::packet::play::GameMode::Survival,
                    },
                );
            }
            _ => {}
        }
        // 确认传送、回区块批次：客户端该做的照做，玩家才会真的换岛、收区块。
        // Confirm teleports and chunk batches as a client would.
        for client in &online {
            server.confirm_teleport(client);
            server.load_player(client);
        }
        assert!(
            tick_within(server, Duration::from_millis(8)).is_continue(),
            "seed {seed} threads {threads} tick {tick}"
        );
        for client in &online {
            for _ in 0..crate::world::tests::frames_with_id(
                &client.take_frames(),
                clientbound::CHUNK_BATCH_FINISHED,
            )
            .len()
            {
                server.acknowledge_batch(client, 64.0);
            }
        }
        if server.world.islands.all_parked() {
            server.world.assert_island_invariants();
        } else {
            late_ticks += 1;
        }
        fewest_islands = fewest_islands.min(server.world.islands.len());
        most_held = most_held.max(server.world.master.hybrid.held_len());
        most_deferred = most_deferred.max(server.world.master.hybrid.fan_out.len());
    }
    assert!(
        late_ticks >= 10 && most_held >= 1 && most_deferred >= 1,
        "seed {seed} threads {threads}：这次运行没有真的碰到慢岛（{late_ticks} 刻有岛迟到，\
         最多排着 {most_held} 件事，最多 {most_deferred} 个岛有延后的投递，最少 {fewest_islands} 个岛）"
    );
    // 收尾：不带截止时间跑到稳定。停放着的岛去掉延迟；还带着延迟的岛回来以后再去掉。
    // Wind down without a deadline until stable, lifting the delays as
    // islands are seen parked.
    let mut quiet = 0;
    for round in 0..600 {
        for island in server.world.islands.parked_mut() {
            island.test_hook = None;
        }
        server.tick();
        for client in &online {
            server.confirm_teleport(client);
            server.load_player(client);
            for _ in 0..crate::world::tests::frames_with_id(
                &client.take_frames(),
                clientbound::CHUNK_BATCH_FINISHED,
            )
            .len()
            {
                server.acknowledge_batch(client, 64.0);
            }
        }
        let world = &server.world;
        let settled = world.islands.all_parked()
            && world.master.hybrid.held_len() == 0
            && world.master.hybrid.joining.is_empty()
            && world
                .islands
                .ids()
                .iter()
                .all(|&id| world.islands.debt(id) == 0);
        quiet = if settled { quiet + 1 } else { 0 };
        if quiet >= 6 {
            break;
        }
        assert!(round < 599, "seed {seed} threads {threads}：收不了尾");
    }
    let world = &mut server.world;
    world.assert_island_invariants();
    assert!(world.master.hybrid.fan_out.is_empty(), "出站队列都发完了");
    for client in &online {
        let holders = world
            .islands
            .iter()
            .filter(|island| island.region.players.contains_key(&client.id))
            .count();
        assert!(
            holders == 1 || (holders == 0 && client.is_closed()),
            "seed {seed} threads {threads}：玩家 {:?} 在 {holders} 个岛上",
            client.id
        );
    }
}

/// 多线程烟雾测试：混合驱动 1 / 4 / 8 条 tick 线程，带真的慢岛，见 [`smoke`]。
///
/// The multi-thread smoke test: the hybrid drive with 1, 4 and 8 tick
/// threads and really slow islands; see [`smoke`].
#[test]
fn slow_islands_under_a_real_deadline_keep_every_invariant() {
    bounded(|| {
        for (seed, threads) in [(21_u64, 1), (22, 4), (23, 8)] {
            smoke(seed, threads, 200);
        }
    });
}

const JOINED: &str = "multiplayer.player.joined";
const LEFT: &str = "multiplayer.player.left";

/// 进服的目标岛（出生点所在的岛）不在主控手里：进服等它停放，期间这名玩家算「正在
/// 进服」、票挂着不会被当成离开撤掉，别人收不到他的进服消息；岛回来以后进服照做，他
/// 等待期间发的包排在进服后面处理，所有人（含慢岛上的）各收到一次进服消息。
///
/// The join's target island (the spawn's) is away: the join waits for it,
/// the player counts as joining with the ticket kept, and nobody gets the
/// join message; once the island is back the join runs, what the player sent
/// meanwhile is handled after it, and everyone (the slow island's players
/// included) gets the join message once.
#[test]
fn a_join_waits_for_its_island_and_keeps_its_ticket() {
    bounded(|| {
        let mut scene = scene(121, 4);
        for _ in 0..3 {
            scene.server.tick();
        }
        prepare(&mut scene);
        let victim = island_of_group(&mut scene, 0);
        scene.server.world.stall_island(victim, 2, 7);
        let mut late: Option<Client> = None;
        let mut frames: Vec<Vec<Frame>> = vec![Vec::new(); scene.clients.len()];
        for tick in 1..=11_u64 {
            if tick == 3 {
                let client = scene.server.join("late");
                // 他在等的时候就发了一条聊天：要排在进服后面。
                // A chat sent while still waiting: it queues behind the join.
                scene
                    .server
                    .request(&client, PlayRequest::unsigned_chat("MARK-LATE"));
                late = Some(client);
            }
            scene.server.tick();
            for (index, (_, client)) in scene.clients.iter().enumerate() {
                frames[index].extend(client.take_frames());
            }
            if let (Some(client), 3..=7) = (&late, tick) {
                let world = &scene.server.world;
                assert!(
                    world.master.hybrid.joining.contains(&client.id),
                    "tick {tick}：正在进服"
                );
                assert!(
                    world.master.ticket_centers.contains_key(&client.id),
                    "tick {tick}：票挂着，没有被当成离开撤掉"
                );
                assert!(world.islands.island_of_player(client.id).is_none());
                for got in &frames {
                    assert_eq!(count_marker(got, JOINED), 0, "tick {tick}：还没进服");
                }
            }
        }
        let late = late.expect("进过服");
        let world = &scene.server.world;
        assert!(world.islands.all_parked() && world.master.hybrid.held_len() == 0);
        assert!(world.master.hybrid.joining.is_empty());
        assert!(
            world.islands.island_of_player(late.id).is_some(),
            "岛回来以后进服照做"
        );
        let own = late.take_frames();
        assert!(
            !super::tests::frames_with_id(&own, clientbound::LOGIN).is_empty(),
            "他收到了进服序列"
        );
        assert_eq!(count_marker(&own, "MARK-LATE"), 1, "他等的时候发的聊天做了");
        for (index, got) in frames.iter().enumerate() {
            assert_eq!(
                count_marker(got, JOINED),
                1,
                "第 {index} 名玩家收到一次进服消息"
            );
            assert_eq!(count_marker(got, "MARK-LATE"), 1, "第 {index} 名玩家");
            assert_eq!(
                marker_order(got, &[JOINED, "MARK-LATE"]),
                vec![JOINED.to_owned(), "MARK-LATE".to_owned()],
                "第 {index} 名玩家：聊天在进服之后"
            );
        }
        scene.server.world.assert_island_invariants();
    });
}

/// 进服还在等目标岛、玩家就断开了：离开经他自己的队列到达，等着的进服作废、票撤掉，
/// 岛回来以后他不在任何岛上，谁也没收到过他的进服消息。
///
/// The player disconnects while the join still waits for its island: the
/// leave arrives through their own queue, the waiting join is dropped with
/// its ticket, and once the island is back they are on no island and nobody
/// ever got a join message.
#[test]
fn leaving_while_the_join_still_waits_cancels_it() {
    bounded(|| {
        let mut scene = scene(122, 4);
        for _ in 0..3 {
            scene.server.tick();
        }
        prepare(&mut scene);
        let victim = island_of_group(&mut scene, 0);
        scene.server.world.stall_island(victim, 2, 7);
        let mut ghost: Option<Client> = None;
        let mut frames: Vec<Frame> = Vec::new();
        for tick in 1..=11_u64 {
            if tick == 3 {
                ghost = Some(scene.server.join("ghost"));
            }
            if tick == 5 {
                scene.server.leave(ghost.as_ref().expect("进过服"));
            }
            scene.server.tick();
            for (_, client) in &scene.clients {
                frames.extend(client.take_frames());
            }
            if let Some(client) = &ghost {
                let world = &scene.server.world;
                let waiting = (3..5).contains(&tick);
                assert_eq!(
                    world.master.hybrid.joining.contains(&client.id),
                    waiting,
                    "tick {tick}：是不是还在等进服"
                );
                assert_eq!(
                    world.master.ticket_centers.contains_key(&client.id),
                    waiting,
                    "tick {tick}：票在不在"
                );
            }
        }
        let ghost = ghost.expect("进过服");
        let world = &scene.server.world;
        assert!(world.islands.all_parked() && world.master.hybrid.held_len() == 0);
        assert!(world.islands.island_of_player(ghost.id).is_none());
        assert_eq!(count_marker(&frames, JOINED), 0, "没人收到过他的进服消息");
        assert_eq!(count_marker(&frames, LEFT), 0);
        scene.server.world.assert_island_invariants();
    });
}

/// 别的岛慢的时候有人进服、有人离开：健康岛上的人当场收到进服和离开消息，慢岛上的人在
/// 它回来的那一刻按同样的先后各收到一次。
///
/// Someone joins and someone leaves while another island is slow: players on
/// healthy islands get both messages at once, and the slow island's players
/// get each once, in the same order, the tick it returns.
#[test]
fn joins_and_leaves_reach_a_slow_island_later_in_order() {
    bounded(|| {
        let mut scene = scene(123, 4);
        for _ in 0..3 {
            scene.server.tick();
        }
        prepare(&mut scene);
        let victim = island_of_group(&mut scene, 1);
        scene.server.world.stall_island(victim, 2, 7);
        let leaver = first_of(&scene, 2);
        let watcher = first_of(&scene, 0);
        let slow = clients_of(&scene, 1);
        let mut healthy: Vec<Frame> = Vec::new();
        let mut late: Vec<Vec<Frame>> = vec![Vec::new(); slow.len()];
        for tick in 1..=10_u64 {
            if tick == 3 {
                let _ = scene.server.join("late");
            }
            if tick == 5 {
                scene.server.leave(&leaver);
            }
            scene.server.tick();
            healthy.extend(watcher.take_frames());
            for (index, client) in slow.iter().enumerate() {
                let got = client.take_frames();
                if (2..=7).contains(&tick) {
                    assert!(got.is_empty(), "tick {tick}：慢岛上的人还收不到");
                }
                late[index].extend(got);
            }
            match tick {
                3 | 4 => assert_eq!(
                    (count_marker(&healthy, JOINED), count_marker(&healthy, LEFT)),
                    (1, 0),
                    "tick {tick}：健康岛上的人当场收到进服消息"
                ),
                5..=7 => assert_eq!(
                    (count_marker(&healthy, JOINED), count_marker(&healthy, LEFT)),
                    (1, 1),
                    "tick {tick}：健康岛上的人当场收到离开消息"
                ),
                _ => {}
            }
        }
        for got in &late {
            assert_eq!(
                (count_marker(got, JOINED), count_marker(got, LEFT)),
                (1, 1),
                "慢岛上的人各收到一次"
            );
            assert_eq!(
                marker_order(got, &[JOINED, LEFT]),
                vec![JOINED.to_owned(), LEFT.to_owned()],
                "先进服、后离开"
            );
        }
        assert!(scene.server.world.islands.all_parked());
        scene.server.world.assert_island_invariants();
    });
}

/// 主控给全服的帧（天气、延迟广播走的 `broadcast_all`）：健康岛上的人当场收到，慢岛上的
/// 人在它回来的那一刻收到，各一次。
///
/// A master frame for everyone (`broadcast_all`, used by weather and the
/// latency update): players on healthy islands get it at once, the slow
/// island's players the tick it returns, once each.
#[test]
fn master_broadcasts_reach_a_slow_island_when_it_lands() {
    bounded(|| {
        let mut scene = scene(131, 4);
        for _ in 0..3 {
            scene.server.tick();
        }
        prepare(&mut scene);
        let victim = island_of_group(&mut scene, 1);
        scene.server.world.stall_island(victim, 1, 4);
        let frame = crate::links::encode_frames(&mc_bamboo_protocol::packet::play::SystemChat {
            content: &mc_bamboo_protocol::text::Component::text("MARK-ALL"),
            overlay: false,
        });
        let slow = clients_of(&scene, 1);
        let healthy = first_of(&scene, 0);
        let (mut early, mut late) = (Vec::new(), Vec::new());
        for tick in 1..=6_u64 {
            if tick == 2 {
                assert!(!scene.server.world.islands.is_parked(victim));
                scene.server.world.broadcast_all(&frame);
            }
            scene.server.tick();
            early.extend(healthy.take_frames());
            for client in &slow {
                late.extend(client.take_frames());
            }
            if tick == 2 {
                assert_eq!(count_marker(&early, "MARK-ALL"), 1, "健康岛当场收到");
            }
            if tick <= 4 {
                assert_eq!(count_marker(&late, "MARK-ALL"), 0, "慢岛还没回来");
            }
        }
        assert_eq!(count_marker(&early, "MARK-ALL"), 1);
        assert_eq!(
            count_marker(&late, "MARK-ALL"),
            slow.len(),
            "慢岛上每人一次"
        );
    });
}

/// 直接从区域文件读一格方块（不起世界）；区块没存过时是 `None`。
///
/// Read one block straight from the region file without starting a world;
/// `None` if the chunk was never saved.
pub(super) fn block_on_disk(root: &std::path::Path, pos: mc_bamboo_core::BlockPos) -> Option<u32> {
    use mc_bamboo_storage::{
        anvil::{RegionCompression, RegionPos, open_region},
        chunk_nbt,
        world_dir::WorldDir,
    };
    let chunk = pos.chunk();
    let mut region = open_region(
        WorldDir::new(root).region_dir(),
        RegionPos::of(chunk),
        RegionCompression::Zlib,
    )
    .ok()?;
    let bytes = region.read(chunk, 64 << 20).ok()??;
    let decoded = chunk_nbt::decode_bytes(&bytes).ok()?;
    Some(
        decoded
            .column
            .chunk()
            .block((pos.x & 15) as usize, pos.y, (pos.z & 15) as usize),
    )
}

/// 慢岛和存盘。三组各改一格方块，第 1 组的岛慢 8 刻。
///
/// - `save_all` 为假：匀速存盘不等慢岛。健康岛上改过的区块在慢岛不在的时候照常写盘，
///   慢岛上改过的在它回来以后写盘（留给它的存盘名单没丢）。
/// - `save_all` 为真：慢岛不在时发的 `/save-all` 等全部岛停放再存（每个岛上改过的区块
///   都要拍到）。慢岛回来之前成功消息不发、它的区块不在盘上；回来以后成功消息发一次，
///   这时盘上三组的改动都在。
///
/// A slow island and saving. Each group edits one block and group 1's island
/// is slow for 8 ticks. Without `save_all`, paced saves do not wait: chunks
/// edited on healthy islands are written while the slow one is away and the
/// slow island's chunk once it is back (the save list kept for it is not
/// lost). With `save_all`, a `/save-all` sent meanwhile waits for every
/// island: no success message and no chunk of the slow island on disk before
/// it is back, one success message afterwards with all three edits on disk.
fn run_saves(save_all: bool) {
    const SUCCESS: &str = "commands.save.success";
    let root = std::env::temp_dir().join(format!(
        "bamboo-island-hybrid-save-{}-{save_all}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    {
        let mut scene = set_up(141, Harness::new_islands_with_save(2, &root, 4));
        for _ in 0..3 {
            scene.server.tick();
        }
        let op = first_of(&scene, 0);
        make_op(&mut scene, &op);
        scene.server.world.master.host.flush_io();
        let _ = super::tests::written_chunks(&scene.server.world.master.host);
        let victim = island_of_group(&mut scene, 1);
        let centers = scene.centers.clone();
        let edits: Vec<mc_bamboo_core::BlockPos> = centers
            .iter()
            .map(|&center| super::island_split_tests::at(center, 4, -60, 4))
            .collect();
        for (&center, &pos) in centers.iter().zip(&edits) {
            let island = scene
                .server
                .world
                .islands
                .island_of_chunk(center)
                .expect("有岛");
            let region = &mut scene
                .server
                .world
                .islands
                .get_mut(island)
                .expect("在表里")
                .region;
            region
                .chunks
                .set_block(pos, mc_bamboo_data::blocks::GOLD_BLOCK);
            region.touch(pos);
            region.chunks.finish_light_updates();
        }
        scene.server.world.stall_island(victim, 1, 8);
        let gold = Some(mc_bamboo_data::blocks::GOLD_BLOCK);
        let mut written = Vec::new();
        let mut feedback = Vec::new();
        for tick in 1..=16_u64 {
            if save_all && tick == 3 {
                command(&scene, 0, "save-all");
            }
            scene.server.tick();
            scene.server.world.master.host.flush_io();
            if !save_all {
                // 取走写盘回执看写了哪些区块（会把栅栏回执也取走，所以只在不发
                // `/save-all` 的那一遍用）。
                // Take the write acks to see what was written (this also takes
                // barrier acks, so only in the run without `/save-all`).
                written.extend(super::tests::written_chunks(
                    &scene.server.world.master.host,
                ));
            }
            feedback.extend(op.take_frames());
            if tick == 8 {
                assert!(!scene.server.world.islands.is_parked(victim));
                if save_all {
                    assert_eq!(
                        count_marker(&feedback, SUCCESS),
                        0,
                        "`/save-all` 还在等慢岛"
                    );
                    assert!(scene.server.world.master.hybrid.held_len() >= 1);
                } else {
                    assert!(
                        written.contains(&centers[0]) && written.contains(&centers[2]),
                        "慢岛不在时健康岛上改过的区块照常写盘：{written:?}"
                    );
                    assert!(!written.contains(&centers[1]));
                }
                assert_ne!(
                    block_on_disk(&root, edits[1]),
                    gold,
                    "慢岛上的改动它不在时存不了"
                );
            }
        }
        if save_all {
            assert_eq!(
                count_marker(&feedback, SUCCESS),
                1,
                "慢岛回来以后 `/save-all` 做了一次"
            );
        } else {
            assert!(
                written.contains(&centers[1]),
                "慢岛回来以后它改过的区块写盘了：{written:?}"
            );
        }
        for (group, &pos) in edits.iter().enumerate() {
            assert_eq!(block_on_disk(&root, pos), gold, "第 {group} 组的改动在盘上");
        }
        assert_eq!(scene.server.world.master.hybrid.held_len(), 0);
        assert!(scene.server.world.master.hybrid.plans.is_empty());
        scene.server.shut_down();
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn paced_saves_do_not_wait_for_a_slow_island() {
    bounded(|| run_saves(false));
}

#[test]
fn save_all_waits_for_a_slow_island_and_then_saves_everything() {
    bounded(|| run_saves(true));
}

/// tick 线程比岛少（三个岛、两条线程）、一个岛卡死：健康的岛不能排到卡死的岛那条线程
/// 后面去。`ticks` 刻以后两个健康的岛的时钟各前进了几刻。`deadline` 给出时每刻带这个
/// 截止时间跑（卡死用岛自己的测试钩子）；不给时用 `stall_island`、主控等全部没卡的岛。
///
/// Fewer tick threads than islands (three islands, two threads) with one
/// island stuck: a healthy island must not queue behind the stuck island's
/// thread. Returns how far the two healthy islands' clocks advanced in
/// `ticks` ticks. With `deadline` every tick runs under that deadline (the
/// stall is the island's own test hook); without, `stall_island` is used and
/// the master waits for every island that is not stalled.
fn healthy_progress_beside_a_stuck_island(
    ticks: u64,
    deadline: Option<Duration>,
    no_shared_queue: bool,
) -> Vec<i64> {
    let mut scene = scene(151, 2);
    for _ in 0..3 {
        scene.server.tick();
    }
    scene
        .server
        .world
        .master
        .pool
        .as_mut()
        .expect("分离驱动")
        .no_shared_queue = no_shared_queue;
    let victim = island_of_group(&mut scene, 0);
    let healthy = [
        island_of_group(&mut scene, 1),
        island_of_group(&mut scene, 2),
    ];
    let release = Arc::new(AtomicBool::new(false));
    match deadline {
        Some(_) => {
            scene
                .server
                .world
                .islands
                .get_mut(victim)
                .expect("停放着")
                .test_hook = Some(IslandTestHook::Stall(Arc::clone(&release)));
        }
        None => scene.server.world.stall_island(victim, 1, ticks),
    }
    let clock = |scene: &Scene, id: RegionId| {
        scene
            .server
            .world
            .islands
            .parked()
            .find(|island| island.id == id)
            .map(|island| island.region.tick.game_time())
    };
    let before: Vec<i64> = healthy
        .iter()
        .map(|&id| clock(&scene, id).expect("停放着"))
        .collect();
    for _ in 0..ticks {
        match deadline {
            Some(window) => {
                assert!(tick_within(&mut scene.server, window).is_continue());
            }
            None => scene.server.tick(),
        }
    }
    // 带截止时间的那种：再给按时的岛一点时间回来（不派新的）。
    // With a deadline: give the on-time islands a moment to return.
    if deadline.is_some() {
        std::thread::sleep(Duration::from_millis(300));
        let _ = scene.server.world.begin_hybrid_tick();
    }
    let progress = healthy
        .iter()
        .zip(&before)
        .map(|(&id, start)| clock(&scene, id).map_or(-1, |now| now - start))
        .collect();
    release.store(true, std::sync::atomic::Ordering::Release);
    progress
}

/// 见 [`healthy_progress_beside_a_stuck_island`]：两条线程、三个岛、一个卡死，另外两个岛
/// 每刻都照常跑（线程都忙着时岛进公共队列，哪条线程先空出来哪条跑）。改坏确认：改回
/// 「排到手里岛最少的那条线程后面」，就有一个健康的岛排在卡死的岛后面再也回不来。
///
/// See [`healthy_progress_beside_a_stuck_island`]: two threads, three
/// islands, one stuck, and the other two run every tick (with every thread
/// busy an island joins the shared queue for whichever frees up first).
/// Break-it check: queueing behind the least loaded thread again traps a
/// healthy island behind the stuck one for good.
#[test]
fn a_healthy_island_never_queues_behind_a_stuck_one() {
    bounded(|| {
        let progress = healthy_progress_beside_a_stuck_island(8, None, false);
        assert_eq!(progress, vec![8, 8], "两个健康的岛每刻都跑了");
        let window = Some(Duration::from_millis(150));
        let progress = healthy_progress_beside_a_stuck_island(6, window, false);
        assert!(
            progress.iter().all(|&ticks| ticks >= 1),
            "带截止时间时两个健康的岛也都在跑：{progress:?}"
        );
        let broken = healthy_progress_beside_a_stuck_island(6, window, true);
        assert!(
            broken.contains(&-1),
            "改坏确认：有一个健康的岛排在卡死的岛后面回不来：{broken:?}"
        );
    });
}

/// 都按时的时候，混合驱动的结果和 tick 线程数无关：1 条和 8 条线程，带进服、离开和换岛
/// 的传送（这些和齐步比不了，分离驱动的交付晚一刻），每刻每组的内容、时钟和每名玩家
/// 收到的全部字节相同。
///
/// With every island on time the hybrid drive does not depend on the thread
/// count: 1 and 8 threads, with a join, a leave and island-changing teleports
/// (which cannot be compared with lockstep, as detached deliveries are a tick
/// later), give the same content, clocks and bytes every tick.
#[test]
fn on_time_hybrid_does_not_depend_on_the_thread_count() {
    bounded(|| {
        let script: Vec<(u64, Act)> = vec![
            (1, Act::Command(0, "gamemode creative @a")),
            (2, Act::Chat(1, "hello")),
            (3, Act::Join("late")),
            (5, Act::Command(2, "tellraw @a {\"text\":\"hi\"}")),
            (6, Act::Command(0, "tp {g0} {c1}")),
            (9, Act::Console("say from the console")),
            (12, Act::Command(1, "tp {g1} {c2}")),
            (15, Act::Leave(2)),
            (17, Act::Chat(0, "bye")),
            (20, Act::Command(0, "tp {g0} {c0}")),
        ];
        let (mut one, mut eight) =
            set_up_pair(43, Harness::new_islands(2, 1), Harness::new_islands(2, 8));
        assert_eq!(
            one.server.world.master.director_tick,
            eight.server.world.master.director_tick
        );
        prepare(&mut one);
        prepare(&mut eight);
        for tick in 0..30_u64 {
            for (_, step) in script.iter().filter(|(at, _)| *at == tick) {
                act(&mut one, step);
                act(&mut eight, step);
            }
            one.server.tick();
            eight.server.tick();
            let what = format!("tick {tick}");
            assert!(
                one.server.world.islands.all_parked() && eight.server.world.islands.all_parked()
            );
            assert_eq!(
                one.server.world.islands.ids(),
                eight.server.world.islands.ids(),
                "{what}：岛的编号"
            );
            for id in one.server.world.islands.ids() {
                for group in 0..one.centers.len() {
                    assert_same_print(
                        &group_print_on(&mut one, id, group),
                        &group_print_on(&mut eight, id, group),
                        &format!("{what}：岛 {id} 上第 {group} 组的内容"),
                    );
                }
            }
            for ((_, mine), (_, theirs)) in one.clients.iter().zip(&eight.clients) {
                assert_eq!(
                    mine.take_frames(),
                    theirs.take_frames(),
                    "{what}：玩家 {:?} 收到的字节",
                    mine.id
                );
            }
        }
    });
}

/// 混合驱动下的剖析没有盲点（第 10 步 d 段）：家岛落后的那几刻，主控这一段照样进报告
/// （原来记进一份不进报告的备用剖析器）；每个岛的每一刻都有一条耗时和一条交接（原来只记
/// 派出那一半、每刻一条）；打报告前各岛的样本并进来，每个岛一行。
///
/// Profiling has no blind spot under the hybrid drive (step 10 part d): the
/// master's part of the ticks in which the home island lags still reaches
/// the report (it used to go to an unreported spare profiler); every island
/// tick has one busy sample and one hand-over sample (only the dispatch half
/// was recorded before, once per tick); before a report every island's
/// samples are folded in, with one line per island.
#[test]
fn profiling_covers_every_master_tick_and_every_island_tick() {
    use crate::profiler::{Metric, Phase, Series};
    bounded(|| {
        let mut scene = scene(54, 4);
        for _ in 0..3 {
            scene.server.tick();
        }
        let world = &mut scene.server.world;
        let islands = world.islands.len() as u64;
        assert!(islands >= 3, "每组一个岛");
        world.master.profiler.reset_samples();
        for island in world.islands.iter_mut() {
            island.region.profiler.reset_samples();
        }
        world.stall_island(HOME_REGION, 1, 6);
        const TICKS: u64 = 10;
        for tick in 1..=TICKS {
            scene.server.tick();
            if tick <= 6 {
                assert!(!scene.server.world.islands.is_parked(HOME_REGION));
            }
        }
        let world = &mut scene.server.world;
        assert!(world.islands.all_parked());
        assert_eq!(world.islands.debt(HOME_REGION), 0, "家岛追上了");
        let count = |world: &crate::world::World, metric| {
            world
                .master
                .profiler
                .histogram(Series::Metric(metric))
                .count()
        };
        assert_eq!(
            count(world, Metric::TickTotal),
            TICKS,
            "主控的每一刻都进报告"
        );
        assert_eq!(count(world, Metric::MasterBusy), TICKS);
        assert_eq!(
            count(world, Metric::IslandHandoff),
            TICKS * islands,
            "每个岛的每一刻一条交接"
        );
        world.fold_island_profiles();
        assert_eq!(
            count(world, Metric::IslandBusy),
            TICKS * islands,
            "并进来的是每个岛的每一刻"
        );
        let report = world.master.profiler.report();
        for id in world.islands.ids() {
            assert!(
                report.contains(&format!("  island {id:<4} ")),
                "岛 {id} 有一行：{report}"
            );
        }
        for island in world.islands.iter() {
            assert_eq!(
                island
                    .region
                    .profiler
                    .histogram(Series::Phase(Phase::LightUpdates))
                    .count(),
                0,
                "并进来以后岛自己的清零"
            );
        }
        assert_eq!(
            world
                .master
                .profiler
                .histogram(Series::Phase(Phase::LightUpdates))
                .count(),
            TICKS * islands,
            "岛的各阶段并进来了"
        );
    });
}

/// 混合驱动下岛在一刻开头收了信、回执还没交回，同一刻收包阶段里（进服挂票）照做一条
/// 卷进它的拆分：拆分要等交回以后（这一刻收尾）再做，不能拆一个回执还没交出去的岛
/// （压测里 `islands-8` 进人时世界线程曾因此 panic：「交回还没处理完就拆岛」）。合并
/// 不会碰上：待合并的岛的信由 Director 扣着，不投。
///
/// Under the hybrid drive an island took mail at the start of a tick and
/// its acks are not handed back yet when, in the same tick's inbound phase
/// (a join placing a ticket), a split of it becomes due: the split must
/// wait for the hand-back at the end of the tick instead of splitting an
/// island whose acks are still out (the world thread once panicked on this
/// in the `islands-8` load test while bots joined). Merges cannot hit it:
/// the Director holds mail for islands about to merge.
#[test]
fn a_split_waits_for_acks_taken_at_the_start_of_the_tick() {
    bounded(|| {
        let mut scene = scene(54, 4);
        for _ in 0..3 {
            scene.server.tick();
        }
        // 先把第 0、1 组并成一个岛（两块相距很远，之后一拆就是两块）。
        // First merge groups 0 and 1 into one island (far apart, so a split
        // later yields two parts).
        let first = island_of_group(&mut scene, 0);
        let second = island_of_group(&mut scene, 1);
        let (into, from) = (first.min(second), first.max(second));
        let world = &mut scene.server.world;
        let partition = world.master.host.partition_mut().expect("多岛开着");
        assert!(partition.force_merge(into, from));
        world.master.host.take_partition_ops();
        world.settle_islands();
        assert!(!world.islands.contains(from), "并成了一个岛");
        scene.server.tick();
        // 这个岛每刻只交付一个区块，旁边挂几张强加载票：信下一刻开头收进信箱，交付一个，
        // 剩下的留在信箱里，之后每刻开头接着交付（回执在交回之前一直在岛上）。
        // This island delivers one chunk per tick, and a few forced tickets
        // go beside it: the letters land in its mailbox at the next tick
        // start, one is delivered and the rest wait there, delivered at later
        // tick starts (the acks stay on the island until the hand-back).
        scene
            .server
            .world
            .islands
            .get_mut(into)
            .expect("停放着")
            .mailbox
            .set_budget(crate::director_host::MailBudget {
                deliveries: 1,
                reclaims: 0,
            });
        let mut pos = scene.centers[1];
        while scene.server.world.islands.island_of_chunk(pos).is_some() {
            pos = ChunkPos::new(pos.x + 1, pos.z);
        }
        for dz in 0..4 {
            scene.server.apply_ticket_for_test(TicketOp::Forced {
                pos: ChunkPos::new(pos.x, pos.z + dz),
                forced: true,
            });
        }
        scene.server.tick();
        let world = &mut scene.server.world;
        assert!(
            world.islands.get(into).expect("停放着").mailbox.backlog().0 > 0,
            "信箱里还有没交付的盒子"
        );
        let partition = world.master.host.partition_mut().expect("多岛开着");
        let made = partition.force_split(into);
        assert_eq!(made.len(), 1, "拆成两块");
        world.master.host.take_partition_ops();
        // 进服在收包阶段挂票，触发一次照做拆合；这时要拆的岛刚收完信、回执还在手里。
        // The join places a ticket in the inbound phase and triggers a
        // replay of changes while the island to split holds fresh acks.
        let _late = scene.server.join("Latecomer");
        scene.server.tick();
        let world = &mut scene.server.world;
        assert!(world.islands.contains(made[0]), "这一刻收尾时拆开了");
        world.assert_island_invariants();
        for _ in 0..5 {
            scene.server.tick();
        }
        scene.server.world.assert_island_invariants();
    });
}

/// 同上，换成跨岛的玩家：原来的岛这一刻开头收了信、回执还没交回时，收包阶段触发的照做不搬
/// 他（搬人要从原来的岛拆出一块），留到这一刻收尾交回以后。长时间压测（`islands-roam`）里
/// 世界线程曾因此 panic：「交回还没处理完就拆岛」。改坏确认：去掉搬人前的回执检查，这条
/// 测试就在照做那一步 panic。
///
/// As above for a crossing player: while the old island holds acks from
/// mail taken at the start of the tick, a settle triggered in the inbound
/// phase does not move them (moving splits a piece off the old island); the
/// move waits for the hand-back at the end of the tick. The long run
/// (`islands-roam`) once panicked on this: "split before the hand-back".
#[test]
fn a_crossing_player_waits_for_acks_taken_at_the_start_of_the_tick() {
    bounded(|| {
        let mut scene = scene(55, 4);
        // 跑够刻数，后面才能判「等太久」（跨岛等原来的岛清干净最多等 40 刻）。
        // Run enough ticks to judge "waited too long" later (a crossing
        // waits at most 40 ticks for the old island to clear).
        while scene.server.world.master.director_tick <= 50 {
            scene.server.tick();
        }
        let from = island_of_group(&mut scene, 1);
        let to = island_of_group(&mut scene, 0);
        assert_ne!(from, to);
        // 原来的岛每刻只交付一个区块，旁边挂几张强加载票：之后每刻开头都从信箱里交付一个，
        // 回执在交回之前一直在岛上。
        // The old island delivers one chunk per tick and gets a few forced
        // tickets beside it: every later tick start delivers one from its
        // mailbox, and the acks stay on the island until the hand-back.
        scene
            .server
            .world
            .islands
            .get_mut(from)
            .expect("停放着")
            .mailbox
            .set_budget(crate::director_host::MailBudget {
                deliveries: 1,
                reclaims: 0,
            });
        let mut pos = scene.centers[1];
        while scene.server.world.islands.island_of_chunk(pos).is_some() {
            pos = ChunkPos::new(pos.x + 1, pos.z);
        }
        for dz in 0..4 {
            scene.server.apply_ticket_for_test(TicketOp::Forced {
                pos: ChunkPos::new(pos.x, pos.z + dz),
                forced: true,
            });
        }
        scene.server.tick();
        assert!(
            scene
                .server
                .world
                .islands
                .get(from)
                .expect("停放着")
                .mailbox
                .backlog()
                .0
                > 0,
            "信箱里还有没交付的盒子"
        );
        // 第 1 组的一名玩家站到第 0 组的区块上，并且已经等够了久（不再等原来的岛清干净）。
        // A group 1 player stands on a group 0 chunk and has waited long
        // enough (no more waiting for the old island to clear them).
        let client = scene
            .clients
            .iter()
            .find(|(group, _)| *group == 1)
            .map(|(_, client)| client.clone())
            .expect("第 1 组有人");
        let target = scene.centers[0];
        scene.server.move_to_position(
            &client,
            Vec3::new(
                f64::from(target.x * 16) + 8.5,
                -60.0,
                f64::from(target.z * 16) + 8.5,
            ),
        );
        let world = &mut scene.server.world;
        world.master.transit.insert(client.id, 0);
        // 这一刻开头：信箱重置预算，岛收信，回执留在岛上；收包阶段（这里直接调）照做拆合和
        // 搬人。
        // The tick starts: mailboxes reset their budgets, islands take mail
        // and keep the acks; the inbound phase (called directly here)
        // settles changes and crossings.
        for island in world.islands.parked_mut() {
            island.mailbox.begin_tick();
        }
        world.start_island_ticks();
        assert!(!world.islands.acks_handed_back(from), "回执还在岛上");
        world.settle_islands();
        assert_eq!(
            world.islands.island_of_player(client.id),
            Some(from),
            "回执交回之前不搬"
        );
        // 等着搬的人记在跨岛途中，不变量照样成立（以前等回执时没记，不变量检查会把他当成
        // 「站在别的岛的区块上却不在那个岛」）。
        // The waiting player is recorded as crossing, so the invariants
        // hold (without the record the check took them for a player on
        // another island's chunk).
        assert!(world.master.transit.contains_key(&client.id));
        world.assert_island_invariants();
        // 这一刻照常跑完：收尾交回以后搬过去。
        // The tick runs on: after the hand-back at its end the player moves.
        scene.server.tick();
        let world = &mut scene.server.world;
        assert_eq!(
            world.islands.island_of_player(client.id),
            Some(to),
            "收尾时搬过去了"
        );
        world.assert_island_invariants();
        for _ in 0..5 {
            scene.server.tick();
        }
        scene.server.world.assert_island_invariants();
    });
}
