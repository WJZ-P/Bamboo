//! 多岛的跨岛部分（岛的实施计划第 6 节第 10 步 b 段、4.1、4.4）：主控借玩家执行命令和
//! 聊天、跨岛传送的离境和到达、进出服。齐步驱动下，凡是不涉及「玩家换岛」的事都和整个
//! 世界一个岛时逐位相同；换岛的传送晚一刻（计划 4.3），单独核对不变量。
//!
//! Cross-island parts of multi-island mode (island plan, step 10 part b, 4.1,
//! 4.4): the master borrowing players for commands and chat, departures and
//! arrivals for cross-island teleports, joins and leaves. Under the lockstep
//! drive everything that does not move a player between islands matches the
//! single-island world bit for bit; a teleport that changes islands lands a
//! tick later (plan 4.3) and is checked through invariants.

use mc_bamboo_protocol::packet::play::PlayRequest;
use mc_bamboo_world::director::RegionId;

use super::{
    island_split_tests::{Scene, Y, at, region_of_player, set_up, with_stack},
    tests::{Client, Harness},
};

/// 名单里这名玩家的名字。
fn name_of(scene: &mut Scene, client: &Client) -> String {
    region_of_player(&mut scene.server.world, client.id).players[&client.id]
        .name
        .clone()
}

/// 让这名玩家成为 OP（权限等级 4）。
fn make_op(scene: &mut Scene, client: &Client) {
    region_of_player(&mut scene.server.world, client.id)
        .players
        .get_mut(&client.id)
        .expect("在世界里")
        .permission_level = 4;
}

fn island_of(scene: &Scene, client: &Client) -> RegionId {
    scene
        .server
        .world
        .islands
        .island_of_player(client.id)
        .expect("在某个岛上")
}

/// 第 `group` 组的第一名玩家。
fn first_of(scene: &Scene, group: usize) -> Client {
    scene
        .clients
        .iter()
        .find(|(index, _)| *index == group)
        .map(|(_, client)| client.clone())
        .expect("每组至少一名玩家")
}

/// 回归：主控借玩家执行 `/tp` 时，被传送的玩家在发起人岛的玩家网格里留下的登记要跟着
/// 还回去。原来只借玩家表、出站链路、对端 IP 和最后操作时刻，网格条目写进了发起人的岛，
/// 还回去以后那里多一个幽灵，玩家自己岛上的网格还是旧位置。
///
/// Regression: when the master borrows players for `/tp`, the teleported
/// player's grid entry must go back with them. The borrow used to cover only
/// the player table, link, peer IP and last action, so the entry landed in
/// the sender's island grid (a ghost after the return) while the player's
/// own island kept the old cell.
#[test]
fn a_teleport_command_leaves_no_ghost_in_the_senders_player_grid() {
    with_stack(|| {
        let mut scene = set_up(21, Harness::new_islands(2, 0));
        let sender = first_of(&scene, 0);
        let target = first_of(&scene, 1);
        make_op(&mut scene, &sender);
        let (here, there) = (island_of(&scene, &sender), island_of(&scene, &target));
        assert_ne!(here, there, "两人在不同的岛上");
        // 传送到目标自己那一组里的另一点：他不换岛。
        // Somewhere else in the target's own group: no island change.
        let to = at(scene.centers[1], 12, Y + 1, 12);
        let name = name_of(&mut scene, &target);
        scene.server.request(
            &sender,
            PlayRequest::unsigned_command(format!("tp {name} {} {} {}", to.x, to.y, to.z)),
        );
        scene.server.tick();
        let world = &scene.server.world;
        let grid_of = |id: RegionId| {
            world
                .islands
                .get(id)
                .expect("在表里")
                .region
                .player_grid
                .placed_players()
        };
        assert!(
            !grid_of(here).contains(&target.id),
            "发起人的岛的网格里没有被传送的人"
        );
        assert!(grid_of(there).contains(&target.id));
        let region = &world.islands.get(there).expect("在表里").region;
        let position = region.players[&target.id].position();
        assert_eq!(
            (position.x, position.z),
            (f64::from(to.x) + 0.5, f64::from(to.z) + 0.5),
            "整数坐标落在格子中心"
        );
        let near: Vec<_> = region
            .player_grid
            .near(position.x, position.z, 1.0)
            .collect();
        assert!(near.contains(&target.id), "网格里是新位置");
        scene.server.world.assert_island_invariants();
    });
}

