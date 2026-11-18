//! 多岛在运行时的验证（岛的实施计划第 6 节第 10 步、6.2）。
//!
//! 场景沿用第 9 步的离线测试（三组相隔 20 多个区块的内容，`randomTickSpeed = 0`，机器不取
//! 随机数），但这里是真的世界：`islands = true`，分区器自己把三组分成三个岛，主控在运行时
//! 照着拆合、把传送过去的玩家搬到新岛。对照是 `islands = false` 的同一个世界（整个世界一个
//! 岛）。比较按组：每组的指纹只能来自拿着这一组中心区块的那个岛（别的岛在这一组上有任何
//! 东西都算放错了地方），网络 ID 和由它派生的实体随机源不比，跨组的先后不比。
//!
//! Run-time checks for multi-island mode (island plan, step 10, 6.2). The
//! scene is step 9's (three groups twenty-odd chunks apart, no random
//! ticks, machines without randomness), but in a real world with
//! `islands = true`: the partitioner makes three islands, the master splits
//! and merges them at run time and moves teleported players. The control is
//! the same world with `islands = false`. Comparison is per group: a group's
//! fingerprint may only come from the island holding its centre chunk
//! (anything of it on another island is misplaced); network ids, the entity
//! random sources derived from them, and cross-group order are not compared.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use mc_bamboo_core::{ChunkPos, Vec3};
use mc_bamboo_protocol::packet::play::GameMode;
use mc_bamboo_world::director::{HOME_REGION, Owner, RegionId, TicketOp};

use super::{
    island_split_tests::{
        Scene, fingerprint, furnish_scene, furnish_scene_with_sand, print, rng, set_up, with_stack,
    },
    tests::{Client, Frame, Harness},
};
use crate::{island::IslandDrive, region::IslandPart};

/// 一个多岛世界按组拼起来的指纹（见模块说明），外加每个岛的时钟。别的岛上出现了某一组的
/// 东西就 panic，报出是哪一类。
///
/// The per-group fingerprint of a multi-island world (see the module docs),
/// plus every island's clock. Panics, naming the class, if a group's
/// content shows up on another island.
fn print_islands(scene: &mut Scene) -> (Vec<String>, BTreeSet<i64>) {
    let Scene {
        server,
        centers,
        hashes,
        ..
    } = scene;
    let groups = centers.len();
    let owners: Vec<RegionId> = centers
        .iter()
        .map(|&center| {
            server
                .world
                .islands
                .island_of_chunk(center)
                .expect("每组的中心区块都加载着")
        })
        .collect();
    let mut out = vec![String::new(); groups];
    let mut clocks = BTreeSet::new();
    for island in server.world.islands.iter() {
        let print = fingerprint(&island.region, centers, false, hashes);
        for group in 0..groups {
            if owners[group] == island.id {
                out[group] = print[group].clone();
            } else if !print[group].is_empty() {
                let first: Vec<&str> = print[group].lines().take(3).collect();
                panic!(
                    "岛 {} 上有第 {group} 组的东西（那一组在岛 {}）：{first:?}",
                    island.id, owners[group]
                );
            }
        }
        clocks.insert(island.region.tick.game_time());
    }
    (out, clocks)
}

/// 对照（一个岛）的按组指纹，去掉全岛共有的那一份。
fn print_control(scene: &mut Scene) -> Vec<String> {
    let mut out = print(scene, false);
    out.pop();
    out
}

/// 两个世界跑到同一个 `director_tick`，再一起空跑几刻，丢掉期间收到的包：之后两边的
/// 时钟相同，放机器的时刻也相同。
///
/// Run both worlds to the same `director_tick`, idle a few ticks together and
/// drop what the clients received: the clocks match from here on.
fn align(a: &mut Scene, b: &mut Scene) {
    loop {
        let (left, right) = (
            a.server.world.master.director_tick,
            b.server.world.master.director_tick,
        );
        if left == right {
            break;
        }
        if left < right {
            a.server.tick();
        } else {
            b.server.tick();
        }
    }
    for _ in 0..3 {
        a.server.tick();
        b.server.tick();
    }
    for (_, client) in a.clients.iter().chain(&b.clients) {
        let _ = client.take_frames();
    }
}

/// 把全部岛的随机源固定成种子 0（测试脚手架只对家岛这么做）。派生的岛后来合并时留
/// 吸收方的随机源，值一样，所以合并前后仍是同一条序列。
///
/// Fix every island's random sources to seed 0 (the harness only does the home
/// island this way). A merge keeps the absorber's sources, which are identical,
/// so the sequence is the same before and after.
fn fix_island_randoms(scene: &mut Scene) {
    for island in scene.server.world.islands.iter_mut() {
        island.region.random = mc_bamboo_blocks::LevelRandom::new(0);
        island.region.sound_seed_generator = mc_bamboo_blocks::LevelRandom::new(0);
    }
    // 多岛时天气用主控自己的随机源（计划 3.3 第 4 条）；正式启动它的种子来自时钟，
    // 对照场景里同样固定成 0，和脚手架对家岛做的一样。
    //
    // With several islands the weather uses the master's own random source
    // (plan 3.3 item 4); its seed comes from the clock on a real start, so
    // fix it to 0 here too, just as the harness fixes the home island's.
    scene.server.world.master.weather_random = mc_bamboo_blocks::LevelRandom::new(0);
}

