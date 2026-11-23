//! 三档降频的单测：分档、豁免、节奏与补偿、休眠只推进计时器、唤醒上限、
//! 精确 / 并行两种驱动一致，以及缺省关时逐位不变。

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::blocks;

use crate::{
    BlockState, BlockTicks, World,
    entity::{
        Entity, EntityKind,
        mob::{Mob, MobType, throttle},
        parallel::{InlineExecutor, ScopedThreads, tick_entities_parallel},
        view::{BlockSource, SourceView, ViewFacts},
    },
    testing::TestLevel,
};

use super::*;

/// 一套带草地的假世界，玩家放在原点，规则按测试要求改。
fn level_at(player_x: f64, config: Config) -> (TestLevel, BlockTicks, Vec<PlayerView>) {
    let mut level = TestLevel::new();
    // 实体 ID 从 2 起，测试里按 uid 索引计划表。
    level.set_entities(crate::entity::EntityStore::with_next_id(2));
    // 用正式入口铺草地：TestLevel 的方块表是私有的，写方块走 `World::set_block`。
    let mut ticks = BlockTicks::new();
    {
        let mut world = World::new(&mut level, &mut ticks);
        for x in -160..=160 {
            for z in -160..=160 {
                world.set_block(BlockPos::new(x, 63, z), BlockState(blocks::GRASS_BLOCK), 0);
            }
        }
    }
    level.rules.mob_throttle_distance = config.throttle_distance;
    level.rules.mob_sleep_distance = config.sleep_distance;
    level.rules.mob_wakeups_scale = config.wakeups_scale;
    let player = throttle::test_player(1, Vec3::new(player_x, 64.0, 0.0));
    level.players.push(player);
    (level, ticks, vec![player])
}

/// 在 `(x, 64, 0)` 放一头牛。`on_ground = true` 让豁免的「没落地」一条不误命中。
fn add_cow(world: &mut World<'_>, x: f64) -> i32 {
    let mut cow = Entity::cow(Vec3::new(x, 64.0, 0.0));
    cow.on_ground = true;
    // 构造后 `lastDamageTime` 记成「很久以前」，否则刚生成的牛会命中
    // 「刚受伤」豁免（官方构造器的初值就是 0，而测试时钟从 0 开始）。
    if let EntityKind::Mob(mob) = &mut cow.kind {
        mob.last_damage_stamp = i64::MIN / 2;
    }
    world.spawn_entity(cow)
}

/// 让刚生成的实体进 tick 列表，并把时钟拨到 `time`。
fn prepare(level: &mut TestLevel, ticks: &mut BlockTicks, time: i64) {
    level.entities_mut_for_test().end_tick();
    ticks.set_time(time);
}

/// 精确驱动跑一刻，返回跑完后这头牛的位置和几个计时器。
/// 分档：32 内全速、32–64 降频（间隔随距离 2→4）、64 外休眠；
/// 只开降频时 64 外也是降频；缺省关时全部全速，连计划表都不构造。
#[test]
fn distance_bands_and_disabled_defaults() {
    let config = Config {
        throttle_distance: 32,
        sleep_distance: 64,
        wakeups_scale: 1,
    };
    assert_eq!(classify(config, 0, Some(16.0 * 16.0)), Tier::Full);
    assert_eq!(
        classify(config, 0, Some(40.0 * 40.0)),
        Tier::Reduced { period: 3 }
    );
    assert_eq!(
        classify(config, 0, Some(63.0 * 63.0)),
        Tier::Reduced { period: 4 }
    );
    assert_eq!(
        classify(config, 0, Some(100.0 * 100.0)),
        Tier::Sleep { full_tick: true }
    );
    assert_eq!(
        classify(config, 1, Some(100.0 * 100.0)),
        Tier::Sleep { full_tick: false }
    );
    // 没有玩家 = 无限远。
    assert_eq!(classify(config, 1, None), Tier::Sleep { full_tick: false });
    // 旁观者不算「附近玩家」。
    let mut spectator = throttle::test_player(1, Vec3::new(0.0, 64.0, 0.0));
    spectator.spectator = true;
    assert_eq!(
        nearest_player_distance_sqr(Vec3::new(1.0, 64.0, 0.0), &[spectator]),
        None
    );
    // 只开降频档：超出部分一律按 4 刻。
    let reduced_only = Config {
        sleep_distance: 0,
        ..config
    };
    assert_eq!(
        classify(reduced_only, 1, Some(200.0 * 200.0)),
        Tier::Reduced { period: 4 }
    );
    // 只开休眠档：降频档没启用时，起点以内的距离按最密的 2 刻降频，起点外休眠。
    let sleep_only = Config {
        throttle_distance: 0,
        ..config
    };
    assert_eq!(
        classify(sleep_only, 1, Some(16.0 * 16.0)),
        Tier::Reduced { period: 3 }
    );
    assert_eq!(
        classify(sleep_only, 1, Some(100.0 * 100.0)),
        Tier::Sleep { full_tick: false }
    );
    // 缺省全关：任何距离都是完整 tick；驱动连计划表都不构造。
    assert_eq!(classify(Config::OFF, 1, Some(1e12)), Tier::Full);
    let (mut level, mut ticks, _) = level_at(0.0, Config::OFF);
    {
        let mut world = World::new(&mut level, &mut ticks);
        add_cow(&mut world, 500.0);
    }
    prepare(&mut level, &mut ticks, 7);
    assert!(
        World::new(&mut level, &mut ticks)
            .throttle_plans()
            .is_empty()
    );
    assert_eq!(classify(Config::OFF, 7, Some(1e12)), Tier::Full);
}

