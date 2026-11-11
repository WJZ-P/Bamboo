//! 已加载模式下的世界：一部分测试自己扮演 Director（手工 `ChunkStore::load` /
//! `take_columns` + [`Parking`]），验证一局之内不丢内容；另一部分走真正的
//! [`Harness::tick`] 和 Director 信箱，验证交付限速、近的先交付、脚下 3×3 豁免
//! 和「交付了才发」。

use std::collections::BTreeSet;

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, Vec3};
use mc_bamboo_data::{blocks, packets::play::clientbound};
use mc_bamboo_world::{column::ChunkColumn, director::ChunkBundle};

use crate::{
    region::{Region, parking::Parking},
    world::tests::{Harness, SPAWN_X, SPAWN_Z},
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

/// 过渡状态的旧断言「整个视野当刻已交付」不再成立：交付每刻有上限，
/// 站着不动、传送、改视距之后都只保证脚下 3×3 当刻到位；整个视野和
/// 客户端持有的集合要过若干刻才收敛。这两个不变量分别由
/// [`assert_footprints_loaded`] 和 `tracking_set` 的检查覆盖。
#[test]
fn the_loaded_set_eventually_covers_every_players_view() {
    let mut server = Harness::new_loaded(2);
    let client = server.join("Kitten");
    let mut held = BTreeSet::new();
    run_ticks(&mut server, &client, 12, 64, &mut held);
    assert_eq!(held, tracking_set(&server, &client), "进服后收齐视野");
    for pos in tracking_set(&server, &client) {
        assert!(
            server.world_region().chunks.is_loaded(pos),
            "收齐之后视野里的 {pos:?} 已交付"
        );
    }

    // 走动（长距离直接放置，模拟传送）：脚下 3×3 立刻到位，视野再陆续收齐。
    server.move_to_position(&client, Vec3::new(100.5, -60.0, 100.5));
    server.acknowledge_batch(&client, 64.0);
    server.tick();
    assert_footprints_loaded(&server);
    let frames = client.take_frames();
    for frame in &frames {
        track_client_chunks(&mut held, frame);
    }
    for _ in super::tests::frames_with_id(&frames, clientbound::CHUNK_BATCH_FINISHED) {
        server.acknowledge_batch(&client, 64.0);
    }
    run_ticks(&mut server, &client, 20, 64, &mut held);
    assert_eq!(held, tracking_set(&server, &client), "走动后收齐视野");

    // 改视距（走 Director 的票入口），也是若干刻后收齐。
    server.apply_ticket_for_test(mc_bamboo_world::director::TicketOp::ViewDistance(4));
    run_ticks(&mut server, &client, 20, 64, &mut held);
    assert_eq!(held, tracking_set(&server, &client), "改视距后收齐视野");
    assert_footprints_loaded(&server);
}

/// 已加载模式的公共检查：每名玩家脚下 3×3 每一刻都必须已交付。
/// 这是设计稿第 10 节的下限（玩家不能站在没加载的地方），也是限速之后
/// 唯一还「当刻成立」的加载不变量；整个视野改成「若干刻后收齐」。
fn assert_footprints_loaded(server: &Harness) {
    let region = server.world_region();
    for player in region.players.values() {
        let center = player.chunk_pos();
        for dx in -1..=1 {
            for dz in -1..=1 {
                let pos = ChunkPos::new(center.x + dx, center.z + dz);
                assert!(
                    region.chunks.is_loaded(pos),
                    "玩家脚下 {pos:?} 还没交付（中心 {center:?}）"
                );
            }
        }
    }
}

/// 一名玩家此刻的跟踪范围 `T(vd)`，和 `chunk_tracking` 用的是同一个定义。
fn tracking_set(server: &Harness, client: &super::tests::Client) -> BTreeSet<ChunkPos> {
    let region = server.world_region();
    let player = server.player(client);
    let center = player.chunk_pos();
    let vd = player.view_distance(region.view_distance);
    let radius = vd + 1;
    (center.x - radius..=center.x + radius)
        .flat_map(|x| (center.z - radius..=center.z + radius).map(move |z| ChunkPos::new(x, z)))
        .filter(|&pos| {
            let dx = (i64::from(pos.x - center.x).abs() - 2).max(0);
            let dz = (i64::from(pos.z - center.z).abs() - 2).max(0);
            dx * dx + dz * dz < i64::from(vd) * i64::from(vd)
        })
        .collect()
}

/// 跑若干刻，每刻回确认、维护客户端持有的区块集合、检查不变量。
/// 返回每刻（交付数、收回数、信箱长度）以及最终的持有集合。
fn run_ticks(
    server: &mut Harness,
    client: &super::tests::Client,
    ticks: usize,
    budget: usize,
    held: &mut BTreeSet<ChunkPos>,
) -> Vec<(usize, usize, (usize, usize))> {
    let mut samples = Vec::new();
    for _ in 0..ticks {
        server.acknowledge_batch(client, 64.0);
        server.tick();
        assert_footprints_loaded(server);
        assert_every_loaded_chunk_has_an_owner(server);
        let (delivered, reclaimed, mail) = server.director_mail();
        assert!(
            delivered <= budget + 9 * server.world_region().players.len(),
            "这一刻交付 {delivered} 个，超过上限 + 3×3 豁免"
        );
        assert!(reclaimed <= budget, "这一刻收回 {reclaimed} 个，超过上限");
        samples.push((delivered, reclaimed, mail));
        let frames = client.take_frames();
        for frame in &frames {
            track_client_chunks(held, frame);
        }
        for _ in super::tests::frames_with_id(&frames, clientbound::CHUNK_BATCH_FINISHED) {
            server.acknowledge_batch(client, 64.0);
        }
    }
    samples
}

/// 按客户端收到的区块包/forget 包维护「客户端持有的区块集合」，和官方客户端一样。
fn track_client_chunks(held: &mut BTreeSet<ChunkPos>, frame: &super::tests::Frame) {
    if frame.id == clientbound::LEVEL_CHUNK_WITH_LIGHT {
        held.insert(super::tests::chunk_position(frame));
    } else if frame.id == clientbound::FORGET_LEVEL_CHUNK {
        let mut reader = mc_bamboo_protocol::codec::Reader::new(&frame.body);
        let z = reader.i32().unwrap();
        let x = reader.i32().unwrap();
        held.remove(&ChunkPos::new(x, z));
    }
}

/// 进服后若干刻内客户端收齐整个视野：交付限速之后视野不再是「当刻全到」，
/// 但玩家脚下 3×3 当刻就到，远处的按到玩家的距离依次交付、依次发出。
#[test]
fn the_view_arrives_over_several_ticks_nearest_first() {
    let mut server = Harness::new_loaded(2);
    let client = server.join("Kitten");
    let mut held = BTreeSet::new();
    let mut order: Vec<ChunkPos> = Vec::new();
    let mut ticks_to_full = None;
    let mut last_distance = 0i64;
    for tick in 0..40 {
        server.acknowledge_batch(&client, 64.0);
        server.tick();
        let frames = client.take_frames();
        let sent: Vec<ChunkPos> = super::tests::received_chunks(&frames);
        let center = server.player(&client).chunk_pos();
        for pos in &sent {
            let distance = i64::from(pos.x - center.x)
                .abs()
                .max(i64::from(pos.z - center.z).abs());
            assert!(
                distance >= last_distance,
                "第 {tick} 刻先发了远的 {pos:?}（距离 {distance}），之前是 {last_distance}"
            );
            last_distance = distance;
            assert!(server.world_region().chunks.is_loaded(*pos), "交付了才发");
            held.insert(*pos);
        }
        for _ in super::tests::frames_with_id(&frames, clientbound::CHUNK_BATCH_FINISHED) {
            server.acknowledge_batch(&client, 64.0);
        }
        if !super::tests::frames_with_id(&frames, clientbound::FORGET_LEVEL_CHUNK).is_empty() {
            // 本次测试里视图不变，离开的包不该出现。
            unreachable!("视图没变");
        }
        order.extend(sent);
        assert_footprints_loaded(&server);
        if ticks_to_full.is_none() && held == tracking_set(&server, &client) {
            ticks_to_full = Some(tick);
        }
    }
    assert!(!order.is_empty(), "总得发出区块");
    assert_eq!(order[0], ChunkPos::new(0, 0), "第一个发出的是脚下区块");
    assert_eq!(
        held,
        tracking_set(&server, &client),
        "若干刻后客户端收齐整个视野"
    );
    let ticks = ticks_to_full.expect("若干刻内收齐");
    assert!(ticks > 0, "限速之后不该在进服那一刻就收齐");
    assert!(ticks < 10, "视距 2 的视野不该拖上十刻");
}

/// 进服那一刻交付的个数不超过「N + 玩家脚下 3×3 的豁免」，剩下的留到下一刻。
/// 用一个小 N 让「留到下一刻」在几刻内可见。
#[test]
fn deliveries_are_capped_and_the_rest_wait_in_the_mailbox() {
    let mut server = Harness::new_loaded(8);
    server.set_mail_budget(crate::director_host::MailBudget {
        deliveries: 7,
        reclaims: 7,
    });
    let client = server.join("Kitten");
    server.confirm_teleport(&client);
    server.load_player(&client);
    let mut held = BTreeSet::new();
    let samples = run_ticks(&mut server, &client, 6, 7, &mut held);
    let loaded = server
        .world_region()
        .chunks
        .loaded_count()
        .expect("已加载模式");
    assert!(loaded > 9 + 7, "几刻里陆续交付了更多区块：{loaded}");
    assert!(samples[0].2.0 > 0, "第一刻信箱里还欠着盒子，不能一刻全交完");
    assert!(
        samples.iter().all(|&(delivered, _, _)| delivered <= 7 + 9),
        "每刻都在上限 + 3×3 豁免之内：{samples:?}"
    );
    assert_footprints_loaded(&server);
    // 一直跑到收齐：上限小，但信箱不会卡住。
    for _ in 0..80 {
        run_ticks(&mut server, &client, 1, 7, &mut held);
        if server.director_mail().2 == (0, 0) {
            break;
        }
    }
    assert_eq!(server.director_mail().2, (0, 0), "信箱最终清空");
    assert_footprints_loaded(&server);
}

/// 传送到很远的地方：落地那一刻脚下 3×3 已加载，视野之后陆续收齐。
#[test]
fn teleport_lands_on_a_loaded_footprint_and_the_view_follows() {
    let mut server = Harness::new_loaded(2);
    server.set_mail_budget(crate::director_host::MailBudget {
        deliveries: 8,
        reclaims: 8,
    });
    let client = server.join("Kitten");
    let mut held = BTreeSet::new();
    run_ticks(&mut server, &client, 12, 8, &mut held);
    assert_eq!(held, tracking_set(&server, &client), "传送前已收齐视野");

    // 长距离直接放置玩家：下一刻开头交付用的是这个新位置。
    let far = Vec3::new(4000.5, -60.0, -3000.5);
    server.move_to_position(&client, far);
    let center = server.player(&client).chunk_pos();
    assert_eq!(center, ChunkPos::new(250, -188));
    // 这一刻就让票变成交付动作，脚下 3×3 必须当刻到位。
    server.acknowledge_batch(&client, 64.0);
    server.tick();
    assert_footprints_loaded(&server);
    let frames = client.take_frames();
    for frame in &frames {
        track_client_chunks(&mut held, frame);
    }
    for _ in super::tests::frames_with_id(&frames, clientbound::CHUNK_BATCH_FINISHED) {
        server.acknowledge_batch(&client, 64.0);
    }
    let (delivered, _, _) = server.director_mail();
    assert!(delivered >= 9, "落地这一刻脚下 3×3 全部交付：{delivered}");
    assert!(
        !held.is_superset(&tracking_set(&server, &client)),
        "视野不该在落地这一刻就全到"
    );

    run_ticks(&mut server, &client, 30, 8, &mut held);
    assert_eq!(held, tracking_set(&server, &client), "视野陆续收齐");
}

/// 玩家每刻跨一个区块，脚下 3×3 始终已加载；身后的区块最终被收回；
/// 信箱长度有界，不会随走过的距离无限增长。
#[test]
fn fast_movement_keeps_the_footprint_loaded_and_the_mailbox_bounded() {
    let mut server = Harness::new_loaded(2);
    server.set_reclaim_delay(1);
    server.set_mail_budget(crate::director_host::MailBudget {
        deliveries: 12,
        reclaims: 12,
    });
    let client = server.join("Kitten");
    let mut held = BTreeSet::new();
    run_ticks(&mut server, &client, 12, 12, &mut held);
    let spawn = server.player(&client).chunk_pos();
    // 每刻往东跨一整个区块（16 格），比正常走路快得多，靠直接放置模拟高速移动。
    for step in 1..=15 {
        let x = f64::from(spawn.x * 16 + step * 16) + 8.5;
        server.move_to_position(&client, Vec3::new(x, -60.0, 8.5));
        run_ticks(&mut server, &client, 1, 12, &mut held);
        let center = server.player(&client).chunk_pos();
        assert_eq!(center.x, spawn.x + step, "每刻确实跨了一个区块");
        assert_footprints_loaded(&server);
        let (_, _, mail) = server.director_mail();
        // 上界：票表每刻只报「这一次移动前后等级变了的位置」，视距 2 的
        // 玩家方形半径 2、传播 11 圈，最多 (2*13+1)^2 = 729 个位置受影响；
        // 每个位置在信箱里至多一条 Deliver / 一条 Reclaim。信箱长度因此
        // 由「单刻能变化的票数」决定，和玩家已经走过的总距离无关。
        const MAX_BACKLOG: usize = 2 * (2 * 13 + 1) * (2 * 13 + 1);
        assert!(mail.0 < MAX_BACKLOG, "交付信箱有界：{mail:?}");
        assert!(mail.1 < MAX_BACKLOG, "收回收信箱有界：{mail:?}");
    }

    // 身后的区块在移动途中就按「远的先收」陆续收回了（票的等级要传播
    // 出 11 圈才彻底没票，移动 15 刻足够）；静置阶段只确认信箱排空。
    let behind = ChunkPos::new(spawn.x - 2, spawn.z);
    // 身后的区块在移动途中就已经按「远的先收」陆续收回，后面只剩
    // 当前视野附近的这一批；停下来之后信箱排空，不会再有多余的收回。
    let mut reclaimed_total = 0;
    for _ in 0..20 {
        server.acknowledge_batch(&client, 64.0);
        server.tick();
        assert_footprints_loaded(&server);
        let (_, reclaimed, _) = server.director_mail();
        reclaimed_total += reclaimed;
        client.take_frames();
    }
    assert!(
        !server.world_region().chunks.is_loaded(behind),
        "身后的区块已经收回（静置阶段又收回 {reclaimed_total} 个）"
    );
    let (_, _, mail) = server.director_mail();
    assert_eq!(mail, (0, 0), "停下来之后信箱清空");
}

/// 确定性随机走动：多名玩家随机走动、传送、进出、改视距，每刻断言脚下 3×3
/// 和每刻的上限，最后停下来若干刻，每名玩家都收齐了自己的视野。
#[test]
fn random_players_keep_the_footprint_loaded_and_converge() {
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
    let mut rng = Rng(0x51ce_9a37_1234_5678);
    let mut server = Harness::new_loaded(4);
    server.set_reclaim_delay(2);
    server.set_mail_budget(crate::director_host::MailBudget {
        deliveries: 16,
        reclaims: 16,
    });
    let mut clients: Vec<(super::tests::Client, BTreeSet<ChunkPos>)> = Vec::new();
    let mut previous_mail = (0usize, 0usize);
    for step in 0..90 {
        // 进出。
        if clients.len() < 3 && rng.below(8) == 0 {
            let name = format!("Player{step}");
            let client = server.join_with(&name, super::tests::client_requesting(2));
            server.confirm_teleport(&client);
            server.load_player(&client);
            clients.push((client, BTreeSet::new()));
        }
        if clients.len() > 1 && rng.below(40) == 0 {
            let (client, _) = clients.pop().expect("至少还有一个");
            server.leave(&client);
        }
        // 走动/传送/改视距。
        for (client, _) in &clients {
            match rng.below(6) {
                0 => {
                    let (x, z) = (rng.below(41) as i32 - 20, rng.below(41) as i32 - 20);
                    server.move_to_position(
                        client,
                        Vec3::new(f64::from(x * 16) + 8.5, -60.0, f64::from(z * 16) + 8.5),
                    );
                }
                1 => {
                    let vd = [2i8, 3, 4][rng.below(3) as usize];
                    server.request(
                        client,
                        mc_bamboo_protocol::packet::play::PlayRequest::ClientInformation(
                            super::tests::client_requesting(vd),
                        ),
                    );
                }
                _ => {}
            }
            server.acknowledge_batch(client, 64.0);
        }
        server.tick();
        assert_footprints_loaded(&server);
        assert_every_loaded_chunk_has_an_owner(&server);
        let (delivered, reclaimed, mail) = server.director_mail();
        assert!(
            delivered <= 16 + 9 * server.world_region().players.len(),
            "第 {step} 刻交付 {delivered} 个超上限"
        );
        // 16 是从 region 取回的上限；信箱里排队、还没进过 region 的盒子
        // 直接还给 Director，不碰 region 也不占这个额度。命中数不超过
        // 本刻或上一刻交付信箱的长度之和（这一刻腾空的也可能来自上一刻）。
        assert!(
            reclaimed <= 16 + mail.0 + previous_mail.0,
            "第 {step} 刻收回 {reclaimed} 个超上限 + 信箱命中：{mail:?} / {previous_mail:?}"
        );
        previous_mail = mail;
        // 最多 3 名玩家、视距 4，单刻票变化的常数上界乘玩家数；和走过的
        // 总距离无关（见 fast_movement 测试里的推导）。
        let players = server.world_region().players.len().max(1);
        const MAX_BACKLOG_PER_PLAYER: usize = (2 * 17 + 1) * (2 * 17 + 1);
        let bound = players * MAX_BACKLOG_PER_PLAYER;
        assert!(
            mail.0 < bound && mail.1 < bound,
            "信箱有界：{mail:?}，上界 {bound}"
        );
        for (client, held) in &mut clients {
            let frames = client.take_frames();
            for frame in &frames {
                track_client_chunks(held, frame);
            }
            for _ in super::tests::frames_with_id(&frames, clientbound::CHUNK_BATCH_FINISHED) {
                server.acknowledge_batch(client, 64.0);
            }
        }
    }
    // 停下来，把配额和信箱都跑空；之后每名玩家都应收齐自己的视野。
    for _ in 0..120 {
        for (client, _) in &clients {
            server.acknowledge_batch(client, 64.0);
        }
        server.tick();
        assert_footprints_loaded(&server);
        assert_every_loaded_chunk_has_an_owner(&server);
        for (client, held) in &mut clients {
            let frames = client.take_frames();
            for frame in &frames {
                track_client_chunks(held, frame);
            }
            for _ in super::tests::frames_with_id(&frames, clientbound::CHUNK_BATCH_FINISHED) {
                server.acknowledge_batch(client, 64.0);
            }
        }
    }
    for (client, held) in &clients {
        assert_eq!(
            *held,
            tracking_set(&server, client),
            "{} 收齐了自己的视野",
            server.player(client).name
        );
    }
    assert!(!clients.is_empty(), "停下来时还有人在");
}

/// 测量用（不是正确性测试）：进服那一刻的总耗时、交付阶段的耗时、交付个数，
/// 以及玩家从进服到收齐整个视野用了多少刻。`--ignored` 是避免它拖慢常规测试。
///
/// 同一轮里把四个预算按正序、逆序交替各跑一次、取中位数：这台机器
/// 不同时间段的 tick 耗时能差出几倍，交错跑才不会把负载漂移算进差异。
/// 每次清零剖析器样本，让 `delivery` 阶段只包含进服这一刻。
#[test]
#[ignore = "手动测量：--ignored --nocapture"]
fn measure_join_tick_and_view_latency() {
    const BUDGETS: [usize; 4] = [0, 16, 64, 256];
    /// 一次测量：(总耗时 us, 交付阶段 us, 发区块阶段 us, 交付个数, 收齐刻数)。
    type Sample = (u64, f64, f64, usize, usize);
    let mut results: Vec<Vec<Sample>> = BUDGETS.iter().map(|_| Vec::new()).collect();
    // 前两轮预热，后五轮取中位数；每轮按正序/逆序交替，抵消一次性开销的方向。
    for round in 0..7 {
        let order: Vec<usize> = if round % 2 == 0 {
            (0..BUDGETS.len()).collect()
        } else {
            (0..BUDGETS.len()).rev().collect()
        };
        for index in order {
            let budget = BUDGETS[index];
            let mut server = Harness::new_loaded(8);
            server.set_mail_budget(crate::director_host::MailBudget {
                deliveries: budget,
                reclaims: budget,
            });
            // 视距 8（压测工具也请求 8）：整个视野是 T(8) = 329 个区块。
            let client = server.join_with("Kitten", super::tests::client_requesting(8));
            client.take_frames();
            server.clear_profile_samples();
            server.tick();
            let samples = server.profile_samples();
            let (_, total_ns, _, _, _) = samples["tick_total"];
            let (_, phase_ns, _, _, _) = samples["delivery"];
            let (_, send_ns, _, _, _) = samples["chunk_sending"];
            let (delivered_count, _, _) = server.director_mail();
            let mut held = BTreeSet::new();
            let frames = client.take_frames();
            for frame in &frames {
                track_client_chunks(&mut held, frame);
            }
            for _ in super::tests::frames_with_id(&frames, clientbound::CHUNK_BATCH_FINISHED) {
                server.acknowledge_batch(&client, 64.0);
            }
            let mut ticks = 1;
            while held != tracking_set(&server, &client) && ticks < 200 {
                server.acknowledge_batch(&client, 64.0);
                server.tick();
                let later = client.take_frames();
                for frame in &later {
                    track_client_chunks(&mut held, frame);
                }
                for _ in super::tests::frames_with_id(&later, clientbound::CHUNK_BATCH_FINISHED) {
                    server.acknowledge_batch(&client, 64.0);
                }
                ticks += 1;
            }
            if round >= 2 {
                results[index].push((
                    (total_ns / 1000) as u64,
                    phase_ns as f64 / 1000.0,
                    send_ns as f64 / 1000.0,
                    delivered_count,
                    ticks,
                ));
            }
        }
    }
    for (&budget, samples) in BUDGETS.iter().zip(&mut results) {
        samples.sort_by_key(|sample| sample.0);
        let (join_us, _, _, delivered, _) = samples[2];
        let mut delivery: Vec<f64> = samples.iter().map(|&(_, d, _, _, _)| d).collect();
        delivery.sort_by(|a, b| a.total_cmp(b));
        let mut sending: Vec<f64> = samples.iter().map(|&(_, _, s, _, _)| s).collect();
        sending.sort_by(|a, b| a.total_cmp(b));
        let mut ticks: Vec<usize> = samples.iter().map(|&(_, _, _, _, t)| t).collect();
        ticks.sort_unstable();
        println!(
            "MEASURE budget={budget} join_tick_us={join_us} delivery_us={:.1} chunk_sending_us={:.1} delivered={delivered} ticks_to_full_view={}",
            delivery[2], sending[2], ticks[2]
        );
    }
}

/// 量「信箱里有东西时 `finish_light_updates()` 强行算完」的独占耗时：
/// 在已交付的世界里改不同数量的方块，再直接调这个函数。
#[test]
#[ignore = "手动测量：--ignored --nocapture"]
fn measure_light_finish_cost() {
    use std::time::Instant;
    for pending_blocks in [0usize, 1, 32, 256] {
        let mut costs = Vec::new();
        for _ in 0..7 {
            let mut server = Harness::new_loaded(8);
            let client = server.join("Kitten");
            for _ in 0..12 {
                server.acknowledge_batch(&client, 64.0);
                server.tick();
                client.take_frames();
            }
            let center = server.player(&client).chunk_pos();
            for i in 0..pending_blocks {
                let x = center.x * 16 + (i % 16) as i32;
                let z = center.z * 16 + (i / 16) as i32;
                server.set_block(
                    BlockPos::new(x, -60, z),
                    if i % 2 == 0 {
                        blocks::STONE
                    } else {
                        blocks::AIR
                    },
                );
            }
            let start = Instant::now();
            server.region().chunks.finish_light_updates();
            costs.push(start.elapsed().as_micros());
        }
        costs.sort_unstable();
        println!(
            "MEASURE light_finish_us={} pending_blocks={pending_blocks}",
            costs[3]
        );
    }
}

/// 玩家进服后马上离开、且每刻只交付 1 个：留在信箱里的盒子必须在
/// 收回请求到来时原样还给 Director，不能晚几刻被插进 region 变成孤儿。
/// 中途每刻都检查「region 里已加载的区块，Director 都有记录且归它管」。
#[test]
fn a_reclaim_takes_a_queued_box_out_of_the_mailbox() {
    let mut server = Harness::new_loaded(2);
    server.set_mail_budget(crate::director_host::MailBudget {
        deliveries: 1,
        reclaims: 1,
    });
    server.set_reclaim_delay(0);
    let client = server.join("Kitten");
    server.tick();
    client.take_frames();
    assert!(
        server.director_mail().2.0 > 0,
        "上限 1 时确实有盒子在信箱里排队"
    );

    server.leave(&client);
    for _ in 0..40 {
        server.tick();
        client.take_frames();
        assert_every_loaded_chunk_has_an_owner(&server);
    }
    assert_eq!(
        server.world_region().chunks.loaded_count(),
        Some(0),
        "region 里没有留下没人管的孤儿"
    );
    assert_eq!(
        server.director_loaded_count(),
        0,
        "Director 的已加载计数回 0"
    );
    assert_eq!(server.director_mail().2, (0, 0), "信箱清空");
    assert_eq!(server.parked_count(), 0, "原样盒子不停放");
}

/// region 里每个已加载的区块，Director 那边都要有一条归它管的记录。
/// 归属可以是「已交付」「交付中」或「收回中」：收回请求先到、盒子还在
/// 信箱里时，Director 已经是 Reclaiming，而 region 里这片还要按上限
/// 逐刻收回，两种状态在这一刻同时存在是正常的。
fn assert_every_loaded_chunk_has_an_owner(server: &Harness) {
    let records = server.director_records();
    for pos in server.loaded_positions() {
        let (owner, _) = records
            .get(&pos)
            .unwrap_or_else(|| panic!("region 里的 {pos:?} 在 Director 那边没有记录"));
        assert!(
            owner.starts_with("Region(") || owner == "Delivering" || owner == "Reclaiming",
            "region 里的 {pos:?} 归属是 {owner}，没人管"
        );
    }
}

/// 改过的盒子在信箱里排队时被收回：它必须原样回到停放表，
/// 再回来时玩家改的方块还在，不能被新生成的模板盒子盖掉。
#[test]
fn a_reclaim_of_a_queued_edited_box_parks_it_and_keeps_its_content() {
    let mut server = Harness::new_loaded(2);
    server.set_mail_budget(crate::director_host::MailBudget {
        deliveries: 1,
        reclaims: 1,
    });
    server.set_reclaim_delay(0);
    let client = server.join("Kitten");
    let mut held = BTreeSet::new();
    run_ticks(&mut server, &client, 80, 1, &mut held);
    assert!(server.world_region().chunks.is_loaded(ORIGIN));
    server.set_block(EDIT, blocks::STONE);
    assert_eq!(
        server.world_region().chunks.block(EDIT),
        Some(blocks::STONE)
    );

    // 走远：出生点的票撤了，盒子被收回、非原样所以进停放表。
    server.move_to_position(&client, Vec3::new(400.5, -60.0, 8.5));
    let mut away = 0;
    while server.parked_count() == 0 && away < 200 {
        server.acknowledge_batch(&client, 64.0);
        server.tick();
        client.take_frames();
        away += 1;
    }
    assert!(server.parked_count() > 0, "改过的盒子被停放了");
    assert!(!server.world_region().chunks.is_loaded(ORIGIN));
    assert_eq!(server.world_region().chunks.block(EDIT), None);

    // 回到离出生点 2 个区块的位置：出生点在视野里（票回来 → 从停放表
    // 重新交付），但不在脚下 3×3 里，所以上限 1 时它还留在信箱里。
    server.move_to_position(&client, Vec3::new(SPAWN_X + 32.0, -60.0, SPAWN_Z));
    server.acknowledge_batch(&client, 64.0);
    server.tick();
    client.take_frames();
    assert!(server.director_mail().2.0 > 0, "还有盒子在交付信箱里排队");

    // 这时离开：收回请求要瞄准信箱里那个非原样的盒子，把它还回去。
    server.leave(&client);
    for _ in 0..200 {
        server.tick();
        client.take_frames();
        assert_every_loaded_chunk_has_an_owner(&server);
    }
    assert_eq!(server.world_region().chunks.loaded_count(), Some(0));
    assert_eq!(server.director_loaded_count(), 0);
    assert_eq!(server.director_mail().2, (0, 0), "信箱清空");
    assert!(server.parked_count() > 0, "改过的盒子最后在停放表里");

    // 再回来：内容还在，不是新生成的模板。
    let back = server.join("Mochi");
    let mut held_back = BTreeSet::new();
    run_ticks(&mut server, &back, 60, 1, &mut held_back);
    assert!(server.world_region().chunks.is_loaded(ORIGIN));
    assert_eq!(
        server.world_region().chunks.block(EDIT),
        Some(blocks::STONE),
        "玩家改的方块跟着盒子回来"
    );
}