/// 用旁观者模式摆场景：官方旁观者打开容器不计数、盖子不动也不响，所以「开箱子菜单」
/// 这一步不从岛的随机源取音高。计划 6.2 第 2 条要求对照场景不取随机数；多岛时每个岛
/// 的随机源由起源 section 派生，和对照那一个岛的序列本来就不一样，少了这次取数才能
/// 逐字节比包。摆完恢复原来的模式。
///
/// Furnish as a spectator: vanilla skips the chest-open sound (and opener
/// count) for spectators, so opening the menus draws no pitch from the
/// island's random source. Plan 6.2 item 2 needs a scene without random
/// draws; every island derives its own source from its origin section, which
/// cannot match the control's single sequence, so this is required for a
/// byte-for-byte comparison. The original game modes are restored afterwards.
fn furnish_without_random(scene: &mut Scene, sand_delay: i32) {
    let mut modes = BTreeMap::new();
    for island in scene.server.world.islands.iter_mut() {
        for (id, player) in island.region.players.iter_mut() {
            modes.insert(*id, player.game_mode);
            player.game_mode = GameMode::Spectator;
        }
    }
    furnish_scene_with_sand(scene, sand_delay);
    for island in scene.server.world.islands.iter_mut() {
        for (id, player) in island.region.players.iter_mut() {
            player.game_mode = modes[id];
        }
    }
}

/// 让分区器当场做一次拆合（不等迟滞），再照做：有两个以上的岛时随机并两个，不然拆开
/// 一个里面有好几组的岛。返回做的是合并还是拆分。
///
/// Make the partitioner split or merge now (no hysteresis) and apply it.
fn force_topology(scene: &mut Scene, next: &mut impl FnMut() -> u64) -> Option<bool> {
    let world = &mut scene.server.world;
    let partition = world.master.host.partition_mut().expect("多岛开着");
    let ids = partition.island_ids();
    let mut merged = None;
    if ids.len() >= 2 && next().is_multiple_of(2) {
        let from = ids[1 + (next() as usize % (ids.len() - 1))];
        if partition.force_merge(ids[0], from) {
            merged = Some(true);
        }
    } else {
        for &id in &ids {
            if !partition.force_split(id).is_empty() {
                merged = Some(false);
                break;
            }
        }
        if merged.is_none() && ids.len() >= 2 && partition.force_merge(ids[0], ids[1]) {
            merged = Some(true);
        }
    }
    world.master.host.take_partition_ops();
    world.settle_islands();
    merged
}

/// 多岛世界和对照一起跑 `ticks` 刻，随机时刻强制拆合；每刻比按组指纹、时钟和每名玩家
/// 收到的字节。返回做了几次（合并，拆分）。
///
/// Run a multi-island world beside its control, forcing splits and merges at
/// random ticks; compare the per-group fingerprints, the clocks and every
/// player's bytes each tick. Returns the (merge, split) counts.
fn run_against_control(seed: u64, ticks: u64, skip: Option<IslandPart>) -> (usize, usize) {
    let mut multi = set_up(seed, Harness::new_islands(2, 0));
    let mut control = set_up(
        seed,
        Harness::new_loaded_with_drive(2, IslandDrive::Lockstep),
    );
    align(&mut multi, &mut control);
    assert_eq!(
        multi.server.world.islands.len(),
        multi.centers.len(),
        "seed {seed}：三组各自一个岛"
    );
    // 摆场景时打开箱子菜单会发开箱音效，音高取自所在岛的 `Level.random`；每个多岛的
    // 随机源由起源 section 派生，和对照那一个岛的序列不一样。计划 6.2 第 2 条要求对照
    // 场景不取随机数，所以两边都用旁观者模式摆场景（官方不响这声），并把全部随机源固定
    // 成种子 0（和脚手架建世界时做的一样）：之后的每刻仍逐字节比全部包，不丢包、不跳过
    // 任何一刻。拆合时吸收方留自己的随机源，值一样。
    //
    // Furnishing opens chest menus whose pitch is drawn from that island's
    // `Level.random`; every multi island derives its own from its origin
    // section, unlike the control's single sequence. Plan 6.2 item 2 needs a
    // scene without random draws, so furnish both sides in spectator mode
    // (vanilla skips that sound) and fix every source to seed 0, as the harness
    // does when building a world. Every later tick still compares all bytes,
    // dropping no frame and skipping no tick. A merge keeps the absorber's
    // sources, which are identical.
    fix_island_randoms(&mut multi);
    fix_island_randoms(&mut control);
    // 悬空沙子的计划刻推到比较窗口之后：那一刻它会生成新的下落方块实体，而实体网络 ID
    // 来自全进程共享的原子分配器。多岛按岛编号轮流驱动，同一批新实体领号的顺序和单岛
    // 不同，包里的 ID 就不可能逐字节相同；计划 6.2 第 1 条明确「实体网络 ID 不进指纹」，
    // 6.2 第 2 条的对照场景也不取随机数。沙子方块和这条计划刻仍在场景里参与拆合与指纹，
    // 只是不在比较窗口里到期。含新生成实体的情形由另一条测试按自己的口径覆盖。
    //
    // Push the sand's scheduled tick past the comparison window: it spawns a
    // new falling-block entity, and entity network ids come from the shared
    // process-wide allocator. Islands are driven in id order, so those ids are
    // taken in a different order than on one island and the id bytes cannot
    // match; plan 6.2 item 1 explicitly leaves network ids out of the
    // fingerprint, and item 2 asks for a scene without random draws. The sand
    // block and its tick still take part in splits and in the print, they just
    // do not come due during the window. Fresh entities are covered by a
    // separate test with its own comparison rules.
    let late_sand = i32::try_from(ticks + 1).expect("刻数放得下 i32");
    furnish_without_random(&mut multi, late_sand);
    furnish_without_random(&mut control, late_sand);
    multi.server.world.master.split_skip = skip;
    let mut next = rng(seed ^ 0x7E57);
    let trips: BTreeSet<u64> = (0..6).map(|_| next() % ticks).collect();
    let (mut merges, mut splits) = (0, 0);
    for tick in 0..ticks {
        if trips.contains(&tick) {
            match force_topology(&mut multi, &mut next) {
                Some(true) => merges += 1,
                Some(false) => splits += 1,
                None => {}
            }
        }
        multi.server.tick();
        control.server.tick();
        let (groups, clocks) = print_islands(&mut multi);
        let expected = print_control(&mut control);
        for (group, (left, right)) in groups.iter().zip(&expected).enumerate() {
            assert_eq!(left, right, "seed {seed} tick {tick}：第 {group} 组的内容");
        }
        let clock = control.server.world.islands.region.tick.game_time();
        assert_eq!(
            clocks,
            BTreeSet::from([clock]),
            "seed {seed} tick {tick}：岛的时钟"
        );
        for ((_, mine), (_, theirs)) in multi.clients.iter().zip(&control.clients) {
            assert_eq!(
                mine.take_frames(),
                theirs.take_frames(),
                "seed {seed} tick {tick}：玩家 {:?} 收到的字节",
                mine.id
            );
        }
    }
    // 比较窗口结束后，让对照世界把推迟的那条沙子刻跑完、沙子落地，再照第 9 步的原样
    // 检查机器还活着。这几刻在窗口之外（和开头的 `align` 一样），不属于逐字节对照；
    // 窗口里的每一刻仍逐字节比过了。
    //
    // After the comparison window, let the control run its postponed sand tick
    // and land the sand, then check the machines are still alive exactly as
    // step 9 does. These ticks are outside the window (like `align` at the
    // start) and are not byte-compared; every tick inside the window was.
    for _ in 0..late_sand + 20 {
        control.server.tick();
    }
    for (_, client) in &control.clients {
        let _ = client.take_frames();
    }
    super::island_split_tests::assert_alive(&control);
    (merges, splits)
}

