//! 方块实体和方块事件，对应官方的 `BlockEntity`、`Level.blockEvents` 和方块实体的 ticker 列表。
//!
//! 方块实体挂在某一格上，活得比一次操作长；`mc-bamboo-blocks` 只定义它和存储的规则，
//! 具体的实体在所属的方块模块里。方块事件是活塞这类方块动画的第一步，官方在
//! `ServerLevel.tick` 里统一执行，见 [`World::run_block_events`]。
//!
//! 方块实体被移除时官方还会按容器内容掉落物品。这个世界没有物品实体，但掉落要取随机数，
//! 数漏了后面的随机刻和计划刻就全错开，所以 [`pre_remove_side_effects`] 照官方容器补上取数。

use std::{
    any::Any,
    collections::{HashMap, HashSet, VecDeque},
};

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{BlockClass, registries, tags::block_tags};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    state::{
        BlockState, HAS_RECORD, Property, SLOT_0_OCCUPIED, SLOT_1_OCCUPIED, SLOT_2_OCCUPIED,
        SLOT_3_OCCUPIED, SLOT_4_OCCUPIED, SLOT_5_OCCUPIED,
    },
    world::World,
};

/// 一种方块实体，对应官方 `BlockEntity` 和它的 `BlockEntityTicker`。
///
/// `Send` 是因为世界线程以后要把整块 `Region` 交给线程池。
pub trait BlockEntity: Any + Send {
    /// `getBlockPos`。
    fn pos(&self) -> BlockPos;

    /// 每刻运行一次。要删掉自己时调用 [`World::remove_block_entity`]，和官方一样。
    fn tick(&mut self, world: &mut World);

