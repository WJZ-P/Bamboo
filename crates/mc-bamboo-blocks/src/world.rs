//! 官方 `Level` 里和方块有关的部分：`setBlock`、`destroyBlock`、邻居更新、计划刻。
//!
//! `Behaviour` 的方法都通过 [`World`] 读写世界。方法名和官方相同，要对照时直接搜官方的名字。

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{Block, BlockAncestor, blocks};
use tracing::error;

use crate::{
    context::BlockHit,
    level::{Effect, Level},
    shape::{self, CollisionContext},
    state::{BlockState, FluidState, SupportType},
    ticks::{BlockTicks, TickPriority},
};

/// `Block.UPDATE_NEIGHBORS`。
pub const UPDATE_NEIGHBORS: u32 = 1;
/// `Block.UPDATE_CLIENTS`。
pub const UPDATE_CLIENTS: u32 = 2;
/// `Block.UPDATE_INVISIBLE`。
pub const UPDATE_INVISIBLE: u32 = 4;
/// `Block.UPDATE_IMMEDIATE`。
pub const UPDATE_IMMEDIATE: u32 = 8;
/// `Block.UPDATE_KNOWN_SHAPE`。
pub const UPDATE_KNOWN_SHAPE: u32 = 16;
/// `Block.UPDATE_SUPPRESS_DROPS`。
pub const UPDATE_SUPPRESS_DROPS: u32 = 32;
/// `Block.UPDATE_MOVE_BY_PISTON`。
pub const UPDATE_MOVE_BY_PISTON: u32 = 64;
/// `Block.UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE`。
pub const UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE: u32 = 128;
/// `Block.UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS`。
pub const UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS: u32 = 256;
/// `Block.UPDATE_SKIP_ON_PLACE`。
pub const UPDATE_SKIP_ON_PLACE: u32 = 512;
/// `Block.UPDATE_NONE`。
pub const UPDATE_NONE: u32 = UPDATE_INVISIBLE | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS;
/// `Block.UPDATE_ALL`。
pub const UPDATE_ALL: u32 = UPDATE_NEIGHBORS | UPDATE_CLIENTS;
/// `Block.UPDATE_ALL_IMMEDIATE`。
pub const UPDATE_ALL_IMMEDIATE: u32 = UPDATE_ALL | UPDATE_IMMEDIATE;
/// `Block.UPDATE_SKIP_ALL_SIDEEFFECTS`。
pub const UPDATE_SKIP_ALL_SIDEEFFECTS: u32 = UPDATE_KNOWN_SHAPE
    | UPDATE_SUPPRESS_DROPS
    | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS
    | UPDATE_SKIP_ON_PLACE;
/// `Block.UPDATE_LIMIT`：形状更新最多连锁这么多层。
pub const UPDATE_LIMIT: u32 = 512;

/// `BlockBehaviour.UPDATE_SHAPE_ORDER`。
const UPDATE_SHAPE_ORDER: [Direction; 6] = [
    Direction::West,
    Direction::East,
    Direction::North,
    Direction::South,
    Direction::Down,
    Direction::Up,
];
/// `NeighborUpdater.UPDATE_ORDER`，和形状更新的顺序不同。
const NEIGHBOR_UPDATE_ORDER: [Direction; 6] = [
    Direction::West,
    Direction::East,
    Direction::Down,
    Direction::Up,
    Direction::North,
    Direction::South,
];

/// `server.properties` 的 `max-chained-neighbor-updates` 默认值。
pub const MAX_CHAINED_NEIGHBOR_UPDATES: u32 = 1_000_000;
/// `Level.isInWorldBoundsHorizontal` 的边界。
const WORLD_BOUND: i32 = 30_000_000;
/// `ChunkPos.isValid` 的边界，单位是区块。
const VALID_CHUNK_BOUND: i32 = 2_097_061;
/// 默认世界边界的半径。这个世界不能改边界。
const WORLD_BORDER_RADIUS: f64 = 29_999_984.0;
/// 这个世界没有光照引擎，一律按开阔的白天算。
const FIXED_BRIGHTNESS: i32 = 15;

/// 流体种类，只用来排流体的计划刻。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fluid {
    Water,
    Lava,
}

/// `CollectingNeighborUpdater` 的一项更新。
#[derive(Debug, Clone, Copy)]
enum NeighborUpdate {
    /// `ShapeUpdate`。
    Shape {
        direction: Direction,
        neighbor_state: BlockState,
        pos: BlockPos,
        neighbor_pos: BlockPos,
        flags: u32,
        limit: u32,
    },
    /// `SimpleNeighborUpdate`：执行时才读这一格的状态。
    Simple {
        pos: BlockPos,
        block: &'static Block,
    },
    /// `FullNeighborUpdate`：状态在排队时就定了。
    Full {
        state: BlockState,
        pos: BlockPos,
        block: &'static Block,
    },
    /// `MultiNeighborUpdate`：六个邻居按顺序更新，中途可以被更深的一层打断。
    Multi {
        source_pos: BlockPos,
        source_block: &'static Block,
        skip: Option<Direction>,
        next: usize,
    },
}

