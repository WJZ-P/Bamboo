//! 这个 crate 和世界存储的边界。

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
};

use mc_bamboo_core::{
    BlockPos, ChunkPos, Hand, LightLayer,
    sign::{SignContent, SignTextSlot},
};
use mc_bamboo_data::SoundSource;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    block_entity::{BlockEntities, BlockEntity, BlockEvent, BlockEvents},
    container::Container,
    entity::{Entity, EntityPublic, EntityStore, EntityUid},
    gamerules::BlockGameRules,
    hopper::HopperEntity,
    random::LevelRandom,
    shape::Aabb,
    state::{BlockState, NoteBlockInstrument},
    ticks::TickPriority,
    world_border::BlockBox,
};

/// 发射器前方一格里、可能被穿上装备的生物。
///
/// A living entity in front of a dispenser that might receive equipment.
///
/// `equipment` 的下标是组件里的 `equipment_slot`：0 主手、1 脚、2 腿、3 胸、4 头、5 副手、
/// 6 身体、7 鞍。`None` 表示这个实体没有这一格，`Some` 空物品表示格子是空的。
///
/// `equipment` is indexed by the component `equipment_slot`: 0 mainhand, 1 feet, 2 legs,
/// 3 chest, 4 head, 5 offhand, 6 body, 7 saddle. `None` means the entity has no such slot.
/// `Some` empty stack means the slot exists and is empty.
#[derive(Clone, Debug)]
pub struct EquipTarget {
    pub entity_id: i32,
    pub bounds: Aabb,
    /// `isSpectator`。
    pub spectator: bool,
    /// `minecraft:entity_type` 的注册表 ID。
    pub entity_type: i32,
    pub equipment: [Option<ItemStack>; 8],
}

/// 一次触发的种类。
///
/// 计划刻和方块事件是设计稿 9.1 定义的两种触发；流体刻和方块实体的 ticker
/// 也按一次触发计，因为漏斗、熔炉这些机器只在这些循环里工作。
///
/// The kind of a trigger. Scheduled block ticks and block events are the two kinds
/// in §9.1; fluid ticks and block-entity tickers count as triggers too, because
/// hopper and furnace machines only run in those loops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// 一条到期的方块计划刻。`kind` 是方块的注册表 ID。
    ScheduledTick { pos: BlockPos, kind: usize },
    /// 一条到期的流体计划刻。`kind` 是流体的注册表 ID。
    ScheduledFluidTick { pos: BlockPos, kind: i32 },
    /// 一个方块事件。`kind` 是方块的注册表 ID，参数照 `BlockEventData`。
    BlockEvent {
        pos: BlockPos,
        kind: usize,
        param_a: i32,
        param_b: i32,
    },
    /// 方块实体 ticker 的一次 tick。`kind` 是这一格方块的注册表 ID。
    BlockEntityTick { pos: BlockPos, kind: usize },
}

/// 录制包装的工作模式。缓存驱动按触发点切换，见 `cache_run::run_cached_trigger`。
///
/// `Off` 只做一次布尔判断；`Count` 只累加一个计数器，用来估计这次解释执行的工作量；
/// `Full` 才是实验和缓存录制用的完整记账。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordMode {
    Off,
    Count,
    Full,
}

/// 方块逻辑里不经过 [`Level`] 读写方法的那些事件，默认谁也不看。
///
/// `World` 在每次触发的开头结尾、读世界时间、收集对外效果、查/排计划刻时各报一次。
/// 录制实现靠它把「读集合」和「效果」补全；默认实现什么都不做，正常执行时只有一次
/// 虚调用的开销。
///
/// Events in block logic that bypass [`Level`] read/write methods. `World` reports
/// each trigger boundary, world-time read, outgoing effect and scheduled-tick
/// query/scheduling. A recording implementation uses them to complete its read set
/// and effect list; the default implementation does nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Observation<'a> {
    TriggerBegin(Trigger),
    TriggerEnd,
    /// 切换录制包装的记账模式。守卫/重放期间是 `Off`，试探便宜触发点时是 `Count`。
    /// 实验驱动不报这个事件，历史数字不变。
    RecordMode(RecordMode),
    /// 一次活塞音高取数。重放时照样从世界随机源取一个数、按 `scale`/`base` 算 pitch。
    PistonPitch {
        pos: BlockPos,
        sound: i32,
        source: mc_bamboo_data::SoundSource,
        volume: f32,
        scale: f32,
        base: f32,
        except_initiator: bool,
    },
    /// `World::game_time()` 被读了。
    TimeRead,
    /// `World::push_effect` 收进去的音效、世界事件等。
    Effect(&'a Effect),
    /// `has_scheduled_tick` 或 `will_tick_this_tick` 的查询结果。
    TickQuery {
        pos: BlockPos,
        kind: usize,
        will_tick_this_tick: bool,
        result: bool,
    },
    /// 新排的一个方块或流体计划刻，延迟是触发时刻起的相对刻数。
    /// `existed` 是排之前这一格这一种刻/事件是不是已经排着：
    /// `BlockTicks::schedule` 和 `BlockEvents::push` 在重复时不产生变化，
    /// 也不递增内部序号，守卫必须复核（设计稿 §3.1）。
    TickScheduled {
        pos: BlockPos,
        kind: i64,
        fluid: bool,
        delay: i32,
        priority: TickPriority,
        existed: bool,
    },
    /// 排一个方块事件之前，它是不是已经在队列里。
    BlockEventQueued {
        event: crate::block_entity::BlockEvent,
        existed: bool,
    },
    /// 方块实体 tick 开始，记下这一格本刻 tick 过。
    NoteBlockEntityTick {
        pos: BlockPos,
    },
    /// 方块实体 tick 结束后，这一格的 `PistonMoving` 状态。缓存重放时按它写回，
    /// 保留 ticker 编号与顺序；实验的 `Record` 忽略这个事件（历史数字不变）。
    PistonTickState {
        pos: BlockPos,
        moved_state: u32,
        direction: u8,
        extending: bool,
        source: bool,
        progress: u32,
        progress_o: u32,
    },
}

/// 方块状态装不下、跟着位置走的世界数据：方块实体、方块事件、红石记忆。
///
/// 每个方法是一次具体的读或写，位置当参数传进来。方块逻辑只能通过这些方法碰这三样东西：
/// 以后它们按区块存放，或者外面套一层记录读写的实现时，这里的签名都不用改。
///
/// 用整张表存这三样东西的世界实现 [`LevelTables`] 就自动得到这个 trait；按区块存、
/// 或者要录制读写的实现者直接实现这个 trait，不必交出一整张表的引用。
///
/// World data that block states cannot hold: block entities, block events and redstone
/// memory. Every method is one concrete read or write, with the position passed in.
/// A world backed by whole tables implements [`LevelTables`]; an implementation that
/// splits state by chunk, or records reads and writes, implements this trait directly.
pub trait LevelSideState {
    // 红石记忆：每个方法对应 `RedstoneMemory` 上的一个操作。
    fn comparator_output(&self, pos: BlockPos) -> i32;
    fn set_comparator_output(&mut self, pos: BlockPos, signal: i32);
    fn last_bookshelf_slot(&self, pos: BlockPos) -> Option<usize>;
    fn set_last_bookshelf_slot(&mut self, pos: BlockPos, slot: usize);
    fn is_jukebox_playing(&self, pos: BlockPos) -> bool;
    fn set_jukebox_playing(&mut self, pos: BlockPos, playing: bool);
    fn crafter_disabled(&self, pos: BlockPos) -> u16;
    fn set_crafter_disabled(&mut self, pos: BlockPos, mask: u16);
    fn set_crafter_crafting(&mut self, pos: BlockPos, remaining: i32, now: i64);
    fn take_crafter_crafting(&mut self, now: i64) -> Vec<(BlockPos, i32)>;
    fn restore_crafter_crafting(&mut self, entries: Vec<(BlockPos, i32)>, now: i64);
    /// 取走这一刻到期的合成器倒计时；没到期的留在表里，每刻不受它们影响。
    fn take_due_crafter_crafting(&mut self, now: i64) -> Vec<BlockPos>;
    fn forget_block_entity_memory(&mut self, pos: BlockPos);
    fn set_pending_lectern(&mut self, facts: LecternFacts);
    fn take_pending_lectern(&mut self) -> Option<LecternFacts>;
    fn set_lectern(&mut self, pos: BlockPos, facts: LecternFacts);
    fn lectern(&self, pos: BlockPos) -> Option<LecternFacts>;
    fn clear_lectern(&mut self, pos: BlockPos);
    fn forget_torch_toggles_before(&mut self, when: i64);
    fn record_torch_toggle(&mut self, pos: BlockPos, when: i64);
    fn torch_toggle_count(&self, pos: BlockPos) -> usize;

    // 方块事件。
    /// 这一条方块事件是不是已经在队列里（`BlockEvents::queued`）。
    fn block_event_queued(&self, _event: BlockEvent) -> bool {
        false
    }
    fn push_block_event(&mut self, event: BlockEvent);
    fn pop_block_event(&mut self) -> Option<BlockEvent>;
    /// 一轮方块事件结束后，按原顺序放回那些所在区块不能 tick 的事件。
    /// 默认逐个走 [`LevelSideState::push_block_event`]，和官方 `LinkedHashSet.addAll` 一样
    /// 由实现者负责查重；用整张表的实现转发到 [`BlockEvents::reschedule`]。
    fn reschedule_block_events(&mut self, events: Vec<BlockEvent>) {
        for event in events {
            self.push_block_event(event);
        }
    }

    // 方块实体和它们的 ticker。
    fn block_entity_dyn(&self, pos: BlockPos) -> Option<&dyn BlockEntity>;
    /// 一次 tick 跑完后，按实体自己的公式算下一次到点时刻；休眠被关掉时返回 `None`。
    fn block_entity_sleep_from(&mut self, pos: BlockPos, now: i64) -> Option<i64>;
    /// 休眠是否启用（差分测试会关掉）。
    fn block_entity_sleep_enabled(&self) -> bool;
    fn has_block_entity(&self, pos: BlockPos) -> bool;
    fn insert_block_entity(&mut self, entity: Box<dyn BlockEntity>);
    fn remove_block_entity(&mut self, pos: BlockPos) -> Option<Box<dyn BlockEntity>>;
    fn take_block_entity_at(&mut self, pos: BlockPos) -> Option<(u64, Box<dyn BlockEntity>)>;
    fn take_block_entity(&mut self, pos: BlockPos, id: u64) -> Option<Box<dyn BlockEntity>>;
    fn put_block_entity(&mut self, pos: BlockPos, id: u64, entity: Box<dyn BlockEntity>);
    fn is_block_entity_taken(&self, pos: BlockPos) -> bool;
    fn wake_block_entity(&mut self, pos: BlockPos);
    fn schedule_block_entity_wake(&mut self, pos: BlockPos, wake_at: Option<i64>);
    fn set_chunk_block_entity_ticking(&mut self, chunk: ChunkPos, ticks: bool);
    /// 把这一刻世界边界的方块盒交给方块实体表，离开边界的睡眠实体暂停，见
    /// [`BlockEntities::sync_border`]。
    fn sync_block_entity_border(&mut self, border: BlockBox);
    fn begin_block_entity_tick(&mut self, now: i64);
    fn next_block_entity_ticker(&mut self) -> Option<(BlockPos, u64)>;
    fn end_block_entity_tick(&mut self);
}