    /// `preRemoveSideEffects`：这一格换成别的方块之前的副作用。
    ///
    /// 官方的顺序是先把新状态写进区块，再调用它，所以这时读到的已经是新状态。
    fn pre_remove_side_effects(&mut self, _world: &mut World, _pos: BlockPos, _state: BlockState) {}

    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// 方块实体和它们的运行顺序。
///
/// 对应官方的 `LevelChunk.blockEntities`、`Level.blockEntityTickers`、
/// `Level.pendingBlockEntityTickers` 和 `LevelChunk.tickersInLevel`。几条规则都照官方：
///
/// - 同一格重新登记时复用列表里的位置，官方 `RebindableTickingBlockEntityWrapper.rebind`。
/// - tick 阶段里新登记的排到下一刻，官方放进 `pendingBlockEntityTickers`。
/// - 被删掉的槽位不再运行，下一刻开头回收。
#[derive(Default)]
pub struct BlockEntities {
    entities: HashMap<BlockPos, Stored>,
    /// 正在 tick、已经从表里拿出来的实体，值是它的编号。
    taken: HashMap<BlockPos, u64>,
    /// tick 里被删掉的编号，放回去的时候直接扔掉。
    removed_while_taken: HashSet<u64>,
    /// 空出来的槽位，见 [`BlockEntities::begin_tick`]。
    slots: Vec<Option<Ticker>>,
    /// 运行顺序，值是 `slots` 的下标。
    order: Vec<usize>,
    /// tick 阶段里登记、等下一刻并入 `order` 的槽位。
    pending: Vec<usize>,
    free: Vec<usize>,
    /// 每一格现在的槽位，官方 `tickersInLevel`。
    positions: HashMap<BlockPos, usize>,
    ticking: bool,
    next: usize,
    next_id: u64,
}

struct Stored {
    id: u64,
    entity: Box<dyn BlockEntity>,
}

#[derive(Clone, Copy)]
struct Ticker {
    pos: BlockPos,
    id: u64,
}

impl BlockEntities {
    /// `Level.setBlockEntity`：放进这一格并登记 ticker。
    ///
    /// 调用方要保证这一格的状态有方块实体，官方也只在这种情况下才放。
    pub fn insert(&mut self, entity: Box<dyn BlockEntity>) {
        let pos = entity.pos();
        let id = self.next_id;
        self.next_id += 1;
        self.entities.insert(pos, Stored { id, entity });

        if let Some(&index) = self.positions.get(&pos) {
            // 同一个位置换了一个实体，包装还在列表里，位置不变。
            self.slots[index] = Some(Ticker { pos, id });
            return;
        }
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                self.slots.push(None);
                self.slots.len() - 1
            }
        };
        self.slots[index] = Some(Ticker { pos, id });
        self.positions.insert(pos, index);
        if self.ticking {
            self.pending.push(index);
        } else {
            self.order.push(index);
        }
    }

    /// `getBlockEntity(pos)` 之后按类型取，类型不对时是 `None`。
    pub fn get<T: BlockEntity>(&self, pos: BlockPos) -> Option<&T> {
        self.entities.get(&pos)?.entity.as_any().downcast_ref()
    }

    /// 可变版本，菜单点击时直接改熔炉、酿造台的内容。
    pub fn get_mut<T: BlockEntity>(&mut self, pos: BlockPos) -> Option<&mut T> {
        self.entities.get_mut(&pos)?.entity.as_any_mut().downcast_mut()
    }

    /// 这一格有没有方块实体。
    pub fn contains(&self, pos: BlockPos) -> bool {
        self.entities.contains_key(&pos)
    }

    /// `removeBlockEntity(pos)` 加上 `setRemoved`。ticker 槽位在下一次 tick 开头回收。
    ///
    /// 实体正在 tick 时（已经从表里拿出来）调用它表示把自己删掉：返回值是 `None`，
    /// 放回去的时候会扔掉。
    pub fn remove(&mut self, pos: BlockPos) -> Option<Box<dyn BlockEntity>> {
        if let Some(stored) = self.entities.remove(&pos) {
            return Some(stored.entity);
        }
        if let Some(id) = self.taken.remove(&pos) {
            self.removed_while_taken.insert(id);
        }
        None
    }

    /// 把这一格的实体拿出来，不管编号。调用方用完要用 [`BlockEntities::put`] 放回去。
    pub fn take_at(&mut self, pos: BlockPos) -> Option<(u64, Box<dyn BlockEntity>)> {
        let stored = self.entities.remove(&pos)?;
        self.taken.insert(pos, stored.id);
        Some((stored.id, stored.entity))
    }

    /// 把这一格正在 tick 的实体拿出来，编号对不上时不动它。
    pub fn take(&mut self, pos: BlockPos, id: u64) -> Option<Box<dyn BlockEntity>> {
        if self.entities.get(&pos).is_none_or(|stored| stored.id != id) {
            return None;
        }
        self.take_at(pos).map(|(_, entity)| entity)
    }

    /// 把 [`BlockEntities::take`] 拿出来的实体放回去。它在这期间被删掉或者被顶替时扔掉。
    pub fn put(&mut self, pos: BlockPos, id: u64, entity: Box<dyn BlockEntity>) {
        self.taken.remove(&pos);
        if self.removed_while_taken.remove(&id) || self.entities.contains_key(&pos) {
            return;
        }
        self.entities.insert(pos, Stored { id, entity });
    }

    /// `Level.tickBlockEntities` 的开头：并进上一刻新登记的，回收已经删掉的槽位。
    pub fn begin_tick(&mut self) {
        debug_assert!(!self.ticking);
        self.ticking = true;
        self.next = 0;

        let previous = std::mem::take(&mut self.order);
        let pending = std::mem::take(&mut self.pending);
        let mut order = Vec::with_capacity(previous.len() + pending.len());
        for index in previous.into_iter().chain(pending) {
            if self.is_live(index) {
                order.push(index);
                continue;
            }
            if let Some(ticker) = self.slots[index]
                && self.positions.get(&ticker.pos) == Some(&index)
            {
                self.positions.remove(&ticker.pos);
            }
            self.slots[index] = None;
            self.free.push(index);
        }
        self.order = order;
    }

    /// 下一个要运行的 ticker。遍历期间删除的槽位只是跳过，下一次 [`BlockEntities::begin_tick`] 回收。
    pub fn next_ticker(&mut self) -> Option<(BlockPos, u64)> {
        while self.next < self.order.len() {
            let index = self.order[self.next];
            self.next += 1;
            let Some(Some(ticker)) = self.slots.get(index).copied() else {
                continue;
            };
            if self
                .entities
                .get(&ticker.pos)
                .is_some_and(|stored| stored.id == ticker.id)
            {
                return Some((ticker.pos, ticker.id));
            }
        }
        None
    }

    /// `Level.tickBlockEntities` 的结尾。
    pub fn end_tick(&mut self) {
        self.ticking = false;
    }

    fn is_live(&self, index: usize) -> bool {
        let Some(Some(ticker)) = self.slots.get(index) else {
            return false;
        };
        self.entities
            .get(&ticker.pos)
            .is_some_and(|stored| stored.id == ticker.id)
    }
}