/// 下一步要执行的一次更新。
#[derive(Debug, Clone, Copy)]
enum UpdateStep {
    Shape {
        direction: Direction,
        neighbor_state: BlockState,
        pos: BlockPos,
        neighbor_pos: BlockPos,
        flags: u32,
        limit: u32,
    },
    /// `state` 是 `None` 时执行的时候才读。
    Neighbor {
        state: Option<BlockState>,
        pos: BlockPos,
        block: &'static Block,
    },
}

impl NeighborUpdate {
    /// `runNext` 里除了执行以外的部分：取出下一步，返回值的第二项是这项更新还有没有下一步。
    fn advance(&mut self) -> (UpdateStep, bool) {
        match *self {
            Self::Shape {
                direction,
                neighbor_state,
                pos,
                neighbor_pos,
                flags,
                limit,
            } => {
                let step = UpdateStep::Shape {
                    direction,
                    neighbor_state,
                    pos,
                    neighbor_pos,
                    flags,
                    limit,
                };
                (step, false)
            }
            Self::Simple { pos, block } => (
                UpdateStep::Neighbor {
                    state: None,
                    pos,
                    block,
                },
                false,
            ),
            Self::Full { state, pos, block } => (
                UpdateStep::Neighbor {
                    state: Some(state),
                    pos,
                    block,
                },
                false,
            ),
            Self::Multi {
                source_pos,
                source_block,
                skip,
                ref mut next,
            } => {
                let direction = NEIGHBOR_UPDATE_ORDER[*next];
                *next = skip_direction(*next + 1, skip);
                let step = UpdateStep::Neighbor {
                    state: None,
                    pos: source_pos.relative(direction),
                    block: source_block,
                };
                (step, *next < NEIGHBOR_UPDATE_ORDER.len())
            }
        }
    }
}

/// 跳过官方要跳过的那个方向。
fn skip_direction(index: usize, skip: Option<Direction>) -> usize {
    if NEIGHBOR_UPDATE_ORDER.get(index).copied() == skip && skip.is_some() {
        index + 1
    } else {
        index
    }
}

/// `CollectingNeighborUpdater` 的队列。
///
/// 更新分层执行：一次更新引起的新更新先收集在 `added_this_layer`，这一步做完后倒着压进栈，
/// 所以新更新比同一层后面的更新先执行，新更新之间保持加入的顺序。
#[derive(Debug, Default)]
struct NeighborQueue {
    stack: Vec<NeighborUpdate>,
    added_this_layer: Vec<NeighborUpdate>,
    count: u32,
}

/// 一次操作期间的世界。
pub struct World<'a> {
    level: &'a mut dyn Level,
    ticks: &'a mut BlockTicks,
    updates: NeighborQueue,
    effects: Vec<Effect>,
}

impl<'a> World<'a> {
    pub fn new(level: &'a mut dyn Level, ticks: &'a mut BlockTicks) -> Self {
        Self {
            level,
            ticks,
            updates: NeighborQueue::default(),
            effects: Vec::new(),
        }
    }

    /// 这次操作收集到的结果。
    pub(crate) fn into_effects(self) -> Vec<Effect> {
        self.effects
    }

    pub(crate) fn ticks_mut(&mut self) -> &mut BlockTicks {
        self.ticks
    }

    /// 记下一个发给玩家的结果，官方的 `levelEvent`。
    pub fn push_effect(&mut self, effect: Effect) {
        self.effects.push(effect);
    }

    /// `getBlockState`。世界外是 `void_air`。
    pub fn block_state(&self, pos: BlockPos) -> BlockState {
        self.level.block_state(pos)
    }

    /// `getFluidState`。
    pub fn fluid_state(&self, pos: BlockPos) -> FluidState {
        self.block_state(pos).fluid()
    }

    /// `isEmptyBlock`。
    pub fn is_empty_block(&self, pos: BlockPos) -> bool {
        self.block_state(pos).is_air()
    }

    /// `isWaterAt`。
    pub fn is_water_at(&self, pos: BlockPos) -> bool {
        self.fluid_state(pos).is_water()
    }

    /// `getMinY`。
    pub fn min_y(&self) -> i32 {
        self.level.min_y()
    }

    /// `getMaxY`。
    pub fn max_y(&self) -> i32 {
        self.level.max_y()
    }

    /// `isInsideBuildHeight`。
    pub fn is_inside_build_height(&self, y: i32) -> bool {
        (self.min_y()..=self.max_y()).contains(&y)
    }

    /// `isInWorldBounds`。
    pub fn is_in_world_bounds(&self, pos: BlockPos) -> bool {
        self.is_inside_build_height(pos.y)
            && (-WORLD_BOUND..WORLD_BOUND).contains(&pos.x)
            && (-WORLD_BOUND..WORLD_BOUND).contains(&pos.z)
    }