/// 用一整张表存方块实体、方块事件和红石记忆的世界。
///
/// 实现它就自动得到 [`LevelSideState`]。这五个方法只有存储实现者能碰到：
/// [`Level`] 不把它当父 trait，方块逻辑拿到的 `&mut dyn Level` 够不着整张表。
///
/// A world that stores these three kinds of state in whole tables. Implementing this
/// trait provides [`LevelSideState`]. [`Level`] does not require it, so block logic
/// holding a `&mut dyn Level` cannot reach the tables.
pub trait LevelTables {
    fn block_entities_table(&self) -> &BlockEntities;
    fn block_entities_table_mut(&mut self) -> &mut BlockEntities;
    fn block_events_table(&self) -> &BlockEvents;
    fn block_events_table_mut(&mut self) -> &mut BlockEvents;
    fn redstone_table(&self) -> &RedstoneMemory;
    fn redstone_table_mut(&mut self) -> &mut RedstoneMemory;
    /// 游戏层记「这个区块的持久化内容可能改了」。默认什么都不做，
    /// `LevelSideState` 的 blanket impl 在每个带位置的写方法上调它。
    /// 宁多勿少：多存一次没有错，漏存一次才是错。
    /// Game layer hook: mark a chunk as possibly changed. Called from every
    /// positional write in the blanket impl; over-marking is harmless.
    fn mark_chunk_dirty(&mut self, _pos: ChunkPos) {}
}

impl<T: LevelTables + ?Sized> LevelSideState for T {
    fn comparator_output(&self, pos: BlockPos) -> i32 {
        self.redstone_table().comparator_output(pos)
    }

    fn set_comparator_output(&mut self, pos: BlockPos, signal: i32) {
        self.mark_chunk_dirty(pos.chunk());
        self.redstone_table_mut().set_comparator_output(pos, signal);
    }

    fn last_bookshelf_slot(&self, pos: BlockPos) -> Option<usize> {
        self.redstone_table().last_bookshelf_slot(pos)
    }

    fn set_last_bookshelf_slot(&mut self, pos: BlockPos, slot: usize) {
        self.mark_chunk_dirty(pos.chunk());
        self.redstone_table_mut().set_last_bookshelf_slot(pos, slot);
    }

    fn is_jukebox_playing(&self, pos: BlockPos) -> bool {
        self.redstone_table().is_jukebox_playing(pos)
    }

    fn set_jukebox_playing(&mut self, pos: BlockPos, playing: bool) {
        self.mark_chunk_dirty(pos.chunk());
        self.redstone_table_mut().set_jukebox_playing(pos, playing);
    }

    fn crafter_disabled(&self, pos: BlockPos) -> u16 {
        self.redstone_table().crafter_disabled(pos)
    }

    fn set_crafter_disabled(&mut self, pos: BlockPos, mask: u16) {
        self.mark_chunk_dirty(pos.chunk());
        self.redstone_table_mut().set_crafter_disabled(pos, mask);
    }

    fn set_crafter_crafting(&mut self, pos: BlockPos, remaining: i32, now: i64) {
        self.mark_chunk_dirty(pos.chunk());
        self.redstone_table_mut()
            .set_crafter_crafting(pos, remaining, now);
    }

    fn take_crafter_crafting(&mut self, now: i64) -> Vec<(BlockPos, i32)> {
        self.redstone_table_mut().take_crafter_crafting(now)
    }

    fn restore_crafter_crafting(&mut self, entries: Vec<(BlockPos, i32)>, now: i64) {
        self.redstone_table_mut()
            .restore_crafter_crafting(entries, now);
    }

    fn take_due_crafter_crafting(&mut self, now: i64) -> Vec<BlockPos> {
        self.redstone_table_mut().take_due_crafter_crafting(now)
    }

    fn forget_block_entity_memory(&mut self, pos: BlockPos) {
        self.mark_chunk_dirty(pos.chunk());
        self.redstone_table_mut().forget_block_entity(pos);
    }

    fn set_pending_lectern(&mut self, facts: LecternFacts) {
        self.redstone_table_mut().set_pending_lectern(facts);
    }

    fn take_pending_lectern(&mut self) -> Option<LecternFacts> {
        self.redstone_table_mut().take_pending_lectern()
    }

    fn set_lectern(&mut self, pos: BlockPos, facts: LecternFacts) {
        self.mark_chunk_dirty(pos.chunk());
        self.redstone_table_mut().set_lectern(pos, facts);
    }

    fn lectern(&self, pos: BlockPos) -> Option<LecternFacts> {
        self.redstone_table().lectern(pos)
    }

    fn clear_lectern(&mut self, pos: BlockPos) {
        self.mark_chunk_dirty(pos.chunk());
        self.redstone_table_mut().clear_lectern(pos);
    }

    fn forget_torch_toggles_before(&mut self, when: i64) {
        self.redstone_table_mut().forget_torch_toggles_before(when);
    }

    fn record_torch_toggle(&mut self, pos: BlockPos, when: i64) {
        self.mark_chunk_dirty(pos.chunk());
        self.redstone_table_mut().record_torch_toggle(pos, when);
    }

    fn torch_toggle_count(&self, pos: BlockPos) -> usize {
        self.redstone_table().torch_toggle_count(pos)
    }

    fn block_event_queued(&self, event: BlockEvent) -> bool {
        self.block_events_table().contains(event)
    }

    fn push_block_event(&mut self, event: BlockEvent) {
        self.block_events_table_mut().push(event);
    }

    fn pop_block_event(&mut self) -> Option<BlockEvent> {
        self.block_events_table_mut().pop()
    }

    fn reschedule_block_events(&mut self, events: Vec<BlockEvent>) {
        self.block_events_table_mut().reschedule(events);
    }

    fn block_entity_dyn(&self, pos: BlockPos) -> Option<&dyn BlockEntity> {
        self.block_entities_table().get_dyn(pos)
    }

    fn block_entity_sleep_enabled(&self) -> bool {
        self.block_entities_table().sleep_enabled()
    }

    fn block_entity_sleep_from(&mut self, pos: BlockPos, now: i64) -> Option<i64> {
        let table = self.block_entities_table_mut();
        if !table.sleep_enabled() {
            return None;
        }
        table.sleep_from(pos, now)
    }

    fn has_block_entity(&self, pos: BlockPos) -> bool {
        self.block_entities_table().contains(pos)
    }

    fn insert_block_entity(&mut self, entity: Box<dyn BlockEntity>) {
        self.mark_chunk_dirty(entity.pos().chunk());
        self.block_entities_table_mut().insert(entity);
    }

    fn remove_block_entity(&mut self, pos: BlockPos) -> Option<Box<dyn BlockEntity>> {
        self.mark_chunk_dirty(pos.chunk());
        self.block_entities_table_mut().remove(pos)
    }

    fn take_block_entity_at(&mut self, pos: BlockPos) -> Option<(u64, Box<dyn BlockEntity>)> {
        self.block_entities_table_mut().take_at(pos)
    }

    fn take_block_entity(&mut self, pos: BlockPos, id: u64) -> Option<Box<dyn BlockEntity>> {
        self.block_entities_table_mut().take(pos, id)
    }

    fn put_block_entity(&mut self, pos: BlockPos, id: u64, entity: Box<dyn BlockEntity>) {
        self.block_entities_table_mut().put(pos, id, entity);
    }

    fn is_block_entity_taken(&self, pos: BlockPos) -> bool {
        self.block_entities_table().is_taken(pos)
    }

    fn wake_block_entity(&mut self, pos: BlockPos) {
        self.block_entities_table_mut().wake(pos);
    }

    fn schedule_block_entity_wake(&mut self, pos: BlockPos, wake_at: Option<i64>) {
        self.block_entities_table_mut().schedule_wake(pos, wake_at);
    }

    fn set_chunk_block_entity_ticking(&mut self, chunk: ChunkPos, _ticks: bool) {
        self.block_entities_table_mut()
            .set_chunk_ticking(chunk, _ticks);
    }

    fn sync_block_entity_border(&mut self, border: BlockBox) {
        self.block_entities_table_mut().sync_border(border);
    }

    fn begin_block_entity_tick(&mut self, now: i64) {
        // 睡着的实体醒来、真正跑了 tick 时会改持久化的数值：把所在区块标脏。
        // 宁多勿少，多存一次没有错（复查「高 3」+ main 的休眠熔炉）。
        let positions: Vec<BlockPos> = self
            .block_entities_table()
            .ticker_positions()
            .map(|(pos, _)| pos)
            .collect();
        for pos in positions {
            self.mark_chunk_dirty(pos.chunk());
        }
        self.block_entities_table_mut().begin_tick(now);
    }

    fn next_block_entity_ticker(&mut self) -> Option<(BlockPos, u64)> {
        self.block_entities_table_mut().next_ticker()
    }

    fn end_block_entity_tick(&mut self) {
        self.block_entities_table_mut().end_tick();
    }
}

/// 方块逻辑能碰到的实体表：一次调用一个具体读或写，位置、范围或 ID 当参数。
///
/// 和 [`LevelSideState`] 一样，用整张表存实体的世界实现 [`EntityTables`] 就自动得到它；
/// 按区块存或者要录制读写的实现者可以直接实现这个 trait，不必交出整张表。
/// 漏斗吸物品这类方块实体阶段的写入是串行的，直接走这里，不经过实体命令。
pub trait LevelEntities {
    fn entity_count(&self) -> usize;
    /// 按 uid 升序，也就是生成的先后。
    fn entity_uids(&self) -> Vec<EntityUid>;
    /// 已经在 tick 列表里的实体。正在 tick 的那一个看不到。
    fn entity(&self, uid: EntityUid) -> Option<Entity>;
    fn entity_public(&self, uid: EntityUid) -> Option<EntityPublic>;
    /// 空间索引范围查询，按 uid 升序访问。
    fn for_each_entity_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic));
    /// `addFreshEntity`：tick 期间生成的实体下一 tick 才动。
    fn spawn_entity(&mut self, entity: Entity) -> EntityUid;
    /// 刚生成、还没并进 tick 列表的实体也找得到。
    fn with_just_spawned(&mut self, uid: EntityUid, f: &mut dyn FnMut(&mut Entity)) -> bool;
    /// 只打上 `removed` 标记，实体还留在这一拍的名单里。
    fn discard_entity(&mut self, uid: EntityUid) -> bool;
    /// 从 tick 列表里立刻拿掉。
    fn remove_entity(&mut self, uid: EntityUid);
    fn begin_entity_tick(&mut self);
    fn end_entity_tick(&mut self);
    fn take_entity(&mut self, uid: EntityUid) -> Option<Entity>;
    fn put_entity(&mut self, entity: Entity);
    fn live_entities(&self) -> Vec<Entity>;
    /// 并行驱动用：算阶段读整张实体表，落地时只发布变了的那几行。
    fn entities_table(&self) -> &EntityStore;
    fn entities_table_mut(&mut self) -> &mut EntityStore;
    fn publish_entity_rows(
        &mut self,
        rows: &[usize],
        public: Vec<crate::entity::EntityPublic>,
        state: Vec<crate::entity::EntityState>,
    );
}

