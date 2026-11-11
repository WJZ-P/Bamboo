//! 已加载模式下的世界：进服、走动、走远再走回来内容还在、视距变化、离开、关服。
//!
//! 这里不用 Director（提交 5 才接进 `World::tick`），测试自己扮演 Director：
//! 用 `ChunkStore::load` 交付盒子、用 `Region::take_columns` + [`Parking`] 收回并停放、
//! 再用 `Parking::take` 装回去。这样提交 3 能单独验证「一局之内不丢内容」。

use std::collections::BTreeSet;

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
use mc_bamboo_data::blocks;
use mc_bamboo_world::{column::ChunkColumn, director::ChunkBundle};

use crate::{
    region::{Region, parking::Parking},
    world::tests::Harness,
};

const ORIGIN: ChunkPos = ChunkPos::new(0, 0);
const FAR: ChunkPos = ChunkPos::new(4, 0);
const EDIT: BlockPos = BlockPos::new(2, -60, 2);

/// 把一个区块加载进世界（相当于 Director 的 `Deliver`）。
/// 停放表里没有就现造一个模板盒子；有的话先把世界层装进去，再还原上层盒子。
fn load(region: &mut Region, _parking: &Parking, pos: ChunkPos) {
    region
        .chunks
        .load(pos, Box::new(ChunkColumn::from_template()));
}

/// 把一个区块从世界收回并停放（相当于 Director 的 `Reclaim` + `Park`）。
fn unload(region: &mut Region, parking: &mut Parking, pos: ChunkPos) {
    assert!(region.chunks.is_loaded(pos));
    region.chunks.finish_light_updates();
    let box_ = region
        .take_columns(&[pos].into_iter().collect())
        .expect("前提满足")
        .remove(&pos)
        .expect("区块盒子在");
    let _ = region.chunks.unload(pos).expect("光照已算完");
    if !box_.is_pristine() {
        parking.park(pos, box_);
    }
}

/// 从停放表还原整个上层盒子（相当于 Director 重新交付）。
fn restore(region: &mut Region, parking: &mut Parking, pos: ChunkPos) {
    let box_ = match parking.take(pos) {
        Some(box_) => box_,
        None => return,
    };
    region
        .chunks
        .load(pos, Box::new(ChunkColumn::from_template()));
    region.insert_column(pos, box_).expect("前提满足");
}

#[test]
fn a_loaded_world_starts_empty_and_delivers_chunks() {
    let mut server = Harness::new_loaded(2);
    assert!(server.region().chunks.is_loaded_mode());
    assert_eq!(server.region().chunks.loaded_count(), Some(0));
    assert_eq!(server.region().chunks.block(EDIT), None);

    let parking = Parking::default();
    load(server.region(), &parking, ORIGIN);
    assert_eq!(server.region().chunks.loaded_count(), Some(1));
    assert_eq!(server.region().chunks.block(EDIT), Some(blocks::AIR));
    assert_eq!(
        server.region().chunks.light_value(LightLayer::Sky, EDIT),
        15
    );
}

#[test]
fn walking_away_and_back_keeps_block_and_light_content() {
    let mut server = Harness::new_loaded(2);
    let mut parking = Parking::default();
    load(server.region(), &parking, ORIGIN);
    load(server.region(), &parking, FAR);

    // 玩家在出生点改了一格。
    assert!(server.region().chunks.set_block(EDIT, blocks::STONE));
    server.region().chunks.finish_light_updates();
    assert_eq!(server.region().chunks.block(EDIT), Some(blocks::STONE));

    // 走远：收回出生点区块。
    unload(server.region(), &mut parking, ORIGIN);
    assert_eq!(server.region().chunks.block(EDIT), None);
    assert_eq!(parking.len(), 1, "改过的盒子要停放");

    // 走回来：从停放表装回去。
    load(server.region(), &parking, ORIGIN);
    restore(server.region(), &mut parking, ORIGIN);
    assert_eq!(server.region().chunks.block(EDIT), Some(blocks::STONE));
    assert_eq!(server.region().chunks.loaded_count(), Some(2));
}

