//! 按需借岛的等价性验证（岛的实施计划 4.5.1 第 7 节）。
//!
//! 同一个多岛世界搭两份，逐位相同之后，一份强制「全拼」、一份走按需借，执行同一条
//! 命令，比较每个岛的内容指纹、随机源、每名玩家收到的全部字节。另有逐命令逐形态用例、
//! 固定种子模糊测试、六种改坏确认。
//!
//! Equivalence checks for on-demand island borrowing (plan 4.5.1, section 7).
//! Two identical multi-island worlds run the same command, one forced to join
//! every island and one borrowing on demand; per-island fingerprints, random
//! sources and every byte each player receives are compared.

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_blocks::entity::Entity;
use mc_bamboo_core::{ChunkPos, Vec3};
use mc_bamboo_protocol::packet::play::PlayRequest;
use mc_bamboo_world::director::RegionId;

use super::{
    island_crossing_tests::{first_of, make_op, name_of},
    island_runtime_tests::random_states,
    island_split_tests::{Scene, Y, at, fingerprint, region_at, set_up, with_stack},
    tests::{Frame, Harness},
};
use crate::island::BorrowBreak;

/// 一份对照场景：`borrow_all` 为真时强制全拼。
struct Side {
    scene: Scene,
}

impl Side {
    fn new(seed: u64, borrow_all: bool) -> Self {
        let mut scene = set_up(seed, Harness::new_islands(2, 0));
        for group in 0..scene.centers.len() {
            let client = first_of(&scene, group);
            make_op(&mut scene, &client);
        }
        scene.server.world.islands.borrow_all_islands = borrow_all;
        Self { scene }
    }
}

/// 两边跑到同一个 `director_tick`，丢掉期间收到的包，随机源对齐成同一状态。
fn align(a: &mut Side, b: &mut Side) {
    loop {
        let (left, right) = (
            a.scene.server.world.master.director_tick,
            b.scene.server.world.master.director_tick,
        );
        if left == right {
            break;
        }
        if left < right {
            a.scene.server.tick();
        } else {
            b.scene.server.tick();
        }
    }
    for _ in 0..3 {
        a.scene.server.tick();
        b.scene.server.tick();
    }
    for (_, client) in a.scene.clients.iter().chain(&b.scene.clients) {
        let _ = client.take_frames();
    }
    // 两份世界的随机源对齐（和 4.5 的对照一样）；别的岛不取数。
    for side in [&mut *a, &mut *b] {
        for island in side.scene.server.world.islands.iter_mut() {
            island.region.random = mc_bamboo_blocks::LevelRandom::new(0);
            island.region.sound_seed_generator = mc_bamboo_blocks::LevelRandom::new(0);
            island.region.entities.set_entity_seeds(4242);
        }
    }
}

/// 所有岛的指纹（按岛编号），含全局共有那一份。
fn prints(side: &mut Side) -> BTreeMap<RegionId, Vec<String>> {
    let centers = side.scene.centers.clone();
    let mut hashes = std::mem::take(&mut side.scene.hashes);
    let out = side
        .scene
        .server
        .world
        .islands
        .iter()
        .map(|island| {
            (
                island.id,
                fingerprint(&island.region, &centers, false, &mut hashes),
            )
        })
        .collect();
    side.scene.hashes = hashes;
    out
}

/// 每名玩家收到的全部字节，按玩家编号。
fn frames(side: &Side) -> BTreeMap<crate::inbound::PlayerId, Vec<Frame>> {
    side.scene
        .clients
        .iter()
        .map(|(_, client)| (client.id, client.take_frames()))
        .collect()
}

/// 一份世界的状态快照：内容指纹、随机源、实体种子源、岛时钟。
struct Snapshot {
    prints: BTreeMap<RegionId, Vec<String>>,
    randoms: BTreeMap<RegionId, String>,
    seeds: BTreeMap<RegionId, i64>,
    clocks: BTreeSet<i64>,
}

