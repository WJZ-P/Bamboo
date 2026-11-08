//! 官方 `Level` 里和方块有关的部分：`setBlock`、`destroyBlock`、邻居更新、计划刻。
//!
//! `Behaviour` 的方法都通过 [`World`] 读写世界。方法名和官方相同，要对照时直接搜官方的名字。

use mc_bamboo_core::{BlockPos, ChunkPos, Direction, LightLayer, Vec3, sign::SignContent};
use mc_bamboo_data::{Block, BlockAncestor, blocks};
use tracing::{error, warn};

use crate::{
    behaviour::machines::{ShulkerBoxEntity, open_support_shape},
    block_entity::{self, BlockEntity, BlockEvent},
    context::BlockHit,
    fluid::Fluid,
    level::{Effect, Level, RedstoneMemory},
    random::LegacyRandom,
    shape::{self, CollisionContext},
    state::{BlockState, FACING, FluidState, POWER, SupportType},
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
/// `Difficulty.NORMAL.getId()`。
const NORMAL_DIFFICULTY: i32 = 2;
/// 游戏规则 `fire_spread_radius_around_player` 的默认值。
const FIRE_SPREAD_RADIUS_AROUND_PLAYER: i32 = 128;
/// `getSkyDarken`。世界时钟停在正午，天空一点也不暗。
const SKY_DARKEN: i32 = 0;

/// `level.getWorldBorder().isWithinBounds(pos)`：这个世界不能改边界，半径是默认的 29999984。
///
/// 方块坐标转成 `double` 再比，左边含、右边不含。
pub fn is_within_world_border(pos: BlockPos) -> bool {
    let inside =
        |value: i32| (-WORLD_BORDER_RADIUS..WORLD_BORDER_RADIUS).contains(&f64::from(value));
    inside(pos.x) && inside(pos.z)
}

/// `ClipContext.Fluid` 里用得到的两种：射线穿过所有流体，或者停在源头上。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipFluid {
    None,
    SourceOnly,
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
    /// `BlockItem.place` 刚放下的那一格，官方在这里读物品上的组件，见 [`World::placed_at`]。
    placed_at: Option<BlockPos>,
    /// 这次右键要换给玩家的物品。方块逻辑只记账，游戏层再改物品栏。
    ///
    /// The item this click hands back. Block behaviour only records it; the game layer edits
    /// the inventory.
    item_exchange: Option<crate::item_exchange::ItemExchange>,
    /// `RedstoneWireBlock.shouldSignal`，见 [`World::wire_block_signal`]。
    wire_should_signal: bool,
    /// `ServerLevel.isHandlingTick`：这次操作在世界刻里。
    handling_tick: bool,
}

impl<'a> World<'a> {
    pub fn new(level: &'a mut dyn Level, ticks: &'a mut BlockTicks) -> Self {
        Self {
            level,
            ticks,
            updates: NeighborQueue::default(),
            effects: Vec::new(),
            placed_at: None,
            item_exchange: None,
            wire_should_signal: true,
            handling_tick: false,
        }
    }

    /// 世界刻里的一次操作，`ServerLevel.tick` 把自己的 `handlingTick` 设成真的时候。
    pub fn in_world_tick(level: &'a mut dyn Level, ticks: &'a mut BlockTicks) -> Self {
        Self {
            handling_tick: true,
            ..Self::new(level, ticks)
        }
    }

    /// `ServerLevel.isHandlingTick`。活塞收回时用它判断前方的移动方块要不要直接落下。
    pub const fn is_handling_tick(&self) -> bool {
        self.handling_tick
    }

    /// `ServerLevel.tick` 在方块事件之后、实体和方块实体之前把 `handlingTick` 放回假。
    pub(crate) fn end_handling_tick(&mut self) {
        self.handling_tick = false;
    }

    /// 这次操作收集到的结果。调用方发出去，见 `mc-bamboo-game` 的 `send_effects`。
    pub fn into_effects(self) -> Vec<Effect> {
        self.effects
    }