#[test]
fn a_pristine_column_is_dropped_not_parked() {
    let mut server = Harness::new_loaded(2);
    let mut parking = Parking::default();
    load(server.region(), &parking, ORIGIN);
    unload(server.region(), &mut parking, ORIGIN);
    assert!(parking.is_empty(), "没改过的盒子不用停放");
    load(server.region(), &parking, ORIGIN);
    assert_eq!(server.region().chunks.block(EDIT), Some(blocks::AIR));
}

#[test]
fn view_distance_change_and_leave_keep_the_loaded_set_bounded() {
    let mut server = Harness::new_loaded(2);
    let mut parking = Parking::default();
    let area: Vec<ChunkPos> = (-2..=2)
        .flat_map(|x| (-2..=2).map(move |z| ChunkPos::new(x, z)))
        .collect();
    for &pos in &area {
        load(server.region(), &parking, pos);
    }
    assert_eq!(server.region().chunks.loaded_count(), Some(area.len()));

    // 模拟视距变小：只保留中心一圈。
    let keep: BTreeSet<ChunkPos> = area
        .iter()
        .copied()
        .filter(|pos| pos.x.abs() <= 1 && pos.z.abs() <= 1)
        .collect();
    for &pos in &area {
        if !keep.contains(&pos) {
            unload(server.region(), &mut parking, pos);
        }
    }
    assert_eq!(server.region().chunks.loaded_count(), Some(keep.len()));

    // 离开/关服：全部收回，停放表里只有改过的盒子（这里一个都没改过）。
    let remaining: Vec<ChunkPos> = area
        .iter()
        .copied()
        .filter(|pos| server.region().chunks.is_loaded(*pos))
        .collect();
    for pos in remaining {
        unload(server.region(), &mut parking, pos);
    }
    assert_eq!(server.region().chunks.loaded_count(), Some(0));
    assert!(parking.is_empty(), "没改过的区块都不该占用停放表");
    assert_eq!(parking.take_all().len(), 0);
}

/// 已加载模式下，每名玩家视野里的区块必须都已经交付（主代理条件 1）。
/// 走动、传送、改视距之后各检查一次。
#[test]
fn the_loaded_set_covers_every_players_view() {
    use mc_bamboo_core::Vec3;

    let mut server = Harness::new_loaded(2);
    let client = server.join_and_settle("Kitten");

    let check = |server: &Harness| {
        let region = &server.world_region();
        for player in region.players.values() {
            let center = player.chunk_pos();
            let view = player.view_distance(region.view_distance);
            let radius = view + 1;
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let pos = ChunkPos::new(center.x + dx, center.z + dz);
                    // `ChunkView::contains` 是圆角的：沿轴半径 vd+1，四角收圆。
                    let ax = (dx.abs() - 2).max(0) as i64;
                    let az = (dz.abs() - 2).max(0) as i64;
                    if ax * ax + az * az >= (view as i64) * (view as i64) {
                        continue;
                    }
                    assert!(
                        region.chunks.is_loaded(pos),
                        "视野里的 {pos:?} 还没交付（中心 {center:?}）"
                    );
                }
            }
        }
    };
    check(&server);

    // 走动（长距离直接放置，模拟传送）。
    server.move_to_position(&client, Vec3::new(100.5, -60.0, 100.5));
    server.tick();
    check(&server);

    // 改视距。
    server.apply_ticket_for_test(mc_bamboo_world::director::TicketOp::ViewDistance(4));
    server.tick();
    check(&server);
}

// ---- 按等级决定谁 tick ----

/// 随机种子驱动的 xorshift64*，不用外部随机依赖。
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, limit: u64) -> u64 {
        (self.next() >> 11) % limit
    }
}