    /// `isInValidBounds`：`setBlock` 用的范围。
    fn is_in_valid_bounds(&self, pos: BlockPos) -> bool {
        self.is_inside_build_height(pos.y)
            && (pos.x >> 4).abs() <= VALID_CHUNK_BOUND
            && (pos.z >> 4).abs() <= VALID_CHUNK_BOUND
    }

    /// `getWorldBorder().isWithinBounds(pos)`。
    pub fn is_within_world_border(&self, pos: BlockPos) -> bool {
        let inside =
            |value: i32| (-WORLD_BORDER_RADIUS..WORLD_BORDER_RADIUS).contains(&f64::from(value));
        inside(pos.x) && inside(pos.z)
    }

    /// `isFaceSturdy`：世界里这一格的这个面能不能支撑东西。
    ///
    /// 大部分方块的结果在 `STATE_FACTS` 里。竹子这类方块的形状跟着渲染偏移走，要按位置算。
    pub fn is_face_sturdy(&self, pos: BlockPos, face: Direction, support: SupportType) -> bool {
        let state = self.block_state(pos);
        if state
            .block()
            .offset
            .is_some_and(|offset| offset.moves_shape)
        {
            return shape::is_supporting(&shape::support_boxes(state, pos), face, support);
        }
        state.is_face_sturdy(face, support)
    }

    /// `getRawBrightness`。没有光照引擎，固定是 15，这是已知偏差。
    pub const fn raw_brightness(&self, _pos: BlockPos) -> i32 {
        FIXED_BRIGHTNESS
    }

    /// `hasNeighborSignal`。世界里没有红石信号。写成函数是为了以后有红石时只改一处。
    pub const fn has_neighbor_signal(&self, _pos: BlockPos) -> bool {
        false
    }

    /// `getSignal`。世界里没有红石信号。
    pub const fn signal(&self, _pos: BlockPos, _direction: Direction) -> i32 {
        0
    }

    /// `getBestNeighborSignal`。世界里没有红石信号。
    pub const fn best_neighbor_signal(&self, _pos: BlockPos) -> i32 {
        0
    }

    /// `getGameTime`。
    pub const fn game_time(&self) -> i64 {
        self.ticks.now()
    }

    /// `isUnobstructed(state, pos, context)`：放下这个状态不会卡住实体。
    pub fn is_unobstructed(
        &self,
        state: BlockState,
        pos: BlockPos,
        context: CollisionContext,
    ) -> bool {
        let boxes = state.collision_boxes(self, pos, context);
        if boxes.is_empty() {
            return true;
        }
        let world_boxes: Vec<_> = boxes.iter().map(|bounds| bounds.at(pos)).collect();
        self.level.is_unobstructed(&world_boxes)
    }

    /// `scheduleTick(pos, block, delay)`。
    pub fn schedule_tick(&mut self, pos: BlockPos, block: &'static Block, delay: i32) {
        self.schedule_tick_with_priority(pos, block, delay, TickPriority::Normal);
    }

    /// `scheduleTick(pos, block, delay, priority)`。
    pub fn schedule_tick_with_priority(
        &mut self,
        pos: BlockPos,
        block: &'static Block,
        delay: i32,
        priority: TickPriority,
    ) {
        let id = BlockState(block.default_state).block_id();
        self.ticks.schedule(pos, id, delay, priority);
    }

    /// `getBlockTicks().hasScheduledTick`。
    pub fn has_scheduled_tick(&self, pos: BlockPos, block: &Block) -> bool {
        let id = BlockState(block.default_state).block_id();
        self.ticks.has_scheduled_tick(pos, id)
    }

    /// `getBlockTicks().willTickThisTick`。
    pub fn will_tick_this_tick(&self, pos: BlockPos, block: &Block) -> bool {
        let id = BlockState(block.default_state).block_id();
        self.ticks.will_tick_this_tick(pos, id)
    }

    /// `scheduleTick(pos, fluid, delay)`。流体不流动，这里什么也不做。
    /// 写成函数是为了以后有流体时只改一处。
    pub const fn schedule_fluid_tick(&mut self, _pos: BlockPos, _fluid: Fluid) {}

    /// `setBlockAndUpdate`：`setBlock(pos, state, UPDATE_ALL)`。
    pub fn set_block_and_update(&mut self, pos: BlockPos, state: BlockState) -> bool {
        self.set_block(pos, state, UPDATE_ALL)
    }

    /// `setBlock(pos, state, flags)`。
    pub fn set_block(&mut self, pos: BlockPos, state: BlockState, flags: u32) -> bool {
        self.set_block_limited(pos, state, flags, UPDATE_LIMIT)
    }

    /// `setBlock(pos, state, flags, updateLimit)`。
    pub fn set_block_limited(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
        limit: u32,
    ) -> bool {
        if !self.is_in_valid_bounds(pos) {
            return false;
        }
        let Some(old) = self.set_block_state(pos, state, flags) else {
            return false;
        };
        // `onPlace` 可能又改了这一格，这时官方跳过后面所有的连锁。
        if self.block_state(pos) == state {
            self.after_block_changed(pos, old, state, flags, limit);
        }
        true
    }