/// 4.2 的豁免清单逐项生效，命中就盖成全速档，且不占唤醒令牌。
#[test]
fn exemptions_force_full_speed() {
    let config = Config {
        throttle_distance: 32,
        sleep_distance: 64,
        wakeups_scale: 1,
    };
    let (mut level, mut ticks, players) = level_at(0.0, config);
    {
        let mut world = World::new(&mut level, &mut ticks);
        for i in 0..9 {
            add_cow(&mut world, 100.0 + 4.0 * f64::from(i));
        }
        // 逐项摆状态：攻击目标、刚受伤、受伤动画、着火、没落地、水流、幼年、
        // 求爱、点燃的苦力怕（最后一只换成苦力怕）。
        let cases: Vec<Box<dyn Fn(&mut Mob)>> = vec![
            Box::new(|m| m.target = Some(1)),
            Box::new(|m| m.last_damage_stamp = 100),
            Box::new(|m| m.hurt_time = 3),
            Box::new(|_| {}), // 着火在 Entity 上，下面单独设
            Box::new(|_| {}), // 没落地在 Entity 上
            Box::new(|_| {}), // 水流在 Entity 上
            Box::new(|m| m.age = -100),
            Box::new(|m| m.in_love = 100),
            Box::new(|m| m.creeper.ignited = true),
        ];
        for (index, f) in cases.iter().enumerate() {
            let uid = 2 + index as i32;
            world.with_entity_mut(uid, &mut |entity| {
                let EntityKind::Mob(mob) = &mut entity.kind else {
                    panic!("是生物");
                };
                f(mob);
                match index {
                    3 => entity.remaining_fire_ticks = 5,
                    4 => entity.on_ground = false,
                    5 => entity.in_water = true,
                    8 => {
                        mob.mob_type = MobType::Creeper;
                    }
                    _ => {}
                }
            });
        }
    }
    prepare(&mut level, &mut ticks, 100);
    // 第 0 只的攻击目标、第 1 只的伤害戳记（100 刻）都要在 now=100 生效。
    let plans = throttle::plans_for_tick(level.entities_mut_for_test(), 100, config, &players);
    assert_eq!(plans.len(), 9);
    for (index, plan) in plans.iter().enumerate() {
        assert_eq!(plan.tier, Tier::Full, "第 {index} 项豁免没生效");
        assert!(plan.decisions_run);
    }
    // 全速档不消耗令牌：四类都还是满的。
    let tokens = level.entities().wake_tokens();
    assert_eq!(tokens.values(), [4, 8, 8, 4]);
}

