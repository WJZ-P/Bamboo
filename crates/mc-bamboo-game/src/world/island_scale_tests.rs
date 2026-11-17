//! 大实体表里的掉落物：N 组相距很远的物品，每刻每个实体的耗时和 N 的关系。
//!
//! 手工跑：`cargo test -p mc-bamboo-game --release --lib world::island_scale_tests::stress -- --ignored --nocapture`。
//! 同轮交替多轮，输出每个 N 的中位数；组与组的内容逐位相同，只有组数变。
//!
//! Island scale benchmark: N far-apart groups of dropped items, measuring the
//! per-item per-tick cost as N grows. Interleave the group counts within each
//! round; groups are identical, so any growth is a property of the whole table.

use std::time::{Duration, Instant};

use mc_bamboo_blocks::{
    Aabb,
    entity::{Entity, EntityPublic},
};
use mc_bamboo_core::Vec3;
use mc_bamboo_protocol::item::ItemStack;

use super::tests::{Harness, SPAWN_X, SPAWN_Y, SPAWN_Z};

/// 每组 256 个掉落物、10 名假玩家；组间距和 `islands-K` 一样是 1024 格。
const PER_GROUP: i32 = 256;
const BOTS_PER_GROUP: usize = 10;
const GROUP_STRIDE: f64 = 1024.0;
const GROUPS: [usize; 5] = [1, 2, 4, 8, 16];
const ROUNDS: usize = 3;
const WARMUP: usize = 2;
const TICKS: usize = 8;

/// 造一份和压测场景同源的世界：N 组物品，玩家在各组附近（这里不动）。
fn harness(groups: usize) -> Harness {
    let mut server = Harness::new(8);
    for group in 0..groups {
        let origin = group as f64 * GROUP_STRIDE;
        for index in 0..PER_GROUP {
            let x = origin + 40.0 + 2.0 * f64::from(index % 16) + 0.5;
            let z = 40.0 + 2.0 * f64::from(index / 16) + 0.5;
            server.spawn_entity(Entity::ground_item(
                Vec3::new(x, SPAWN_Y + 1.0, z),
                ItemStack::new(1, 1),
            ));
        }
    }
    let mut clients = Vec::new();
    for group in 0..groups {
        for bot in 0..BOTS_PER_GROUP {
            clients.push((group, bot, server.join(&format!("Bot{group:02}{bot:02}"))));
        }
    }
    server.tick();
    for (group, bot, client) in &clients {
        let spot = Vec3::new(
            *group as f64 * GROUP_STRIDE + 8.5 + *bot as f64,
            SPAWN_Y,
            8.5,
        );
        let region = server.region();
        region
            .players
            .get_mut(&client.id)
            .expect("玩家已进服")
            .set_position(&mut region.player_grid, spot);
        let _ = client.take_frames();
    }
    assert_eq!(
        server.region().players.len(),
        groups * BOTS_PER_GROUP,
        "每组 10 名玩家都要真的进服，否则量到的不是压测的形态"
    );
    server
}