    /// 这次操作放下的那一格方块，官方 `BlockItem.place` 里 `updateBlockEntityComponents`
    /// 看的位置。组件在 `mc-bamboo-protocol`，这一层看不见，所以只报位置，由调用方接着做。
    pub const fn placed_at(&self) -> Option<BlockPos> {
        self.placed_at
    }

    /// `BlockItem.place` 放成功时记下这一格。
    pub(crate) fn set_placed_at(&mut self, pos: BlockPos) {
        self.placed_at = Some(pos);
    }

    /// 记下这次右键要换给玩家的物品。
    ///
    /// Record the item this click hands back to the player.
    pub(crate) fn set_item_exchange(&mut self, exchange: crate::item_exchange::ItemExchange) {
        self.item_exchange = Some(exchange);
    }

    /// 这次右键记下的物品交换。
    ///
    /// The item exchange recorded for this click.
    pub(crate) fn item_exchange(&self) -> Option<&crate::item_exchange::ItemExchange> {
        self.item_exchange.as_ref()
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
        is_within_world_border(pos)
    }

    /// `isFaceSturdy`：世界里这一格的这个面能不能支撑东西。
    ///
    /// 大部分方块的结果在 `STATE_FACTS` 里。竹子这类方块的形状跟着渲染偏移走，要按位置算；
    /// `moving_piston` 官方标了 `dynamicShape`，支撑形状来自方块实体的碰撞形状，也跟着动画走。
    pub fn is_face_sturdy(&self, pos: BlockPos, face: Direction, support: SupportType) -> bool {
        self.is_state_face_sturdy(self.block_state(pos), pos, face, support)
    }

    /// `state.isFaceSturdy(level, pos, face, support)`：按传进来的状态算，不重新读这一格。
    ///
    /// 形状更新拿到的邻居状态可能比世界里的新或者旧，官方用的是传进来的那个；但 `moving_piston`
    /// 的形状照样从这一格的方块实体读，所以要带上世界和位置，不能只查状态的表。
    pub fn is_state_face_sturdy(
        &self,
        state: BlockState,
        pos: BlockPos,
        face: Direction,
        support: SupportType,
    ) -> bool {
        if state.is(blocks::MOVING_PISTON) {
            let boxes = state.collision_boxes(self, pos, CollisionContext::Empty);
            return shape::is_supporting(&boxes, face, support);
        }
        // `ShulkerBoxBlock.getBlockSupportShape`：盖子打开时只剩背面那一层薄板。
        if state.is_a(BlockAncestor::ShulkerBoxBlock) {
            let open = self
                .block_entity::<ShulkerBoxEntity>(pos)
                .is_some_and(|entity| !entity.is_closed());
            if open {
                let boxes = [open_support_shape(state.get(FACING))];
                return shape::is_supporting(&boxes, face, support);
            }
            return shape::is_supporting(&shape::support_boxes(state, pos), face, support);
        }
        if state
            .block()
            .offset
            .is_some_and(|offset| offset.moves_shape)
        {
            return shape::is_supporting(&shape::support_boxes(state, pos), face, support);
        }
        state.is_face_sturdy(face, support)
    }

    /// `getBrightness(layer, pos)`：只看天空光或者只看方块光。
    pub fn brightness(&self, layer: LightLayer, pos: BlockPos) -> i32 {
        self.level.brightness(layer, pos)
    }

    /// `getBlockEntity(pos) instanceof SignBlockEntity` 之后读到的内容，不是告示牌时是 `None`。
    pub fn sign(&self, pos: BlockPos) -> Option<SignContent> {
        self.level.sign(pos)
    }

    /// 改写告示牌的内容。不是告示牌方块时什么也不做，返回 `false`。
    pub fn set_sign(&mut self, pos: BlockPos, content: SignContent) -> bool {
        self.level.set_sign(pos, content)
    }

