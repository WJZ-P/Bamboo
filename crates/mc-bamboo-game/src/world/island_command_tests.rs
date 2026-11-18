//! 命令作用到别的岛（岛的实施计划 4.5）：方块坐标、容器、命令生成的实体和读方块的玩家
//! 命令。齐步驱动下，多岛世界和整个世界一个岛逐刻比每组的内容指纹、岛的时钟和每名玩家
//! 收到的全部字节；目标全在别的岛、源和目标在不同的岛、范围跨两个岛的情况都有。
//!
//! Bamboo 现在没有 `/execute`、`/kill`、`/data`，选择器 `@e` 只返回玩家（单岛也一样），
//! 所以这里覆盖的是现有命令里碰方块和非玩家实体的那些：`/setblock`、`/fill`、`/clone`、
//! `/loot spawn|insert|replace block`、`/summon`、`/gamemode` / `/defaultgamemode`（读目标
//! 玩家脚下的方块），玩家发起和控制台发起各有。
//!
//! Commands reaching other islands (island plan 4.5): block positions,
//! containers, entities spawned by commands and player commands that read
//! blocks. Under the lockstep drive a multi-island world is compared with a
//! single-island one tick by tick: per-group content prints, island clocks
//! and every byte each player receives, with targets entirely on another
//! island, source and destination on different islands and a range spanning
//! two islands. Bamboo has no `/execute`, `/kill` or `/data` yet and `@e`
//! returns players only (on one island too), so this covers the existing
//! commands that touch blocks and non-player entities, from players and from
//! the console.

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_data::blocks;
use mc_bamboo_protocol::packet::play::PlayRequest;
use mc_bamboo_world::director::RegionId;

use super::{
    island_crossing_tests::{first_of, make_op, name_of},
    island_runtime_tests::{align, assert_in_place, print_control, print_islands, random_states},
    island_split_tests::{Scene, Y, at, centers_for, set_up, with_stack},
    tests::Harness,
};
use crate::island::IslandDrive;

/// 比较窗口里的一步。
enum Step {
    /// 第几组的第一名玩家发一条命令（不带斜杠）。
    Player(usize, String),
    /// 控制台命令。
    Console(String),
}

/// 第 `group` 组中心区块里的一格，写成命令里的坐标。
fn spot(centers: &[ChunkPos], group: usize, x: i32, dy: i32, z: i32) -> String {
    let pos = at(centers[group], x, Y + dy, z);
    format!("{} {} {}", pos.x, pos.y, pos.z)
}

/// 两组的中心在同一条轴线上（x 或者 z 相同）：一条一格粗的 `/fill` 可以从一组拉到另一组
/// 而不超过体积上限。返回这两组。
///
/// Two groups whose centres share an axis (same x or same z), so a one-block
/// thick `/fill` can run from one to the other within the volume limit.
fn aligned_pair(centers: &[ChunkPos]) -> Option<(usize, usize)> {
    for a in 0..centers.len() {
        for b in a + 1..centers.len() {
            if centers[a].x == centers[b].x || centers[a].z == centers[b].z {
                return Some((a, b));
            }
        }
    }
    None
}

/// 有一对组在同一条轴线上的种子（从 `from` 起找）。
fn seed_with_aligned_groups(from: u64) -> u64 {
    (from..from + 200)
        .find(|&seed| aligned_pair(&centers_for(seed)).is_some())
        .expect("两百个种子里总有一对组在同一条轴线上")
}