/// 从票表和已加载集合从头按定义算两个集合；测试拿它和 region 的增量集合比。
fn expected_sets(server: &Harness) -> (BTreeSet<ChunkPos>, BTreeSet<ChunkPos>, BTreeSet<ChunkPos>) {
    let region = server.world_region();
    let tickets = server.tickets();
    let mut blocks = BTreeSet::new();
    let mut entities = BTreeSet::new();
    let mut random = BTreeSet::new();
    // 视野级别的范围足够覆盖所有可能进集合的位置；票表之外的等级都是「没票」。
    for player in region.players.values() {
        let center = player.chunk_pos();
        for dx in -40..=40 {
            for dz in -40..=40 {
                let pos = ChunkPos::new(center.x.saturating_add(dx), center.z.saturating_add(dz));
                if tickets.loading_level(pos) > 32 || tickets.simulation_level(pos) > 32 {
                    continue;
                }
                let ring = |radius: i32| {
                    (-radius..=radius).all(|ox| {
                        (-radius..=radius).all(|oz| {
                            region.chunks.is_loaded(ChunkPos::new(
                                pos.x.saturating_add(ox),
                                pos.z.saturating_add(oz),
                            ))
                        })
                    })
                };
                if !ring(1) {
                    continue;
                }
                blocks.insert(pos);
                if tickets.simulation_level(pos) <= 31 {
                    random.insert(pos);
                }
                if tickets.loading_level(pos) <= 31
                    && tickets.simulation_level(pos) <= 31
                    && ring(2)
                {
                    entities.insert(pos);
                }
            }
        }
    }
    (blocks, entities, random)
}

/// 每刻断言：集合里的区块周围一圈/两圈都已交付，且两个集合和从头算的一致。
fn assert_tick_sets(server: &Harness) {
    let region = server.world_region();
    let levels = &region.tick_levels;
    levels.assert_invariants();
    let neighborhood = |pos: ChunkPos, radius: i32| {
        (-radius..=radius).flat_map(move |dx| {
            (-radius..=radius)
                .map(move |dz| ChunkPos::new(pos.x.saturating_add(dx), pos.z.saturating_add(dz)))
        })
    };
    for &pos in levels.block_chunks() {
        for neighbor in neighborhood(pos, 1) {
            assert!(
                region.chunks.is_loaded(neighbor),
                "能跑方块逻辑的 {pos:?} 少了邻居 {neighbor:?}"
            );
        }
    }
    for &pos in levels.entity_chunks() {
        for neighbor in neighborhood(pos, 2) {
            assert!(
                region.chunks.is_loaded(neighbor),
                "能跑实体的 {pos:?} 少了邻居 {neighbor:?}"
            );
        }
    }
    let (blocks, entities, random) = expected_sets(server);
    assert_eq!(levels.block_chunks(), &blocks, "方块集合与定义不一致");
    assert_eq!(levels.entity_chunks(), &entities, "实体集合与定义不一致");
    assert_eq!(
        levels.random_chunks(),
        &random,
        "逐区块 tick 集合与定义不一致"
    );
}

/// 玩家随机走动、传送、改视距/模拟距离、进出，每刻检查两个集合的不变量。
#[test]
fn tick_sets_follow_ticket_levels_through_random_player_actions() {
    let mut server = Harness::new_loaded(4);
    let mut rng = Rng(0x5eed_1234_5678_9abc);
    let mut clients = Vec::new();
    for step in 0..120 {
        match rng.below(10) {
            0 if clients.len() < 3 => clients.push(server.join_and_settle("Wanderer")),
            1 if !clients.is_empty() => {
                let index = rng.below(clients.len() as u64) as usize;
                let client = clients.swap_remove(index);
                server.leave(&client);
            }
            // 短距离走动：一格一格挪。
            2..=4 if !clients.is_empty() => {
                let index = rng.below(clients.len() as u64) as usize;
                let here = server.player(&clients[index]).position();
                let dx = (rng.below(33) as f64) - 16.0;
                let dz = (rng.below(33) as f64) - 16.0;
                server.move_to_position(
                    &clients[index],
                    mc_bamboo_core::Vec3::new(here.x + dx, here.y, here.z + dz),
                );
            }
            // 传送到远处。
            5 if !clients.is_empty() => {
                let index = rng.below(clients.len() as u64) as usize;
                let x = (rng.below(16) as f64) * 160.0 - 1200.0;
                let z = (rng.below(16) as f64) * 160.0 - 1200.0;
                server.move_to_position(&clients[index], mc_bamboo_core::Vec3::new(x, -60.0, z));
            }
            6 => server.apply_ticket_for_test(mc_bamboo_world::director::TicketOp::ViewDistance(
                2 + rng.below(5) as i32,
            )),
            7 => server.apply_ticket_for_test(
                mc_bamboo_world::director::TicketOp::SimulationDistance(rng.below(9) as i32),
            ),
            _ => {}
        }
        server.tick();
        assert_tick_sets(&server);
        // 让收区块的确认也有机会跑，别让出站缓冲一直堆着。
        if step % 4 == 0 {
            for client in &clients {
                server.acknowledge_batch(client, 64.0);
            }
        }
    }
}