    /// `getBlockEntity(pos) instanceof SkullBlockEntity` 时的 `getNoteBlockSound`。
    ///
    /// 音符盒的 `getCustomSoundId` 读上面那一格，见 [`crate::behaviour::machines`] 里的音符盒。
    pub fn note_block_sound(&self, pos: BlockPos) -> Option<String> {
        self.level.note_block_sound(pos)
    }

    /// `getBlockEntity(pos)` 之后按类型取。
    pub fn block_entity<T: BlockEntity>(&self, pos: BlockPos) -> Option<&T> {
        self.level.block_entities().get(pos)
    }

    /// 这一格有没有方块实体。
    pub fn has_block_entity(&self, pos: BlockPos) -> bool {
        self.level.block_entities().contains(pos)
    }

    /// 这一格的物品栏，见 [`Level::with_container`]。
    ///
    /// 堆肥桶没有方块实体，官方 `ComposterBlock.getContainer` 按等级现做一个容器，这里同样处理。
    pub(crate) fn with_container(
        &mut self,
        pos: BlockPos,
        f: &mut dyn FnMut(&mut dyn crate::container::Container),
    ) -> bool {
        if self
            .block_state(pos)
            .is_a(mc_bamboo_data::BlockAncestor::ComposterBlock)
        {
            return crate::composter::with_container(self, pos, f);
        }
        self.level.with_container(pos, f)
    }

    /// 比较器从这一格的容器读到的模拟信号，见 [`Level::container_signal`]。
    pub(crate) fn container_signal(&self, pos: BlockPos) -> i32 {
        self.level.container_signal(pos)
    }

    /// 这一格容器背后的方块位置，见 [`Level::container_positions`]。
    pub(crate) fn container_positions(&self, pos: BlockPos) -> Vec<BlockPos> {
        self.level.container_positions(pos)
    }

    /// 带着这一格的漏斗方块实体运行一段代码，见 [`Level::with_hopper`]。
    pub(crate) fn with_hopper(
        &mut self,
        pos: BlockPos,
        f: &mut dyn FnMut(&mut crate::hopper::HopperEntity),
    ) -> bool {
        self.level.with_hopper(pos, f)
    }

    /// `Level.setBlockEntity`：放进这一格并登记 ticker。状态不收方块实体时和官方一样什么也不做，
    /// 只打一条警告，说明有地方放错了。
    pub fn set_block_entity(&mut self, entity: Box<dyn BlockEntity>) {
        let pos = entity.pos();
        let state = self.block_state(pos);
        if !state.has_block_entity() {
            warn!(
                "Trying to set block entity at {} {} {}, but state {} does not allow it",
                pos.x,
                pos.y,
                pos.z,
                state.block().name
            );
            return;
        }
        self.level.block_entities_mut().insert(entity);
    }

    /// `Level.removeBlockEntity`。
    pub fn remove_block_entity(&mut self, pos: BlockPos) {
        self.level.block_entities_mut().remove(pos);
    }

    /// 带着这一格的方块实体运行一段代码。实体在运行期间不在表里，所以这段代码用
    /// `getBlockEntity(pos)` 找不到它自己，别的地方和官方相同。
    pub fn with_block_entity<T: BlockEntity, R>(
        &mut self,
        pos: BlockPos,
        f: impl FnOnce(&mut T, &mut World) -> R,
    ) -> Option<R> {
        let (id, mut entity) = self.level.block_entities_mut().take_at(pos)?;
        let result = entity
            .as_any_mut()
            .downcast_mut::<T>()
            .map(|typed| f(typed, self));
        self.level.block_entities_mut().put(pos, id, entity);
        result
    }

    /// `level.blockEvent(pos, block, paramA, paramB)`。`block` 是排进队列的方块。
    pub fn block_event(
        &mut self,
        pos: BlockPos,
        block: &'static Block,
        param_a: i32,
        param_b: i32,
    ) {
        let block = BlockState(block.default_state).block_id();
        self.level.block_events().push(BlockEvent {
            pos,
            block,
            param_a,
            param_b,
        });
    }