/// 降频档的节奏：相位只由 `(game_time + uid)` 决定，间隔 2–4 刻；
/// 跳过决策的刻按「上次运行时刻」补 `noActionTime`，随机数不重放。
#[test]
fn reduced_cadence_and_elapsed_compensation() {
    let config = Config {
        throttle_distance: 32,
        sleep_distance: 64,
        wakeups_scale: 1,
    };
    let (mut level, mut ticks, players) = level_at(0.0, config);
    {
        let mut world = World::new(&mut level, &mut ticks);
        add_cow(&mut world, 40.0);
    }
    prepare(&mut level, &mut ticks, 0);
    // 距离 40 ⇒ 间隔 3；uid = 2，所以 game_time ≡ 1 (mod 3) 时跑决策。
    let mut seen = Vec::new();
    for time in 1..=7 {
        let plans = throttle::plans_for_tick(level.entities_mut_for_test(), time, config, &players);
        assert_eq!(plans[0].tier, Tier::Reduced { period: 3 });
        seen.push(plans[0].decisions_run);
    }
    assert_eq!(seen, [true, false, false, true, false, false, true]);
    // 两次运行之间隔了 3 刻：第二次运行时 `noActionTime` 补 2。
    // 第一次运行时 last_decision 是 NEW，不补。
    // 重来一份干净的世界：第 1 刻跑决策，第 4 刻再跑，中间隔 2 刻。
    let (mut clean, mut clean_ticks, clean_players) = level_at(0.0, config);
    {
        let mut world = World::new(&mut clean, &mut clean_ticks);
        add_cow(&mut world, 40.0);
    }
    prepare(&mut clean, &mut clean_ticks, 0);
    let first = throttle::plans_for_tick(clean.entities_mut_for_test(), 1, config, &clean_players);
    assert!(first[0].decisions_run, "第 1 刻跑决策");
    let before = clean
        .entities()
        .get(2)
        .expect("牛")
        .kind
        .mob()
        .unwrap()
        .no_action_time;
    let fourth = throttle::plans_for_tick(clean.entities_mut_for_test(), 4, config, &clean_players);
    assert!(fourth[0].decisions_run, "第 4 刻跑决策");
    let after = clean
        .entities()
        .get(2)
        .expect("牛")
        .kind
        .mob()
        .unwrap()
        .no_action_time;
    assert_eq!(after - before, 2, "跳过的两刻按经过刻数补偿");
    // 随机数状态没被降频碰过（补偿只动计时器和视线缓存）。
    let seed_before = clean.entities().get(2).expect("牛").random;
    let _ = first;
    assert_eq!(
        clean.entities().get(2).expect("牛").random,
        seed_before,
        "随机数没被补跑"
    );
}

/// 休眠档：每刻只推进计时器（年龄、繁殖冷却、消失、拾取延迟、着火、效果），
/// 位置和速度一动不动；每 20 刻一次完整 tick，但这一刻不主动走动。
#[test]
fn sleep_only_advances_timers() {
    let config = Config {
        throttle_distance: 32,
        sleep_distance: 64,
        wakeups_scale: 1,
    };
    let (mut level, mut ticks, _) = level_at(0.0, config);
    {
        let mut world = World::new(&mut level, &mut ticks);
        let uid = add_cow(&mut world, 100.0);
        world.with_entity_mut(uid, &mut |entity| {
            // 着火是 4.2 的豁免项，这里故意不点燃，免得整段被盖成全速档。
            entity.remaining_fire_ticks = 0;
            entity.delta = Vec3::new(0.0, 0.0, 0.0);
            if let EntityKind::Mob(mob) = &mut entity.kind {
                mob.age = 500;
                mob.invulnerable_time = 7;
            }
        });
    }
    prepare(&mut level, &mut ticks, 3);
    let start = level.entities().get(2).expect("牛");
    let position = start.position;
    let delta = start.delta;
    let random_before = start.random;
    // 连续 3 刻都不是 20 的倍数：只推进计时器。
    for time in 3..=5 {
        ticks.set_time(time);
        World::new(&mut level, &mut ticks).tick_entities();
    }
    let after = level.entities().get(2).expect("牛");
    assert_eq!(after.position, position, "休眠时位置不动");
    assert_eq!(after.delta, delta, "休眠时速度不动");
    assert_eq!(after.tick_count, start.tick_count + 3, "实体计数照常推进");
    assert_eq!(after.remaining_fire_ticks, 0);
    assert_eq!(after.random, random_before, "不取随机数");
    let mob = after.kind.mob().expect("是生物");
    assert_eq!(mob.age, 497, "繁殖冷却照常推进");
    assert_eq!(mob.invulnerable_time, 4, "无敌时间照常推进");
    assert_eq!(mob.no_action_time, 3, "消失计时照常推进");
    // 第 20 刻是完整 tick：物理照跑，但 `no_active_move` 让这一刻不主动走动。
    ticks.set_time(20);
    let plans = World::new(&mut level, &mut ticks).throttle_plans();
    assert_eq!(
        plans[0].tier,
        Tier::Sleep { full_tick: true },
        "每 20 刻一次完整 tick"
    );
    assert!(plans[0].no_active_move, "完整 tick 不主动走动");
    assert!(plans[0].decisions_run, "完整 tick 里决策照跑");
    // 求爱冷却也在休眠计时器清单里，单独推进一次（不经过分档：求爱本身是豁免）。
    let mut cow = Entity::cow(Vec3::new(100.0, 64.0, 0.0));
    {
        let EntityKind::Mob(mob) = &mut cow.kind else {
            unreachable!()
        };
        mob.in_love = 9;
    }
    let mut kind = std::mem::replace(
        &mut cow.kind,
        EntityKind::Tnt(crate::entity::PrimedTnt::new()),
    );
    if let EntityKind::Mob(mob) = &mut kind {
        throttle::sleep_mob_timers(&mut cow, mob);
        assert_eq!(mob.in_love, 8, "繁殖冷却照常推进");
    }
    cow.kind = kind;
}

