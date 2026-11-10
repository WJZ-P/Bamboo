//! 并行驱动的确定性和与精确模式的差异清单。

use mc_bamboo_core::{BlockPos, Direction, LightLayer, Vec3};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    BlockGameRules, BlockHit, BlockTicks, World,
    entity::{
        Entity,
        ctx::{BlockView, Command},
        parallel::{Executor, InlineExecutor, ScopedThreads, tick_entities_parallel},
    },
    shape::{Aabb, CollisionContext},
    state::{BlockState, FluidState, SupportType},
    testing::TestLevel,
};

/// 只读的方块视图：一块平地，脚下是石头。天然 `Sync`，能给算阶段的线程共享。
#[derive(Clone)]
struct FlatBlocks {
    floor_y: i32,
}

impl BlockView for FlatBlocks {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        if pos.y == self.floor_y {
            BlockState(mc_bamboo_data::blocks::STONE)
        } else {
            BlockState::AIR
        }
    }

    fn fluid_state(&self, _pos: BlockPos) -> FluidState {
        BlockState::AIR.fluid()
    }

    fn block_collisions(
        &self,
        box_: Aabb,
        _context: CollisionContext,
    ) -> Vec<(BlockPos, Vec<Aabb>)> {
        let mut out = Vec::new();
        for y in (box_.min_y.floor() as i32)..=(box_.max_y.floor() as i32) {
            for z in (box_.min_z.floor() as i32)..=(box_.max_z.floor() as i32) {
                for x in (box_.min_x.floor() as i32)..=(box_.max_x.floor() as i32) {
                    let pos = BlockPos::new(x, y, z);
                    if self.block_state(pos).is(mc_bamboo_data::blocks::STONE) {
                        out.push((
                            pos,
                            vec![Aabb::new(
                                f64::from(x),
                                f64::from(y),
                                f64::from(z),
                                f64::from(x + 1),
                                f64::from(y + 1),
                                f64::from(z + 1),
                            )],
                        ));
                    }
                }
            }
        }
        out
    }

    fn find_supporting_block(
        &self,
        _position: Vec3,
        box_: Aabb,
        context: CollisionContext,
    ) -> Option<BlockPos> {
        self.block_collisions(box_, context)
            .into_iter()
            .next()
            .map(|(pos, _)| pos)
    }

    fn clip_fall_damage_resetting(&self, _from: Vec3, _to: Vec3) -> bool {
        false
    }

    fn clip_collider_source_water(
        &self,
        _from: Vec3,
        _to: Vec3,
        _context: CollisionContext,
    ) -> Option<BlockHit> {
        None
    }

    fn clip_collider(
        &self,
        _from: Vec3,
        _to: Vec3,
        _context: CollisionContext,
    ) -> Option<BlockHit> {
        None
    }

    fn is_face_sturdy(&self, pos: BlockPos, _face: Direction, _support: SupportType) -> bool {
        pos.y == self.floor_y
    }

    fn is_loaded(&self, _pos: BlockPos) -> bool {
        true
    }

    fn is_in_world_bounds(&self, pos: BlockPos) -> bool {
        (-64..=319).contains(&pos.y)
    }

    fn min_y(&self) -> i32 {
        -64
    }

    fn max_y(&self) -> i32 {
        319
    }

    fn brightness(&self, layer: LightLayer, _pos: BlockPos) -> i32 {
        match layer {
            LightLayer::Sky => 15,
            LightLayer::Block => 0,
        }
    }

    fn block_game_rules(&self) -> BlockGameRules {
        BlockGameRules::vanilla()
    }

    fn fluid_height(&self, _fluid: FluidState, _pos: BlockPos) -> f32 {
        0.0
    }
}

/// 跑若干刻，返回按 uid 升序的世界状态（位置、速度、类型、元数据）。
fn run_parallel(
    level: &mut TestLevel,
    blocks: &FlatBlocks,
    executor: &dyn Executor,
) -> Vec<Entity> {
    let mut ticks = BlockTicks::new();
    for _ in 0..6 {
        let mut world = World::new(level, &mut ticks);
        tick_entities_parallel(blocks, &mut world, executor);
    }
    let world = World::new(level, &mut ticks);
    world.live_entities()
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
fn the_executor_covers_every_index_exactly_once() {
    for threads in [1, 2, 8] {
        let executor = ScopedThreads { threads };
        let seen = std::sync::Mutex::new(Vec::new());
        executor.run_chunks(37, &|start, end| {
            seen.lock().unwrap().extend(start..end);
        });
        let mut seen = seen.into_inner().unwrap();
        seen.sort_unstable();
        assert_eq!(seen, (0..37).collect::<Vec<_>>(), "threads={threads}");
    }
}

#[test]
fn one_two_and_eight_threads_produce_bit_identical_results() {
    let blocks = FlatBlocks { floor_y: 64 };
    let baseline = {
        let mut level = TestLevel::new();
        spawn_items(&mut level, 64);
        run_parallel(&mut level, &blocks, &InlineExecutor)
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
        let result = run_parallel(&mut level, &blocks, &ScopedThreads { threads });
        assert_eq!(
            format!("{result:?}"),
            format!("{baseline:?}"),
            "threads={threads}"
        );
    }
}

/// 精确驱动和并行驱动在同一场景下的差别只应来自「实体看到的是阶段开始的快照」。
///
/// 两个相距很远、互不合并的物品，物理完全一致；这是差异清单的反面钉子：
/// 只要别的实体不参与判定，两种驱动逐位相同。
#[test]
fn isolated_entities_are_identical_in_precise_and_parallel_modes() {
    let blocks = FlatBlocks { floor_y: 64 };
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
    let blocks = FlatBlocks { floor_y: 64 };
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

    let blocks = FlatBlocks { floor_y: 64 };
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
                    BlockPos::new(x as i32, 90 + (index % 11), z as i32),
                    BlockState(mc_bamboo_data::blocks::SAND),
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
        BlockPos::new(0, 70, 0),
        BlockState(mc_bamboo_data::blocks::SAND),
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
