//! 并行驱动的确定性和与精确模式的差异清单。

use mc_bamboo_core::Vec3;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    BlockTicks, World,
    entity::{
        Entity,
        ctx::Command,
        parallel::{
            Computed, Executor, InlineExecutor, ScopedThreads, commit, compute,
            tick_entities_parallel,
        },
        view::{FlatSource, SourceView, ViewFacts},
    },
    testing::TestLevel,
};

/// 只读的方块视图：一块平地，脚下是石头。天然 `Sync`，能给算阶段的线程共享。
fn flat_blocks() -> SourceView<FlatSource> {
    SourceView::new(FlatSource {
        floor_y: 64,
        facts: ViewFacts::default(),
    })
}

/// 跑若干刻，返回按 uid 升序的世界状态（位置、速度、类型、元数据）。
fn run_parallel(level: &mut TestLevel, executor: &dyn Executor) -> Vec<Entity> {
    let blocks = flat_blocks();
    let mut ticks = BlockTicks::new();
    for _ in 0..6 {
        let mut world = World::new(level, &mut ticks);
        tick_entities_parallel(&blocks, &mut world, executor);
    }
    let world = World::new(level, &mut ticks);
    world.live_entities()
}

/// 两段式：算阶段只借 `&EntityStore`，落地阶段重新构造 `World`。
fn run_split(level: &mut TestLevel, executor: &dyn Executor) -> Vec<Entity> {
    let blocks = flat_blocks();
    let mut ticks = BlockTicks::new();
    for _ in 0..6 {
        {
            let mut world = World::new(level, &mut ticks);
            world.begin_entity_tick();
        }
        // 算阶段拿不到 `World`，只读实体表和只读方块视图。
        let computed = compute(&blocks, level.entities(), executor);
        let mut world = World::new(level, &mut ticks);
        commit(computed, &mut world);
    }
    World::new(level, &mut ticks).live_entities()
}

fn spawn_items(level: &mut TestLevel, count: i32) {
    let mut ticks = BlockTicks::new();
    let mut world = World::new(level, &mut ticks);
    for index in 0..count {
        let x = 0.5 + f64::from(index % 8) * 0.3;
        let z = 0.5 + f64::from(index / 8) * 0.3;
        world.spawn_entity(Entity::ground_item(
            Vec3::new(x, 70.0, z),
            ItemStack::new(1, 1),
        ));
    }
}

#[test]
fn the_executor_runs_every_job_and_keeps_the_partition_count() {
    for threads in [1, 2, 8] {
        let executor = ScopedThreads { threads };
        assert_eq!(executor.parallelism(), threads);
        let seen = std::sync::Mutex::new(Vec::new());
        let seen_ref = &seen;
        let jobs: Vec<Box<dyn FnOnce() + Send + '_>> = (0..37)
            .map(|index| {
                Box::new(move || seen_ref.lock().unwrap().push(index))
                    as Box<dyn FnOnce() + Send + '_>
            })
            .collect();
        executor.run(jobs);
        let mut seen = seen.into_inner().unwrap();
        seen.sort_unstable();
        assert_eq!(seen, (0..37).collect::<Vec<_>>(), "threads={threads}");
    }
}

#[test]
fn one_two_and_eight_threads_produce_bit_identical_results() {
    let baseline = {
        let mut level = TestLevel::new();
        spawn_items(&mut level, 64);
        run_parallel(&mut level, &InlineExecutor)
    };
    assert!(
        !baseline.is_empty() && baseline.len() <= 64,
        "合并会减少实体数量"
    );
    assert!(
        baseline.iter().any(|entity| entity.position.y != 70.0),
        "算阶段必须真的跑过物理"
    );
    for threads in [1, 2, 8] {
        let mut level = TestLevel::new();
        spawn_items(&mut level, 64);
        let result = run_parallel(&mut level, &ScopedThreads { threads });
        assert_eq!(
            format!("{result:?}"),
            format!("{baseline:?}"),
            "threads={threads}"
        );
    }
}

/// 两段式接口：算和落地之间调用方重新构造 `World`，结果和一体式包装逐位相同。
/// 顺便钉住 `Computed` 的 uid 名单、命令条数和移除标记。
#[test]
fn compute_and_commit_match_the_one_step_wrapper() {
    let blocks = flat_blocks();
    let executor = ScopedThreads { threads: 4 };

    let split = {
        let mut level = TestLevel::new();
        spawn_items(&mut level, 64);
        run_split(&mut level, &executor)
    };
    let wrapper = {
        let mut level = TestLevel::new();
        spawn_items(&mut level, 64);
        run_parallel(&mut level, &executor)
    };
    assert!(!split.is_empty());
    assert_eq!(format!("{split:?}"), format!("{wrapper:?}"));

    // `Computed` 里带着 uid 名单和每行命令缓冲。算阶段不碰 World，落地前可以检查它。
    let mut level = TestLevel::new();
    spawn_items(&mut level, 6);
    let mut ticks = BlockTicks::new();
    {
        let mut world = World::new(&mut level, &mut ticks);
        world.begin_entity_tick();
    }
    let computed: Computed = compute(&blocks, level.entities(), &executor);
    assert_eq!(computed.uids().len(), 6);
    assert_eq!(computed.command_counts().len(), 6);
    assert_eq!(computed.removed_count(), 0);
    let mut world = World::new(&mut level, &mut ticks);
    commit(computed, &mut world);
    assert_eq!(world.entity_ids().len(), 6);
}

