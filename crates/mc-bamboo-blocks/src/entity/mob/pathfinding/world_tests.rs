//! 独立寻路阶段的整世界单测：开关打开后随机闲逛真的走起来，
//! 精确 / 并行驱动和不同线程数逐位一致。

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::blocks;

use crate::{
    BlockState, BlockTicks, World,
    entity::{
        Entity, EntityKind,
        ctx::PlayerView,
        parallel::{ScopedThreads, tick_entities_parallel},
        view::{BlockSource, SourceView, ViewFacts},
    },
    testing::TestLevel,
};

/// 一块 `-16..=16` 的草地，实体 ID 从 2 起。
fn level() -> (TestLevel, BlockTicks) {
    let mut level = TestLevel::new();
    level.set_entities(crate::entity::EntityStore::with_next_id(2));
    let mut ticks = BlockTicks::new();
    {
        let mut world = World::new(&mut level, &mut ticks);
        for x in -16..=16 {
            for z in -16..=16 {
                world.set_block(BlockPos::new(x, 63, z), BlockState(blocks::GRASS_BLOCK), 0);
            }
        }
    }
    level.rules.mob_pathfinding_phase = true;
    level.rules.mob_pathfinding_budget = 0;
    level.rules.mob_pathfinding_fail_cooldown = false;
    // 玩家保持在原点：没有玩家时 `noActionTime` 涨过 100，闲逛目标永远不启动。
    level
        .players
        .push(crate::entity::mob::throttle::test_player(
            1,
            Vec3::new(0.0, 64.0, 0.0),
        ));
    (level, ticks)
}

fn add_cow(world: &mut World<'_>, x: f64, z: f64) -> i32 {
    let mut cow = Entity::cow(Vec3::new(x, 64.0, z));
    cow.on_ground = true;
    if let EntityKind::Mob(mob) = &mut cow.kind {
        mob.last_damage_stamp = i64::MIN / 2;
    }
    world.spawn_entity(cow)
}

fn prepare(level: &mut TestLevel, ticks: &mut BlockTicks, time: i64) {
    level.entities_mut_for_test().end_tick();
    ticks.set_time(time);
}

fn positions(level: &TestLevel, uids: &[i32]) -> Vec<Vec3> {
    uids.iter()
        .map(|&uid| level.entities().get(uid).expect("实体还在").position)
        .collect()
}

/// 开关打开后，随机闲逛的牛最终真的走起来，而且请求确实经过了寻路阶段。
#[test]
fn wander_walks_with_the_phase_enabled() {
    let (mut level, mut ticks) = level();
    let uid = {
        let mut world = World::new(&mut level, &mut ticks);
        add_cow(&mut world, 0.5, 0.5)
    };
    prepare(&mut level, &mut ticks, 1);
    let start = positions(&level, &[uid])[0];
    for tick in 1..=1200 {
        ticks.set_time(tick);
        World::new(&mut level, &mut ticks).tick_entities();
    }
    let end = positions(&level, &[uid])[0];
    assert!(
        start.distance_to_sqr(end) > 0.25,
        "开关打开后牛应当走起来：start {start:?}, end {end:?}"
    );
    assert!(
        level.entities().pathfinding().total_searches > 0,
        "请求应当经过寻路阶段，而不是当场同步算"
    );
}

/// 精确驱动、单线程并行、四线程并行：同一份世界跑同样的刻数后逐位一致。
#[test]
fn precise_and_parallel_phase_results_match() {
    let mut worlds = Vec::new();
    for _ in 0..3 {
        let (mut level, mut ticks) = level();
        let uids = {
            let mut world = World::new(&mut level, &mut ticks);
            vec![
                add_cow(&mut world, 0.5, 0.5),
                add_cow(&mut world, 4.5, 1.5),
                add_cow(&mut world, -3.5, 2.5),
            ]
        };
        prepare(&mut level, &mut ticks, 1);
        worlds.push((level, ticks, uids));
    }
    let ticks_to_run = 50;
    // 精确驱动。
    {
        let (level, ticks, _) = &mut worlds[0];
        for tick in 1..=ticks_to_run {
            ticks.set_time(tick);
            World::new(level, ticks).tick_entities();
        }
    }
    // 并行驱动：1 线程和 4 线程。
    for threads in [1, 4] {
        let (level, ticks, _) = &mut worlds[if threads == 1 { 1 } else { 2 }];
        for tick in 1..=ticks_to_run {
            ticks.set_time(tick);
            let view = test_view(level, tick);
            let mut world = World::new(level, ticks);
            tick_entities_parallel(&view, &mut world, &ScopedThreads { threads });
        }
    }
    let base = positions(&worlds[0].0, &worlds[0].2);
    for (index, (level, _, uids)) in worlds.iter().enumerate().skip(1) {
        assert_eq!(
            positions(level, uids),
            base,
            "第 {index} 份世界（并行）应当和精确驱动逐位一致"
        );
    }
}

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