/// 用一整张 [`EntityStore`] 存实体的世界。实现它就自动得到 [`LevelEntities`]。
///
/// [`Level`] 不把它当父 trait，方块逻辑拿到的 `&mut dyn Level` 够不着整张表。
pub trait EntityTables {
    fn entities_table(&self) -> &EntityStore;
    fn entities_table_mut(&mut self) -> &mut EntityStore;
    /// 实体写也要把所在区块标脏（实体 NBT 独立于区块文件）。
    fn mark_chunk_dirty(&mut self, _pos: ChunkPos) {}
}

impl<T: EntityTables + ?Sized> LevelEntities for T {
    fn entity_count(&self) -> usize {
        self.entities_table().len()
    }

    fn entity_uids(&self) -> Vec<EntityUid> {
        self.entities_table().ids()
    }

    fn entity(&self, uid: EntityUid) -> Option<Entity> {
        self.entities_table().get(uid)
    }

    fn entity_public(&self, uid: EntityUid) -> Option<EntityPublic> {
        self.entities_table().public_at(uid).cloned()
    }

    fn for_each_entity_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic)) {
        self.entities_table().for_each_in(area, visit);
    }

    fn spawn_entity(&mut self, entity: Entity) -> EntityUid {
        self.mark_chunk_dirty(entity.block_position().chunk());
        self.entities_table_mut().spawn(entity)
    }

    fn with_just_spawned(&mut self, uid: EntityUid, f: &mut dyn FnMut(&mut Entity)) -> bool {
        self.entities_table_mut()
            .with_mut_including_pending(uid, f)
            .is_some()
    }

    fn discard_entity(&mut self, uid: EntityUid) -> bool {
        if let Some(public) = self.entities_table().public_at(uid) {
            self.mark_chunk_dirty(public.chunk_position());
        }
        self.entities_table_mut()
            .with_mut(uid, |entity| entity.discard())
            .is_some()
    }

    fn remove_entity(&mut self, uid: EntityUid) {
        if let Some(public) = self.entities_table().public_at(uid) {
            self.mark_chunk_dirty(public.chunk_position());
        }
        self.entities_table_mut().remove(uid);
    }

    fn begin_entity_tick(&mut self) {
        self.entities_table_mut().begin_tick();
    }

    fn end_entity_tick(&mut self) {
        self.entities_table_mut().end_tick();
    }

    fn take_entity(&mut self, uid: EntityUid) -> Option<Entity> {
        self.entities_table_mut().take(uid)
    }

    fn put_entity(&mut self, entity: Entity) {
        self.mark_chunk_dirty(entity.block_position().chunk());
        self.entities_table_mut().put(entity);
    }

    fn live_entities(&self) -> Vec<Entity> {
        self.entities_table().iter().collect()
    }

    fn entities_table(&self) -> &EntityStore {
        <T as EntityTables>::entities_table(self)
    }

    fn entities_table_mut(&mut self) -> &mut EntityStore {
        <T as EntityTables>::entities_table_mut(self)
    }

    fn publish_entity_rows(
        &mut self,
        rows: &[usize],
        public: Vec<crate::entity::EntityPublic>,
        state: Vec<crate::entity::EntityState>,
    ) {
        self.entities_table_mut().publish_rows(rows, public, state);
    }
}

/// 世界的存储和世界外的一切。`mc-bamboo-game` 和 oracle 各有一个实现。
pub trait Level: LevelSideState + LevelEntities {
    /// 世界外返回 `void_air`。
    fn block_state(&self, pos: BlockPos) -> BlockState;

    /// 写入一格，不做任何连锁。高度超出世界时什么也不做，返回 `false`。
    ///
    /// `flags` 是 [`set_block`](crate::World::set_block) 的那一组，存储只需要看
    /// [`UPDATE_CLIENTS`](crate::world::UPDATE_CLIENTS)：官方只在带这个标记时才把改动发给客户端。
    fn write_block(&mut self, pos: BlockPos, state: BlockState, flags: u32) -> bool;

    /// 这些盒子（世界坐标）和所有挡路的实体都没有正体积重叠。
    fn is_unobstructed(&self, boxes: &[Aabb]) -> bool;

    /// 世界最低的一层，`getMinY`。
    fn min_y(&self) -> i32;

    /// 世界最高的一层（含），`getMaxY`。
    fn max_y(&self) -> i32;

    /// `Level.random` 和 `randValue`，方块逻辑的随机数都从这里取。
    fn random(&self) -> &LevelRandom;

    /// 只取一个 `next_float()`，供活塞音高这类「取数 → 算参数 → 发效果」的动作使用。
    /// 默认实现转发给 [`Self::random`]；录制包装覆盖它把这次取数记成可重放的效果。
    fn next_random_float(&self) -> f32 {
        self.random().source().next_float()
    }

    /// `LevelChunkSection.isRandomlyTicking`：这个区块里纵坐标是 `section_y`（方块的 `y >> 4`）的区块段里
    /// 有会随机刻的方块或者流体，也就是有状态的 `is_randomly_ticking` 或者 `is_fluid_randomly_ticking` 为真。
    fn is_randomly_ticking_section(&self, chunk: ChunkPos, section_y: i32) -> bool;

    /// `section_y`（方块的 `y >> 4`）这一段的整段方块状态哈希，红石缓存的键读它。
    ///
    /// 默认实现逐格重算；有区块存储的实现应该覆盖它，直接读区块段里维护好的哈希。
    /// 段内坐标用于哈希，段坐标本身另进缓存键。段超出世界高度或所在区块不可用时
    /// 返回 `None`：这时调用方必须退回解释执行。
    fn section_hash(&self, _chunk: ChunkPos, _section_y: i32) -> Option<u128> {
        None
    }

    /// 这个区块里哪些区块段 [`is_randomly_ticking_section`](Self::is_randomly_ticking_section)：
    /// 第 i 位对应从世界底部数起的第 i 段。
    ///
    /// 随机刻每刻对每个区块问一次。默认实现逐段去问；手里有整个区块的实现应该覆盖它，
    /// 一次答完，答案必须和逐段问的相同。
    fn randomly_ticking_sections(&self, chunk: ChunkPos) -> u32 {
        let first = self.min_y() >> 4;
        let last = self.max_y() >> 4;
        (first..=last)
            .enumerate()
            .filter(|&(_, section_y)| self.is_randomly_ticking_section(chunk, section_y))
            .fold(0, |mask, (index, _)| mask | 1 << index)
    }

    /// `ChunkMap.anyPlayerCloseEnoughTo`：有不是旁观者的玩家，脚到这一格最小的角不到 `max_distance` 格。
    fn any_player_close_enough_to(&self, pos: BlockPos, max_distance: i32) -> bool;

    /// `getBrightness(layer, pos)`：这一格的天空光或者方块光，0 到 15。
    fn brightness(&self, layer: LightLayer, pos: BlockPos) -> i32;

    /// `getBlockEntity(pos) instanceof SignBlockEntity` 之后读到的内容。
    ///
    /// 还没写过字的告示牌返回 [`SignContent::BLANK`]，官方也是按状态新建一个空的方块实体。
    /// 不是告示牌方块时返回 `None`。
    fn sign(&self, pos: BlockPos) -> Option<SignContent>;

    /// 改写告示牌的内容，对应 `SignBlockEntity` 里那几个字段。不是告示牌方块时返回 `false`。
    fn set_sign(&mut self, pos: BlockPos, content: SignContent) -> bool;

    /// `getBlockEntity(pos) instanceof SkullBlockEntity` 时它的 `getNoteBlockSound`。
    ///
    /// 官方把物品上的 `note_block_sound` 组件搬进头颅方块实体，音符盒响的时候按它找自定义
    /// 音效，见 [`Effect::NoteBlockPlayed`]。不是头颅方块实体、或者头颅没带音效时返回
    /// `None`，官方对应 `null`。
    fn note_block_sound(&self, pos: BlockPos) -> Option<String>;

    /// 这个区块现在跑不跑方块逻辑（计划刻、流体刻、方块事件、方块实体）。默认都跑。
    ///
    /// 到期的计划刻在不能跑的区块里留在队列里，等这个区块重新能跑时按原来的排序键执行。
    /// 和 [`crate::entity::ctx::BlockView::chunk_ticks_entities`] 是两件不同的事；
    /// [`crate::World`] 实现后者时转发到 [`Level::chunk_ticks_entities`]，两者必须给出同样的答案。
    ///
    /// Whether this chunk runs block logic now: scheduled block/fluid ticks, block events and
    /// block entities. All chunks do by default. Due ticks in a chunk that cannot run stay in
    /// the queue and keep their original sort key until it can run again.
    fn chunk_ticks_blocks(&self, _chunk: ChunkPos) -> bool {
        true
    }

    /// 这个区块里的实体现在 tick 不 tick。默认都 tick。
    ///
    /// 不 tick 的实体仍在世界里：别的实体照样查得到它，只是自己这一刻不动。
    /// 和 [`crate::entity::ctx::BlockView::chunk_ticks_entities`] 必须给出同样的答案；
    /// [`crate::World`] 实现后者时转发到这里，并行驱动从只读方块视图读同一个判断。
    ///
    /// Whether entities in this chunk tick now. All do by default. An entity that does not tick
    /// stays in the world and remains visible to other entities; only its own tick is skipped.
    fn chunk_ticks_entities(&self, _chunk: ChunkPos) -> bool {
        true
    }

    /// 这一格的方块实体这一刻要不要跑。默认要跑。
    ///
    /// 方块 oracle 的假世界只登记漏斗和移动中的活塞，熔炉和酿造台不跑。
    ///
    /// Whether this cell's block entity ticks. The default is yes.
    ///
    /// The block oracle's fake level only registers hoppers and moving pistons, so furnaces
    /// and brewing stands do not tick.
    fn runs_block_entity_tick(&self, _pos: BlockPos) -> bool {
        true
    }

