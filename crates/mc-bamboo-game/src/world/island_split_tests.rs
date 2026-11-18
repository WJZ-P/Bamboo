//! 岛的拆合（岛的实施计划第 6 节第 9 步）的离线验证。还不在运行时调用，所以这里直接
//! 拿一个已经跑起来的世界的岛，用和分离驱动相同的 [`run_detached_island`] 一刻一刻地
//! 驱动（只碰岛，不碰主控），在随机的时刻按随机的组拆开、合回来，和从没拆过的同一个
//! 世界逐刻比内容。
//!
//! 场景：三组相隔 20 多个区块的内容，每组一到两名玩家、一条漏斗链、一座在烧的熔炉、
//! 一对互相盯着的侦测器（永不停的计划刻）、一格流动的水（流体刻）、一块掉下来的沙子
//! （实体）、地上的物品、讲台、命令方块、带组件的箱子、红石记忆、等着重查的容器、
//! 打开着的菜单、待发的讲台格子和没确认的传送。`randomTickSpeed = 0`，机器都不取
//! 随机数（计划 6.2 第 2 条的前提）。
//!
//! Offline checks for island split and merge (plan step 9): drive a running
//! world's island tick by tick with [`run_detached_island`] (island only),
//! split and merge by random groups at random ticks, and compare the content
//! with the same world that never split.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    hash::{Hash, Hasher},
};

use mc_bamboo_blocks::{
    BlockState, HopperEntity, LecternFacts, TickPriority,
    behaviour::machines::FurnaceEntity,
    entity::{Entity, JavaRandom},
    state::{FACING, FACING_HOPPER, HAS_BOOK},
};
use mc_bamboo_core::{BlockPos, ChunkPos, Direction, Vec3};
use mc_bamboo_data::{CookingKind, block_id_of_state, blocks, items, registries::fluid};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry},
    item::ItemStack,
    text::Component,
};

use super::{
    encode_player_save, run_detached_island,
    tests::{Client, Harness},
};
use crate::{
    container::{ContainerKind, entry_in},
    inbound::PlayerId,
    island::{Island, IslandDrive},
    island_partition::SectionPos,
    region::{ClockParts, IslandPart, Region, RegionTick},
};

/// 远期计划刻的延迟：比任何一次测试跑的刻数都长，一直排着。
pub(super) const PARKED_DELAY: i32 = 400;
/// 两半分开跑到第几刻时各自做一次「中途的事」（见 [`mid_run`]）。
const MID_RUN_STEP: u64 = 2;

/// 地面上一格。
pub(super) const Y: i32 = -60;
/// 两组之间的基本间隔（区块）。
const GROUP_GAP: i32 = 24;

pub(super) fn rng(seed: u64) -> impl FnMut() -> u64 {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    }
}

/// 测试线程的栈给足：`World` 很大，调试构建按值搬它时栈余量很薄。
pub(super) fn with_stack(test: impl FnOnce() + Send + 'static) {
    let handle = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(test)
        .expect("开测试线程");
    if let Err(payload) = handle.join() {
        std::panic::resume_unwind(payload);
    }
}

/// 一个世界里算过的区块内容哈希，按（位置，版本号）记。
pub(super) type ChunkHashes =
    std::collections::HashMap<(ChunkPos, mc_bamboo_world::store::ChunkRevision), u64>;

pub(super) struct Scene {
    pub(super) server: Harness,
    pub(super) centers: Vec<ChunkPos>,
    /// （组号，客户端）。
    pub(super) clients: Vec<(usize, Client)>,
    pub(super) hashes: ChunkHashes,
}

/// 这个世界的岛现在的指纹。
pub(super) fn print(scene: &mut Scene, strict: bool) -> Vec<String> {
    let Scene {
        server,
        centers,
        hashes,
        ..
    } = scene;
    fingerprint(&server.world.islands.region, centers, strict, hashes)
}

pub(super) fn chebyshev(a: ChunkPos, b: ChunkPos) -> i32 {
    (a.x - b.x).abs().max((a.z - b.z).abs())
}

pub(super) fn group_of(centers: &[ChunkPos], chunk: ChunkPos) -> usize {
    (0..centers.len())
        .min_by_key(|&index| chebyshev(centers[index], chunk))
        .expect("至少一组")
}

/// 三组的中心：出生点那组，加上按种子挑的两个方向。
pub(super) fn centers_for(seed: u64) -> Vec<ChunkPos> {
    let mut next = rng(seed);
    let directions = [(1, 0), (0, 1), (-1, 0), (0, -1), (1, 1), (-1, 1), (1, -1)];
    let first = (next() % directions.len() as u64) as usize;
    let mut second = (next() % directions.len() as u64) as usize;
    if second == first {
        second = (second + 1) % directions.len();
    }
    let mut out = vec![ChunkPos::new(0, 0)];
    for index in [first, second] {
        let (dx, dz) = directions[index];
        let jitter = (next() % 4) as i32;
        out.push(ChunkPos::new(
            dx * (GROUP_GAP + jitter),
            dz * (GROUP_GAP + jitter),
        ));
    }
    out
}

pub(super) fn at(center: ChunkPos, x: i32, y: i32, z: i32) -> BlockPos {
    BlockPos::new(center.x * 16 + x, y, center.z * 16 + z)
}

fn build(seed: u64) -> Scene {
    let mut scene = set_up(seed, Harness::new_loaded_with_drive(2, IslandDrive::Lockstep));
    furnish_scene(&mut scene);
    scene
}