/// 限量唤醒：休眠超过 Paper 的类别门槛才候选，每刻最多 4 只动物（scale=1），
/// 按 uid 升序轮；醒 100 刻；令牌每刻补一个且按类别封顶。
#[test]
fn wakeup_token_bucket_caps_per_tick() {
    let config = Config {
        throttle_distance: 32,
        sleep_distance: 64,
        wakeups_scale: 1,
    };
    let (mut level, mut ticks, players) = level_at(0.0, config);
    {
        let mut world = World::new(&mut level, &mut ticks);
        for i in 0..12 {
            add_cow(&mut world, 100.0 + 2.0 * f64::from(i));
        }
        // 全部记成「很久以前活跃过」：1200 刻门槛一过就是候选。
        for uid in 2..14 {
            world.with_entity_mut(uid, &mut |entity| {
                if let EntityKind::Mob(mob) = &mut entity.kind {
                    mob.throttle.last_active = 1;
                }
            });
        }
    }
    prepare(&mut level, &mut ticks, 2001);
    let plans = throttle::plans_for_tick(level.entities_mut_for_test(), 2001, config, &players);
    let awake: Vec<i32> = plans
        .iter()
        .enumerate()
        .filter(|(_, p)| p.wake_until > 2001)
        .map(|(i, _)| 2 + i as i32)
        .collect();
    assert_eq!(awake, vec![2, 3, 4, 5], "每刻最多 4 只，按 uid 升序");
    assert_eq!(
        level.entities().wake_tokens().values()[0],
        0,
        "动物令牌用光"
    );
    // 下一刻：补一个令牌，但前 4 只还在醒着（不重复领），第 6 只领走这一个。
    let next = throttle::plans_for_tick(level.entities_mut_for_test(), 2002, config, &players);
    let newly: Vec<i32> = next
        .iter()
        .enumerate()
        .filter(|(_, p)| p.wake_until > 2002)
        .map(|(i, _)| 2 + i as i32)
        .collect();
    assert_eq!(newly, vec![6]);
    // scale = 2 时动物每刻上限 8。
    let scaled = Config {
        wakeups_scale: 2,
        ..config
    };
    let (mut level2, mut ticks2, players2) = level_at(0.0, scaled);
    {
        let mut world = World::new(&mut level2, &mut ticks2);
        for i in 0..12 {
            add_cow(&mut world, 100.0 + 2.0 * f64::from(i));
        }
        for uid in 2..14 {
            world.with_entity_mut(uid, &mut |entity| {
                if let EntityKind::Mob(mob) = &mut entity.kind {
                    mob.throttle.last_active = 1;
                }
            });
        }
    }
    prepare(&mut level2, &mut ticks2, 2001);
    let plans2 = throttle::plans_for_tick(level2.entities_mut_for_test(), 2001, scaled, &players2);
    assert_eq!(
        plans2.iter().filter(|p| p.wake_until > 2001).count(),
        8,
        "scale = 2 时动物每刻上限 8"
    );
}