fn snapshot(side: &mut Side) -> Snapshot {
    let prints = prints(side);
    let randoms = random_states(&side.scene);
    let world = &side.scene.server.world;
    let seeds = world
        .islands
        .iter()
        .map(|island| (island.id, island.region.entities.entity_seed_state()))
        .collect();
    let clocks = world
        .islands
        .iter()
        .map(|island| island.region.tick.game_time())
        .collect();
    Snapshot {
        prints,
        randoms,
        seeds,
        clocks,
    }
}

/// 找到两组按岛指纹里第一行不一样的内容，失败信息只打这一行。
fn first_difference(
    a: &BTreeMap<RegionId, Vec<String>>,
    b: &BTreeMap<RegionId, Vec<String>>,
) -> String {
    for (id, left) in a {
        let Some(right) = b.get(id) else {
            return format!("岛 {id} 只在一边有");
        };
        for (group, (l, r)) in left.iter().zip(right).enumerate() {
            if l == r {
                continue;
            }
            for (line_l, line_r) in l.lines().zip(r.lines()) {
                if line_l != line_r {
                    return format!(
                        "岛 {id} 第 {group} 组：\n  全拼: {}\n  按需: {}",
                        &line_l[..line_l.len().min(240)],
                        &line_r[..line_r.len().min(240)]
                    );
                }
            }
            return format!(
                "岛 {id} 第 {group} 组行数不同：{} / {}",
                l.lines().count(),
                r.lines().count()
            );
        }
    }
    "岛编号不同".to_owned()
}

fn assert_equal(a: &Snapshot, b: &Snapshot, what: &str) {
    assert!(
        a.prints == b.prints,
        "{what}：每个岛的内容指纹：{}",
        first_difference(&a.prints, &b.prints)
    );
    assert_eq!(a.randoms, b.randoms, "{what}：岛的随机源");
    assert_eq!(a.seeds, b.seeds, "{what}：岛的实体种子源");
    assert_eq!(a.clocks, b.clocks, "{what}：岛的时钟");
}

/// 一条命令的两种形态：玩家发或控制台发。
#[derive(Clone)]
enum Step {
    Player(usize, String),
    Console(String),
}

impl Step {
    fn line(&self) -> &str {
        match self {
            Step::Player(_, line) | Step::Console(line) => line,
        }
    }
}

/// 第 `group` 组中心里的一格，写成命令里的坐标。
fn spot(centers: &[ChunkPos], group: usize, x: i32, dy: i32, z: i32) -> String {
    let pos = at(centers[group], x, Y + dy, z);
    format!("{} {} {}", pos.x, pos.y, pos.z)
}

fn perform(side: &mut Side, step: &Step) {
    match step {
        Step::Player(group, line) => {
            let client = first_of(&side.scene, *group);
            side.scene
                .server
                .request(&client, PlayRequest::unsigned_command(line.clone()));
        }
        Step::Console(line) => side
            .scene
            .server
            .inbox_sender()
            .send(crate::inbound::Inbound::ConsoleCommand(line.clone()))
            .expect("世界还在"),
    }
}