/// 计划 6.2 第 2 条「拆合不改内容」（运行时）：分区器把三组分成三个岛，随机时刻强制拆合，
/// 齐步驱动下每组的内容、每个岛的时钟、每名玩家收到的字节都和整个世界一个岛时逐刻相同。
///
/// Plan 6.2 item 2 at run time: the partitioner makes three islands, splits
/// and merges are forced at random ticks, and under the lockstep drive every
/// group's content, every island clock and every player's bytes match the
/// single-island world tick by tick.
#[test]
fn runtime_splits_and_merges_keep_every_group_as_on_one_island() {
    with_stack(|| {
        let (mut merges, mut splits) = (0, 0);
        for seed in 1..=3_u64 {
            let (m, s) = run_against_control(seed, 40, None);
            merges += m;
            splits += s;
        }
        assert!(
            merges > 0 && splits > 0,
            "合并 {merges} 次、拆分 {splits} 次"
        );
    });
}

/// 改坏确认：运行时拆岛漏搬任何一类（23 类），按组拼指纹时都要报出来（那一组的东西
/// 留在了别的岛上），或者之后某一刻的内容对不上。
///
/// Break-it check: leaving any of the 23 classes behind on a run-time split
/// must be caught (that group's content stays on another island) or show
/// up as a content difference later.
#[test]
fn leaving_any_class_behind_on_a_runtime_split_is_caught() {
    with_stack(|| {
        for part in IslandPart::ALL {
            let caught = std::panic::catch_unwind(|| run_against_control(2, 24, Some(part)));
            assert!(caught.is_err(), "运行时拆岛漏搬「{part:?}」没有被抓到");
        }
    });
}

/// 一个世界里在场实体的网络 ID 集合，以及每名玩家的（包 ID，包体长度）序列。含新生成
/// 实体的对照按这三项比（见 `fresh_entities_survive_runtime_splits_and_merges`）。
///
/// The set of live entity network ids in a world and each player's sequence of
/// (packet id, body length), for the fresh-entity comparison (see
/// `fresh_entities_survive_runtime_splits_and_merges`).
fn fresh_state(scene: &mut Scene) -> (BTreeSet<i32>, Vec<Vec<(i32, usize)>>) {
    let mut all = BTreeSet::new();
    let mut duplicates = Vec::new();
    for island in scene.server.world.islands.iter() {
        for id in island.region.entities.ids() {
            if !all.insert(id) {
                duplicates.push(id);
            }
        }
    }
    assert!(duplicates.is_empty(), "实体网络 ID 重复：{duplicates:?}");
    let frames: Vec<Vec<(i32, usize)>> = scene
        .clients
        .iter()
        .map(|(_, client)| {
            client
                .take_frames()
                .iter()
                .map(|Frame { id, body }| (*id, body.len()))
                .collect()
        })
        .collect();
    (all, frames)
}