/// 比较窗口的脚本。取实体种子的命令（`/loot spawn`、`/summon`）都由第 0 组的玩家发：
/// 多岛时取的是发起人岛的种子源，只有一个发起人时它和单岛那一份走得一样。
///
/// The script. Commands that draw entity seeds (`/loot spawn`, `/summon`) all
/// come from group 0's player: with several islands the sender island's seed
/// source is used, and with a single sender it advances like the single
/// island's.
fn script(scene: &mut Scene) -> Vec<(u64, Step)> {
    let centers = scene.centers.clone();
    let c = |group, x, dy, z| spot(&centers, group, x, dy, z);
    let name = |scene: &mut Scene, group| {
        let client = first_of(scene, group);
        name_of(scene, &client)
    };
    let (g1, g2) = (name(scene, 1), name(scene, 2));
    let (a, b) = aligned_pair(&centers).expect("种子挑过了");
    let table = "minecraft:blocks/diamond_ore";
    let mut out = vec![
        // 目标全在别的岛：玩家发起、控制台发起。
        (
            1,
            Step::Player(0, format!("setblock {} minecraft:stone", c(1, 3, 1, 3))),
        ),
        // 控制台发的方块命令在 Bamboo 里现在报 `permissions.requires.player`（控制台来源
        // 没有位置，和岛无关）：两边要报同样的错。
        // Block commands from the console currently fail with
        // `permissions.requires.player` in Bamboo (the console source has no
        // position, islands or not); both worlds must report the same.
        (
            2,
            Step::Console(format!("setblock {} minecraft:stone", c(2, 8, 1, 3))),
        ),
        (
            2,
            Step::Player(0, format!("setblock {} minecraft:glowstone", c(2, 3, 1, 3))),
        ),
        // 同一刻里别的岛上的玩家把刚放的光源拆掉：两条命令的光照任务在同一轮里算。
        (
            2,
            Step::Player(1, format!("setblock {} minecraft:air", c(2, 3, 1, 3))),
        ),
        (
            3,
            Step::Player(0, format!("setblock {} minecraft:chest", c(1, 4, 1, 3))),
        ),
        // 别的岛上的容器。
        (
            4,
            Step::Player(2, format!("loot insert {} loot {table}", c(1, 4, 1, 3))),
        ),
        // 源在岛 1、目标在岛 2、发起人在岛 0。
        (
            5,
            Step::Player(
                0,
                format!(
                    "clone {} {} {}",
                    c(1, 3, 1, 3),
                    c(1, 4, 1, 3),
                    c(2, 6, 1, 6)
                ),
            ),
        ),
        // 岛 1 上的玩家把它从岛 2 搬到岛 0（带箱子里的东西）：三个岛各不相同。
        (
            6,
            Step::Player(
                1,
                format!(
                    "clone {} {} {} replace move",
                    c(2, 6, 1, 6),
                    c(2, 7, 1, 6),
                    c(0, 2, 3, 2)
                ),
            ),
        ),
        // 范围跨两个岛：两头各在一个岛上，中间是没加载的区块。
        (
            7,
            Step::Player(
                1,
                format!("fill {} {} minecraft:glass", c(a, 5, 4, 5), c(b, 5, 4, 5)),
            ),
        ),
        // 掉落物和生物生成在别的岛。
        (
            8,
            Step::Player(0, format!("loot spawn {} loot {table}", c(2, 12, 1, 12))),
        ),
        (
            9,
            Step::Player(0, format!("summon minecraft:cow {}", c(1, 12, 1, 12))),
        ),
        // 读目标玩家脚下的方块：目标在别的岛。
        (10, Step::Player(0, format!("tp {g1} {}", c(1, 9, 3, 2)))),
        (11, Step::Player(2, "gamemode creative @a".to_owned())),
        (12, Step::Player(0, format!("gamemode survival {g1}"))),
        (13, Step::Console("defaultgamemode adventure".to_owned())),
        (14, Step::Console(format!("gamemode creative {g2}"))),
        // 哪个岛都没加载的位置：两边报同样的错。
        (
            15,
            Step::Player(1, "setblock 100000 -60 100000 minecraft:stone".to_owned()),
        ),
        // 别的岛上一小块空心的玻璃盒子（含邻居更新）。
        (
            16,
            Step::Player(
                2,
                format!(
                    "fill {} {} minecraft:glass hollow",
                    c(0, 12, 1, 2),
                    c(0, 14, 3, 4)
                ),
            ),
        ),
        (
            17,
            Step::Player(
                2,
                format!(
                    "loot replace block {} container.3 loot {table}",
                    c(0, 3, 3, 2)
                ),
            ),
        ),
        // 换一个发起人再碰同一处。
        (
            18,
            Step::Player(
                1,
                format!("setblock {} minecraft:redstone_block", c(0, 2, 3, 2)),
            ),
        ),
    ];
    out.sort_by_key(|(tick, _)| *tick);
    out
}