/// 覆盖 4.5.1 第 4 节每条命令的每种形态（含跨岛、未加载、选择器、execute、UUID）。
fn cases(centers: &[ChunkPos], names: &BTreeMap<usize, String>) -> Vec<Vec<Step>> {
    let c = |group, x, dy, z| spot(centers, group, x, dy, z);
    let n = |group| names.get(&group).cloned().unwrap_or_default();
    let table = "minecraft:blocks/diamond_ore";
    // 显式 UUID：和 `uuid_tag([1,..])` 同一份 IntArray[4] 编码。
    let uuid = "{UUID:[I;1,2,3,4]}";
    let mut out: Vec<Vec<Step>> = Vec::new();
    // 玩家 / 坐标 / 未加载 / 相对坐标的 tp：一个岛都不该拼。
    out.push(vec![Step::Player(
        0,
        format!("tp {} {}", n(1), c(2, 8, 1, 8)),
    )]);
    out.push(vec![Step::Player(
        1,
        format!("tp {} {}", n(0), c(0, 3, 1, 3)),
    )]);
    out.push(vec![Step::Player(
        0,
        format!("tp {} 100000 -60 100000", n(2)),
    )]);
    out.push(vec![Step::Player(0, "tp @s ~ ~1 ~".to_owned())]);
    // setblock / fill / clone 跨岛与同岛、未加载。
    out.push(vec![Step::Player(
        0,
        format!("setblock {} minecraft:stone", c(2, 3, 1, 3)),
    )]);
    out.push(vec![Step::Player(
        1,
        format!("fill {} {} minecraft:glass", c(0, 2, 1, 2), c(0, 4, 1, 4)),
    )]);
    out.push(vec![Step::Player(
        0,
        format!(
            "clone {} {} {}",
            c(1, 2, 1, 2),
            c(1, 3, 1, 3),
            c(2, 6, 1, 6)
        ),
    )]);
    out.push(vec![Step::Player(
        2,
        "setblock 100000 -60 100000 minecraft:stone".to_owned(),
    )]);
    // summon（含显式 UUID）、loot、kill、damage、enchant、gamemode、defaultgamemode、data。
    out.push(vec![Step::Player(
        0,
        format!("summon minecraft:cow {}", c(1, 8, 1, 8)),
    )]);
    // 显式 UUID 的牛在第 1 组，这里召唤到第 2 组：撞车全靠 UUID 查出第 1 组。
    // The explicit-UUID cow lives at group 1 while this summons at group 2:
    // only the UUID lookup can bring group 1's island in.
    out.push(vec![Step::Player(
        0,
        format!("summon minecraft:cow {} {uuid}", c(2, 9, 1, 9)),
    )]);
    out.push(vec![Step::Player(
        0,
        format!("loot spawn {} loot {table}", c(2, 8, 1, 8)),
    )]);
    out.push(vec![Step::Player(
        0,
        format!("loot insert {} loot {table}", c(1, 4, 1, 3)),
    )]);
    out.push(vec![Step::Player(
        0,
        "kill @e[type=minecraft:cow]".to_owned(),
    )]);
    out.push(vec![Step::Player(0, format!("damage {} 1", n(1)))]);
    out.push(vec![Step::Player(
        0,
        format!("enchant {} minecraft:sharpness 1", n(2)),
    )]);
    out.push(vec![Step::Player(0, "gamemode creative @a".to_owned())]);
    out.push(vec![Step::Console("defaultgamemode adventure".to_owned())]);
    out.push(vec![Step::Console(format!("gamemode survival {}", n(1)))]);
    // 第二轮收紧的常用写法：只按候选目标算盒。
    out.push(vec![Step::Player(0, "gamemode creative".to_owned())]);
    out.push(vec![Step::Player(0, "kill @s".to_owned())]);
    out.push(vec![Step::Player(
        0,
        "kill @e[type=minecraft:cow,distance=..5]".to_owned(),
    )]);
    out.push(vec![Step::Player(0, "effect give @s speed 5".to_owned())]);
    out.push(vec![Step::Player(
        0,
        "effect give @e[type=minecraft:cow] slowness 5".to_owned(),
    )]);
    out.push(vec![Step::Player(0, "effect clear @a".to_owned())]);
    out.push(vec![Step::Player(
        0,
        "effect clear @e[type=minecraft:cow] minecraft:slowness".to_owned(),
    )]);
    out.push(vec![Step::Console(format!(
        "execute as {} run gamemode survival",
        n(1)
    ))]);
    out.push(vec![Step::Console(format!(
        "execute as {} at @s run setblock ~ ~-1 ~ minecraft:stone",
        n(1)
    ))]);
    out.push(vec![Step::Player(
        0,
        "execute as @s at @s run setblock ~ ~-1 ~ minecraft:stone".to_owned(),
    )]);
    out.push(vec![Step::Player(0, format!("gamemode survival {}", n(1)))]);
    out.push(vec![Step::Player(
        0,
        format!("effect give {} minecraft:speed 5", n(1)),
    )]);
    out.push(vec![Step::Player(
        2,
        "effect give @e[type=minecraft:cow,sort=nearest,limit=1] minecraft:glowing 5".to_owned(),
    )]);
    out.push(vec![Step::Player(
        0,
        "effect give @r minecraft:haste 5".to_owned(),
    )]);
    out.push(vec![Step::Console(
        "effect give @e[sort=random,limit=1] minecraft:speed 5".to_owned(),
    )]);
    out.push(vec![Step::Player(1, "effect clear".to_owned())]);
    // 分叉上的 `@s` 是牛：链尾按分叉的执行者算候选。
    // `@s` on each branch is a cow: the tail's candidates follow the branch.
    out.push(vec![Step::Console(
        "execute as @e[type=minecraft:cow] run effect give @s minecraft:slowness 5".to_owned(),
    )]);
    out.push(vec![Step::Console(
        "execute as @e[type=minecraft:cow] run kill".to_owned(),
    )]);
    // `at` 别的岛上的玩家：链尾的距离按他的位置算。
    // `at` a player on another island: the tail's distance uses their position.
    out.push(vec![Step::Console(format!(
        "execute at {} run kill @e[type=minecraft:cow,distance=..5]",
        n(1)
    ))]);
    out.push(vec![Step::Player(
        2,
        format!(
            "damage {} 1 minecraft:generic by @e[type=minecraft:cow,sort=nearest,limit=1]",
            n(1)
        ),
    )]);
    out.push(vec![Step::Player(
        0,
        format!(
            "damage @e[type=minecraft:cow,sort=furthest,limit=1] 1 minecraft:generic by {}",
            n(2)
        ),
    )]);
    // `positioned <pos>` 之后的相对坐标按新位置解：从第 1 组发、落点在第 0 组，
    // 偏移到第 2 组。
    // Relative coordinates after `positioned <pos>` use the new position:
    // sent from group 1, positioned at group 0, offset into group 2.
    out.push(vec![Step::Player(
        1,
        format!(
            "execute positioned {} run setblock ~{} ~ ~{} minecraft:stone",
            c(0, 0, 1, 0),
            (centers[2].x - centers[0].x) * 16 + 5,
            (centers[2].z - centers[0].z) * 16 + 5
        ),
    )]);
    // `store ... entity` 的目标按 (0, 0, 0) 的零上下文选：从第 1 组发，选中家岛的牛。
    // The `store ... entity` target is chosen from the zero context at
    // (0, 0, 0): sent from group 1, it picks the home island's cow.
    out.push(vec![Step::Player(
        1,
        "execute store result entity @e[type=minecraft:cow,distance=..80,limit=1] Health float 1 run kill @e[type=minecraft:cow,distance=..5]".to_owned(),
    )]);
    out.push(vec![Step::Player(
        0,
        "data get block 100000 -60 100000".to_owned(),
    )]);
    // 选择器各形态：sort / limit / distance / @e / @p / @r / 点名 / UUID。
    for line in [
        "kill @e",
        "kill @e[sort=nearest,limit=1]",
        "kill @e[sort=furthest,limit=2]",
        "kill @e[sort=random,limit=1]",
        "kill @e[distance=..64]",
        "kill @e[type=minecraft:cow,distance=..1000]",
        "kill @p",
        "kill @r",
        "kill @a",
        "kill @s",
    ] {
        out.push(vec![Step::Console(line.to_owned())]);
    }
    // 控制台的方块命令按出生点解析，和玩家发起的形态都覆盖。
    out.push(vec![Step::Console(format!(
        "setblock {} minecraft:gold_block",
        c(1, 5, 1, 5)
    ))]);
    // execute：能规划、兜底、多分叉、条件为假、store。
    out.push(vec![Step::Console(format!(
        "execute positioned {} run setblock ~ ~ ~ minecraft:stone",
        c(2, 5, 1, 5)
    ))]);
    out.push(vec![Step::Console(format!(
        "execute as @a at @s run setblock {} minecraft:stone",
        c(1, 6, 1, 6)
    ))]);
    out.push(vec![Step::Console(format!(
        "execute if block {} minecraft:air run setblock ~ ~ ~ minecraft:stone",
        c(0, 7, 1, 7)
    ))]);
    out.push(vec![Step::Console(format!(
        "execute unless entity @e[type=cow] run setblock {} minecraft:stone",
        c(2, 7, 1, 7)
    ))]);
    out.push(vec![Step::Console(
        "execute if score nobody noobjective matches 1 run say hi".to_owned(),
    )]);
    out
}