/// 计划 6.2 第 1、2 条的补集：比较窗口里真的会新生成实体（悬空沙子落地变成下落方块），
/// 拆合也照旧随机发生。实体网络 ID 来自全进程共享的原子分配器，多岛按岛编号轮流驱动时
/// 领号的先后和单岛不同，所以这里（且只在这里）按三条口径比：按组内容指纹和岛时钟必须
/// 完全相等（计划 6.2 第 1 条明确网络 ID 不进指纹）；两边在场实体的 ID 集合每刻相等且
/// 各自不重复；每名玩家收到的包只比「包 ID + 包长度」序列，必须完全相等。不比包体字节
/// 是因为包里就差那个共享计数器给的 ID；ID 都从 `1 << 24` 起，varint 长度相同，长度序列
/// 对不上就是真问题。这条测试不替代上一条逐字节测试，两条各自独立。
///
/// Complement to plan 6.2 items 1 and 2: the window really spawns fresh
/// entities (sand turns into falling blocks) while splits and merges happen.
/// Entity network ids come from a process-wide shared allocator, so islands
/// driven in id order take them in a different order than one island. This
/// test, and only this test, compares three things: per-group fingerprints and
/// island clocks must be exactly equal (plan 6.2 item 1 leaves network ids out
/// of the fingerprint); the set of live entity ids must be equal each tick and
/// unique within each world; and each player's packets must have exactly equal
/// (packet id, body length) sequences. Body bytes are not compared here only
/// because they differ by that shared-counter id; all ids start at `1 << 24`
/// and have the same varint length, so a length mismatch is a real bug. This
/// test does not replace the byte-for-byte one above; both stand on their own.
fn run_fresh_entities(
    seed: u64,
    ticks: u64,
    break_kind: Option<super::island_runtime::FreshEntityBreak>,
) -> (usize, usize) {
    let mut multi = set_up(seed, Harness::new_islands(2, 0));
    let mut control = set_up(
        seed,
        Harness::new_loaded_with_drive(2, IslandDrive::Lockstep),
    );
    align(&mut multi, &mut control);
    assert_eq!(
        multi.server.world.islands.len(),
        multi.centers.len(),
        "seed {seed}：三组各自一个岛"
    );
    // 和逐字节对照同一套场景准备：不取随机数的机器、旁观者摆菜单、随机源固定成 0。
    // 沙子按原来的 delay = 2 落地，比较窗口里因此会有新实体。
    //
    // The same scene preparation as the byte-for-byte comparison: random-free
    // machines, spectator-mode furnishing, sources fixed to 0. The sand keeps
    // its original delay of 2, so fresh entities appear inside the window.
    fix_island_randoms(&mut multi);
    fix_island_randoms(&mut control);
    furnish_without_random(&mut multi, 2);
    furnish_without_random(&mut control, 2);
    // 摆场景直接登记了方块实体，还没跑过一刻的话方块实体表不在 tick 边界（`due` 非空），
    // 这时拆合会撞上「只能拆停放着的岛」的前提。两边先各跑一刻、丢掉这一刻的包：之后
    // 是真正的 tick 边界，而沙子的 delay = 2 还没到期，新实体仍在比较窗口里生成。
    //
    // Furnishing registers block entities directly, so before any tick the
    // block-entity table is not yet at a tick boundary (`due` is non-empty)
    // and a split or merge would violate its precondition. Run one settling
    // tick on both worlds and drop its frames: from here it is a real tick
    // boundary, and the sand's delay of 2 has not come due yet, so the fresh
    // entities still spawn inside the comparison window.
    multi.server.tick();
    control.server.tick();
    for (_, client) in multi.clients.iter().chain(&control.clients) {
        let _ = client.take_frames();
    }
    multi.server.world.master.split_break = break_kind;
    let mut next = rng(seed ^ 0x7E57);
    let trips: BTreeSet<u64> = (0..6).map(|_| next() % ticks).collect();
    let (mut merges, mut splits) = (0, 0);
    let mut seen_fresh = false;
    let mut last_peak = multi
        .server
        .world
        .islands
        .get(HOME_REGION)
        .expect("家岛")
        .region
        .entities
        .ids_handle()
        .peek();
    for tick in 0..ticks {
        if trips.contains(&tick) {
            match force_topology(&mut multi, &mut next) {
                Some(true) => merges += 1,
                Some(false) => splits += 1,
                None => {}
            }
        }
        multi.server.tick();
        control.server.tick();
        let (groups, clocks) = print_islands(&mut multi);
        let expected = print_control(&mut control);
        for (group, (left, right)) in groups.iter().zip(&expected).enumerate() {
            assert_eq!(left, right, "seed {seed} tick {tick}：第 {group} 组的内容");
        }
        let clock = control.server.world.islands.region.tick.game_time();
        assert_eq!(
            clocks,
            BTreeSet::from([clock]),
            "seed {seed} tick {tick}：岛的时钟"
        );
        let (multi_ids, multi_frames) = fresh_state(&mut multi);
        let (control_ids, control_frames) = fresh_state(&mut control);
        assert_eq!(
            multi_ids, control_ids,
            "seed {seed} tick {tick}：在场实体的网络 ID 集合"
        );
        for ((_, client), (mine, theirs)) in multi
            .clients
            .iter()
            .zip(multi_frames.iter().zip(&control_frames))
        {
            assert_eq!(
                mine, theirs,
                "seed {seed} tick {tick}：玩家 {:?} 的包 ID 与长度序列",
                client.id
            );
        }
        // 共享分配器的号只增不减：窗口里领过新号就说明真的生成过新实体。
        // The shared allocator only grows: a new peak means fresh entities
        // were really spawned inside the window.
        let peak = multi
            .server
            .world
            .islands
            .get(HOME_REGION)
            .expect("家岛")
            .region
            .entities
            .ids_handle()
            .peek();
        seen_fresh |= peak > last_peak;
        last_peak = peak;
    }
    assert!(seen_fresh, "seed {seed}：窗口里真的生成了新实体");
    (merges, splits)
}