/// 挂起的结果在 `stop`、换目标或序号过期后都丢掉，不塞给生物。
#[test]
fn install_drops_cancelled_and_stale_results() {
    use crate::entity::mob::{
        Mob, MobType,
        navigation::NavMob,
        path::PathMob,
        pathfinding::{Action, Replay, Use, install},
    };

    let level = TestLevel::new();
    let view = test_view(&level, 0);
    let nav = NavMob {
        path: PathMob {
            position: Vec3::new(0.5, 64.0, 0.5),
            on_ground: true,
            in_floatable_fluid: false,
            in_water: false,
            width: 0.6,
            height: 1.8,
            max_up_step: 0.6,
            max_fall_distance: 3,
            malus: Default::default(),
            context: crate::shape::CollisionContext::Empty,
        },
        speed: 1.0,
        in_liquid: false,
        follow_range: 16.0,
        game_time: 0,
        owner_uid: 1,
    };
    let replay = |local_serial, target| Replay {
        serial: (2_u64 << 32) | u64::from(local_serial),
        local_serial,
        target,
        at: BlockPos::new(1, 64, 0),
        path: None,
        action: Action::Inspect(Use::Ignore),
        nav,
        consumed: false,
    };

    // stop(): 序号到 3 为止都作废。
    let mut mob = Mob::new(MobType::Cow);
    mob.target = None;
    mob.navigation.request_serial = 3;
    mob.navigation.stop();
    mob.navigation.replay = Some(replay(3, -1));
    install(&mut mob, &view);
    assert!(mob.navigation.replay.is_none(), "stop 之后的结果要丢掉");

    // 目标变了：发请求时没有目标，结果回来时已经有目标。
    let mut mob = Mob::new(MobType::Cow);
    mob.target = Some(9);
    mob.navigation.replay = Some(replay(4, -1));
    install(&mut mob, &view);
    assert!(mob.navigation.replay.is_none(), "换目标之后的结果要丢掉");

    // 目标没变、序号也新：结果留着给原调用点取。
    let mut mob = Mob::new(MobType::Cow);
    mob.target = None;
    mob.navigation.replay = Some(replay(4, -1));
    install(&mut mob, &view);
    assert!(mob.navigation.replay.is_some(), "有效结果应当保留");
}