    /// `ServerLevel.runBlockEvents`：按排队的顺序一直处理到队列空，处理中新排进来的同一轮执行。
    ///
    /// 处理成功的事件记成 [`Effect::BlockEvent`]，由调用方发给附近的玩家。
    pub fn run_block_events(&mut self) {
        while let Some(event) = self.level.block_events().pop() {
            let state = self.block_state(event.pos);
            if state.block_id() != event.block {
                continue;
            }
            if state.trigger_event(self, event.pos, event.param_a, event.param_b) {
                self.push_effect(Effect::BlockEvent {
                    pos: event.pos,
                    block: event.block,
                    param_a: event.param_a,
                    param_b: event.param_b,
                });
            }
        }
    }

    /// `Level.tickBlockEntities`：按 ticker 列表的顺序运行每刻的方块实体。
    pub fn tick_block_entities(&mut self) {
        self.level.block_entities_mut().begin_tick();
        while let Some((pos, id)) = self.level.block_entities_mut().next_ticker() {
            let Some(mut entity) = self.level.block_entities_mut().take(pos, id) else {
                continue;
            };
            entity.tick(self);
            self.level.block_entities_mut().put(pos, id, entity);
        }
        self.level.block_entities_mut().end_tick();
    }

    /// `getMaxLocalRawBrightness`：带上天空变暗的原始亮度。时间停在正午，天总是晴的，天空不变暗。
    pub fn max_local_raw_brightness(&self, pos: BlockPos) -> i32 {
        self.brightness(LightLayer::Block, pos)
            .max(self.brightness(LightLayer::Sky, pos) - SKY_DARKEN)
    }

    /// `getBrightness(LightLayer.BLOCK, pos)`：方块光。
    pub fn block_light(&self, pos: BlockPos) -> i32 {
        self.brightness(LightLayer::Block, pos)
    }

    /// `isRaining`。这个世界永远是晴天。
    pub const fn is_raining(&self) -> bool {
        false
    }

    /// `isRainingAt`。这个世界永远是晴天。
    pub const fn is_raining_at(&self, _pos: BlockPos) -> bool {
        false
    }

    /// `getDifficulty().getId()`。这个世界没有难度设置，客户端收不到难度时显示普通，这里也按普通算。
    pub const fn difficulty_id(&self) -> i32 {
        NORMAL_DIFFICULTY
    }

    /// `getRandom()`，也就是 `Level.random`。
    pub fn random(&self) -> &LegacyRandom {
        self.level.random().source()
    }

    /// `getBlockRandomPos`。
    pub(crate) fn block_random_pos(&self, origin: BlockPos, y_mask: i32) -> BlockPos {
        self.level.random().block_random_pos(origin, y_mask)
    }

    /// `LevelChunkSection.isRandomlyTicking`。
    pub(crate) fn is_randomly_ticking_section(&self, chunk: ChunkPos, section_y: i32) -> bool {
        self.level.is_randomly_ticking_section(chunk, section_y)
    }

    /// `isLoaded`：所有区块都在内存里，只看高度和坐标的范围。
    pub fn is_loaded(&self, pos: BlockPos) -> bool {
        self.is_in_valid_bounds(pos)
    }

    /// `ServerLevel.canSpreadFireAround`：游戏规则 `fire_spread_radius_around_player` 是默认的 128，
    /// 离所有玩家都不到这么远时火才蔓延。
    pub fn can_spread_fire_around(&self, pos: BlockPos) -> bool {
        self.level
            .any_player_close_enough_to(pos, FIRE_SPREAD_RADIUS_AROUND_PLAYER)
    }

    /// `getRawBrightness(pos, 0)`：天空光和方块光里亮的那个，不算天色。
    pub fn raw_brightness(&self, pos: BlockPos) -> i32 {
        self.brightness(LightLayer::Block, pos)
            .max(self.brightness(LightLayer::Sky, pos))
    }

    /// `getEffectiveSkyBrightness`：天空光减去天色变暗的部分。
    pub fn effective_sky_brightness(&self, pos: BlockPos) -> i32 {
        (self.brightness(LightLayer::Sky, pos) - SKY_DARKEN).max(0)
    }