/// 每组中心放一头牛，让选择器和实体相关用例真的踩到别的岛；第 1 组的牛带
/// 显式 UUID `[I;1,2,3,4]`，和用例里的 `/summon ... {UUID:...}` 撞车。
///
/// Put a cow at each group centre so selectors and entity cases really reach
/// another island; group 1's cow carries the explicit UUID used by the
/// `/summon` collision case.
fn furnish_entities(side: &mut Side) {
    let centers = side.scene.centers.clone();
    for (group, center) in centers.iter().enumerate() {
        let pos = Vec3::new(
            f64::from(center.x * 16) + 8.5,
            f64::from(Y + 1),
            f64::from(center.z * 16) + 8.5,
        );
        let region = region_at(&mut side.scene.server.world, *center);
        let uid = region.entities.spawn(Entity::cow(pos));
        if group == 1 {
            let mut uuid = [0u8; 16];
            uuid[3] = 1;
            uuid[7] = 2;
            uuid[11] = 3;
            uuid[15] = 4;
            region.entities.with_mut(uid, |entity| {
                entity.with_mob(|_, mob| mob.uuid = uuid);
            });
        }
    }
}

/// 一次双跑对照：同一串命令在两份世界里各执行一次，每步比较整个世界。
fn compare(seed: u64, steps: &[Step], break_kind: BorrowBreak, furnish: bool) -> usize {
    let mut full = Side::new(seed, true);
    let mut on_demand = Side::new(seed, false);
    if furnish {
        furnish_entities(&mut full);
        furnish_entities(&mut on_demand);
    }
    align(&mut full, &mut on_demand);
    on_demand.scene.server.world.islands.borrow_break = break_kind;
    let mut joins = 0;
    for (index, step) in steps.iter().enumerate() {
        let what = format!("seed {seed} step {index} `{}`", step.line());
        for side in [&mut full, &mut on_demand] {
            let _ = frames(side);
        }
        // `sort=random` 用进程级随机源，命令在下一刻的收包阶段执行：两份
        // 世界各自「发请求 → 跑一刻」之间重置一次，取数序列才相同。
        // `sort=random` uses a process-wide source and the command runs in
        // the next tick's inbound phase: reset between each world's request
        // and tick so both draw the same sequence.
        crate::command::select::reset_shuffle_random();
        perform(&mut full, step);
        full.scene.server.tick();
        crate::command::select::reset_shuffle_random();
        perform(&mut on_demand, step);
        on_demand.scene.server.tick();
        let left_frames = frames(&full);
        let right_frames = frames(&on_demand);
        assert_eq!(left_frames, right_frames, "{what}：每名玩家收到的字节");
        let left = snapshot(&mut full);
        let right = snapshot(&mut on_demand);
        assert_equal(&left, &right, &what);
        on_demand.scene.server.world.assert_island_invariants();
        joins += on_demand.scene.server.world.islands.last_borrowed;
        on_demand.scene.server.world.islands.last_borrowed = 0;
    }
    on_demand.scene.server.world.islands.borrow_break = BorrowBreak::None;
    joins
}

