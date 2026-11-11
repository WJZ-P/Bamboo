//! 只有界面和比较器输出的容器：`BarrelBlock`、`ShulkerBoxBlock`、`AbstractFurnaceBlock` 的三个子类、
//! `HopperBlock`、`DispenserBlock`、`DropperBlock`。
//!
//! 内容在方块实体里，这里不做。木桶的 `open` 状态和潜影盒的盖子动画都由 `mc-bamboo-game` 的
//! `ContainerOpenersCounter` 和方块事件驱动，这里只放潜影盒的方块实体和形状。

use std::any::Any;

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{Block, BlockAncestor};
use mc_bamboo_nbt::Tag;

use super::{cooking::FurnaceEntity, facing_the_view};
use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, facing_the_player, update_neighbours_after_destroy},
    },
    block_entity::BlockEntity,
    context::{PlaceContext, UseContext},
    dispense,
    dispenser::dispense_from,
    hopper::HopperEntity,
    interact::InteractionResult,
    shape::{Aabb, Boxes, CollisionContext},
    state::{BlockState, ENABLED, FACING, FACING_HOPPER, TRIGGERED},
    world::{DeferredRead, UPDATE_ALL, UPDATE_CLIENTS, UPDATE_LIMIT, World},
};

/// `BarrelBlock`。
pub(super) struct Barrel;

impl Behaviour for Barrel {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_view(block, ctx))
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }

    /// `BarrelBlock.getAnalogOutputSignal`：比较器读木桶的内容。
    fn analog_output_signal(
        &self,
        _state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        world.container_signal(pos)
    }
}

/// 潜影盒动画每个 tick 走的进度，官方 `progress += 0.1F`。
const SHULKER_STEP: f32 = 0.1;
/// 整格的形状，没有方块实体时潜影盒的碰撞和支撑形状。
const FULL_BLOCK: Aabb = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

/// `ShulkerBoxBlockEntity.AnimationStatus`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShulkerStatus {
    Closed,
    Opening,
    Opened,
    Closing,
}

/// `ShulkerBoxBlockEntity`：潜影盒的盖子和开合人数。
///
/// 进度每刻由方块实体的 ticker 推进，形状和支撑面跟着进度走，见 [`ShulkerBoxEntity::bounding_box`]
/// 和 [`open_support_shape`]。
pub struct ShulkerBoxEntity {
    pos: BlockPos,
    open_count: i32,
    status: ShulkerStatus,
    progress: f32,
    progress_old: f32,
}

impl ShulkerBoxEntity {
    /// 刚放下的潜影盒：盖子关着，没有人在开。
    pub fn new(pos: BlockPos) -> Self {
        Self {
            pos,
            open_count: 0,
            status: ShulkerStatus::Closed,
            progress: 0.0,
            progress_old: 0.0,
        }
    }

    /// `isClosed`。官方只看状态，不看进度。
    pub const fn is_closed(&self) -> bool {
        matches!(self.status, ShulkerStatus::Closed)
    }

    /// `getAnimationStatus`。
    pub const fn status(&self) -> ShulkerStatus {
        self.status
    }

    /// 现在的开盖人数，官方 `openCount`。
    pub const fn open_count(&self) -> i32 {
        self.open_count
    }

    /// `getProgress(a)`：上一次和这一次的进度之间插值。
    pub fn progress(&self, a: f32) -> f32 {
        let factor = a.min(1.0);
        self.progress_old + factor * (self.progress - self.progress_old)
    }

    /// `getBoundingBox(state)`：盖子抬起来之后，碰撞形状跟着进度变。
    ///
    /// 官方这里用的是方块内的中心 `(0.5, 0.0, 0.5)`，碰撞形状是相对这一格的，调用方再加位置。
    pub fn bounding_box(&self, state: BlockState) -> Aabb {
        let progress = self.progress(1.0);
        progress_delta_aabb(
            1.0,
            state.get(FACING),
            -1.0,
            0.5 * progress,
            Vec3::new(0.5, 0.0, 0.5),
        )
    }

    /// `triggerEvent`：事件 1 换开盖人数，0 开始关、1 开始开，其余交给基类。
    pub fn trigger_event(&mut self, param_a: i32, param_b: i32) -> bool {
        if param_a != 1 {
            return false;
        }
        self.open_count = param_b;
        match param_b {
            0 => self.status = ShulkerStatus::Closing,
            1 => self.status = ShulkerStatus::Opening,
            _ => {}
        }
        true
    }