    /// 方块逻辑读到的游戏规则快照。默认是官方默认值。
    ///
    /// The game-rule snapshot block logic reads. The default is the vanilla defaults.
    fn block_game_rules(&self) -> BlockGameRules {
        BlockGameRules::vanilla()
    }

    /// 方块逻辑读到的天气快照。默认是晴天，权威副本在世界线程的 `Global` 上。
    ///
    /// The weather snapshot block logic reads. The default is clear weather; the
    /// authoritative copy lives on the world thread's `Global`.
    fn block_weather(&self) -> crate::weather::BlockWeather {
        crate::weather::BlockWeather::CLEAR
    }

    /// `Level.getWorldBorder()`：这一维度的世界边界。默认是官方默认边界，权威副本
    /// 在世界线程的 `Global` 上，游戏层和实体的只读视图各覆盖一份快照。
    ///
    /// `Level.getWorldBorder()`: this dimension's world border. The default is the
    /// vanilla default border; the authoritative copy lives on the world thread's
    /// `Global`, and the game layer and entity read-only views each supply a snapshot.
    fn world_border(&self) -> crate::WorldBorder {
        crate::WorldBorder::default()
    }

    /// `BlockAndLightGetter.canSeeSky(pos)`：天空光是 15 就看得见天。
    ///
    /// `isRainingAt` 用得到。默认实现读上一轮算好的天空光，和官方一致。
    fn can_see_sky(&self, pos: BlockPos) -> bool {
        self.brightness(LightLayer::Sky, pos) >= 15
    }

    /// `getHeightmapPos(MOTION_BLOCKING, pos).getY()`：这一列挡运动的最高方块上面一格。
    /// 整列都是空气时是世界底部下面一格。
    ///
    /// `isRainingAt` 和 `tickPrecipitation` 用得到。有真实高度图的实现应该覆盖它。
    fn heightmap_y(&self, pos: BlockPos) -> i32 {
        for y in (self.min_y()..=self.max_y()).rev() {
            let state = self.block_state(BlockPos::new(pos.x, y, pos.z));
            if state.is_tagged(mc_bamboo_data::tags::block_tags::BLOCKS_MOTION_IN_HEIGHTMAP)
                || !state.fluid().is_empty()
            {
                return y + 1;
            }
        }
        self.min_y() - 1
    }

    /// 这一格所在群系的降水条件，`Biome.getPrecipitationAt(pos, seaLevel)` 加上
    /// `Biome.warmEnoughToRain(pos, seaLevel)`。默认平原（测试用），真实实现读区块的
    /// 群系调色板再按 `mc-bamboo-worldgen` 算温度。
    fn precipitation_facts(&self, _pos: BlockPos) -> crate::weather::PrecipitationFacts {
        crate::weather::PrecipitationFacts::PLAINS
    }

    /// 这一格的物品栏，对应官方的 `getBlockContainer`（摆在方块里的那个）。
    ///
    /// 双箱已经按官方 `CompoundContainer` 的顺序合并成一张表。这一格不是容器方块时
    /// 不调用 `f`，返回 `false`。
    /// 漏斗取放物品时官方会忽略压住箱子的方块（`getContainerAt` 传 `ignoreBeingBlocked=true`），
    /// 所以这里的容器是能取到的那个，和比较器看到的可能不同。
    fn with_container(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut dyn Container)) -> bool {
        let _ = (pos, f);
        false
    }

    /// 这一格自己那张物品栏里每格有多少个物品，双箱只算这一半。
    ///
    /// 官方的 `BlockEntity.preRemoveSideEffects` 掉落的是方块实体自己的 `getItems`，
    /// 所以它和 [`Level::with_container`] 不同：不合并双箱，也不管压住箱子的方块。
    /// 没有物品表时返回空表，调用方按空槽处理。
    fn own_container_counts(&self, pos: BlockPos) -> Vec<i32> {
        let _ = pos;
        Vec::new()
    }

    /// 比较器从这一格的容器读到的模拟信号，官方 `getRedstoneSignalFromBlockEntity`。
    ///
    /// 双箱按合并后的 54 格算，和被压住读不到时是 0，见 `ChestBlock.getAnalogOutputSignal`。
    fn container_signal(&self, pos: BlockPos) -> i32 {
        let _ = pos;
        0
    }

    /// 这一格容器背后的方块位置：单箱是这一格，双箱是两半，见 `CompoundContainer.setChanged`。
    ///
    /// 和 [`Level::with_container`] 一样按能取到的那条规则算，被压住的箱子也算能取到。
    fn container_positions(&self, pos: BlockPos) -> Vec<BlockPos> {
        vec![pos]
    }

    /// 带着这一格的漏斗方块实体运行一段代码。这一格没有漏斗实体时不调用 `f`，返回 `false`。
    ///
    /// 实体正在 tick 时已经从表里拿出来了，这里取不到。漏斗的物品在槽位表，不在这个实体上。
    ///
    /// While the entity is ticking it is out of the map, so this cannot see it.
    /// Hopper items live in the slot table, not on the entity.
    fn with_hopper(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut HopperEntity)) -> bool {
        let _ = (pos, f);
        false
    }

    /// 雕纹书架、唱片机、饰纹陶罐和合成器的槽位表。没有这张表时不调用 `f`。
    ///
    /// Slot list for a chiseled bookshelf, jukebox, decorated pot or crafter. `f` is not called
    /// when the list is absent.
    fn with_slots(
        &mut self,
        pos: BlockPos,
        len: usize,
        f: &mut dyn FnMut(&mut [ItemStack]),
    ) -> bool {
        let _ = (pos, len, f);
        false
    }

    /// 这一格容器现在的槽位。没有容器时是空的。
    ///
    /// The container slots at this position, or an empty list when there is no container.
    fn slots_at(&self, pos: BlockPos) -> Vec<ItemStack> {
        let _ = pos;
        Vec::new()
    }

    /// 删掉这一格的容器槽位。破坏唱片机时 `setTheItem(EMPTY)` 在通知邻居之前清空。
    ///
    /// Drop the container slots at this position. Breaking a jukebox runs `setTheItem(EMPTY)`
    /// before neighbors are told.
    fn clear_container(&mut self, pos: BlockPos) {
        let _ = pos;
    }

    /// `BlockBehaviour.onExplosionHit` 里的 `state.getDrops`。
    ///
    /// TNT 的 `tntExplosionDropDecay` 默认是假，所以没有 `EXPLOSION_RADIUS`，`explosion_decay`
    /// 不取数。假世界没有战利品表时返回空，这样不会多取 `level.random`。
    ///
    /// `state.getDrops` inside `BlockBehaviour.onExplosionHit`.
    ///
    /// `tntExplosionDropDecay` defaults to false, so there is no `EXPLOSION_RADIUS` and
    /// `explosion_decay` draws nothing. A world without loot tables returns nothing, and does
    /// not draw extra `level.random` values.
    fn explosion_block_drops(&mut self, state: BlockState, pos: BlockPos) -> Vec<ItemStack> {
        let _ = (state, pos);
        Vec::new()
    }

    /// `Block.getDrops`，`destroyBlock` 在移除方块之前按战利品表算出的每一组。
    ///
    /// 空堆也交给调用方 `popResource`：官方对每一组都取三次位置偏移。
    /// 方块 oracle 把表换成空表，这里默认返回空，不额外取数。
    ///
    /// `Block.getDrops`, the stacks `destroyBlock` computes before removing the block.
    /// Even an empty stack reaches `popResource` in vanilla, which draws three position
    /// offsets. The block oracle empties those tables, so the default here draws nothing.
    fn block_loot_drops(&mut self, state: BlockState, pos: BlockPos) -> Vec<ItemStack> {
        let _ = (state, pos);
        Vec::new()
    }

    /// 官方 `RandomizableContainer.unpackLootTable` 的调用点。有战利品表的容器在
    /// 第一次被读或写之前先填好内容。默认空实现用于没有战利品表的假世界。
    ///
    /// The hook for vanilla `RandomizableContainer.unpackLootTable`. A container with a
    /// loot table is filled before its first read or write. Fake worlds leave this empty.
    fn unpack_loot_table(&mut self, _pos: BlockPos) {}

    /// 这一格有没有还没解包的战利品表。缓存守卫用它确认解包动作可以重放，
    /// 默认返回 `false`（假世界没有战利品表）；游戏层按容器条目覆盖。
    fn loot_table_pending(&self, _pos: BlockPos) -> bool {
        false
    }

    /// 官方 `ShulkerBoxBlock.playerWillDestroy` 的创造模式分支：非空潜影盒直接
    /// 掉出带 `BlockEntity.collectComponents()` 的方块物品。默认没有这类物品。
    ///
    /// Vanilla `ShulkerBoxBlock.playerWillDestroy` in creative mode: a non-empty
    /// shulker box drops the block item with `BlockEntity.collectComponents()`.
    fn creative_container_drop(&mut self, _pos: BlockPos, _item: i32) -> Option<ItemStack> {
        None
    }

    /// `BlockState.spawnAfterBreak` 里 `tryDropExperience` 和虫蚀方块生成蠹虫
    /// 可能要取的 `level.random` 数。没有经验球和生物时实体不生成，取数仍要照官方。
    ///
    /// `DropExperienceBlock` 的 `xpRange.sample(level.random)` 和 `ExperienceOrb.award`
    /// 的拆分次数都在这里；虫蚀方块不取 `level.random`。默认没有方块需要取数。
    ///
    /// The `level.random` draws inside `BlockState.spawnAfterBreak`: ore experience and
    /// infested-block spawns. Experience orbs and silverfish are not spawned, but the
    /// draws still follow vanilla. `DropExperienceBlock` samples `xpRange` and splits
    /// the amount in `ExperienceOrb.award`; infested blocks draw none. The default draws
    /// nothing.
    fn spawn_after_break_random(&mut self, state: BlockState, pos: BlockPos) {
        let _ = (state, pos);
    }
    /// `getEntitiesOfClass(LivingEntity, box)` 能看见的生物。默认没有。
    ///
    /// Living entities visible to `getEntitiesOfClass`. None by default.
    fn dispenser_targets(&self) -> Vec<EquipTarget> {
        Vec::new()
    }

    /// `LivingEntity.setItemSlot`。写不进这一格时返回 `None`。
    ///
    /// 返回值是玩家物品栏菜单槽，调用方用它发 `container_set_slot`。不是玩家时可以是任意非空值。
    ///
    /// `LivingEntity.setItemSlot`. `None` when the slot cannot be stored.
    ///
    /// The value is the player's inventory-menu slot, used to send `container_set_slot`.
    /// A non-player may return any `Some`.
    fn set_dispenser_equipment(
        &mut self,
        entity_id: i32,
        component_slot: i32,
        stack: ItemStack,
    ) -> Option<u16> {
        let _ = (entity_id, component_slot, stack);
        None
    }

    /// 取走录制包装产出的运行时记录。默认实现为空：只有 [`crate::recording::Recording`]
    /// 会返回内容，缓存驱动用它收成一条记录。
    fn runtime_records(&self) -> Vec<crate::cache::RuntimeRecord> {
        Vec::new()
    }

    /// 取走 [`RecordMode::Count`] 累计的读写项数。只有录制包装会返回非零值。
    fn take_counted_reads(&self) -> u64 {
        0
    }

    /// 缓存重放：原地换掉这一格的方块实体但保留 ticker 编号和顺序。
    /// 默认实现走 `insert`，会分配新编号；带表的实现覆盖它。
    fn replace_block_entity_for_cache(&mut self, entity: Box<dyn BlockEntity>) {
        self.insert_block_entity(entity);
    }

    /// 观察一次不经过本 trait 读写方法的事件，见 [`Observation`]。
    ///
    /// 默认是空的。接收者是 `&self`：读方法也要能报事件，录制实现用内部可变性记账。
    /// 实现者不能因为观察到事件而改变方块逻辑的结果。
    ///
    /// Observe an event that bypasses this trait's read/write methods; see
    /// [`Observation`]. The default does nothing. The receiver is `&self` because
    /// read-only paths report events too; a recording implementation uses
    /// interior mutability. An implementation must not change block behaviour.
    fn observe(&self, _event: Observation<'_>) {}
}