    /// `getSignal`：`pos` 这一格朝 `direction` 输出多强的信号。
    ///
    /// 导体还要算上强充能它的邻居，这就是红石线能被旁边的方块“穿过”充能的原因。
    pub fn signal(&self, pos: BlockPos, direction: Direction) -> i32 {
        let state = self.block_state(pos);
        let signal = state.signal(self, pos, direction);
        if state.is_redstone_conductor() {
            signal.max(self.direct_signal_to(pos))
        } else {
            signal
        }
    }

    /// `hasSignal`。
    pub fn has_signal(&self, pos: BlockPos, direction: Direction) -> bool {
        self.signal(pos, direction) > 0
    }

    /// `getDirectSignal`。
    pub fn direct_signal(&self, pos: BlockPos, direction: Direction) -> i32 {
        self.block_state(pos).direct_signal(self, pos, direction)
    }

    /// `getDirectSignalTo`：周围六格强充能 `pos` 的最大值。
    pub fn direct_signal_to(&self, pos: BlockPos) -> i32 {
        let mut best = 0;
        for direction in Direction::ALL {
            best = best.max(self.direct_signal(pos.relative(direction), direction));
            if best >= 15 {
                break;
            }
        }
        best
    }

    /// `getControlInputSignal`：中继器和比较器读侧面的输入。
    /// `only_diodes` 为真时只认中继器和比较器，锁定用的是这种。
    pub fn control_input_signal(
        &self,
        pos: BlockPos,
        direction: Direction,
        only_diodes: bool,
    ) -> i32 {
        let state = self.block_state(pos);
        if only_diodes {
            if state.is_a(BlockAncestor::DiodeBlock) {
                self.direct_signal(pos, direction)
            } else {
                0
            }
        } else if state.is(blocks::REDSTONE_BLOCK) {
            15
        } else if state.is(blocks::REDSTONE_WIRE) {
            state.get(POWER)
        } else if state.is_signal_source() {
            self.direct_signal(pos, direction)
        } else {
            0
        }
    }

    /// `hasNeighborSignal`。
    pub fn has_neighbor_signal(&self, pos: BlockPos) -> bool {
        Direction::ALL
            .into_iter()
            .any(|direction| self.has_signal(pos.relative(direction), direction))
    }

    /// `getBestNeighborSignal`：周围六格给 `pos` 的最强信号。
    pub fn best_neighbor_signal(&self, pos: BlockPos) -> i32 {
        let mut best = 0;
        for direction in Direction::ALL {
            let signal = self.signal(pos.relative(direction), direction);
            if signal >= 15 {
                return 15;
            }
            best = best.max(signal);
        }
        best
    }

    /// `getBestOwnOrNeighbourSignal`。
    pub fn best_own_or_neighbour_signal(&self, pos: BlockPos) -> i32 {
        let state = self.block_state(pos);
        let own = if state.is_signal_source() {
            state.own_signal(self, pos)
        } else {
            0
        };
        self.best_neighbor_signal(pos).max(own)
    }

    /// `RedstoneWireBlock.getBlockSignal`：算导线的输入时先把导线的信号关掉，
    /// 不然导线自己的功率会绕回自己身上。官方的 `shouldSignal` 是方块实例上的字段。
    pub fn wire_block_signal(&mut self, pos: BlockPos) -> i32 {
        self.wire_should_signal = false;
        let signal = self.best_neighbor_signal(pos);
        self.wire_should_signal = true;
        signal
    }

    /// 红石线此刻算不算信号源，见 [`World::wire_block_signal`]。
    pub const fn wire_should_signal(&self) -> bool {
        self.wire_should_signal
    }

    /// 比较器的输出和火把最近的开关，见 [`RedstoneMemory`]。
    pub fn redstone_memory(&self) -> &RedstoneMemory {
        self.level.redstone()
    }

