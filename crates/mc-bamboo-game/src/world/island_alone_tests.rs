//! 计划 6.2 第 1 条「分开跑 = 一起跑」，带会取随机数的新实体：`/loot spawn` 的物品、
//! `/summon` 的牛（跑 `finalizeSpawn`）、刷怪蛋的牛。三组在一个多岛世界里（齐步驱动），
//! 每组又单独起一个世界（只有这一组的玩家，别的组只留一块强加载的中心区块），逐刻比
//! 每组的内容指纹、实体的完整状态（含网络 ID 和自己的随机源）、这一组的岛的四个随机源，
//! 以及这一组每名玩家收到的全部字节。
//!
//! 成立的前提是岛的随机源只由（世界种子，起源 section，建岛时的全局游戏时间）派生
//! （计划 3.3「岛的随机源」第 1 条）：两边都在同一刻把全部岛并成一个、再按组拆开，同一组
//! 的岛起源相同、建岛时刻相同，随机源就相同；比较窗口中途再并拆一次，牛的跟踪状态和
//! 实体种子源都要跟着走对。实体网络 ID 是每个世界一个的分配器给的，三组在一起时按组
//! 轮流领号；单独那份在别的组生成实体的那一刻领同样多的空号，两边的号就对得上。
//!
//! Plan 6.2 item 1 ("run apart = run together") with fresh entities that draw
//! random numbers: `/loot spawn` items, `/summon` cows (with `finalizeSpawn`)
//! and spawn-egg cows. Three groups share one multi-island world (lockstep)
//! and each group also runs alone in its own world (only its players; the
//! other groups keep one force-loaded centre chunk). Every tick compares each
//! group's content print, the full entity state (network id and own random
//! included), the four random sources of the group's island and every byte
//! the group's players received.
//!
//! This holds because an island's random sources come only from (world seed,
//! origin section, global game time at creation) (plan 3.3): both sides merge
//! everything and split by group at the same tick, so a group's island has the
//! same origin and creation time and therefore the same sources; a second
//! merge and split mid-window checks that cow tracking state and the entity
//! seed source move correctly. Entity network ids come from one allocator per
//! world; together the groups take ids in turn, so the alone worlds take the
//! same number of blank ids at the ticks the other groups spawn.

use std::fmt::Write as _;

use mc_bamboo_core::{Direction, Hand};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{GameMode, PlayRequest},
};
use mc_bamboo_world::director::HOME_REGION;

use super::{
    island_runtime::FreshEntityBreak,
    island_runtime_tests::{print_islands, set_up_alone},
    island_split_tests::{Scene, Y, at, group_of, region_of_player, set_up, with_stack},
    tests::{Client, Frame, Harness},
};
use crate::{island_partition::SectionPos, region::IslandRandom};

/// 第一波生成的刻（组 0），之后每组晚 [`GROUP_STRIDE`] 刻。
const FIRST_WAVE: u64 = 4;
/// 比较窗口中途再并拆一次的刻。
const REGROUP_AT: u64 = 100;
/// 第二波生成的刻（组 0）。在中途并拆之后，用的是新派生的实体种子源。
const SECOND_WAVE: u64 = 104;
/// 相邻两组生成的间隔。刷怪蛋的牛下一刻才进实体表，隔两刻以上各组领号不交错。
const GROUP_STRIDE: u64 = 3;
/// 每组每一波生成的实体数：一个物品、两头牛。
const PER_WAVE: usize = 3;
/// 比较窗口的长度，够牛随机游走、寻路几次。
const TICKS: u64 = 260;

/// 第 `group` 组的第一名玩家。
fn first_of(scene: &Scene, group: usize) -> Client {
    scene
        .clients
        .iter()
        .find(|(index, _)| *index == group)
        .map(|(_, client)| client.clone())
        .expect("每组至少一名玩家")
}

/// 这个世界里有没有第 `group` 组的玩家（一起跑的世界三组都有）。
fn holds(scene: &Scene, group: usize) -> bool {
    scene.clients.iter().any(|(index, _)| *index == group)
}