/// 含新生成实体的对照（见 [`run_fresh_entities`]）：三个种子、随机时刻强制拆合，逐刻比
/// 按组内容、岛时钟、实体 ID 集合和（包 ID，包长度）序列。
///
/// Fresh-entity comparison (see [`run_fresh_entities`]): three seeds, forced
/// splits and merges at random ticks, per-tick checks of group content,
/// island clocks, entity id sets and (packet id, length) sequences.
#[test]
fn fresh_entities_survive_runtime_splits_and_merges() {
    with_stack(|| {
        let (mut merges, mut splits) = (0, 0);
        for seed in 1..=3_u64 {
            let (m, s) = run_fresh_entities(seed, 24, None);
            merges += m;
            splits += s;
        }
        assert!(
            merges > 0 && splits > 0,
            "合并 {merges} 次、拆分 {splits} 次"
        );
    });
}

/// 改坏确认（含新实体）：拆合后故意丢掉一个新生成的实体、或者清掉它的追踪配对，
/// 上面的对照都要报出来（内容指纹、ID 集合或包序列对不上）。
///
/// Break-it check with fresh entities: dropping a newly spawned entity or
/// clearing its tracking entry after a split or merge must be caught by the
/// comparison above (content fingerprint, id set or packet sequence differ).
#[test]
fn losing_a_fresh_entity_or_its_pairing_is_caught() {
    use super::island_runtime::FreshEntityBreak;
    with_stack(|| {
        for kind in [FreshEntityBreak::LoseEntity, FreshEntityBreak::LoseTracking] {
            let caught = std::panic::catch_unwind(|| run_fresh_entities(2, 16, Some(kind)));
            assert!(caught.is_err(), "拆合后弄坏「{kind:?}」没有被抓到");
        }
    });
}

/// 计划 6.2 第 4 条「线程数无关」：没有跨岛交互的场景里，分离驱动 1 / 4 / 8 条 tick 线程，
/// 每组的内容逐刻相同（各岛的一刻在不同的线程上并行跑）。
///
/// Plan 6.2 item 4: without cross-island interaction, the detached drive with
/// 1, 4 and 8 tick threads gives the same per-group content every tick.
#[test]
fn island_content_does_not_depend_on_the_thread_count() {
    with_stack(|| {
        let seed = 3;
        let mut worlds: Vec<Scene> = [1, 4, 8]
            .into_iter()
            .map(|threads| set_up(seed, Harness::new_islands(2, threads)))
            .collect();
        let target = worlds
            .iter()
            .map(|scene| scene.server.world.master.director_tick)
            .max()
            .expect("三份");
        for scene in &mut worlds {
            while scene.server.world.master.director_tick < target {
                scene.server.tick();
            }
            for _ in 0..3 {
                scene.server.tick();
            }
            furnish_scene(scene);
        }
        for tick in 0..30 {
            let mut prints = Vec::new();
            for scene in &mut worlds {
                scene.server.tick();
                prints.push(print_islands(scene).0);
            }
            assert_eq!(prints[0], prints[1], "tick {tick}：1 条和 4 条线程");
            assert_eq!(prints[0], prints[2], "tick {tick}：1 条和 8 条线程");
        }
    });
}

/// 计划 6.2 第 1 条「分开跑 = 一起跑」：三组在一个世界里（三个岛，齐步驱动），和每组单独
/// 起一个世界（只有这一组的玩家和机器）跑同样的刻数，每组的内容逐刻相同。单独那份的岛
/// 随机源和一起跑时这一组的岛不同，场景不取随机数，所以照样相同。
///
/// Plan 6.2 item 1: three groups in one world (three islands, lockstep)
/// against each group alone in its own world; every group's content matches
/// tick by tick (the scene draws no random numbers, so the different island
/// random sources do not matter).
#[test]
fn islands_run_together_equal_each_group_run_alone() {
    with_stack(|| {
        let seed = 1;
        let mut together = set_up(seed, Harness::new_islands(2, 0));
        let groups = together.centers.len();
        let mut alone: Vec<Scene> = (0..groups).map(|group| set_up_alone(seed, group)).collect();
        let target = alone
            .iter()
            .map(|scene| scene.server.world.master.director_tick)
            .chain([together.server.world.master.director_tick])
            .max()
            .expect("有世界");
        for scene in alone.iter_mut().chain([&mut together]) {
            while scene.server.world.master.director_tick < target {
                scene.server.tick();
            }
            for _ in 0..3 {
                scene.server.tick();
            }
            furnish_scene(scene);
        }
        for tick in 0..30 {
            together.server.tick();
            let (prints, _) = print_islands(&mut together);
            for (group, scene) in alone.iter_mut().enumerate() {
                scene.server.tick();
                let (own, _) = print_islands(scene);
                assert_eq!(
                    prints[group], own[group],
                    "tick {tick}：第 {group} 组一起跑和单独跑"
                );
            }
        }
    });
}