/// 红石用到的、方块状态装不下的世界数据，活得比一次操作长。
///
/// 官方把这些放在方块实体（比较器的输出、雕纹书架最近动过的格子、唱片机在不在放）
/// 和按世界分组的静态表（火把最近的开关）里。
#[derive(Debug, Default)]
pub struct RedstoneMemory {
    /// `ComparatorBlockEntity.outputSignal`。没有记录的格子是 0，和空的方块实体一样。
    comparator_outputs: HashMap<BlockPos, i32>,
    /// `ChiseledBookShelfBlockEntity.lastInteractedSlot`。没有记录的格子是 -1，也就是没动过。
    bookshelf_slots: HashMap<BlockPos, usize>,
    /// `JukeboxBlockEntity` 的 `songPlayer.isPlaying()`。唱片在容器槽位里。
    /// 场景里直接摆下的 `has_record=true` 唱机没有经过 `setTheItem`，所以没在放。
    ///
    /// `JukeboxSongPlayer.isPlaying`. The disc lives in the container slot.
    /// A scene jukebox placed with `has_record=true` never went through `setTheItem`, so it is
    /// not playing.
    jukebox_playing: HashSet<BlockPos>,
    /// `CrafterBlockEntity` 的禁用槽。位 `n` 为 1 表示第 `n` 格禁用。没有记录时全是 0。
    ///
    /// Disabled slots of a `CrafterBlockEntity`. Bit `n` set means slot `n` is disabled.
    /// Missing positions are all enabled.
    crafter_disabled: HashMap<BlockPos, u16>,
    /// `CrafterBlockEntity.craftingTicksRemaining` 的到期时刻（绝对 `gameTime`）。
    /// 官方每刻减 1，归零时关掉 `crafting`；记到期时刻后没到期的条目每刻不用碰。
    ///
    /// Absolute expiry `gameTime` of `craftingTicksRemaining`.
    crafter_crafting: HashMap<BlockPos, i64>,
    /// 到期时刻 → 位置，只处理这一刻到期的合成器。
    crafter_due: BTreeMap<i64, Vec<BlockPos>>,
    /// `RedstoneTorchBlock.RECENT_TOGGLES` 里属于这个世界的那一项，按时间排好。
    torch_toggles: Vec<(BlockPos, i64)>,
    /// `LecternBlockEntity` 的页码。比较器只读得了这三项，书本身在游戏层。
    ///
    /// Page, page count, and whether the book has writable or written content.
    /// The item stack itself lives in the game layer.
    lecterns: HashMap<BlockPos, LecternFacts>,
    /// 这次右键即将放上的书。方块逻辑看不见物品组件，由调用方先放进来。
    ///
    /// The book about to be placed. Block logic cannot see item components,
    /// so the caller deposits the facts before the use.
    pending_lectern: Option<LecternFacts>,
    /// 每个区块里**可能**有红石记忆的位置，是超集：一张表里删掉的格子可不去掉。
    /// 只在某张表新增键时登记，按区块取出时只走这个集合。
    by_chunk: HashMap<ChunkPos, HashSet<BlockPos>>,
}

impl RedstoneMemory {
    /// 某张表第一次记下这一格时登记位置。
    fn note_pos(&mut self, pos: BlockPos) {
        self.by_chunk.entry(pos.chunk()).or_default().insert(pos);
    }

    /// `forget_block_entity` 把这一格的所有记忆都清掉，索引也一起删。
    fn forget_pos(&mut self, pos: BlockPos) {
        let chunk = pos.chunk();
        let Some(positions) = self.by_chunk.get_mut(&chunk) else {
            return;
        };
        positions.remove(&pos);
        if positions.is_empty() {
            self.by_chunk.remove(&chunk);
        }
    }
}

/// 讲台比较器要的三项，对应 `LecternBlockEntity` 的 `page`、`pageCount` 和 `hasBook()`。
///
/// The three facts a lectern comparator reads: `page`, `pageCount`, and `hasBook()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LecternFacts {
    pub page: i32,
    pub page_count: i32,
    pub has_content: bool,
}

impl LecternFacts {
    /// `LecternBlockEntity.getRedstoneSignal`。
    ///
    /// `pageCount > 1` 时进度是 `page / (pageCount - 1)`，否则是 1。
    /// `Mth.floor` 先把 float 提升成 double。
    ///
    /// Progress is `page / (pageCount - 1)` when `pageCount > 1`, otherwise 1.
    /// `Mth.floor` widens the float to double before truncating.
    pub fn signal(self) -> i32 {
        let progress = if self.page_count > 1 {
            self.page as f32 / (self.page_count as f32 - 1.0_f32)
        } else {
            1.0_f32
        };
        mc_bamboo_core::mth::floor(f64::from(progress * 14.0_f32)) + i32::from(self.has_content)
    }
}

impl RedstoneMemory {
    /// `ComparatorBlockEntity.getOutputSignal`。
    pub fn comparator_output(&self, pos: BlockPos) -> i32 {
        self.comparator_outputs.get(&pos).copied().unwrap_or(0)
    }

    /// `ComparatorBlockEntity.setOutputSignal`。
    pub fn set_comparator_output(&mut self, pos: BlockPos, signal: i32) {
        if self.comparator_outputs.insert(pos, signal).is_none() {
            self.note_pos(pos);
        }
    }

    /// `ChiseledBookShelfBlockEntity.getLastInteractedSlot`，没动过时是 `None`。
    pub fn last_bookshelf_slot(&self, pos: BlockPos) -> Option<usize> {
        self.bookshelf_slots.get(&pos).copied()
    }

    /// 放进或者拿出一本书时记下是哪一格。
    pub fn set_last_bookshelf_slot(&mut self, pos: BlockPos, slot: usize) {
        if self.bookshelf_slots.insert(pos, slot).is_none() {
            self.note_pos(pos);
        }
    }

    /// `JukeboxSongPlayer.isPlaying`。
    pub fn is_jukebox_playing(&self, pos: BlockPos) -> bool {
        self.jukebox_playing.contains(&pos)
    }

    /// `JukeboxSongPlayer.play`、`stop` 之后记下这一格在不在放。
    pub fn set_jukebox_playing(&mut self, pos: BlockPos, playing: bool) {
        if playing {
            if self.jukebox_playing.insert(pos) {
                self.note_pos(pos);
            }
        } else {
            self.jukebox_playing.remove(&pos);
        }
    }

    /// `CrafterBlockEntity.isSlotDisabled` 的九个位。
    ///
    /// The nine bits behind `CrafterBlockEntity.isSlotDisabled`.
    pub fn crafter_disabled(&self, pos: BlockPos) -> u16 {
        self.crafter_disabled.get(&pos).copied().unwrap_or(0)
    }

    /// 写下合成器的禁用槽。全是 0 时不留记录。
    ///
    /// Store a crafter's disabled slots. An all-zero mask is not recorded.
    pub fn set_crafter_disabled(&mut self, pos: BlockPos, mask: u16) {
        if mask == 0 {
            self.crafter_disabled.remove(&pos);
        } else if self.crafter_disabled.insert(pos, mask).is_none() {
            self.note_pos(pos);
        }
    }

    /// `CrafterBlockEntity.setCraftingTicksRemaining`：亮起 `crafting` 时记下还剩几刻。
    ///
    /// 内部记的是绝对到期时刻，配合 `crafter_due` 只处理到期的条目。
    ///
    /// `CrafterBlockEntity.setCraftingTicksRemaining`: record the countdown when `crafting`
    /// turns on. Stored as an absolute expiry so only due entries are visited each tick.
    pub fn set_crafter_crafting(&mut self, pos: BlockPos, remaining: i32, now: i64) {
        if remaining > 0 {
            let at = now.saturating_add(i64::from(remaining));
            if let Some(old) = self.crafter_crafting.insert(pos, at) {
                self.unschedule_crafter(old, pos);
            } else {
                self.note_pos(pos);
            }
            self.crafter_due.entry(at).or_default().push(pos);
        } else {
            if let Some(old) = self.crafter_crafting.remove(&pos) {
                self.unschedule_crafter(old, pos);
            }
        }
    }

    fn unschedule_crafter(&mut self, at: i64, pos: BlockPos) {
        if let std::collections::btree_map::Entry::Occupied(mut entry) = self.crafter_due.entry(at)
        {
            entry.get_mut().retain(|&other| other != pos);
            if entry.get().is_empty() {
                entry.remove();
            }
        }
    }

    /// 取走全部倒计时，供测试和差分对照用；返回的是还剩几刻。
    ///
    /// Take every countdown (as remaining ticks); for tests and differential comparisons.
    pub fn take_crafter_crafting(&mut self, now: i64) -> Vec<(BlockPos, i32)> {
        self.crafter_due.clear();
        std::mem::take(&mut self.crafter_crafting)
            .into_iter()
            .map(|(pos, at)| (pos, i32::try_from(at.saturating_sub(now)).unwrap_or(0)))
            .collect()
    }

    /// 把还没到期的倒计时放回去，返回的是还剩几刻。
    ///
    /// Put the countdowns (as remaining ticks) back.
    pub fn restore_crafter_crafting(&mut self, entries: Vec<(BlockPos, i32)>, now: i64) {
        for (pos, remaining) in entries {
            self.set_crafter_crafting(pos, remaining, now);
        }
    }