/// 全部岛并成一个（家岛），`reset` 时把家岛的四个随机源换成同一份固定值，再按组拆开。
/// 返回拆开那一刻的全局游戏时间（新岛的派生时刻）。
///
/// Merge every island into home, optionally reset home's four random sources
/// to one fixed set, then split by group. Returns the global game time of
/// the split (the new islands' derivation time).
pub(super) fn regroup(scene: &mut Scene, reset: bool) -> i64 {
    let world = &mut scene.server.world;
    let partition = world.master.host.partition_mut().expect("多岛开着");
    let ids = partition.island_ids();
    for &id in &ids[1..] {
        assert!(partition.force_merge(ids[0], id), "并进岛 {}", ids[0]);
    }
    world.master.host.take_partition_ops();
    world.settle_islands();
    assert_eq!(world.islands.len(), 1, "全部并成了一个岛");
    let home = world.islands.home_mut();
    if reset {
        // 家岛从开服起一直在，此前三组都在它上面取过数；对照从这里开始，两边换成同一份。
        // Home has existed since start-up and all groups drew from it; the
        // comparison starts here, so both sides get the same set.
        let world_seed = home.region.loot_sequences.world_seed();
        home.region.replace_random(IslandRandom::derive(
            0x5EED,
            SectionPos::new(0, 0),
            0,
            world_seed,
        ));
    }
    let time = home.region.global.time.0;
    let partition = world.master.host.partition_mut().expect("多岛开着");
    let home = partition.island_ids()[0];
    partition.force_split(home);
    world.master.host.take_partition_ops();
    world.settle_islands();
    assert_eq!(
        world.islands.len(),
        scene.centers.len(),
        "按组拆回每组一个岛"
    );
    time
}

/// 拿着第 `group` 组中心区块的岛的方块随机源、音效种子源和实体种子源（含内部状态）。
/// 战利品的 `random_sequence` 序列这里没有取数的地方，不比。
fn group_random(scene: &Scene, group: usize) -> String {
    let islands = &scene.server.world.islands;
    let id = islands
        .island_of_chunk(scene.centers[group])
        .expect("每组的中心区块都加载着");
    let region = &islands.get(id).expect("在表里").region;
    format!(
        "random {:?} sound {:?} entity seeds {}",
        region.random,
        region.sound_seed_generator,
        region.entities.entity_seed_state(),
    )
}

/// 第 `group` 组的实体，完整状态（网络 ID、自己的随机源都在），按网络 ID 排。
fn group_entities(scene: &Scene, group: usize) -> String {
    let mut out = String::new();
    for island in scene.server.world.islands.iter() {
        for entity in island.region.entities.iter() {
            if group_of(&scene.centers, entity.block_position().chunk()) == group {
                let _ = writeln!(out, "{entity:?}");
            }
        }
    }
    out
}

/// 这一组的玩家：（玩家 ID，客户端）。
fn clients_of(scene: &Scene, group: usize) -> Vec<Client> {
    scene
        .clients
        .iter()
        .filter(|(index, _)| *index == group)
        .map(|(_, client)| client.clone())
        .collect()
}

/// 第 `group` 组的第一名玩家生成一波实体：脚下附近 `/loot spawn` 一个泥土物品、
/// `/summon` 一头牛、用刷怪蛋点地放一头牛。`wave` 让两波的位置错开。
fn spawn_wave(scene: &mut Scene, group: usize, wave: i32) {
    let center = scene.centers[group];
    let client = first_of(scene, group);
    let item = at(center, 3 + wave, Y + 1, 12);
    let cow = at(center, 12, Y + 1, 4 + wave);
    let egg = at(center, 5 + wave, Y, 5);
    scene.server.request(
        &client,
        PlayRequest::unsigned_command(format!(
            "loot spawn {} {} {} loot minecraft:blocks/dirt",
            item.x, item.y, item.z
        )),
    );
    scene.server.request(
        &client,
        PlayRequest::unsigned_command(format!(
            "summon minecraft:cow {} {} {}",
            cow.x, cow.y, cow.z
        )),
    );
    scene.server.request(
        &client,
        PlayRequest::UseItemOn {
            hand: Hand::Main,
            pos: egg,
            face: Direction::Up,
            cursor: (0.5, 1.0, 0.5),
            inside: false,
            world_border_hit: false,
            sequence: 10 + wave,
        },
    );
}