/// 只是加载着的最外圈里排一条计划刻：到期不跑；玩家走近、区块进入方块 tick
/// 范围的那一刻按原排序键补跑。用会亮灭的红石灯看效果。
#[test]
fn a_scheduled_tick_waits_in_a_loaded_only_chunk_until_it_can_tick() {
    use mc_bamboo_blocks::{BlockState, TickPriority, state::LIT};

    let mut server = Harness::new_loaded(2);
    let client = server.join_and_settle("Kitten");
    // 视距 2：(4, 0) 只是加载着（等级 33），玩家走到 (2, 0) 后它进方块 tick 范围。
    let pos = BlockPos::new(4 * 16 + 8, -60, 8);
    let chunk = pos.chunk();
    assert!(
        server.world_region().chunks.is_loaded(chunk),
        "边界区块已交付"
    );
    assert!(!server.world_region().tick_levels.chunk_ticks_blocks(chunk));

    let lit = BlockState(blocks::REDSTONE_LAMP).with(LIT, true);
    assert!(server.region().chunks.set_block(pos, lit.0));
    server.region().chunks.finish_light_updates();
    server
        .region()
        .block_ticks
        .schedule(pos, lit.block_id(), 1, TickPriority::Normal);

    for _ in 0..6 {
        server.tick();
        assert_eq!(
            server.world_region().chunks.block(pos),
            Some(lit.0),
            "冻结期间不执行"
        );
    }

    // 走近：这个区块进入方块 tick 范围，搁置的刻在那一刻补跑，灯灭。
    server.move_to_position(
        &client,
        mc_bamboo_core::Vec3::new(2.0 * 16.0 + 8.5, -60.0, 8.5),
    );
    let mut ran_at = None;
    for step in 0..6 {
        server.tick();
        if server.world_region().chunks.block(pos) != Some(lit.0) {
            ran_at = Some(step);
            break;
        }
    }
    let ran_at = ran_at.expect("恢复之后到期的计划刻要执行");
    assert!(server.world_region().tick_levels.chunk_ticks_blocks(chunk));
    assert_eq!(
        server.world_region().chunks.block(pos),
        Some(BlockState(blocks::REDSTONE_LAMP).with(LIT, false).0),
        "计划刻补跑把灯灭了（第 {ran_at} 刻）"
    );
}

/// 边界上的水：在方块 tick 范围最外一格放水源。水会往旁边一圈（只是加载着、
/// 流体刻被搁置）写一格流水的方块状态，但不会从那里继续往外流，也不会把刻丢掉。
#[test]
fn boundary_water_does_not_keep_flowing_through_a_frozen_ring() {
    use mc_bamboo_blocks::TickPriority;
    use mc_bamboo_data::registries::fluid;

    let mut server = Harness::new_loaded(2);
    let _client = server.join_and_settle("Kitten");
    let region = server.region();
    // (3, 0) 能跑方块逻辑，(4, 0) 只是加载着：水从 3 号区块的东边缘流过去。
    let source = BlockPos::new(3 * 16 + 15, -60, 8);
    let first = source.east();
    let second = first.east();
    assert!(region.tick_levels.chunk_ticks_blocks(source.chunk()));
    assert!(!region.tick_levels.chunk_ticks_blocks(first.chunk()));
    assert!(region.chunks.set_block(source, blocks::WATER));
    region.chunks.finish_light_updates();
    region
        .block_ticks
        .schedule_fluid(source, fluid::WATER, 1, TickPriority::Normal);

    for _ in 0..12 {
        server.tick();
    }
    let region = server.world_region();
    assert_eq!(region.chunks.block(source), Some(blocks::WATER), "源头还在");
    assert_ne!(
        region.chunks.block(first),
        Some(blocks::AIR),
        "水进了旁边一圈"
    );
    assert_eq!(
        region.chunks.block(second),
        Some(blocks::AIR),
        "没继续往没加载的区块流"
    );
    let deferred = region
        .block_ticks
        .pending_fluids()
        .any(|tick| tick.pos.chunk() == first.chunk());
    assert!(deferred, "被冻结那圈里的流体刻还排队等着，没有丢");
}