/// 搭好玩家：每组一到两名玩家，进服后放到组的中心，收完区块。还没有放机器。
/// 多岛世界里玩家先进家岛，放过去以后由主控搬到那一组的岛上。
///
/// Place the players (one or two per group at the group centre, all chunks
/// received) without the machines. In a multi-island world they join home
/// and the master moves them to their group's island.
pub(super) fn set_up(seed: u64, mut server: Harness) -> Scene {
    server
        .world
        .master
        .global
        .rules_mut()
        .assign(mc_bamboo_data::gamerules::RANDOM_TICK_SPEED, "0");
    server.world.publish_global();
    let centers = centers_for(seed);
    let mut next = rng(seed ^ 0x5555);
    let mut clients = Vec::new();
    for (group, &center) in centers.iter().enumerate() {
        let count = 1 + (next() % 2) as usize;
        for index in 0..count {
            let client = server.join_and_settle(&format!("g{group}p{index}"));
            let x = f64::from(center.x * 16) + 8.5 + index as f64 * 3.0;
            let z = f64::from(center.z * 16) + 8.5;
            if (group, index) != (0, 0) {
                server.move_to(&client, x, z);
            }
            clients.push((group, client));
        }
    }
    for (_, client) in &clients {
        server.receive_all_chunks(client);
    }
    Scene {
        server,
        centers,
        clients,
        hashes: ChunkHashes::new(),
    }
}

/// 拿着 `chunk` 的岛的 `Region`；哪个岛都没有时是家岛。
pub(super) fn region_at(world: &mut super::World, chunk: ChunkPos) -> &mut Region {
    let id = world
        .islands
        .island_of_chunk(chunk)
        .unwrap_or(mc_bamboo_world::director::HOME_REGION);
    &mut world.islands.get_mut(id).expect("在表里").region
}

/// 这名玩家所在的岛的 `Region`。
pub(super) fn region_of_player(world: &mut super::World, id: PlayerId) -> &mut Region {
    let island = world.islands.island_of_player(id).expect("玩家在世界里");
    &mut world.islands.get_mut(island).expect("在表里").region
}

/// 每组放上机器、打开菜单、排好待发的讲台格子和一条方块事件（见 [`furnish`]）。
pub(super) fn furnish_scene(scene: &mut Scene) {
    let Scene {
        server,
        centers,
        clients,
        ..
    } = scene;
    let now = server.world.islands.region.tick;
    for (group, &center) in centers.iter().enumerate() {
        let region = region_at(&mut server.world, center);
        assert!(
            region.tick_levels.chunk_ticks_entities(center),
            "第 {group} 组的中心区块要能跑实体"
        );
        furnish(region, center, group as i32, now);
    }
    for (group, client) in clients.iter() {
        let region = region_of_player(&mut server.world, client.id);
        let center = centers[*group];
        assert!(
            crate::menu::open_block(region, client.id, at(center, 2, Y, 2)),
            "打开箱子菜单"
        );
        region
            .lecterns
            .insert_pending_for_test(client.id, vec![0], now.plus(400));
    }
    // 方块事件和还没广播的方块 / 光照变化平时在边界上是空的，这里各排一条，好让
    // 「漏搬一类」的核对每一类都有东西可查。
    for &center in centers.iter() {
        let region = region_at(&mut server.world, center);
        region.push_block_event_for_test(mc_bamboo_blocks::BlockEvent {
            pos: at(center, 15, Y, 15),
            block: block_id_of_state(blocks::STONE).expect("石头"),
            param_a: 0,
            param_b: 0,
        });
    }
}