fn perform(scene: &mut Scene, step: &Step) {
    match step {
        Step::Player(group, line) => {
            let client = first_of(scene, *group);
            scene
                .server
                .request(&client, PlayRequest::unsigned_command(line.clone()));
        }
        Step::Console(line) => scene
            .server
            .inbox_sender()
            .send(crate::inbound::Inbound::ConsoleCommand(line.clone()))
            .expect("世界还在"),
    }
}

/// 多岛对单岛的一次对照。`issuer_only` 为真时多岛那份把「整个世界」退回只借玩家（只看得见
/// 发起人的岛的方块，改坏确认）。返回多岛那份拼了几次。
///
/// One multi-island against single-island comparison. With `issuer_only` the
/// multi-island world falls back to borrowing players only (it sees just the
/// sender island's blocks; the break-it check). Returns how many times the
/// multi-island world joined its islands.
fn run(seed: u64, issuer_only: bool) -> usize {
    let mut multi = set_up(seed, Harness::new_islands(2, 0));
    let mut control = set_up(
        seed,
        Harness::new_loaded_with_drive(2, IslandDrive::Lockstep),
    );
    align(&mut multi, &mut control);
    multi.server.world.islands.world_view_off = issuer_only;
    control.server.world.master.port_mode = crate::global_port::PortMode::Direct;
    assert_eq!(multi.server.world.islands.len(), multi.centers.len());
    let sender = first_of(&multi, 0);
    let sender_island = multi
        .server
        .world
        .islands
        .island_of_player(sender.id)
        .expect("在某个岛上");
    for scene in [&mut multi, &mut control] {
        for group in 0..scene.centers.len() {
            let client = first_of(scene, group);
            make_op(scene, &client);
        }
        for island in scene.server.world.islands.iter_mut() {
            island.region.clock_millis = Some(1_700_000_000_000);
        }
    }
    // 取随机数用发起人岛的（计划 4.5）：第 0 组玩家的岛和对照那一个岛对齐成同样的状态，
    // 之后两边取的数就一样。别的岛的随机源不动，下面逐刻断言它们没被推进。
    // Random numbers come from the sender's island (plan 4.5): group 0's
    // island and the control island are aligned to the same state, so both
    // draw the same numbers. The other islands' sources are left alone and
    // asserted untouched every tick below.
    for region in [
        &mut multi
            .server
            .world
            .islands
            .get_mut(sender_island)
            .expect("在表里")
            .region,
        &mut control.server.world.islands.region,
    ] {
        region.random = mc_bamboo_blocks::LevelRandom::new(0);
        region.sound_seed_generator = mc_bamboo_blocks::LevelRandom::new(0);
        region.entities.set_entity_seeds(4242);
    }
    let seeds_of = |scene: &Scene| -> BTreeMap<RegionId, i64> {
        scene
            .server
            .world
            .islands
            .iter()
            .map(|island| (island.id, island.region.entities.entity_seed_state()))
            .collect()
    };
    let steps = script(&mut multi);
    let last = steps.last().expect("脚本不为空").0;
    let mut filled = BTreeSet::new();
    for tick in 0..last + 12 {
        for (_, step) in steps.iter().filter(|(at, _)| *at == tick) {
            perform(&mut multi, step);
            perform(&mut control, step);
        }
        let before = (random_states(&multi), seeds_of(&multi));
        multi.server.tick();
        control.server.tick();
        let what = format!("seed {seed} tick {tick}");
        // 发起人以外的岛的随机源和实体种子源没有被别的岛的命令推进。
        // No island but the sender's had its random or seed source advanced.
        let after = (random_states(&multi), seeds_of(&multi));
        for (id, state) in &before.0 {
            if *id != sender_island {
                assert_eq!(after.0.get(id), Some(state), "{what}：岛 {id} 的随机源");
            }
        }
        for (id, state) in &before.1 {
            if *id != sender_island {
                assert_eq!(after.1.get(id), Some(state), "{what}：岛 {id} 的实体种子源");
            }
        }
        assert_in_place(&multi, &what);
        let (groups, clocks) = print_islands(&mut multi);
        let expected = print_control(&mut control);
        for (group, (left, right)) in groups.iter().zip(&expected).enumerate() {
            assert_eq!(left, right, "{what}：第 {group} 组的内容");
        }
        let clock = control.server.world.islands.region.tick.game_time();
        assert_eq!(clocks, BTreeSet::from([clock]), "{what}：岛的时钟");
        for ((_, mine), (_, theirs)) in multi.clients.iter().zip(&control.clients) {
            assert_eq!(
                mine.take_frames(),
                theirs.take_frames(),
                "{what}：玩家 {:?} 收到的字节",
                mine.id
            );
        }
        multi.server.world.assert_island_invariants();
        // 跨两个岛的那条 `/fill` 两头都真的填上了。
        // The `/fill` spanning two islands really filled both ends.
        let (a, b) = aligned_pair(&multi.centers).expect("种子挑过了");
        for group in [a, b] {
            let pos = at(multi.centers[group], 5, Y + 4, 5);
            let region = super::island_split_tests::region_at(&mut multi.server.world, pos.chunk());
            if region.chunks.block(pos) == Some(blocks::GLASS) {
                filled.insert(group);
            }
        }
    }
    assert!(
        multi.server.world.islands.several(),
        "seed {seed}：一直是多个岛"
    );
    if !issuer_only {
        assert_eq!(filled.len(), 2, "seed {seed}：跨岛的 fill 两头都填上了");
        // 脚本真的把东西放到了别的岛上：岛 1 有牛，岛 2 有掉落物，岛 0 有搬过来的箱子。
        // The script really put things on other islands.
        let world = &mut multi.server.world;
        let centers = multi.centers.clone();
        let count = |world: &mut super::World, group: usize| {
            super::island_split_tests::region_at(world, centers[group])
                .entities
                .len()
        };
        assert!(count(world, 1) >= 1, "seed {seed}：牛在第 1 组的岛上");
        assert!(count(world, 2) >= 1, "seed {seed}：掉落物在第 2 组的岛上");
        let chest = at(centers[0], 3, Y + 3, 2);
        let region = super::island_split_tests::region_at(world, chest.chunk());
        assert!(
            region
                .containers
                .get(&chest)
                .is_some_and(|container| container.slots.iter().any(|stack| !stack.is_empty())),
            "seed {seed}：搬到第 0 组的箱子带着东西"
        );
    }
    multi.server.world.islands.unified
}