/// 只有一组的世界：玩家照样从出生点进服、放到这一组的中心；别的组不放玩家，中心区块
/// 用强加载票留着（让「每组中心都加载着」的指纹照样能拼），机器只放这一组。
fn set_up_alone(seed: u64, group: usize) -> Scene {
    let mut scene = set_up(seed, Harness::new_islands(2, 0));
    let centers = scene.centers.clone();
    let others: Vec<Client> = scene
        .clients
        .iter()
        .filter(|(index, _)| *index != group)
        .map(|(_, client)| client.clone())
        .collect();
    for (index, &center) in centers.iter().enumerate() {
        if index != group {
            scene.server.apply_ticket_for_test(TicketOp::Forced {
                pos: center,
                forced: true,
            });
        }
    }
    for client in &others {
        scene.server.leave(client);
    }
    scene.clients.retain(|(index, _)| *index == group);
    for _ in 0..4 {
        scene.server.tick();
    }
    scene
}

/// 分离驱动：投给一个岛、它还没收的信（主控在两刻之间投的），在它被并进别的岛、或者它
/// 的一块被拆出去以后，照样交付到现在拿着那块的岛，一封不丢，Director 记的归属跟着改。
///
/// Detached drive: letters posted to an island it has not taken yet still
/// reach whichever island holds those chunks after a merge or a split; none
/// is lost and the Director's owners follow.
#[test]
fn letters_in_flight_follow_their_chunks_through_merges_and_splits() {
    with_stack(|| {
        let mut scene = set_up(4, Harness::new_islands(2, 1));
        let world = &mut scene.server.world;
        let ids = world
            .master
            .host
            .partition_mut()
            .expect("多岛")
            .island_ids();
        assert!(ids.len() >= 2, "至少两个岛");
        // 两个岛各挂一张强加载票，信投进邮筒，岛下一刻开头才收。
        // A forced ticket on each of two islands; the letters wait in the postbox.
        let centers = scene.centers.clone();
        let near: Vec<ChunkPos> = centers
            .iter()
            .take(2)
            .map(|center| ChunkPos::new(center.x + 3, center.z + 3))
            .collect();
        for &pos in &near {
            scene
                .server
                .apply_ticket_for_test(TicketOp::Forced { pos, forced: true });
        }
        let world = &mut scene.server.world;
        let owner_a = world.master.host.island_of(near[0]).expect("有岛");
        let owner_b = world.master.host.island_of(near[1]).expect("有岛");
        assert_ne!(owner_a, owner_b);
        let posted_b = world.master.host.posted_to(owner_b);
        assert!(posted_b > 0, "第二个岛的信还在邮筒里");
        // 合并：B 的信并到 A 的邮筒。
        // Merge: B's letters join A's postbox.
        let (into, from) = (owner_a.min(owner_b), owner_a.max(owner_b));
        let partition = world.master.host.partition_mut().expect("多岛");
        assert!(partition.force_merge(into, from));
        world.master.host.take_partition_ops();
        world.settle_islands();
        assert_eq!(world.master.host.posted_to(from), 0, "被吸收方的邮筒空了");
        assert!(!world.islands.contains(from), "被吸收方没了");
        scene.server.tick();
        scene.server.tick();
        for &pos in &near {
            let at = scene.server.world.islands.island_of_chunk(pos);
            assert_eq!(at, Some(into), "{pos:?} 交付到吸收方");
            assert_eq!(
                scene.server.world.master.host.director().owner_at(pos),
                Some(Owner::Region(into))
            );
        }
        // 拆分：再挂一张票，信还在邮筒里时把那一块拆出去，信跟着去新岛。
        // Split: post again and split the part off while the letters wait.
        let extra = ChunkPos::new(near[1].x + 1, near[1].z);
        scene.server.apply_ticket_for_test(TicketOp::Forced {
            pos: extra,
            forced: true,
        });
        let world = &mut scene.server.world;
        assert!(world.master.host.posted_to(into) > 0);
        let made = world
            .master
            .host
            .partition_mut()
            .expect("多岛")
            .force_split(into);
        assert!(!made.is_empty(), "能拆开");
        world.master.host.take_partition_ops();
        world.settle_islands();
        scene.server.tick();
        scene.server.tick();
        let world = &mut scene.server.world;
        let now_at = world.master.host.island_of(extra).expect("有岛");
        assert_eq!(world.islands.island_of_chunk(extra), Some(now_at));
        assert_eq!(
            world.master.host.director().owner_at(extra),
            Some(Owner::Region(now_at))
        );
        world.assert_island_invariants();
    });
}

