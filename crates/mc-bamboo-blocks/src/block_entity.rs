//! 方块实体和方块事件，对应官方的 `BlockEntity`、`Level.blockEvents` 和方块实体的 ticker 列表。
//!
//! 方块实体挂在某一格上，活得比一次操作长；`mc-bamboo-blocks` 只定义它和存储的规则，
//! 具体的实体在所属的方块模块里。方块事件是活塞这类方块动画的第一步，官方在
//! `ServerLevel.tick` 里统一执行，见 [`World::run_block_events`]。

use std::{
    any::Any,
    collections::{HashMap, HashSet, VecDeque},
};

use mc_bamboo_core::BlockPos;

use crate::{state::BlockState, world::World};

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
