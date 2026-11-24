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