/// 传送到另一个岛：玩家只在一个岛上（离境以后、到达之前也不会两边都有或两边都没有），
/// 传送之后的那一刻原来的岛把他的视野和配对清干净、主控把他搬到新岛；在途中掉线，两边
/// 都不留他，票也撤掉。
///
/// A teleport to another island: the player is on exactly one island at
/// every hand-back; the old island clears them the tick after and the master
/// moves them; disconnecting midway leaves them on neither island and drops
/// the ticket.
#[test]
fn a_player_crossing_islands_is_always_on_exactly_one_island() {
    with_stack(|| {
        for threads in [0, 1] {
            let mut scene = set_up(5, Harness::new_islands(2, threads));
            let (_, traveller) = scene.clients[0].clone();
            let target = scene.centers[1];
            let start = scene
                .server
                .world
                .islands
                .island_of_player(traveller.id)
                .expect("在世界里");
            let to = Vec3::new(
                f64::from(target.x * 16) + 3.5,
                -59.0,
                f64::from(target.z * 16) + 3.5,
            );
            scene.server.move_to_position(&traveller, to);
            let mut seen = Vec::new();
            for _ in 0..4 {
                scene.server.tick();
                let world = &scene.server.world;
                let holders: Vec<RegionId> = world
                    .islands
                    .iter()
                    .filter(|island| island.region.players.contains_key(&traveller.id))
                    .map(|island| island.id)
                    .collect();
                assert_eq!(holders.len(), 1, "threads {threads}：玩家只在一个岛上");
                seen.push(holders[0]);
            }
            let end = *seen.last().expect("跑过");
            assert_ne!(end, start, "threads {threads}：搬到了目的地的岛");
            assert_eq!(
                Some(end),
                scene.server.world.islands.island_of_chunk(target),
                "threads {threads}"
            );
            // 再传送回去，这一刻还没搬就掉线。
            // Teleport back and disconnect before the move.
            let home = scene.centers[0];
            scene.server.move_to_position(
                &traveller,
                Vec3::new(
                    f64::from(home.x * 16) + 3.5,
                    -59.0,
                    f64::from(home.z * 16) + 3.5,
                ),
            );
            scene.server.leave(&traveller);
            scene.server.tick();
            scene.server.tick();
            let world = &mut scene.server.world;
            assert!(
                world.islands.island_of_player(traveller.id).is_none(),
                "threads {threads}：掉线以后哪个岛都没有他"
            );
            assert!(!world.master.transit.contains_key(&traveller.id));
            assert!(
                !world.master.ticket_centers.contains_key(&traveller.id),
                "threads {threads}：他的票撤了"
            );
            world.assert_island_invariants();
        }
    });
}

/// 关服的时候还有没照做的拆合：先照做完，关服的收回投到对的岛，每个岛上改过的区块都
/// 存下来，锁放掉，同目录马上能重开。
///
/// Shutdown with a split or merge still pending: it is applied first, the
/// shutdown reclaims reach the right islands, every island's edited chunks
/// are saved, the lock is released and the directory reopens at once.
#[test]
fn shutdown_with_a_pending_split_or_merge_saves_every_island() {
    with_stack(|| {
        for merge in [true, false] {
            let root = std::env::temp_dir().join(format!(
                "bamboo-island-shutdown-{}-{merge}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            let centers;
            {
                let mut scene = set_up(6, Harness::new_islands_with_save(2, &root, 1));
                centers = scene.centers.clone();
                for &center in &centers {
                    let pos = super::island_split_tests::at(center, 4, -60, 4);
                    let island = scene
                        .server
                        .world
                        .islands
                        .island_of_chunk(center)
                        .expect("有岛");
                    scene
                        .server
                        .world
                        .islands
                        .get_mut(island)
                        .expect("在表里")
                        .region
                        .chunks
                        .set_block(pos, mc_bamboo_data::blocks::GOLD_BLOCK);
                }
                let world = &mut scene.server.world;
                let partition = world.master.host.partition_mut().expect("多岛");
                let ids = partition.island_ids();
                if merge {
                    assert!(partition.force_merge(ids[0], ids[1]));
                } else {
                    assert!(partition.force_merge(ids[0], ids[1]));
                    world.master.host.take_partition_ops();
                    world.settle_islands();
                    let partition = world.master.host.partition_mut().expect("多岛");
                    assert!(!partition.force_split(ids[0]).is_empty());
                }
                world.master.host.take_partition_ops();
                assert!(world.master.host.has_topology(), "拆合还没照做");
                scene.server.shut_down();
            }
            // 同目录马上重开（锁放掉了），每组改过的那一格都在盘上。
            // Reopen at once (the lock is free); every group's edit is on disk.
            let mut reopened = Harness::new_loaded_with_save(2, &root);
            for &center in &centers {
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
                    "merge={merge}：{center:?} 的改动存下来了"
                );
            }
            drop(reopened);
            let _ = std::fs::remove_dir_all(&root);
        }
    });
}

/// 分离驱动、多岛：不是家岛的岛在 tick 线程上 panic，世界线程照第 7 步的路径原样抛出，
/// 抛出之前所有岛都已经交回（主控等全部岛回来再抛，不会有岛留在线程上）。
///
/// Detached multi-island: a non-home island panicking on its tick thread is
/// re-raised on the world thread as in step 7, after every island came back.
#[test]
fn a_panicking_island_is_raised_after_every_island_is_back() {
    with_stack(|| {
        let mut scene = set_up(7, Harness::new_islands(2, 4));
        let world = &mut scene.server.world;
        let victim = world
            .islands
            .ids()
            .into_iter()
            .find(|&id| id != HOME_REGION)
            .expect("有别的岛");
        world.islands.get_mut(victim).expect("在表里").test_hook =
            Some(crate::island::IslandTestHook::Panic);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = scene.server.world.tick();
        }));
        assert!(outcome.is_err(), "panic 要在世界线程上抛出来");
        assert!(scene.server.world.islands.all_parked(), "所有岛都交回了");
    });
}