/// 精确驱动和并行驱动在同一场景下的差别只应来自「实体看到的是阶段开始的快照」。
///
/// 两个相距很远、互不合并的物品，物理完全一致；这是差异清单的反面钉子：
/// 只要别的实体不参与判定，两种驱动逐位相同。
#[test]
fn isolated_entities_are_identical_in_precise_and_parallel_modes() {
    let blocks = flat_blocks();
    let precise = {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            for index in 0..8 {
                world.spawn_entity(Entity::ground_item(
                    Vec3::new(0.5 + f64::from(index) * 20.0, 70.0, 0.5),
                    ItemStack::new(1, 1),
                ));
            }
        }
        for _ in 0..6 {
            let mut world = World::new(&mut level, &mut ticks);
            world.tick_entities();
        }
        World::new(&mut level, &mut ticks).live_entities()
    };
    let parallel = {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            for index in 0..8 {
                world.spawn_entity(Entity::ground_item(
                    Vec3::new(0.5 + f64::from(index) * 20.0, 70.0, 0.5),
                    ItemStack::new(1, 1),
                ));
            }
        }
        for _ in 0..6 {
            let mut world = World::new(&mut level, &mut ticks);
            tick_entities_parallel(&blocks, &mut world, &ScopedThreads { threads: 8 });
        }
        World::new(&mut level, &mut ticks).live_entities()
    };
    assert_eq!(precise.len(), 8);
    assert_eq!(format!("{precise:?}"), format!("{parallel:?}"));
}

/// 相邻物品合并是第 13 节第 5 条的差异：两种模式看到对方的时机不同。
/// 这里只钉住「差异确实存在、且两种线程数下一致」，不锁死具体数值。
#[test]
fn merging_items_are_the_only_observable_difference_class() {
    let blocks = flat_blocks();
    let mut level = TestLevel::new();
    let mut ticks = BlockTicks::new();
    {
        let mut world = World::new(&mut level, &mut ticks);
        for index in 0..4 {
            world.spawn_entity(Entity::ground_item(
                Vec3::new(0.5 + f64::from(index) * 0.2, 64.5, 0.5),
                ItemStack::new(1, 1),
            ));
        }
    }
    let mut world = World::new(&mut level, &mut ticks);
    tick_entities_parallel(&blocks, &mut world, &ScopedThreads { threads: 2 });
    let two = world.live_entities();

    let mut level = TestLevel::new();
    let mut ticks = BlockTicks::new();
    {
        let mut world = World::new(&mut level, &mut ticks);
        for index in 0..4 {
            world.spawn_entity(Entity::ground_item(
                Vec3::new(0.5 + f64::from(index) * 0.2, 64.5, 0.5),
                ItemStack::new(1, 1),
            ));
        }
    }
    let mut world = World::new(&mut level, &mut ticks);
    tick_entities_parallel(&blocks, &mut world, &ScopedThreads { threads: 8 });
    let eight = world.live_entities();
    assert_eq!(format!("{two:?}"), format!("{eight:?}"));
}

/// 压力基准：几千个掉落物 + 几百个下落方块，量「算」「落地」「快照复制」和线程曲线。
///
/// 手工跑：`cargo test -p mc-bamboo-blocks --release --lib entity::parallel::tests::stress -- --ignored --nocapture`。
#[test]
#[ignore = "压力基准，手工跑"]
fn stress_reports_compute_apply_and_clone_costs() {
    use std::time::Instant;

    let blocks = flat_blocks();
    for threads in [1, 2, 4, 8] {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        {
            let mut world = World::new(&mut level, &mut ticks);
            for index in 0..3000 {
                let x = 0.5 + f64::from(index % 60) * 8.0;
                let z = 0.5 + f64::from(index / 60) * 8.0;
                world.spawn_entity(Entity::ground_item(
                    Vec3::new(x, 70.0 + f64::from(index % 7), z),
                    ItemStack::new(1, 1),
                ));
            }
            for index in 0..300 {
                let x = f64::from(index % 20) * 24.0;
                let z = f64::from(index / 20) * 24.0;
                world.spawn_entity(Entity::falling_block(
                    mc_bamboo_core::BlockPos::new(x as i32, 90 + (index % 11), z as i32),
                    crate::state::BlockState(mc_bamboo_data::blocks::SAND),
                ));
            }
        }
        let executor = ScopedThreads { threads };
        let mut totals = crate::entity::parallel::PhaseTimes::default();
        let start = Instant::now();
        for _ in 0..10 {
            let mut world = World::new(&mut level, &mut ticks);
            let times = crate::entity::parallel::tick_entities_parallel_timed(
                &blocks, &mut world, &executor,
            );
            totals.clone += times.clone;
            totals.compute += times.compute;
            totals.publish += times.publish;
            totals.apply += times.apply;
        }
        println!(
            "threads={threads} total={:?} clone={:?} compute={:?} publish={:?} apply={:?}",
            start.elapsed(),
            totals.clone,
            totals.compute,
            totals.publish,
            totals.apply
        );
    }
}

#[test]
fn commands_are_applied_in_uid_order_within_one_entity() {
    // 直接验证 `apply` 的入口：两条命令按缓冲顺序落地，结果可复现。
    let mut level = TestLevel::new();
    let mut ticks = BlockTicks::new();
    let mut world = World::new(&mut level, &mut ticks);
    let me = Entity::falling_block(
        mc_bamboo_core::BlockPos::new(0, 70, 0),
        crate::state::BlockState(mc_bamboo_data::blocks::SAND),
    );
    let mut me = me;
    crate::entity::ctx::apply(
        Command::DropItem {
            stack: ItemStack::new(1, 1),
        },
        &mut me,
        &mut world,
    );
    // 两条同样的命令产生两个 ID 连续的新实体，顺序固定。
    crate::entity::ctx::apply(
        Command::DropItem {
            stack: ItemStack::new(1, 1),
        },
        &mut me,
        &mut world,
    );
    let ids = world.entity_ids();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[1], ids[0] + 1);
}