/// `BlockEventData`：`level.blockEvent` 排进队列的一项。方块是注册表 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockEvent {
    pub pos: BlockPos,
    pub block: usize,
    pub param_a: i32,
    pub param_b: i32,
}

/// `Level.blockEvents`：官方的 `LinkedHashSet`，同一项只排一次。
///
/// 官方在方块所在区块没有加载时把事件往后挪。这个世界里队列只碰得到的方块，
/// 不需要重排，见 `ServerLevel.runBlockEvents`。
#[derive(Debug, Default)]
pub struct BlockEvents {
    queue: VecDeque<BlockEvent>,
}

impl BlockEvents {
    /// `ServerLevel.blockEvent`。
    pub fn push(&mut self, event: BlockEvent) {
        if !self.queue.contains(&event) {
            self.queue.push_back(event);
        }
    }

    /// 按排队的顺序取出一项。
    pub fn pop(&mut self) -> Option<BlockEvent> {
        self.queue.pop_front()
    }
}

/// `ChiseledBookShelfBlock.SLOT_OCCUPIED_PROPERTIES`。
const SLOT_OCCUPIED: [Property<bool>; 6] = [
    SLOT_0_OCCUPIED,
    SLOT_1_OCCUPIED,
    SLOT_2_OCCUPIED,
    SLOT_3_OCCUPIED,
    SLOT_4_OCCUPIED,
    SLOT_5_OCCUPIED,
];

/// `EntityType.ITEM` 注册时的宽高，`Containers.dropItemStack` 用宽度算掉落点的抖动范围。
const ITEM_ENTITY_SIZE: f64 = 0.25;
/// `ItemEntity.setDeltaMovement` 里三个 `triangle` 的分散度。
const ITEM_SPREAD: f64 = 0.114_850_001_711_398_36;

/// `Containers.dropItemStack`：把一堆物品掉在方块角上。
///
/// 世界里没有物品实体，但掉落点和速度的随机数照抽：空堆取三个 `nextDouble`，
/// 每轮分堆取一个 `nextInt(21)`，再取三个 `triangle`（每个两次 `nextDouble`）。
/// `stack` 按官方的 `split` 语义在这里被分完，调用方传副本。
pub(crate) fn drop_item_stack(world: &mut World, pos: BlockPos, stack: &ItemStack) {
    let center_range = 1.0 - ITEM_ENTITY_SIZE;
    let half_size = ITEM_ENTITY_SIZE / 2.0;
    let _x = f64::from(pos.x) + world.random().next_double() * center_range + half_size;
    let _y = f64::from(pos.y) + world.random().next_double() * center_range;
    let _z = f64::from(pos.z) + world.random().next_double() * center_range + half_size;

    let mut stack = stack.clone();
    while !stack.is_empty() {
        let count = world.random().next_int(21) + 10;
        stack.count -= count.min(stack.count);
        if stack.count <= 0 {
            stack = ItemStack::empty();
        }
        // `new Vec3(random.triangle(0.0, ...), random.triangle(0.2, ...), random.triangle(0.0, ...))`
        world.random().triangle(0.0, ITEM_SPREAD);
        world.random().triangle(0.2, ITEM_SPREAD);
        world.random().triangle(0.0, ITEM_SPREAD);
    }
}