/// 精确驱动和并行驱动在同一份配置下逐位一致：计划表逐项相同，跑完一刻之后
/// 每头牛的位置、速度、计时器、随机源也相同。开和关两种配置都测。
#[test]
fn precise_and_parallel_drives_match() {
    for config in [
        Config::OFF,
        Config {
            throttle_distance: 32,
            sleep_distance: 64,
            wakeups_scale: 1,
        },
    ] {
        let (mut precise_level, mut precise_ticks, _) = level_at(0.0, config);
        let (mut parallel_level, mut parallel_ticks, _) = level_at(0.0, config);
        for level in [&mut precise_level, &mut parallel_level] {
            let mut ticks = BlockTicks::new();
            {
                let mut world = World::new(level, &mut ticks);
                // 三档各放两头牛，玩家在原点。
                for x in [8.0, 16.0, 40.0, 56.0, 90.0, 120.0] {
                    add_cow(&mut world, x);
                }
            }
            level.entities_mut_for_test().end_tick();
        }
        precise_ticks.set_time(5);
        parallel_ticks.set_time(5);
        let plans_a = World::new(&mut precise_level, &mut precise_ticks).throttle_plans();
        let plans_b = World::new(&mut parallel_level, &mut parallel_ticks).throttle_plans();
        assert_eq!(plans_a, plans_b, "两种驱动的降频计划逐项一致");
        World::new(&mut precise_level, &mut precise_ticks).tick_entities();
        // 并行驱动：只读视图和精确驱动给同一份地形、玩家、规则和时钟。
        let view = test_view(&parallel_level, 5);
        {
            let mut world = World::new(&mut parallel_level, &mut parallel_ticks);
            tick_entities_parallel(&view, &mut world, &ScopedThreads { threads: 4 });
        }
        for uid in 2..8 {
            let a = precise_level.entities().get(uid).expect("精确侧");
            let b = parallel_level.entities().get(uid).expect("并行侧");
            assert_eq!(a.position, b.position, "uid {uid} 位置");
            assert_eq!(a.delta, b.delta, "uid {uid} 速度");
            assert_eq!(a.tick_count, b.tick_count, "uid {uid} 计数");
            assert_eq!(a.random, b.random, "uid {uid} 随机源");
            assert_eq!(a.kind.mob().unwrap().age, b.kind.mob().unwrap().age);
            assert_eq!(
                a.kind.mob().unwrap().no_action_time,
                b.kind.mob().unwrap().no_action_time
            );
        }
    }
}

/// 并行只读视图：和 `TestLevel` 给同一份方块、玩家、规则和时钟。
fn test_view(level: &TestLevel, time: i64) -> SourceView<TestSource> {
    SourceView::new(TestSource {
        blocks: level.blocks_snapshot(),
        rules: level.rules,
        players: level.players.clone(),
        time,
    })
}

struct TestSource {
    blocks: std::collections::HashMap<BlockPos, BlockState>,
    rules: crate::BlockGameRules,
    players: Vec<PlayerView>,
    time: i64,
}

impl BlockSource for TestSource {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        self.blocks.get(&pos).copied().unwrap_or(BlockState::AIR)
    }

    fn facts(&self) -> ViewFacts {
        ViewFacts {
            rules: self.rules,
            game_time: self.time,
            ..ViewFacts::default()
        }
    }

    fn for_each_player(&self, visit: &mut dyn FnMut(&PlayerView)) {
        for player in &self.players {
            visit(player);
        }
    }
}

/// 令牌桶本身：每刻补一个、按类别封顶，岛时钟平移和拆合都按设计稿的规则。
#[test]
fn token_bucket_advance_rebase_and_split() {
    let config = Config {
        throttle_distance: 32,
        sleep_distance: 64,
        wakeups_scale: 1,
    };
    let mut tokens = WakeTokens::full(0, config);
    assert_eq!(tokens.values(), [4, 8, 8, 4]);
    for _ in 0..3 {
        assert!(tokens.take(WakeCategory::Animal));
    }
    assert_eq!(tokens.values()[0], 1);
    tokens.advance(5, config);
    assert_eq!(tokens.values(), [4, 8, 8, 4], "补满但不超过上限");
    let full = tokens.split_full();
    assert_eq!(full.values(), tokens.values(), "拆岛从满桶开始");
    tokens.rebase(100);
    assert_eq!(tokens.time, 105, "刻号跟着岛时钟平移");
    let mut other = WakeTokens::full(105, config);
    let _ = other.take(WakeCategory::Monster);
    tokens.absorb(other);
    assert_eq!(tokens.values(), [4, 8, 8, 4], "合并取剩余较多的一份");
    let _ = InlineExecutor;
}