/// 掉落物所在区块退出实体 tick 范围后不再移动、年龄不增长；回来后继续。
#[test]
fn an_item_freezes_outside_the_entity_ticking_range_and_resumes() {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::item::ItemStack;

    let mut server = Harness::new_loaded(2);
    let client = server.join_and_settle("Kitten");
    // (3, 0) 能跑实体，(4, 0) 不能。物品放在 (4, 0) 里，往天上抛一点好观察。
    let start = Vec3::new(4.0 * 16.0 + 8.5, -50.0, 8.5);
    let chunk = ChunkPos::containing(start.x, start.z);
    let uid = server.spawn_entity(Entity::ground_item_with_delta(
        start,
        ItemStack::new(1, 1),
        Vec3::new(0.0, -0.04, 0.0),
    ));
    let state = |server: &Harness| {
        let region = server.world_region();
        let entity = region.entities.get(uid).expect("物品还在");
        let age = entity.ground().map(|item| item.age).unwrap_or(-1);
        (entity.position, age, entity.tick_count)
    };
    for _ in 0..8 {
        server.tick();
    }
    let frozen = state(&server);
    assert_eq!(frozen.0, start, "不能 tick 实体时不移动");
    assert_eq!(frozen.1, 0, "年龄不增长");
    assert_eq!(frozen.2, 0, "tickCount 也不增长");
    assert!(
        !server
            .world_region()
            .tick_levels
            .chunk_ticks_entities(chunk)
    );

    // 玩家走近，这个区块进入实体 tick 范围：物品开始下落、年龄增长。
    server.move_to_position(&client, Vec3::new(2.0 * 16.0 + 8.5, -60.0, 8.5));
    for _ in 0..4 {
        server.tick();
    }
    let resumed = state(&server);
    assert!(
        server
            .world_region()
            .tick_levels
            .chunk_ticks_entities(chunk)
    );
    assert_ne!(resumed.0, frozen.0, "恢复之后继续移动");
    assert!(resumed.1 > frozen.1, "恢复之后年龄继续增长");
    assert!(resumed.2 > frozen.2);
}

/// 模拟距离小于视距：视野里、模拟距离外的区块确实发给了客户端，但不 tick。
#[test]
fn chunks_beyond_the_simulation_distance_are_sent_but_do_not_tick() {
    use crate::world::tests::{chunk_position, frames_with_id, received_chunks};
    use mc_bamboo_data::packets::play::clientbound;

    let mut server = Harness::new_loaded(5);
    let client = server.join_with("Kitten", crate::world::tests::client_requesting(5));
    server.confirm_teleport(&client);
    server.load_player(&client);
    // 先把模拟距离改小，再收完视野里的区块。
    server.apply_ticket_for_test(mc_bamboo_world::director::TicketOp::SimulationDistance(2));
    let mut sent = Vec::new();
    for _ in 0..30 {
        server.tick();
        let frames = client.take_frames();
        let finished = frames_with_id(&frames, clientbound::CHUNK_BATCH_FINISHED).len();
        sent.extend(received_chunks(&frames));
        for _ in 0..finished {
            server.acknowledge_batch(&client, 64.0);
        }
        if finished == 0 && !sent.is_empty() {
            break;
        }
    }
    let _ = chunk_position;
    let region = server.world_region();
    // 模拟距离 2：方块逻辑只到半径 3、实体只到半径 2；视野半径 5 里
    // 更外面的那些区块全都发给了客户端，但一套 tick 都不跑。
    let outside: Vec<ChunkPos> = sent
        .iter()
        .copied()
        .filter(|&pos| pos.x.abs() > 3 || pos.z.abs() > 3)
        .collect();
    assert!(!outside.is_empty(), "视野里应该有模拟距离外的区块包");
    for &pos in &outside {
        assert!(region.chunks.is_loaded(pos), "{pos:?} 已交付");
        assert!(
            !region.tick_levels.chunk_ticks_blocks(pos),
            "{pos:?} 不该跑方块逻辑"
        );
        assert!(
            !region.tick_levels.chunk_ticks_entities(pos),
            "{pos:?} 不该 tick 实体"
        );
    }
    // 模拟距离外、但仍在方块 tick 半径上的那一圈：计划刻照跑，实体不跑。
    let ring = ChunkPos::new(3, 0);
    assert!(sent.contains(&ring), "这一圈发给了客户端");
    assert!(region.tick_levels.chunk_ticks_blocks(ring));
    assert!(!region.tick_levels.chunk_ticks_entities(ring));
    assert!(region.tick_levels.chunk_ticks_entities(ChunkPos::new(0, 0)));
}

