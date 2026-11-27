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

use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::{blocks, packets::play::clientbound};
use mc_bamboo_protocol::packet::play::{ClientCommandKind, GameMode, PlayRequest};
use mc_bamboo_world::director::{HOME_REGION, Owner, RegionId, TicketOp};

use super::{
    island_split_tests::{
        Scene, Y, at, fingerprint, furnish_scene, furnish_scene_with_sand, group_of, misplaced,
        print, region_at, rng, set_up, with_stack,
    },
    tests::{Client, Frame, Harness, block_changes, frames_with_id},
};
use crate::{island::IslandDrive, profiler::Metric, region::IslandPart, world_state::RespawnData};

/// 一个多岛世界按组拼起来的指纹（见模块说明），外加每个岛的时钟。别的岛上出现了某一组的
/// 东西就 panic，报出是哪一类。
///
/// The per-group fingerprint of a multi-island world (see the module docs),
/// plus every island's clock. Panics, naming the class, if a group's
/// content shows up on another island.
pub(super) fn print_islands(scene: &mut Scene) -> (Vec<String>, BTreeSet<i64>) {
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

/// 每个岛上的每一类东西都属于这个岛拿着的组（第 9 步离线测试的按类核对 `misplaced`，
/// 用在运行时）：按位置的看位置所在区块属于哪一组，按玩家的看玩家站在哪一组，岛这一层
/// 的信箱、存盘名单也看位置。指纹不覆盖的类（红石记忆、对端 IP 等）漏搬了也报在这里，
/// 报出的是类名。
///
/// Every class on every island belongs to a group that island holds (step 9's
/// per-class `misplaced` check, at run time): positioned entries by the
/// group of their chunk, per-player entries by the group the player stands
/// in, the island-level mailbox and save lists by position. Classes the
/// fingerprint does not cover (redstone memory, peer IPs, …) are caught
/// here, by name.
pub(super) fn assert_in_place(scene: &Scene, what: &str) {
    let world = &scene.server.world;
    let centers = &scene.centers;
    let holder: Vec<RegionId> = centers
        .iter()
        .map(|&center| {
            world
                .islands
                .island_of_chunk(center)
                .expect("每组的中心区块都加载着")
        })
        .collect();
    let mut chunks = BTreeSet::new();
    let mut players = BTreeMap::new();
    for island in world.islands.iter() {
        chunks.extend(island.region.chunks.loaded_positions());
        chunks.extend(island.mailbox.positions());
        for player in island.region.players.values() {
            players.insert(player.id, group_of(centers, player.chunk_pos()));
        }
    }
    for island in world.islands.iter() {
        let mine = |group: usize| holder[group] == island.id;
        let own_chunks: BTreeSet<ChunkPos> = chunks
            .iter()
            .copied()
            .filter(|&pos| mine(group_of(centers, pos)))
            .collect();
        let own_players: BTreeSet<crate::inbound::PlayerId> = players
            .iter()
            .filter(|&(_, &group)| mine(group))
            .map(|(&id, _)| id)
            .collect();
        let wrong = misplaced(island, &own_chunks, &own_players, true);
        assert!(
            wrong.is_empty(),
            "{what}：岛 {} 上有不属于它的组的东西：{wrong:?}",
            island.id
        );
    }
}

/// 对照（一个岛）的按组指纹，去掉全岛共有的那一份。
pub(super) fn print_control(scene: &mut Scene) -> Vec<String> {
    let mut out = print(scene, false);
    out.pop();
    out
}

/// 两个世界跑到同一个 `director_tick`，再一起空跑几刻，丢掉期间收到的包：之后两边的
/// 时钟相同，放机器的时刻也相同。
///
/// Run both worlds to the same `director_tick`, idle a few ticks together and
/// drop what the clients received: the clocks match from here on.
pub(super) fn align(a: &mut Scene, b: &mut Scene) {
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

/// 把全部岛的方块随机源和音效种子源固定成种子 0（测试脚手架建世界时只对家岛这么做）。
/// 只在 [`prepare_pair`] 里、两个世界都只剩一个内容相同的岛时调用：那一刻两边的岛是同一个
/// 岛，此前的差别只来自设计允许的地方（对照的天气从岛的随机源取数，多岛的天气用主控自己的；
/// 多岛拆出来的岛按起源 section 派生），对齐成同一状态等于从这里开始对照。
///
/// Fix every island's block random and sound-seed sources to seed 0 (the
/// harness only does the home island when building a world). Only called from
/// [`prepare_pair`], when both worlds are down to one island with identical
/// content: they are the same island at that point and differ only where the
/// design allows (the control's weather draws from the island source while
/// the multi-island weather uses the master's own; split-off islands derive
/// theirs from their origin section), so aligning them is the comparison's start.
fn fix_island_randoms(scene: &mut Scene) {
    assert_eq!(
        scene.server.world.islands.len(),
        1,
        "只在一个岛时对齐随机源"
    );
    for island in scene.server.world.islands.iter_mut() {
        island.region.random = mc_bamboo_blocks::LevelRandom::new(0);
        island.region.sound_seed_generator = mc_bamboo_blocks::LevelRandom::new(0);
    }
}

/// 每个岛的方块随机源和音效种子源现在的状态（含内部种子），按岛编号。
///
/// Every island's block random and sound-seed source state (internal seed
/// included), by island id.
pub(super) fn random_states(scene: &Scene) -> BTreeMap<RegionId, String> {
    scene
        .server
        .world
        .islands
        .iter()
        .map(|island| {
            (
                island.id,
                format!(
                    "{:?} {:?}",
                    island.region.random, island.region.sound_seed_generator
                ),
            )
        })
        .collect()
}

/// 计划 6.2 第 2 条的前提「对照场景不取随机数」：这一刻前后都在的岛，随机源的状态没变
/// （合并留吸收方的、拆分时原岛留自己的，所以拆合本身不改它）。多岛时各岛的随机源本来就
/// 和对照那一个岛的不同（计划 3.3「岛的随机源」），取了数的场景不能逐字节比；这里把前提
/// 变成断言，场景以后改得取了随机数会直接报在这里。
///
/// The premise of plan 6.2 item 2, "the scene draws no random numbers", as an
/// assertion: islands present before and after a tick kept their random
/// state (a merge keeps the absorber's, a split leaves the original's, so the
/// structural changes do not touch it). Multi-island sources differ from the
/// control's by design (plan 3.3), so a scene that draws cannot be compared
/// byte for byte; a future scene change that draws fails here.
pub(super) fn assert_no_draws(
    before: &BTreeMap<RegionId, String>,
    after: &BTreeMap<RegionId, String>,
    what: &str,
) {
    for (id, state) in before {
        if let Some(now) = after.get(id) {
            assert_eq!(state, now, "{what}：岛 {id} 的随机源在比较窗口里被取过数");
        }
    }
}

/// 两个世界的场景准备（严格对照和含新实体对照共用）：多岛那份先把全部岛并成一个，两边
/// 对齐随机源、摆好同样的机器和菜单（这时两边都是一个岛，摆的过程逐位相同，开箱音效
/// 取的音高也相同），然后多岛那份马上按组拆回三个岛。拆分发生在摆完、还没跑过一刻的
/// 时候，每一类都有东西：排着的方块事件、刚登记还没跑过的方块实体、开着的菜单、待发的
/// 讲台格子、没确认的传送……漏搬哪一类都看得见（改坏确认靠这一点）。
///
/// Scene preparation for both comparisons: the multi-island world first
/// merges every island into one, both sides align their random sources and
/// get the same machines and menus (one island each, so furnishing is
/// identical down to the chest sound's pitch), then the multi-island world
/// splits back into one island per group at once. The split happens before
/// any tick, when every class holds something (queued block events, block
/// entities registered but never ticked, open menus, pending lectern slots,
/// unconfirmed teleports), so leaving any class behind is visible; the
/// break-it check relies on that.
fn prepare_pair(multi: &mut Scene, control: &mut Scene, sand_delay: i32) {
    let world = &mut multi.server.world;
    let partition = world.master.host.partition_mut().expect("多岛开着");
    let ids = partition.island_ids();
    for &id in &ids[1..] {
        assert!(partition.force_merge(ids[0], id), "并进岛 {}", ids[0]);
    }
    world.master.host.take_partition_ops();
    world.settle_islands();
    assert_eq!(world.islands.len(), 1, "全部并成了一个岛");
    fix_island_randoms(multi);
    fix_island_randoms(control);
    furnish_scene_with_sand(multi, sand_delay);
    furnish_scene_with_sand(control, sand_delay);
    // 平时在 tick 边界上是空的两类（还没广播的光照和方块变化），和第 9 步的离线核对一样
    // 每组各放一条，两边放的相同，下一刻照常广播。
    // The two classes that are normally empty at a tick boundary (unsent light
    // and block changes) get one entry per group, as in step 9's offline
    // check, identically on both sides; the next tick broadcasts them.
    for scene in [&mut *multi, &mut *control] {
        for &center in &scene.centers.clone() {
            let region = region_at(&mut scene.server.world, center);
            region
                .light_changes
                .entry(center)
                .or_insert(mc_bamboo_world::light::LightChanges { sky: 1, block: 0 });
            region.block_changes.changed.push(at(center, 0, Y, 0));
        }
        // 离场生物留下的事件平时在 tick 边界上也是空的（实体阶段放进去、同一刻的同步阶段
        // 取走）：每组放一条，记在这一组的中心区块上。实体号用不到的大数，没有人看着它，
        // 下一刻同步层取走时什么都不发。
        // Departed-mob events are empty at a tick boundary too (kept in the
        // entity phase, taken by the same tick's sync phase): one per group,
        // recorded on the group's centre chunk. The ids are large unused
        // numbers nobody watches, so the next sync takes them and sends nothing.
        for (group, &center) in scene.centers.clone().iter().enumerate() {
            region_at(&mut scene.server.world, center)
                .entities
                .keep_departed_events_at(
                    i32::MAX - 16 - group as i32,
                    center,
                    vec![mc_bamboo_blocks::entity::mob::MobEvent::Poof],
                );
        }
    }
    let world = &mut multi.server.world;
    let partition = world.master.host.partition_mut().expect("多岛开着");
    let home = partition.island_ids()[0];
    let made = partition.force_split(home);
    assert_eq!(made.len() + 1, multi.centers.len(), "按组拆开");
    world.master.host.take_partition_ops();
    world.settle_islands();
    assert_eq!(
        multi.server.world.islands.len(),
        multi.centers.len(),
        "摆完马上拆回每组一个岛"
    );
    assert_in_place(multi, "摆完拆开");
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
    // 悬空沙子的计划刻推到比较窗口之后：那一刻它会生成新的下落方块实体，而实体网络 ID
    // 来自全进程共享的原子分配器。多岛按岛编号轮流驱动，同一批新实体领号的顺序和单岛
    // 不同，包里的 ID 就不可能逐字节相同；计划 6.2 第 1 条明确「实体网络 ID 不进指纹」。
    // 沙子方块和这条计划刻仍在场景里参与拆合与指纹，只是不在比较窗口里到期。含新生成
    // 实体的情形由另一条测试按自己的口径覆盖。
    //
    // Push the sand's scheduled tick past the comparison window: it spawns a
    // new falling-block entity, and entity network ids come from the shared
    // process-wide allocator. Islands are driven in id order, so those ids are
    // taken in a different order than on one island and the id bytes cannot
    // match; plan 6.2 item 1 explicitly leaves network ids out of the
    // fingerprint. The sand block and its tick still take part in splits and
    // in the print, they just do not come due during the window. Fresh
    // entities are covered by a separate test with its own comparison rules.
    let late_sand = i32::try_from(ticks + 1).expect("刻数放得下 i32");
    // 拆分时就要漏搬（改坏确认），所以在摆场景之前设好。
    // Set before preparing: the preparation's split must already leave it behind.
    multi.server.world.master.split_skip = skip;
    prepare_pair(&mut multi, &mut control, late_sand);
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

/// 改坏确认：运行时拆岛漏搬任何一类（25 类），按组拼指纹时都要报出来（那一组的东西
/// 留在了别的岛上），或者之后某一刻的内容对不上。
///
/// Break-it check: leaving any of the 25 classes behind on a run-time split
/// must be caught (that group's content stays on another island) or show
/// up as a content difference later.
#[test]
fn leaving_any_class_behind_on_a_runtime_split_is_caught() {
    with_stack(|| {
        // 同样的参数、不漏搬时必须通过：下面抓到的失败只能来自漏搬的那一类。
        // The same run without a skip must pass, so every failure below comes
        // from the class left behind.
        let (_, splits) = run_against_control(2, 24, None);
        assert!(splits > 0, "比较窗口里也有拆分");
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

/// 含新实体对照里一定会强制拆合的那一刻（沙子在第 1 刻变成实体之后）。
const FRESH_TRIP: u64 = 3;

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
    // 和逐字节对照同一套场景准备（[`prepare_pair`]）。沙子按原来的 delay = 2 落地，比较
    // 窗口里因此会有新实体。
    //
    // The same preparation as the byte-for-byte comparison ([`prepare_pair`]).
    // The sand keeps its original delay of 2, so fresh entities appear inside
    // the window.
    prepare_pair(&mut multi, &mut control, 2);
    multi.server.world.master.split_break = break_kind;
    let mut next = rng(seed ^ 0x7E57);
    let mut trips: BTreeSet<u64> = (0..6).map(|_| next() % ticks).collect();
    // 至少一次拆合落在沙子变成实体之后（delay = 2：第 1 刻生成），新实体真的经历拆合，
    // 改坏确认也一定有东西可弄坏。
    // At least one structural change after the sand became an entity (delay 2:
    // spawned on tick 1), so fresh entities really go through one and the
    // break-it check always has something to corrupt.
    trips.insert(FRESH_TRIP);
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
        // 同样的参数、不弄坏时必须通过：下面抓到的失败只能来自故意弄坏的那一处。
        // The same run without a break must pass, so failures below come from it.
        run_fresh_entities(2, 16, None);
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
pub(super) fn set_up_alone(seed: u64, group: usize) -> Scene {
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
    super::island_hybrid_tests::bounded(|| {
        let mut scene = set_up(8, Harness::new_islands(2, 4));
        let world = &mut scene.server.world;
        world.master.island_stop_timeout = Duration::from_millis(300);
        world.master.island_dead_timeout = Duration::from_secs(300);
        let victim = world
            .islands
            .ids()
            .into_iter()
            .find(|&id| id != HOME_REGION)
            .expect("有别的岛");
        let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        world.islands.get_mut(victim).expect("在表里").test_hook =
            Some(crate::island::IslandTestHook::StallNotifying {
                entered: entered_tx,
                release: Arc::clone(&release),
            });
        let sender = scene.server.inbox_sender();
        // 等卡住的岛真的在 tick 线程上进入卡顿后再发关服，不靠固定睡眠竞态。
        // Wait until the stuck island actually enters the stall on its worker
        // thread before sending Shutdown, avoiding a fixed-sleep race.
        let stopper = std::thread::spawn(move || {
            entered_rx
                .recv_timeout(Duration::from_secs(60))
                .expect("卡住的岛 60 秒内应进入卡顿点");
            sender.send(crate::inbound::Inbound::Shutdown).unwrap();
        });
        let flow = scene.server.world.tick();
        assert!(flow.is_break(), "关服请求之后世界循环结束");
        assert!(
            !release.load(std::sync::atomic::Ordering::Acquire),
            "卡住的岛一直没开闸，证明没有死等它"
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

/// 经验球现在会跟随玩家、被玩家吸收并给经验。多岛和整个世界一个岛逐刻比玩家身边的
/// 同一个球：窗口里先由别的岛上的 OP 借全部玩家执行 `/give @a`，再把球所在的岛强制并进
/// 家岛、再强制拆开。每一刻都核对球的位置、速度、跟随谁和 count，直到球被吸收；
/// 两边必须在同一刻吸收，玩家的经验等级 / 进度 / 总经验和收到的字节也要逐项相同。
///
/// An experience orb now follows, is absorbed by and grants experience to a
/// player. The multi-island world is compared with a single island tick by
/// tick on one orb next to a player: an op on another island borrows all
/// players for `/give @a`, then the orb's island is forced into home and
/// split off again. Position, velocity, `following` and `count` are compared
/// until the orb is absorbed; both sides must absorb on the same tick, and
/// the player's level / progress / total experience and received bytes match.
#[test]
fn an_orb_beside_a_player_matches_one_island_through_borrowing_and_splits() {
    with_stack(|| {
        let seed = 71;
        let mut multi = set_up(seed, Harness::new_islands(2, 0));
        let mut control = set_up(
            seed,
            Harness::new_loaded_with_drive(2, IslandDrive::Lockstep),
        );
        align(&mut multi, &mut control);
        // 反馈和聊天带服务端的墙上时钟毫秒：两边固定成同一个值才能逐字节比。
        // Feedback and chat carry wall-clock millis; pin both for byte comparison.
        for scene in [&mut multi, &mut control] {
            for island in scene.server.world.islands.iter_mut() {
                island.region.clock_millis = Some(1_700_000_000_000);
            }
        }
        let op = super::island_crossing_tests::first_of(&multi, 0);
        super::island_crossing_tests::make_op(&mut multi, &op);
        let op_control = super::island_crossing_tests::first_of(&control, 0);
        super::island_crossing_tests::make_op(&mut control, &op_control);
        let target = super::island_crossing_tests::first_of(&multi, 1);
        let target_control = super::island_crossing_tests::first_of(&control, 1);
        let island = multi
            .server
            .world
            .islands
            .island_of_player(target.id)
            .expect("在某个岛上");
        assert_ne!(island, HOME_REGION, "第 1 组不在家岛上");
        let spawn = |scene: &mut Scene, island: RegionId| -> i32 {
            let region = &mut scene
                .server
                .world
                .islands
                .get_mut(island)
                .expect("停放着")
                .region;
            region.entities.set_entity_seeds(4242);
            let at = region.players[&target.id].position();
            region
                .entities
                .spawn(mc_bamboo_blocks::entity::Entity::experience_orb(
                    Vec3::new(at.x + 3.0, at.y + 0.5, at.z),
                    5,
                ))
        };
        let orb = spawn(&mut multi, island);
        let orb_control = spawn(&mut control, HOME_REGION);
        #[derive(Debug, PartialEq)]
        struct OrbState {
            position: Vec3,
            delta: Vec3,
            following: Option<i32>,
            count: i32,
        }
        let state = |scene: &Scene, uid: i32| -> Option<OrbState> {
            let entity = scene
                .server
                .world
                .islands
                .iter()
                .find_map(|island| island.region.entities.get(uid))?;
            if entity.removed {
                return None;
            }
            let mc_bamboo_blocks::entity::EntityKind::ExperienceOrb(state) = &entity.kind else {
                panic!("是经验球");
            };
            Some(OrbState {
                position: entity.position,
                delta: entity.delta,
                following: state.following,
                count: state.count,
            })
        };
        let experience = |scene: &Scene, id: crate::inbound::PlayerId| -> (i32, u32, i32) {
            let player = scene
                .server
                .world
                .islands
                .iter()
                .find_map(|island| island.region.players.get(&id))
                .expect("目标还在世界里");
            (
                player.experience.level,
                player.experience.progress.to_bits(),
                player.experience.total,
            )
        };
        let mut split_back = false;
        let mut absorbed_at = None;
        for tick in 0..24 {
            match tick {
                3 => {
                    for (scene, client) in [(&multi, &op), (&control, &op_control)] {
                        scene.server.request(
                            client,
                            mc_bamboo_protocol::packet::play::PlayRequest::unsigned_command(
                                "give @a minecraft:stone 1",
                            ),
                        );
                    }
                }
                5 => {
                    let world = &mut multi.server.world;
                    let partition = world.master.host.partition_mut().expect("多岛开着");
                    assert!(partition.force_merge(HOME_REGION, island));
                    world.master.host.take_partition_ops();
                    world.settle_islands();
                    assert!(!world.islands.contains(island), "并进家岛了");
                }
                8 => {
                    let world = &mut multi.server.world;
                    let partition = world.master.host.partition_mut().expect("多岛开着");
                    assert!(!partition.force_split(HOME_REGION).is_empty(), "拆开了");
                    world.master.host.take_partition_ops();
                    world.settle_islands();
                }
                _ => {}
            }
            multi.server.tick();
            control.server.tick();
            let (mine, theirs) = (state(&multi, orb), state(&control, orb_control));
            assert_eq!(mine, theirs, "tick {tick}：球的状态");
            assert_eq!(
                experience(&multi, target.id),
                experience(&control, target_control.id),
                "tick {tick}：玩家的等级、进度和总经验"
            );
            assert_eq!(
                target.take_frames(),
                target_control.take_frames(),
                "tick {tick}：目标玩家收到的字节"
            );
            // 两边逐刻的状态相等，所以「同一刻吸收」由上面的 assert 保证；
            // 继续跑到拆合结束，别提前退出窗口。
            // Equal states each tick guarantee absorption on the same tick;
            // keep ticking through the split instead of leaving the window.
            if mine.is_none() && theirs.is_none() && absorbed_at.is_none() {
                absorbed_at = Some(tick);
            }
            let world = &mut multi.server.world;
            if tick >= 8
                && world.islands.island_of_player(target.id)
                    != world.islands.island_of_player(op.id)
            {
                split_back = true;
            }
            world.assert_island_invariants();
        }
        assert!(absorbed_at.is_some(), "窗口里球应该被玩家吸收");
        assert!(split_back, "第 1 组又拆成了自己的岛");
    });
}

/// 第 11 步长时间压测里那次没复现的不变量失败（计划 17.7）的第一种猜测：拆分用的是
/// 「做决定那一刻」的名单，照做之前有人移动了。这里确定性地复现那个窗口：分区器当场定下
/// 一条拆分，先不照做；把一名本来会留在岛上的玩家直接放到即将拆出去的那一半的区块上
/// （视野还留在原来的位置），然后照做。
///
/// **修之前这条测试在正常路径上就报出 17.7 的原文**：`玩家跟踪着别的岛的区块`。真正的原因
/// 不是「名单过期」，而是 `split_parts` 按**瞬时位置**分人、`split_ready` 只查视野中心：
/// 直接放人（重生、压测钩子、`/tp`）先改位置，视野要到岛下一次 tick 才跟上；这中间照做拆分
/// 时位置与视野中心分在切口两边，玩家被算进拆出去的一边，随后 `relocate_players` 把他
/// 连同旧岛的跟踪区块搬进新岛。那片区块之后的方块更新只发给新岛自己的玩家，他永久漏包，
/// 所以不能靠放宽不变量断言来遮。
///
/// 修法：`split_parts` 按**视野中心**（正在跟踪的区块）分人，`split_ready` 还要求每名玩家
/// 跟踪范围里没有任何一格落在另一边，否则推迟拆分、记一条 `island_split_deferrals`；视野
/// 跟上之后下一次照做才拆。跟踪区块属于本岛的断言保持严格。
///
/// The first guess behind the unreproduced invariant failure in the step 11
/// long run (plan 17.7): a split used the roster from decision time and
/// someone moved before it was replayed. This reproduces that window
/// deterministically: the partitioner records a split, it is not replayed yet;
/// a player who would stay is placed directly on a chunk that will split off
/// while their view is still at the old place, then the split is replayed.
///
/// **Before the fix this test reported the exact step 17.7 message on the
/// normal path**. The cause was not an outdated roster: `split_parts` used the
/// instantaneous position while `split_ready` only checked the view centre.
/// Placing a player directly (respawn, load-test hooks, `/tp`) moves them
/// first, and their view follows only on the island's next tick; replaying a
/// split in between put them on the outgoing side and `relocate_players` then
/// carried them, with the old island's tracked chunks, to the new island.
/// Block updates there go only to that island's own players, so loosening the
/// invariant would hide a permanent client desync.
///
/// Fix: `split_parts` assigns players by **view centre** (the chunks they
/// track); `split_ready` also requires that no tracked chunk of any player
/// lies on the other side, postponing the split and recording an
/// `island_split_deferrals` sample otherwise. The "tracked chunks belong to
/// this island" assertion stays strict.
#[test]
fn a_split_decided_before_a_player_moves_uses_the_live_roster() {
    with_stack(|| {
        // 直接放人（压测钩子 / `/tp` 走的就是这条路径）。
        // Direct placement (the load-test hook and `/tp` use this path).
        run_split_window(false, |scene, traveller, position| {
            scene.server.move_to_position(traveller, position);
        });
        // 改坏确认：按瞬时位置分人，同一个场景必须在不变量断言上失败。
        // Break-it check: splitting players by instantaneous position must
        // fail an invariant assertion in the same scene.
        let broken = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_split_window(true, |scene, traveller, position| {
                scene.server.move_to_position(traveller, position);
            });
        }));
        assert!(
            broken.is_err(),
            "按瞬时位置分人却没有被不变量断言抓到：那次失败复现不出来"
        );
    });
}

/// 重生落在「已定拆分、还没照做」的那一半：和直接放人是同一条路径
/// （重生在岛的这一刻里调 `command::place_player_at_respawn`，先改位置、视野后跟）。
/// 走一遍重生用的放置入口，确认修法也盖住这条路径；改坏确认同上一个测试。
///
/// A respawn landing on the half whose split is already decided but not
/// replayed: the same path as direct placement (respawn calls
/// `command::place_player_at_respawn` inside the island's tick, moving first
/// with the view following later). Exercise the respawn placement entry to
/// confirm the fix covers it too; the break-it check is the one above.
#[test]
fn a_respawn_into_a_pending_split_keeps_the_view_on_one_island() {
    with_stack(|| {
        run_split_window(false, |scene, traveller, position| {
            // 重生路径只改位置 / 朝向 / player_grid，不发第二个传送包，也不动视野。
            // The respawn path only changes position, rotation and the player
            // grid: no second teleport packet, no view change yet.
            let world = &mut scene.server.world;
            let island = world
                .islands
                .island_of_player(traveller.id)
                .expect("玩家在岛上");
            let region = &mut world.islands.get_mut(island).expect("在表里").region;
            crate::player_move::place_player(region, traveller.id, position);
        });
        let broken = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_split_window(true, |scene, traveller, position| {
                let world = &mut scene.server.world;
                let island = world
                    .islands
                    .island_of_player(traveller.id)
                    .expect("玩家在岛上");
                let region = &mut world.islands.get_mut(island).expect("在表里").region;
                crate::player_move::place_player(region, traveller.id, position);
            });
        }));
        assert!(broken.is_err(), "重生路径上按瞬时位置分人却没被抓到");
    });
}

/// 正式运行里最常撞进这个窗口的是重生：玩家在岛 A 被 `/kill` 打死，世界出生点在
/// 「已定拆分、还没照做」的那一半，客户端发 `PERFORM_RESPAWN` 后 `player_respawn::respawn`
/// 经 `command::place_player_at_respawn` 在岛的这一刻里先改位置、视野后跟。这里走完整的
/// 真实路径（不直接调放人函数），确认推迟有界、跟踪区块属于本岛、玩家所在岛拥有他跟踪的
/// 全部区块，而且那片区块里的方块更新真的发到他手上。两种方向都造得出来，就都跑：
/// 玩家留在原岛、重生点在拆出去的那半（`spawn_moves = true`），以及玩家所在的那半被
/// 拆出去、重生点留在原岛（`spawn_moves = false`）。`immediate_respawn` 打开时不经死亡
/// 界面，客户端自己发 `PERFORM_RESPAWN` 后走同一个 `respawn()`，也各跑一遍。
/// 改坏确认同前两条测试。
///
/// The window is most often hit by a respawn in a real server: a player on
/// island A is killed by `/kill`, the world spawn sits in the half whose split
/// is already decided but not replayed, and `PERFORM_RESPAWN` runs
/// `player_respawn::respawn`, which places through
/// `command::place_player_at_respawn` inside the island's tick (position moves
/// first, view follows later). This walks the whole real path without calling
/// any placement helper: postponement is bounded, tracked chunks stay on the
/// player's island, that island owns every tracked chunk, and a block update
/// in one of those chunks really reaches the player. Both directions of the
/// window can be built, so both run: the player stays while the spawn is on
/// the split-off half (`spawn_moves = true`), and the player's half splits off
/// while the spawn stays (`spawn_moves = false`). With `immediate_respawn`
/// on the client skips the death screen but still sends PERFORM_RESPAWN and
/// takes the same `respawn()`; run those too. Break-it checks as in the two
/// tests above.
#[test]
fn a_real_death_and_respawn_into_a_pending_split_delivers_block_updates() {
    with_stack(|| {
        for spawn_moves in [true, false] {
            for immediate in [false, true] {
                run_death_respawn_split_window(false, spawn_moves, immediate);
                // 改坏确认：四种组合都要被按瞬时位置分人抓到。
                // Break-it check: all four combinations must fail with the
                // instantaneous-position split.
                let broken = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_death_respawn_split_window(true, spawn_moves, immediate);
                }));
                assert!(
                    broken.is_err(),
                    "spawn_moves={spawn_moves} immediate_respawn={immediate}：按瞬时位置分人却没被抓到"
                );
            }
        }
    });
}

/// 跟踪范围真的跨了切口时拆分必须推迟，并且推迟有界：视野跟上（不再跨切口）之后
/// 下一次照做就拆。改坏确认：把 `split_ready` 的跨切口检查拿掉（按瞬时位置分人），
/// 这次拆分当场照做、推迟计数为 0；`a_split_decided_before_a_player_moves_uses_the_live_roster`
/// 里同一处改坏还会报出不变量失败的原文。
///
/// A tracked range that really straddles the cut postpones the split, and the
/// postponement is bounded: once the view follows (no longer straddling) the
/// next replay applies it. Break-it check: without the straddle check
/// (splitting by instantaneous position) the split applies at once with no
/// deferral; the same break in the test above also reproduces the historical
/// invariant failure verbatim.
#[test]
fn a_straddling_view_postpones_a_split_only_until_it_follows() {
    with_stack(|| {
        // 正常路径：推迟一次、记一条样本，随后视野跟上来就拆。
        // Normal path: exactly one deferral, then the view follows and it
        // splits.
        let (deferrals, split_at) = run_straddling_view(false);
        assert_eq!(deferrals, 1, "推迟一次并记一条样本");
        assert!(split_at.is_some(), "推迟有界：视野跟上之后 6 刻内拆完");
        // 改坏确认：按瞬时位置分人、跳过视野检查，拆分当场照做（不再推迟）。
        // 同样的场景在 `a_split_decided_before_a_player_moves_uses_the_live_roster`
        // 里还会报出不变量失败的原文。
        // Break-it check: splitting by instantaneous position with the view
        // checks skipped applies the split at once (no deferral). The same
        // break in the test above also reproduces the historical invariant
        // failure verbatim.
        let broken = run_straddling_view(true);
        assert_eq!(broken.0, 0, "按瞬时位置分人时不再推迟");
        assert_eq!(broken.1, Some(0), "按瞬时位置分人时当场就拆");
    });
}

/// 造一个「视野中心在拆出去的那半、但跟踪范围里还有留下的那半的区块」的玩家：
/// 把他的位置放到中心附近（未加载的一格），视野钉在切口边界上，然后照做拆分。
/// 返回（`island_split_deferrals` 的增量，拆分在第几刻照做）。
///
/// Make a player whose view centre is on the outgoing half but whose tracked
/// range still contains chunks of the half that stays: place them near the
/// centre (an unloaded chunk), pin the view to the cut boundary, then replay
/// the split. Returns (increase in `island_split_deferrals`, the tick the
/// split was applied).
fn run_straddling_view(break_view: bool) -> (u64, Option<u64>) {
    let mut scene = set_up(91, Harness::new_islands(2, 0));
    scene.server.world.master.split_by_position = break_view;
    let first = scene
        .server
        .world
        .master
        .host
        .island_of(scene.centers[0])
        .expect("中心归某个岛");
    let second = scene
        .server
        .world
        .master
        .host
        .island_of(scene.centers[1])
        .expect("中心归某个岛");
    let (into, from) = (first.min(second), first.max(second));
    {
        let world = &mut scene.server.world;
        let partition = world.master.host.partition_mut().expect("多岛开着");
        assert!(partition.force_merge(into, from));
        world.master.host.take_partition_ops();
        world.settle_islands();
    }
    scene.server.tick();
    let ops = {
        let world = &mut scene.server.world;
        let partition = world.master.host.partition_mut().expect("多岛开着");
        let made = partition.force_split(into);
        assert_eq!(made.len(), 1, "合起来的岛拆成两块");
        let ops = partition.take_ops();
        assert_eq!(ops.len(), 1, "只有一条拆分");
        ops
    };
    // 切口附近的 section：拆出去的 section 里取最小 x，留下的那边取 x-1。
    // Find the cut: the smallest x of the outgoing sections; x-1 stays.
    let crate::island_partition::TopologyOp::Split { sections, .. } = &ops[0] else {
        panic!("定下的是拆分");
    };
    let size = crate::island_partition::SECTION_SIZE;
    let edge_x = sections
        .iter()
        .map(|section| section.x)
        .min()
        .expect("拆出去的部分不为空");
    let edge_z = sections
        .iter()
        .filter(|section| section.x == edge_x)
        .map(|section| section.z)
        .min()
        .expect("有一行 section");
    let boundary = ChunkPos::new(edge_x * size, edge_z * size);
    let traveller = scene
        .clients
        .iter()
        .find(|(group, _)| scene.centers[*group] == scene.centers[0])
        .map(|(_, client)| client.clone())
        .expect("第 0 组有人");
    {
        let world = &mut scene.server.world;
        let island = world
            .islands
            .island_of_player(traveller.id)
            .expect("玩家在岛上");
        let region = &mut world.islands.get_mut(island).expect("在表里").region;
        // 位置放在切口内侧一格（拆出去的那半），视野中心钉在切口外一格：
        // 视距 2 的跟踪范围同时盖住两边。
        // Position one chunk inside the cut, view centre one chunk outside:
        // a view distance of 2 covers both sides.
        crate::player_move::place_player(
            region,
            traveller.id,
            Vec3::new(
                f64::from((boundary.x + 1) * 16) + 8.5,
                f64::from(Y),
                f64::from((boundary.z + 1) * 16) + 8.5,
            ),
        );
        let player = region.players.get_mut(&traveller.id).expect("玩家");
        player
            .chunks
            .set_view_for_test(ChunkPos::new(boundary.x - 1, boundary.z), 2);
    }
    let world = &mut scene.server.world;
    world.master.host.queue_topology_for_test(ops);
    let before = world.profiler().count_of(Metric::IslandSplitDeferrals);
    world.settle_islands();
    world.assert_island_invariants();
    let deferrals = world.profiler().count_of(Metric::IslandSplitDeferrals) - before;
    // 把视野跟到新位置（岛下一次 tick 会做这件事），让拆分能照做。
    // Let the view follow the new position (the island's next tick does this),
    // so the split can be applied.
    {
        let island = world
            .islands
            .island_of_player(traveller.id)
            .expect("玩家在岛上");
        let region = &mut world.islands.get_mut(island).expect("在表里").region;
        let view_distance = region.view_distance;
        if let Some(player) = region.players.get_mut(&traveller.id) {
            crate::chunk_tracking::update_player_view(player, &mut region.links, view_distance);
        }
    }
    let mut split_at = None;
    for tick in 0..6 {
        scene.server.tick();
        scene.server.world.assert_island_invariants();
        if !scene.server.world.master.host.has_topology() && split_at.is_none() {
            split_at = Some(tick);
        }
    }
    (deferrals, split_at)
}

/// `place` 把玩家直接放到 `position`（两种入口见两条测试）。`break_view` 打开时
/// `apply_split` 改回按瞬时位置分人（改坏确认）。
///
/// `place` puts the player directly at `position` (the two entries are in the
/// tests above). With `break_view`, `apply_split` goes back to splitting
/// players by instantaneous position (break-it check).
fn run_split_window(break_view: bool, place: impl Fn(&mut Scene, &Client, Vec3)) {
    let mut scene = set_up(77, Harness::new_islands(2, 0));
    assert_eq!(scene.server.world.islands.len(), 3, "三组各成一个岛");
    scene.server.world.master.split_by_position = break_view;
    // 把第 1、2 组并进第 0 组：`force_split` 定下一条拆分。
    // Merge groups 1 and 2 into group 0: `force_split` records one split.
    let first = scene
        .server
        .world
        .master
        .host
        .island_of(scene.centers[0])
        .expect("中心归某个岛");
    let second = scene
        .server
        .world
        .master
        .host
        .island_of(scene.centers[1])
        .expect("中心归某个岛");
    let (into, from) = (first.min(second), first.max(second));
    {
        let world = &mut scene.server.world;
        let partition = world.master.host.partition_mut().expect("多岛开着");
        assert!(partition.force_merge(into, from));
        world.master.host.take_partition_ops();
        world.settle_islands();
    }
    scene.server.tick();
    // 拆出去的 section 里挑一个组的中心当目的地，另一边挑一名玩家当移动的人。
    // Pick a group centre inside the split-off sections as the destination,
    // and a player on the other side as the one who moves.
    let (target, stayed) = {
        let world = &mut scene.server.world;
        let partition = world.master.host.partition_mut().expect("多岛开着");
        let made = partition.force_split(into);
        assert_eq!(made.len(), 1, "合起来的岛拆成两块");
        let ops = partition.take_ops();
        assert_eq!(ops.len(), 1, "只有一条拆分");
        let crate::island_partition::TopologyOp::Split { sections, .. } = &ops[0] else {
            panic!("定下的是拆分");
        };
        let inside =
            |center: ChunkPos| sections.contains(&crate::island_partition::SectionPos::of(center));
        let target = scene
            .centers
            .iter()
            .copied()
            .find(|&center| inside(center))
            .expect("拆出去的部分里有一组的中心");
        let stayed = scene
            .centers
            .iter()
            .copied()
            .find(|&center| !inside(center))
            .expect("另一边也有一组的中心");
        world.master.host.queue_topology_for_test(ops);
        (target, stayed)
    };
    // 这名玩家本来会留在岛上；直接放到拆出去的那一半的区块上，视野还留在原来的位置。
    // This player would have stayed; put them directly on a chunk of the
    // split-off half while their view is still at the old place.
    let traveller = scene
        .clients
        .iter()
        .find(|(group, _)| scene.centers[*group] == stayed)
        .map(|(_, client)| client.clone())
        .expect("留下的那组有人");
    let old_center = scene
        .server
        .world
        .islands
        .iter()
        .flat_map(|island| island.region.players.values())
        .find(|player| player.id == traveller.id)
        .and_then(|player| player.chunks.view_center())
        .expect("玩家已经有视野");
    let position = Vec3::new(
        f64::from(target.x * 16) + 8.5,
        f64::from(Y),
        f64::from(target.z * 16) + 8.5,
    );
    place(&mut scene, &traveller, position);
    assert_ne!(old_center, target, "视野和新位置不在同一边");
    assert_eq!(
        scene
            .server
            .world
            .islands
            .iter()
            .flat_map(|island| island.region.players.values())
            .find(|player| player.id == traveller.id)
            .and_then(|player| player.chunks.view_center()),
        Some(old_center),
        "视野还留在原来的位置"
    );
    // 修法：玩家按视野中心留在原来的岛，拆分可以当场照做，也可以因为别的玩家的
    // 跟踪范围跨切口而推迟；两种情况下不变量都必须成立，推迟的话记一条
    // `island_split_deferrals`，并且是有界的（6 刻内拆完）。
    // The fix: the player stays on the old island by view centre. The split
    // may apply now or be postponed because another player's tracked range
    // straddles the cut; either way invariants hold, a postponement is
    // recorded as `island_split_deferrals` and is bounded (done in 6 ticks).
    let before = scene
        .server
        .world
        .profiler()
        .count_of(Metric::IslandSplitDeferrals);
    scene.server.world.settle_islands();
    scene.server.world.assert_island_invariants();
    let deferred = scene.server.world.master.host.has_topology();
    if deferred {
        assert_eq!(
            scene
                .server
                .world
                .profiler()
                .count_of(Metric::IslandSplitDeferrals),
            before + 1,
            "推迟记了一条样本"
        );
    }
    let mut split_at = None;
    for tick in 0..6 {
        scene.server.tick();
        scene.server.world.assert_island_invariants();
        if !scene.server.world.master.host.has_topology() && split_at.is_none() {
            split_at = Some(tick);
        }
    }
    assert!(split_at.is_some(), "6 刻内拆分照做了");
    // 玩家最终只在一个岛上、那个岛就是他脚下区块的岛。
    // The player ends up on exactly one island: the one holding the chunk
    // underfoot.
    let world = &mut scene.server.world;
    assert_eq!(world.islands.len(), 3, "拆分照做了");
    let holder = world
        .islands
        .island_of_player(traveller.id)
        .expect("玩家还在世界里");
    assert_eq!(
        Some(holder),
        world.islands.island_of_chunk(target),
        "玩家搬到了脚下区块的岛"
    );
    assert_eq!(
        world
            .islands
            .iter()
            .filter(|island| island.region.players.contains_key(&traveller.id))
            .count(),
        1,
        "玩家只在一个岛上"
    );
}

/// 造一个「已定拆分、还没照做」的窗口，把世界出生点和死者分在切口两边，再走完整的
/// `/kill` → `PERFORM_RESPAWN` → `player_respawn::respawn`（`immediate_respawn`
/// 打开时客户端也只是不显示死亡界面，仍然发同一个包）。`spawn_moves` 决定重生点在拆出去的那半
/// （死者在留下的那半）还是反过来。`break_view` 用 e0526f0c 的改坏钩子改回按瞬时
/// 位置分人。
///
/// Make a window where a split is decided but not replayed, with the world
/// spawn and the dying player on opposite sides of the cut, then walk the
/// whole `/kill` → `PERFORM_RESPAWN` → `player_respawn::respawn` path (with
/// `immediate_respawn` on the client only skips the death screen and still
/// sends the same packet). `spawn_moves`
/// picks whether the spawn is on the split-off half (the player stays) or the
/// other way round. `break_view` uses e0526f0c's break-it switch to split
/// players by instantaneous position.
fn run_death_respawn_split_window(break_view: bool, spawn_moves: bool, immediate: bool) {
    let mut scene = set_up(6, Harness::new_islands(2, 0));
    assert_eq!(scene.server.world.islands.len(), 3, "三组各成一个岛");
    scene.server.world.master.split_by_position = break_view;
    // 把第 1、2 组并进第 0 组：`force_split` 定下一条拆分，先不照做。
    // Merge groups 1 and 2 into group 0: `force_split` records one split.
    let first = scene
        .server
        .world
        .master
        .host
        .island_of(scene.centers[0])
        .expect("中心归某个岛");
    let second = scene
        .server
        .world
        .master
        .host
        .island_of(scene.centers[1])
        .expect("中心归某个岛");
    let (into, from) = (first.min(second), first.max(second));
    {
        let world = &mut scene.server.world;
        let partition = world.master.host.partition_mut().expect("多岛开着");
        assert!(partition.force_merge(into, from));
        world.master.host.take_partition_ops();
        world.settle_islands();
    }
    scene.server.tick();
    // 切口两边各挑一个组的中心：一个当重生点，另一个当死者所在处。
    // Pick one group centre on each side of the cut: one for the respawn
    // spot and the other for the dying player.
    let (split_off, remaining) = {
        let world = &mut scene.server.world;
        let partition = world.master.host.partition_mut().expect("多岛开着");
        let made = partition.force_split(into);
        assert_eq!(made.len(), 1, "合起来的岛拆成两块");
        let ops = partition.take_ops();
        assert_eq!(ops.len(), 1, "只有一条拆分");
        let crate::island_partition::TopologyOp::Split { sections, .. } = &ops[0] else {
            panic!("定下的是拆分");
        };
        let inside =
            |center: ChunkPos| sections.contains(&crate::island_partition::SectionPos::of(center));
        let split_off = scene
            .centers
            .iter()
            .copied()
            .find(|&center| inside(center))
            .expect("拆出去的部分里有一组的中心");
        let remaining = scene
            .centers
            .iter()
            .copied()
            .find(|&center| !inside(center))
            .expect("另一边也有一组的中心");
        world.master.host.queue_topology_for_test(ops);
        (split_off, remaining)
    };
    let (target, stayed) = if spawn_moves {
        (split_off, remaining)
    } else {
        (remaining, split_off)
    };
    assert!(
        scene.server.world.master.host.has_topology(),
        "拆分已定、还没照做"
    );
    let traveller = scene
        .clients
        .iter()
        .find(|(group, _)| scene.centers[*group] == stayed)
        .map(|(_, client)| client.clone())
        .expect("留下的那组有人");
    // 世界出生点和死者分在切口两边。测试世界的 `respawn_radius` 是 0，
    // 搜索直接落在出生点这一列；y 取超平坦地面之上一格。
    // Put the world spawn on the other side of the cut from the player. The
    // test world's `respawn_radius` is 0, so the search lands on this very
    // column; y is the block just above the superflat ground.
    let spawn_pos = BlockPos::new(target.x * 16 + 8, Y, target.z * 16 + 8);
    scene.server.set_spawn(RespawnData::of(spawn_pos, 0.0, 0.0));
    scene.server.world.publish_global();
    {
        let player = scene.server.player_region(&traveller);
        let state = player.players.get_mut(&traveller.id).expect("玩家");
        state.permission_level = 4;
        state.game_mode = GameMode::Survival;
        state.respawn = None;
    }
    if immediate {
        let _ = scene
            .server
            .world
            .master
            .global
            .rules_mut()
            .assign(mc_bamboo_data::gamerules::IMMEDIATE_RESPAWN, "true");
        scene.server.world.publish_global();
    }
    let old_center = scene
        .server
        .player(&traveller)
        .chunks
        .view_center()
        .expect("玩家已经有视野");
    assert_ne!(old_center, target, "视野和重生点不在同一边");
    let _ = traveller.take_frames();
    let before = scene
        .server
        .world
        .profiler()
        .count_of(Metric::IslandSplitDeferrals);
    // 两条入站请求排进同一刻：`/kill @s` 走 genericKill 的完整死亡流程，
    // 客户端收到死亡界面包后发 PERFORM_RESPAWN（规则开着只是不显示界面），
    // 经 `chat::on_client_command` 调 `player_respawn::respawn`。
    // Queue both inbound requests for the same tick: `/kill @s` goes through
    // the full genericKill death flow; the client sends PERFORM_RESPAWN after
    // the death-screen packet (with the rule on it just skips showing it),
    // which goes through `chat::on_client_command` into
    // `player_respawn::respawn`.
    scene.server.request(
        &traveller,
        PlayRequest::unsigned_command("kill @s".to_owned()),
    );
    scene.server.request(
        &traveller,
        PlayRequest::ClientCommand {
            action: ClientCommandKind::PerformRespawn,
        },
    );
    scene.server.tick();
    let frames = traveller.take_frames();
    assert!(
        !frames_with_id(&frames, clientbound::PLAYER_COMBAT_KILL).is_empty(),
        "走了完整的死亡流程"
    );
    assert!(
        !frames_with_id(&frames, clientbound::RESPAWN).is_empty(),
        "重生包发给了玩家"
    );
    let position = scene.server.player(&traveller).position();
    assert_eq!(
        (position.x.floor() as i32, position.z.floor() as i32),
        (spawn_pos.x, spawn_pos.z),
        "重生落在世界出生点那一列"
    );
    assert_eq!(
        scene.server.player(&traveller).chunk_pos(),
        target,
        "重生点和死时所在的区块分在切口两边"
    );
    assert_eq!(
        scene.server.player(&traveller).chunks.view_center(),
        Some(target),
        "这一刻岛的视野更新已经跟上"
    );
    // 拆分在这一刻的收包阶段照做（或推迟），岛的视野更新在后面的模拟阶段：
    // `settle_islands` 的严格不变量断言是在「重生已改位置、视野还没跟上」时跑的，
    // 上面这个视野已经跟上说明窗口确实被走过，而不是拆分被拖到了下一刻。
    // The split was replayed (or postponed) in the inbound phase, while the
    // view update belongs to the later simulation phase: `settle_islands`
    // asserted its strict invariants when the respawn had moved the player
    // but the view had not followed yet. The view above is now caught up, so
    // the window was really crossed rather than deferred to the next tick.
    scene.server.world.assert_island_invariants();
    let deferred = scene.server.world.master.host.has_topology();
    if deferred {
        assert_eq!(
            scene
                .server
                .world
                .profiler()
                .count_of(Metric::IslandSplitDeferrals),
            before + 1,
            "推迟记了一条样本"
        );
    }
    // 推迟有界，和 e0526f0c 的三条测试同一个上限：6 刻内拆分照做。
    // Bounded like the three tests from e0526f0c: the split happens in 6 ticks.
    let mut split_at = None;
    for tick in 0..6 {
        scene.server.tick();
        scene.server.world.assert_island_invariants();
        if !scene.server.world.master.host.has_topology() && split_at.is_none() {
            split_at = Some(tick);
        }
    }
    assert!(split_at.is_some(), "6 刻内拆分照做了");
    assert_eq!(scene.server.world.islands.len(), 3, "拆分照做了");
    // 玩家最终所在的岛拥有他跟踪的全部区块。
    // The player's final island owns every chunk they track.
    let world = &scene.server.world;
    let holder = world
        .islands
        .island_of_player(traveller.id)
        .expect("玩家还在世界里");
    assert_eq!(
        Some(holder),
        world.islands.island_of_chunk(target),
        "玩家搬到了重生点所在的岛"
    );
    let player = world
        .islands
        .get(holder)
        .expect("在表里")
        .region
        .players
        .get(&traveller.id)
        .expect("玩家在岛上");
    let (center, reach) = player.chunks.tracked_bounds().expect("重生后有视野");
    let tracked: Vec<ChunkPos> = (-reach..=reach)
        .flat_map(|dx| (-reach..=reach).map(move |dz| ChunkPos::new(center.x + dx, center.z + dz)))
        .filter(|&pos| player.chunks.is_chunk_tracked(pos))
        .collect();
    assert!(!tracked.is_empty(), "玩家跟踪着区块");
    for &pos in &tracked {
        assert_eq!(
            world.islands.island_of_chunk(pos),
            Some(holder),
            "跟踪区块 {pos:?} 属于玩家所在的岛"
        );
    }
    // 跟踪区块里改一格，玩家必须收到这个方块更新：跨岛漏包是永久的客户端失步。
    // Change one block in a tracked chunk: the player must receive the update.
    // Missing it across islands is a permanent client desync.
    let pos = at(target, 1, Y + 1, 1);
    let world = &mut scene.server.world;
    let region = &mut world.islands.get_mut(holder).expect("在表里").region;
    assert!(region.chunks.set_block(pos, blocks::GOLD_BLOCK));
    region.block_changes.changed.push(pos);
    let _ = traveller.take_frames();
    scene.server.tick();
    scene.server.world.assert_island_invariants();
    let frames = traveller.take_frames();
    assert!(
        block_changes(&frames).contains(&(pos, blocks::GOLD_BLOCK)),
        "玩家收到了跟踪区块里的方块更新"
    );
}

/// 玩家骑手和载具在岛拆分时是不可分的一组：`split_ready` 在「玩家留在原岛、
/// 载具在拆出去的那半」时推迟拆分；玩家和载具在同一边时放行。
#[test]
fn a_split_never_separates_a_rider_from_the_vehicle() {
    use mc_bamboo_blocks::entity::{Entity, mob::MobType};
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{
        inbound::PlayerId, player::Player, region::Region, world::island_runtime::split_ready,
    };

    let mut region = Region::new(2, 20, false);
    let id = PlayerId(4);
    region.insert_player(Player::new(
        id,
        Uuid::from_u128(9),
        "Rider".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(8.5, -60.0, 8.5),
    ));
    let horse = region.entities.spawn(Entity::by_mob_type(
        MobType::Horse,
        Vec3::new(8.5, -60.0, 8.5),
    ));
    crate::riding::mount(&mut region, id, horse);
    assert!(crate::riding::check_consistency(&region).is_empty());

    // 玩家和载具都在区块 (0,0)，把 (1,0) 拆出去：玩家留在原岛，放行。
    let far = BTreeSet::from([ChunkPos::new(1, 0)]);
    let leaving = BTreeSet::new();
    let inside = |pos: ChunkPos| far.contains(&pos);
    assert!(split_ready(&region, &leaving, &inside, true));

    // 载具挪到拆出去的那一边，玩家还在原岛：不能拆。
    region.entities.with_mut(horse, |entity| {
        entity.position = Vec3::new(24.5, -60.0, 8.5);
    });
    assert!(!split_ready(&region, &leaving, &inside, true));

    // 玩家也分到拆出去的那边（视野中心已跟过去）：整组一起走，放行。
    let leaving = BTreeSet::from([id]);
    assert!(split_ready(&region, &leaving, &inside, true));
}

/// 抽取发给客户端的 `ClientboundMapItemDataPacket` 的 `(map_id, has_decorations, patch_width)`。
fn decode_map_packets(frames: &[Frame]) -> Vec<(i32, bool, u8)> {
    use mc_bamboo_protocol::codec::Reader;

    let mut out = Vec::new();
    for frame in frames_with_id(frames, clientbound::MAP_ITEM_DATA) {
        let mut r = Reader::new(&frame.body);
        let map_id = r.varint().expect("map_id");
        let _scale = r.i8().expect("scale");
        let _locked = r.bool().expect("locked");
        let has_decs = r.bool().expect("has_decorations");
        if has_decs {
            let count = r.varint().expect("dec_count");
            for _ in 0..count {
                let _kind = r.varint().expect("kind");
                let _x = r.i8().expect("x");
                let _y = r.i8().expect("y");
                let _rot = r.i8().expect("rot");
                assert!(!r.bool().expect("has_name"));
            }
        }
        let patch_width = r.u8().expect("patch_width");
        out.push((map_id, has_decs, patch_width));
    }
    out
}

/// 多岛（分离驱动）下两个岛各自创建地图：ID 全局唯一不撞号，制图台放大同样走全服计数器，
/// 关服存盘再重开后 `last_id.dat` 与全部 `<id>.dat` 都完整存活。
///
/// Under detached multi-island mode, creating maps on two different islands
/// produces distinct global IDs, cartography scaling uses the same server-wide
/// counter, and all maps plus `last_id.dat` survive save/reload.
#[test]
fn multi_island_maps_allocate_unique_ids_and_round_trip_save() {
    use mc_bamboo_core::Hand;
    use mc_bamboo_data::items;
    use mc_bamboo_protocol::item::ItemStack;

    with_stack(|| {
        let root = std::env::temp_dir().join(format!(
            "bamboo-multi-island-maps-save-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let (id_a, id_b, center_a, center_b, colors_a, colors_b);
        {
            let mut scene = set_up(11, Harness::new_islands_with_save(2, &root, 2));
            let alice = scene
                .clients
                .iter()
                .find(|(g, _)| *g == 0)
                .map(|(_, c)| c.clone())
                .expect("第 0 组有玩家");
            let bob = scene
                .clients
                .iter()
                .find(|(g, _)| *g == 1)
                .map(|(_, c)| c.clone())
                .expect("第 1 组有玩家");
            let island_a = scene
                .server
                .world
                .islands
                .island_of_player(alice.id)
                .expect("Alice 在岛上");
            let island_b = scene
                .server
                .world
                .islands
                .island_of_player(bob.id)
                .expect("Bob 在岛上");
            assert_ne!(island_a, island_b, "两名玩家必须在两个不同的岛上");

            // 两名玩家在各自岛上同时拿到空地图，在同一刻右键创建已填充地图。
            for client in [&alice, &bob] {
                let region = scene.server.player_region(client);
                let player = region.players.get_mut(&client.id).expect("玩家");
                player.game_mode = GameMode::Survival;
                player.inventory.selected = 0;
                player.inventory.slots[36] = ItemStack::new(items::MAP, 1);
            }
            scene.server.request(
                &alice,
                PlayRequest::UseItem {
                    hand: Hand::Main,
                    sequence: 1,
                    yaw: 0.0,
                    pitch: 0.0,
                },
            );
            scene.server.request(
                &bob,
                PlayRequest::UseItem {
                    hand: Hand::Main,
                    sequence: 1,
                    yaw: 0.0,
                    pitch: 0.0,
                },
            );
            scene.server.tick();

            let stack_a = scene.server.player(&alice).inventory.slots[36].clone();
            let stack_b = scene.server.player(&bob).inventory.slots[36].clone();
            assert_eq!(stack_a.item, items::FILLED_MAP);
            assert_eq!(stack_b.item, items::FILLED_MAP);
            id_a = crate::map::stack_map_id(&stack_a).expect("Alice 拿到 map_id");
            id_b = crate::map::stack_map_id(&stack_b).expect("Bob 拿到 map_id");
            assert_ne!(id_a, id_b, "两个岛创建的地图 ID 绝不能撞号");
            assert_eq!(
                scene.server.world.master.global.maps().index.last_map_id,
                1,
                "全服 MapIndex 计到 1"
            );

            // 跑 16 刻让两张地图各自采样地形颜色。
            for _ in 0..16 {
                scene.server.tick();
            }
            let maps = scene.server.world.master.global.maps();
            let data_a = maps.get(id_a).expect("地图 A 在权威表里");
            let data_b = maps.get(id_b).expect("地图 B 在权威表里");
            center_a = (data_a.center_x, data_a.center_z);
            center_b = (data_b.center_x, data_b.center_z);
            assert_ne!(center_a, center_b, "两张地图各自对齐到所在岛的中心");
            colors_a = data_a.colors;
            colors_b = data_b.colors;
            assert!(
                colors_a.iter().any(|&c| c != 0),
                "地图 A 采到了非零地形颜色"
            );
            assert!(
                colors_b.iter().any(|&c| c != 0),
                "地图 B 采到了非零地形颜色"
            );

            scene.server.shut_down();
        }

        // 重开同一个世界目录：last_id.dat 与两张地图的数据都要完整读回。
        let reopened = Harness::new_islands_with_save(2, &root, 2);
        let restored = reopened.world.master.global.maps();
        assert_eq!(restored.index.last_map_id, 1);
        let ra = restored.get(id_a).expect("存档读回地图 A");
        let rb = restored.get(id_b).expect("存档读回地图 B");
        assert_eq!((ra.center_x, ra.center_z), center_a);
        assert_eq!((rb.center_x, rb.center_z), center_b);
        assert_eq!(ra.colors, colors_a);
        assert_eq!(rb.colors, colors_b);
        assert_eq!(
            reopened.world.islands.region.global.maps.index.last_map_id, 1,
            "重开后岛快照同步拿到全服地图表"
        );
        let _ = std::fs::remove_dir_all(&root);
    });
}

/// 多岛（分离驱动）下玩家手持、以及物品展示框挂载「中心在别的岛上」的地图：
/// 跨岛玩家照样更新颜色、照样收到地图包、且不互相驱逐另一座岛上的持有者；
/// 跨岛展示框照样登记标记并向本岛玩家发包。
///
/// Under detached multi-island mode, a player or item frame on island B using
/// a map centered on island A still updates colors, preserves both islands'
/// holders/markers, and emits map packets.
#[test]
fn multi_island_player_and_frame_sync_map_centered_on_another_island() {
    use mc_bamboo_blocks::entity::{Entity, EntityKind};
    use mc_bamboo_core::Direction;
    use mc_bamboo_data::state_map_color;
    use mc_bamboo_protocol::item::ItemStack;

    with_stack(|| {
        let mut scene = set_up(12, Harness::new_islands(2, 2));
        let alice = scene
            .clients
            .iter()
            .find(|(g, _)| *g == 0)
            .map(|(_, c)| c.clone())
            .expect("第 0 组有玩家");
        let bob = scene
            .clients
            .iter()
            .find(|(g, _)| *g == 1)
            .map(|(_, c)| c.clone())
            .expect("第 1 组有玩家");
        let island_a = scene
            .server
            .world
            .islands
            .island_of_player(alice.id)
            .expect("Alice 在岛上");
        let island_b = scene
            .server
            .world
            .islands
            .island_of_player(bob.id)
            .expect("Bob 在岛上");
        assert_ne!(island_a, island_b, "两名玩家必须在两个不同的岛上");

        let pos_a = scene.server.player(&alice).position();
        let pos_b = scene.server.player(&bob).position();

        // 在 Bob 脚下的岛 B 区块放一块金块（颜色醒目），验证 Bob 拿着中心在岛 A 侧的
        // 比例 4 地图（覆盖 2048×2048，同时涵盖岛 A 和岛 B）时照样采样并更新颜色。
        let bob_chunk = scene.server.player(&bob).chunk_pos();
        let gold_pos = at(bob_chunk, 8, Y, 8);
        {
            let region_b = scene.server.player_region(&bob);
            for dx in -16..=16 {
                for dz in -16..=16 {
                    let p = BlockPos::new(gold_pos.x + dx, Y, gold_pos.z + dz);
                    let _ = region_b.chunks.set_block(p, blocks::GOLD_BLOCK);
                }
            }
        }

        // 在全服 MapStore 建两张图：
        // - map 0: 比例 0，中心在岛 A（Alice 处），Bob 在岛 B 拿副本与挂展示框。
        // - map 1: 比例 4，中心对齐到 Alice 处（不在岛 B），同时覆盖岛 A 与岛 B 像素。
        let (id0, id1) = {
            let maps = scene.server.world.master.global.maps_mut();
            let id0 = maps.next_map_id();
            maps.insert(
                id0,
                crate::map::MapItemSavedData::create_fresh(
                    pos_a.x,
                    pos_a.z,
                    0,
                    true,
                    false,
                    crate::region::DIMENSION,
                ),
            );
            let id1 = maps.next_map_id();
            let mut d1 = crate::map::MapItemSavedData::create_fresh(
                0.0,
                0.0,
                4,
                true,
                false,
                crate::region::DIMENSION,
            );
            // 中心显式落在岛 A（(0, 0)），scale=4 覆盖 [-1024, 1023]，同时涵盖岛 B 的 Bob。
            d1.center_x = 0;
            d1.center_z = 0;
            maps.insert(id1, d1);
            (id0, id1)
        };
        assert_eq!(
            scene
                .server
                .world
                .islands
                .island_of_chunk(ChunkPos::new(0, 0)),
            Some(island_a),
            "map 0 与 map 1 的中心区块都在岛 A 上"
        );
        scene.server.world.publish_global();

        // Alice 主手拿 map 0；Bob 主手拿 map 1、副手拿 map 0。
        {
            let region_a = scene.server.player_region(&alice);
            let p = region_a.players.get_mut(&alice.id).expect("Alice");
            p.inventory.selected = 0;
            p.inventory.slots[36] = crate::map::make_filled_map(id0);
        }
        {
            let region_b = scene.server.player_region(&bob);
            let p = region_b.players.get_mut(&bob.id).expect("Bob");
            p.inventory.selected = 0;
            p.inventory.slots[36] = crate::map::make_filled_map(id1);
            p.inventory.slots[45] = crate::map::make_filled_map(id0);
        }

        let _ = alice.take_frames();
        let _ = bob.take_frames();
        let mut bob_got_map0 = false;
        let mut bob_got_map1 = false;
        for _ in 0..16 {
            scene.server.tick();
            for (map_id, _, _) in decode_map_packets(&bob.take_frames()) {
                if map_id == id0 {
                    bob_got_map0 = true;
                }
                if map_id == id1 {
                    bob_got_map1 = true;
                }
            }
        }
        assert!(
            bob_got_map0,
            "岛 B 的 Bob 持有中心在岛 A 的 map 0，必须收到地图包"
        );
        assert!(
            bob_got_map1,
            "岛 B 的 Bob 持有中心不在岛 B 的 map 1，必须收到地图包"
        );

        let maps = scene.server.world.master.global.maps();
        let data0 = maps.get(id0).expect("map 0");
        assert!(
            data0.colors.iter().any(|&c| c != 0),
            "岛 A 的 Alice 更新了 map 0 的颜色"
        );
        // 两个岛的持有者都必须留在 carried_by 里，互不驱逐。
        assert!(
            data0.carried_by.iter().any(|h| h.player == alice.id),
            "map 0 保留岛 A 的 Alice"
        );
        assert!(
            data0.carried_by.iter().any(|h| h.player == bob.id),
            "map 0 保留岛 B 的 Bob"
        );

        let data1 = maps.get(id1).expect("map 1");
        let scale1 = 1 << data1.scale;
        let bob_px = ((pos_b.x - f64::from(data1.center_x)).floor() as i32 / scale1 + 64) as usize;
        let bob_pz = ((pos_b.z - f64::from(data1.center_z)).floor() as i32 / scale1 + 64) as usize;
        let sampled_color = data1.colors[bob_px + bob_pz * 128] >> 2;
        assert_eq!(
            sampled_color,
            state_map_color(blocks::GOLD_BLOCK),
            "岛 B 的 Bob 拿着中心不在岛 B 的 map 1，必须把脚下金块采样进权威地图数据"
        );

        // 展示框测试：一张中心在岛 A（(0, 0)）、Bob 从未拿过的地图（id2，已有颜色），
        // 挂在岛 B 的物品展示框里；Bob 手上没有任何地图，照样收到展示框地图包并登记标记。
        let id2 = {
            let maps = scene.server.world.master.global.maps_mut();
            let id2 = maps.next_map_id();
            let mut d2 = crate::map::MapItemSavedData::create_fresh(
                0.0,
                0.0,
                4,
                true,
                false,
                crate::region::DIMENSION,
            );
            d2.center_x = 0;
            d2.center_z = 0;
            d2.set_color(
                10,
                10,
                crate::map::packed_color(
                    crate::map::map_color::STONE,
                    crate::map::Brightness::High,
                ),
            );
            maps.insert(id2, d2);
            id2
        };
        scene.server.world.publish_global();
        {
            let region_b = scene.server.player_region(&bob);
            let p = region_b.players.get_mut(&bob.id).expect("Bob");
            p.inventory.slots[36] = ItemStack::empty();
            p.inventory.slots[45] = ItemStack::empty();
            let mut frame = Entity::item_frame(
                BlockPos::new(gold_pos.x, Y + 1, gold_pos.z),
                Direction::South,
                false,
            );
            if let EntityKind::ItemFrame(f) = &mut frame.kind {
                f.item = crate::map::make_filled_map(id2);
            }
            region_b.entities.spawn(frame);
        }
        let _ = bob.take_frames();
        scene.server.tick();
        let frame_packets = decode_map_packets(&bob.take_frames());
        assert!(
            frame_packets
                .iter()
                .any(|&(id, has_decs, patch_w)| id == id2 && has_decs && patch_w == 128),
            "岛 B 的展示框装着中心在岛 A 的 map {id2}，必须向岛 B 玩家发完整地图包和展示框标记：{frame_packets:?}"
        );
        assert_eq!(
            scene
                .server
                .world
                .master
                .global
                .maps()
                .get(id2)
                .expect("map 2")
                .frame_markers()
                .len(),
            1,
            "岛 B 展示框标记写入全服权威地图数据"
        );
    });
}

/// 拆岛再合岛后，拆前和分岛期间建立与更新的全部地图数据都完整保留、不丢失不覆盖。
///
/// Splitting and re-merging islands preserves every map created and explored
/// before and during the split without losing or overwriting data.
#[test]
fn multi_island_split_and_remerge_preserves_all_maps() {
    use mc_bamboo_core::Hand;
    use mc_bamboo_data::items;
    use mc_bamboo_protocol::item::ItemStack;

    with_stack(|| {
        let mut scene = set_up(13, Harness::new_islands(2, 2));
        let alice = scene
            .clients
            .iter()
            .find(|(g, _)| *g == 0)
            .map(|(_, c)| c.clone())
            .expect("第 0 组有玩家");
        let bob = scene
            .clients
            .iter()
            .find(|(g, _)| *g == 1)
            .map(|(_, c)| c.clone())
            .expect("第 1 组有玩家");

        // 1. 先全部合并成一个岛，在单岛上由 Alice 创建 map 0 并采样。
        {
            let world = &mut scene.server.world;
            let partition = world.master.host.partition_mut().expect("多岛");
            let ids = partition.island_ids();
            for &other in &ids[1..] {
                assert!(partition.force_merge(ids[0], other));
            }
            world.master.host.take_partition_ops();
            world.settle_islands();
            assert_eq!(world.islands.len(), 1, "先合为一个岛");
        }
        {
            let region = scene.server.player_region(&alice);
            let p = region.players.get_mut(&alice.id).expect("Alice");
            p.game_mode = GameMode::Survival;
            p.inventory.selected = 0;
            p.inventory.slots[36] = ItemStack::new(items::MAP, 1);
        }
        scene.server.request(
            &alice,
            PlayRequest::UseItem {
                hand: Hand::Main,
                sequence: 1,
                yaw: 0.0,
                pitch: 0.0,
            },
        );
        for _ in 0..16 {
            scene.server.tick();
        }
        let colors0_before_split = scene
            .server
            .world
            .master
            .global
            .maps()
            .get(0)
            .expect("map 0")
            .colors;
        assert!(
            colors0_before_split.iter().any(|&c| c != 0),
            "拆岛前 map 0 已采样出颜色"
        );

        // 2. 拆岛：Alice 和 Bob 分到不同岛上。
        {
            let world = &mut scene.server.world;
            let home = world.islands.ids()[0];
            let partition = world.master.host.partition_mut().expect("多岛");
            assert!(!partition.force_split(home).is_empty());
            world.master.host.take_partition_ops();
            world.settle_islands();
        }
        assert!(scene.server.world.islands.len() >= 2, "已拆成多个岛");
        assert_ne!(
            scene.server.world.islands.island_of_player(alice.id),
            scene.server.world.islands.island_of_player(bob.id),
            "拆岛后 Alice 与 Bob 在不同岛"
        );

        // 分岛期间：Bob 在岛 B 创建 map 1（必须拿到 id=1）并采样。
        {
            let region = scene.server.player_region(&bob);
            let p = region.players.get_mut(&bob.id).expect("Bob");
            p.game_mode = GameMode::Survival;
            p.inventory.selected = 0;
            p.inventory.slots[36] = ItemStack::new(items::MAP, 1);
        }
        scene.server.request(
            &bob,
            PlayRequest::UseItem {
                hand: Hand::Main,
                sequence: 1,
                yaw: 0.0,
                pitch: 0.0,
            },
        );
        for _ in 0..16 {
            scene.server.tick();
        }
        assert_eq!(
            crate::map::stack_map_id(&scene.server.player(&bob).inventory.slots[36]),
            Some(1),
            "拆岛后 Bob 在另一座岛建图必须拿到 id=1"
        );
        let colors1_during_split = scene
            .server
            .world
            .master
            .global
            .maps()
            .get(1)
            .expect("map 1")
            .colors;
        assert!(
            colors1_during_split.iter().any(|&c| c != 0),
            "分岛期间 map 1 已采样出颜色"
        );

        // 3. 合岛：把所有岛再合回一个岛。
        {
            let world = &mut scene.server.world;
            let partition = world.master.host.partition_mut().expect("多岛");
            let ids = partition.island_ids();
            for &other in &ids[1..] {
                assert!(partition.force_merge(ids[0], other));
            }
            world.master.host.take_partition_ops();
            world.settle_islands();
            assert_eq!(world.islands.len(), 1, "已全部合回一个岛");
        }
        scene.server.tick();

        let maps = scene.server.world.master.global.maps();
        assert_eq!(maps.index.last_map_id, 1);
        assert_eq!(
            maps.get(0).expect("合岛后 map 0 仍在").colors,
            colors0_before_split,
            "拆岛再合岛后 map 0 颜色无损"
        );
        assert_eq!(
            maps.get(1).expect("合岛后 map 1 仍在").colors,
            colors1_during_split,
            "拆岛再合岛后 map 1 颜色无损"
        );
        assert!(
            scene
                .server
                .world
                .islands
                .region
                .global
                .maps
                .get(0)
                .is_some()
                && scene
                    .server
                    .world
                    .islands
                    .region
                    .global
                    .maps
                    .get(1)
                    .is_some(),
            "合岛后的岛快照能同时读到 map 0 与 map 1"
        );
    });
}