/// 单独跑的世界在别的组生成实体的那一刻领同样多的空号。
fn take_blank_ids(scene: &mut Scene) {
    let ids = scene
        .server
        .world
        .islands
        .get(HOME_REGION)
        .expect("家岛一直在")
        .region
        .entities
        .ids_handle()
        .clone();
    for _ in 0..PER_WAVE {
        ids.allocate();
    }
}

fn next_entity_id(scene: &Scene) -> i32 {
    scene
        .server
        .world
        .islands
        .get(HOME_REGION)
        .expect("家岛一直在")
        .region
        .entities
        .ids_handle()
        .peek()
}

/// 准备：对齐时钟，关掉给别的 OP 的命令广播（那是跨组的，单独跑的世界里没有收件人），
/// 每组第一名玩家成为 OP、手里拿刷怪蛋。
fn prepare(scene: &mut Scene) {
    scene
        .server
        .world
        .master
        .global
        .rules_mut()
        .assign(mc_bamboo_data::gamerules::LOG_ADMIN_COMMANDS, "false");
    scene.server.world.publish_global();
    for group in 0..scene.centers.len() {
        if !holds(scene, group) {
            continue;
        }
        let client = first_of(scene, group);
        let player = region_of_player(&mut scene.server.world, client.id)
            .players
            .get_mut(&client.id)
            .expect("在世界里");
        player.permission_level = 4;
        player.game_mode = GameMode::Survival;
        let slot = player.inventory.hand_slot(Hand::Main);
        player.inventory.slots[slot] = ItemStack::new(mc_bamboo_data::items::COW_SPAWN_EGG, 4);
    }
}