/// 计划 4.5：碰方块、容器、实体和读方块的命令作用到别的岛，齐步驱动下多岛和整个世界
/// 一个岛逐字节相同。
///
/// Plan 4.5: commands touching blocks, containers and entities, or reading
/// blocks, on other islands match the single-island world byte for byte under
/// the lockstep drive.
#[test]
fn commands_reaching_other_islands_match_one_island() {
    with_stack(|| {
        let mut seed = 40;
        for _ in 0..2 {
            seed = seed_with_aligned_groups(seed);
            let unified = run(seed, false);
            assert!(
                unified >= 10,
                "seed {seed}：要整个世界的命令都拼过（{unified} 次）"
            );
            seed += 1;
        }
    });
}

/// 改坏确认：把「整个世界」退回只借玩家（命令只看得见发起人的岛的方块），上面的对照要
/// 失败。同样的种子不改坏时通过（上一条测试的第一个种子）。
///
/// Break-it check: falling back to borrowing players only (the command sees
/// just the sender island's blocks) must fail the comparison above; the same
/// seed passes unbroken (the first seed of the test above).
#[test]
fn seeing_only_the_senders_island_is_caught() {
    with_stack(|| {
        let seed = seed_with_aligned_groups(40);
        let caught = std::panic::catch_unwind(|| run(seed, true));
        let message = match caught {
            Ok(_) => panic!("只看发起人的岛没有被抓到"),
            Err(payload) => payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&str>()
                        .map(|text| (*text).to_owned())
                })
                .unwrap_or_default(),
        };
        // 第一条命令（玩家往别的岛放石头）那一刻就对不上：内容或者收到的字节。
        // The very first command (a stone placed on another island) already differs.
        assert!(
            message.contains("tick 1："),
            "失败应该出在第一条跨岛命令那一刻：{message}"
        );
    });
}