    /// `Level.setBlock` 在 `LevelChunk.setBlockState` 之后的部分。
    fn after_block_changed(
        &mut self,
        pos: BlockPos,
        old: BlockState,
        state: BlockState,
        flags: u32,
        limit: u32,
    ) {
        if flags & UPDATE_NEIGHBORS != 0 {
            self.update_neighbors_at(pos, old.block());
            if state.has_analog_output_signal() {
                self.update_neighbour_for_output_signal(pos, state.block());
            }
        }
        if flags & UPDATE_KNOWN_SHAPE == 0 && limit > 0 {
            let neighbour_flags = flags & !(UPDATE_NEIGHBORS | UPDATE_SUPPRESS_DROPS);
            old.update_indirect_neighbour_shapes(self, pos, neighbour_flags, limit - 1);
            self.update_neighbour_shapes(state, pos, neighbour_flags, limit - 1);
            state.update_indirect_neighbour_shapes(self, pos, neighbour_flags, limit - 1);
        }
    }

    /// `LevelChunk.setBlockState`：写入这一格。没有写入或者写入后被换掉时返回 `None`。
    ///
    /// 官方在区段全是空气、写入的也是空气时直接返回。那种情况只会把 `cave_air` 换成 `air`，
    /// 这个世界里没有 `cave_air`，所以这里不看区段。
    fn set_block_state(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
    ) -> Option<BlockState> {
        let old = self.block_state(pos);
        if old == state || !self.level.write_block(pos, state) {
            return None;
        }

        let block_changed = !old.is_same_block(state);
        let moved_by_piston = flags & UPDATE_MOVE_BY_PISTON != 0;
        let is_rail = state.is_a(BlockAncestor::BaseRailBlock);
        if (block_changed || is_rail) && (flags & UPDATE_NEIGHBORS != 0 || moved_by_piston) {
            old.affect_neighbors_after_removal(self, pos);
        }
        // 上面的连锁可能已经把这一格换成了别的方块。
        if !self.block_state(pos).is_same_block(state) {
            return None;
        }
        if flags & UPDATE_SKIP_ON_PLACE == 0 {
            state.on_place(self, pos, old);
        }
        Some(old)
    }

    /// `removeBlock(pos, false)`：换成这一格的流体，没有流体时是空气。
    pub fn remove_block(&mut self, pos: BlockPos) -> bool {
        let fluid = self.fluid_state(pos);
        self.set_block(pos, fluid.legacy_block(), UPDATE_ALL)
    }

    /// `destroyBlock(pos, drop, null, updateLimit)`。这个世界没有掉落物。
    pub fn destroy_block(&mut self, pos: BlockPos, limit: u32) -> bool {
        let state = self.block_state(pos);
        if state.is_air() {
            return false;
        }
        state.spawn_destroy_particles(self, pos);
        let fluid = self.fluid_state(pos);
        self.set_block_limited(pos, fluid.legacy_block(), UPDATE_ALL, limit)
    }

    /// `Block.updateOrDestroy`。
    pub fn update_or_destroy(
        &mut self,
        old: BlockState,
        new: BlockState,
        pos: BlockPos,
        flags: u32,
        limit: u32,
    ) {
        if new == old {
            return;
        }
        if new.is_air() {
            self.destroy_block(pos, limit);
        } else {
            self.set_block_limited(pos, new, flags & !UPDATE_SUPPRESS_DROPS, limit);
        }
    }

    /// `Block.updateFromNeighbourShapes`：按六个邻居重新算一遍状态，不写入世界。
    pub fn update_from_neighbour_shapes(&mut self, state: BlockState, pos: BlockPos) -> BlockState {
        let mut updated = state;
        for direction in UPDATE_SHAPE_ORDER {
            let neighbor_pos = pos.relative(direction);
            let neighbor = self.block_state(neighbor_pos);
            updated = updated.update_shape(self, pos, direction, neighbor_pos, neighbor);
        }
        updated
    }

    /// `BlockState.updateNeighbourShapes`：通知六个邻居这一格变了。
    pub fn update_neighbour_shapes(
        &mut self,
        state: BlockState,
        pos: BlockPos,
        flags: u32,
        limit: u32,
    ) {
        for direction in UPDATE_SHAPE_ORDER {
            self.neighbor_shape_changed(
                direction.opposite(),
                pos.relative(direction),
                pos,
                state,
                flags,
                limit,
            );
        }
    }

    /// `neighborShapeChanged`：`pos` 这一格朝 `direction` 的邻居变成了 `neighbor_state`。
    pub fn neighbor_shape_changed(
        &mut self,
        direction: Direction,
        pos: BlockPos,
        neighbor_pos: BlockPos,
        neighbor_state: BlockState,
        flags: u32,
        limit: u32,
    ) {
        self.add_and_run(
            pos,
            NeighborUpdate::Shape {
                direction,
                neighbor_state,
                pos,
                neighbor_pos,
                flags,
                limit,
            },
        );
    }