/// 逐条命令逐形态：同一条命令全拼和按需借逐字节相同。
#[test]
fn every_command_shape_matches_a_full_join() {
    with_stack(|| {
        // 每条用例在自己的种子里跑，坐标和玩家名按那个种子的岛布局算。
        // Each case runs under its own seed with coordinates and names taken
        // from that seed's layout.
        let pools: BTreeMap<u64, Vec<Vec<Step>>> =
            (51..58).map(|seed| (seed, pool_for(seed))).collect();
        let count = pools[&51].len();
        assert!(pools.values().all(|pool| pool.len() == count));
        // 每条用例各自搭两份世界，避免前一条留下的改动影响后一条。
        let seeds = (0..count).map(|index| (index, 51 + index as u64 % 7));
        for (index, seed) in seeds {
            compare(seed, &pools[&seed][index], BorrowBreak::None, true);
        }
    });
}

/// 一个种子的命令池：坐标和玩家名都按这个种子的岛布局。
///
/// One seed's command pool, with coordinates and names from its layout.
fn pool_for(seed: u64) -> Vec<Vec<Step>> {
    let mut scene = Side::new(seed, true);
    let mut names = BTreeMap::new();
    for group in 0..scene.scene.centers.len() {
        let client = first_of(&scene.scene, group);
        names.insert(group, name_of(&mut scene.scene, &client));
    }
    cases(&scene.scene.centers, &names)
}