/// 比较窗口里的一步：谁在第几刻做什么。
enum Act {
    /// 第几组的第一名玩家发一条命令（不带斜杠）。
    Command(usize, &'static str),
    /// 第几组的第一名玩家发一条聊天。
    Chat(usize, &'static str),
    /// 控制台命令。
    Console(&'static str),
    /// 新玩家进服（出生点在第 0 组）。
    Join(&'static str),
    /// 第几组的最后一名玩家离开。
    Leave(usize),
}

/// 做一步。`{gN}` 换成第 N 组第一名玩家的名字，`{cN}` 换成第 N 组中心旁边一格的坐标。
fn act(scene: &mut Scene, step: &Act) {
    let expand = |scene: &mut Scene, text: &str| {
        let mut out = text.to_owned();
        for group in 0..scene.centers.len() {
            let client = first_of(scene, group);
            let name = name_of(scene, &client);
            out = out.replace(&format!("{{g{group}}}"), &name);
            let spot = at(scene.centers[group], 11, Y + 1, 5);
            out = out.replace(
                &format!("{{c{group}}}"),
                &format!("{} {} {}", spot.x, spot.y, spot.z),
            );
        }
        out
    };
    match *step {
        Act::Command(group, text) => {
            let client = first_of(scene, group);
            let line = expand(scene, text);
            scene
                .server
                .request(&client, PlayRequest::unsigned_command(line));
        }
        Act::Chat(group, text) => {
            let client = first_of(scene, group);
            scene
                .server
                .request(&client, PlayRequest::unsigned_chat(text));
        }
        Act::Console(text) => {
            let line = expand(scene, text);
            scene
                .server
                .inbox_sender()
                .send(crate::inbound::Inbound::ConsoleCommand(line))
                .expect("世界还在");
        }
        Act::Join(name) => {
            let client = scene.server.join(name);
            scene.clients.push((0, client));
        }
        Act::Leave(group) => {
            let client = scene
                .clients
                .iter()
                .rev()
                .find(|(index, _)| *index == group)
                .map(|(_, client)| client.clone())
                .expect("这一组有人");
            scene.server.leave(&client);
        }
    }
}

/// 不换岛的跨岛命令、聊天、控制台命令、进出服：齐步驱动下多岛和整个世界一个岛逐位相同
/// （每刻比每组的内容、岛的时钟、每名玩家收到的全部字节；窗口里不取岛的随机数、不生成
/// 新实体）。主控借玩家执行，选择器 `@a`、玩家名、广播、给 OP 的反馈、Tab 名单都和一个岛
/// 时一样，跨岛也不晚一刻。
///
/// Cross-island commands that move nobody between islands, chat, console
/// commands, joins and leaves: under the lockstep drive the multi-island world
/// matches the single-island one bit for bit (per-group content, island
/// clocks and every player's bytes each tick; no island random draws and no
/// new entities in the window). The master borrows players, so `@a`, names,
/// broadcasts, op feedback and the tab list behave as on one island, with no
/// extra tick across islands.
#[test]
fn cross_island_commands_and_chat_match_one_island() {
    with_stack(|| {
        for seed in [31_u64, 32] {
            run_cross_island(seed, false);
        }
    });
}

/// 改坏确认：借玩家时一个都不借，上面的对照要报出来（`@a` 只到得了本岛的人）。
///
/// Break-it check: borrowing nobody must fail the comparison above (`@a`
/// only reaches the sender's island).
#[test]
fn borrowing_nobody_is_caught() {
    with_stack(|| {
        let caught = std::panic::catch_unwind(|| run_cross_island(31, true));
        assert!(caught.is_err(), "不借玩家没有被抓到");
    });
}

/// [`cross_island_commands_and_chat_match_one_island`] 的一次运行；`lend_nothing` 为真时
/// 多岛那份借玩家时一个都不借。
fn run_cross_island(seed: u64, lend_nothing: bool) {
    use super::island_runtime_tests::{
        align, assert_in_place, assert_no_draws, print_control, print_islands, random_states,
    };
    let script: Vec<(u64, Act)> = vec![
        (1, Act::Command(0, "gamemode creative @a")),
        (2, Act::Command(1, "give @a minecraft:stone 3")),
        (3, Act::Command(2, "tellraw @a {\"text\":\"hi\"}")),
        (4, Act::Chat(2, "hello everyone")),
        (5, Act::Console("say from the console")),
        (6, Act::Command(0, "tp {g1} {c1}")),
        (7, Act::Command(1, "msg {g2} psst")),
        (8, Act::Command(2, "gamemode survival {g0}")),
        (9, Act::Command(0, "time set 1000")),
        (10, Act::Console("give {g2} minecraft:dirt 2")),
        (11, Act::Join("late")),
        (13, Act::Command(1, "clear @a minecraft:stone")),
        (15, Act::Leave(2)),
        (16, Act::Chat(0, "bye")),
        (18, Act::Command(0, "list")),
        // 按名随机序列全世界一份（在主控上）：两个岛上先后取同一条序列，接着往下走。
        // One named sequence per world (on the master): two islands draw from it in turn.
        (
            19,
            Act::Command(1, "random value 1..1000000 minecraft:shared"),
        ),
        (
            20,
            Act::Command(0, "random value 1..1000000 minecraft:shared"),
        ),
        (
            21,
            Act::Command(1, "loot give @a loot minecraft:blocks/diamond_ore"),
        ),
    ];
    let mut multi = set_up(seed, Harness::new_islands(2, 0));
    let mut control = set_up(
        seed,
        Harness::new_loaded_with_drive(2, crate::island::IslandDrive::Lockstep),
    );
    align(&mut multi, &mut control);
    multi.server.world.islands.lend_nothing = lend_nothing;
    // 对照是精确模式：全局改动当场应用，不看 `BAMBOO_TEST_GLOBAL_PORT`（多岛时主控借玩家
    // 当场执行，本来就不排队）。
    // The control is exact mode: global changes apply at once regardless of
    // `BAMBOO_TEST_GLOBAL_PORT` (multi-island borrowing runs at once anyway).
    control.server.world.master.port_mode = crate::global_port::PortMode::Direct;
    assert_eq!(multi.server.world.islands.len(), multi.centers.len());
    for scene in [&mut multi, &mut control] {
        for group in 0..scene.centers.len() {
            let client = first_of(scene, group);
            make_op(scene, &client);
        }
        // 聊天包带服务端的墙上时钟毫秒，两个世界跑的时刻不同：固定住。
        // Chat carries the server's wall-clock millis; pin it for both worlds.
        for island in scene.server.world.islands.iter_mut() {
            island.region.clock_millis = Some(1_700_000_000_000);
        }
    }
    for tick in 0..26_u64 {
        for (_, step) in script.iter().filter(|(at, _)| *at == tick) {
            act(&mut multi, step);
            act(&mut control, step);
        }
        let draws = (random_states(&multi), random_states(&control));
        multi.server.tick();
        control.server.tick();
        let what = format!("seed {seed} tick {tick}");
        assert_no_draws(&draws.0, &random_states(&multi), &what);
        assert_no_draws(&draws.1, &random_states(&control), &what);
        assert_in_place(&multi, &what);
        let (groups, clocks) = print_islands(&mut multi);
        let expected = print_control(&mut control);
        for (group, (left, right)) in groups.iter().zip(&expected).enumerate() {
            assert_eq!(left, right, "{what}：第 {group} 组的内容");
        }
        let clock = control.server.world.islands.region.tick.game_time();
        assert_eq!(clocks, std::collections::BTreeSet::from([clock]), "{what}");
        for ((_, mine), (_, theirs)) in multi.clients.iter().zip(&control.clients) {
            assert_eq!(
                mine.take_frames(),
                theirs.take_frames(),
                "{what}：玩家 {:?} 收到的字节",
                mine.id
            );
        }
        multi.server.world.assert_island_invariants();
    }
    assert!(
        multi.server.world.islands.several(),
        "seed {seed}：一直是多个岛"
    );
}

/// 借玩家的入口可以只借某几名玩家所在的岛（[`crate::island::Borrow::PlayersOf`]）：别的岛上的
/// 玩家不动，还回去以后每个人回到自己的岛，七类表一样不少。
///
/// The borrowing entry can borrow only the islands of some players
/// ([`crate::island::Borrow::PlayersOf`]): other islands' players stay put, and
/// after the return everyone is back home with all seven tables.
#[test]
fn borrowing_some_players_leaves_the_other_islands_alone() {
    use crate::island::Borrow;
    with_stack(|| {
        let mut scene = set_up(22, Harness::new_islands(2, 0));
        let (a, b, c) = (
            first_of(&scene, 0),
            first_of(&scene, 1),
            first_of(&scene, 2),
        );
        let (home, there, third) = (
            island_of(&scene, &a),
            island_of(&scene, &b),
            island_of(&scene, &c),
        );
        let islands = &mut scene.server.world.islands;
        let gathered = islands.gather(home, &Borrow::PlayersOf(vec![b.id]));
        let here = &islands.get(home).expect("在表里").region;
        assert!(here.players.contains_key(&b.id), "借来了 b 所在的岛的人");
        assert!(here.player_grid.placed_players().contains(&b.id));
        assert!(!here.players.contains_key(&c.id), "别的岛的人没借");
        assert!(
            islands
                .get(there)
                .expect("在表里")
                .region
                .players
                .is_empty()
        );
        assert!(
            islands
                .get(third)
                .expect("在表里")
                .region
                .players
                .contains_key(&c.id)
        );
        islands.scatter(home, gathered);
        for (client, id) in [(&a, home), (&b, there), (&c, third)] {
            let region = &islands.get(id).expect("在表里").region;
            assert!(region.players.contains_key(&client.id));
            assert!(region.player_grid.placed_players().contains(&client.id));
            assert!(region.links.is_open(client.id));
        }
        scene.server.world.assert_island_invariants();
    });
}

/// 跑几刻，每刻断言不变量、每名在线玩家恰好在一个岛上。
fn run_checked(scene: &mut Scene, ticks: usize, online: &[&Client]) {
    for _ in 0..ticks {
        scene.server.tick();
        let world = &mut scene.server.world;
        world.assert_island_invariants();
        for client in online {
            let holders = world
                .islands
                .iter()
                .filter(|island| island.region.players.contains_key(&client.id))
                .count();
            assert_eq!(holders, 1, "玩家 {:?} 恰好在一个岛上", client.id);
        }
        for (_, client) in &scene.clients {
            for _ in 0..super::tests::frames_with_id(
                &client.take_frames(),
                mc_bamboo_data::packets::play::clientbound::CHUNK_BATCH_FINISHED,
            )
            .len()
            {
                scene.server.acknowledge_batch(client, 64.0);
            }
        }
    }
}

/// 换岛的 `/tp`（计划 4.1「离境和到达」）：传送自己到别的岛的玩家身边、把别人传送到另一组、
/// 把人传送到谁都没加载的远处（分区器新建一个岛）。玩家在原来的岛上等它把视野和配对清
/// 干净（通常就是下一刻），再整个搬到目的地的岛；全程恰好在一个岛上，到了以后收得到那里
/// 的区块、看得见那里的人。比一个岛时晚一刻（计划 4.3），所以不逐字节比。
///
/// Island-changing `/tp` (plan 4.1): to a player on another island, another
/// player into a different group, and someone to a spot nobody has loaded (the
/// partitioner makes a new island). The player waits on the old island until
/// it has cleared their view and pairings (normally the next tick) and then
/// moves whole; they are on exactly one island throughout and afterwards get
/// the chunks and see the players there. It lands a tick later than on one
/// island (plan 4.3), so bytes are not compared.
#[test]
fn teleport_commands_move_players_between_islands() {
    with_stack(|| {
        let mut scene = set_up(23, Harness::new_islands(2, 0));
        let (a, b, c) = (
            first_of(&scene, 0),
            first_of(&scene, 1),
            first_of(&scene, 2),
        );
        make_op(&mut scene, &a);
        let everyone = [&a, &b, &c];
        // a 传送到 b 身边。
        // a goes to b.
        let b_name = name_of(&mut scene, &b);
        scene
            .server
            .request(&a, PlayRequest::unsigned_command(format!("tp {b_name}")));
        run_checked(&mut scene, 3, &everyone);
        assert_eq!(
            island_of(&scene, &a),
            island_of(&scene, &b),
            "a 到了 b 的岛"
        );
        run_checked(&mut scene, 4, &everyone);
        let world = &scene.server.world;
        let region = &world
            .islands
            .get(island_of(&scene, &b))
            .expect("在表里")
            .region;
        assert!(
            region.players[&a.id].sync.seen_by().contains(&b.id),
            "b 看得见 a"
        );
        assert!(
            region.players[&a.id]
                .chunks
                .is_chunk_tracked(scene.centers[1]),
            "a 跟踪着那一组的中心区块"
        );
        // a 把 c 传送到第 0 组（那里现在没人，岛还在：区块还没收回）。
        // a sends c to group 0.
        let c_name = name_of(&mut scene, &c);
        let spot = at(scene.centers[0], 6, Y + 1, 6);
        scene.server.request(
            &a,
            PlayRequest::unsigned_command(format!("tp {c_name} {} {} {}", spot.x, spot.y, spot.z)),
        );
        run_checked(&mut scene, 6, &everyone);
        let home_of_group = scene
            .server
            .world
            .islands
            .island_of_chunk(scene.centers[0])
            .expect("第 0 组还加载着");
        assert_eq!(island_of(&scene, &c), home_of_group, "c 到了第 0 组的岛");
        // a 把 b 传送到谁都没加载的远处：新建一个岛，b 到了那里，区块交付过去。
        // a sends b far away: a new island, b moves there, chunks follow.
        let before: std::collections::BTreeSet<RegionId> =
            scene.server.world.islands.ids().into_iter().collect();
        scene.server.request(
            &a,
            PlayRequest::unsigned_command(format!("tp {b_name} 4000 -59 4000")),
        );
        run_checked(&mut scene, 12, &everyone);
        let far = mc_bamboo_core::ChunkPos::new(4000 >> 4, 4000 >> 4);
        let island = island_of(&scene, &b);
        assert!(!before.contains(&island), "b 在一个新岛上");
        assert_eq!(
            scene.server.world.islands.island_of_chunk(far),
            Some(island),
            "b 脚下的区块交付到了 b 的岛"
        );
    });
}

/// 跨岛传送途中掉线（计划第 6 节第 10 步「跨岛传送途中玩家掉线」）：命令和离开在同一刻、
/// 离开在等原来的岛清理的那一刻、离开在刚搬到新岛的那一刻。三种都不留人、不留票、不留
/// 在途记录，不变量一直成立。
///
/// Disconnecting during a cross-island teleport: in the same tick as the
/// command, while waiting for the old island to clear, and right after the
/// move. None leaves the player, the ticket or the transit record behind.
#[test]
fn leaving_during_a_cross_island_teleport_leaves_nothing_behind() {
    with_stack(|| {
        for wait in [0_usize, 1, 2, 3] {
            let mut scene = set_up(24, Harness::new_islands(2, 0));
            let (a, b) = (first_of(&scene, 0), first_of(&scene, 1));
            make_op(&mut scene, &a);
            let b_name = name_of(&mut scene, &b);
            scene
                .server
                .request(&a, PlayRequest::unsigned_command(format!("tp {b_name}")));
            run_checked(&mut scene, wait, &[&b]);
            scene.server.leave(&a);
            run_checked(&mut scene, 3, &[&b]);
            let world = &mut scene.server.world;
            assert!(
                world.islands.island_of_player(a.id).is_none(),
                "wait {wait}：哪个岛都没有 a"
            );
            assert!(!world.master.transit.contains_key(&a.id), "wait {wait}");
            assert!(
                !world.master.ticket_centers.contains_key(&a.id),
                "wait {wait}：a 的票撤了"
            );
            let region = &world
                .islands
                .get(world.islands.island_of_player(b.id).expect("b 还在"))
                .expect("在表里")
                .region;
            assert!(
                !region.players[&b.id].sync.seen_by().contains(&a.id),
                "wait {wait}：b 不再记着 a"
            );
            world.assert_island_invariants();
        }
    });
}

/// 命令把实体放到别的岛的已加载区块上（`/loot spawn` 到另一组）：执行完马上搬到那个岛，
/// 发起人的岛上不留。那一组唯一的玩家在同一刻、命令之前或之后离开，也一样（那个岛的区块
/// 还没收回，实体跟着区块走）。
///
/// A command puts an entity on another island's loaded chunk (`/loot spawn`
/// into another group): it moves to that island right away, nothing stays on
/// the sender's island. The same holds when that group's only player leaves in
/// the same tick, before or after the command (the chunks are not reclaimed
/// yet and the entity goes with them).
#[test]
fn entities_a_command_puts_on_another_island_move_there() {
    use mc_bamboo_blocks::entity::KindPublic;
    with_stack(|| {
        for leave in [None, Some(true), Some(false)] {
            let mut scene = set_up(25, Harness::new_islands(2, 0));
            let (a, c) = (first_of(&scene, 0), first_of(&scene, 2));
            make_op(&mut scene, &a);
            let spot = at(scene.centers[2], 3, Y + 1, 9);
            let target = island_of(&scene, &c);
            let sender = island_of(&scene, &a);
            let alone = scene
                .clients
                .iter()
                .filter(|(group, _)| *group == 2)
                .count()
                == 1;
            let command = PlayRequest::unsigned_command(format!(
                "loot spawn {} {} {} loot minecraft:blocks/diamond_ore",
                spot.x, spot.y, spot.z
            ));
            let items = |scene: &Scene, id: RegionId| {
                scene.server.world.islands.get(id).map_or(0, |island| {
                    island
                        .region
                        .entities
                        .iter_public()
                        .filter(|public| matches!(public.kind, KindPublic::Item { .. }))
                        .count()
                })
            };
            let before = items(&scene, target);
            if leave == Some(true) {
                scene.server.leave(&c);
            }
            scene.server.request(&a, command);
            if leave == Some(false) {
                scene.server.leave(&c);
            }
            scene.server.tick();
            let world = &mut scene.server.world;
            world.assert_island_invariants();
            assert_eq!(items(&scene, sender), 0, "{leave:?}：发起人的岛上没有留下");
            assert_eq!(
                items(&scene, target),
                before + 1,
                "{leave:?}：物品到了第 2 组的岛（独自一人：{alone}）"
            );
        }
    });
}