/// 一组的机器和各张带位置的表。`salt` 让各组略有不同。
pub(super) fn furnish(region: &mut Region, center: ChunkPos, salt: i32, now: RegionTick) {
    let hopper_down = BlockState(blocks::HOPPER)
        .with(FACING_HOPPER, Direction::Down)
        .0;
    // 漏斗链：上面的箱子 → 两个漏斗 → 下面的箱子。
    let top = at(center, 2, Y + 3, 2);
    let first = at(center, 2, Y + 2, 2);
    let second = at(center, 2, Y + 1, 2);
    let bottom = at(center, 2, Y, 2);
    for (pos, state) in [
        (top, blocks::CHEST),
        (first, hopper_down),
        (second, hopper_down),
        (bottom, blocks::CHEST),
    ] {
        region.chunks.set_block(pos, state);
    }
    entry_in(&mut region.containers, top, ContainerKind::Chest).slots[0] =
        ItemStack::new(items::DIAMOND, 20 + salt);
    entry_in(&mut region.containers, top, ContainerKind::Chest).slots[1] =
        ItemStack::new(items::COAL, 7);
    entry_in(&mut region.containers, first, ContainerKind::Hopper);
    entry_in(&mut region.containers, second, ContainerKind::Hopper);
    entry_in(&mut region.containers, bottom, ContainerKind::Chest);
    for pos in [first, second] {
        region.block_entities.remove(pos);
        region
            .block_entities
            .insert(Box::new(HopperEntity::new(pos)));
    }
    let mut name = ComponentPatch::default();
    name.put(
        mc_bamboo_data::registries::data_component_type::CUSTOM_NAME,
        PatchEntry::Set(ComponentValue::CustomName(Component::text("岛"))),
    );
    region.block_items.insert(top, name);

    // 一个单独的漏斗，上面躺着物品：等着被捡。
    let catcher = at(center, 12, Y, 12);
    region.chunks.set_block(catcher, hopper_down);
    entry_in(&mut region.containers, catcher, ContainerKind::Hopper);
    region.block_entities.remove(catcher);
    region
        .block_entities
        .insert(Box::new(HopperEntity::new(catcher)));
    region.entities.spawn(Entity::ground_item_with_delta(
        Vec3::new(
            f64::from(catcher.x) + 0.5,
            f64::from(Y + 1),
            f64::from(catcher.z) + 0.5,
        ),
        ItemStack::new(items::IRON_INGOT, 3 + salt),
        Vec3::default(),
    ));
    region.entities.spawn(Entity::ground_item_with_delta(
        Vec3::new(
            f64::from(at(center, 6, Y, 13).x) + 0.5,
            f64::from(Y),
            f64::from(at(center, 6, Y, 13).z) + 0.5,
        ),
        ItemStack::new(items::GOLD_INGOT, 1),
        Vec3::default(),
    ));

    // 在烧的熔炉。
    let furnace = at(center, 5, Y, 2);
    region.chunks.set_block(furnace, blocks::FURNACE);
    region.block_entities.remove(furnace);
    let mut entity = FurnaceEntity::new(furnace, CookingKind::Smelting);
    entity.set_item(0, ItemStack::new(items::RAW_IRON, 6));
    entity.set_item(1, ItemStack::new(items::COAL, 2));
    region.block_entities.insert(Box::new(entity));

    // 两个互相盯着的侦测器：永不停的计划刻。
    let west = at(center, 5, Y, 6);
    let east = at(center, 6, Y, 6);
    region.chunks.set_block(
        west,
        BlockState(blocks::OBSERVER).with(FACING, Direction::East).0,
    );
    region.chunks.set_block(
        east,
        BlockState(blocks::OBSERVER).with(FACING, Direction::West).0,
    );
    region.block_ticks.schedule(
        west,
        block_id_of_state(blocks::OBSERVER).expect("侦测器"),
        2,
        TickPriority::Normal,
    );

    // 三条远期的计划刻（那几格没有这个方块，到期时跳过）：拆开以后两边会在同一个
    // （时刻，优先级）上再排几条，合回来后每组自己的先后要和从没拆过一样。
    for index in 0..3 {
        region.block_ticks.schedule(
            at(center, 1 + index, Y + 5, 13),
            block_id_of_state(blocks::STONE).expect("石头"),
            PARKED_DELAY,
            TickPriority::Normal,
        );
    }

    // 一格水，流开要好几刻。
    let water = at(center, 10, Y, 8 + salt);
    region.chunks.set_block(water, blocks::WATER);
    region
        .block_ticks
        .schedule_fluid(water, fluid::WATER, 5, TickPriority::Normal);

    // 一块悬空的沙子，下一刻开始掉。
    let sand = at(center, 8, Y + 6, 3);
    region.chunks.set_block(sand, blocks::SAND);
    region.block_ticks.schedule(
        sand,
        block_id_of_state(blocks::SAND).expect("沙子"),
        2,
        TickPriority::Normal,
    );

    // 讲台、命令方块、红石记忆。
    let lectern = at(center, 3, Y, 9);
    region
        .chunks
        .set_block(lectern, BlockState(blocks::LECTERN).with(HAS_BOOK, true).0);
    region.lecterns.books.insert(
        lectern,
        crate::lectern::Book::for_tests(ItemStack::new(items::BOOK, 1), 1, 4, true),
    );
    region.redstone.set_lectern(
        lectern,
        LecternFacts {
            page: 1,
            page_count: 4,
            has_content: true,
        },
    );
    let command = at(center, 4, Y, 9);
    region.chunks.set_block(command, blocks::COMMAND_BLOCK);
    crate::command_block::on_placed(region, command);
    region
        .redstone
        .set_comparator_output(at(center, 7, Y, 9), 5 + salt);
    region
        .redstone
        .record_torch_toggle(at(center, 8, Y, 9), now.game_time());

    // 方块没了、等菜单关掉的容器，带着重查时刻。
    let ghost = at(center, 13, Y, 2);
    let entry = entry_in(&mut region.containers, ghost, ContainerKind::Chest);
    entry.dead = true;
    entry.recheck_at = Some(now.plus(300));
}

/// 两半分开跑的中途，每组各做一次同样的事（从没拆过的对照在同一刻对每组都做）：
/// 新放一个漏斗（方块实体两边都新登记，ordinal 会撞，合并时要重排），再在拆分前那三条
/// 远期刻的同一个（时刻，优先级）上排三条（顺序号两边各自往上数，合并后每组自己的先后
/// 要不变）。`elapsed` 是从搭好场景起跑了几刻，`here` 说哪几组在这个岛上。
pub(super) fn mid_run(region: &mut Region, centers: &[ChunkPos], here: &BTreeSet<usize>, elapsed: i32) {
    let hopper_down = BlockState(blocks::HOPPER)
        .with(FACING_HOPPER, Direction::Down)
        .0;
    for (group, &center) in centers.iter().enumerate() {
        if !here.contains(&group) {
            continue;
        }
        let chest = at(center, 14, Y + 2, 6);
        let hopper = at(center, 14, Y + 1, 6);
        region.chunks.set_block(chest, blocks::CHEST);
        region.chunks.set_block(hopper, hopper_down);
        entry_in(&mut region.containers, chest, ContainerKind::Chest).slots[0] =
            ItemStack::new(items::EMERALD, 9);
        entry_in(&mut region.containers, hopper, ContainerKind::Hopper);
        region.block_entities.remove(hopper);
        region
            .block_entities
            .insert(Box::new(HopperEntity::new(hopper)));
        for index in 0..3 {
            region.block_ticks.schedule(
                at(center, 1 + index, Y + 5, 14),
                block_id_of_state(blocks::STONE).expect("石头"),
                PARKED_DELAY - elapsed,
                TickPriority::Normal,
            );
        }
    }
}

/// 一刻：和分离驱动在 tick 线程上跑的同一段，再把出站缓冲刷给假客户端。排队的全局
/// 改动（计分板准则）这里没有主控来应用，丢掉。
pub(super) fn step(island: &mut Island) {
    run_detached_island(island);
    island.region.links.flush();
    island.effects.clear();
}