/// 固定种子的随机模糊测试：随机岛布局 × 随机命令 × 随机参数，每条都做双跑对照。
#[test]
fn fuzzed_commands_match_a_full_join() {
    with_stack(|| {
        let mut next = super::island_split_tests::rng(0x5eed_b077_0000);
        // 每个种子一份命令池：命令里的坐标必须和该种子的岛布局一致。
        // One command pool per seed: the coordinates must match that seed's
        // island layout.
        let pools: BTreeMap<u64, Vec<Vec<Step>>> =
            (51..60).map(|seed| (seed, pool_for(seed))).collect();
        for round in 0..500 {
            let seed = 51 + next() % 9;
            let pool = &pools[&seed];
            let mut steps = Vec::new();
            for _ in 0..8 {
                let pick = (next() as usize) % pool.len();
                steps.extend(pool[pick].iter().cloned());
            }
            compare(seed, &steps, BorrowBreak::None, round % 2 == 0);
        }
    });
}

/// 改坏确认：漏借一个岛 / 漏一个候选岛 / 不借玩家 / 漏目的地岛 / 漏查 UUID /
/// 少还原一个岛的时钟，都必须被双跑对照抓到（不许为通过删用例、放宽比较）。
#[test]
fn missing_an_island_is_caught() {
    with_stack(|| {
        let mut scene = Side::new(51, true);
        let mut names = BTreeMap::new();
        for group in 0..scene.scene.centers.len() {
            let client = first_of(&scene.scene, group);
            names.insert(group, name_of(&mut scene.scene, &client));
        }
        let centers = scene.scene.centers.clone();
        let steps: Vec<Step> = cases(&centers, &names).into_iter().flatten().collect();
        for kind in [
            BorrowBreak::MissPlayers,
            BorrowBreak::MissIsland,
            BorrowBreak::MissSelector,
            BorrowBreak::MissDestination,
            BorrowBreak::MissUuid,
            BorrowBreak::MissIdentity,
            BorrowBreak::MissCandidatePlayer,
            BorrowBreak::MissEffect,
        ] {
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                compare(51, &steps, kind, true);
            }));
            assert!(caught.is_err(), "改坏 {kind:?} 没有被抓到");
        }
    });
}

/// 第二轮的两条改坏各自用对应的命令单独确认：只放一条（或一组）命令，抓到的一定是
/// 这条命令的形态。`/kill` 别的岛上的玩家要先给他东西，死亡掉落才会落在他的岛上。
///
/// The two second-round breaks are each confirmed on their own command: with
/// only that command in the run, the catch must come from its shape. The
/// other-island player gets items first so their death drops land there.
#[test]
fn narrowing_breaks_are_caught_by_their_own_commands() {
    with_stack(|| {
        let mut scene = Side::new(51, true);
        let client = first_of(&scene.scene, 1);
        let name = name_of(&mut scene.scene, &client);
        let runs = [
            (
                BorrowBreak::MissEffect,
                vec![Step::Player(
                    0,
                    "effect give @e[type=minecraft:cow] minecraft:slowness 5".to_owned(),
                )],
            ),
            (
                BorrowBreak::MissCandidatePlayer,
                vec![
                    Step::Console(format!("give {name} minecraft:stone 5")),
                    Step::Player(0, format!("kill {name}")),
                ],
            ),
        ];
        for (kind, steps) in runs {
            // 不改坏时同一串命令必须相等，抓到的差异才算改坏造成的。
            // Unbroken, the same run must match, so a catch is the break's.
            compare(51, &steps, BorrowBreak::None, true);
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                compare(51, &steps, kind, true);
            }));
            assert!(caught.is_err(), "改坏 {kind:?} 没有被它自己的命令抓到");
        }
    });
}