/// 分离驱动、多岛：一个岛卡住时主控一直等（不会把别的岛丢下先走）；收到关服请求后等够
/// 超时就不等了，已经交回的岛照常处理、世界循环结束，不会死锁。
///
/// Detached multi-island: with one island stuck the master keeps waiting;
/// after a shutdown request it gives up past the timeout and the loop ends.
#[test]
fn a_stuck_island_never_deadlocks_the_master() {
    with_stack(|| {
        let mut scene = set_up(8, Harness::new_islands(2, 4));
        let world = &mut scene.server.world;
        world.master.island_stop_timeout = Duration::from_millis(300);
        let victim = world
            .islands
            .ids()
            .into_iter()
            .find(|&id| id != HOME_REGION)
            .expect("有别的岛");
        let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
        world.islands.get_mut(victim).expect("在表里").test_hook =
            Some(crate::island::IslandTestHook::Stall(Arc::clone(&release)));
        let sender = scene.server.inbox_sender();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            sender.send(crate::inbound::Inbound::Shutdown).unwrap();
        });
        let started = std::time::Instant::now();
        let flow = scene.server.world.tick();
        assert!(flow.is_break(), "关服请求之后世界循环结束");
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "没有死锁：{:?}",
            started.elapsed()
        );
        release.store(true, std::sync::atomic::Ordering::Release);
        stopper.join().unwrap();
    });
}

/// 计划 6.2 第 3 条「不变量压力测试」：随机走动、随机传送、随机进出服的假玩家，齐步和
/// 分离驱动 1 / 4 / 8 条 tick 线程，每刻交回之后断言全部不变量（不同岛的已交付区块不相邻；
/// 每个已交付区块恰好在一个岛、Director 记的归属就是它；每名玩家恰好在一个岛、不在跨岛
/// 途中时在他所在区块的岛里；玩家跟踪的区块都在他的岛里；实体网络 ID 唯一；停放着的岛
/// 没有被派出去）。
///
/// Plan 6.2 item 3: random walks, teleports, joins and leaves under lockstep
/// and 1 / 4 / 8 tick threads; every invariant is asserted after each tick.
#[test]
fn random_players_keep_every_island_invariant() {
    with_stack(|| {
        for (seed, threads) in [(11_u64, 0), (12, 1), (13, 4), (14, 8)] {
            stress(seed, threads, 160);
        }
    });
}

/// 压力测试的一次运行（见 [`random_players_keep_every_island_invariant`]）。
fn stress(seed: u64, threads: usize, ticks: u64) {
    let mut server = Harness::new_islands(2, threads);
    let mut next = rng(seed);
    let mut online: BTreeMap<usize, Client> = BTreeMap::new();
    let mut counter = 0;
    let spots: Vec<(f64, f64)> = vec![(8.5, 8.5), (400.5, 8.5), (8.5, -420.5), (-380.5, 390.5)];
    for tick in 0..ticks {
        match next() % 10 {
            0 | 1 if online.len() < 6 => {
                counter += 1;
                let client = server.join(&format!("s{counter}"));
                server.confirm_teleport(&client);
                server.load_player(&client);
                online.insert(counter, client);
            }
            2 if !online.is_empty() => {
                let key = *online
                    .keys()
                    .nth(next() as usize % online.len())
                    .expect("有人");
                let client = online.remove(&key).expect("在表里");
                server.leave(&client);
            }
            3 | 4 if !online.is_empty() => {
                let key = *online
                    .keys()
                    .nth(next() as usize % online.len())
                    .expect("有人");
                let (x, z) = spots[next() as usize % spots.len()];
                let jitter = (next() % 24) as f64;
                let client = online[&key].clone();
                server.move_to(&client, x + jitter, z - jitter);
            }
            _ => {
                for client in online.values() {
                    if let Some(island) = server.world.islands.island_of_player(client.id) {
                        let here = server
                            .world
                            .islands
                            .get(island)
                            .expect("在表里")
                            .region
                            .players[&client.id]
                            .position();
                        let step = (next() % 5) as f64 - 2.0;
                        server.request(
                            client,
                            mc_bamboo_protocol::packet::play::PlayRequest::MovePlayer {
                                position: Some(Vec3::new(here.x + step, here.y, here.z - step)),
                                rotation: None,
                                on_ground: true,
                                horizontal_collision: false,
                            },
                        );
                    }
                }
            }
        }
        server.tick();
        for client in online.values() {
            for _ in 0..crate::world::tests::frames_with_id(
                &client.take_frames(),
                mc_bamboo_data::packets::play::clientbound::CHUNK_BATCH_FINISHED,
            )
            .len()
            {
                server.acknowledge_batch(client, 64.0);
            }
        }
        server.world.assert_island_invariants();
        for client in online.values() {
            let holders = server
                .world
                .islands
                .iter()
                .filter(|island| island.region.players.contains_key(&client.id))
                .count();
            assert!(
                holders <= 1,
                "seed {seed} threads {threads} tick {tick}：玩家 {:?} 在 {holders} 个岛上",
                client.id
            );
        }
    }
    // 最后所有人都在某个岛上（进服处理完了的）。
    // Everyone who joined is on some island by the end.
    for _ in 0..4 {
        server.tick();
    }
    for client in online.values() {
        assert!(
            server.world.islands.island_of_player(client.id).is_some() || client.is_closed(),
            "seed {seed} threads {threads}：玩家 {:?} 不见了",
            client.id
        );
    }
}