    /// `updateAnimation`：每刻推进一步。开始时和停住时更新邻居，形状跟着变。
    fn update_animation(&mut self, state: BlockState, world: &mut World) {
        self.progress_old = self.progress;
        match self.status {
            ShulkerStatus::Closed => self.progress = 0.0,
            ShulkerStatus::Opening => {
                self.progress += SHULKER_STEP;
                if self.progress_old == 0.0 {
                    do_neighbor_updates(state, world, self.pos);
                }
                if self.progress >= 1.0 {
                    self.status = ShulkerStatus::Opened;
                    self.progress = 1.0;
                    do_neighbor_updates(state, world, self.pos);
                }
            }
            ShulkerStatus::Opened => self.progress = 1.0,
            ShulkerStatus::Closing => {
                self.progress -= SHULKER_STEP;
                if self.progress_old == 1.0 {
                    do_neighbor_updates(state, world, self.pos);
                }
                if self.progress <= 0.0 {
                    self.status = ShulkerStatus::Closed;
                    self.progress = 0.0;
                    do_neighbor_updates(state, world, self.pos);
                }
            }
        }
    }
}

/// `ShulkerBoxBlockEntity.doNeighborUpdates`：形状变了，告诉邻居重新算。
///
/// 实体正被拿着时先排进队列。邻居要问盖子开没开，表里没有实体时会被当成关上。
///
/// While the entity is out of the map this is queued. Neighbours ask whether the lid is open,
/// and a missing entity is treated as closed.
fn do_neighbor_updates(state: BlockState, world: &mut World, pos: BlockPos) {
    if world.block_entity_is_taken(pos) {
        world.defer_block_entity_read(DeferredRead::ShulkerNeighbors { pos });
        return;
    }
    push_shulker_neighbor_updates(state, world, pos);
}

fn push_shulker_neighbor_updates(state: BlockState, world: &mut World, pos: BlockPos) {
    world.update_neighbour_shapes(state, pos, UPDATE_ALL, UPDATE_LIMIT);
    world.update_neighbors_at(pos, state.block());
}

/// 潜影盒放回表之后补跑 `doNeighborUpdates`。方块已经不是潜影盒时什么也不做。
///
/// Runs `doNeighborUpdates` after the shulker is put back. Does nothing if the cell is no
/// longer a shulker box.
pub(crate) fn apply_shulker_neighbor_updates(world: &mut World, pos: BlockPos) {
    let state = world.block_state(pos);
    if !state.is_a(BlockAncestor::ShulkerBoxBlock) {
        return;
    }
    push_shulker_neighbor_updates(state, world, pos);
}

impl BlockEntity for ShulkerBoxEntity {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, world: &mut World) {
        let state = world.block_state(self.pos);
        self.update_animation(state, world);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn write_additional(&self) -> Result<Vec<(String, Tag)>, crate::archive::Error> {
        // 官方 ShulkerBoxBlockEntity.saveAdditional 只写容器内容和战利品表；
        // 盖子动画和开合人数是运行时的状态，不存盘。
        Ok(Vec::new())
    }
}

/// `ShulkerBoxBlock.getBlockSupportShape` 里打开时的薄板：`SHAPES_OPEN_SUPPORT` 的一项。
///
/// 基准形状是底面那一层 `Block.boxZ(16.0, 0.0, 1.0)`，`rotateAll` 按方向转过去，
/// 箱子朝哪边抬盖子，剩下的薄板就在对面上。
pub(crate) const fn open_support_shape(facing: Direction) -> Aabb {
    match facing {
        Direction::Up => Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0 / 16.0, 1.0),
        Direction::Down => Aabb::new(0.0, 15.0 / 16.0, 0.0, 1.0, 1.0, 1.0),
        Direction::North => Aabb::new(0.0, 0.0, 15.0 / 16.0, 1.0, 1.0, 1.0),
        Direction::South => Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0 / 16.0),
        Direction::West => Aabb::new(15.0 / 16.0, 0.0, 0.0, 1.0, 1.0, 1.0),
        Direction::East => Aabb::new(0.0, 0.0, 0.0, 1.0 / 16.0, 1.0, 1.0),
    }
}

/// `ShulkerBoxBlock.canOpen` 检查的那半格空间，已经缩进 1e-6。世界坐标。
pub fn open_lid_box(pos: BlockPos, facing: Direction) -> Aabb {
    progress_delta_aabb(1.0, facing, 0.0, 0.5, bottom_center_of(pos)).deflated(1.0e-6)
}

