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
        // 一个半径 2 的方形加上传播两圈，任何一个玩家位置涉及的记录是常数个；
        // 每刻最多产生一条带的交付/收回，所以信箱长度不随走过的距离增长。
        assert!(mail.0 < 1000, "交付信箱不会无限增长：{mail:?}");
        assert!(mail.1 < 1000, "收回收信箱不会无限增长：{mail:?}");
    }

    // 停下来之后身后的区块要被收回：票撤了，逗留 1 刻，收回上限 12 个/刻。
    // 视距 2 的票覆盖半径 2，玩家已经走出 15 个区块，出生点的票早就没了。
    // 收回有 12 个/刻的上限，未必立刻轮到出生点，所以静置阶段要跑够刻数。
    // 出生点这一侧要等票的等级传播出 11 圈（约 14 刻）才彻底没票；
    // 之后逗留 1 刻、按 12 个/刻收回。
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
        let (delivered, reclaimed, mail) = server.director_mail();
        assert!(
            delivered <= 16 + 9 * server.world_region().players.len(),
            "第 {step} 刻交付 {delivered} 个超上限"
        );
        assert!(reclaimed <= 16, "第 {step} 刻收回 {reclaimed} 个超上限");
        assert!(mail.0 < 10_000 && mail.1 < 10_000, "信箱有界：{mail:?}");
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