    /// 取走这一刻到期的倒计时。它们到点归零，官方关掉 `crafting` 方块状态。
    /// 没到期的条目留在表里，这一刻完全不碰。
    ///
    /// Take only the countdowns expiring at `now`; untouched entries stay in the table.
    pub fn take_due_crafter_crafting(&mut self, now: i64) -> Vec<BlockPos> {
        let mut due = Vec::new();
        while let Some(entry) = self.crafter_due.first_entry()
            && *entry.key() <= now
        {
            for pos in entry.remove() {
                if self.crafter_crafting.remove(&pos).is_some() {
                    due.push(pos);
                }
            }
        }
        due.sort_unstable();
        due
    }

    /// 这一格换了方块，官方把方块实体删掉，下次新建的是空的。
    pub fn forget_block_entity(&mut self, pos: BlockPos) {
        self.comparator_outputs.remove(&pos);
        self.bookshelf_slots.remove(&pos);
        self.jukebox_playing.remove(&pos);
        self.crafter_disabled.remove(&pos);
        self.crafter_crafting.remove(&pos);
        self.lecterns.remove(&pos);
        self.forget_pos(pos);
    }

    /// 这次交互要放上的书。方块行为在 `placeBook` 里取走。
    ///
    /// Facts for the book the current interaction is placing. `placeBook` takes them.
    pub fn set_pending_lectern(&mut self, facts: LecternFacts) {
        self.pending_lectern = Some(facts);
    }

    /// 取走上一段放进来的书。没有就返回 `None`。
    ///
    /// Takes the facts deposited for this interaction, if any.
    pub fn take_pending_lectern(&mut self) -> Option<LecternFacts> {
        self.pending_lectern.take()
    }

    /// 记下这一格讲台的页码。
    ///
    /// Stores the lectern facts at `pos`.
    pub fn set_lectern(&mut self, pos: BlockPos, facts: LecternFacts) {
        if self.lecterns.insert(pos, facts).is_none() {
            self.note_pos(pos);
        }
    }

    /// 这一格讲台的页码。没有记录时是 `None`。
    ///
    /// Lectern facts at `pos`, if a book was recorded there.
    pub fn lectern(&self, pos: BlockPos) -> Option<LecternFacts> {
        self.lecterns.get(&pos).copied()
    }

    /// 取书或拆掉讲台时丢掉页码。
    ///
    /// Drops the lectern facts when the book is removed or the block goes away.
    pub fn clear_lectern(&mut self, pos: BlockPos) {
        self.lecterns.remove(&pos);
    }

    /// 清空开关记录。存档往返用：官方不存 `RECENT_TOGGLES`，读回后是空的。
    pub fn clear_torch_toggles(&mut self) {
        self.torch_toggles.clear();
    }

    /// 扔掉 `when` 之前的开关记录，官方在红石火把的计划刻开头做这件事。
    pub fn forget_torch_toggles_before(&mut self, when: i64) {
        let keep = self
            .torch_toggles
            .iter()
            .position(|&(_, toggled_at)| toggled_at >= when)
            .unwrap_or(self.torch_toggles.len());
        self.torch_toggles.drain(..keep);
    }

    /// 记下一次开关。
    pub fn record_torch_toggle(&mut self, pos: BlockPos, when: i64) {
        self.torch_toggles.push((pos, when));
    }

    /// 这一格还留着几次开关记录。
    pub fn torch_toggle_count(&self, pos: BlockPos) -> usize {
        self.torch_toggles
            .iter()
            .filter(|&&(toggled, _)| toggled == pos)
            .count()
    }

    /// 把 `chunk` 里的红石记忆装进盒子搬走。只在 tick 边界调用。
    ///
    /// 火把开关记录带世界时间，盒子里改成相对年龄（`now` - 记录时刻），
    /// 这样目标表的时钟不同也能保持剩余寿命。`pending_lectern` 是一次交互内部的
    /// 瞬时量，tick 边界上一定是 `None`，不搬。
    pub fn take_chunk(&mut self, chunk: ChunkPos, now: i64) -> ChunkRedstone {
        debug_assert!(self.pending_lectern.is_none());
        let mut memory = ChunkRedstone::default();
        if let Some(positions) = self.by_chunk.remove(&chunk) {
            let mut positions: Vec<BlockPos> = positions.into_iter().collect();
            positions.sort_unstable();
            for pos in positions {
                // 索引是超集，已经删掉的格子查出来就是空。
                if let Some(signal) = self.comparator_outputs.remove(&pos) {
                    memory.comparator_outputs.push((pos, signal));
                }
                if let Some(slot) = self.bookshelf_slots.remove(&pos) {
                    memory.bookshelf_slots.push((pos, slot));
                }
                if self.jukebox_playing.remove(&pos) {
                    memory.jukebox_playing.push(pos);
                }
                if let Some(mask) = self.crafter_disabled.remove(&pos) {
                    memory.crafter_disabled.push((pos, mask));
                }
                if let Some(at) = self.crafter_crafting.remove(&pos) {
                    // 盒子里存剩余刻数；目标表的时钟不同也能保持剩余寿命。
                    let remaining = i32::try_from(at.saturating_sub(now)).unwrap_or(0);
                    memory.crafter_crafting.push((pos, remaining));
                }
                self.crafter_due.retain(|_, positions| {
                    positions.retain(|other| other.chunk() != chunk);
                    !positions.is_empty()
                });
                if let Some(facts) = self.lecterns.remove(&pos) {
                    memory.lecterns.push((pos, facts));
                }
            }
        }
        // 火把记录不进位置索引，得单独取；它只保留最近 60 刻，很短，允许整表扫。
        memory.torch_toggles = self
            .torch_toggles
            .iter()
            .filter(|&&(pos, _)| pos.chunk() == chunk)
            .map(|&(pos, toggled_at)| (pos, now.saturating_sub(toggled_at)))
            .collect();
        self.torch_toggles.retain(|&(pos, _)| pos.chunk() != chunk);
        memory
    }

    /// 只读快照：和 [`Self::take_chunk`] 一样收集这一区块会存盘的红石记忆，
    /// 但只 `.get()` 不 `.remove()`，也不要求 `pending_lectern` 为空（补 11）。
    /// 给已加载区块的只读存档编码用，原表分毫不动。
    /// Read-only snapshot: gather the same saved redstone state as `take_chunk`
    /// with `.get()` instead of `.remove()`, no `pending_lectern` precondition
    /// (item 11). The original memory is untouched.
    pub fn snapshot_chunk(&self, chunk: ChunkPos, now: i64) -> ChunkRedstone {
        let mut memory = ChunkRedstone::default();
        if let Some(positions) = self.by_chunk.get(&chunk) {
            let mut positions: Vec<BlockPos> = positions.iter().copied().collect();
            positions.sort_unstable();
            for pos in positions {
                if let Some(&signal) = self.comparator_outputs.get(&pos) {
                    memory.comparator_outputs.push((pos, signal));
                }
                if let Some(&slot) = self.bookshelf_slots.get(&pos) {
                    memory.bookshelf_slots.push((pos, slot));
                }
                if self.jukebox_playing.contains(&pos) {
                    memory.jukebox_playing.push(pos);
                }
                if let Some(&mask) = self.crafter_disabled.get(&pos) {
                    memory.crafter_disabled.push((pos, mask));
                }
                if let Some(&at) = self.crafter_crafting.get(&pos) {
                    // 和 take_chunk 一样存剩余刻数，不存绝对到期时刻。
                    let remaining = i32::try_from(at.saturating_sub(now)).unwrap_or(0);
                    memory.crafter_crafting.push((pos, remaining));
                }
                if let Some(&facts) = self.lecterns.get(&pos) {
                    memory.lecterns.push((pos, facts));
                }
            }
        }
        memory.torch_toggles = self
            .torch_toggles
            .iter()
            .filter(|&&(pos, _)| pos.chunk() == chunk)
            .map(|&(pos, toggled_at)| (pos, now.saturating_sub(toggled_at)))
            .collect();
        memory
    }

    /// 把盒子里的红石记忆放回表里。只在 tick 边界调用。
    ///
    /// 火把记录按目标表的 `now` 从相对年龄换算回世界时间，再用稳定排序接回
    /// 表里已有记录的后面；同一时刻内的先后不影响任何查询。
    /// 调用方保证表里已经没有 `chunk` 的条目。
    pub fn insert_chunk(&mut self, memory: ChunkRedstone, now: i64) {
        for (pos, signal) in memory.comparator_outputs {
            self.set_comparator_output(pos, signal);
        }
        for (pos, slot) in memory.bookshelf_slots {
            self.set_last_bookshelf_slot(pos, slot);
        }
        for pos in memory.jukebox_playing {
            self.set_jukebox_playing(pos, true);
        }
        for (pos, mask) in memory.crafter_disabled {
            self.set_crafter_disabled(pos, mask);
        }
        for (pos, remaining) in memory.crafter_crafting {
            // 盒子里是剩余刻数，按目标表的 `now` 换算成新的到期时刻。
            self.set_crafter_crafting(pos, remaining, now);
        }
        for (pos, facts) in memory.lecterns {
            self.set_lectern(pos, facts);
        }
        for (pos, age) in memory.torch_toggles {
            self.torch_toggles.push((pos, now.saturating_sub(age)));
        }
        // 稳定排序：原有条目在前，放入的在后，整体仍按时刻升序。
        self.torch_toggles
            .sort_by_key(|&(_, toggled_at)| toggled_at);
    }
}

/// 一个区块的全部红石记忆，按位置排序。火把开关记录存的是相对年龄。
#[derive(Debug, Default)]
pub struct ChunkRedstone {
    comparator_outputs: Vec<(BlockPos, i32)>,
    bookshelf_slots: Vec<(BlockPos, usize)>,
    jukebox_playing: Vec<BlockPos>,
    crafter_disabled: Vec<(BlockPos, u16)>,
    crafter_crafting: Vec<(BlockPos, i32)>,
    lecterns: Vec<(BlockPos, LecternFacts)>,
    torch_toggles: Vec<(BlockPos, i64)>,
}