    /// `updateNeighborsAt(pos, block)`。
    pub fn update_neighbors_at(&mut self, pos: BlockPos, block: &'static Block) {
        self.push_multi_update(pos, block, None);
    }

    /// `updateNeighborsAtExceptFromFacing`。
    pub fn update_neighbors_at_except_from_facing(
        &mut self,
        pos: BlockPos,
        block: &'static Block,
        skip: Direction,
    ) {
        self.push_multi_update(pos, block, Some(skip));
    }

    fn push_multi_update(
        &mut self,
        source_pos: BlockPos,
        source_block: &'static Block,
        skip: Option<Direction>,
    ) {
        self.add_and_run(
            source_pos,
            NeighborUpdate::Multi {
                source_pos,
                source_block,
                skip,
                next: skip_direction(0, skip),
            },
        );
    }

    /// `neighborChanged(pos, block, orientation)`：执行时才读 `pos` 的状态。
    pub fn neighbor_changed(&mut self, pos: BlockPos, block: &'static Block) {
        self.add_and_run(pos, NeighborUpdate::Simple { pos, block });
    }

    /// `neighborChanged(state, pos, block, orientation, movedByPiston)`。
    pub fn neighbor_changed_with_state(
        &mut self,
        state: BlockState,
        pos: BlockPos,
        block: &'static Block,
    ) {
        self.add_and_run(pos, NeighborUpdate::Full { state, pos, block });
    }

    /// `updateNeighbourForOutputSignal`：告诉旁边和隔一格的比较器这里的信号变了。
    pub fn update_neighbour_for_output_signal(&mut self, pos: BlockPos, block: &'static Block) {
        for direction in Direction::HORIZONTAL {
            let relative = pos.relative(direction);
            let state = self.block_state(relative);
            if state.is(blocks::COMPARATOR) {
                self.neighbor_changed_with_state(state, relative, block);
            } else if state.is_redstone_conductor() {
                let beyond = relative.relative(direction);
                let state = self.block_state(beyond);
                if state.is(blocks::COMPARATOR) {
                    self.neighbor_changed_with_state(state, beyond, block);
                }
            }
        }
    }

    /// `CollectingNeighborUpdater.addAndRun`。
    fn add_and_run(&mut self, pos: BlockPos, update: NeighborUpdate) {
        let running_already = self.updates.count > 0;
        let too_many = self.updates.count >= MAX_CHAINED_NEIGHBOR_UPDATES;
        self.updates.count += 1;
        if !too_many {
            if running_already {
                self.updates.added_this_layer.push(update);
            } else {
                self.updates.stack.push(update);
            }
        } else if self.updates.count - 1 == MAX_CHAINED_NEIGHBOR_UPDATES {
            error!(
                "Too many chained neighbor updates. Skipping the rest. First skipped position: {} {} {}",
                pos.x, pos.y, pos.z
            );
        }
        if !running_already {
            self.run_updates();
        }
    }

    /// `CollectingNeighborUpdater.runUpdates`。
    fn run_updates(&mut self) {
        while !self.updates.stack.is_empty() || !self.updates.added_this_layer.is_empty() {
            let added = std::mem::take(&mut self.updates.added_this_layer);
            self.updates.stack.extend(added.into_iter().rev());
            self.run_top_update();
        }
        self.updates = NeighborQueue::default();
    }

    /// 执行栈顶的更新，直到它做完，或者它引起了新的更新。
    fn run_top_update(&mut self) {
        while self.updates.added_this_layer.is_empty() {
            let top = self
                .updates
                .stack
                .last_mut()
                .expect("外层循环保证了栈不为空");
            let (step, more) = top.advance();
            self.run_step(step);
            if !more {
                self.updates.stack.pop();
                break;
            }
        }
    }

    fn run_step(&mut self, step: UpdateStep) {
        match step {
            UpdateStep::Shape {
                direction,
                neighbor_state,
                pos,
                neighbor_pos,
                flags,
                limit,
            } => self.execute_shape_update(
                direction,
                pos,
                neighbor_pos,
                neighbor_state,
                flags,
                limit,
            ),
            UpdateStep::Neighbor { state, pos, block } => {
                let state = state.unwrap_or_else(|| self.block_state(pos));
                state.neighbor_changed(self, pos, block);
            }
        }
    }

    /// `NeighborUpdater.executeShapeUpdate`。
    fn execute_shape_update(
        &mut self,
        direction: Direction,
        pos: BlockPos,
        neighbor_pos: BlockPos,
        neighbor_state: BlockState,
        flags: u32,
        limit: u32,
    ) {
        let current = self.block_state(pos);
        if flags & UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE != 0 && current.is(blocks::REDSTONE_WIRE) {
            return;
        }
        let updated = current.update_shape(self, pos, direction, neighbor_pos, neighbor_state);
        self.update_or_destroy(current, updated, pos, flags, limit);
    }