/// 审查要求的三条方块盒对抗用例：① 命令落在岛 A 已加载区域的边缘、紧贴间隔带；
/// ② 落在间隔带里、离岛 B 的已加载区块只有 1 格；③ 红石线 / 玻璃一直铺到岛的
/// 边缘，在另一端触发。每条都和全拼逐字节相同。
///
/// The three adversarial box-edge cases required at review: a command at the
/// edge of a loaded island, one in the gap one block from another island's
/// loaded chunk, and a long line of blocks reaching the island edge.
#[test]
fn block_updates_at_the_gap_match_a_full_join() {
    with_stack(|| {
        for seed in 51..54 {
            let scene = Side::new(seed, true);
            let centers = scene.scene.centers.clone();
            let base = centers[1];
            // 岛 1 已加载区域的边缘（视距 2，中心 ±4 区块）；间隔带在它外面。
            // Edge of group 1's loaded footprint (view distance 2, ±4 chunks).
            let edge = ChunkPos::new(base.x + 4, base.z);
            let gap = ChunkPos::new(base.x + 5, base.z);
            let beyond = ChunkPos::new(base.x + 7, base.z);
            let edge_pos = at(edge, 0, Y + 1, 0);
            let gap_pos = at(gap, 0, Y + 1, 0);
            let beyond_pos = at(beyond, 0, Y + 1, 0);
            let mut steps = vec![
                // ① 紧贴间隔带的边缘放红石方块（会通知六邻格和比较器）。
                Step::Player(
                    1,
                    format!(
                        "setblock {} {} {} minecraft:redstone_block",
                        edge_pos.x, edge_pos.y, edge_pos.z
                    ),
                ),
                // ② 间隔带里离岛 B 已加载区块只有 1 格的位置。
                Step::Player(
                    1,
                    format!(
                        "setblock {} {} {} minecraft:stone",
                        gap_pos.x, gap_pos.y, gap_pos.z
                    ),
                ),
                // ③ 从边缘拉一条玻璃到间隔带，再在另一端触发邻居更新。
                Step::Player(
                    1,
                    format!(
                        "fill {} {} {} {} {} {} minecraft:glass",
                        edge_pos.x, edge_pos.y, edge_pos.z, gap_pos.x, gap_pos.y, gap_pos.z
                    ),
                ),
                Step::Player(
                    1,
                    format!(
                        "setblock {} {} {} minecraft:glowstone",
                        beyond_pos.x, beyond_pos.y, beyond_pos.z
                    ),
                ),
            ];
            // 相对坐标形态也走一遍同样的盒。
            steps.push(Step::Player(
                1,
                "setblock ~ ~1 ~ minecraft:stone".to_owned(),
            ));
            compare(seed, &steps, BorrowBreak::None, true);
        }
    });
}