/// `Shulker.getProgressDeltaAabb`。`center` 是盒子底面的中心，方块形状用格内的 `(0.5, 0, 0.5)`，
/// `canOpen` 用 `Vec3.atBottomCenterOf(pos)`。
fn progress_delta_aabb(size: f32, direction: Direction, from: f32, to: f32, center: Vec3) -> Aabb {
    let size = f64::from(size);
    let half = size * 0.5;
    let base = Aabb::new(-half, 0.0, -half, half, size, half);
    let max_movement = f64::from(from.max(to)) * size;
    let min_movement = f64::from(from.min(to)) * size;
    let (x, y, z) = direction.normal();
    let expanded = base.expanded_towards(
        f64::from(x) * max_movement,
        f64::from(y) * max_movement,
        f64::from(z) * max_movement,
    );
    let moved = expanded.contracted(
        -f64::from(x) * (size + min_movement),
        -f64::from(y) * (size + min_movement),
        -f64::from(z) * (size + min_movement),
    );
    moved.moved(center.x, center.y, center.z)
}

/// `Vec3.atBottomCenterOf`：方块底面的中心。
fn bottom_center_of(pos: BlockPos) -> Vec3 {
    Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    )
}

/// `ShulkerBoxBlock`。形状跟着方块实体里的盖子进度走。
pub(super) struct ShulkerBox;

impl Behaviour for ShulkerBox {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(default_state(block).with(FACING, ctx.clicked_face()))
    }

    /// `newBlockEntity`：换上潜影盒时建一个盖着的实体。
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) && !world.has_block_entity(pos) {
            world.set_block_entity(Box::new(ShulkerBoxEntity::new(pos)));
        }
    }

    /// `BaseEntityBlock.triggerEvent`：交给方块实体，没有实体时是假。
    fn trigger_event(
        &self,
        _state: BlockState,
        world: &mut World,
        pos: BlockPos,
        param_a: i32,
        param_b: i32,
    ) -> bool {
        world
            .with_block_entity::<ShulkerBoxEntity, _>(pos, |entity, _| {
                entity.trigger_event(param_a, param_b)
            })
            .unwrap_or(false)
    }

    /// `getShape`：有实体时按盖子的进度，没有实体时是整格。
    fn collision_boxes(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        _context: CollisionContext,
    ) -> Boxes {
        match world.block_entity::<ShulkerBoxEntity>(pos) {
            Some(entity) => Boxes::Owned(vec![entity.bounding_box(state)]),
            None => Boxes::Owned(vec![FULL_BLOCK]),
        }
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }

    /// `ShulkerBoxBlock.getAnalogOutputSignal`：比较器读潜影盒的内容。
    fn analog_output_signal(
        &self,
        _state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        world.container_signal(pos)
    }
}

/// `AbstractFurnaceBlock`：`FurnaceBlock`、`BlastFurnaceBlock`、`SmokerBlock` 只有界面不同。
pub(super) struct Furnace;

impl Behaviour for Furnace {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_player(block, ctx))
    }

    /// `newBlockEntity`：放下熔炉系列时建一个空实体，种类由方块决定。
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state)
            && !world.has_block_entity(pos)
            && let Some(kind) = FurnaceEntity::kind_of(state.class())
        {
            world.set_block_entity(Box::new(FurnaceEntity::new(pos, kind)));
        }
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    /// `AbstractFurnaceBlock.getAnalogOutputSignal`：比较器读三格内容。
    ///
    /// 实体被拿出去时这里读不到它，比较器会等它放回去再读。没有实体时用槽位表。
    ///
    /// `AbstractFurnaceBlock.getAnalogOutputSignal` reads the three slots.
    ///
    /// While the entity is out of the map this cannot see it; comparators read it after it is
    /// put back. With no entity, the slot table is used.
    fn analog_output_signal(
        &self,
        _state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        crate::machine_container::furnace_signal(world, pos)
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }
}

/// `HopperBlock`。
pub(super) struct Hopper;

/// `checkPoweredState`。
fn check_powered_state(state: BlockState, world: &mut World, pos: BlockPos) {
    let should_be_on = !world.has_neighbor_signal(pos);
    if should_be_on != state.get(ENABLED) {
        world.set_block(pos, state.with(ENABLED, should_be_on), UPDATE_CLIENTS);
    }
}