pub(super) fn hash_of(value: &impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// 一个岛的内容指纹，按组分开：方块和光照（存档字节）、计划刻（含执行顺序）、方块实体
/// 的运行顺序和日程、容器、组件、命令方块、红石记忆、讲台、实体（`strict` 为假时去掉
/// 网络 ID 和由 ID 派生的随机源）、追踪配对、tick 档位、玩家状态、待存标记。
/// 最后一项是全岛共有的：时钟、讲台表、红石记忆。
///
/// `strict` 为真时连网络 ID、版本号和跨组的先后都比：拆了马上合必须逐位相同。
/// 为假时只比每组自己的先后：两组各自单独跑时互不影响，跨组的先后没有意义。
pub(super) fn fingerprint(
    region: &Region,
    centers: &[ChunkPos],
    strict: bool,
    hashes: &mut ChunkHashes,
) -> Vec<String> {
    let group = |chunk: ChunkPos| group_of(centers, chunk);
    let mut out = vec![String::new(); centers.len() + 1];
    let global = centers.len();
    for pos in region.chunks.loaded_positions() {
        // 方块和光照：版本号没变内容就没变，同一个世界里按（位置，版本号）记住算过的哈希。
        let revision = region.chunks.revision(pos);
        let content = *hashes
            .entry((pos, revision))
            .or_insert_with(|| hash_of(&format!("{:?}", region.chunks.column_clone(pos))));
        let _ = writeln!(
            out[group(pos)],
            "chunk {pos:?} pristine {} {content}",
            region.chunks.is_pristine(pos)
        );
        if strict {
            let _ = writeln!(
                out[global],
                "revision {pos:?} {:?}",
                region.chunks.revision(pos)
            );
        }
    }
    for tick in region.block_ticks.pending() {
        let line = format!("tick {:?}", tick);
        if strict {
            let _ = writeln!(out[global], "{line}");
        }
        let _ = writeln!(out[group(tick.pos.chunk())], "{line}");
    }
    for tick in region.block_ticks.pending_fluids() {
        let line = format!("fluid {:?}", tick);
        if strict {
            let _ = writeln!(out[global], "{line}");
        }
        let _ = writeln!(out[group(tick.pos.chunk())], "{line}");
    }
    let mut cells = Vec::new();
    region.block_entities.visit(|pos, entity| {
        cells.push((
            pos,
            format!(
                "{:?}",
                mc_bamboo_blocks::archive::write_block_entity_additional(entity)
            ),
        ));
    });
    cells.sort();
    for (pos, data) in cells {
        let _ = writeln!(out[group(pos.chunk())], "block entity {pos:?} {data}");
    }
    let (now, rows) = region.block_entities.schedule_rows();
    let _ = writeln!(out[global], "block entities now {now}");
    for (pos, wake_at) in rows {
        let line = format!("ticker {pos:?} {wake_at:?}");
        if strict {
            let _ = writeln!(out[global], "{line}");
        }
        let _ = writeln!(out[group(pos.chunk())], "{line}");
    }
    for (pos, entry) in &region.containers {
        let _ = writeln!(out[group(pos.chunk())], "container {pos:?} {entry:?}");
    }
    for (pos, patch) in &region.block_items {
        let _ = writeln!(out[group(pos.chunk())], "items {pos:?} {patch:?}");
    }
    for (pos, data) in &region.command_blocks {
        let _ = writeln!(out[group(pos.chunk())], "command {pos:?} {data:?}");
    }
    let mut ranks: BTreeMap<i32, (usize, usize)> = BTreeMap::new();
    let mut counts = vec![0; centers.len()];
    for mut entity in region.entities.iter() {
        let g = group(entity.block_position().chunk());
        ranks.insert(entity.id, (g, counts[g]));
        counts[g] += 1;
        if !strict {
            entity.id = 0;
            entity.random = JavaRandom::new(0);
        }
        let _ = writeln!(out[g], "entity {entity:?}");
    }
    let player_group: BTreeMap<PlayerId, usize> = region
        .players
        .values()
        .map(|player| (player.id, group(player.chunk_pos())))
        .collect();
    for (id, seen_by) in region.entity_tracking.pairs() {
        let (g, key) = match ranks.get(&id) {
            Some(&(g, rank)) if !strict => (g, format!("#{rank}")),
            Some(&(g, _)) => (g, format!("{id}")),
            None => (
                seen_by
                    .first()
                    .and_then(|player| player_group.get(player).copied())
                    .unwrap_or(global),
                format!("gone {}", if strict { id } else { 0 }),
            ),
        };
        let _ = writeln!(out[g], "tracked {key} {seen_by:?}");
    }
    for pos in region.tick_levels.delivered() {
        let _ = writeln!(
            out[group(pos)],
            "levels {pos:?} {:?} {} {}",
            region.tick_levels.tiers(pos),
            region.tick_levels.chunk_ticks_blocks(pos),
            region.tick_levels.chunk_ticks_entities(pos)
        );
    }
    for pos in region.dirty.pending() {
        let _ = writeln!(out[group(*pos)], "dirty {pos:?}");
    }
    for (id, player) in &region.players {
        let save = encode_player_save(player).map(|(uuid, bytes)| (uuid, hash_of(&bytes)));
        let tracked: Vec<ChunkPos> = region
            .chunks
            .loaded_positions()
            .into_iter()
            .filter(|&pos| player.chunks.is_chunk_tracked(pos))
            .collect();
        let _ = writeln!(
            out[player_group[id]],
            "player {id:?} {:?} {} {} {:?} {save:?} seen_by {:?} tracks {tracked:?} menu {} ip {:?} acted {} open {}",
            player.position(),
            player.yaw,
            player.pitch,
            player.teleport,
            player.sync.seen_by(),
            region.menus.contains_key(id),
            region.peer_ips.get(id),
            region.last_action_millis.contains_key(id),
            region.links.is_open(*id),
        );
    }
    let _ = writeln!(
        out[global],
        "clock {:?} ticks now {} lecterns {:?} redstone {}",
        region.tick,
        region.block_ticks.now(),
        region.lecterns,
        region.redstone.debug_rows()
    );
    out
}

pub(super) fn island(scene: &mut Scene) -> &mut Island {
    &mut scene.server.world.islands
}

/// 按组号挑出要拆走的区块和玩家。
fn parts_of(scene: &Scene, groups: &BTreeSet<usize>) -> (BTreeSet<ChunkPos>, BTreeSet<PlayerId>) {
    let chunks = scene
        .server
        .world
        .islands
        .region
        .chunks
        .loaded_positions()
        .into_iter()
        .filter(|&pos| groups.contains(&group_of(&scene.centers, pos)))
        .collect();
    let players = scene
        .clients
        .iter()
        .filter(|(group, _)| groups.contains(group))
        .map(|(_, client)| client.id)
        .collect();
    (chunks, players)
}

/// 随机挑一个非空的真子集。
fn random_groups(next: &mut impl FnMut() -> u64, count: usize) -> BTreeSet<usize> {
    loop {
        let mask = next() % (1 << count);
        if mask != 0 && mask != (1 << count) - 1 {
            return (0..count).filter(|bit| mask & (1 << bit) != 0).collect();
        }
    }
}

fn split(scene: &mut Scene, groups: &BTreeSet<usize>) -> Island {
    let (chunks, players) = parts_of(scene, groups);
    let origin = SectionPos::of(scene.centers[*groups.first().expect("非空")]);
    let island = island(scene);
    let random = island.region.derive_island_random(0, origin);
    island
        .split_off_island(1, &chunks, &players, random)
        .expect("tick 边界上可以拆")
}

/// 一个岛上放错了地方的类别：`inside` 为真时它只该有 `chunks` / `players` 的东西，为假时
/// 一样都不该有。每一类都查：按位置的看位置所在区块，按玩家的看玩家，岛这一层的信箱
/// 和存盘名单也看位置。
pub(super) fn misplaced(
    island: &Island,
    chunks: &BTreeSet<ChunkPos>,
    players: &BTreeSet<PlayerId>,
    inside: bool,
) -> BTreeSet<String> {
    let region = &island.region;
    let wrong_chunk = |pos: ChunkPos| chunks.contains(&pos) != inside;
    let wrong_block = |pos: &BlockPos| wrong_chunk(pos.chunk());
    let wrong_player = |id: &PlayerId| players.contains(id) != inside;
    let mut out = BTreeSet::new();
    let mut check = |name: &str, wrong: bool| {
        if wrong {
            out.insert(name.to_owned());
        }
    };
    check(
        "Chunks",
        region
            .chunks
            .loaded_positions()
            .into_iter()
            .any(wrong_chunk),
    );
    check(
        "TickLevels",
        region.tick_levels.delivered().into_iter().any(wrong_chunk),
    );
    check(
        "BlockTicks",
        region
            .block_ticks
            .pending()
            .any(|tick| wrong_block(&tick.pos))
            || region
                .block_ticks
                .pending_fluids()
                .any(|tick| wrong_block(&tick.pos)),
    );
    check(
        "BlockEvents",
        region
            .queued_block_event_chunks()
            .into_iter()
            .any(wrong_chunk),
    );
    let mut entity_cells = Vec::new();
    region.block_entities.visit(|pos, _| entity_cells.push(pos));
    check(
        "BlockEntities",
        entity_cells.iter().any(wrong_block)
            || region
                .block_entities
                .schedule_rows()
                .1
                .iter()
                .any(|(pos, _)| wrong_block(pos)),
    );
    check(
        "Redstone",
        region.redstone.positions().iter().any(wrong_block),
    );
    check("Containers", region.containers.keys().any(wrong_block));
    check("BlockItems", region.block_items.keys().any(wrong_block));
    check(
        "CommandBlocks",
        region.command_blocks.keys().any(wrong_block),
    );
    check("Books", region.lecterns.books.keys().any(wrong_block));
    check(
        "Entities",
        region
            .entities
            .iter_public()
            .any(|public| wrong_chunk(public.chunk_position())),
    );
    check(
        "EntityTracking",
        region
            .entity_tracking
            .pairs()
            .iter()
            .any(|(_, seen_by)| seen_by.iter().any(wrong_player)),
    );
    check(
        "Dirty",
        region.dirty.pending().iter().any(|pos| wrong_chunk(*pos)),
    );
    check(
        "LightChanges",
        region.light_changes.keys().any(|pos| wrong_chunk(*pos)),
    );
    check(
        "BlockChanges",
        region.block_changes.changed.iter().any(wrong_block),
    );
    check(
        "ChunkCache",
        region.chunk_cache.positions().into_iter().any(wrong_chunk),
    );
    check("Players", region.players.keys().any(wrong_player));
    check(
        "PlayerGrid",
        region.player_grid.placed_players().iter().any(wrong_player),
    );
    check("Links", region.links.players().iter().any(wrong_player));
    check("Menus", region.menus.keys().any(wrong_player));
    let (views, pending) = region.lecterns.player_keys();
    check(
        "LecternViews",
        views.iter().chain(&pending).any(wrong_player),
    );
    check("PeerIps", region.peer_ips.keys().any(wrong_player));
    check(
        "LastAction",
        region.last_action_millis.keys().any(wrong_player),
    );
    check(
        "Mailbox",
        island.mailbox.positions().into_iter().any(wrong_chunk),
    );
    check(
        "SavePlan",
        island
            .save_plan
            .remark
            .iter()
            .chain(&island.save_plan.encode)
            .any(|pos| wrong_chunk(*pos)),
    );
    let report = &island.save_report;
    check(
        "SaveReport",
        report
            .encoded
            .iter()
            .any(|encoded| wrong_chunk(encoded.pos))
            || report.changes.as_ref().is_some_and(|changes| {
                changes
                    .changed
                    .iter()
                    .any(|changed| wrong_chunk(changed.pos))
                    || changes.content.iter().any(|pos| wrong_chunk(*pos))
            }),
    );
    out
}

/// 拆开以后两边各自的核对都是空的。
fn assert_owned(
    parent: &Island,
    child: &Island,
    chunks: &BTreeSet<ChunkPos>,
    players: &BTreeSet<PlayerId>,
) {
    let left = misplaced(parent, chunks, players, false);
    let right = misplaced(child, chunks, players, true);
    assert!(
        left.is_empty() && right.is_empty(),
        "拆完放错了地方：原岛 {left:?}，新岛 {right:?}"
    );
}

/// 场景真的在动：每组的侦测器还在互相触发、漏斗把东西运到了下面的箱子、熔炉在烧、
/// 水流开了、沙子掉到了地上、实体有人看着。拆合测试只有在这些都发生时才有意义。
pub(super) fn assert_alive(scene: &Scene) {
    let region = &scene.server.world.islands.region;
    let observer = block_id_of_state(blocks::OBSERVER).expect("侦测器");
    for (group, &center) in scene.centers.iter().enumerate() {
        assert!(
            region
                .block_ticks
                .pending()
                .any(|tick| tick.block == observer
                    && group_of(&scene.centers, tick.pos.chunk()) == group),
            "第 {group} 组的侦测器时钟停了"
        );
        let bottom = region
            .containers
            .get(&at(center, 2, Y, 2))
            .expect("下面的箱子");
        assert!(
            bottom.slots.iter().any(|stack| !stack.is_empty()),
            "第 {group} 组的漏斗链没有运东西"
        );
        assert_eq!(
            region.chunks.block(at(center, 8, Y, 3)),
            Some(blocks::SAND),
            "第 {group} 组的沙子没有落地"
        );
        assert_ne!(
            region.chunks.block(at(center, 11, Y, 8 + group as i32)),
            Some(blocks::AIR),
            "第 {group} 组的水没有流开"
        );
    }
    assert!(
        region
            .entity_tracking
            .pairs()
            .iter()
            .any(|(_, seen_by)| !seen_by.is_empty()),
        "实体没有人看着"
    );
}

/// 拆了马上合回来，和从没拆过的世界逐位相同（含网络 ID、版本号、跨组的先后），之后
/// 继续跑也逐刻相同，每名玩家收到的字节也逐位相同。随机的组、随机的时刻、几个种子。
#[test]
fn an_island_split_and_merged_back_at_once_is_bit_identical() {
    with_stack(|| {
        for seed in 1..=4_u64 {
            let mut split_world = build(seed);
            let mut control = build(seed);
            assert_eq!(
                print(&mut split_world, true),
                print(&mut control, true),
                "seed {seed}：两份同样搭的世界一开始就该相同"
            );
            let mut next = rng(seed ^ 0xA5A5);
            let trips: BTreeSet<u64> = (0..3).map(|_| next() % 30).collect();
            for tick in 0..30 {
                if trips.contains(&tick) {
                    let groups = random_groups(&mut next, split_world.centers.len());
                    let (chunks, players) = parts_of(&split_world, &groups);
                    let child = split(&mut split_world, &groups);
                    assert_owned(island(&mut split_world), &child, &chunks, &players);
                    assert!(!child.region.players.is_empty());
                    island(&mut split_world).absorb_island(child, 0);
                    // 对照这一刻也把光照算完（拆合前会算完），两边比的是同一个状态。
                    island(&mut control).region.chunks.finish_light_updates();
                    assert_eq!(
                        print(&mut split_world, true),
                        print(&mut control, true),
                        "seed {seed} tick {tick} groups {groups:?}：拆了马上合回来要逐位相同"
                    );
                }
                step(island(&mut split_world));
                step(island(&mut control));
                assert_eq!(
                    print(&mut split_world, true),
                    print(&mut control, true),
                    "seed {seed} tick {tick}"
                );
                for ((_, mine), (_, theirs)) in split_world.clients.iter().zip(&control.clients) {
                    assert_eq!(
                        mine.take_frames(),
                        theirs.take_frames(),
                        "seed {seed} tick {tick}：玩家 {:?} 收到的字节",
                        mine.id
                    );
                }
            }
            assert_alive(&split_world);
        }
    });
}

/// 跑一遍「拆开、两半各自单独跑若干刻、再合回来」，合并时被拆出去的那一半的时钟
/// 比原岛慢 `delta` 刻（拆完就把它平移 −delta），合并时按 `clock` 平移回来。
/// 返回合并那一刻和之后 20 刻每刻的按组指纹。
fn apart_then_merged(seed: u64, delta: i64, clock: ClockParts) -> Vec<Vec<String>> {
    let mut scene = build(seed);
    let mut next = rng(seed ^ 0x3C3C);
    let warm = next() % 10;
    for _ in 0..warm {
        step(island(&mut scene));
    }
    let groups = random_groups(&mut next, scene.centers.len());
    let apart = 5 + next() % 30;
    let mut child = split(&mut scene, &groups);
    child.region.rebase_clock(-delta);
    let rest: BTreeSet<usize> = (0..scene.centers.len())
        .filter(|group| !groups.contains(group))
        .collect();
    for index in 0..apart {
        if index == MID_RUN_STEP {
            let elapsed = (warm + index) as i32;
            mid_run(
                &mut scene.server.world.islands.region,
                &scene.centers,
                &rest,
                elapsed,
            );
            mid_run(&mut child.region, &scene.centers, &groups, elapsed);
        }
        step(island(&mut scene));
        step(&mut child);
    }
    let gap = scene.server.world.islands.region.tick.game_time() - child.region.tick.game_time();
    assert_eq!(gap, delta);
    let parent = island(&mut scene);
    parent.region.chunks.finish_light_updates();
    child.region.chunks.finish_light_updates();
    parent.region.absorb_island_parts(child.region, gap, clock);
    let mut out = vec![print(&mut scene, false)];
    for _ in 0..20 {
        step(island(&mut scene));
        out.push(print(&mut scene, false));
    }
    out
}

/// 同一个种子从没拆过：合并那一刻和之后 20 刻的按组指纹。
fn never_split(seed: u64) -> Vec<Vec<String>> {
    let mut scene = build(seed);
    let mut next = rng(seed ^ 0x3C3C);
    let warm = next() % 10;
    for _ in 0..warm {
        step(island(&mut scene));
    }
    let _ = random_groups(&mut next, scene.centers.len());
    let apart = 5 + next() % 30;
    let all: BTreeSet<usize> = (0..scene.centers.len()).collect();
    for index in 0..apart {
        if index == MID_RUN_STEP {
            let elapsed = (warm + index) as i32;
            mid_run(
                &mut scene.server.world.islands.region,
                &scene.centers,
                &all,
                elapsed,
            );
        }
        step(island(&mut scene));
    }
    scene
        .server
        .world
        .islands
        .region
        .chunks
        .finish_light_updates();
    let mut out = vec![print(&mut scene, false)];
    for _ in 0..20 {
        step(island(&mut scene));
        out.push(print(&mut scene, false));
    }
    out
}

/// 拆出去的两半各自单独跑若干刻再合回来，等于从没拆过一直跑（计划 6.2 第 2 条，
/// 离线）：合并那一刻和之后 20 刻，每组的内容逐刻相同。网络 ID 不比（两半各自取号，
/// 交错的先后和不拆时不同），跨组的先后不比（两组互不影响）。
#[test]
fn islands_run_apart_then_merged_equal_never_splitting() {
    with_stack(|| {
        for seed in 1..=4_u64 {
            let control = never_split(seed);
            let apart = apart_then_merged(seed, 0, ClockParts::ALL);
            for (tick, (left, right)) in apart.iter().zip(&control).enumerate() {
                assert_eq!(left, right, "seed {seed}：合并后第 {tick} 刻");
            }
        }
    });
}

/// 平移和不平移的对照：合并时两边时钟差 d，结果等于两边时钟本来就相同。改坏确认：
/// 漏平移任何一类（岛的时钟、计划刻、红石火把记录、方块实体、容器重查、讲台待发、
/// 玩家传送）都要被抓到——要么合并时的时钟断言、要么之后某一刻的指纹对不上。
#[test]
fn merging_islands_whose_clocks_differ_equals_equal_clocks() {
    with_stack(|| {
        let seed = 2;
        let same = apart_then_merged(seed, 0, ClockParts::ALL);
        for delta in [3, 17] {
            assert_eq!(
                apart_then_merged(seed, delta, ClockParts::ALL),
                same,
                "时钟差 {delta} 刻合并后要和时钟相同时一样"
            );
        }
        type LeaveOut = fn(&mut ClockParts);
        let classes: [(&str, LeaveOut); 7] = [
            ("clock", |parts| parts.clock = false),
            ("block_ticks", |parts| parts.block_ticks = false),
            ("redstone", |parts| parts.redstone = false),
            ("block_entities", |parts| parts.block_entities = false),
            ("containers", |parts| parts.containers = false),
            ("lecterns", |parts| parts.lecterns = false),
            ("teleports", |parts| parts.teleports = false),
        ];
        for (name, leave_out) in classes {
            let mut parts = ClockParts::ALL;
            leave_out(&mut parts);
            let caught = std::panic::catch_unwind(|| apart_then_merged(seed, 17, parts))
                .map_or(true, |broken| broken != same);
            assert!(caught, "漏平移「{name}」没有被抓到");
        }
    });
}

/// 拆分时漏搬任何一类，按位置 / 按玩家的核对都要报出那一类；一类不漏时核对为空。
/// 岛这一层的区块信箱和存盘名单也一样（手动把拆好的那份塞回原岛来模拟漏搬）。
#[test]
fn leaving_any_class_behind_on_a_split_is_reported() {
    with_stack(|| {
        let mut scene = build(3);
        for _ in 0..4 {
            step(island(&mut scene));
        }
        let groups: BTreeSet<usize> = [1].into_iter().collect();
        let (chunks, players) = parts_of(&scene, &groups);
        let inside = *chunks.first().expect("第 1 组有区块");
        let scene_center = scene.centers[1];
        // 平时在 tick 边界上是空的几类，各放一条。
        {
            let island = island(&mut scene);
            island.region.light_changes.insert(
                inside,
                mc_bamboo_world::light::LightChanges { sky: 1, block: 0 },
            );
            island.region.block_changes.changed.push(BlockPos::new(
                inside.x * 16,
                Y,
                inside.z * 16,
            ));
            island.region.dirty.mark(inside);
            island
                .region
                .last_action_millis
                .extend(players.iter().map(|&id| (id, 7)));
            island.save_plan.remark.push(inside);
            island.save_plan.encode.push(inside);
            let revision = island.region.chunks.revision(inside);
            island.save_report.encoded.push(crate::island::Encoded {
                pos: inside,
                revision,
                result: None,
            });
            island.save_report.changes = Some(crate::island::SaveChanges {
                changed: vec![crate::island::ChangedChunk {
                    pos: inside,
                    revision,
                    pristine: false,
                    upper_dirty: true,
                }],
                content: vec![inside],
            });
            island.mailbox.queue_reclaim(inside, 1);

            island
                .region
                .push_block_event_for_test(mc_bamboo_blocks::BlockEvent {
                    pos: BlockPos::new(inside.x * 16 + 1, Y, inside.z * 16 + 1),
                    block: block_id_of_state(blocks::STONE).expect("石头"),
                    param_a: 0,
                    param_b: 0,
                });
        }
        for part in IslandPart::ALL {
            let name = format!("{part:?}");
            let island = island(&mut scene);
            island.region.chunks.finish_light_updates();
            // 区块包缓存拆合时会丢掉被拆走的那部分，每一轮重新编一份。
            let region = &mut island.region;
            crate::chunk_encoding::chunk_frame(
                &mut region.chunk_cache,
                &region.chunks,
                &region.template_body,
                scene_center,
                &mut region.profiler,
            )
            .expect("中心区块已加载");
            let origin = SectionPos::of(inside);
            let random = island.region.derive_island_random(0, origin);
            let child = island
                .region
                .split_off_island_parts(&chunks, &players, random, Some(part))
                .expect("tick 边界上可以拆");
            let mut child = Island::new(1, child, crate::director_host::MailBudget::default());
            let found: BTreeSet<String> = misplaced(island, &chunks, &players, false)
                .into_iter()
                .chain(misplaced(&child, &chunks, &players, true))
                .filter(|class| class != "Mailbox" && class != "SavePlan" && class != "SaveReport")
                .collect();
            assert!(
                found.contains(&name),
                "漏搬「{name}」没有被报出来（报出的是 {found:?}）"
            );
            child.region.chunks.finish_light_updates();
            island.region.absorb_island(child.region, 0);
        }
        // 一类不漏：核对为空。岛这一层的两类：拆完再手动塞回去，核对要报出来。
        let parent = island(&mut scene);
        let origin = SectionPos::of(inside);
        let random = parent.region.derive_island_random(0, origin);
        let mut child = parent
            .split_off_island(1, &chunks, &players, random)
            .expect("tick 边界上可以拆");
        assert_owned(parent, &child, &chunks, &players);
        parent.mailbox.absorb(std::mem::replace(
            &mut child.mailbox,
            crate::island::ChunkMailbox::new(crate::director_host::MailBudget::default()),
        ));
        assert!(misplaced(parent, &chunks, &players, false).contains("Mailbox"));
        parent
            .save_plan
            .absorb(std::mem::take(&mut child.save_plan));
        assert!(misplaced(parent, &chunks, &players, false).contains("SavePlan"));
        parent
            .save_report
            .absorb(std::mem::take(&mut child.save_report));
        assert!(misplaced(parent, &chunks, &players, false).contains("SaveReport"));
    });
}

/// 微基准（`--release -- --ignored --nocapture`）：1000 个已加载区块（每个区块有改过的
/// 方块、计划刻、容器、漏斗、地上的物品）拆出去再合回来的耗时，另一半 1000 个区块
/// 留在原岛。目标单次 < 5 ms（计划 3.2）。
#[test]
#[ignore]
fn island_split_and_merge_bench() {
    with_stack(|| {
        let mut region = Region::with_keys(2, 20, false, false, crate::secure_chat::empty_keys());
        let side = |x0: i32| -> Vec<ChunkPos> {
            (0..40)
                .flat_map(|x| (0..25).map(move |z| ChunkPos::new(x0 + x, z)))
                .collect()
        };
        let moved = side(0);
        let kept = side(100);
        let desired = mc_bamboo_world::director::Desired::new(31, 31);
        for &pos in moved.iter().chain(&kept) {
            region
                .deliver_column(pos, Box::new(crate::region::RegionColumn::from_template()))
                .unwrap_or_else(|_| unreachable!("刚建好的岛可以交付"));
        }
        region
            .tick_levels
            .deliver(moved.iter().chain(&kept).map(|&pos| (pos, desired)));
        let hopper = block_id_of_state(blocks::HOPPER).expect("漏斗");
        for &pos in moved.iter().chain(&kept) {
            let base = BlockPos::new(pos.x * 16, Y, pos.z * 16);
            region.chunks.set_block(base, blocks::STONE);
            region
                .chunks
                .set_block(base.offset(3, 0, 3), blocks::HOPPER);
            entry_in(
                &mut region.containers,
                base.offset(3, 0, 3),
                ContainerKind::Hopper,
            );
            region
                .block_entities
                .insert(Box::new(HopperEntity::new(base.offset(3, 0, 3))));
            entry_in(
                &mut region.containers,
                base.offset(5, 0, 5),
                ContainerKind::Chest,
            )
            .slots[0] = ItemStack::new(items::DIAMOND, 1);
            for delay in 1..=4 {
                region.block_ticks.schedule(
                    base.offset(delay, 1, 0),
                    hopper,
                    delay * 10,
                    TickPriority::Normal,
                );
            }
            region.entities.spawn(Entity::ground_item_with_delta(
                Vec3::new(
                    f64::from(base.x) + 8.5,
                    f64::from(Y),
                    f64::from(base.z) + 8.5,
                ),
                ItemStack::new(items::IRON_INGOT, 1),
                Vec3::default(),
            ));
        }
        region.chunks.finish_light_updates();
        let chunks: BTreeSet<ChunkPos> = moved.iter().copied().collect();
        let players = BTreeSet::new();
        let mut splits = Vec::new();
        let mut merges = Vec::new();
        for round in 0..7 {
            let random = region.derive_island_random(round, SectionPos::new(0, 0));
            let started = std::time::Instant::now();
            let child = region
                .split_off_island(&chunks, &players, random)
                .unwrap_or_else(|_| unreachable!("tick 边界上可以拆"));
            splits.push(started.elapsed());
            assert_eq!(child.chunks.loaded_positions().len(), 1000);
            let started = std::time::Instant::now();
            region.absorb_island(child, 0);
            merges.push(started.elapsed());
            assert_eq!(region.chunks.loaded_positions().len(), 2000);
        }
        splits.sort();
        merges.sort();
        println!(
            "island split/merge of 1000 of 2000 chunks: split median {:.3} ms (min {:.3}, max {:.3}), merge median {:.3} ms (min {:.3}, max {:.3})",
            splits[3].as_secs_f64() * 1000.0,
            splits[0].as_secs_f64() * 1000.0,
            splits[6].as_secs_f64() * 1000.0,
            merges[3].as_secs_f64() * 1000.0,
            merges[0].as_secs_f64() * 1000.0,
            merges[6].as_secs_f64() * 1000.0,
        );
    });
}