/// 寻路阶段打开时，控制载具的骑手把请求挂在**载具**那份导航上：阶段按载具 uid 记账，
/// 而不是骑手 uid。精确驱动和并行驱动都走同一条写回路径。
///
/// With the phase enabled, a controlling rider's path request belongs to the
/// vehicle's navigation and is keyed by the vehicle uid, not the rider uid.
#[test]
fn rider_requests_are_keyed_by_the_vehicle_uid() {
    let (mut level, mut ticks) = level();
    // 乘客 ID 必须 >= FIRST_ENTITY_ID：低于它的号在 `ride_tick_passengers` 里被当成
    // 玩家跳过，骑手就根本不会 tick。用世界实体的号段，和并行逐位一致测试一致。
    level.set_entities(crate::entity::EntityStore::with_next_id(
        crate::entity::FIRST_ENTITY_ID,
    ));
    let (spider, rider) = {
        let mut world = World::new(&mut level, &mut ticks);
        let spider = world.spawn_entity(Entity::by_mob_type(
            crate::entity::mob::MobType::Spider,
            Vec3::new(0.5, 64.0, 0.5),
        ));
        let rider = world.spawn_entity(Entity::by_mob_type(
            crate::entity::mob::MobType::Zombie,
            Vec3::new(0.5, 64.0, 0.5),
        ));
        (spider, rider)
    };
    prepare(&mut level, &mut ticks, 1);
    {
        let mut world = World::new(&mut level, &mut ticks);
        assert!(world.start_riding(rider, spider));
    }
    // 每刻读一次待交付结果的 `(记账 uid, 导航 owner)`，覆盖整段跑下来的所有请求，
    // 又不用取走结果（正常的交付 / install / 消费流程照常发生）。
    let mut owners = Vec::new();
    for tick in 1..=160 {
        ticks.set_time(tick);
        World::new(&mut level, &mut ticks).tick_entities();
        owners.extend(level.entities().pathfinding().delivery_owners());
    }
    assert!(
        level.entities().pathfinding().total_searches > 0,
        "骑手应当真的发起过寻路请求"
    );
    assert!(!owners.is_empty(), "阶段应当为骑手的请求交付过结果");
    assert!(
        owners.iter().all(|&(uid, _)| uid == spider),
        "挂起请求必须按载具 uid 记账: {owners:?}"
    );
    assert!(
        owners.iter().all(|&(_, owner)| owner == spider),
        "寻路快照的 owner 也必须是载具: {owners:?}"
    );
    assert!(
        !owners.iter().any(|&(uid, _)| uid == rider),
        "请求不能记在骑手名下: {owners:?}"
    );
}

/// 开关打开且有控制骑手时，精确驱动和 1/2/8 线程并行驱动逐位一致：整棵乘客树
/// （含借来的导航、挂起请求、交付结果）和寻路阶段状态都一样。
///
/// With the phase enabled and a controlling rider, the precise driver and the
/// parallel driver (1/2/8 threads) are bit-identical: the whole riding tree
/// (borrowed navigation, pending requests, deliveries) and the phase state match.
#[test]
fn rider_phase_is_bit_identical_in_precise_and_parallel_modes() {
    use crate::entity::FIRST_ENTITY_ID;

    fn build() -> (TestLevel, BlockTicks) {
        let (mut level, mut ticks) = level();
        level.set_entities(crate::entity::EntityStore::with_next_id(FIRST_ENTITY_ID));
        let (spider, rider) = {
            let mut world = World::new(&mut level, &mut ticks);
            let spider = world.spawn_entity(Entity::by_mob_type(
                crate::entity::mob::MobType::Spider,
                Vec3::new(0.5, 64.0, 0.5),
            ));
            let rider = world.spawn_entity(Entity::by_mob_type(
                crate::entity::mob::MobType::Zombie,
                Vec3::new(0.5, 64.0, 0.5),
            ));
            (spider, rider)
        };
        prepare(&mut level, &mut ticks, 1);
        {
            let mut world = World::new(&mut level, &mut ticks);
            assert!(world.start_riding(rider, spider));
        }
        (level, ticks)
    }

    fn snapshot(level: &mut TestLevel, ticks: &mut BlockTicks) -> String {
        let live = World::new(level, ticks).live_entities();
        let phase = level.entities().pathfinding();
        format!("{live:?}|{phase:?}")
    }

    let ticks_to_run = 40;
    let (mut precise_level, mut precise_ticks) = build();
    for tick in 1..=ticks_to_run {
        precise_ticks.set_time(tick);
        World::new(&mut precise_level, &mut precise_ticks).tick_entities();
    }
    let baseline = snapshot(&mut precise_level, &mut precise_ticks);

    for threads in [1, 2, 8] {
        let (mut level, mut ticks) = build();
        for tick in 1..=ticks_to_run {
            ticks.set_time(tick);
            let view = test_view(&level, tick);
            let mut world = World::new(&mut level, &mut ticks);
            tick_entities_parallel(&view, &mut world, &ScopedThreads { threads });
        }
        let got = snapshot(&mut level, &mut ticks);
        assert_eq!(
            got, baseline,
            "threads={threads}: 开关打开 + 控制骑手时精确与并行必须逐位一致"
        );
    }
}