impl Behaviour for Hopper {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        let toward_clicked = ctx.clicked_face().opposite();
        let facing = if toward_clicked.axis().is_vertical() {
            Direction::Down
        } else {
            toward_clicked
        };
        let state = default_state(block)
            .with(FACING_HOPPER, facing)
            .with(ENABLED, true);
        Some(state)
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) {
            check_powered_state(state, world, pos);
            if !world.has_block_entity(pos) {
                world.set_block_entity(Box::new(HopperEntity::new(pos)));
            }
        }
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        check_powered_state(state, world, pos);
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }

    /// `HopperBlock.getAnalogOutputSignal`：比较器读漏斗里的 5 格。
    fn analog_output_signal(
        &self,
        _state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        world.container_signal(pos)
    }
}

/// `TRIGGER_DURATION`：发射器被触发之后隔这么多刻发射。
const TRIGGER_DURATION: i32 = 4;

/// `DispenserBlock` 和 `DropperBlock`。被信号触发时排一个计划刻，真正发射物品要方块实体里的
/// 内容，不在范围里。两个类只有发射的方式不同。
pub(super) struct Dispenser;

impl Behaviour for Dispenser {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_view(block, ctx))
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        let should_trigger =
            world.has_neighbor_signal(pos) || world.has_neighbor_signal(pos.above());
        if should_trigger && !state.get(TRIGGERED) {
            world.schedule_tick(pos, state.block(), TRIGGER_DURATION);
            world.set_block(pos, state.with(TRIGGERED, true), UPDATE_CLIENTS);
        } else if !should_trigger && state.get(TRIGGERED) {
            world.set_block(pos, state.with(TRIGGERED, false), UPDATE_CLIENTS);
        }
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }

    /// `DispenserBlock.tick`：计划刻到期。投掷器把物品塞进容器或抛出去。
    ///
    /// 发射器走 `DispenserBlock.dispenseFrom`，见 [`crate::dispense`]。
    ///
    /// `DispenserBlock.tick`: the scheduled tick. A dropper inserts into a container or throws.
    ///
    /// A dispenser runs `DispenserBlock.dispenseFrom`. See [`crate::dispense`].
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.is_a(BlockAncestor::DropperBlock) {
            dispense_from(world, pos, state);
        } else {
            dispense::dispense_from(world, pos, state);
        }
    }

    /// `DispenserBlock.getAnalogOutputSignal`：比较器读 9 格。
    fn analog_output_signal(
        &self,
        _state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        world.container_signal(pos)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::blocks;

    use super::*;
    use crate::{BlockTicks, Effect, SupportType, testing::TestLevel};

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    /// 放着一个盖着的潜影盒、实体已经登记好。
    fn placed_shulker(facing: Direction) -> (TestLevel, BlockTicks) {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::SHULKER_BOX).with(FACING, facing));
        let mut ticks = BlockTicks::new();
        World::new(&mut level, &mut ticks).set_block_entity(Box::new(ShulkerBoxEntity::new(POS)));
        (level, ticks)
    }

    fn state_at(world: &World) -> BlockState {
        world.block_state(POS)
    }

    fn open_event(world: &mut World, param_b: i32) {
        let block = state_at(world).block();
        world.block_event(POS, block, 1, param_b);
        world.run_block_events();
    }

    #[test]
    fn the_open_event_starts_the_lid_and_reports_it() {
        let (mut level, mut ticks) = placed_shulker(Direction::Up);
        let mut world = World::new(&mut level, &mut ticks);

        open_event(&mut world, 1);
        let entity = world
            .block_entity::<ShulkerBoxEntity>(POS)
            .expect("实体应该还在");
        assert_eq!(entity.status(), ShulkerStatus::Opening);
        assert_eq!(entity.open_count(), 1);
        assert!(matches!(
            world.into_effects().as_slice(),
            [Effect::BlockEvent {
                param_a: 1,
                param_b: 1,
                ..
            }]
        ));
    }

    #[test]
    fn ten_ticks_open_the_lid_and_neighbour_updates_follow_it() {
        let (mut level, mut ticks) = placed_shulker(Direction::Up);
        let mut world = World::new(&mut level, &mut ticks);
        open_event(&mut world, 1);

        for _ in 0..10 {
            world.tick_block_entities();
        }

        let entity = world
            .block_entity::<ShulkerBoxEntity>(POS)
            .expect("实体应该还在");
        assert_eq!(entity.status(), ShulkerStatus::Opened);
        assert_eq!(entity.progress(1.0), 1.0);
        let state = state_at(&world);
        assert_eq!(
            entity.bounding_box(state),
            Aabb::new(0.0, 0.0, 0.0, 1.0, 1.5, 1.0)
        );
        // 盖子打开了，只剩底面那一层支撑。
        assert!(world.is_face_sturdy(POS, Direction::Down, SupportType::Full));
        assert!(!world.is_face_sturdy(POS, Direction::Up, SupportType::Full));
    }

    #[test]
    fn the_close_event_brings_the_lid_back_down() {
        let (mut level, mut ticks) = placed_shulker(Direction::Up);
        let mut world = World::new(&mut level, &mut ticks);
        open_event(&mut world, 1);
        for _ in 0..10 {
            world.tick_block_entities();
        }

        open_event(&mut world, 0);
        let entity = world
            .block_entity::<ShulkerBoxEntity>(POS)
            .expect("实体应该还在");
        assert_eq!(entity.status(), ShulkerStatus::Closing);

        for _ in 0..10 {
            world.tick_block_entities();
        }
        let entity = world
            .block_entity::<ShulkerBoxEntity>(POS)
            .expect("实体应该还在");
        assert_eq!(entity.status(), ShulkerStatus::Closed);
        assert_eq!(entity.progress(1.0), 0.0);
        assert_eq!(
            entity.bounding_box(state_at(&world)),
            Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)
        );
    }

    #[test]
    fn other_events_fall_through_to_the_base_class() {
        let mut entity = ShulkerBoxEntity::new(POS);
        assert!(entity.trigger_event(1, 1));
        assert!(!entity.trigger_event(2, 0));
        assert!(!entity.trigger_event(0, 1));
    }

    #[test]
    fn only_chests_and_shulkers_answer_the_open_event() {
        let (mut level, mut ticks) = placed_shulker(Direction::Up);
        let mut world = World::new(&mut level, &mut ticks);
        let mut a = |block| BlockState(block).trigger_event(&mut world, POS, 1, 1);

        assert!(a(blocks::CHEST));
        assert!(a(blocks::TRAPPED_CHEST));
        assert!(a(blocks::COPPER_CHEST));
        assert!(a(blocks::ENDER_CHEST));
        assert!(a(blocks::SHULKER_BOX), "有实体时潜影盒自己处理");
        assert!(!a(blocks::BARREL), "木桶没有 triggerEvent");
        assert!(!a(blocks::HOPPER));
        assert!(!BlockState(blocks::CHEST).trigger_event(&mut world, POS, 2, 1));
    }

    #[test]
    fn a_shulker_without_its_entity_ignores_the_event() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::SHULKER_BOX));
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        assert!(!BlockState(blocks::SHULKER_BOX).trigger_event(&mut world, POS, 1, 1));
    }

    #[test]
    fn the_open_support_slab_sits_opposite_the_lid() {
        let up = open_support_shape(Direction::Up);
        assert_eq!(up, Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0 / 16.0, 1.0));
        let down = open_support_shape(Direction::Down);
        assert_eq!(down, Aabb::new(0.0, 15.0 / 16.0, 0.0, 1.0, 1.0, 1.0));
        let north = open_support_shape(Direction::North);
        assert_eq!(north, Aabb::new(0.0, 0.0, 15.0 / 16.0, 1.0, 1.0, 1.0));
        let south = open_support_shape(Direction::South);
        assert_eq!(south, Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0 / 16.0));
        let west = open_support_shape(Direction::West);
        assert_eq!(west, Aabb::new(15.0 / 16.0, 0.0, 0.0, 1.0, 1.0, 1.0));
        let east = open_support_shape(Direction::East);
        assert_eq!(east, Aabb::new(0.0, 0.0, 0.0, 1.0 / 16.0, 1.0, 1.0));
    }

    #[test]
    fn the_lid_box_is_the_half_block_in_front_of_the_face() {
        let pos = BlockPos::new(3, 64, -2);
        let up = open_lid_box(pos, Direction::Up);
        assert_eq!(
            up,
            Aabb::new(3.0, 65.0, -2.0, 4.0, 65.5, -1.0).deflated(1.0e-6)
        );
        let down = open_lid_box(pos, Direction::Down);
        assert_eq!(
            down,
            Aabb::new(3.0, 63.5, -2.0, 4.0, 64.0, -1.0).deflated(1.0e-6)
        );
    }
}