impl ChunkRedstone {
    /// 存档编码用：逐项只读遍历建模的红石记忆。
    pub fn archive_entries(&self) -> ArchiveRedstone<'_> {
        ArchiveRedstone {
            comparator_outputs: &self.comparator_outputs,
            bookshelf_slots: &self.bookshelf_slots,
            jukebox_playing: &self.jukebox_playing,
            crafter_disabled: &self.crafter_disabled,
            crafter_crafting: &self.crafter_crafting,
            lecterns: &self.lecterns,
        }
    }

    /// 存档解码用：把一项红石记忆放回盒子。火把抖动表官方不存，不写进盒子。
    pub fn push_archive_entry(&mut self, entry: RedstoneEntry) {
        match entry {
            RedstoneEntry::Comparator { pos, signal } => {
                self.comparator_outputs.push((pos, signal));
            }
            RedstoneEntry::Bookshelf { pos, slot } => {
                self.bookshelf_slots.push((pos, slot));
            }
            RedstoneEntry::Jukebox { pos } => self.jukebox_playing.push(pos),
            RedstoneEntry::CrafterDisabled { pos, mask } => {
                self.crafter_disabled.push((pos, mask));
            }
            RedstoneEntry::CrafterCrafting { pos, remaining } => {
                self.crafter_crafting.push((pos, remaining));
            }
            RedstoneEntry::Lectern { pos, facts } => self.lecterns.push((pos, facts)),
        }
    }

    /// 盒子里有没有红石记忆。`is_pristine` 用它判断盒子能不能丢掉。
    pub fn is_empty(&self) -> bool {
        self.comparator_outputs.is_empty()
            && self.bookshelf_slots.is_empty()
            && self.jukebox_playing.is_empty()
            && self.crafter_disabled.is_empty()
            && self.crafter_crafting.is_empty()
            && self.lecterns.is_empty()
            && self.torch_toggles.is_empty()
    }
}

/// [`ChunkRedstone::archive_entries`] 的只读视图。
pub struct ArchiveRedstone<'a> {
    pub comparator_outputs: &'a [(BlockPos, i32)],
    pub bookshelf_slots: &'a [(BlockPos, usize)],
    pub jukebox_playing: &'a [BlockPos],
    pub crafter_disabled: &'a [(BlockPos, u16)],
    pub crafter_crafting: &'a [(BlockPos, i32)],
    pub lecterns: &'a [(BlockPos, LecternFacts)],
}

/// 一条能从方块实体 NBT 还原的红石记忆。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedstoneEntry {
    Comparator { pos: BlockPos, signal: i32 },
    Bookshelf { pos: BlockPos, slot: usize },
    Jukebox { pos: BlockPos },
    CrafterDisabled { pos: BlockPos, mask: u16 },
    CrafterCrafting { pos: BlockPos, remaining: i32 },
    Lectern { pos: BlockPos, facts: LecternFacts },
}

const _: fn() = || {
    fn assert_send_debug<T: Send + fmt::Debug>() {}
    assert_send_debug::<ChunkRedstone>();
};

/// 一次操作留下的、方块以外的结果，由调用方发给玩家。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    /// `levelEvent(2001, pos, state)`。`except_initiator` 表示不用发给发起者，它自己已经预测过了。
    Destroyed {
        pos: BlockPos,
        state: BlockState,
        except_initiator: bool,
    },
    /// `levelEvent(1502, pos, 0)`：红石火把开关得太频繁，烧坏时冒的那股烟。
    TorchBurnout { pos: BlockPos },
    /// 空桶舀起了东西。游戏层按 `ItemUtils.createFilledResult` 把 `item` 交还到 `hand`。
    ///
    /// An empty bucket picked something up. The game layer runs `ItemUtils.createFilledResult`
    /// and puts `item` back into `hand`.
    FilledBucket { item: i32, hand: Hand },
    /// 满桶倒成功了。生存模式交还空桶，创造模式交还手里原来的那一堆。
    ///
    /// A full bucket emptied successfully. Survival hands back an empty bucket. Creative hands
    /// back the stack that was already in the hand.
    EmptiedBucket { hand: Hand },
    /// `levelEvent(1505, pos, 15)`：骨粉起作用的粒子，发给附近所有人。
    BoneMealUsed { pos: BlockPos },
    /// 告示牌的内容变了，`SignBlockEntity.markUpdated` 里的 `sendBlockUpdated`：
    /// 方块状态没变，但要把方块和方块实体数据重新发给追踪这个区块的玩家。
    SignChanged { pos: BlockPos },
    /// 上蜡成功的光照效果，`levelEvent(null, 3003, pos, 0)`。
    SignWaxed { pos: BlockPos },
    /// `SignBlock.openTextEdit`：给这名玩家打开编辑器，`player.openTextEdit(sign, slot)`。
    OpenSignEditor { pos: BlockPos, slot: SignTextSlot },
    /// `playSound` / `playSeededSound`：坐标乘 8 截断后发给范围内的玩家。
    Sound {
        /// `sound_event` 注册表 ID。
        sound: i32,
        source: SoundSource,
        x: f64,
        y: f64,
        z: f64,
        volume: f32,
        pitch: f32,
        /// 官方传的 `except` 是发起这次操作的玩家。
        except_initiator: bool,
        /// 显式给的种子。音符盒用 `Level.random.nextLong()`，其余由网络层另取。
        seed: Option<i64>,
    },
    /// `levelEvent`：客户端自己放粒子和声音。传播范围是 64 格，坐标是方块整数坐标。
    LevelEvent {
        event: i32,
        pos: BlockPos,
        data: i32,
        except_initiator: bool,
    },
    /// 音符盒响了，`NoteBlock.triggerEvent`。`pitch` 是官方的 `getPitchFromNote(note)`，
    /// `seed` 是官方传给 `playSeededSound` 的那个随机数。
    ///
    /// 上面是带 `note_block_sound` 的头颅时，官方播放的是那个音效，不按乐器查表。
    /// 名字在这一次操作的自定义音效表里，[`NoteBlockPlayed::custom`] 是下标。
    NoteBlockPlayed {
        pos: BlockPos,
        instrument: NoteBlockInstrument,
        note: i32,
        pitch: f32,
        seed: i64,
        /// 自定义音效在 [`intern_custom_sound`] 那张表里的下标。`None` 用乐器表。
        custom: Option<u32>,
    },
    /// `ServerLevel.runBlockEvents` 里的 `ClientboundBlockEventPacket`：方块事件处理成功了，
    /// 把这一项发给附近 64 格内的玩家。`block` 是方块的注册表 ID，官方包里的第三个字段。
    BlockEvent {
        pos: BlockPos,
        block: usize,
        param_a: i32,
        param_b: i32,
    },
    /// `levelEvent(1500, pos, data)`：堆肥桶收进一个能堆肥的物品。
    /// `success` 为真时数据是 1（等级升了），否则是 0。客户端自己播粒子和声音。
    ComposterFill { pos: BlockPos, success: bool },
    /// 讲台翻页，`levelEvent(1043, pos, 0)`。客户端自己播翻页的声音。
    ///
    /// `LecternBlock.signalPageChange` emits `levelEvent(1043, pos, 0)`.
    /// The client plays the page-turn sound. Nearby players, including the one
    /// who turned the page, all receive it.
    PageTurned { pos: BlockPos },
    /// 发射器把一件装备写进了这名玩家的物品栏。菜单槽已经改过，调用方发 `container_set_slot`。
    ///
    /// A dispenser wrote equipment into this player's inventory. The menu slot is already
    /// updated. The caller sends `container_set_slot`.
    DispenserEquipped { entity_id: i32, menu_slot: u16 },
    /// 营火上的食物变了，`CampfireBlockEntity.getUpdateTag` 要重新发给客户端。
    ///
    /// Food on a campfire changed. `CampfireBlockEntity.getUpdateTag` has to be sent again.
    CampfireChanged { pos: BlockPos },
    /// 快捷栏上方的提示。`key` 是翻译键，参数是已经序列化好的文本。
    ///
    /// An action-bar message. `key` is the translation key and the arguments are already text.
    Overlay {
        key: &'static str,
        first: Option<&'static str>,
        second: Option<&'static str>,
    },
    /// 有权限的玩家右键了命令方块。客户端自己打开编辑界面，服务端补一份方块实体数据。
    ///
    /// A permitted player used a command block. The client opens the editor; the server sends
    /// the block-entity data.
    OpenCommandBlock { pos: BlockPos },
}

/// 自定义音效的名字。`Effect` 要能按值拷贝，名字放在这里，效果里只留下标。
///
/// 一次结算会把好几个世界的效果拼到一起，这张表不能挂在某一个世界上。
static CUSTOM_SOUNDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// 记下一个 `SoundEvent.createVariableRangeEvent` 的标识符，返回下标。
pub fn intern_custom_sound(name: &str) -> u32 {
    let mut table = CUSTOM_SOUNDS.lock().expect("custom sound table");
    if let Some(index) = table.iter().position(|existing| existing == name) {
        return u32::try_from(index).expect("custom sound index");
    }
    let index = u32::try_from(table.len()).expect("custom sound index");
    table.push(name.to_owned());
    index
}