/// `Containers.dropItemStack` 在堆是空的时候取的三对 double：掉落点的 `x/y/z`。
fn drop_empty_stack(world: &mut World) {
    for _ in 0..3 {
        world.random().next_double();
    }
}

/// 一格空的槽位：`dropContents` 对每一格都调用 `dropItemStack`，空堆只取掉落点。
fn drop_empty_slot(world: &mut World) {
    drop_empty_stack(world);
}

/// 一本 `chiseled_bookshelf` 里的书：掉落点之后，`split(nextInt(21) + 10)` 一次就分完，
/// 再是 `setDeltaMovement` 里的三个 `triangle`，每个取两个 double。
fn drop_book(world: &mut World) {
    drop_empty_stack(world);
    world.random().next_int(21);
    for _ in 0..6 {
        world.random().next_double();
    }
}

/// 官方容器实体的 `getContainerSize`，内容为空时槽位一个不少。
fn drop_empty_slots(world: &mut World, slots: i32) {
    for _ in 0..slots {
        drop_empty_slot(world);
    }
}

/// `BlockEntity.preRemoveSideEffects`。
pub(crate) fn pre_remove_side_effects(state: BlockState, world: &mut World) {
    let Some(entity) = state.block().block_entity_type else {
        return;
    };
    use registries::block_entity_type as E;

    match entity {
        // `ChestBlockEntity`、`TrappedChestBlockEntity`（铜箱子也是这个实体）、`BarrelBlockEntity`。
        E::CHEST | E::TRAPPED_CHEST | E::BARREL => drop_empty_slots(world, 27),
        // `AbstractFurnaceBlockEntity`：三格燃料，经验来自空的 `recipesUsed`，不取数。
        E::FURNACE | E::SMOKER | E::BLAST_FURNACE => drop_empty_slots(world, 3),
        E::HOPPER | E::BREWING_STAND => drop_empty_slots(world, 5),
        E::DISPENSER | E::DROPPER | E::CRAFTER => drop_empty_slots(world, 9),
        E::SHELF => drop_empty_slots(world, 3),
        E::DECORATED_POT => drop_empty_slots(world, 1),
        E::CAMPFIRE => drop_empty_slots(world, 4),
        E::CHISELED_BOOKSHELF => {
            for slot in SLOT_OCCUPIED {
                if state.get(slot) {
                    drop_book(world);
                } else {
                    drop_empty_slot(world);
                }
            }
        }
        // `JukeboxBlockEntity.popOutTheItem` 用 `offsetRandomXZ` 取两个 `nextFloat`。
        // 潜影盒把这个方法覆盖成空的。
        E::JUKEBOX if state.get(HAS_RECORD) => {
            world.random().next_float();
            world.random().next_float();
        }
        _ => {}
    }
}

/// `state.shouldChangedStateKeepBlockEntity(old)`：新状态的方块决定旧的方块实体留不留。
pub(crate) fn keeps_block_entity(state: BlockState, old: BlockState) -> bool {
    match state.class() {
        BlockClass::CopperChestBlock | BlockClass::WeatheringCopperChestBlock => {
            old.is_tagged(block_tags::COPPER_CHESTS)
        }
        BlockClass::CopperGolemStatueBlock | BlockClass::WeatheringCopperGolemStatueBlock => {
            old.is_tagged(block_tags::COPPER_GOLEM_STATUES)
        }
        _ => false,
    }
}