    /// `BlockGetter.clip`：从 `from` 到 `to` 的视线碰到的第一个方块或者水源。
    ///
    /// 上下文是 `ClipContext.Block.OUTLINE` 和 `ClipContext.Fluid.SOURCE_ONLY`，
    /// `PlaceOnWaterBlockItem` 只用这一种。没有用 `getInteractionShape`，
    /// 它只影响点中炼药锅和漏斗内壁时报告的面，结果都是放不下。
    pub(crate) fn clip_source_fluids(&self, from: Vec3, to: Vec3) -> BlockHit {
        traverse_blocks(from, to, |pos| self.clip_one_block(pos, from, to))
            .unwrap_or_else(|| miss(from, to))
    }

    fn clip_one_block(&self, pos: BlockPos, from: Vec3, to: Vec3) -> Option<BlockHit> {
        let state = self.block_state(pos);
        let block_hit = shape::clip(&shape::outline_boxes(state, pos), from, to, pos);
        let fluid_hit = self.clip_fluid(state.fluid(), pos, from, to);
        let distance =
            |hit: &Option<BlockHit>| hit.map_or(f64::MAX, |hit| from.distance_to_sqr(hit.location));
        if distance(&block_hit) <= distance(&fluid_hit) {
            block_hit
        } else {
            fluid_hit
        }
    }

    fn clip_fluid(
        &self,
        fluid: FluidState,
        pos: BlockPos,
        from: Vec3,
        to: Vec3,
    ) -> Option<BlockHit> {
        if !fluid.is_source() {
            return None;
        }
        let above = self.fluid_state(pos.above());
        let same_above = fluid.is_water() == above.is_water() && fluid.is_lava() == above.is_lava();
        let height = if same_above { 1.0 } else { fluid.own_height() };
        let bounds = shape::Aabb::new(0.0, 0.0, 0.0, 1.0, f64::from(height), 1.0);
        shape::clip(&[bounds], from, to, pos)
    }
}

/// `BlockGetter.traverseBlocks` 的未命中结果。
fn miss(from: Vec3, to: Vec3) -> BlockHit {
    let delta = from.subtract(to);
    BlockHit {
        pos: BlockPos::containing(to),
        face: Direction::nearest_to(delta.x, delta.y, delta.z),
        location: to,
        inside: false,
    }
}