/// 一次「分开跑 = 一起跑」；`break_kind` 给一起跑的世界装上改坏。返回牛走过的最远距离
/// （格），用来确认牛真的动过。
fn run_apart_and_together(seed: u64, break_kind: Option<FreshEntityBreak>) -> f64 {
    let mut together = set_up(seed, Harness::new_islands(2, 0));
    let groups = together.centers.len();
    let mut alone: Vec<Scene> = (0..groups).map(|group| set_up_alone(seed, group)).collect();
    let mut worlds: Vec<&mut Scene> = std::iter::once(&mut together)
        .chain(alone.iter_mut())
        .collect();
    let target = worlds
        .iter()
        .map(|scene| scene.server.world.master.director_tick)
        .max()
        .expect("有世界");
    for scene in worlds.iter_mut() {
        while scene.server.world.master.director_tick < target {
            scene.server.tick();
        }
        for _ in 0..3 {
            scene.server.tick();
        }
    }
    let mut times = Vec::new();
    for scene in worlds.iter_mut() {
        prepare(scene);
        times.push(regroup(scene, true));
    }
    let ids: Vec<i32> = worlds.iter().map(|scene| next_entity_id(scene)).collect();
    // 建岛时刻和实体号的起点两边都必须相同，对照才成立；这里断言，不靠巧合。
    // The creation time and the next entity id must match on every side for
    // the comparison to hold; assert it rather than rely on luck.
    assert!(
        times.windows(2).all(|pair| pair[0] == pair[1]),
        "各世界拆岛的全局游戏时间不同：{times:?}"
    );
    assert!(
        ids.windows(2).all(|pair| pair[0] == pair[1]),
        "各世界下一个实体号不同：{ids:?}"
    );
    for scene in worlds.iter_mut() {
        for (_, client) in &scene.clients {
            let _ = client.take_frames();
        }
    }
    drop(worlds);
    together.server.world.master.split_break = break_kind;

    let mut spawned_at: Vec<(i32, mc_bamboo_core::Vec3)> = Vec::new();
    let mut farthest: f64 = 0.0;
    for tick in 0..TICKS {
        if tick == REGROUP_AT {
            let mut times = vec![regroup(&mut together, false)];
            for scene in &mut alone {
                times.push(regroup(scene, false));
            }
            assert!(
                times.windows(2).all(|pair| pair[0] == pair[1]),
                "中途并拆的全局游戏时间不同：{times:?}"
            );
        }
        for (wave, start) in [(0, FIRST_WAVE), (1, SECOND_WAVE)] {
            for group in 0..groups {
                if tick != start + group as u64 * GROUP_STRIDE {
                    continue;
                }
                spawn_wave(&mut together, group, wave);
                for (index, scene) in alone.iter_mut().enumerate() {
                    if index == group {
                        spawn_wave(scene, group, wave);
                    } else {
                        take_blank_ids(scene);
                    }
                }
            }
        }
        together.server.tick();
        for scene in &mut alone {
            scene.server.tick();
        }
        let (prints, _) = print_islands(&mut together);
        for (group, scene) in alone.iter_mut().enumerate() {
            let place = format!("seed {seed} tick {tick} 第 {group} 组");
            let (own, _) = print_islands(scene);
            assert_eq!(prints[group], own[group], "{place}：内容指纹");
            assert_eq!(
                group_entities(&together, group),
                group_entities(scene, group),
                "{place}：实体的完整状态"
            );
            assert_eq!(
                group_random(&together, group),
                group_random(scene, group),
                "{place}：岛的随机源"
            );
            let mine = clients_of(&together, group);
            let theirs = clients_of(scene, group);
            assert_eq!(mine.len(), theirs.len());
            for (left, right) in mine.iter().zip(&theirs) {
                assert_eq!(left.id, right.id, "{place}：两边的玩家对得上");
                let (got, expected): (Vec<Frame>, Vec<Frame>) =
                    (left.take_frames(), right.take_frames());
                assert_eq!(got, expected, "{place}：玩家 {:?} 收到的字节", left.id);
            }
        }
        for island in together.server.world.islands.iter() {
            for entity in island.region.entities.iter() {
                if !matches!(entity.kind, mc_bamboo_blocks::entity::EntityKind::Mob(_)) {
                    continue;
                }
                match spawned_at.iter().find(|(id, _)| *id == entity.id) {
                    Some((_, start)) => {
                        let (dx, dz) = (entity.position.x - start.x, entity.position.z - start.z);
                        farthest = farthest.max((dx * dx + dz * dz).sqrt());
                    }
                    None => spawned_at.push((entity.id, entity.position)),
                }
            }
        }
    }
    assert_eq!(spawned_at.len(), 2 * 2 * groups, "每组每波两头牛都生成了");
    farthest
}

/// 三组一起跑（三个岛，中途并拆一次）和每组单独跑逐刻相同，物品和牛都在；牛真的走动过。
///
/// Three groups together (three islands, one merge and split mid-window)
/// match each group alone tick by tick, items and cows included; the cows
/// really moved.
#[test]
fn random_entities_run_together_equal_each_group_run_alone() {
    with_stack(|| {
        for seed in [1, 2] {
            let farthest = run_apart_and_together(seed, None);
            assert!(
                farthest > 2.0,
                "seed {seed}：牛应该随机游走过（最远只走了 {farthest} 格）"
            );
        }
    });
}

/// 改坏确认：新岛的实体种子源掺进原岛种子源的状态（派生依赖拆合历史），或者中途并拆时
/// 漏搬一头牛的跟踪状态，「分开跑 = 一起跑」都要失败。
///
/// Break-it checks: mixing the parent island's seed state into a new island's
/// entity seed source, or losing a cow's tracking state on the mid-window
/// merge and split, must both fail the comparison.
#[test]
fn mixing_parent_seeds_or_losing_cow_tracking_is_caught() {
    with_stack(|| {
        for kind in [
            FreshEntityBreak::MixParentSeeds,
            FreshEntityBreak::LoseMobTracking,
        ] {
            let caught = std::panic::catch_unwind(|| run_apart_and_together(1, Some(kind)));
            assert!(caught.is_err(), "{kind:?} 没有被抓到");
        }
    });
}