/// 精确驱动和并行驱动跳过同一批实体：被冻结的区块里的物品两种驱动都不动，
/// 能 tick 的区块里的物品两种驱动都动。
#[test]
fn precise_and_parallel_drivers_skip_the_same_entities() {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::item::ItemStack;

    fn snapshot(threads: usize) -> Vec<(Vec3, i32)> {
        let mut server = Harness::new_loaded(2);
        let _client = server.join_and_settle("Kitten");
        server.set_entity_threads(threads);
        for chunk_x in [0, 4] {
            let position = Vec3::new(f64::from(chunk_x) * 16.0 + 8.5, -50.0, 8.5);
            server.spawn_entity(Entity::ground_item_with_delta(
                position,
                ItemStack::new(1, 1),
                Vec3::new(0.0, -0.04, 0.0),
            ));
        }
        for _ in 0..6 {
            server.tick();
        }
        let region = server.world_region();
        assert!(
            region.tick_levels.chunk_ticks_entities(ChunkPos::new(0, 0)),
            "中心区块该 tick 实体"
        );
        assert!(
            !region.tick_levels.chunk_ticks_entities(ChunkPos::new(4, 0)),
            "最外圈不该 tick 实体"
        );
        region
            .entities
            .ids()
            .into_iter()
            .map(|uid| {
                let entity = region.entities.get(uid).expect("物品还在");
                (entity.position, entity.ground().map_or(-1, |item| item.age))
            })
            .collect()
    }

    let precise = snapshot(0);
    let parallel = snapshot(2);
    assert_eq!(precise.len(), 2);
    assert_eq!(precise[0], parallel[0], "能 tick 的物品两种驱动结果一样");
    assert_eq!(precise[1], parallel[1], "被跳过的物品两种驱动结果一样");
    assert_ne!(precise[0].0.y, precise[1].0.y, "一个动了、一个没动");
    assert_ne!(precise[0].1, precise[1].1, "一个年龄增长、一个没增长");
}

/// 撤票之后、100 刻收回延迟之内，区块还在 region 里但等级已经跌到 34 以上：
/// 它必须立刻停止 tick（Director 的 `LevelChanged` 要覆盖 Unload/Demote 这类跨档）。
#[test]
fn a_chunk_waiting_for_reclaim_stops_ticking_as_soon_as_its_ticket_goes() {
    let mut server = Harness::new_loaded(2);
    let client = server.join_and_settle("Kitten");
    let center = ChunkPos::new(0, 0);
    assert!(
        server
            .world_region()
            .tick_levels
            .chunk_ticks_entities(center)
    );

    server.leave(&client);
    server.tick();
    let region = server.world_region();
    assert!(
        region.chunks.is_loaded(center),
        "收回延迟内区块还在 region 里"
    );
    assert!(
        !region.tick_levels.chunk_ticks_blocks(center),
        "没票的逗留区块不该跑方块逻辑"
    );
    assert!(
        !region.tick_levels.chunk_ticks_entities(center),
        "没票的逗留区块不该 tick 实体"
    );
    assert!(
        region.tick_levels.random_chunks().is_empty(),
        "没票之后没有逐区块 tick"
    );
    assert_tick_sets(&server);
}