/// 按下标取回自定义音效的标识符。
pub fn custom_sound_name(index: u32) -> String {
    CUSTOM_SOUNDS
        .lock()
        .expect("custom sound table")
        .get(index as usize)
        .cloned()
        .expect("custom sound index")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 不实现 `LevelTables`、直接实现 `LevelSideState` 也能当 trait 对象用：
    /// 按区块存或者录制读写的实现者走的就是这条路。
    struct NoTables;

    impl LevelSideState for NoTables {
        fn comparator_output(&self, _pos: BlockPos) -> i32 {
            unimplemented!()
        }
        fn set_comparator_output(&mut self, _pos: BlockPos, _signal: i32) {
            unimplemented!()
        }
        fn last_bookshelf_slot(&self, _pos: BlockPos) -> Option<usize> {
            unimplemented!()
        }
        fn set_last_bookshelf_slot(&mut self, _pos: BlockPos, _slot: usize) {
            unimplemented!()
        }
        fn is_jukebox_playing(&self, _pos: BlockPos) -> bool {
            unimplemented!()
        }
        fn set_jukebox_playing(&mut self, _pos: BlockPos, _playing: bool) {
            unimplemented!()
        }
        fn crafter_disabled(&self, _pos: BlockPos) -> u16 {
            unimplemented!()
        }
        fn set_crafter_disabled(&mut self, _pos: BlockPos, _mask: u16) {
            unimplemented!()
        }
        fn set_crafter_crafting(&mut self, _pos: BlockPos, _remaining: i32, _now: i64) {
            unimplemented!()
        }
        fn take_crafter_crafting(&mut self, _now: i64) -> Vec<(BlockPos, i32)> {
            unimplemented!()
        }
        fn restore_crafter_crafting(&mut self, _entries: Vec<(BlockPos, i32)>, _now: i64) {
            unimplemented!()
        }
        fn take_due_crafter_crafting(&mut self, _now: i64) -> Vec<BlockPos> {
            unimplemented!()
        }
        fn forget_block_entity_memory(&mut self, _pos: BlockPos) {
            unimplemented!()
        }
        fn set_pending_lectern(&mut self, _facts: LecternFacts) {
            unimplemented!()
        }
        fn take_pending_lectern(&mut self) -> Option<LecternFacts> {
            unimplemented!()
        }
        fn set_lectern(&mut self, _pos: BlockPos, _facts: LecternFacts) {
            unimplemented!()
        }
        fn lectern(&self, _pos: BlockPos) -> Option<LecternFacts> {
            unimplemented!()
        }
        fn clear_lectern(&mut self, _pos: BlockPos) {
            unimplemented!()
        }
        fn forget_torch_toggles_before(&mut self, _when: i64) {
            unimplemented!()
        }
        fn record_torch_toggle(&mut self, _pos: BlockPos, _when: i64) {
            unimplemented!()
        }
        fn torch_toggle_count(&self, _pos: BlockPos) -> usize {
            unimplemented!()
        }
        fn push_block_event(&mut self, _event: BlockEvent) {
            unimplemented!()
        }
        fn pop_block_event(&mut self) -> Option<BlockEvent> {
            unimplemented!()
        }
        fn block_entity_dyn(&self, _pos: BlockPos) -> Option<&dyn BlockEntity> {
            unimplemented!()
        }
        fn block_entity_sleep_enabled(&self) -> bool {
            unimplemented!()
        }
        fn block_entity_sleep_from(&mut self, _pos: BlockPos, _now: i64) -> Option<i64> {
            unimplemented!()
        }
        fn has_block_entity(&self, _pos: BlockPos) -> bool {
            unimplemented!()
        }
        fn insert_block_entity(&mut self, _entity: Box<dyn BlockEntity>) {
            unimplemented!()
        }
        fn remove_block_entity(&mut self, _pos: BlockPos) -> Option<Box<dyn BlockEntity>> {
            unimplemented!()
        }
        fn take_block_entity_at(&mut self, _pos: BlockPos) -> Option<(u64, Box<dyn BlockEntity>)> {
            unimplemented!()
        }
        fn take_block_entity(&mut self, _pos: BlockPos, _id: u64) -> Option<Box<dyn BlockEntity>> {
            unimplemented!()
        }
        fn put_block_entity(&mut self, _pos: BlockPos, _id: u64, _entity: Box<dyn BlockEntity>) {
            unimplemented!()
        }
        fn is_block_entity_taken(&self, _pos: BlockPos) -> bool {
            unimplemented!()
        }
        fn wake_block_entity(&mut self, _pos: BlockPos) {
            unimplemented!()
        }
        fn schedule_block_entity_wake(&mut self, _pos: BlockPos, _wake_at: Option<i64>) {
            unimplemented!()
        }
        fn set_chunk_block_entity_ticking(&mut self, _chunk: ChunkPos, _ticks: bool) {
            unimplemented!()
        }
        fn sync_block_entity_border(&mut self, _border: BlockBox) {
            unimplemented!()
        }
        fn begin_block_entity_tick(&mut self, _now: i64) {
            unimplemented!()
        }
        fn next_block_entity_ticker(&mut self) -> Option<(BlockPos, u64)> {
            unimplemented!()
        }
        fn end_block_entity_tick(&mut self) {
            unimplemented!()
        }
    }

    /// 两个 trait 都要 dyn 安全：`Level` 的调用方拿的是 trait 对象，
    /// 按区块存或录制读写的实现者可以只实现 `LevelSideState`。
    #[test]
    fn side_state_traits_are_object_safe() {
        fn assert_object_safe(_: &dyn LevelSideState, _: &dyn LevelTables) {}
        let state: &dyn LevelSideState = &NoTables;
        let _ = state as *const dyn LevelSideState;
        let _ = assert_object_safe;
    }

    const CHUNK_A: ChunkPos = ChunkPos::new(0, 0);
    const A1: BlockPos = BlockPos::new(1, 64, 1);
    const A2: BlockPos = BlockPos::new(2, 64, 2);
    const B1: BlockPos = BlockPos::new(20, 64, 1);

    const FACTS: LecternFacts = LecternFacts {
        page: 2,
        page_count: 5,
        has_content: true,
    };

    /// 填满六张表里属于 A1 / B1 的条目，火把记录落在 100 / 101 / 102 刻。
    fn fill(memory: &mut RedstoneMemory) {
        memory.set_comparator_output(A1, 7);
        memory.set_last_bookshelf_slot(A1, 3);
        memory.set_jukebox_playing(A1, true);
        memory.set_crafter_disabled(A1, 0b101);
        // 这些测试用 `now = 102` 装盒/放回，倒计时也按这个时钟登记。
        memory.set_crafter_crafting(A1, 4, 102);
        memory.set_lectern(A1, FACTS);
        memory.set_comparator_output(B1, 9);
        memory.set_crafter_disabled(B1, 0b11);
        memory.record_torch_toggle(A1, 100);
        memory.record_torch_toggle(B1, 101);
        memory.record_torch_toggle(A1, 102);
    }

    /// 所有能观察到的读，用来比较搬前搬后。倒计时取走后原样放回。
    fn observed(memory: &mut RedstoneMemory, now: i64) -> Vec<String> {
        let countdowns = memory.take_crafter_crafting(102);
        let mut out = Vec::new();
        for pos in [A1, A2, B1] {
            out.push(format!(
                "{pos:?}:{}:{}:{}:{}:{}:{}:{}",
                memory.comparator_output(pos),
                memory.last_bookshelf_slot(pos).unwrap_or(usize::MAX),
                memory.is_jukebox_playing(pos),
                memory.crafter_disabled(pos),
                memory.lectern(pos).is_some(),
                memory.torch_toggle_count(pos),
                countdowns.iter().filter(|&&(at, _)| at == pos).count()
            ));
        }
        memory.restore_crafter_crafting(countdowns, 102);
        // `forget_torch_toggles_before` 也依赖时间顺序，搬完以后它仍要按时刻工作。
        memory.forget_torch_toggles_before(now);
        out.push(format!("toggles:{}", memory.torch_toggles.len()));
        out
    }

    #[test]
    fn taking_a_chunk_leaves_no_trace_and_keeps_other_chunks() {
        let mut memory = RedstoneMemory::default();
        fill(&mut memory);
        let boxed = memory.take_chunk(CHUNK_A, 102);
        assert!(format!("{boxed:?}").contains("ChunkRedstone"));
        assert_eq!(memory.comparator_output(A1), 0);
        assert_eq!(memory.last_bookshelf_slot(A1), None);
        assert!(!memory.is_jukebox_playing(A1));
        assert_eq!(memory.crafter_disabled(A1), 0);
        assert_eq!(memory.lectern(A1), None);
        assert_eq!(memory.torch_toggle_count(A1), 0);
        // 别的区块一点没动。
        assert_eq!(memory.comparator_output(B1), 9);
        assert_eq!(memory.crafter_disabled(B1), 0b11);
        assert_eq!(memory.torch_toggle_count(B1), 1);
        assert_eq!(memory.take_crafter_crafting(102).len(), 0);

        memory.insert_chunk(boxed, 102);
        assert_eq!(memory.comparator_output(A1), 7);
        assert_eq!(memory.last_bookshelf_slot(A1), Some(3));
        assert!(memory.is_jukebox_playing(A1));
        assert_eq!(memory.crafter_disabled(A1), 0b101);
        assert_eq!(memory.lectern(A1), Some(FACTS));
        assert_eq!(memory.torch_toggle_count(A1), 2);
        assert_eq!(memory.take_crafter_crafting(102), [(A1, 4)]);
    }

    #[test]
    fn taking_and_putting_back_keeps_the_same_observations() {
        let mut untouched = RedstoneMemory::default();
        let mut moved = RedstoneMemory::default();
        fill(&mut untouched);
        fill(&mut moved);
        let boxed = moved.take_chunk(CHUNK_A, 102);
        moved.insert_chunk(boxed, 102);
        assert_eq!(observed(&mut untouched, 0), observed(&mut moved, 0));
        assert_eq!(observed(&mut untouched, 101), observed(&mut moved, 101));
        assert_eq!(observed(&mut untouched, 103), observed(&mut moved, 103));
    }

    #[test]
    fn a_box_moves_into_a_new_table_with_a_different_clock() {
        let mut source = RedstoneMemory::default();
        fill(&mut source);
        let boxed = source.take_chunk(CHUNK_A, 102);
        // 火把记录存的是相对年龄：100 -> 2，102 -> 0。
        assert_eq!(boxed.torch_toggles, [(A1, 2), (A1, 0)]);

        // 目标表的时钟晚 900 刻，换算回来的时刻是 995 / 997（加上原有的一条）。
        let mut target = RedstoneMemory::default();
        target.set_comparator_output(A2, 5);
        target.record_torch_toggle(A2, 996);
        target.insert_chunk(boxed, 997);
        assert_eq!(target.comparator_output(A1), 7);
        assert_eq!(target.comparator_output(A2), 5);
        assert_eq!(target.torch_toggle_count(A1), 2);
        assert_eq!(target.torch_toggle_count(A2), 1);
        // `forget_torch_toggles_before` 仍按时刻工作：995 被丢掉，996/997 留着。
        target.forget_torch_toggles_before(996);
        assert_eq!(target.torch_toggle_count(A1), 1);
        assert_eq!(target.torch_toggle_count(A2), 1);
        target.forget_torch_toggles_before(998);
        assert_eq!(target.torch_toggle_count(A1), 0);
        assert_eq!(target.torch_toggle_count(A2), 0);
    }

    #[test]
    fn torch_toggles_move_even_without_any_position_memory() {
        let mut source = RedstoneMemory::default();
        source.record_torch_toggle(A1, 100);
        source.record_torch_toggle(B1, 101);
        let boxed = source.take_chunk(CHUNK_A, 102);
        assert_eq!(boxed.torch_toggles, [(A1, 2)]);
        assert_eq!(source.torch_toggle_count(B1), 1);

        let mut target = RedstoneMemory::default();
        target.insert_chunk(boxed, 200);
        assert_eq!(target.torch_toggle_count(A1), 1);
        // 相对年龄 2 换算到目标时刻就是 198：更早的门限留着它。
        target.forget_torch_toggles_before(198);
        assert_eq!(target.torch_toggle_count(A1), 1);
        target.forget_torch_toggles_before(199);
        assert_eq!(target.torch_toggle_count(A1), 0);
    }

    #[test]
    fn taking_again_finds_nothing_and_index_stays_clean() {
        let mut memory = RedstoneMemory::default();
        fill(&mut memory);
        memory.forget_block_entity(A1);
        assert_eq!(memory.comparator_output(A1), 0);
        assert!(memory.by_chunk.get(&CHUNK_A).is_none_or(HashSet::is_empty));
        let boxed = memory.take_chunk(CHUNK_A, 102);
        assert!(boxed.comparator_outputs.is_empty());
        assert!(
            memory
                .take_chunk(CHUNK_A, 102)
                .comparator_outputs
                .is_empty()
        );
    }
}