/// `BlockGetter.traverseBlocks`：沿着线段依次访问经过的方块，`visit` 返回结果时停下。
fn traverse_blocks<T>(
    from: Vec3,
    to: Vec3,
    mut visit: impl FnMut(BlockPos) -> Option<T>,
) -> Option<T> {
    use mc_bamboo_core::mth::{floor, frac, lerp, sign};

    if from == to {
        return None;
    }
    // 把线段两头各拉长一点点，线段刚好停在方块表面时也能算穿过。
    const EXTEND: f64 = -1.0e-7;
    let end = Vec3::new(
        lerp(EXTEND, to.x, from.x),
        lerp(EXTEND, to.y, from.y),
        lerp(EXTEND, to.z, from.z),
    );
    let start = Vec3::new(
        lerp(EXTEND, from.x, to.x),
        lerp(EXTEND, from.y, to.y),
        lerp(EXTEND, from.z, to.z),
    );
    let mut pos = BlockPos::new(floor(start.x), floor(start.y), floor(start.z));
    if let Some(result) = visit(pos) {
        return Some(result);
    }

    let delta = end.subtract(start);
    let (sign_x, sign_y, sign_z) = (sign(delta.x), sign(delta.y), sign(delta.z));
    let step_of = |axis_sign: i32, axis_delta: f64| {
        if axis_sign == 0 {
            f64::MAX
        } else {
            f64::from(axis_sign) / axis_delta
        }
    };
    let (t_delta_x, t_delta_y, t_delta_z) = (
        step_of(sign_x, delta.x),
        step_of(sign_y, delta.y),
        step_of(sign_z, delta.z),
    );
    let first_t = |axis_sign: i32, t_delta: f64, value: f64| {
        t_delta
            * if axis_sign > 0 {
                1.0 - frac(value)
            } else {
                frac(value)
            }
    };
    let mut t_x = first_t(sign_x, t_delta_x, start.x);
    let mut t_y = first_t(sign_y, t_delta_y, start.y);
    let mut t_z = first_t(sign_z, t_delta_z, start.z);

    while t_x <= 1.0 || t_y <= 1.0 || t_z <= 1.0 {
        if t_x < t_y {
            if t_x < t_z {
                pos.x += sign_x;
                t_x += t_delta_x;
            } else {
                pos.z += sign_z;
                t_z += t_delta_z;
            }
        } else if t_y < t_z {
            pos.y += sign_y;
            t_y += t_delta_y;
        } else {
            pos.z += sign_z;
            t_z += t_delta_z;
        }
        if let Some(result) = visit(pos) {
            return Some(result);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::items;

    use super::*;
    use crate::{
        state::HORIZONTAL_FACING,
        testing::{TestLevel, break_block},
    };

    const CENTER: BlockPos = BlockPos::new(0, 64, 0);

    fn wall_torch_on(side: Direction) -> BlockState {
        BlockState(blocks::WALL_TORCH).with(HORIZONTAL_FACING, side)
    }

    fn with_world<T>(level: &mut TestLevel, run: impl FnOnce(&mut World) -> T) -> T {
        let mut ticks = BlockTicks::new();
        let mut world = World::new(level, &mut ticks);
        run(&mut world)
    }

    fn destroyed_positions(effects: &[Effect]) -> Vec<BlockPos> {
        effects
            .iter()
            .map(|&Effect::Destroyed { pos, .. }| pos)
            .collect()
    }

    /// 六个邻居按 `UPDATE_SHAPE_ORDER` 收到形状更新，破坏的先后就是这个顺序。
    #[test]
    fn shape_updates_follow_the_official_order() {
        let mut level = TestLevel::new();
        level.set(CENTER, BlockState(blocks::STONE));
        for side in Direction::HORIZONTAL {
            level.set(CENTER.relative(side), wall_torch_on(side));
        }
        level.set(CENTER.above(), BlockState(blocks::TORCH));

        let outcome = break_block(&mut level, CENTER);
        assert_eq!(
            destroyed_positions(&outcome.effects),
            [
                CENTER,
                CENTER.west(),
                CENTER.east(),
                CENTER.north(),
                CENTER.south(),
                CENTER.above(),
            ]
        );
    }

    #[test]
    fn multi_update_visits_neighbours_in_the_official_order() {
        let block = BlockState(blocks::STONE).block();
        let mut update = NeighborUpdate::Multi {
            source_pos: CENTER,
            source_block: block,
            skip: None,
            next: skip_direction(0, None),
        };
        let mut visited = Vec::new();
        loop {
            let (step, more) = update.advance();
            let UpdateStep::Neighbor { pos, .. } = step else {
                panic!("这种更新只有邻居更新");
            };
            visited.push(pos);
            if !more {
                break;
            }
        }
        assert_eq!(
            visited,
            [
                CENTER.west(),
                CENTER.east(),
                CENTER.below(),
                CENTER.above(),
                CENTER.north(),
                CENTER.south(),
            ]
        );
    }

    #[test]
    fn multi_update_skips_the_given_side() {
        for skip in NEIGHBOR_UPDATE_ORDER {
            let mut update = NeighborUpdate::Multi {
                source_pos: CENTER,
                source_block: BlockState(blocks::STONE).block(),
                skip: Some(skip),
                next: skip_direction(0, Some(skip)),
            };
            let mut count = 0;
            loop {
                let (step, more) = update.advance();
                let UpdateStep::Neighbor { pos, .. } = step else {
                    panic!("这种更新只有邻居更新");
                };
                assert_ne!(pos, CENTER.relative(skip));
                count += 1;
                if !more {
                    break;
                }
            }
            assert_eq!(count, 5);
        }
    }

    #[test]
    fn setting_the_same_state_changes_nothing() {
        let mut level = TestLevel::new();
        level.set(CENTER, BlockState(blocks::STONE));
        let changed = with_world(&mut level, |world| {
            world.set_block_and_update(CENTER, BlockState(blocks::STONE))
        });
        assert!(!changed);
        assert!(level.writes.is_empty());
    }

    #[test]
    fn writes_outside_the_world_are_refused() {
        let mut level = TestLevel::new();
        let outside = BlockPos::new(0, 400, 0);
        let changed = with_world(&mut level, |world| {
            world.set_block_and_update(outside, BlockState(blocks::STONE))
        });
        assert!(!changed);
        assert!(level.writes.is_empty());
    }

    #[test]
    fn known_shape_skips_the_neighbour_shapes() {
        let mut level = TestLevel::new();
        level.set(CENTER, BlockState(blocks::STONE));
        level.set(CENTER.above(), BlockState(blocks::TORCH));
        with_world(&mut level, |world| {
            world.set_block(CENTER, BlockState::AIR, UPDATE_ALL | UPDATE_KNOWN_SHAPE);
        });
        assert!(level.get(CENTER.above()).is(blocks::TORCH));

        level.set(CENTER, BlockState(blocks::STONE));
        with_world(&mut level, |world| {
            world.set_block(CENTER, BlockState::AIR, UPDATE_ALL);
        });
        assert!(level.get(CENTER.above()).is_air());
    }

    #[test]
    fn limit_zero_stops_the_shape_chain() {
        let mut level = TestLevel::new();
        level.set(CENTER, BlockState(blocks::STONE));
        level.set(CENTER.above(), BlockState(blocks::TORCH));
        with_world(&mut level, |world| {
            world.set_block_limited(CENTER, BlockState::AIR, UPDATE_ALL, 0);
        });
        assert!(level.get(CENTER.above()).is(blocks::TORCH));
    }

    #[test]
    fn update_or_destroy_keeps_or_breaks_the_block() {
        let mut level = TestLevel::new();
        let stone = BlockState(blocks::STONE);
        level.set(CENTER, stone);
        with_world(&mut level, |world| {
            world.update_or_destroy(stone, stone, CENTER, UPDATE_ALL, UPDATE_LIMIT);
        });
        assert!(level.writes.is_empty());

        let effects = {
            let mut ticks = BlockTicks::new();
            let mut world = World::new(&mut level, &mut ticks);
            world.update_or_destroy(stone, BlockState::AIR, CENTER, UPDATE_ALL, UPDATE_LIMIT);
            world.into_effects()
        };
        assert!(level.get(CENTER).is_air());
        assert_eq!(
            effects,
            [Effect::Destroyed {
                pos: CENTER,
                state: stone,
                except_initiator: false,
            }]
        );
    }

    #[test]
    fn destroying_air_does_nothing() {
        let mut level = TestLevel::new();
        let destroyed = with_world(&mut level, |world| {
            world.destroy_block(CENTER, UPDATE_LIMIT)
        });
        assert!(!destroyed);
    }

    #[test]
    fn neighbour_shapes_can_be_recomputed_without_writing() {
        let mut level = TestLevel::new();
        let torch = BlockState(blocks::TORCH);
        let updated = with_world(&mut level, |world| {
            world.update_from_neighbour_shapes(torch, CENTER)
        });
        // 下面没有东西，火把算出来是空气，世界不变。
        assert!(updated.is_air());
        assert!(level.writes.is_empty());
    }

    #[test]
    fn world_helpers_follow_the_level() {
        let mut level = TestLevel::new();
        level.set(CENTER, BlockState::WATER);
        with_world(&mut level, |world| {
            assert!(world.is_water_at(CENTER));
            assert!(world.is_empty_block(CENTER.above()));
            assert!(world.is_inside_build_height(CENTER.y));
            assert!(!world.is_inside_build_height(world.max_y() + 1));
            assert!(world.is_in_world_bounds(CENTER));
            assert!(!world.is_in_world_bounds(BlockPos::new(30_000_000, 64, 0)));
            assert!(world.is_within_world_border(CENTER));
            assert!(!world.is_within_world_border(BlockPos::new(29_999_984, 64, 0)));
            assert_eq!(world.raw_brightness(CENTER), 15);
            assert!(!world.has_neighbor_signal(CENTER));
            assert_eq!(world.signal(CENTER, Direction::Up), 0);
        });
    }

    #[test]
    fn scheduled_ticks_land_in_the_shared_queue() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let stone = BlockState(blocks::STONE).block();
        let mut world = World::new(&mut level, &mut ticks);
        world.schedule_tick(CENTER, stone, 3);
        assert!(world.has_scheduled_tick(CENTER, stone));
        drop(world);
        let queued: Vec<_> = ticks.pending().collect();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].trigger_tick, 3);
    }

    #[test]
    fn clip_stops_at_the_first_block() {
        let mut level = TestLevel::new();
        let wall = BlockPos::new(3, 64, 0);
        level.set(wall, BlockState(blocks::STONE));
        level.set(wall.east(), BlockState(blocks::STONE));
        with_world(&mut level, |world| {
            let hit =
                world.clip_source_fluids(Vec3::new(0.5, 64.5, 0.5), Vec3::new(9.5, 64.5, 0.5));
            assert_eq!(hit.pos, wall);
            assert_eq!(hit.face, Direction::West);
        });
    }

    #[test]
    fn clip_sees_water_sources_but_not_flowing_water() {
        let mut level = TestLevel::new();
        let flowing = BlockPos::new(0, 62, 0);
        let source = BlockPos::new(0, 60, 0);
        level.set(flowing, BlockState::WATER.with(crate::state::LEVEL, 2));
        level.set(source, BlockState::WATER);
        with_world(&mut level, |world| {
            let hit =
                world.clip_source_fluids(Vec3::new(0.5, 64.5, 0.5), Vec3::new(0.5, 55.5, 0.5));
            assert_eq!(hit.pos, source);
            assert_eq!(hit.face, Direction::Up);
        });
    }

    #[test]
    fn clip_miss_points_back_at_the_viewer() {
        let mut level = TestLevel::new();
        with_world(&mut level, |world| {
            let to = Vec3::new(0.5, 60.5, 0.5);
            let hit = world.clip_source_fluids(Vec3::new(0.5, 64.5, 0.5), to);
            assert_eq!(hit.pos, BlockPos::new(0, 60, 0));
            assert_eq!(hit.face, Direction::Up);
            assert_eq!(hit.location, to);
        });
    }

    #[test]
    fn lily_pad_item_exists_for_the_clip_path() {
        // `PlaceOnWaterBlockItem` 只有这两种，新版本多了要回来看一眼 `use_in_air`。
        let kinds = items::ITEMS
            .iter()
            .filter(|item| item.kind == mc_bamboo_data::ItemKind::PlaceOnWater)
            .count();
        assert_eq!(kinds, 2);
    }
}
