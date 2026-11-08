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

    /// 见 [`BlockEntities::get`]，拿的是可变引用。实体正在 tick 时不在表里，返回 `None`。
    pub fn get_mut<T: BlockEntity>(&mut self, pos: BlockPos) -> Option<&mut T> {
        self.entities
            .get_mut(&pos)?
            .entity
            .as_any_mut()
            .downcast_mut()
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockEvent {
    pub pos: BlockPos,
    pub block: usize,
    pub param_a: i32,
    pub param_b: i32,
}

/// `Level.blockEvents`：官方的 `LinkedHashSet`，同一项只排一次。
///
/// 队列管顺序，集合管查重，两边始终是同一批事件。出队以后集合里也删掉，
/// 所以和官方一样，处理过的事件同一轮里还能再排进来。
///
/// 官方在方块所在区块没有加载时把事件往后挪。这个世界里队列只碰得到的方块，
/// 不需要重排，见 `ServerLevel.runBlockEvents`。
///
/// Vanilla's `LinkedHashSet`: an event is queued only once. The deque keeps the order and the
/// set answers the duplicate check; both always hold the same events. Popping removes the event
/// from the set too, so, as in vanilla, a processed event can be queued again in the same round.
/// Vanilla postpones events in unloaded chunks; this queue only touches reachable blocks, so
/// nothing is postponed.
#[derive(Debug, Default)]
pub struct BlockEvents {
    queue: VecDeque<BlockEvent>,
    queued: HashSet<BlockEvent>,
}

impl BlockEvents {
    /// `ServerLevel.blockEvent`。
    pub fn push(&mut self, event: BlockEvent) {
        if self.queued.insert(event) {
            self.queue.push_back(event);
        }
    }

    /// 按排队的顺序取出一项。
    pub fn pop(&mut self) -> Option<BlockEvent> {
        let event = self.queue.pop_front()?;
        self.queued.remove(&event);
        Some(event)
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

/// `Containers.dropItemStack` 开头的掉落点：`floor(x/y/z)` 之后各取一个 double。
fn drop_point(world: &mut World) {
    for _ in 0..3 {
        world.random().next_double();
    }
}

/// 一格空的槽位：`dropContents` 对每一格都调用 `dropItemStack`，空堆只取掉落点。
fn drop_empty_slot(world: &mut World) {
    drop_point(world);
}

/// 一格装 `count` 件物品的槽位。掉落点之后，每一堆取一次 `nextInt(21) + 10`
/// （最后一堆不足时只拿走剩下的），再取 `setDeltaMovement` 里三个 `triangle` 的六个 double。
fn drop_stack(world: &mut World, count: i32) {
    drop_point(world);
    let mut left = count;
    while left > 0 {
        let split = (world.random().next_int(21) + 10).min(left);
        left -= split;
        for _ in 0..6 {
            world.random().next_double();
        }
    }
}

/// 一张 `Container` 的槽位表。`counts` 是每格的数量，没有的格子按空槽算。
///
/// 官方 `dropContents` 从第 0 格数到 `getContainerSize() - 1`，空格也照样调用
/// `dropItemStack`，所以槽位数不能省。
fn drop_slots(world: &mut World, slots: i32, counts: &[i32]) {
    for slot in 0..slots {
        match counts.get(slot as usize).copied().unwrap_or(0) {
            0 => drop_empty_slot(world),
            count => drop_stack(world, count),
        }
    }
}

/// `BlockEntity.preRemoveSideEffects`。`counts` 是这一格自己那张物品表每格的数量。
pub(crate) fn pre_remove_side_effects(state: BlockState, world: &mut World, counts: &[i32]) {
    let Some(entity) = state.block().block_entity_type else {
        return;
    };
    use registries::block_entity_type as E;

    match entity {
        // `ChestBlockEntity`、`TrappedChestBlockEntity`（铜箱子也是这个实体）、`BarrelBlockEntity`。
        E::CHEST | E::TRAPPED_CHEST | E::BARREL => drop_slots(world, 27, counts),
        // `AbstractFurnaceBlockEntity`：三格燃料，经验来自空的 `recipesUsed`，不取数。
        E::FURNACE | E::SMOKER | E::BLAST_FURNACE => drop_slots(world, 3, counts),
        E::HOPPER | E::BREWING_STAND => drop_slots(world, 5, counts),
        E::DISPENSER | E::DROPPER | E::CRAFTER => drop_slots(world, 9, counts),
        E::SHELF => drop_slots(world, 3, counts),
        E::DECORATED_POT => drop_slots(world, 1, counts),
        E::CAMPFIRE => drop_slots(world, 4, counts),
        // 书架的 `SLOT_OCCUPIED` 就是它那六格的内容：每格一本书，数量是一。
        E::CHISELED_BOOKSHELF => {
            for slot in SLOT_OCCUPIED {
                if state.get(slot) {
                    drop_stack(world, 1);
                } else {
                    drop_empty_slot(world);
                }
            }
        }
        // `JukeboxBlockEntity.preRemoveSideEffects` → `popOutTheItem`：唱片非空时
        // `offsetRandomXZ` 取两个 `nextFloat`。`LevelChunk.setBlockState` 在
        // `affectNeighborsAfterRemoval` 之前调用，那边不要再取。
        // 潜影盒把 `preRemoveSideEffects` 覆盖成空的。
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

#[cfg(test)]
mod tests {
    use super::*;

    fn event(x: i32, param_b: i32) -> BlockEvent {
        BlockEvent {
            pos: BlockPos::new(x, 64, 0),
            block: 1,
            param_a: 0,
            param_b,
        }
    }

    #[test]
    fn block_events_keep_order_and_skip_queued_duplicates() {
        let mut events = BlockEvents::default();
        events.push(event(1, 0));
        events.push(event(2, 0));
        events.push(event(1, 0));
        // 参数不同就是另一项。
        // Different parameters make a different event.
        events.push(event(1, 3));

        assert_eq!(events.pop(), Some(event(1, 0)));
        // 出队以后同一项可以再排进来，排在最后。
        // Once popped, the same event can be queued again, at the back.
        events.push(event(1, 0));
        assert_eq!(events.pop(), Some(event(2, 0)));
        assert_eq!(events.pop(), Some(event(1, 3)));
        assert_eq!(events.pop(), Some(event(1, 0)));
        assert_eq!(events.pop(), None);
    }
}
