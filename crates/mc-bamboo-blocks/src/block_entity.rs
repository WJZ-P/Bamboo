//! 方块实体和方块事件，对应官方的 `BlockEntity`、`Level.blockEvents` 和方块实体的 ticker 列表。
//!
//! 方块实体挂在某一格上，活得比一次操作长；`mc-bamboo-blocks` 只定义它和存储的规则，
//! 具体的实体在所属的方块模块里。方块事件是活塞这类方块动画的第一步，官方在
//! `ServerLevel.tick` 里统一执行，见 [`World::run_block_events`]。
//!
//! 方块实体被移除时按容器内容生成物品实体。取数次数和官方 `dropContents` 一样，
//! 空槽也要取掉落点，否则后面的随机刻和计划刻会错开。
//!
//! Removing a block entity spawns item entities from its container. The number of draws
//! matches `dropContents`, including the drop point of every empty slot.

use std::{
    any::Any,
    collections::{HashMap, HashSet, VecDeque},
    fmt,
};

use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::{BlockClass, items, registries, sounds::sound_event, tags::block_tags};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    state::{
        BlockState, Property, SLOT_0_OCCUPIED, SLOT_1_OCCUPIED, SLOT_2_OCCUPIED, SLOT_3_OCCUPIED,
        SLOT_4_OCCUPIED, SLOT_5_OCCUPIED,
    },
    ticks::Busy,
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

    /// 这次 `tick` 通知比较器之后还会改物品时返回真。
    /// 驱动把实体放回表、跑完排队的读取，再调用一次 `tick`。
    ///
    /// Returns true when this tick notifies a comparator and then still changes items.
    /// The driver puts the entity back, runs the queued reads, and calls `tick` again.
    fn resume_tick(&self) -> bool {
        false
    }

    /// `preRemoveSideEffects`：这一格换成别的方块之前的副作用。
    ///
    /// 官方的顺序是先把新状态写进区块，再调用它，所以这时读到的已经是新状态。
    fn pre_remove_side_effects(&mut self, _world: &mut World, _pos: BlockPos, _state: BlockState) {}

    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// 官方 `saveAdditional`：写这个方块实体自己的键（不含 `id`、`x/y/z` 和 `components`）。
    ///
    /// 没有默认实现：新加一种方块实体时编译器会要求补上，内容不会悄悄漏存。
    /// 读盘时按 `id` 造回具体类型，见 [`crate::archive::block_entity`]。
    fn write_additional(&self) -> Result<Vec<(String, Tag)>, crate::archive::Error>;
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
    /// 每个区块里可能有方块实体的位置，语义是超集：删掉的格子可以不从里面去掉。
    /// 按区块取出时只走这个集合，不扫整张表。
    by_chunk: HashMap<ChunkPos, HashSet<BlockPos>>,
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
    /// 现在能不能按区块搬方块实体：不在 tick 中途，也没有实体被拿出去还没放回。
    ///
    /// [`Self::take_chunk`] 的前提，可以在动手之前只读地问一声，好让一次要搬几张表的
    /// 调用方先检查全部前提。
    pub fn ready_to_move_chunks(&self) -> bool {
        !self.ticking && self.taken.is_empty() && self.removed_while_taken.is_empty()
    }

    /// `Level.setBlockEntity`：放进这一格并登记 ticker。
    ///
    /// 调用方要保证这一格的状态有方块实体，官方也只在这种情况下才放。
    pub fn insert(&mut self, entity: Box<dyn BlockEntity>) {
        let pos = entity.pos();
        let id = self.next_id;
        self.next_id += 1;
        if self.entities.insert(pos, Stored { id, entity }).is_none() {
            self.by_chunk.entry(pos.chunk()).or_default().insert(pos);
        }

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
    ///
    /// 正在 tick 或被 [`World::with_block_entity`](crate::World::with_block_entity) 拿出来的那一个
    /// 不在表里，这里和 [`BlockEntities::get_dyn`] 都返回 `None`。比较器要读它时先排进队列，放回去再读。
    ///
    /// `getBlockEntity(pos)`, then downcast. The wrong type is `None`.
    ///
    /// An entity that is ticking, or held by [`World::with_block_entity`](crate::World::with_block_entity),
    /// is not in the map, so this returns `None`. Comparator reads are queued and run after it is put back.
    pub fn get<T: BlockEntity>(&self, pos: BlockPos) -> Option<&T> {
        self.entities.get(&pos)?.entity.as_any().downcast_ref()
    }

    /// 见 [`BlockEntities::get`]，不按具体类型取。给 dyn 安全的 [`crate::LevelSideState`] 用，
    /// 向下转型放在调用方。
    ///
    /// See [`BlockEntities::get`] without a concrete type. For the object-safe
    /// [`crate::LevelSideState`]; the caller downcasts.
    pub fn get_dyn(&self, pos: BlockPos) -> Option<&dyn BlockEntity> {
        Some(self.entities.get(&pos)?.entity.as_ref())
    }

    /// 见 [`BlockEntities::get`]，拿的是可变引用。实体正在 tick 时不在表里，返回 `None`。
    ///
    /// See [`BlockEntities::get`], but mutable. Returns `None` while that entity is ticking,
    /// because it is not in the map.
    pub fn get_mut<T: BlockEntity>(&mut self, pos: BlockPos) -> Option<&mut T> {
        self.entities
            .get_mut(&pos)?
            .entity
            .as_any_mut()
            .downcast_mut()
    }

    /// 这一格有没有方块实体。被拿出去、还没被删掉的那一个也算。
    ///
    /// Whether this cell has a block entity. One that was taken out and has not been removed counts.
    pub fn contains(&self, pos: BlockPos) -> bool {
        self.entities.contains_key(&pos) || self.taken.contains_key(&pos)
    }

    /// 这一格的方块实体是不是正被 tick 或 [`World::with_block_entity`](crate::World::with_block_entity) 拿在外面。
    ///
    /// Whether this cell's block entity is out of the map for a tick or
    /// [`World::with_block_entity`](crate::World::with_block_entity).
    pub fn is_taken(&self, pos: BlockPos) -> bool {
        self.taken.contains_key(&pos)
    }

    /// `removeBlockEntity(pos)` 加上 `setRemoved`。ticker 槽位在下一次 tick 开头回收。
    ///
    /// 实体正在 tick 时（已经从表里拿出来）调用它表示把自己删掉：返回值是 `None`，
    /// 放回去的时候会扔掉。删掉之后 [`BlockEntities::contains`] 也不再算这一格。
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

    /// 表里每个还在的方块实体。正在 tick、被拿出来的那一个不在这里。
    ///
    /// Every block entity still in the map. The one taken out for its tick is not included.
    pub fn visit(&self, mut visit: impl FnMut(BlockPos, &dyn BlockEntity)) {
        for (&pos, stored) in &self.entities {
            visit(pos, stored.entity.as_ref());
        }
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

    /// 把 `chunk` 里的方块实体装进盒子搬走。只在 tick 边界调用。
    ///
    /// 走的是和 [`Self::remove`] 完全相同的删除路径：ticker 槽位留到下一次
    /// [`Self::begin_tick`] 统一回收，`order` / `pending` 不动，所以别的区块的
    /// 运行顺序不受影响。正在 tick 或有实体被拿出去还没放回时返回 [`Err`]。
    pub fn take_chunk(&mut self, chunk: ChunkPos) -> Result<ChunkBlockEntities, Busy> {
        if !self.ready_to_move_chunks() {
            return Err(Busy);
        }
        let Some(positions) = self.by_chunk.remove(&chunk) else {
            return Ok(ChunkBlockEntities {
                entities: Vec::new(),
            });
        };
        let mut positions: Vec<BlockPos> = positions.into_iter().collect();
        positions.sort_unstable();
        let mut entities = Vec::with_capacity(positions.len());
        for pos in positions {
            // 索引是超集，已经删掉的位置 `remove` 会落空，跳过即可。
            if let Some(entity) = self.remove(pos) {
                entities.push(entity);
            }
        }
        Ok(ChunkBlockEntities { entities })
    }

    /// 把盒子里的方块实体放回表里。只在 tick 边界调用。
    ///
    /// 等价于按盒子里的顺序逐个 [`Self::insert`]。重新登记的 ticker 追加到运行顺序
    /// 末尾，和官方区块重新加载时把 ticker 加进列表末尾一致。
    /// 调用方保证表里已经没有 `chunk` 的方块实体。
    pub fn insert_chunk(&mut self, entities: ChunkBlockEntities) {
        for entity in entities.entities {
            self.insert(entity);
        }
    }
}

/// 一个区块的全部方块实体，按位置排序。
///
/// 字段不公开：区块束只把它整体搬走，不逐个读内容。
#[derive(Default)]
pub struct ChunkBlockEntities {
    entities: Vec<Box<dyn BlockEntity>>,
}

impl ChunkBlockEntities {
    /// 盒子里的方块实体，按位置排序。存档编码和读回后用。
    pub fn entities(&self) -> &[Box<dyn BlockEntity>] {
        &self.entities
    }

    /// 从按位置排序的实体建盒子，存档解码时用。
    pub fn from_entities(entities: Vec<Box<dyn BlockEntity>>) -> Self {
        Self { entities }
    }

    /// 盒子里有没有方块实体。`is_pristine` 用它判断盒子能不能丢掉。
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

impl fmt::Debug for ChunkBlockEntities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChunkBlockEntities")
            .field("count", &self.entities.len())
            .field(
                "positions",
                &self
                    .entities
                    .iter()
                    .map(|entity| entity.pos())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

const _: fn() = || {
    fn assert_send_debug<T: Send + fmt::Debug>() {}
    assert_send_debug::<ChunkBlockEntities>();
};

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
/// 官方在方块所在区块不能 tick 时把事件收进 `blockEventsToReschedule`，这一轮
/// 全部处理完之后才 `addAll` 放回队列，下一刻再看；见 `ServerLevel.runBlockEvents`。
/// [`BlockEvents::reschedule`] 实现这一步，和官方的 `LinkedHashSet.addAll` 一样，
/// 这一轮里又排进来的同一项不会被覆盖也不会重复。
///
/// Vanilla's `LinkedHashSet`: an event is queued only once. The deque keeps the order and the
/// set answers the duplicate check; both always hold the same events. Popping removes the event
/// from the set too, so, as in vanilla, a processed event can be queued again in the same round.
/// Events in chunks that cannot tick are collected and added back only after the whole round,
/// preserving their order and not duplicating an event queued again during the round.
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

    /// `ServerLevel.runBlockEvents` 末尾的 `blockEvents.addAll(blockEventsToReschedule)`。
    pub fn reschedule(&mut self, events: Vec<BlockEvent>) {
        for event in events {
            self.push(event);
        }
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

/// `Containers.dropItemStack`：生成真的物品实体，取数次数和 `dropContents` 一样。
///
/// 熔炉燃料余料和酿造台余料的取数走这里，方块实体拆除走 [`drop_slots`]，两套一致。
///
/// `Containers.dropItemStack` spawns the item entities. The draws match `dropContents`.
/// Fuel and brewing remainders share this sequence with [`drop_slots`].
pub(crate) fn drop_item_stack(world: &mut World, pos: BlockPos, stack: &ItemStack) {
    let (x, y, z) = (f64::from(pos.x), f64::from(pos.y), f64::from(pos.z));
    world.drop_container_stack(x, y, z, stack.clone());
}

/// 熔炉系列和酿造台的内容在方块实体上，不在区域的容器表里。
///
/// 有实体时由实体的 `preRemoveSideEffects` 掉落；没有实体时仍走按数量的那条。
///
/// Furnace-family and brewing contents live on the block entity, not in the region
/// container table. An entity drops them itself; without one, the stack-based drop runs.
pub(crate) fn drops_own_contents(state: BlockState) -> bool {
    use registries::block_entity_type as E;
    matches!(
        state.block().block_entity_type,
        Some(E::FURNACE | E::SMOKER | E::BLAST_FURNACE | E::BREWING_STAND | E::CAMPFIRE)
    )
}

/// `Containers.dropContents`：从第 0 格到 `size - 1`，空格也调用 `dropItemStack`。
///
/// `Containers.dropContents`: every slot from 0 to `size - 1` calls `dropItemStack`, including empty ones.
fn drop_slots(world: &mut World, pos: BlockPos, slots: i32, stacks: &[ItemStack]) {
    let (x, y, z) = (f64::from(pos.x), f64::from(pos.y), f64::from(pos.z));
    for slot in 0..slots {
        let stack = stacks
            .get(slot as usize)
            .cloned()
            .unwrap_or_else(ItemStack::empty);
        world.drop_container_stack(x, y, z, stack);
    }
}

/// `JukeboxBlockEntity.popOutTheItem`：非空时 `offsetRandomXZ` 两个 `nextFloat`，再生成物品。
///
/// `JukeboxBlockEntity.popOutTheItem`: a non-empty record draws two `nextFloat`s, then spawns the item.
fn pop_jukebox(world: &mut World, pos: BlockPos, stack: ItemStack) {
    if stack.is_empty() {
        return;
    }
    let dx = (world.random().next_float() - 0.5_f32) * 0.7_f32;
    let dz = (world.random().next_float() - 0.5_f32) * 0.7_f32;
    let at = Vec3::new(
        f64::from(pos.x) + 0.5 + f64::from(dx),
        f64::from(pos.y) + 1.01,
        f64::from(pos.z) + 0.5 + f64::from(dz),
    );
    world.spawn_at_location(at, stack);
}

/// `BlockEntity.preRemoveSideEffects`。`stacks` 是这一格自己那张物品表。
///
/// `BlockEntity.preRemoveSideEffects`. `stacks` is this block entity's own item list.
pub(crate) fn pre_remove_side_effects(
    state: BlockState,
    world: &mut World,
    pos: BlockPos,
    stacks: &[ItemStack],
) {
    let Some(entity) = state.block().block_entity_type else {
        return;
    };
    use registries::block_entity_type as E;

    match entity {
        // `ChestBlockEntity`、`TrappedChestBlockEntity`（铜箱子也是这个实体）、`BarrelBlockEntity`。
        E::CHEST | E::TRAPPED_CHEST | E::BARREL => drop_slots(world, pos, 27, stacks),
        // `AbstractFurnaceBlockEntity`：三格。经验来自空的 `recipesUsed`，不取数。
        //
        // `AbstractFurnaceBlockEntity`: three slots. Experience comes from an empty `recipesUsed` and draws nothing.
        E::FURNACE | E::SMOKER | E::BLAST_FURNACE => drop_slots(world, pos, 3, stacks),
        E::HOPPER | E::BREWING_STAND => drop_slots(world, pos, 5, stacks),
        E::DISPENSER | E::DROPPER | E::CRAFTER => drop_slots(world, pos, 9, stacks),
        E::SHELF => drop_slots(world, pos, 3, stacks),
        E::DECORATED_POT => drop_slots(world, pos, 1, stacks),
        E::CAMPFIRE => drop_slots(world, pos, 4, stacks),
        // 有 6 格物品栏时按堆掉。没有条目时退回 `SLOT_OCCUPIED`，占着的格子掉一本普通书。
        //
        // A 6-slot list drops those stacks. Without one, fall back to `SLOT_OCCUPIED`:
        // each occupied slot drops one plain book.
        E::CHISELED_BOOKSHELF => {
            if stacks.len() == 6 {
                drop_slots(world, pos, 6, stacks);
            } else {
                let books: Vec<ItemStack> = SLOT_OCCUPIED
                    .iter()
                    .map(|slot| {
                        if state.get(*slot) {
                            ItemStack::new(items::BOOK, 1)
                        } else {
                            ItemStack::empty()
                        }
                    })
                    .collect();
                drop_slots(world, pos, 6, &books);
            }
        }
        // 通知邻居在 `World::set_block_state` 里：正在放的那一次在取数之前，另一次在取数之后。
        //
        // Neighbor notifications live in `World::set_block_state`: the playing song's one is
        // before the rolls, the other after.
        E::JUKEBOX => {
            pop_jukebox(
                world,
                pos,
                stacks.first().cloned().unwrap_or_else(ItemStack::empty),
            );
        }
        // `LecternBlockEntity.preRemoveSideEffects` 生成的 `ItemEntity` 用实体自己的
        // `RandomSource`，不取 `level.random`。书在游戏层，这里不掉落、不取数。
        //
        // The item entity uses its own `RandomSource`, not `level.random`. The book
        // itself lives in the game layer, so this arm neither drops it nor draws.
        E::LECTERN => {}
        // 潜影盒把 `preRemoveSideEffects` 覆盖成空的。
        _ => {}
    }
}

/// `BlockEntity.setRemoved`：这个位置在 `preRemoveSideEffects` 之后，不管跳过副作用与否都会走到。
pub(crate) fn removed_side_effects(state: BlockState, world: &mut World, pos: BlockPos) {
    match state.block().block_entity_type {
        // `BeaconBlockEntity.setRemoved`。
        Some(registries::block_entity_type::BEACON) => {
            world.play_block_sound(
                pos,
                sound_event("minecraft:block.beacon.deactivate"),
                1.0,
                1.0,
            );
        }
        // `JukeboxBlockEntity.setRemoved`：没在放歌也报 1011。
        Some(registries::block_entity_type::JUKEBOX) => world.level_event(1011, pos, 0),
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

    /// 只带位置和编号的测试用方块实体。
    #[derive(Debug)]
    struct Dummy {
        pos: BlockPos,
        value: i32,
    }

    impl Dummy {
        fn boxed(pos: BlockPos, value: i32) -> Box<dyn BlockEntity> {
            Box::new(Self { pos, value })
        }
    }

    impl BlockEntity for Dummy {
        fn pos(&self) -> BlockPos {
            self.pos
        }

        fn tick(&mut self, _world: &mut World) {}

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }

        fn write_additional(&self) -> Result<Vec<(String, Tag)>, crate::archive::Error> {
            Ok(Vec::new())
        }
    }

    const CHUNK_A: ChunkPos = ChunkPos::new(0, 0);
    const A1: BlockPos = BlockPos::new(1, 64, 1);
    const A2: BlockPos = BlockPos::new(2, 64, 2);
    const B1: BlockPos = BlockPos::new(20, 64, 1);
    const B2: BlockPos = BlockPos::new(21, 64, 2);

    fn ticker_order(entities: &mut BlockEntities) -> Vec<(BlockPos, i32)> {
        entities.begin_tick();
        let mut order = Vec::new();
        while let Some((pos, id)) = entities.next_ticker() {
            let entity = entities.take(pos, id).unwrap();
            order.push((pos, entity.as_any().downcast_ref::<Dummy>().unwrap().value));
            entities.put(pos, id, entity);
        }
        entities.end_tick();
        order
    }

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

    #[test]
    fn taking_a_chunk_leaves_no_trace_and_keeps_other_chunks() {
        let mut entities = BlockEntities::default();
        entities.insert(Dummy::boxed(A1, 1));
        entities.insert(Dummy::boxed(B1, 2));
        entities.insert(Dummy::boxed(A2, 3));
        entities.insert(Dummy::boxed(B2, 4));

        let boxed = entities.take_chunk(CHUNK_A).unwrap();
        assert!(format!("{boxed:?}").contains("ChunkBlockEntities"));
        assert!(!entities.contains(A1));
        assert!(!entities.contains(A2));
        assert!(entities.get::<Dummy>(A1).is_none());
        assert!(entities.get::<Dummy>(A2).is_none());
        assert_eq!(entities.get::<Dummy>(B1).unwrap().value, 2);
        assert_eq!(entities.get::<Dummy>(B2).unwrap().value, 4);
        // 别的区块的运行顺序一点没变。槽位按官方留到下一次 begin_tick 回收。
        assert_eq!(ticker_order(&mut entities), [(B1, 2), (B2, 4)]);

        // 取出再放回：内容和查询都回来了。
        entities.insert_chunk(boxed);
        assert_eq!(entities.get::<Dummy>(A1).unwrap().value, 1);
        assert_eq!(entities.get::<Dummy>(A2).unwrap().value, 3);
        assert!(entities.contains(A1));
    }

    #[test]
    fn taking_and_putting_back_keeps_other_chunks_ticking_order() {
        let mut untouched = BlockEntities::default();
        let mut moved = BlockEntities::default();
        for &(pos, value) in &[(A1, 1), (B1, 2), (A2, 3), (B2, 4)] {
            untouched.insert(Dummy::boxed(pos, value));
            moved.insert(Dummy::boxed(pos, value));
        }
        let boxed = moved.take_chunk(CHUNK_A).unwrap();
        moved.insert_chunk(boxed);

        // 取出走的是现有 remove：槽位留着，insert 复用它，运行顺序也和没搬过完全一样。
        assert_eq!(
            ticker_order(&mut untouched),
            [(A1, 1), (B1, 2), (A2, 3), (B2, 4)]
        );
        assert_eq!(
            ticker_order(&mut moved),
            [(A1, 1), (B1, 2), (A2, 3), (B2, 4)]
        );
    }

    #[test]
    fn a_box_moves_into_a_new_table() {
        let mut source = BlockEntities::default();
        source.insert(Dummy::boxed(A1, 7));
        source.insert(Dummy::boxed(B1, 8));
        let boxed = source.take_chunk(CHUNK_A).unwrap();

        let mut target = BlockEntities::default();
        target.insert(Dummy::boxed(A2, 9));
        target.insert_chunk(boxed);
        assert_eq!(target.get::<Dummy>(A1).unwrap().value, 7);
        assert_eq!(target.get::<Dummy>(A2).unwrap().value, 9);
        assert_eq!(ticker_order(&mut target), [(A2, 9), (A1, 7)]);
        // 源表里只剩别的区块。
        assert_eq!(ticker_order(&mut source), [(B1, 8)]);
    }

    /// 只读查询和 `take_chunk` 用的是同一个前提，tick 中途问一次就能整批不动。
    #[test]
    fn the_ready_query_tracks_a_mid_tick_table() {
        let mut entities = BlockEntities::default();
        entities.insert(Dummy::boxed(A1, 1));
        assert!(entities.ready_to_move_chunks());

        entities.begin_tick();
        assert!(!entities.ready_to_move_chunks());
        assert!(entities.next_ticker().is_some());
        assert!(!entities.ready_to_move_chunks());
        entities.end_tick();
        assert!(entities.ready_to_move_chunks());

        let (id, entity) = entities.take_at(A1).unwrap();
        assert!(!entities.ready_to_move_chunks());
        entities.put(A1, id, entity);
        assert!(entities.ready_to_move_chunks());

        let (_id, _entity) = entities.take_at(A1).unwrap();
        assert!(entities.remove(A1).is_none());
        assert!(!entities.ready_to_move_chunks());
        assert!(entities.take_chunk(CHUNK_A).is_err());
    }

    #[test]
    fn taking_a_chunk_is_busy_mid_tick() {
        let mut entities = BlockEntities::default();
        entities.insert(Dummy::boxed(A1, 1));
        entities.begin_tick();
        assert!(matches!(entities.take_chunk(CHUNK_A), Err(Busy)));
        entities.end_tick();
        assert!(entities.take_chunk(CHUNK_A).is_ok());

        // 拿出去还没放回也算 tick 中途。
        entities.insert(Dummy::boxed(A2, 2));
        let (pos, id, entity) = {
            let (id, entity) = entities.take_at(A2).unwrap();
            (A2, id, entity)
        };
        assert!(matches!(entities.take_chunk(CHUNK_A), Err(Busy)));
        entities.put(pos, id, entity);
        assert!(entities.take_chunk(CHUNK_A).is_ok());

        // 拿出去时被删掉，残留的登记也要挡住（这时 taken 和 removed_while_taken 都有条目）。
        entities.insert(Dummy::boxed(A2, 3));
        let (_id, _entity) = entities.take_at(A2).unwrap();
        assert!(entities.remove(A2).is_none());
        assert!(matches!(entities.take_chunk(CHUNK_A), Err(Busy)));
    }
}
