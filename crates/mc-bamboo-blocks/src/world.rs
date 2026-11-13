//! 官方 `Level` 里和方块有关的部分：`setBlock`、`destroyBlock`、邻居更新、计划刻。
//!
//! `Behaviour` 的方法都通过 [`World`] 读写世界。方法名和官方相同，要对照时直接搜官方的名字。

use std::collections::VecDeque;

use mc_bamboo_core::{BlockPos, ChunkPos, Direction, LightLayer, Vec3, mth, sign::SignContent};
use mc_bamboo_data::{Block, BlockAncestor, blocks, sounds};
use mc_bamboo_protocol::item::ItemStack;
use tracing::{error, warn};

use crate::{
    behaviour::machines::{ShulkerBoxEntity, open_support_shape},
    block_entity::{self, BlockEntity, BlockEvent},
    context::BlockHit,
    entity::{Entity, PrimedTnt},
    fluid::Fluid,
    level::{Effect, LecternFacts, Level, Observation, Trigger},
    random::LegacyRandom,
    shape::{self, CollisionContext},
    state::{BlockState, FACING, FluidState, POWER, SupportType, WATERLOGGED},
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
/// `Difficulty.NORMAL.getId()`。
const NORMAL_DIFFICULTY: i32 = 2;
/// `getSkyDarken`。世界时钟停在正午，天空一点也不暗。
const SKY_DARKEN: i32 = 0;

/// 官方 `Level.playSound(null, pos, ...)` 用的坐标：方块的中心。
pub fn block_center_public(pos: BlockPos) -> Vec3 {
    block_center(pos)
}

fn block_center(pos: BlockPos) -> Vec3 {
    Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    )
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

/// 方块实体不在表里时不能读它。这些检查先排着，放回去之后按请求顺序再跑。
///
/// These checks need a block entity that is out of the map. They run, in request order, after
/// that entity is put back.
#[derive(Clone, Copy)]
pub(crate) enum DeferredRead {
    /// `updateNeighbourForOutputSignal`。源格子的实体被拿着。
    ///
    /// `updateNeighbourForOutputSignal` while the source cell's entity is out of the map.
    OutputSignal {
        pos: BlockPos,
        block: &'static Block,
    },
    /// 比较器 `neighborChanged`。输入要读被拿着的熔炉或酿造台。
    ///
    /// Comparator `neighborChanged`. Its input would read a furnace or brewing stand that is out.
    ComparatorNeighbor {
        pos: BlockPos,
        source: &'static Block,
    },
    /// 比较器 `setPlacedBy`。
    ///
    /// Comparator `setPlacedBy`.
    ComparatorPlaced { pos: BlockPos },
    /// 比较器 `refreshOutputState`。
    ///
    /// Comparator `refreshOutputState`.
    ComparatorRefresh { pos: BlockPos },
    /// 潜影盒 `doNeighborUpdates`。打开时的形状在方块实体上。
    ///
    /// Shulker `doNeighborUpdates`. The open shape lives on the block entity.
    ShulkerNeighbors { pos: BlockPos },
}

impl DeferredRead {
    fn pos(self) -> BlockPos {
        match self {
            Self::OutputSignal { pos, .. }
            | Self::ComparatorNeighbor { pos, .. }
            | Self::ComparatorPlaced { pos }
            | Self::ComparatorRefresh { pos }
            | Self::ShulkerNeighbors { pos } => pos,
        }
    }

    fn is_blocked(self, world: &World) -> bool {
        match self {
            Self::OutputSignal { pos, .. } | Self::ShulkerNeighbors { pos } => {
                world.block_entity_is_taken(pos)
            }
            Self::ComparatorNeighbor { pos, .. }
            | Self::ComparatorPlaced { pos }
            | Self::ComparatorRefresh { pos } => {
                crate::behaviour::machines::comparator_reads_taken_entity(world, pos)
            }
        }
    }

    fn run(self, world: &mut World) {
        match self {
            Self::OutputSignal { pos, block } => {
                world.update_neighbour_for_output_signal(pos, block);
            }
            Self::ComparatorNeighbor { pos, source } => {
                let state = world.block_state(pos);
                if state.is(blocks::COMPARATOR) {
                    state.neighbor_changed(world, pos, source);
                }
            }
            Self::ComparatorPlaced { pos } => {
                crate::behaviour::machines::resume_comparator_placed(world, pos);
            }
            Self::ComparatorRefresh { pos } => {
                crate::behaviour::machines::resume_comparator_refresh(world, pos);
            }
            Self::ShulkerNeighbors { pos } => {
                crate::behaviour::machines::apply_shulker_neighbor_updates(world, pos);
            }
        }
    }
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
    /// 这次右键要从方块里拿进物品栏的一堆，比如雕纹书架取出的书。
    ///
    /// A stack this click moves from a block into the inventory, such as a book
    /// taken from a chiseled bookshelf.
    gained: Option<mc_bamboo_protocol::item::ItemStack>,
    /// `RedstoneWireBlock.shouldSignal`，见 [`World::wire_block_signal`]。
    wire_should_signal: bool,
    /// `ServerLevel.isHandlingTick`：这次操作在世界刻里。
    handling_tick: bool,
    /// 方块实体被拿出去时排进来的比较器读取和潜影盒邻居更新。一层对应一次拿出。
    ///
    /// Comparator reads and shulker neighbour updates queued while a block entity is out of
    /// the map. One frame per take.
    held_reads: Vec<VecDeque<DeferredRead>>,
}

/// `CollisionGetter.getBlockCollisions`，方块从 `block_at` 读。
///
/// 玩家移动不能借 `World`：`RegionLevel` 借着玩家表。形状顺序和 [`World::block_collisions`] 相同。
///
/// `CollisionGetter.getBlockCollisions`, reading blocks from `block_at`.
///
/// Player movement cannot borrow `World`: `RegionLevel` already borrows the player map.
/// Shape order matches [`World::block_collisions`].
pub(crate) fn collect_block_collisions(
    box_: shape::Aabb,
    context: CollisionContext,
    mut block_at: impl FnMut(BlockPos) -> BlockState,
) -> Vec<(BlockPos, Vec<shape::Aabb>)> {
    let x_min = mth::floor(box_.min_x - shape::EPSILON) - 1;
    let x_max = mth::floor(box_.max_x + shape::EPSILON) + 1;
    let y_min = mth::floor(box_.min_y - shape::EPSILON) - 1;
    let y_max = mth::floor(box_.max_y + shape::EPSILON) + 1;
    let z_min = mth::floor(box_.min_z - shape::EPSILON) - 1;
    let z_max = mth::floor(box_.max_z + shape::EPSILON) + 1;
    let mut collisions = Vec::new();
    for z in z_min..=z_max {
        for y in y_min..=y_max {
            for x in x_min..=x_max {
                let corner = u8::from(x == x_min || x == x_max)
                    + u8::from(y == y_min || y == y_max)
                    + u8::from(z == z_min || z == z_max);
                if corner == 3 {
                    continue;
                }
                let pos = BlockPos::new(x, y, z);
                let state = block_at(pos);
                if corner == 1 && !shape::has_large_collision_shape(state) {
                    continue;
                }
                if corner == 2 && !state.is(blocks::MOVING_PISTON) {
                    continue;
                }
                let boxes = shape::collision_boxes(state, pos, context);
                if boxes.is_empty() {
                    continue;
                }
                let (x_origin, y_origin, z_origin) = (f64::from(x), f64::from(y), f64::from(z));
                let matched = if shape::is_unit_cube(&boxes) {
                    box_.intersects_coords(
                        x_origin,
                        y_origin,
                        z_origin,
                        x_origin + 1.0,
                        y_origin + 1.0,
                        z_origin + 1.0,
                    )
                } else {
                    let at_position = shape::moved(&boxes, x_origin, y_origin, z_origin);
                    shape::join_is_not_empty(&at_position, &[box_], shape::BooleanOp::And)
                };
                if matched {
                    collisions.push((pos, shape::moved(&boxes, x_origin, y_origin, z_origin)));
                }
            }
        }
    }
    collisions
}

impl<'a> World<'a> {
    pub fn new(level: &'a mut dyn Level, ticks: &'a mut BlockTicks) -> Self {
        // 睡着的实体按公式现算数值时要知道现在是哪一刻。
        crate::block_entity::set_block_entity_clock(ticks.now());
        Self {
            level,
            ticks,
            updates: NeighborQueue::default(),
            effects: Vec::new(),
            placed_at: None,
            item_exchange: None,
            gained: None,
            wire_should_signal: true,
            handling_tick: false,
            held_reads: Vec::new(),
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

    /// 和 [`Self::into_effects`] 一样，但世界还要继续用。
    ///
    /// Same collected effects, leaving the world usable afterwards.
    pub fn drain_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
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

    /// 这次右键记下的、要放进物品栏的物品。
    ///
    /// The stack recorded for this click, to be added to the inventory.
    pub(crate) fn gained(&self) -> Option<&mc_bamboo_protocol::item::ItemStack> {
        self.gained.as_ref()
    }

    pub(crate) fn ticks_mut(&mut self) -> &mut BlockTicks {
        self.ticks
    }

    /// 取出这一刻到期的流体计划刻，判断从这一层背后的 `Level` 读。
    /// 单独开个入口是为了让 `Level` 的只读借用和计划刻表的可变借用不相交。
    pub(crate) fn collect_due_fluids(&mut self) {
        let level: &dyn Level = self.level;
        let can_tick = |chunk| level.chunk_ticks_blocks(chunk);
        self.ticks.collect_due_fluids(&can_tick);
    }

    /// 记下一个发给玩家的结果，官方的 `levelEvent`。
    pub fn push_effect(&mut self, effect: Effect) {
        self.level.observe(Observation::Effect(&effect));
        self.effects.push(effect);
    }

    /// 活塞伸出/收回的「取一个数 → 算音高 → 发音效」动作。
    ///
    /// 官方 `PistonBaseBlock.triggerEvent` 在分支开头取 `level.getRandom()`，
    /// 伸出用 `nextFloat()*0.25F+0.6F`，收回用 `nextFloat()*0.15F+0.6F`。
    /// 录制包装靠 `observe` 把这次取数记成一条可重放的效果，重放时照样从
    /// 世界随机源取，不换成自己的随机源。
    pub fn play_piston_sound(
        &mut self,
        pos: BlockPos,
        sound: u16,
        volume: f32,
        scale: f32,
        base: f32,
    ) {
        let pitch = self.level.next_random_float() * scale + base;
        self.level.observe(Observation::PistonPitch {
            pos,
            sound: i32::from(sound),
            source: sounds::BLOCKS,
            volume,
            scale,
            base,
            except_initiator: false,
        });
        self.push_effect(Effect::Sound {
            sound: i32::from(sound),
            source: sounds::BLOCKS,
            x: block_center(pos).x,
            y: block_center(pos).y,
            z: block_center(pos).z,
            volume,
            pitch,
            except_initiator: false,
            seed: None,
        });
    }

    /// `playSound(null, pos, sound, BLOCKS, volume, pitch)`，坐标是方块中心。
    pub fn play_block_sound(&mut self, pos: BlockPos, sound: u16, volume: f32, pitch: f32) {
        self.play_sound(
            block_center(pos),
            sound,
            sounds::BLOCKS,
            volume,
            pitch,
            false,
        );
    }

    /// 同上，但跳过发起这次操作的玩家，官方传的是 `player` 作 `except`。
    pub fn play_block_sound_except(&mut self, pos: BlockPos, sound: u16, volume: f32, pitch: f32) {
        self.play_sound_except(pos, sound, sounds::BLOCKS, volume, pitch);
    }

    /// `Level.playSound(player, pos, sound, source, volume, pitch)`。
    /// 坐标是方块中心，跳过发起的玩家。
    /// Block centre; the player who started it is skipped.
    pub fn play_sound_except(
        &mut self,
        pos: BlockPos,
        sound: u16,
        source: sounds::SoundSource,
        volume: f32,
        pitch: f32,
    ) {
        self.play_sound(block_center(pos), sound, source, volume, pitch, true);
    }

    /// `playSound(except, x, y, z, sound, source, volume, pitch)`，允许任意坐标和声音来源。
    pub fn play_sound(
        &mut self,
        position: Vec3,
        sound: u16,
        source: sounds::SoundSource,
        volume: f32,
        pitch: f32,
        except_initiator: bool,
    ) {
        self.push_effect(Effect::Sound {
            sound: i32::from(sound),
            source,
            x: position.x,
            y: position.y,
            z: position.z,
            volume,
            pitch,
            except_initiator,
            seed: None,
        });
    }

    /// `playSeededSound(null, x, y, z, sound, source, volume, pitch, seed)`，音符盒用。
    pub fn play_seeded_sound(
        &mut self,
        position: Vec3,
        sound: u16,
        source: sounds::SoundSource,
        volume: f32,
        pitch: f32,
        seed: i64,
    ) {
        self.push_effect(Effect::Sound {
            sound: i32::from(sound),
            source,
            x: position.x,
            y: position.y,
            z: position.z,
            volume,
            pitch,
            except_initiator: false,
            seed: Some(seed),
        });
    }

    /// 见 [`Level::dispenser_targets`]。
    pub fn dispenser_targets(&self) -> Vec<crate::level::EquipTarget> {
        self.level.dispenser_targets()
    }

    /// 见 [`Level::set_dispenser_equipment`]。
    pub fn set_dispenser_equipment(
        &mut self,
        entity_id: i32,
        component_slot: i32,
        stack: ItemStack,
    ) -> Option<u16> {
        self.level
            .set_dispenser_equipment(entity_id, component_slot, stack)
    }

    /// `levelEvent(null, type, pos, data)`。
    pub fn level_event(&mut self, event: i32, pos: BlockPos, data: i32) {
        self.push_effect(Effect::LevelEvent {
            event,
            pos,
            data,
            except_initiator: false,
        });
    }

    /// 同上，但跳过发起这次操作的玩家，官方传的是 `player` 作 `source`。
    pub fn level_event_except(&mut self, event: i32, pos: BlockPos, data: i32) {
        self.push_effect(Effect::LevelEvent {
            event,
            pos,
            data,
            except_initiator: true,
        });
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

    /// 这次操作背后的存储。天气查询要问高度图和群系，只有这些只读查询用得到。
    pub fn level(&self) -> &dyn Level {
        self.level
    }

    /// 实体驱动的入口：写一个 `World` 入口函数给并行/精确两种驱动用。
    pub fn entities_mut(&mut self) -> &mut dyn crate::level::LevelEntities {
        self.level
    }

    /// 两段式并行驱动的算阶段之前调用：把实体表切进「这一刻正在 tick」的状态。
    pub fn begin_entity_tick(&mut self) {
        self.level.begin_entity_tick();
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

    /// `hasChunkAt`：只看区块坐标是否在合法范围内，不看这一格有没有真正加载。
    ///
    /// `hasChunkAt` for the command position check: the chunk coordinate is in range.
    /// This world treats every in-range chunk as present.
    pub fn has_chunk_at(&self, pos: BlockPos) -> bool {
        (pos.x >> 4).abs() <= VALID_CHUNK_BOUND && (pos.z >> 4).abs() <= VALID_CHUNK_BOUND
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

    /// `getWorldBorder().isWithinBounds(pos)`：读 `Level` 给的边界快照。
    ///
    /// `getWorldBorder().isWithinBounds(pos)`: read the border snapshot from `Level`.
    pub fn is_within_world_border(&self, pos: BlockPos) -> bool {
        self.world_border().is_within_bounds_pos(pos)
    }

    /// 这一格的方块实体本刻是不是已经 tick 过，见 [`BlockTicks::entity_ticked_this_tick`]。
    pub fn entity_ticked_this_tick(&self, pos: BlockPos) -> bool {
        self.ticks.entity_ticked_this_tick(pos)
    }

    /// `Level.getWorldBorder()`，方块逻辑和实体逻辑读到的同一份快照。
    ///
    /// `Level.getWorldBorder()`: the same snapshot block and entity logic read.
    pub fn world_border(&self) -> crate::WorldBorder {
        self.level.world_border()
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
        self.level.block_entity_dyn(pos)?.as_any().downcast_ref()
    }

    /// 这一格有没有方块实体。
    pub fn has_block_entity(&self, pos: BlockPos) -> bool {
        self.level.has_block_entity(pos)
    }

    /// 这一格容器里非空的物品。对照漏斗吸物品时用。
    ///
    /// Non-empty stacks in the container at this cell. Used when comparing a hopper pull.
    pub fn container_contents(&mut self, pos: BlockPos) -> Vec<(i32, i32)> {
        let mut found = Vec::new();
        let _ = self.with_container(pos, &mut |container| {
            for slot in 0..container.size() {
                let stack = container.get(slot);
                if !stack.is_empty() {
                    found.push((stack.item, stack.count));
                }
            }
        });
        found
    }

    /// 这一格的物品栏，见 [`Level::with_container`]。
    ///
    /// 堆肥桶没有方块实体，官方 `ComposterBlock.getContainer` 按等级现做一个容器，这里同样处理。
    pub(crate) fn with_container(
        &mut self,
        pos: BlockPos,
        f: &mut dyn FnMut(&mut dyn crate::container::Container),
    ) -> bool {
        self.level.wake_block_entity(pos);
        // 官方 `RandomizableContainerBlockEntity.getItem/removeItem/setItem/isEmpty`
        // 都先 `unpackLootTable(null)`；漏斗、投掷器和发射器走这条路。双箱两半
        // 合成一张 `CompoundContainer`，两半都要解包。
        // Vanilla's `getItem/removeItem/setItem/isEmpty` all unpack the loot table first;
        // hoppers, droppers and dispensers use this path. A double chest combines both
        // halves, so both are unpacked.
        self.level.unpack_loot_table(pos);
        for other in self.level.container_positions(pos) {
            if other != pos {
                self.level.unpack_loot_table(other);
            }
        }
        if self
            .block_state(pos)
            .is_a(mc_bamboo_data::BlockAncestor::ComposterBlock)
        {
            return crate::composter::with_container(self, pos, f);
        }
        if crate::block_container::is_block_container(self.block_state(pos)) {
            return crate::block_container::with_container(self, pos, f);
        }
        if let Some(found) = crate::machine_container::try_with_container(self, pos, f) {
            return found;
        }
        self.level.with_container(pos, f)
    }

    /// 这四种方块容器的槽位表，见 [`Level::with_slots`]。
    ///
    /// Slot list for those four block containers. See [`Level::with_slots`].
    pub(crate) fn with_slots(
        &mut self,
        pos: BlockPos,
        len: usize,
        f: &mut dyn FnMut(&mut [mc_bamboo_protocol::item::ItemStack]),
    ) -> bool {
        self.level.wake_block_entity(pos);
        // 陶罐的 `getTheItem/splitTheItem/setTheItem` 和漏斗/比较器读这一格都解包。
        // A decorated pot's `getTheItem/splitTheItem/setTheItem`, hoppers and comparators
        // all unpack the table before touching its single slot.
        self.level.unpack_loot_table(pos);
        self.level.with_slots(pos, len, f)
    }

    /// 这一格容器现在的槽位，见 [`Level::slots_at`]。
    ///
    /// The container slots at this position. See [`Level::slots_at`].
    pub(crate) fn slots_at(&self, pos: BlockPos) -> Vec<mc_bamboo_protocol::item::ItemStack> {
        self.level.slots_at(pos)
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
        self.level.wake_block_entity(pos);
        self.level.with_hopper(pos, f)
    }

    /// `Level.setBlockEntity`：放进这一格并登记 ticker。状态不收方块实体时和官方一样什么也不做，
    /// 只打一条警告，说明有地方放错了。
    pub fn set_block_entity(&mut self, entity: Box<dyn BlockEntity>) {
        let pos = entity.pos();
        self.level.wake_block_entity(pos);
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
        self.level.insert_block_entity(entity);
    }

    /// `Level.removeBlockEntity`。
    pub fn remove_block_entity(&mut self, pos: BlockPos) {
        self.level.wake_block_entity(pos);
        let _ = self.level.remove_block_entity(pos);
    }

    /// 带着这一格的方块实体运行一段代码。这段时间它不在表里，[`BlockEntities::get`] 是 `None`，
    /// [`BlockEntities::contains`] 仍算它在，除非回调里把它删了。要读它的比较器检查先排进队列，
    /// 放回去之后按请求顺序再跑，然后这个函数才返回。
    ///
    /// Runs code with this cell's block entity. For the call it is out of the map, so
    /// [`BlockEntities::get`] is `None`, while [`BlockEntities::contains`] still counts it unless
    /// the callback removes it. Comparator checks that need it are queued and run, in request
    /// order, after it is put back and before this function returns.
    pub fn with_block_entity<T: BlockEntity, R>(
        &mut self,
        pos: BlockPos,
        f: impl FnOnce(&mut T, &mut World) -> R,
    ) -> Option<R> {
        self.level.wake_block_entity(pos);
        let (id, mut entity) = self.level.take_block_entity_at(pos)?;
        self.begin_held_reads();
        let result = entity
            .as_any_mut()
            .downcast_mut::<T>()
            .map(|typed| f(typed, self));
        self.level.put_block_entity(pos, id, entity);
        self.end_held_reads();
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
        let event = BlockEvent {
            pos,
            block,
            param_a,
            param_b,
        };
        let existed = self.level.block_event_queued(event);
        self.level
            .observe(Observation::BlockEventQueued { event, existed });
        self.level.push_block_event(event);
    }

    /// `ServerLevel.runBlockEvents`：按排队的顺序一直处理到队列空，处理中新排进来的同一轮执行。
    ///
    /// 所在区块不能 tick 的事件收进 `blockEventsToReschedule`，这一轮全部处理完之后
    /// 才按原顺序加回队列，下一刻再看。处理成功的事件记成 [`Effect::BlockEvent`]，
    /// 由调用方发给附近的玩家。
    pub fn run_block_events(&mut self) {
        let mut reschedule = Vec::new();
        while let Some(event) = self.level.pop_block_event() {
            if !self.level.chunk_ticks_blocks(event.pos.chunk()) {
                reschedule.push(event);
                continue;
            }
            let state = self.block_state(event.pos);
            if state.block_id() != event.block {
                continue;
            }
            self.begin_trigger(Trigger::BlockEvent {
                pos: event.pos,
                kind: event.block,
                param_a: event.param_a,
                param_b: event.param_b,
            });
            if state.trigger_event(self, event.pos, event.param_a, event.param_b) {
                self.push_effect(Effect::BlockEvent {
                    pos: event.pos,
                    block: event.block,
                    param_a: event.param_a,
                    param_b: event.param_b,
                });
            }
            self.level.observe(Observation::TriggerEnd);
        }
        self.level.reschedule_block_events(reschedule);
    }

    /// 驱动循环在每次触发开始前报一次边界。
    pub fn begin_trigger(&self, trigger: Trigger) {
        self.level.observe(Observation::TriggerBegin(trigger));
    }

    /// 切换录制包装的记账模式，见 [`Observation::RecordMode`]。
    pub fn set_record_mode(&self, mode: crate::level::RecordMode) {
        self.level.observe(Observation::RecordMode(mode));
    }

    /// 取走 `Count` 模式累计的读写项数，别的 `Level` 实现返回 0。
    pub fn take_counted_reads(&self) -> u64 {
        self.level.take_counted_reads()
    }

    /// 缓存守卫/重放期间关掉录制记账。
    pub fn set_capture(&self, capture: bool) {
        self.set_record_mode(if capture {
            crate::level::RecordMode::Full
        } else {
            crate::level::RecordMode::Off
        });
    }

    /// 一条到期的方块计划刻的边界。
    pub fn begin_scheduled_trigger(&self, pos: BlockPos, kind: usize) {
        self.begin_trigger(Trigger::ScheduledTick { pos, kind });
    }

    /// 一条到期的流体计划刻的边界。
    pub fn begin_fluid_trigger(&self, pos: BlockPos, kind: i32) {
        self.begin_trigger(Trigger::ScheduledFluidTick { pos, kind });
    }

    /// 一次触发结束。
    pub fn end_trigger(&self) {
        self.level.observe(Observation::TriggerEnd);
    }

    /// `Level.tickBlockEntities`：按 ticker 列表的顺序运行每刻的方块实体。
    ///
    /// 实体在 tick 期间不在表里，`get` 是 `None`。要读它的比较器检查和潜影盒的邻居更新先排进队列，
    /// 放回去之后、下一个 ticker 之前按请求顺序跑完。顺序不改。方块 oracle 会跳过熔炉和酿造台。
    ///
    /// `Level.tickBlockEntities` runs each block entity in ticker order.
    ///
    /// The entity is out of the map while it ticks, so `get` is `None`. Comparator checks that
    /// need it, and a shulker's neighbour updates, are queued and run in request order after it
    /// is put back and before the next ticker. The order does not change. The block oracle skips
    /// furnaces and brewing stands.
    pub fn tick_block_entities(&mut self) {
        let now = self.ticks.now();
        self.level.begin_block_entity_tick(now);
        while let Some((pos, id)) = self.level.next_block_entity_ticker() {
            // 官方先判 `shouldTickBlocksAt(pos)`，再判 `LevelChunk.isTicking(pos)`；
            // 跳过的 ticker 留在列表里，运行顺序不变。
            if !self.level.chunk_ticks_blocks(pos.chunk())
                || !self.level.runs_block_entity_tick(pos)
            {
                continue;
            }
            let Some(entity) = self.level.take_block_entity(pos, id) else {
                continue;
            };
            self.begin_trigger(Trigger::BlockEntityTick {
                pos,
                kind: self.block_state(pos).block_id(),
            });
            self.ticks.note_block_entity_tick(pos);
            self.run_taken_block_entity(pos, id, entity);
            // 一次 tick 跑完后按实体自己的公式算下一次真正需要跑的时刻。
            let wake = self.level.block_entity_sleep_from(pos, now);
            self.level.schedule_block_entity_wake(pos, wake);
            self.level.observe(Observation::TriggerEnd);
        }
        self.level.end_block_entity_tick();
    }

    /// 拿出的实体跑完 `tick` 再放回去，然后按请求顺序做排队的读取。
    /// 酿造台会在燃料通知之后停一次，读完再继续。
    ///
    /// Runs `tick` for an entity that was taken out, puts it back, then runs queued reads in
    /// request order. A brewing stand pauses after the fuel notification so those reads see the
    /// items from that moment.
    fn run_taken_block_entity(&mut self, pos: BlockPos, id: u64, mut entity: Box<dyn BlockEntity>) {
        // 燃料和后面的酿造最多两段。多出来的次数只是拦住停不下来的实现。
        // Fuel and the brew after it are at most two segments. The extra iterations only stop
        // an implementation that never clears its resume flag.
        for _ in 0..4 {
            self.begin_held_reads();
            entity.tick(self);
            let again = entity.resume_tick();
            self.level.put_block_entity(pos, id, entity);
            self.end_held_reads();
            if !again {
                return;
            }
            let Some(next) = self.level.take_block_entity(pos, id) else {
                return;
            };
            entity = next;
        }
        self.level.put_block_entity(pos, id, entity);
    }

    /// 这一格的方块实体是不是正被拿在外面。
    ///
    /// Whether this cell's block entity is currently out of the map.
    pub(crate) fn block_entity_is_taken(&self, pos: BlockPos) -> bool {
        self.level.is_block_entity_taken(pos)
    }

    fn begin_held_reads(&mut self) {
        self.held_reads.push(VecDeque::new());
    }

    /// 放回之后跑这一层里不再被挡住的读取。还要读更外层那份实体的，按原顺序交回去。
    ///
    /// After the put, runs reads in this frame that are no longer blocked. Reads that still
    /// need an outer taken entity are handed back in the same order.
    fn end_held_reads(&mut self) {
        let Some(mut frame) = self.held_reads.pop() else {
            return;
        };
        while let Some(item) = frame.pop_front() {
            if item.is_blocked(self) {
                self.defer_block_entity_read(item);
                for rest in frame.drain(..) {
                    self.defer_block_entity_read(rest);
                }
                return;
            }
            item.run(self);
        }
    }

    /// 现在有拿出的实体时排进当前这一层，否则马上跑。
    ///
    /// Queues onto the current frame while an entity is out. Otherwise runs it now.
    pub(crate) fn defer_block_entity_read(&mut self, item: DeferredRead) {
        if let Some(frame) = self.held_reads.last_mut() {
            frame.push_back(item);
            return;
        }
        if item.is_blocked(self) {
            let pos = item.pos();
            error!(
                "deferred block-entity read at {} {} {} is still blocked",
                pos.x, pos.y, pos.z
            );
            return;
        }
        item.run(self);
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

    /// `Level.isRaining()`：雨强超过 0.2 才算下雨。
    pub fn is_raining(&self) -> bool {
        self.level.block_weather().is_raining()
    }

    /// `Level.isThundering()`：雷暴强度和雨强的乘积超过 0.9 才算打雷。
    pub fn is_thundering(&self) -> bool {
        self.level.block_weather().is_thundering()
    }

    /// `Level.isRainingAt(pos)`：下雨、看得见天、这一格不低于 MOTION_BLOCKING 高度图，
    /// 而且群系在这一格的降水类型是雨（雪不算）。
    pub fn is_raining_at(&self, pos: BlockPos) -> bool {
        if !self.is_raining() || !self.level.can_see_sky(pos) {
            return false;
        }
        if self.level.heightmap_y(pos) > pos.y {
            return false;
        }
        self.level.precipitation_facts(pos).precipitation == crate::weather::Precipitation::Rain
    }

    /// `FireBlock.isNearRain(pos)`：这一格和四个水平邻居里有一格在下 RAIN。
    pub fn is_near_rain(&self, pos: BlockPos) -> bool {
        [pos, pos.west(), pos.east(), pos.north(), pos.south()]
            .into_iter()
            .any(|at| self.is_raining_at(at))
    }

    /// `getDifficulty().getId()`。这个世界没有难度设置，客户端收不到难度时显示普通，这里也按普通算。
    pub const fn difficulty_id(&self) -> i32 {
        NORMAL_DIFFICULTY
    }

    /// 只取一个 `next_float()`，见 [`Level::next_random_float`]。
    pub fn next_random_float(&self) -> f32 {
        self.level.next_random_float()
    }

    /// `getRandom()`，也就是 `Level.random`。
    pub fn random(&self) -> &LegacyRandom {
        self.level.random().source()
    }

    /// `BlockBehaviour.onExplosionHit` 里的 `getDrops`。假世界默认是空的。
    ///
    /// `getDrops` from `BlockBehaviour.onExplosionHit`. A world without loot returns nothing.
    pub(crate) fn explosion_block_drops(
        &mut self,
        state: BlockState,
        pos: BlockPos,
    ) -> Vec<mc_bamboo_protocol::item::ItemStack> {
        self.level.explosion_block_drops(state, pos)
    }

    /// `Block.getDrops`，见 [`Level::block_loot_drops`]。
    pub(crate) fn block_loot_drops(
        &mut self,
        state: BlockState,
        pos: BlockPos,
    ) -> Vec<mc_bamboo_protocol::item::ItemStack> {
        self.level.block_loot_drops(state, pos)
    }

    /// `BlockState.spawnAfterBreak` 的随机取数，见 [`Level::spawn_after_break_random`]。
    pub(crate) fn spawn_after_break_random(&mut self, state: BlockState, pos: BlockPos) {
        self.level.spawn_after_break_random(state, pos);
    }

    /// `getBlockRandomPos`。
    pub(crate) fn block_random_pos(&self, origin: BlockPos, y_mask: i32) -> BlockPos {
        self.level.random().block_random_pos(origin, y_mask)
    }

    /// 这个区块里哪些区块段 `LevelChunkSection.isRandomlyTicking`，第 i 位是从底部数起的第 i 段。
    pub(crate) fn randomly_ticking_sections(&self, chunk: ChunkPos) -> u32 {
        self.level.randomly_ticking_sections(chunk)
    }

    /// `isLoaded`：所有区块都在内存里，只看高度和坐标的范围。
    pub fn is_loaded(&self, pos: BlockPos) -> bool {
        self.is_in_valid_bounds(pos)
    }

    /// 方块逻辑读到的游戏规则。
    ///
    /// The game rules block logic reads.
    pub fn block_game_rules(&self) -> crate::gamerules::BlockGameRules {
        self.level.block_game_rules()
    }

    /// `ServerLevel.canSpreadFireAround`：`fire_spread_radius_around_player` 为 `-1` 时不看玩家，
    /// 否则离所有玩家都不到这个距离火才蔓延。
    ///
    /// `ServerLevel.canSpreadFireAround`: `-1` skips the player check. Otherwise fire spreads
    /// only when some player is within that distance.
    pub fn can_spread_fire_around(&self, pos: BlockPos) -> bool {
        let radius = self.block_game_rules().fire_spread_radius_around_player;
        // 官方在半径是 -1 时短路，不会调用 `anyPlayerCloseEnoughTo`。
        //
        // Vanilla short-circuits on -1 and does not call `anyPlayerCloseEnoughTo`.
        radius == -1 || self.level.any_player_close_enough_to(pos, radius)
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

    /// 红石记忆的每个读写入口，见 [`crate::LevelSideState`] 和 [`crate::RedstoneMemory`]。
    ///
    /// `World` 不把整张表交出去：这样录制实现能看见每次读了哪一格、写了哪一格。
    /// Every redstone-memory read/write goes through here; see [`crate::LevelSideState`].
    pub fn comparator_output(&self, pos: BlockPos) -> i32 {
        self.level.comparator_output(pos)
    }

    pub fn set_comparator_output(&mut self, pos: BlockPos, signal: i32) {
        self.level.set_comparator_output(pos, signal);
    }

    pub fn last_bookshelf_slot(&self, pos: BlockPos) -> Option<usize> {
        self.level.last_bookshelf_slot(pos)
    }

    pub fn set_last_bookshelf_slot(&mut self, pos: BlockPos, slot: usize) {
        self.level.set_last_bookshelf_slot(pos, slot);
    }

    pub fn is_jukebox_playing(&self, pos: BlockPos) -> bool {
        self.level.is_jukebox_playing(pos)
    }

    pub fn set_jukebox_playing(&mut self, pos: BlockPos, playing: bool) {
        self.level.set_jukebox_playing(pos, playing);
    }

    pub fn crafter_disabled(&self, pos: BlockPos) -> u16 {
        self.level.crafter_disabled(pos)
    }

    pub fn set_crafter_disabled(&mut self, pos: BlockPos, mask: u16) {
        self.level.set_crafter_disabled(pos, mask);
    }

    pub fn set_crafter_crafting(&mut self, pos: BlockPos, remaining: i32) {
        self.level
            .set_crafter_crafting(pos, remaining, self.ticks.now());
    }

    pub fn take_crafter_crafting(&mut self) -> Vec<(BlockPos, i32)> {
        self.level.take_crafter_crafting(self.ticks.now())
    }

    /// 这一刻到期的合成器倒计时，见 `LevelSideState::take_due_crafter_crafting`。
    pub fn take_due_crafter_crafting(&mut self, now: i64) -> Vec<BlockPos> {
        self.level.take_due_crafter_crafting(now)
    }

    pub fn restore_crafter_crafting(&mut self, entries: Vec<(BlockPos, i32)>) {
        self.level
            .restore_crafter_crafting(entries, self.ticks.now());
    }

    pub fn forget_block_entity_memory(&mut self, pos: BlockPos) {
        self.level.forget_block_entity_memory(pos);
    }

    pub fn set_pending_lectern(&mut self, facts: LecternFacts) {
        self.level.set_pending_lectern(facts);
    }

    pub fn take_pending_lectern(&mut self) -> Option<LecternFacts> {
        self.level.take_pending_lectern()
    }

    pub fn set_lectern(&mut self, pos: BlockPos, facts: LecternFacts) {
        self.level.set_lectern(pos, facts);
    }

    pub fn lectern(&self, pos: BlockPos) -> Option<LecternFacts> {
        self.level.lectern(pos)
    }

    pub fn clear_lectern(&mut self, pos: BlockPos) {
        self.level.clear_lectern(pos);
    }

    pub fn forget_torch_toggles_before(&mut self, when: i64) {
        self.level.forget_torch_toggles_before(when);
    }

    pub fn record_torch_toggle(&mut self, pos: BlockPos, when: i64) {
        self.level.record_torch_toggle(pos, when);
    }

    pub fn torch_toggle_count(&self, pos: BlockPos) -> usize {
        self.level.torch_toggle_count(pos)
    }

    /// `getGameTime`。
    pub fn game_time(&self) -> i64 {
        self.level.observe(Observation::TimeRead);
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
        self.level.observe(Observation::TickScheduled {
            pos,
            kind: i64::try_from(id).unwrap_or(i64::MAX),
            fluid: false,
            delay,
            priority,
            existed: self.ticks.has_scheduled_tick(pos, id),
        });
        self.ticks.schedule(pos, id, delay, priority);
    }

    /// 缓存守卫用的计划刻队列，见 `guard_tick_query`。
    pub fn ticks_ref(&self) -> &BlockTicks {
        self.ticks
    }

    /// 缓存驱动：取走录制包装刚产出的一条运行时记录。
    pub fn take_runtime_records(&self) -> Vec<crate::cache::RuntimeRecord> {
        self.level.runtime_records()
    }

    /// 缓存驱动：按排队顺序取一个方块事件。
    pub fn pop_block_event(&mut self) -> Option<BlockEvent> {
        self.level.pop_block_event()
    }

    /// 缓存驱动：这一格所在区块现在跑不跑方块逻辑。
    pub fn chunk_ticks_blocks(&self, chunk: ChunkPos) -> bool {
        self.level.chunk_ticks_blocks(chunk)
    }

    /// 缓存驱动：一轮结束后把推迟的事件按原顺序放回。
    pub fn reschedule_block_events(&mut self, events: Vec<BlockEvent>) {
        self.level.reschedule_block_events(events);
    }

    /// 缓存重放：按效果序列写一格方块，不触发邻居更新，也不报观察事件。
    /// `flags` 照旧登记广播和光照，见 [`Level::write_block`]。
    pub fn replay_block(&mut self, pos: BlockPos, state: BlockState, flags: u32) {
        let _ = self.level.write_block(pos, state, flags);
    }

    /// 缓存重放：写比较器输出，不报观察事件。
    pub fn replay_comparator(&mut self, pos: BlockPos, signal: i32) {
        self.level.set_comparator_output(pos, signal);
    }

    /// 缓存重放：排一个方块事件，不报观察事件。
    pub fn replay_block_event(&mut self, event: BlockEvent) {
        self.level.push_block_event(event);
    }

    /// 缓存重放：排一个方块计划刻，不报观察事件。
    pub fn replay_scheduled_tick(
        &mut self,
        pos: BlockPos,
        block: usize,
        delay: i32,
        priority: TickPriority,
    ) {
        self.ticks.schedule(pos, block, delay, priority);
    }

    /// 缓存重放：排一个流体计划刻，不报观察事件。
    pub fn replay_scheduled_fluid_tick(&mut self, pos: BlockPos, fluid: i32, delay: i32) {
        self.ticks
            .schedule_fluid(pos, fluid, delay, TickPriority::Normal);
    }

    /// 缓存重放：按录下的全部字段造回 `PistonMoving` 并走原始 `insert`。
    pub fn replay_piston_moving(
        &mut self,
        pos: BlockPos,
        moved_state: BlockState,
        direction: u8,
        extending: bool,
        source: bool,
        progress: u32,
        progress_o: u32,
    ) {
        let entity = crate::behaviour::machines::PistonMoving::from_facts(
            pos,
            moved_state,
            direction,
            extending,
            source,
            progress,
            progress_o,
        );
        self.level.wake_block_entity(pos);
        self.level.insert_block_entity(Box::new(entity));
    }

    /// 缓存重放：移除这一格的方块实体，走原始 `remove`。
    pub fn replay_remove_block_entity(&mut self, pos: BlockPos) {
        self.level.wake_block_entity(pos);
        let _ = self.level.remove_block_entity(pos);
    }

    /// 缓存重放：把这一格的槽位表整个换掉，走原始 `with_slots`。
    pub fn replay_container_slots(&mut self, pos: BlockPos, slots: Vec<mc_bamboo_protocol::item::ItemStack>) {
        let len = slots.len();
        let mut it = slots.into_iter();
        let _ = self
            .level
            .with_slots(pos, len, &mut |target| {
                for slot in target.iter_mut() {
                    if let Some(stack) = it.next() {
                        *slot = stack;
                    }
                }
            });
    }

    /// 缓存重放：清掉这一格的容器。
    pub fn replay_clear_container(&mut self, pos: BlockPos) {
        self.level.clear_container(pos);
    }

    /// 缓存重放：清掉比较器输出等红石记忆。
    pub fn replay_forget_block_entity_memory(&mut self, pos: BlockPos) {
        self.level.forget_block_entity_memory(pos);
    }

    /// 缓存重放：解包这一格的战利品表。
    pub fn replay_unpack_loot_table(&mut self, pos: BlockPos) {
        self.level.unpack_loot_table(pos);
    }

    /// 缓存重放：活塞音高照样从世界随机源取一个数，再发 `Effect::Sound`。
    pub fn replay_piston_sound(
        &mut self,
        pos: BlockPos,
        sound: i32,
        source: mc_bamboo_data::SoundSource,
        volume: f32,
        scale: f32,
        base: f32,
        except_initiator: bool,
    ) {
        let pitch = self.level.next_random_float() * scale + base;
        self.push_effect(Effect::Sound {
            sound,
            source,
            x: block_center(pos).x,
            y: block_center(pos).y,
            z: block_center(pos).z,
            volume,
            pitch,
            except_initiator,
            seed: None,
        });
    }

    /// 缓存守卫：重查计划刻队列（`Level` 包装看不见它）。
    pub fn guard_tick_query(&self, pos: BlockPos, block: usize, will_tick_this_tick: bool) -> bool {
        if will_tick_this_tick {
            self.ticks.will_tick_this_tick(pos, block)
        } else {
            self.ticks.has_scheduled_tick(pos, block)
        }
    }

    /// `getBlockTicks().hasScheduledTick`。
    pub fn has_scheduled_tick(&self, pos: BlockPos, block: &Block) -> bool {
        let id = BlockState(block.default_state).block_id();
        let result = self.ticks.has_scheduled_tick(pos, id);
        self.level.observe(Observation::TickQuery {
            pos,
            kind: id,
            will_tick_this_tick: false,
            result,
        });
        result
    }

    /// `getBlockTicks().willTickThisTick`。
    pub fn will_tick_this_tick(&self, pos: BlockPos, block: &Block) -> bool {
        let id = BlockState(block.default_state).block_id();
        let result = self.ticks.will_tick_this_tick(pos, id);
        self.level.observe(Observation::TickQuery {
            pos,
            kind: id,
            will_tick_this_tick: true,
            result,
        });
        result
    }

    /// `scheduleTick(pos, fluid, fluid.getTickDelay(level))`，`fluid` 是流体的注册表 ID。
    pub fn schedule_fluid_tick(&mut self, pos: BlockPos, fluid: i32) {
        // 空的流体的 `getTickDelay` 是 0，官方也照样排，到期时什么也不做。
        let delay = Fluid::of_type(fluid).map_or(0, Fluid::tick_delay);
        self.schedule_fluid_tick_with_delay(pos, fluid, delay);
    }

    /// `scheduleTick(pos, fluid, delay)`。
    pub fn schedule_fluid_tick_with_delay(&mut self, pos: BlockPos, fluid: i32, delay: i32) {
        self.level.observe(Observation::TickScheduled {
            pos,
            kind: i64::from(fluid),
            fluid: true,
            delay,
            priority: TickPriority::Normal,
            existed: self.ticks.has_scheduled_fluid(pos, fluid),
        });
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
        self.level.wake_block_entity(pos);
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
        // 所以先抄一份槽位；双箱这里只算这一半，官方掉的是方块实体自己的槽位表。
        //
        // The draw count depends on how many items each slot holds. The list is replaced after
        // the write, so copy the slots first. A double chest drops only this half.
        // 破坏容器时官方 `BlockEntity.preRemoveSideEffects` 读的是解包后的表。
        // 潜影盒的 `playerWillDestroy` 更早，也走同一个钩子。
        // Vanilla drops the unpacked contents in `preRemoveSideEffects`; a shulker box
        // unpacks even earlier in `playerWillDestroy`.
        if drops_own_contents {
            self.level.unpack_loot_table(pos);
        }
        let dropped_stacks = drops_own_contents.then(|| self.slots_at(pos));
        // 破坏唱片机时 `popOutTheItem` → `setTheItem(EMPTY)`：正在放的歌先 `stop`（通知邻居），
        // 再取两个 `nextFloat`，然后无条件再 `onSongChanged`。物品在第一次通知之前已经是空的。
        //
        // Breaking a jukebox runs `popOutTheItem` → `setTheItem(EMPTY)`: a playing song `stop`s
        // (neighbors are told) before the two `nextFloat`s, then `onSongChanged` runs again.
        // The item is already empty before the first notification.
        let jukebox_count = dropped_stacks
            .as_ref()
            .filter(|_| old.is_a(BlockAncestor::JukeboxBlock))
            .map(|stacks| stacks.first().map(|stack| stack.count).unwrap_or(0))
            .unwrap_or(0);
        let jukebox_playing = jukebox_count > 0 && self.level.is_jukebox_playing(pos);
        if !self.level.write_block(pos, state, flags) {
            return None;
        }

        // 换了方块，官方把方块实体一起删掉：先跑 remove 之前的副作用，再 remove。
        // 新状态自己声明要留下实体时（铜箱子这些）两步都跳过。
        let mut entity_dropped_contents = false;
        if block_changed && old.has_block_entity() && !block_entity::keeps_block_entity(state, old)
        {
            if flags & UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS == 0 && self.has_block_entity(pos) {
                // 活塞的 `finalTick`、熔炉和酿造台的掉落都在实体自己的回调里。
                // Piston `finalTick`, and furnace or brewing drops, run on the entity.
                self.pre_remove_block_entity(pos, old);
                entity_dropped_contents = block_entity::drops_own_contents(old);
            }
            self.remove_block_entity(pos);
        }
        // 比较器存的输出这些也跟着没了。
        if block_changed {
            self.level.forget_block_entity_memory(pos);
        }
        if jukebox_count > 0 {
            self.level.clear_container(pos);
            // `JukeboxSongPlayer.stop`：正在放才报 1011，然后 `onSongChanged`。两个 `nextFloat` 在这之后。
            // `JukeboxSongPlayer.stop`: 1011 only while a song is playing, then `onSongChanged`.
            // The two `nextFloat`s come after that.
            if jukebox_playing {
                self.level_event(1011, pos, 0);
                crate::block_container::on_jukebox_song_changed(old, self, pos);
            }
        }
        let moved_by_piston = flags & UPDATE_MOVE_BY_PISTON != 0;
        // 官方的方块实体在 `affectNeighborsAfterRemoval` 之前掉落，顺序影响取数。
        // 熔炉和酿造台有实体时已经在上面掉过，这里再掉一次随机数会翻倍。唱片机不在那一组里，
        // 弹出唱片的两个 `nextFloat` 仍走下面的 `pre_remove_side_effects`。
        //
        // Vanilla drops before `affectNeighborsAfterRemoval`. A furnace or brewing stand
        // that still had an entity already dropped above; dropping again would double the draws.
        // A jukebox is not in that group, so popping the disc still runs below.
        if let Some(stacks) = dropped_stacks
            && !entity_dropped_contents
        {
            block_entity::pre_remove_side_effects(old, self, pos, &stacks);
        }
        if jukebox_count > 0 {
            // `popOutTheItem` 末尾的 `onSongChanged`，在 `setRemoved` 的 1011 之前。
            // The `onSongChanged` at the end of `popOutTheItem`, before `setRemoved`'s 1011.
            crate::block_container::on_jukebox_song_changed(old, self, pos);
        }
        // `removeBlockEntity` 不看跳过副作用的标志，`setRemoved` 一定会走。
        // `removeBlockEntity` ignores the skip-side-effects flag, so `setRemoved` always runs.
        if block_changed && old.has_block_entity() && !block_entity::keeps_block_entity(state, old)
        {
            block_entity::removed_side_effects(old, self, pos);
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

    /// `BlockEntity.preRemoveSideEffects`。实体在回调期间不在表里。要读它的比较器检查先排着，
    /// 放回去之后、调用方 `remove` 之前跑完。
    ///
    /// The entity is out of the map for the callback. Comparator checks that need it stay queued
    /// and run after it is put back, before the caller's `remove`.
    fn pre_remove_block_entity(&mut self, pos: BlockPos, state: BlockState) {
        let Some((id, mut entity)) = self.level.take_block_entity_at(pos) else {
            return;
        };
        self.begin_held_reads();
        entity.pre_remove_side_effects(self, pos, state);
        self.level.put_block_entity(pos, id, entity);
        self.end_held_reads();
    }

    /// `removeBlock(pos, false)`：换成这一格的流体，没有流体时是空气。
    pub fn remove_block(&mut self, pos: BlockPos) -> bool {
        let fluid = self.fluid_state(pos);
        self.set_block(pos, fluid.legacy_block(), UPDATE_ALL)
    }

    /// `Level.destroyBlock(pos, dropResources, null, updateLimit)`。
    ///
    /// 官方顺序是取状态、破碎粒子、`dropResources` 为真的 `Block.dropResources`
    /// （先 `getDrops` 再一组一组 `popResource`，最后 `spawnAfterBreak`），
    /// 然后才换成这一格的流体方块。掉落必须排在改方块之前，否则邻居更新
    /// 和后续计划刻拿到的随机数会和官方错开。方块 oracle 把这些表换成空表。
    ///
    /// `Level.destroyBlock(pos, dropResources, null, updateLimit)`: state, break
    /// particles, then `Block.dropResources` when requested (getDrops, popResource
    /// for each stack, spawnAfterBreak), and only then the fluid's legacy block.
    /// Drops precede the block write so neighbour updates consume the same random draws.
    /// The block oracle empties those loot tables.
    pub fn destroy_block(&mut self, pos: BlockPos, drop: bool, limit: u32) -> bool {
        let state = self.block_state(pos);
        if state.is_air() {
            return false;
        }
        state.spawn_destroy_particles(self, pos);
        if drop {
            for stack in self.block_loot_drops(state, pos) {
                self.pop_resource(pos, stack);
            }
            self.spawn_after_break_random(state, pos);
        }
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
            // 官方 `updateOrDestroy` 的 `flags & 32` 会关掉掉落。
            // Official `updateOrDestroy` disables drops when `flags & 32` is set.
            self.destroy_block(pos, flags & UPDATE_SUPPRESS_DROPS == 0, limit);
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
    ///
    /// 这一格的实体正被拿着时先排进队列。放回去再跑，比较器读到的是放回去之后的那一份。
    ///
    /// While this cell's entity is out of the map the call is queued. It runs after the put, so
    /// the comparator reads that put-back value.
    pub fn update_neighbour_for_output_signal(&mut self, pos: BlockPos, block: &'static Block) {
        // 比较器读容器内容时官方先 `unpackLootTable(null)`，见容器的
        // `getItem` / `isEmpty`。这条通知会触发那次读，解包要排在前面。
        // A comparator reads `getItem` / `isEmpty`, which unpacks the loot table first.
        // This notification triggers that read, so unpack before it.
        self.level.unpack_loot_table(pos);
        if self.block_entity_is_taken(pos) {
            self.defer_block_entity_read(DeferredRead::OutputSignal { pos, block });
            return;
        }
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

    /// 官方 `RandomizableContainer.unpackLootTable`：破坏容器前把表填进内容。
    ///
    /// Vanilla `RandomizableContainer.unpackLootTable`: fill the table before a
    /// container's contents are dropped.
    pub(crate) fn unpack_container_loot(&mut self, pos: BlockPos) {
        self.level.unpack_loot_table(pos);
    }

    /// 官方 `ShulkerBoxBlock.playerWillDestroy` 的创造模式分支：生成带组件的内容。
    ///
    /// The creative branch of vanilla `ShulkerBoxBlock.playerWillDestroy`.
    pub(crate) fn creative_container_drop(
        &mut self,
        pos: BlockPos,
        item: i32,
    ) -> Option<mc_bamboo_protocol::item::ItemStack> {
        self.level.creative_container_drop(pos, item)
    }

    /// `addFreshEntity`：放进世界，返回实体 ID。tick 的时候生成的实体下一 tick 才开始动。
    pub fn spawn_entity(&mut self, entity: Entity) -> i32 {
        self.level.spawn_entity(entity)
    }

    /// `FallingBlockEntity.fall`：这一格只剩流体，方块本身变成实体从这一格的中心开始掉。
    ///
    /// 生成时 `WATERLOGGED` 要清掉，落地时再按那一格的水重新设。
    pub fn spawn_falling_block(&mut self, pos: BlockPos, state: BlockState) -> i32 {
        let falling = if state.has(WATERLOGGED) {
            state.with(WATERLOGGED, false)
        } else {
            state
        };
        let entity = Entity::falling_block(pos, falling);
        self.set_block_and_update(pos, state.fluid().legacy_block());
        self.spawn_entity(entity)
    }

    /// `TntBlock.prime`：在这一格生成一个默认引信的 TNT。方块本身的移除由调用方负责。
    ///
    /// 朝向取自 `level.getRandom()`，和方块逻辑共用同一个随机源。
    ///
    /// `TntBlock.prime`: spawns a primed TNT with the default fuse. The caller removes the block.
    /// The yaw comes from `level.getRandom()`, the same source block logic uses.
    pub fn spawn_tnt(&mut self, pos: BlockPos) -> i32 {
        // `TntBlock.prime` 在规则为假时直接返回，不取朝向的随机数。
        //
        // `TntBlock.prime` returns before drawing the yaw when the rule is false.
        if !self.block_game_rules().tnt_explodes {
            return -1;
        }
        let position = block_center_bottom(pos);
        let angle = self.random().next_double();
        let entity = Entity::primed_tnt(position, angle);
        let id = self.spawn_entity(entity);
        // `TntBlock.prime` 在 `addFreshEntity` 之后，于实体脚底播放。被爆炸点燃的短引信不走这里。
        // `TntBlock.prime` plays this at the entity's feet after `addFreshEntity`.
        // Short-fuse TNT lit by an explosion does not.
        self.play_sound(
            position,
            sounds::sound_event("minecraft:entity.tnt.primed"),
            sounds::BLOCKS,
            1.0,
            1.0,
            false,
        );
        id
    }

    /// `TntBlock.wasExploded`：被爆炸点着的 TNT 引信比默认的短一截。
    ///
    /// `TntBlock.wasExploded`: TNT lit by an explosion gets a shorter fuse.
    pub fn spawn_short_fuse_tnt(&mut self, pos: BlockPos) -> i32 {
        // `TntBlock.wasExploded` 先看 `TNT_EXPLODES`，关着时不取短引信的随机数。
        //
        // `TntBlock.wasExploded` checks `TNT_EXPLODES` first and draws no short-fuse random when it is off.
        if !self.block_game_rules().tnt_explodes {
            return -1;
        }
        let angle = self.random().next_double();
        let mut entity = Entity::primed_tnt(block_center_bottom(pos), angle);
        let fuse = short_fuse(PrimedTnt::DEFAULT_FUSE, self.random());
        entity.set_fuse(fuse);
        self.spawn_entity(entity)
    }

    /// `addFreshEntity` 之后按 ID 改这个实体。刚生成、还没并进 tick 列表的也找得到。
    pub fn with_entity_mut(&mut self, id: i32, f: &mut dyn FnMut(&mut Entity)) -> bool {
        self.level.with_just_spawned(id, f)
    }

    /// 已经在 tick 列表里的实体。正在 tick 的那一个不在里面。
    ///
    /// An entity already in the tick list. The one currently ticking is not in it.
    pub fn entity(&self, id: i32) -> Option<Entity> {
        self.level.entity(id)
    }

    /// 这一拍会 tick 的实体 ID，按 ID 升序。
    ///
    /// Entity ids that tick this step, in ascending id order.
    pub fn entity_ids(&self) -> Vec<i32> {
        self.level.entity_uids()
    }

    /// 从 tick 列表里拿掉。正在 tick 的实体只标记 `discard`，由 tick 循环丢掉。
    ///
    /// Removes an entity from the tick list. The entity currently ticking is only marked `discard`.
    pub fn remove_entity(&mut self, id: i32) {
        self.level.remove_entity(id);
    }

    /// `EntityTickList.forEach`：一个 tick 里所有实体按 ID 顺序各 tick 一次。
    ///
    /// 实体 tick 期间新生成的实体进 `pending`，下一 tick 才动。
    pub fn tick_entities(&mut self) {
        self.level.begin_entity_tick();
        for id in self.level.entity_uids() {
            let Some(mut entity) = self.level.take_entity(id) else {
                continue;
            };
            // 所在区块不在实体 tick 范围时，官方跳过这一刻；实体仍留在世界里，
            // 别的实体照常查得到它，位置和年龄都不动。
            let ticks = !entity.removed
                && self.level.chunk_ticks_entities(ChunkPos::containing(
                    entity.position.x,
                    entity.position.z,
                ));
            // 这一拍已经被别的实体丢掉的，官方不再 tick，名单里也不再留下。
            // An entity another one discarded earlier this tick is not ticked. Vanilla drops it from the list.
            if ticks {
                let mut ctx = crate::entity::ctx::PreciseCtx::new(self, id);
                entity.tick(&mut ctx);
            }
            if !entity.removed {
                self.level.put_entity(entity);
            }
        }
        self.level.end_entity_tick();
    }

    /// 和 [`World::tick_entities`] 同一套顺序，但把这一拍 tick 过的实体留下。
    ///
    /// 已经 `discard` 的也在里面，后面是这一拍新生成、还没 tick 的。实体物理对照要拿落地后的位置。
    ///
    /// Same order as [`World::tick_entities`], but the entities ticked this step are kept.
    /// Discarded ones stay in the list; entities spawned this step and not yet ticked follow them.
    /// The physics comparison needs the position after landing.
    pub fn tick_entities_kept(&mut self) -> Vec<crate::entity::Entity> {
        self.level.begin_entity_tick();
        let mut after = Vec::new();
        for id in self.level.entity_uids() {
            let Some(mut entity) = self.level.take_entity(id) else {
                continue;
            };
            // 合并把对方丢掉之后，对方这一拍还在快照里，但是不再 tick。
            // After a merge discards the other item, it stays in this tick's snapshot and does not tick.
            // 和 `tick_entities` 一样跳过不在实体 tick 范围的区块。
            let ticks = !entity.removed
                && self.level.chunk_ticks_entities(ChunkPos::containing(
                    entity.position.x,
                    entity.position.z,
                ));
            if ticks {
                let mut ctx = crate::entity::ctx::PreciseCtx::new(self, id);
                entity.tick(&mut ctx);
            }
            after.push(entity.clone());
            if !entity.removed {
                self.level.put_entity(entity);
            }
        }
        self.level.end_entity_tick();
        let spawned: Vec<_> = self
            .level
            .live_entities()
            .into_iter()
            .filter(|entity| after.iter().all(|ticked| ticked.id != entity.id))
            .collect();
        after.extend(spawned);
        after
    }

    /// 还在世界里的实体。已经 `discard` 的不在里面。
    ///
    /// Entities still in the world. Discarded ones are absent.
    pub fn live_entities(&self) -> Vec<crate::entity::Entity> {
        self.level.live_entities()
    }

    /// `CollisionGetter.getBlockCollisions`：和 `box_` 碰上的方块的碰撞形状，按 `Cursor3D` 的顺序。
    ///
    /// 返回的形状已经是世界坐标。顺序有影响：官方在运动被完全挡住后就不再往后看。
    ///
    /// `CollisionGetter.getBlockCollisions`: collision shapes that meet `box_`, in `Cursor3D` order.
    ///
    /// The shapes are in world coordinates. Order matters: vanilla stops looking once movement is fully blocked.
    pub(crate) fn block_collisions(
        &self,
        box_: shape::Aabb,
        context: CollisionContext,
    ) -> Vec<(BlockPos, Vec<shape::Aabb>)> {
        collect_block_collisions(box_, context, |pos| self.block_state(pos))
    }

    /// `CollisionGetter.findSupportingBlock`：脚下这一片里离实体中心最近的方块。
    ///
    /// 距离一样时取坐标大的那个，`Vec3i.compareTo` 先比 y 再比 z 最后比 x。
    pub(crate) fn find_supporting_block(
        &self,
        position: Vec3,
        box_: shape::Aabb,
        context: CollisionContext,
    ) -> Option<BlockPos> {
        let mut main = None;
        let mut main_distance = f64::MAX;
        for (pos, _) in self.block_collisions(box_, context) {
            let center = Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + 0.5,
                f64::from(pos.z) + 0.5,
            );
            let distance = position.distance_to_sqr(center);
            let larger = main.is_none_or(|main: BlockPos| compare_positions(main, pos).is_lt());
            if distance < main_distance || distance == main_distance && larger {
                main = Some(pos);
                main_distance = distance;
            }
        }
        main
    }

    /// `ClipContext.Block.FALLDAMAGE_RESETTING` 加 `ClipContext.Fluid.WATER` 的射线。
    ///
    /// 只用它判断碰没碰到，落到哪一格不要。
    pub(crate) fn clip_fall_damage_resetting(&self, from: Vec3, to: Vec3) -> bool {
        const FULL: [shape::Aabb; 1] = [shape::Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)];
        traverse_blocks(from, to, |pos| {
            let state = self.block_state(pos);
            let block_hit =
                if state.is_tagged(mc_bamboo_data::tags::block_tags::FALL_DAMAGE_RESETTING) {
                    shape::clip(&FULL, from, to, pos)
                } else {
                    None
                };
            let fluid = state.fluid();
            let fluid_hit = if fluid.is_water() && !fluid.is_empty() {
                let bounds =
                    shape::Aabb::new(0.0, 0.0, 0.0, 1.0, f64::from(fluid.own_height()), 1.0);
                shape::clip(&[bounds], from, to, pos)
            } else {
                None
            };
            let distance = |hit: Option<BlockHit>| {
                hit.map_or(f64::MAX, |hit| from.distance_to_sqr(hit.location))
            };
            if distance(block_hit) <= distance(fluid_hit) {
                block_hit.map(|_| ())
            } else {
                fluid_hit.map(|_| ())
            }
        })
        .is_some()
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
            let state = self.block_state(pos);
            let outline = outline_seen_holding(state, pos, main_hand);
            self.clip_one_block(pos, from, to, &outline, state, fluid)
        })
    }

    /// `ClipContext.Block.COLLIDER` 加 `Fluid.NONE`。爆炸的遮挡采样用它。
    ///
    /// `ClipContext.Block.COLLIDER` with `Fluid.NONE`. Explosion exposure samples use it.
    pub(crate) fn clip_collider(
        &self,
        from: Vec3,
        to: Vec3,
        context: CollisionContext,
    ) -> Option<BlockHit> {
        traverse_blocks(from, to, |pos| {
            let state = self.block_state(pos);
            let collider = shape::collision_boxes(state, pos, context);
            self.clip_one_block(pos, from, to, &collider, state, ClipFluid::None)
        })
    }

    /// `ClipContext.Block.COLLIDER` 加 `Fluid.SOURCE_ONLY` 的射线，混凝土粉末掉进水里时用。
    pub(crate) fn clip_collider_source_water(
        &self,
        from: Vec3,
        to: Vec3,
        context: CollisionContext,
    ) -> Option<BlockHit> {
        traverse_blocks(from, to, |pos| {
            let state = self.block_state(pos);
            let collider = shape::collision_boxes(state, pos, context);
            self.clip_one_block(pos, from, to, &collider, state, ClipFluid::SourceOnly)
        })
    }

    fn clip_one_block(
        &self,
        pos: BlockPos,
        from: Vec3,
        to: Vec3,
        boxes: &[shape::Aabb],
        state: BlockState,
        fluid: ClipFluid,
    ) -> Option<BlockHit> {
        let block_hit = clip_with_interaction_override(boxes, state, from, to, pos);
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

/// `Vec3i.compareTo`：先比 y，再比 z，最后比 x。
fn compare_positions(first: BlockPos, second: BlockPos) -> std::cmp::Ordering {
    (first.y, first.z, first.x).cmp(&(second.y, second.z, second.x))
}

/// 方块的底心，官方生成实体的位置：`pos.getX() + 0.5, pos.getY(), pos.getZ() + 0.5`。
fn block_center_bottom(pos: BlockPos) -> Vec3 {
    Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    )
}

/// `PrimedTnt.getRandomShortFuse`。
///
/// `PrimedTnt.getRandomShortFuse`.
fn short_fuse(fuse: i32, random: &crate::random::LegacyRandom) -> i32 {
    random.next_int((fuse / 4).max(1)) + fuse / 8
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
pub(crate) fn traverse_blocks<T>(
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
    use std::any::Any;

    use mc_bamboo_data::items;
    use mc_bamboo_protocol::item::ItemStack;

    use super::*;
    use crate::{
        block_entity::BlockEntity,
        state::HORIZONTAL_FACING,
        testing::{TestLevel, break_block},
    };

    /// 数自己被 tick 了几次的测试方块实体，用来验证冻结时留在表里、顺序不变。
    struct Counter {
        pos: BlockPos,
        ticks: i32,
    }

    impl Counter {
        fn boxed(pos: BlockPos) -> Box<dyn BlockEntity> {
            Box::new(Self { pos, ticks: 0 })
        }
    }

    impl BlockEntity for Counter {
        fn pos(&self) -> BlockPos {
            self.pos
        }

        fn tick(&mut self, _world: &mut World) {
            self.ticks += 1;
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }

        // 测试用的计数器没有要存盘的内容。
        fn write_additional(
            &self,
        ) -> Result<Vec<(String, mc_bamboo_nbt::Tag)>, crate::archive::Error> {
            Ok(Vec::new())
        }
    }

    const CENTER: BlockPos = BlockPos::new(0, 64, 0);

    fn wall_torch_on(side: Direction) -> BlockState {
        BlockState(blocks::WALL_TORCH).with(HORIZONTAL_FACING, side)
    }

    fn with_world<T>(level: &mut TestLevel, run: impl FnOnce(&mut World) -> T) -> T {
        let mut ticks = BlockTicks::new();
        let mut world = World::new(level, &mut ticks);
        run(&mut world)
    }

    /// 冻结区块里的方块实体不 tick，留在表里、运行顺序不变；解冻后照常接着跑。
    #[test]
    fn frozen_chunks_skip_block_entity_tickers_without_changing_order() {
        let mut level = TestLevel::new();
        let a = BlockPos::new(1, 64, 0);
        let b = BlockPos::new(2, 64, 0);
        let c = BlockPos::new(20, 64, 0);
        for pos in [a, b, c] {
            level.set(pos, BlockState(blocks::CHEST).default_state());
            with_world(&mut level, |world| {
                world.set_block_entity(Counter::boxed(pos));
            });
        }
        level.set_frozen(ChunkPos::new(0, 0), true);

        // 冻结时只有 c 跑；冻结的两个一次都没动，状态不变。
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.tick_block_entities();
        assert_eq!(world.block_entity::<Counter>(a).unwrap().ticks, 0);
        assert_eq!(world.block_entity::<Counter>(b).unwrap().ticks, 0);
        assert_eq!(world.block_entity::<Counter>(c).unwrap().ticks, 1);
        assert!(world.has_block_entity(a) && world.has_block_entity(b));
        drop(world);

        level.set_frozen(ChunkPos::new(0, 0), false);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.tick_block_entities();
        // 三个 ticker 都还在原来的表里，按原顺序各跑一次。
        assert_eq!(world.block_entity::<Counter>(a).unwrap().ticks, 1);
        assert_eq!(world.block_entity::<Counter>(b).unwrap().ticks, 1);
        assert_eq!(world.block_entity::<Counter>(c).unwrap().ticks, 2);
    }

    fn destroyed_positions(effects: &[Effect]) -> Vec<BlockPos> {
        effects
            .iter()
            .filter_map(|effect| match *effect {
                Effect::Destroyed { pos, .. } => Some(pos),
                Effect::BoneMealUsed { .. }
                | Effect::TorchBurnout { .. }
                | Effect::FilledBucket { .. }
                | Effect::EmptiedBucket { .. }
                | Effect::SignChanged { .. }
                | Effect::SignWaxed { .. }
                | Effect::OpenSignEditor { .. }
                | Effect::Sound { .. }
                | Effect::LevelEvent { .. }
                | Effect::NoteBlockPlayed { .. }
                | Effect::BlockEvent { .. }
                | Effect::ComposterFill { .. }
                | Effect::PageTurned { .. }
                | Effect::DispenserEquipped { .. }
                | Effect::CampfireChanged { .. }
                | Effect::Overlay { .. }
                | Effect::OpenCommandBlock { .. } => None,
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

    /// 官方 `Level.destroyBlock` 的掉落和 `spawnAfterBreak` 都在写方块之前。
    #[test]
    fn drops_precede_the_block_write() {
        let mut level = TestLevel::new();
        level.set(CENTER, BlockState(blocks::STONE));
        level.loot = vec![ItemStack::new(items::STONE, 2)];
        let dropped = with_world(&mut level, |world| {
            world.destroy_block(CENTER, true, UPDATE_LIMIT)
        });
        assert!(dropped);
        // 两次 "drops"/"after_break" 都看到 0 次写入，最后的 "write" 才看到 1 次。
        assert_eq!(level.drop_order.len(), 3);
        assert_eq!(level.drop_order[0].0, "drops");
        assert_eq!(level.drop_order[1].0, "after_break");
        assert_eq!(level.drop_order[2].0, "write");
        assert!(level.drop_order[..2].iter().all(|(_, writes)| *writes == 0));
        assert_eq!(level.drop_order[2].1, 1);
        // 掉落堆变成一个物品实体，三个位置 double 照取。
        assert_eq!(level.entities().len(), 1);
        assert_eq!(
            level
                .entities()
                .iter()
                .next()
                .and_then(|entity| entity.ground().cloned())
                .map(|item| item.stack),
            Some(ItemStack::new(items::STONE, 2))
        );
    }

    /// `dropResources` 为假时既不算掉落也不调 `spawnAfterBreak`，但仍写方块。
    #[test]
    fn destroy_without_drops_skips_the_loot_hooks() {
        let mut level = TestLevel::new();
        level.set(CENTER, BlockState(blocks::STONE));
        level.loot = vec![ItemStack::new(items::STONE, 2)];
        assert!(with_world(&mut level, |world| {
            world.destroy_block(CENTER, false, UPDATE_LIMIT)
        }));
        assert_eq!(level.drop_order, [("write", 1)]);
        assert!(level.entities().is_empty());
    }

    /// `Block.updateOrDestroy` 的 `UPDATE_SUPPRESS_DROPS` 关掉掉落。
    #[test]
    fn suppress_drops_flag_skips_the_loot_hooks() {
        let mut level = TestLevel::new();
        level.set(CENTER, BlockState(blocks::STONE));
        with_world(&mut level, |world| {
            world.update_or_destroy(
                BlockState(blocks::STONE),
                BlockState::AIR,
                CENTER,
                UPDATE_ALL | UPDATE_SUPPRESS_DROPS,
                UPDATE_LIMIT,
            );
        });
        assert_eq!(level.drop_order, [("write", 1)]);
        assert!(level.entities().is_empty());
    }

    #[test]
    fn destroying_air_does_nothing() {
        let mut level = TestLevel::new();
        let destroyed = with_world(&mut level, |world| {
            world.destroy_block(CENTER, true, UPDATE_LIMIT)
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
    /// 同时生成物品实体。随机数一个都不能少，否则后面的随机刻会错开。
    ///
    /// Breaking a container draws those numbers and spawns the item entities.
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
            // 一件和三件各成一堆，空槽不生成实体。
            // One item and three items each become one entity. Empty slots do not.
            assert_eq!(world.entity_ids().len(), 2);
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