/// 只碰玩家的命令不拼（计划 4.5「为什么不是每条命令都拼」）；要整个世界的命令每条拼一次，
/// 做完每个岛还是原来那个：编号、区块、玩家、欠刻、信箱和存盘名单都没动，别的岛的四个
/// 随机源状态原样。
///
/// Player-only commands do not join the islands; whole-world commands join
/// once each, and afterwards every island is what it was: id, chunks,
/// players, mailbox and save lists untouched, and the other islands' four
/// random sources unchanged.
#[test]
fn joining_for_a_command_leaves_every_island_as_it_was() {
    with_stack(|| {
        let mut scene = set_up(51, Harness::new_islands(2, 0));
        for group in 0..scene.centers.len() {
            let client = first_of(&scene, group);
            make_op(&mut scene, &client);
        }
        let shape = |scene: &Scene| -> Vec<(
            RegionId,
            Vec<ChunkPos>,
            Vec<crate::inbound::PlayerId>,
            String,
        )> {
            scene
                .server
                .world
                .islands
                .iter()
                .map(|island| {
                    (
                        island.id,
                        island.region.chunks.loaded_positions(),
                        island.region.players.keys().copied().collect(),
                        format!(
                            "{:?} {:?} {} {:?}",
                            island.region.random,
                            island.region.sound_seed_generator,
                            island.region.entities.entity_seed_state(),
                            island.mailbox.positions(),
                        ),
                    )
                })
                .collect()
        };
        let before = shape(&scene);
        let sender = first_of(&scene, 1);
        for line in [
            "msg @a hi",
            "list",
            "give @a minecraft:stone 1",
            "time set 10",
        ] {
            scene
                .server
                .request(&sender, PlayRequest::unsigned_command(line));
        }
        scene.server.tick();
        assert_eq!(scene.server.world.islands.unified, 0, "只碰玩家的命令不拼");
        let target = spot(&scene.centers, 2, 4, 1, 4);
        scene.server.request(
            &sender,
            PlayRequest::unsigned_command(format!("setblock {target} minecraft:stone")),
        );
        let other = first_of(&scene, 0);
        scene.server.request(
            &other,
            PlayRequest::unsigned_command(format!("setblock {target} minecraft:dirt")),
        );
        scene.server.tick();
        assert_eq!(
            scene.server.world.islands.unified, 2,
            "每条要整个世界的命令拼一次"
        );
        assert_eq!(shape(&scene), before, "拼完拆回去，每个岛还是原来那个");
        let pos = at(scene.centers[2], 4, Y + 1, 4);
        let region = super::island_split_tests::region_at(&mut scene.server.world, pos.chunk());
        assert_eq!(region.chunks.block(pos), Some(blocks::DIRT));
        scene.server.world.assert_island_invariants();
    });
}