/// 第二轮收紧的常用写法：每条命令只借候选目标所在的岛，借几个岛按场景推算后写死
/// （计划 4.5.1 第 7 节第 2 条；不许悄悄退回全拼）。
///
/// 场景（种子 51）：三组各一座岛，第 0 组就在家岛上；每组中心一头牛；发起人是
/// 第 0 组的第一名玩家（管理员）。借岛数含发起人自己的岛：只用自己的岛是 1、
/// 不拼；大于 1 才拼一次。
///
/// The common shapes narrowed in the second round, with the island counts
/// worked out from the scene and pinned: three groups on three islands
/// (group 0 on home), a cow at each group centre, the sender is group 0's
/// first player. Counts include the sender's island; 1 means nothing joined.
#[test]
fn narrow_commands_borrow_only_their_targets() {
    with_stack(|| {
        let mut side = Side::new(51, false);
        furnish_entities(&mut side);
        // 场景前提：三组、三座岛，每组的岛各不相同，第 0 组在家岛上。
        // Preconditions: three groups on three distinct islands, group 0 at home.
        let centers = side.scene.centers.clone();
        assert_eq!(centers.len(), 3, "场景应有三组");
        assert_eq!(
            side.scene.server.world.islands.ids().len(),
            3,
            "场景应有三座岛"
        );
        let owners: BTreeSet<RegionId> = centers
            .iter()
            .map(|&center| {
                side.scene
                    .server
                    .world
                    .islands
                    .island_of_chunk(center)
                    .expect("组中心已加载")
            })
            .collect();
        assert_eq!(owners.len(), 3, "三组各在一座岛上");
        assert!(
            owners.contains(&mc_bamboo_world::director::HOME_REGION),
            "第 0 组在家岛上"
        );
        let mut names = BTreeMap::new();
        for group in 0..centers.len() {
            let client = first_of(&side.scene, group);
            names.insert(group, name_of(&mut side.scene, &client));
        }
        let sender = first_of(&side.scene, 0);
        let other = &names[&1];
        let cases = [
            // 改自己的模式：只读自己脚下，自己的岛。
            ("gamemode creative".to_owned(), 1),
            // 别的岛上的玩家：他脚下那格在他的岛上，借 2 个。
            (format!("gamemode survival {other}"), 2),
            // 自己 `as` / `at` 自己：链尾的方块在自己的岛上。
            (
                "execute as @s at @s run setblock ~ ~-1 ~ minecraft:stone".to_owned(),
                1,
            ),
            // 玩家效果不读方块、不碰实体表，只用自己的岛。
            ("effect give @s minecraft:speed 5".to_owned(), 1),
            // 三头牛在三座岛上：三座都借。
            (
                "effect give @e[type=minecraft:cow] minecraft:slowness 5".to_owned(),
                3,
            ),
            (
                "effect clear @e[type=minecraft:cow] minecraft:slowness".to_owned(),
                3,
            ),
            // 只选玩家的选择器：一个别的岛都不借。
            ("effect clear @a".to_owned(), 1),
            // 打别的岛上的玩家：可能致死，掉落物按他的位置归属，借他的岛。
            (format!("damage {other} 1"), 2),
            // 只选身边 5 格的牛：别的岛上的牛都离得远，只用自己的岛。
            ("kill @e[type=minecraft:cow,distance=..5]".to_owned(), 1),
            // 对照：不限距离的牛三座岛都有。
            ("kill @e[type=minecraft:cow]".to_owned(), 3),
            // 杀自己：掉落在自己的位置。放最后，死了以后不再发命令。
            ("kill @s".to_owned(), 1),
        ];
        for (line, expected) in cases {
            let islands = &mut side.scene.server.world.islands;
            islands.unified = 0;
            islands.last_borrowed = 0;
            islands.last_fallback = false;
            side.scene
                .server
                .request(&sender, PlayRequest::unsigned_command(line.clone()));
            side.scene.server.tick();
            let islands = &side.scene.server.world.islands;
            assert!(!islands.last_fallback, "`{line}` 不该退回全拼");
            assert_eq!(islands.last_borrowed, expected, "`{line}` 借了几个岛");
            assert_eq!(
                islands.unified,
                usize::from(expected > 1),
                "`{line}` 拼了几次"
            );
        }
    });
}

/// `tp <玩家> <坐标>` 一个岛都不拼（用户问的头号场景），但仍和全拼逐字节相同。
#[test]
fn teleporting_a_player_to_coordinates_joins_no_island() {
    with_stack(|| {
        let mut on_demand = Side::new(51, false);
        let target = first_of(&on_demand.scene, 1);
        let name = name_of(&mut on_demand.scene, &target);
        let pos = spot(&on_demand.scene.centers, 2, 8, 1, 8);
        on_demand.scene.server.world.islands.unified = 0;
        on_demand.scene.server.request(
            &first_of(&on_demand.scene, 0),
            PlayRequest::unsigned_command(format!("tp {name} {pos}")),
        );
        on_demand.scene.server.tick();
        assert_eq!(
            on_demand.scene.server.world.islands.unified, 0,
            "传送玩家到坐标不该拼任何岛"
        );
    });
}