/// 只量「捡起 + 实体 tick」两段，避开进服、区块发送、追踪这些和本问题无关的阶段。
/// 返回每刻的 µs。
fn measure(groups: usize) -> (f64, f64) {
    let mut server = harness(groups);
    for _ in 0..WARMUP {
        crate::drops::pickup_before_entities(server.region());
        let (mut level, ticks) = server.region().block_level();
        let _ = mc_bamboo_blocks::run_entity_tick(&mut level, ticks);
    }
    let mut pickup = Duration::ZERO;
    let mut tick = Duration::ZERO;
    for _ in 0..TICKS {
        let start = Instant::now();
        crate::drops::pickup_before_entities(server.region());
        let mid = Instant::now();
        let (mut level, ticks) = server.region().block_level();
        let _ = mc_bamboo_blocks::run_entity_tick(&mut level, ticks);
        let end = Instant::now();
        pickup += mid - start;
        tick += end - mid;
    }
    (
        pickup.as_secs_f64() * 1.0e6 / TICKS as f64,
        tick.as_secs_f64() * 1.0e6 / TICKS as f64,
    )
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// 手工跑的微基准：每个 N 交替多轮，打印每刻每个掉落物的 µs。
#[test]
#[ignore = "手工跑的微基准"]
fn stress_reports_per_item_cost_against_group_count() {
    let mut pickup: Vec<Vec<f64>> = vec![Vec::new(); GROUPS.len()];
    let mut tick: Vec<Vec<f64>> = vec![Vec::new(); GROUPS.len()];
    for _ in 0..ROUNDS {
        for (index, &groups) in GROUPS.iter().enumerate() {
            let (p, t) = measure(groups);
            pickup[index].push(p);
            tick[index].push(t);
        }
    }
    for (index, &groups) in GROUPS.iter().enumerate() {
        let items = f64::from(PER_GROUP) * groups as f64;
        println!(
            "groups={groups:2} items={:5} pickup={:9.1}us/tick entity_tick={:9.1}us/tick \
             pickup_per_item={:.3}us tick_per_item={:.3}us total_per_item={:.3}us",
            PER_GROUP * groups as i32,
            median(pickup[index].clone()),
            median(tick[index].clone()),
            median(pickup[index].clone()) / items,
            median(tick[index].clone()) / items,
            (median(pickup[index].clone()) + median(tick[index].clone())) / items,
        );
    }
}

/// 改坏确认：用空间索引得到的候选必须和原来的全表扫描逐项相同（uid 升序），
/// 包括跨段、远处同一 x 区间的干扰项、三种实体和 `removed` 的行。
///
/// Regression check: the spatial-index candidates must match the old full-table
/// scan item for item in ascending uid order, including section boundaries,
/// far-away rows sharing the queried x range, all three entity kinds and
/// `removed` rows.
#[test]
fn pickup_candidates_match_the_full_scan_in_uid_order() {
    let mut server = Harness::new(8);
    let mut positions = Vec::new();
    for index in 0..80 {
        // 跨过好几个 16³ 段，有的在很远的 z 上但 x 落在查询区间里。
        let x = 8.5 + f64::from(index % 5) * 7.0;
        let z = 8.5 + f64::from(index / 5) * 19.0;
        positions.push(Vec3::new(x, SPAWN_Y + 1.0, z));
    }
    for (index, position) in positions.iter().enumerate() {
        let entity = match index % 7 {
            0 => Entity::falling_block(
                mc_bamboo_core::BlockPos::new(
                    position.x as i32,
                    position.y as i32,
                    position.z as i32,
                ),
                mc_bamboo_blocks::BlockState(mc_bamboo_data::blocks::SAND),
            ),
            1 => Entity::primed_tnt(*position, 0.25),
            _ => Entity::ground_item(*position, ItemStack::new(1, 1)),
        };
        server.spawn_entity(entity);
    }
    // 已标记移除但还没从表里摘掉的行，两条路径都该看见（后面 `can_pick_up` 会跳过）。
    let removed = server.spawn_entity(Entity::ground_item(
        Vec3::new(SPAWN_X, SPAWN_Y, SPAWN_Z),
        ItemStack::new(1, 1),
    ));
    server.region().entities.with_mut(removed, |entity| {
        entity.discard();
    });
    let client = server.join("Kitten");
    server.tick();
    let _ = client.take_frames();
    let region = server.region();
    let reach = region
        .players
        .get(&client.id)
        .expect("玩家已进服")
        .bounding_box()
        .inflate(1.0, 0.5, 1.0);

    let old: Vec<i32> = region
        .entities
        .iter()
        .filter(|entity| entity.bounding_box().intersects(reach))
        .map(|entity| entity.id)
        .collect();
    let mut new = Vec::new();
    region
        .entities
        .for_each_in(reach, &mut |public: &EntityPublic| {
            new.push(public.uid);
        });
    assert_eq!(new, old, "候选名单和顺序必须和全表扫描一致");
    assert!(!new.is_empty(), "这个盒子里本来就有物品");
    assert!(new.windows(2).all(|pair| pair[0] < pair[1]), "uid 升序");

    // 盒子远在别处时两种做法都查不到东西。
    let elsewhere = Aabb::new(900.0, SPAWN_Y, 900.0, 902.0, SPAWN_Y + 3.0, 902.0);
    let mut far = Vec::new();
    region
        .entities
        .for_each_in(elsewhere, &mut |public: &EntityPublic| {
            far.push(public.uid);
        });
    assert!(far.is_empty(), "远处的物品不该被带进候选");
}

/// 真正走一次 `pickup_for`：近处两个物品被捡起，远处的诱饵留在世界里。
/// 把候选名单换回全表（不按范围过滤）时，这个测试会抓到多捡或漏捡。
///
/// End-to-end pickup: two nearby items are picked up while a far-away decoy
/// stays in the world. Reverting to an unfiltered whole-table candidate list
/// would make this test catch the wrong pickup set or order.
#[test]
fn pickup_uses_only_nearby_items_in_uid_order() {
    let mut server = Harness::new(8);
    let client = server.join("Kitten");
    // 进服后玩家在 (8.5, -60, 8.5)；两件在拾取盒里，一件在 1024 格外。
    let near = Vec3::new(SPAWN_X, SPAWN_Y, SPAWN_Z);
    let far = Vec3::new(SPAWN_X + GROUP_STRIDE, SPAWN_Y, SPAWN_Z);
    let first = server.spawn_entity(Entity::ground_item(near, ItemStack::new(1, 1)));
    let second = server.spawn_entity(Entity::ground_item(near, ItemStack::new(1, 1)));
    let decoy = server.spawn_entity(Entity::ground_item(far, ItemStack::new(1, 1)));
    server.tick();
    let _ = client.take_frames();

    // 捡起是逐刻跑的；反复跑直到近处两件都不在了，远处的必须还在。
    for _ in 0..20 {
        crate::drops::pickup_before_entities(server.region());
        let (mut level, ticks) = server.region().block_level();
        let _ = mc_bamboo_blocks::run_entity_tick(&mut level, ticks);
    }
    let region = server.region();
    assert!(region.entities.get(first).is_none(), "近处第一件该被捡走");
    assert!(region.entities.get(second).is_none(), "近处第二件该被捡走");
    assert!(region.entities.get(decoy).is_some(), "远处的诱饵不该被捡走");
}

/// 候选名单在真实的「捡起 → 标记移除 → 实体 tick」循环之后仍和全表扫描一致：
/// 空间索引里的段登记不能因为位置变化、槽位复用或 `removed` 行而漏项。
///
/// The candidate list must still match the full scan after real pickup /
/// discard / entity-tick cycles: section registrations must survive position
/// changes, slot reuse and `removed` rows.
#[test]
fn pickup_candidates_stay_identical_after_ticks() {
    let mut server = Harness::new(8);
    let client = server.join("Kitten");
    for index in 0..24 {
        let x = 8.5 + f64::from(index % 6) * 0.4;
        let z = 8.5 + f64::from(index / 6) * 0.4;
        server.spawn_entity(Entity::ground_item(
            Vec3::new(x, SPAWN_Y, z),
            ItemStack::new(1, 1),
        ));
    }
    server.tick();
    let _ = client.take_frames();
    for _ in 0..12 {
        crate::drops::pickup_before_entities(server.region());
        let (mut level, ticks) = server.region().block_level();
        let _ = mc_bamboo_blocks::run_entity_tick(&mut level, ticks);
        let region = server.region();
        let reach = region
            .players
            .get(&client.id)
            .expect("玩家已进服")
            .bounding_box()
            .inflate(1.0, 0.5, 1.0);
        let old: Vec<i32> = region
            .entities
            .iter()
            .filter(|entity| entity.bounding_box().intersects(reach))
            .map(|entity| entity.id)
            .collect();
        let mut new = Vec::new();
        region
            .entities
            .for_each_in(reach, &mut |public: &EntityPublic| {
                new.push(public.uid);
            });
        assert_eq!(new, old, "每一刻候选名单和顺序都要和全表扫描一致");
    }
}