    /// 见 [`World::redstone_memory`]。
    pub fn redstone_memory_mut(&mut self) -> &mut RedstoneMemory {
        self.level.redstone_mut()
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

    /// `scheduleTick(pos, fluid, fluid.getTickDelay(level))`，`fluid` 是流体的注册表 ID。
    pub fn schedule_fluid_tick(&mut self, pos: BlockPos, fluid: i32) {
        // 空的流体的 `getTickDelay` 是 0，官方也照样排，到期时什么也不做。
        let delay = Fluid::of_type(fluid).map_or(0, Fluid::tick_delay);
        self.schedule_fluid_tick_with_delay(pos, fluid, delay);
    }

    /// `scheduleTick(pos, fluid, delay)`。
    pub fn schedule_fluid_tick_with_delay(&mut self, pos: BlockPos, fluid: i32, delay: i32) {
        self.ticks
            .schedule_fluid(pos, fluid, delay, TickPriority::Normal);
    }

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
        if old == state {
            return None;
        }
        let block_changed = !old.is_same_block(state);
        let drops_own_contents = block_changed
            && old.has_block_entity()
            && !block_entity::keeps_block_entity(state, old)
            && flags & UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS == 0;
        // 掉落要取随机数，取的个数取决于每格有几件物品。物品表在写入之后就被换了，
        // 所以先抄一份数量；双箱这里只算这一半，官方掉的是方块实体自己的槽位表。
        let dropped_counts = drops_own_contents.then(|| self.level.own_container_counts(pos));
        if !self.level.write_block(pos, state, flags) {
            return None;
        }

        // 换了方块，官方把方块实体一起删掉：先跑 remove 之前的副作用，再 remove。
        if block_changed && old.has_block_entity() {
            if flags & UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS == 0 {
                self.pre_remove_block_entity(pos, old);
            }
            self.remove_block_entity(pos);
        }
        // 比较器存的输出这些也跟着没了。
        if block_changed {
            self.level.redstone_mut().forget_block_entity(pos);
        }
        let moved_by_piston = flags & UPDATE_MOVE_BY_PISTON != 0;
        // 官方的方块实体在 `affectNeighborsAfterRemoval` 之前掉落，顺序影响取数。
        if let Some(counts) = dropped_counts {
            block_entity::pre_remove_side_effects(old, self, &counts);
        }
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

    /// `BlockEntity.preRemoveSideEffects`。实体在回调期间不在表里，和 [`World::with_block_entity`] 一样。
    fn pre_remove_block_entity(&mut self, pos: BlockPos, state: BlockState) {
        let Some((id, mut entity)) = self.level.block_entities_mut().take_at(pos) else {
            return;
        };
        entity.pre_remove_side_effects(self, pos, state);
        self.level.block_entities_mut().put(pos, id, entity);
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

    /// `BlockGetter.clip`：从 `from` 到 `to` 的视线碰到的第一个方块或者流体，什么也没碰到时是 `None`。
    ///
    /// 方块的上下文是 `ClipContext.Block.OUTLINE`，选中框要看玩家主手拿着什么，所以要传 `main_hand`。
    pub(crate) fn clip(
        &self,
        from: Vec3,
        to: Vec3,
        fluid: ClipFluid,
        main_hand: i32,
    ) -> Option<BlockHit> {
        traverse_blocks(from, to, |pos| {
            self.clip_one_block(pos, from, to, fluid, main_hand)
        })
    }

    fn clip_one_block(
        &self,
        pos: BlockPos,
        from: Vec3,
        to: Vec3,
        fluid: ClipFluid,
        main_hand: i32,
    ) -> Option<BlockHit> {
        let state = self.block_state(pos);
        let outline = outline_seen_holding(state, pos, main_hand);
        let block_hit = clip_with_interaction_override(&outline, state, from, to, pos);
        let fluid_hit = match fluid {
            ClipFluid::SourceOnly => clip_source(state.fluid(), pos, from, to),
            ClipFluid::None => None,
        };
        let distance =
            |hit: &Option<BlockHit>| hit.map_or(f64::MAX, |hit| from.distance_to_sqr(hit.location));
        if distance(&block_hit) <= distance(&fluid_hit) {
            block_hit
        } else {
            fluid_hit
        }
    }
}

/// `EntityCollisionContext` 下的选中框：主手拿着光源方块或者脚手架时，它们的选中框是整格。
fn outline_seen_holding(state: BlockState, pos: BlockPos, main_hand: i32) -> shape::Boxes {
    let held_is_own_item =
        main_hand != mc_bamboo_data::items::AIR && main_hand == state.block().item;
    if held_is_own_item && (state.is(blocks::LIGHT) || state.is(blocks::SCAFFOLDING)) {
        let full = shape::Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        return std::borrow::Cow::Owned(vec![full]);
    }
    shape::outline_boxes(state, pos)
}

/// `BlockGetter.clipWithInteractionOverride`：交互形状碰得更早时，用它的面。
fn clip_with_interaction_override(
    outline: &[shape::Aabb],
    state: BlockState,
    from: Vec3,
    to: Vec3,
    pos: BlockPos,
) -> Option<BlockHit> {
    let hit = shape::clip(outline, from, to, pos)?;
    let interaction = shape::clip(&shape::interaction_boxes(state), from, to, pos);
    match interaction {
        Some(closer)
            if from.distance_to_sqr(closer.location) < from.distance_to_sqr(hit.location) =>
        {
            Some(BlockHit {
                face: closer.face,
                ..hit
            })
        }
        _ => Some(hit),
    }
}

/// `ClipContext.Fluid.SOURCE_ONLY` 的流体形状。
///
/// 官方 `FlowingFluid.getShape` 按流体状态缓存，高度是进程里第一次调用时那个位置算出来的，
/// 和后来的位置无关。这里固定用流体自己的高度，也就是上面没有同种流体时的高度，oracle 用同样的位置预热了缓存。
fn clip_source(fluid: FluidState, pos: BlockPos, from: Vec3, to: Vec3) -> Option<BlockHit> {
    if !fluid.is_source() {
        return None;
    }
    let bounds = shape::Aabb::new(0.0, 0.0, 0.0, 1.0, f64::from(fluid.own_height()), 1.0);
    shape::clip(&[bounds], from, to, pos)
}

/// `BlockGetter.traverseBlocks` 的未命中结果，类型是 `MISS` 的 `BlockHitResult`。
pub(crate) fn miss(from: Vec3, to: Vec3) -> BlockHit {
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
    use mc_bamboo_protocol::item::ItemStack;

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
            .filter_map(|effect| match *effect {
                Effect::Destroyed { pos, .. } => Some(pos),
                Effect::BoneMealUsed { .. }
                | Effect::TorchBurnout { .. }
                | Effect::FilledBucket { .. }
                | Effect::SignChanged { .. }
                | Effect::SignWaxed { .. }
                | Effect::OpenSignEditor { .. }
                | Effect::NoteBlockPlayed { .. }
                | Effect::BlockEvent { .. }
                | Effect::ComposterFill { .. } => None,
            })
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

    fn clip_empty_handed(
        world: &World,
        from: Vec3,
        to: Vec3,
        fluid: ClipFluid,
    ) -> Option<BlockHit> {
        world.clip(from, to, fluid, items::AIR)
    }

    #[test]
    fn clip_stops_at_the_first_block() {
        let mut level = TestLevel::new();
        let wall = BlockPos::new(3, 64, 0);
        level.set(wall, BlockState(blocks::STONE));
        level.set(wall.east(), BlockState(blocks::STONE));
        with_world(&mut level, |world| {
            let from = Vec3::new(0.5, 64.5, 0.5);
            let hit = clip_empty_handed(world, from, Vec3::new(9.5, 64.5, 0.5), ClipFluid::None)
                .expect("视线碰到墙");
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
        let (from, to) = (Vec3::new(0.5, 64.5, 0.5), Vec3::new(0.5, 55.5, 0.5));
        with_world(&mut level, |world| {
            let hit = clip_empty_handed(world, from, to, ClipFluid::SourceOnly).expect("看到水源");
            assert_eq!(hit.pos, source);
            assert_eq!(hit.face, Direction::Up);
            // 水面的高度是流体自己的高度，不看上面。
            assert_eq!(hit.location.y, 60.0 + f64::from(8.0_f32 / 9.0));
            assert_eq!(clip_empty_handed(world, from, to, ClipFluid::None), None);
        });
    }

    #[test]
    fn clip_misses_when_nothing_is_in_the_way() {
        let mut level = TestLevel::new();
        with_world(&mut level, |world| {
            let from = Vec3::new(0.5, 64.5, 0.5);
            let to = Vec3::new(0.5, 60.5, 0.5);
            assert_eq!(
                clip_empty_handed(world, from, to, ClipFluid::SourceOnly),
                None
            );
            let hit = miss(from, to);
            assert_eq!(hit.pos, BlockPos::new(0, 60, 0));
            assert_eq!(hit.face, Direction::Up);
            assert_eq!(hit.location, to);
        });
    }

    #[test]
    fn the_inside_of_a_cauldron_reports_its_top_face() {
        let mut level = TestLevel::new();
        let cauldron = BlockPos::new(0, 64, 0);
        level.set(cauldron, BlockState(blocks::CAULDRON));
        // 斜着看进锅里，射线落在里面的北墙上，交互形状的顶面更近，报告的面换成顶面。
        let from = Vec3::new(0.5, 66.0, 1.5);
        let to = Vec3::new(0.5, 64.3, 0.05);
        with_world(&mut level, |world| {
            let hit = clip_empty_handed(world, from, to, ClipFluid::None).expect("看到锅");
            assert_eq!(hit.pos, cauldron);
            assert_eq!(hit.face, Direction::Up);
            assert!(hit.location.y < 65.0);
        });
    }

    #[test]
    fn holding_scaffolding_makes_its_outline_a_full_block() {
        let mut level = TestLevel::new();
        let scaffolding = BlockPos::new(0, 64, 0);
        level.set(scaffolding, BlockState(blocks::SCAFFOLDING));
        // 从四条腿中间横穿过去。
        let (from, to) = (Vec3::new(-1.0, 64.5, 0.5), Vec3::new(3.0, 64.5, 0.5));
        with_world(&mut level, |world| {
            assert_eq!(clip_empty_handed(world, from, to, ClipFluid::None), None);
            let held = world
                .clip(from, to, ClipFluid::None, items::SCAFFOLDING)
                .expect("拿着脚手架时它是整格");
            assert_eq!(held.face, Direction::West);
            assert_eq!(held.location.x, 0.0);
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

    /// 容器被换掉时，官方 `preRemoveSideEffects` 按每格的数量取随机数：每一格三个
    /// double 的掉落点，非空的堆再按 `split(nextInt(21) + 10)` 一块一块取六个 double。
    /// 这个世界没有物品实体，但随机数一个都不能少，否则后面的随机刻会错开。
    #[test]
    fn destroying_a_container_consumes_the_drop_random_numbers() {
        let mut level = TestLevel::new();
        level.set(CENTER, BlockState(blocks::CHEST));
        let mut slots = vec![ItemStack::empty(); 27];
        // 第一格一件，第二格三件，其余 25 格是空的。
        slots[0] = ItemStack::new(items::STONE, 1);
        slots[1] = ItemStack::new(items::DIRT, 3);
        level.set_container(CENTER, slots);
        with_world(&mut level, |world| {
            world.set_block(CENTER, BlockState::AIR, UPDATE_ALL);
        });

        let expected = LegacyRandom::new(0);
        for _ in [1, 3] {
            for _ in 0..3 {
                expected.next_double();
            }
            expected.next_int(21);
            for _ in 0..6 {
                expected.next_double();
            }
        }
        for _ in 2..27 {
            for _ in 0..3 {
                expected.next_double();
            }
        }
        assert_eq!(
            expected.next_double().to_bits(),
            level.random.source().next_double().to_bits()
        );
    }
}
