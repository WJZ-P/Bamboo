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
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    block_entity::{BlockEntities, BlockEntity, BlockEvent, BlockEvents},
    container::Container,
    entity::{Entity, EntityPublic, EntityStore, EntityUid, mob::DamageSource},
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
    /// `AgeableMob.isBaby()`：快乐恶魂幼年不能穿 BODY 槽的挽具。
    ///
    /// `AgeableMob.isBaby()`: a baby happy ghast cannot wear a BODY harness.
    pub baby: bool,
    pub equipment: [Option<ItemStack>; 8],
}

/// 试炼刷怪笼一次玩家探测看到的一名玩家：UUID、眼睛位置和身上的不祥效果。
///
/// 效果信息在游戏层（`PlayerView` 没有它），所以 [`Level::trial_spawner_players`]
/// 负责距离筛选并给出眼睛位置；视线遮挡由方块层用 [`World::clip_visual`] 再滤一遍。
///
/// One player seen by the trial spawner's detector: its UUID, eye position and
/// ominous effects. The effect facts live in the game layer, so
/// [`Level::trial_spawner_players`] filters by distance and reports the eye
/// position; the block layer then applies the line-of-sight clip.
#[derive(Clone, Debug, Copy, PartialEq)]
pub struct TrialSpawnerPlayer {
    pub uuid: [u8; 16],
    /// 身上有 `TRIAL_OMEN`。
    pub trial_omen: bool,
    /// 身上的 `BAD_OMEN` 等级；没有时是 `None`。
    pub bad_omen_amplifier: Option<i32>,
    /// 眼睛位置，`levelEvent(3020, ...)` 用 `BlockPos::containing`。
    pub eye_position: mc_bamboo_core::Vec3,
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
    /// 一次触发通过 `Level` 读或写了这一格。多区域 tick 第 2 步的越界统计
    /// 只用它判断「碰到的区块在不在自己单元范围内」；默认实现什么都不做。
    ChunkAccess {
        pos: BlockPos,
        write: bool,
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
    fn set_crafter_crafting(&mut self, pos: BlockPos, remaining: i32);
    /// 全部还在倒数的合成器倒计时，按位置排序。
    fn crafter_crafting(&self) -> Vec<(BlockPos, i32)>;
    /// 这一格合成器还剩几刻，没有记录是 0。
    fn crafter_crafting_at(&self, pos: BlockPos) -> i32;
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
    /// 这一岛的游戏事件监听者登记表。没有登记表的世界（世界生成、压测）返回 `None`，
    /// 游戏事件在那里不投递。
    fn game_event_listeners(&self) -> Option<&crate::game_event::registry::GameEventListeners> {
        None
    }
    /// 见 [`LevelSideState::game_event_listeners`]。
    fn game_event_listeners_mut(
        &mut self,
    ) -> Option<&mut crate::game_event::registry::GameEventListeners> {
        None
    }

    /// 这一格的方块或容器内容变了：叫醒关注它的睡眠条目，见 [`BlockEntities::notify_cell`]。
    /// 默认什么都不做（没有休眠的世界）。
    fn notify_block_entity_cell(&mut self, _pos: BlockPos) {}
    /// 世界里有没有任何睡着的条目登记了关注格子。没有时写入口直接跳过通知。
    fn has_any_block_entity_watchers(&self) -> bool {
        false
    }
    /// 这一格有没有别的睡眠条目关注它。方块实体自己 tick 改了物品时要查。
    fn has_block_entity_watchers(&self, _pos: BlockPos) -> bool {
        false
    }
    /// 有实体在这个盒子里出现、移动或换了物品，见 [`BlockEntities::notify_box`]。
    fn notify_block_entity_box(&mut self, _area: Aabb) {}
    /// 睡眠条目关注的 16³ 段变过时交出现在的集合，见 [`BlockEntities::take_watched_sections`]。
    fn take_block_entity_watched_sections(
        &mut self,
    ) -> Option<std::collections::HashSet<(i32, i32, i32)>> {
        None
    }
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
    /// 见 [`LevelSideState::game_event_listeners`]。
    fn game_event_listeners(&self) -> Option<&crate::game_event::registry::GameEventListeners> {
        None
    }
    /// 见 [`LevelSideState::game_event_listeners`]。
    fn game_event_listeners_mut(
        &mut self,
    ) -> Option<&mut crate::game_event::registry::GameEventListeners> {
        None
    }
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

    fn set_crafter_crafting(&mut self, pos: BlockPos, remaining: i32) {
        self.mark_chunk_dirty(pos.chunk());
        self.redstone_table_mut()
            .set_crafter_crafting(pos, remaining);
    }

    fn crafter_crafting(&self) -> Vec<(BlockPos, i32)> {
        self.redstone_table().crafter_crafting()
    }

    fn crafter_crafting_at(&self, pos: BlockPos) -> i32 {
        self.redstone_table().crafter_crafting_at(pos)
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

    fn notify_block_entity_cell(&mut self, pos: BlockPos) {
        self.block_entities_table_mut().notify_cell(pos);
    }

    fn has_block_entity_watchers(&self, pos: BlockPos) -> bool {
        self.block_entities_table().has_cell_watchers(pos)
    }

    fn has_any_block_entity_watchers(&self) -> bool {
        self.block_entities_table().has_any_watchers()
    }

    fn notify_block_entity_box(&mut self, area: Aabb) {
        self.block_entities_table_mut().notify_box(area);
    }

    fn take_block_entity_watched_sections(
        &mut self,
    ) -> Option<std::collections::HashSet<(i32, i32, i32)>> {
        self.block_entities_table_mut().take_watched_sections()
    }

    fn game_event_listeners(&self) -> Option<&crate::game_event::registry::GameEventListeners> {
        LevelTables::game_event_listeners(self)
    }

    fn game_event_listeners_mut(
        &mut self,
    ) -> Option<&mut crate::game_event::registry::GameEventListeners> {
        LevelTables::game_event_listeners_mut(self)
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
    /// 借用式读取公开数据，找到时返回 `true`，省掉整份克隆。默认回落到
    /// [`Self::entity_public`] 的克隆实现；有整张实体表的实现覆盖成直接借用那一行。
    ///
    /// Borrows the public data and returns `true` when found, avoiding a full clone.
    /// The default falls back to the cloning [`Self::entity_public`]; implementations
    /// with a whole entity table override it to borrow the row directly.
    fn with_entity_public(&self, uid: EntityUid, visit: &mut dyn FnMut(&EntityPublic)) -> bool {
        match self.entity_public(uid) {
            Some(public) => {
                visit(&public);
                true
            }
            None => false,
        }
    }
    /// 空间索引范围查询，按 uid 升序访问。
    fn for_each_entity_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic));
    /// 同 [`Self::for_each_entity_in`]，但只访问 `types` 里那几种注册表实体类型。
    /// 没有按类型建索引的实现退回全量扫描加过滤，顺序和结果都不变。
    ///
    /// Like [`Self::for_each_entity_in`] but only the given registry entity types.
    /// Implementations without a type index fall back to a filtered full scan;
    /// the order and the result are unchanged.
    fn for_each_entity_of_types_in(
        &self,
        area: Aabb,
        types: &[i32],
        visit: &mut dyn FnMut(&EntityPublic),
    ) {
        self.for_each_entity_in(area, &mut |public| {
            if types.contains(&public.kind.entity_type()) {
                visit(public);
            }
        });
    }
    /// `getEntitiesOfClass` 的官方分区顺序（`SectionPos.asLong` 升序，段内加入顺序）。
    ///
    /// Vanilla `EntitySectionStorage` order for `getEntitiesOfClass`.
    fn for_each_entity_in_section_order(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic));
    /// 同 [`Self::for_each_entity_in`]，但最多访问 `limit` 个。
    fn for_each_pushable_in(
        &self,
        area: Aabb,
        limit: usize,
        visit: &mut dyn FnMut(&EntityPublic) -> bool,
    ) {
        let mut seen = 0;
        self.for_each_entity_in(area, &mut |public| {
            if seen < limit && visit(public) {
                seen += 1;
            }
        });
    }
    /// `ComparatorBlock.getItemFrame`：在 `pos` 这一格内查找面向 `facing` 的唯一展示框，
    /// 恰好找到 1 个时返回其 `getAnalogOutput()`。
    ///
    /// `ComparatorBlock.getItemFrame`: return `getAnalogOutput()` when exactly one
    /// item frame facing `facing` intersects cell `pos`.
    fn comparator_item_frame_signal(
        &self,
        facing: mc_bamboo_core::Direction,
        pos: BlockPos,
    ) -> Option<i32> {
        use mc_bamboo_data::registries::entity_type;
        let cell = Aabb::new(
            f64::from(pos.x),
            f64::from(pos.y),
            f64::from(pos.z),
            f64::from(pos.x + 1),
            f64::from(pos.y + 1),
            f64::from(pos.z + 1),
        );
        let mut count = 0usize;
        let mut signal = 0i32;
        self.for_each_entity_of_types_in(
            cell,
            &[entity_type::ITEM_FRAME, entity_type::GLOW_ITEM_FRAME],
            &mut |public| {
                if !public.removed
                    && let crate::entity::KindPublic::ItemFrame(frame) = &public.kind
                    && frame.direction == facing
                {
                    count += 1;
                    signal = frame.analog_output();
                }
            },
        );
        (count == 1).then_some(signal)
    }
    /// `addFreshEntity`：tick 期间生成的实体下一 tick 才动。
    fn spawn_entity(&mut self, entity: Entity) -> EntityUid;
    /// 构造实体：分配 ID、取种子和构造时的随机数，还不放进世界，见 [`EntityStore::prepare`]。
    fn prepare_entity(&mut self, entity: Entity) -> Entity;
    /// 放进一个 [`Self::prepare_entity`] 过的实体。
    fn add_prepared_entity(&mut self, entity: Entity) -> EntityUid;
    /// 世界里有没有这个 UUID 的生物。
    fn has_mob_uuid(&self, uuid: &[u8; 16]) -> bool;
    /// 刚生成、还没并进 tick 列表的实体也找得到。
    fn with_just_spawned(&mut self, uid: EntityUid, f: &mut dyn FnMut(&mut Entity)) -> bool;
    /// 实体这一拍被丢弃时，把它身上攒下的生物事件留给同步层。
    /// 默认什么都不做，按实体表建模的世界覆盖。
    ///
    /// Keep a discarded entity's pending mob events for the sync layer.
    fn keep_departed_mob_events(&mut self, _entity: &crate::entity::Entity) {}
    /// 只打上 `removed` 标记，实体还留在这一拍的名单里。
    fn discard_entity(&mut self, uid: EntityUid) -> bool;
    /// 从 tick 列表里立刻拿掉。
    fn remove_entity(&mut self, uid: EntityUid);
    fn begin_entity_tick(&mut self);
    fn end_entity_tick(&mut self);
    /// 独立寻路阶段：把上一刻的结果挂到各生物的导航上。缺省什么都不做。
    fn begin_path_tick(&mut self) {}
    /// 收走这一刻挂起的寻路请求。缺省没有。
    fn take_path_requests(&mut self, _now: i64) -> Vec<crate::entity::mob::pathfinding::Pending> {
        Vec::new()
    }
    /// 取出寻路阶段状态（精确驱动在算搜索时临时搬出去，避免和只读方块视图同时借）。
    fn take_pathfinding(&mut self) -> crate::entity::mob::pathfinding::PhaseState {
        Default::default()
    }
    fn put_pathfinding(&mut self, _state: crate::entity::mob::pathfinding::PhaseState) {}
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
    /// 取走实体表攒下的位置事件，见 [`EntityStore::take_wake_events`]。
    fn take_entity_wake_events(&mut self) -> Vec<Aabb> {
        Vec::new()
    }
    /// 把关注段集合交给实体表，见 [`EntityStore::set_watched_sections`]。
    fn set_entity_watched_sections(
        &mut self,
        _sections: std::collections::HashSet<(i32, i32, i32)>,
    ) {
    }
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

    fn with_entity_public(&self, uid: EntityUid, visit: &mut dyn FnMut(&EntityPublic)) -> bool {
        match self.entities_table().public_at(uid) {
            Some(public) => {
                visit(public);
                true
            }
            None => false,
        }
    }

    fn for_each_entity_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic)) {
        self.entities_table().for_each_in(area, visit);
    }

    fn for_each_entity_of_types_in(
        &self,
        area: Aabb,
        types: &[i32],
        visit: &mut dyn FnMut(&EntityPublic),
    ) {
        self.entities_table()
            .for_each_of_types_in(area, types, visit);
    }

    fn for_each_entity_in_section_order(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic)) {
        self.entities_table().for_each_in_section_order(area, visit);
    }

    fn for_each_pushable_in(
        &self,
        area: Aabb,
        limit: usize,
        visit: &mut dyn FnMut(&EntityPublic) -> bool,
    ) {
        self.entities_table()
            .for_each_in_limited(area, limit, visit);
    }

    fn spawn_entity(&mut self, entity: Entity) -> EntityUid {
        self.mark_chunk_dirty(entity.block_position().chunk());
        self.entities_table_mut().spawn(entity)
    }

    fn prepare_entity(&mut self, entity: Entity) -> Entity {
        self.entities_table_mut().prepare(entity)
    }

    fn add_prepared_entity(&mut self, entity: Entity) -> EntityUid {
        self.mark_chunk_dirty(entity.block_position().chunk());
        self.entities_table_mut().add(entity)
    }

    fn has_mob_uuid(&self, uuid: &[u8; 16]) -> bool {
        self.entities_table().has_mob_uuid(uuid)
    }

    fn with_just_spawned(&mut self, uid: EntityUid, f: &mut dyn FnMut(&mut Entity)) -> bool {
        self.entities_table_mut()
            .with_mut_including_pending(uid, f)
            .is_some()
    }

    fn keep_departed_mob_events(&mut self, entity: &crate::entity::Entity) {
        self.entities_table_mut().keep_departed_events(entity);
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

    fn begin_path_tick(&mut self) {
        self.entities_table_mut().begin_path_tick();
    }

    fn take_path_requests(&mut self, now: i64) -> Vec<crate::entity::mob::pathfinding::Pending> {
        self.entities_table_mut().take_path_requests(now)
    }

    fn take_pathfinding(&mut self) -> crate::entity::mob::pathfinding::PhaseState {
        std::mem::take(self.entities_table_mut().pathfinding_mut())
    }

    fn put_pathfinding(&mut self, state: crate::entity::mob::pathfinding::PhaseState) {
        *self.entities_table_mut().pathfinding_mut() = state;
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

    fn take_entity_wake_events(&mut self) -> Vec<Aabb> {
        self.entities_table_mut().take_wake_events()
    }

    fn set_entity_watched_sections(
        &mut self,
        sections: std::collections::HashSet<(i32, i32, i32)>,
    ) {
        self.entities_table_mut().set_watched_sections(sections);
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

    /// `Level.getSeaLevel()`：海平面。噪声主世界是 63，超平坦是 -63。
    ///
    /// `Level.getSeaLevel()`: the sea level. 63 for the noise overworld and -63 for
    /// superflat.
    fn sea_level(&self) -> i32 {
        63
    }

    /// `Level.random` 和 `randValue`，方块逻辑的随机数都从这里取。
    fn random(&self) -> &LevelRandom;

    /// 通过游戏层的附魔表执行生成装备附魔。
    ///
    /// Apply the mob-spawn equipment enchantment provider from the game registry.
    fn enchant_mob_spawn_equipment(&self, _stack: &mut ItemStack, _special: f32) {}

    /// 同一套附魔提供器，但用调用方给的随机源（陷阱骑士用骷髅自己的
    /// `getRandom()`，不是 `level.random`）。默认退回世界随机源。
    ///
    /// Same provider with an explicit random source (trap riders use the
    /// skeleton's own random, not the level random).
    fn enchant_mob_spawn_equipment_from(
        &self,
        stack: &mut ItemStack,
        special: f32,
        _random: &dyn crate::random::RandomSource,
    ) {
        self.enchant_mob_spawn_equipment(stack, special);
    }

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

    /// 官方 `PoiManager.getInRange(#bee_home, center, radius, ANY)`：`beehive` /
    /// `bee_nest` 两类 POI 的全部位置，保持官方遍历序（欧氏半径过滤）。
    /// 有 POI 表的实现（精确 / 并行驱动、游戏层只读视图）覆盖它查询 `PoiStore`；
    /// 没有 POI 数据的假世界默认返回空。
    ///
    /// Vanilla `PoiManager.getInRange(#bee_home, center, radius, ANY)`: all
    /// `beehive` / `bee_nest` POIs in official traversal order. Levels with a
    /// POI store override it; fake levels without POI data return empty.
    fn beehives_near(&self, _center: BlockPos, _radius: i32) -> Vec<BlockPos> {
        Vec::new()
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

    /// `CopperGolemStatueBlockEntity.components().get(CUSTOM_NAME)` 对应的文本组件标签。
    /// 官方把它存进 `components` 组件表；没建模的世界返回 `None`。
    ///
    /// The text-component tag behind a statue's `CUSTOM_NAME`. Vanilla stores it in the
    /// `components` map; unmodelled worlds return `None`.
    fn statue_custom_name(&self, _pos: BlockPos) -> Option<mc_bamboo_nbt::Tag> {
        None
    }

    /// 把铜傀儡的自定义名写进雕像方块实体的组件表。没建模的世界什么都不做。
    ///
    /// Write a copper golem's custom name into the statue block entity's components.
    /// Unmodelled worlds do nothing.
    fn set_statue_custom_name(&mut self, _pos: BlockPos, _name: Option<mc_bamboo_nbt::Tag>) {}

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

    /// 爆炸对拍的改坏确认钩子：正式世界恒为 0。位 0 让 `explode_tnt` 跳过
    /// `hurtEntities`，位 1 让它丢掉爆炸中心的 `getY(0.0625)` 抬升。
    ///
    /// Oracle break-test hook for explosions; real levels always return 0. Bit 0 makes
    /// `explode_tnt` skip `hurtEntities`; bit 1 drops the `getY(0.0625)` centre lift.
    #[cfg(feature = "oracle-samples")]
    fn explosion_break(&self) -> u8 {
        0
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
    /// `Level.isRainingAt(pos)`：精确世界按天气、天空可见性、高度图和降水类型算；
    /// 夹具可以直接覆盖，让注入的天气状态与官方假世界一致。
    ///
    /// `Level.isRainingAt(pos)`: the precise world derives it from weather, sky
    /// visibility, the heightmap and precipitation; oracles may override it.
    fn is_raining_at(&self, pos: BlockPos) -> bool {
        if !self.block_weather().is_raining() || !self.can_see_sky(pos) {
            return false;
        }
        if self.heightmap_y(pos) > pos.y {
            return false;
        }
        self.precipitation_facts(pos).precipitation == crate::weather::Precipitation::Rain
    }

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

    /// `getHeight(WORLD_SURFACE, pos)`：最高的非空气方块上面一格，整列是空气时是世界底部。
    /// 自然刷怪取随机起点用得到（`NaturalSpawner.getRandomPosWithin`）。有真实高度图的
    /// 实现应该覆盖它。
    ///
    /// `getHeight(WORLD_SURFACE, pos)`: one above the highest non-air block, or the world
    /// bottom for an all-air column. Used by `NaturalSpawner.getRandomPosWithin`; levels
    /// with a real heightmap should override it.
    fn world_surface_y(&self, pos: BlockPos) -> i32 {
        for y in (self.min_y()..=self.max_y()).rev() {
            if !self.block_state(BlockPos::new(pos.x, y, pos.z)).is_air() {
                return y + 1;
            }
        }
        self.min_y()
    }

    /// 这一格所在群系的降水条件，`Biome.getPrecipitationAt(pos, seaLevel)` 加上    /// `Biome.warmEnoughToRain(pos, seaLevel)`。默认平原（测试用），真实实现读区块的
    /// 群系调色板再按 `mc-bamboo-worldgen` 算温度。
    fn precipitation_facts(&self, _pos: BlockPos) -> crate::weather::PrecipitationFacts {
        crate::weather::PrecipitationFacts::PLAINS
    }

    /// 这一格的群系注册表 ID（`level.getBiome(pos)`），生物按群系挑花色用。默认平原（测试用）。
    ///
    /// The biome registry id at this cell, for mob variants. Plains by default.
    fn biome_id(&self, _pos: BlockPos) -> i32 {
        mc_bamboo_data::registries::biome::PLAINS
    }

    /// 官方 `ServerLevel.findLightningRod`：在 POI 里按 `LIGHTNING_ROD` 类型、
    /// `ANY` 占用口径、半径 `radius` 取最近的一根，再用 `WORLD_SURFACE - 1`
    /// 的高度过滤（只认露天的那一格），返回它上方一格。并列距离时 POI 遍历序
    /// 里的第一根胜出；没有 POI 数据的视图返回 `None`。
    ///
    /// Vanilla `ServerLevel.findLightningRod`: the closest `LIGHTNING_ROD` POI
    /// (`ANY`, within `radius`) that sits on `WORLD_SURFACE - 1`, mapped to the
    /// cell above it. Ties go to the first in POI traversal order; views without
    /// POI data return `None`.
    fn lightning_rod_near(&self, center: BlockPos, radius: i32) -> Option<BlockPos> {
        let kind = mc_bamboo_data::poi::poi_type_id("lightning_rod")?;
        self.poi_find_closest(center, radius, kind, 2, &|pos| {
            pos.y == self.world_surface_y(pos) - 1
        })
        .map(|pos| pos.above())
    }

    /// 这个区块现在有没有加载。默认都加载；已加载模式的世界覆盖它。
    ///
    /// Whether this chunk is currently loaded. All chunks are loaded by default;
    /// loaded-mode worlds override it.
    fn chunk_is_loaded(&self, _chunk: ChunkPos) -> bool {
        true
    }

    /// `ChunkGenerator.getMobsAt` 的加权表。默认取群系表；对拍场景可以固定候选表。
    ///
    /// The weighted table from `ChunkGenerator.getMobsAt`. Defaults to the biome table;
    /// oracle scenes can supply a fixed candidate table.
    fn natural_spawn_table(
        &self,
        pos: BlockPos,
        category: mc_bamboo_data::spawning::MobCategory,
    ) -> &'static [mc_bamboo_data::spawning::MobSpawnerDef] {
        mc_bamboo_data::spawning::for_biome_id(self.biome_id(pos)).mobs_to_spawn(category)
    }

    /// `ChunkGenerator.getMobsAt` 的结构刷怪覆盖：位置落在某结构的覆盖盒里时，用
    /// 该结构的怪物表替换群系表（掠夺者前哨站、女巫小屋等）。没有运行时结构数据的
    /// 实现（测试用的假世界、无结构维度）默认返回 `None`，走群系表。
    ///
    /// The structure spawn override of `ChunkGenerator.getMobsAt`: when a position
    /// lies in a structure's override box, replace the biome table with the
    /// structure's monster list (pillager outposts, swamp huts, ...). Levels with
    /// no runtime structure data default to `None` and fall back to the biome table.
    fn structure_spawn_override(
        &self,
        _pos: BlockPos,
        _category: mc_bamboo_data::spawning::MobCategory,
    ) -> Option<&'static [mc_bamboo_data::spawning::MobSpawnerDef]> {
        None
    }

    /// `EnvironmentAttributes.CAN_PILLAGER_PATROL_SPAWN`：群系属性（只有蘑菇岛关掉）
    /// 叠加「早期世界」时间线（主世界时钟 120000 刻之前不刷巡逻队）的结果。
    /// 默认真，供没有时间线的测试世界使用。
    ///
    /// `EnvironmentAttributes.CAN_PILLAGER_PATROL_SPAWN`: the biome attribute (only
    /// mushroom fields disables it) combined with the early-game timeline (no patrols
    /// before overworld-clock tick 120000). Defaults to true for timeless test levels.
    fn can_pillager_patrol_spawn(&self, _pos: BlockPos) -> bool {
        true
    }

    /// `ServerLevel.hasChunksAt(...)`：巡逻队落点周围 10 格的方块是否都已加载。
    /// 默认真，供整块都在的测试世界使用。
    ///
    /// `ServerLevel.hasChunksAt(...)`: whether the 10-block margin around a patrol
    /// landing spot is loaded. Defaults to true for fully-present test levels.
    fn patrol_chunks_loaded(&self, _center: BlockPos, _margin: i32) -> bool {
        true
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

    /// `ChestBlockEntity.openersCounter.openCount`：这一刻有几个 `ContainerUser`
    /// 开着这个箱子。铜傀儡排队判定读它；没有容器表时返回 0。
    ///
    /// The chest's `ContainerOpenersCounter.openCount`; read by the copper golem's
    /// queue check. Worlds without a container table report zero.
    fn container_open_count(&self, _pos: BlockPos) -> i32 {
        0
    }

    /// `Container.startOpen/stopOpen`：开箱者人数增减 `delta`。
    fn change_container_open_count(&mut self, _pos: BlockPos, _delta: i32) {}

    /// 这一格容器背后的方块位置：单箱是这一格，双箱是两半，见 `CompoundContainer.setChanged`。
    ///
    /// 和 [`Level::with_container`] 一样按能取到的那条规则算，被压住的箱子也算能取到。
    fn container_positions(&self, pos: BlockPos) -> Vec<BlockPos> {
        vec![pos]
    }

    /// `TransportItemsBetweenContainers.getTransportTarget` 的只读查询：以 `center`
    /// 的方块为中心、`horizontal`/`vertical` 为半径的盒子里所有容器方块位置。
    /// 没有容器表的世界返回空表。
    ///
    /// Read-only query for `getTransportTarget`: container positions inside the search
    /// box. Worlds without a container table return an empty list.
    fn container_positions_in(
        &self,
        _center: BlockPos,
        _horizontal: i32,
        _vertical: i32,
    ) -> Vec<BlockPos> {
        Vec::new()
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

    /// `DESTROY_WITH_DECAY` 爆炸里的 `getDrops`：多一个 `EXPLOSION_RADIUS` 参数，
    /// 战利品表的 `survives_explosion` / `explosion_decay` 按它取随机数。缺省不带半径。
    ///
    /// `getDrops` in a `DESTROY_WITH_DECAY` explosion: the extra `EXPLOSION_RADIUS`
    /// parameter drives `survives_explosion` / `explosion_decay`. Default: no radius.
    fn explosion_block_drops_decayed(
        &mut self,
        state: BlockState,
        pos: BlockPos,
        radius: f32,
    ) -> Vec<ItemStack> {
        let _ = radius;
        self.explosion_block_drops(state, pos)
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

    /// 生物死时的战利品，取战利品表自己的随机序列。默认不掉落。
    ///
    /// Mob death loot. The draw uses the table's random sequence. Default: nothing.
    fn mob_loot_drops(
        &mut self,
        loot: crate::entity::mob::MobDeathLoot,
        origin: mc_bamboo_core::Vec3,
    ) -> Vec<ItemStack> {
        let _ = (loot, origin);
        Vec::new()
    }

    /// `dropFromShearingLootTable` / `dropFromGiftLootTable`：原点和实体本身（剪毛时还有工具）。
    /// 缺省没有掉落。
    ///
    /// `dropFromShearingLootTable` / `dropFromGiftLootTable`: origin and this entity, plus the
    /// tool when shearing. No drops by default.
    fn entity_loot_drops(
        &mut self,
        request: crate::entity::mob::EntityLootRequest,
    ) -> Vec<ItemStack> {
        let _ = request;
        Vec::new()
    }

    /// 世界里的玩家，按加入顺序。生物的 AI 读它。
    fn for_each_player(&self, _visit: &mut dyn FnMut(&crate::entity::ctx::PlayerView)) {}

    /// 和 [`Self::for_each_player`] 同一份名单，按值返回；给降频算档用。
    /// 默认实现仍然走一遍 `for_each_player`，两种驱动看到同一份玩家。
    fn player_views(&self) -> Vec<crate::entity::ctx::PlayerView> {
        let mut out = Vec::new();
        self.for_each_player(&mut |player| out.push(*player));
        out
    }

    /// 这名玩家在不在床上（`LivingEntity.isSleeping()`）。末影珍珠决定要不要传送主人时读它。
    ///
    /// Whether this player is sleeping (`LivingEntity.isSleeping()`), read when an
    /// ender pearl decides whether it may teleport its owner.
    fn player_sleeping(&self, _uid: EntityUid) -> bool {
        false
    }

    // 试炼刷怪笼的世界副作用。方块层只跑状态机，这些副作用里需要游戏层数据
    // （玩家的不祥效果、装备表、战利品表、实体位置）的都由游戏层实现；缺省是
    // 「没有玩家 / 不生成 / 不掉落」，只读测试世界和方块 oracle 走这一条。
    // The trial spawner's world side effects. The block layer only runs the state
    // machine; hooks that need game-layer data default to no players / no spawn /
    // no drops, which the block oracle and read-only test worlds use.

    /// `PlayerDetector.detect` 的候选玩家：`pos` 附近 `range` 格内、带不祥效果和眼睛
    /// 位置的玩家。视线遮挡由方块层用 [`World::clip_visual`] 再滤。默认没有玩家。
    fn trial_spawner_players(&self, _pos: BlockPos, _range: f64) -> Vec<TrialSpawnerPlayer> {
        Vec::new()
    }

    /// `EquipmentTable` 套到一只还没放进世界的生物上：按战利品表给装备槽，写
    /// 掉率。返回被替换掉、要按掉落几率掉出来的旧装备（调用方 `spawnAtLocation`）。
    /// 默认不做事、不掉落。
    fn equip_mob(
        &mut self,
        _entity: &mut crate::entity::Entity,
        _equipment: &Tag,
    ) -> Vec<mc_bamboo_protocol::item::ItemStack> {
        Vec::new()
    }

    /// `TrialSpawner.ejectReward`：按战利品表在 `pos` 弹出一组奖励，返回有没有弹过。
    /// 默认不弹。
    fn trial_spawner_eject_reward(&mut self, _pos: BlockPos, _loot_table: &str) -> bool {
        false
    }

    /// `TrialSpawnerStateData.getDispensingItems`：按战利品表和 lowResolutionPosition
    /// 求出一组（数量为 1 的物品堆，权重 = 原数量）。默认空。
    fn trial_spawner_dispensing(
        &mut self,
        _pos: BlockPos,
        _loot_table: &str,
    ) -> Vec<(ItemStack, i32)> {
        Vec::new()
    }

    /// `resetAfterBecomingOminous`：变成不祥试炼时让已记录的生物消失。默认不做事。
    fn trial_spawner_discard_mobs(&mut self, _mobs: &[[u8; 16]]) {}

    /// `shouldMobBeUntracked`：这只已记录的生物是否已经不活 / 太远 / 换维度。
    /// 默认只在实体不存在时算「已不活」。
    fn trial_spawner_mob_untracked(&self, uuid: [u8; 16], _pos: BlockPos) -> bool {
        !self.has_mob_uuid(&uuid)
    }

    /// `getOrCreateDisplayEntity` 能不能造出显示实体。默认可以，让刷怪笼从
    /// `inactive` 推进到 `waiting_for_players`。
    fn trial_spawner_has_display_entity(&self) -> bool {
        true
    }

    // 宝库的世界副作用。方块层只跑状态机；需要游戏层数据的（玩家探测、VAULT
    // 战利品表、世界随机源、玩家物品栏）由游戏层实现，缺省是「没有玩家 / 空奖励 /
    // 不消耗」，只读测试世界和方块 oracle 走这一条。

    /// `PlayerDetector.detect` 的候选玩家：`pos` 附近 `range` 格内的玩家 UUID
    /// （`INCLUDING_CREATIVE_PLAYERS`、`lineOfSight=false`）。默认没有玩家。
    fn vault_players(&self, _pos: BlockPos, _range: f64) -> Vec<[u8; 16]> {
        Vec::new()
    }

    /// `resolveItemsToEject`：按 VAULT 战利品表（玩家 luck / this_entity / 钥匙 tool）
    /// 弹出一组物品。默认空。
    fn vault_resolve_reward(
        &mut self,
        _pos: BlockPos,
        _player: [u8; 16],
        _key: &ItemStack,
        _loot_table: &str,
    ) -> Vec<ItemStack> {
        Vec::new()
    }

    /// `getRandomDisplayItemFromLootTable`：用世界随机源从表里取一件展示物品。
    /// 默认空。
    fn vault_random_display_item(&mut self, _loot_table: &str, _pos: BlockPos) -> ItemStack {
        ItemStack::empty()
    }

    /// `ItemStack.consume`：消耗玩家手里的 `count` 把钥匙。默认不消耗。
    fn vault_consume_key(&mut self, _player: [u8; 16], _count: i32) {}

    /// 现实中的本地日期，不是世界时间。
    ///
    /// The local calendar date, independent of the world clock.
    fn is_halloween(&self) -> bool {
        false
    }

    fn player_mob_attack(&self, _uid: EntityUid) -> Option<crate::entity::mob::PlayerMobAttack> {
        None
    }

    /// 一个实体（按 UUID 找它的计分板名字）所在的队伍，和
    /// [`PlayerView::team`](crate::entity::ctx::PlayerView::team) 用同一套下标。默认谁都不在队伍里。
    ///
    /// The team of an entity looked up by UUID, indexed like `PlayerView::team`.
    fn entity_team(&self, _uuid: &[u8; 16]) -> Option<u32> {
        None
    }

    /// `getSkyDarken()`。天色由主控的时钟和天气发布，这一层只读快照。
    fn sky_darken(&self) -> i32 {
        0
    }

    /// 岛级的方块变更版本戳，见 [`BlockView::block_change_stamp`](crate::entity::ctx::BlockView::block_change_stamp)。
    /// 默认 0：没有版本戳的世界不复用视线结果。
    ///
    /// Island-level block-change stamp; 0 by default means no sight-result reuse.
    fn block_change_stamp(&self) -> u64 {
        0
    }

    /// `Level.isBrightOutside()`：主世界没有固定时间，`skyDarken < 4` 就是白天。
    fn is_bright_outside(&self) -> bool {
        !self.dimension_has_fixed_time() && self.sky_darken() < 4
    }

    /// `DimensionType.hasFixedTime()`。主世界为假。
    fn dimension_has_fixed_time(&self) -> bool {
        false
    }

    /// `EnvironmentAttributes.BEES_STAY_IN_HIVE`：主世界的时间线和天气层。
    /// 对拍夹具可以直接给官方那一侧算出的布尔值。
    fn bees_stay_in_hive(&self) -> bool {
        crate::sky::bees_stay_in_hive(self.clock_ticks())
            || self.block_weather().rain_level > 0.0
            || self.block_weather().thunder_level > 0.0
    }

    /// `EnvironmentAttributes.CREAKING_ACTIVE`：主世界按 `day` 时间线求值；
    /// 有固定时间的维度没有这条时间线，恒假。
    fn creaking_active(&self) -> bool {
        !self.dimension_has_fixed_time() && crate::sky::creaking_active(self.clock_ticks())
    }

    /// `EnvironmentAttributes.MONSTERS_BURN`：主世界按 `day` 时间线在
    /// [`Level::clock_ticks`] 处求值；有固定时间的维度没有这条时间线，恒假。
    fn monsters_burn(&self) -> bool {
        !self.dimension_has_fixed_time() && crate::sky::monsters_burn(self.clock_ticks())
    }

    /// `EnvironmentAttributes.VILLAGER_ACTIVITY`：主控按主世界时钟和
    /// `villager_schedule.json` 关键帧发布给各岛，和 `sky_darken` 走同一条路径。
    ///
    /// `EnvironmentAttributes.VILLAGER_ACTIVITY`: published by the master from the
    /// overworld clock and `villager_schedule.json` keyframes, on the same path as
    /// `sky_darken`.
    fn villager_activity(&self) -> crate::entity::mob::brain::Activity {
        crate::sky::villager_activity(self.clock_ticks())
    }

    /// `EnvironmentAttributes.BABY_VILLAGER_ACTIVITY`：幼年村民的日程活动。
    ///
    /// `EnvironmentAttributes.BABY_VILLAGER_ACTIVITY`: the baby villager schedule activity.
    fn baby_villager_activity(&self) -> crate::entity::mob::brain::Activity {
        crate::sky::baby_villager_activity(self.clock_ticks())
    }

    /// `EnvironmentAttributes.SNOW_GOLEM_MELTS`：按这一格的群系查生成的表。
    /// 夹具可以覆盖它，模拟热群系或下界。
    ///
    /// `EnvironmentAttributes.SNOW_GOLEM_MELTS`, looked up from the generated
    /// biome table. Oracles can override it for hot biomes or the Nether.
    fn snow_golem_melts(&self, pos: BlockPos) -> bool {
        mc_bamboo_data::spawning::SNOW_GOLEM_MELTS
            .get(usize::try_from(self.biome_id(pos)).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or(false)
    }

    /// `getOverworldClockTime()`：主世界时钟的整数刻，和算 `skyDarken` 的是同一个值。
    /// 默认 0（新世界的第一刻）。
    fn clock_ticks(&self) -> i64 {
        0
    }

    /// `WorldGenLevel.getSeed()`：世界种子，史莱姆区块读它。默认 0。
    ///
    /// `WorldGenLevel.getSeed()`: the world seed, read for slime chunks. Defaults to 0.
    fn world_seed(&self) -> i64 {
        0
    }

    /// 区块的 `InhabitedTime`：区域难度读它。默认 0。
    fn inhabited_time(&self, _chunk: ChunkPos) -> i64 {
        0
    }

    /// `PoiManager.existsAtPosition` / `getType`：这一格登记的 POI 类型 ID，
    /// `None` 表示没有。默认没有（只读测试视图）。
    fn poi_type_at(&self, _pos: BlockPos) -> Option<u8> {
        None
    }

    /// `PoiManager.getCountInRange`：以 `center` 为心、`radius` 格内
    /// 指定类型 / 占用口径的记录条数。默认 0。
    ///
    /// `occupancy`：`0 = HAS_SPACE`、`1 = IS_OCCUPIED`、`2 = ANY`。
    fn poi_count_in_range(
        &self,
        _center: BlockPos,
        _radius: i32,
        _kind: u8,
        _occupancy: u8,
    ) -> usize {
        0
    }

    /// `PoiManager.findClosest`：按官方遍历序取距离最近的一格；并列时取遍历序
    /// 第一个。默认没有。
    fn poi_find_closest(
        &self,
        _center: BlockPos,
        _radius: i32,
        _kind: u8,
        _occupancy: u8,
        _position_filter: &dyn Fn(BlockPos) -> bool,
    ) -> Option<BlockPos> {
        None
    }

    /// `PoiManager.getInRange`：按官方遍历序返回半径内匹配类型位掩码（`1 << kind`）
    /// 与占用口径的全部 `(kind, pos)`。默认为空。
    ///
    /// `PoiManager.getInRange`: all `(kind, pos)` within `radius` matching `type_mask`
    /// (`1 << kind`) and `occupancy`, in vanilla traversal order. Empty by default.
    fn poi_in_range(
        &self,
        _center: BlockPos,
        _radius: i32,
        _type_mask: u32,
        _occupancy: u8,
    ) -> Vec<(u8, BlockPos)> {
        Vec::new()
    }

    /// `PoiManager.take`：在 `pos` 处若存在匹配 `type_mask` 且 `HAS_SPACE` 的 POI 则扣一张票。
    ///
    /// `PoiManager.take`: consumes one ticket at `pos` if a matching `HAS_SPACE` POI exists.
    fn poi_take(&mut self, _pos: BlockPos, _type_mask: u32) -> bool {
        false
    }

    /// `PoiManager.release`：归还 `pos` 处 POI 的一张票。
    ///
    /// `PoiManager.release`: releases one ticket for the POI at `pos`.
    fn poi_release(&mut self, _pos: BlockPos) -> bool {
        false
    }

    /// `ServerLevel.sectionsToVillage(SectionPos.of(pos))`。默认 7（不可达）。
    fn poi_sections_to_village(&self, _pos: BlockPos) -> i32 {
        7
    }

    /// `ServerLevel.isCloseToVillage(pos, sections)`。默认假。
    fn poi_is_close_to_village(&self, _pos: BlockPos, _sections: i32) -> bool {
        false
    }

    /// `ServerLevel.getRaidAt(pos)`：`Raids.getNearbyRaid` 找到的那场袭击。
    /// 默认没有袭击（夹具 / 非主世界）。
    ///
    /// `ServerLevel.getRaidAt(pos)`: the raid found by `Raids.getNearbyRaid`;
    /// none by default (fixtures / non-overworld dimensions).
    fn raid_at(&self, _pos: BlockPos) -> Option<crate::entity::raid::RaidView> {
        None
    }

    /// `Level.isDarkOutside()`：没有固定时间的维度里 `skyDarken >= 4`。
    /// 主世界没有 `hasFixedTime`，床的 `when_dark` 规则走这个判断。
    ///
    /// `Level.isDarkOutside()`: `skyDarken >= 4` in dimensions without a
    /// fixed time. The overworld uses it for the bed's `when_dark` rule.
    fn is_dark_outside(&self) -> bool {
        self.sky_darken() >= 4
    }

    /// 精确驱动当场落地生物对玩家的一次伤害，官方 `Mob.doHurtTarget` 里的
    /// `target.hurtServer(...)`。返回真实的 `wasHurt` 和死亡掉落；`None` 表示这一层
    /// 不能当场落地，调用方改成延后的 [`Effect::HurtPlayer`]。`attacker` 是打人的生物
    /// 这一刻的公开数据：它正在 tick，不在实体表里，死亡消息要靠它拿名字。
    ///
    /// Applies a mob's hit on a player at once in the precise driver. `None` means
    /// this level cannot, and the caller falls back to a deferred effect.
    fn hurt_player_now(
        &mut self,
        _target: crate::entity::EntityUid,
        _source: crate::entity::mob::DamageSource,
        _amount: f32,
        _attacker: Option<&crate::entity::EntityPublic>,
    ) -> Option<PlayerHurtNow> {
        None
    }

    /// 精确驱动当场把玩家放到 `position`（末影珍珠的 `ServerPlayer.teleport` 落点）。
    /// `false` 表示这一层做不到，调用方改成延后的 [`Effect::EnderPearlTeleport`]。
    ///
    /// Apply an ender pearl teleport to a player at once in the precise driver.
    /// `false` means this level cannot, and the caller defers an
    /// [`Effect::EnderPearlTeleport`].
    fn teleport_player_now(
        &mut self,
        _target: crate::entity::EntityUid,
        _position: mc_bamboo_core::Vec3,
    ) -> bool {
        false
    }

    /// 精确驱动当场给玩家一次 `LivingEntity.knockback(strength, dx, dz)`（山羊冲撞）。
    /// `false` 表示这一层做不到，这次击退就没有了。
    ///
    /// Applies one `LivingEntity.knockback(strength, dx, dz)` to a player at once in the
    /// precise driver (a goat ram). `false` means this level cannot, and the push is lost.
    fn knock_back_player_now(
        &mut self,
        _target: crate::entity::EntityUid,
        _strength: f64,
        _direction: (f64, f64),
    ) -> bool {
        false
    }

    /// 精确驱动当场给玩家加一次 `Entity.push(dx, dy, dz)`（音波命中）。
    fn push_player_now(
        &mut self,
        _target: crate::entity::EntityUid,
        _push: mc_bamboo_core::Vec3,
    ) -> bool {
        false
    }

    /// 精确驱动当场把附着烟花算出的加速速度写进玩家 `getDeltaMovement()`（鞘翅滑翔）。
    /// `false` 表示这一层做不到，这次加速就丢了。默认不改，游戏层和对拍夹具各自覆盖。
    ///
    /// Lands an attached firework's boosted velocity on the player's `getDeltaMovement()`
    /// at once in the precise driver (elytra gliding). `false` means this level cannot and
    /// the boost is dropped. The default is a no-op; the game layer and the oracle override it.
    fn set_player_delta_now(
        &mut self,
        _target: crate::entity::EntityUid,
        _delta: mc_bamboo_core::Vec3,
    ) -> bool {
        false
    }

    /// 精确驱动当场给玩家点火（`Entity.igniteForSeconds`）。`false` 表示不能当场做，
    /// 调用方改成延后的 [`Effect::IgnitePlayer`]。
    fn ignite_player_now(&mut self, _target: crate::entity::EntityUid, _ticks: i32) -> bool {
        false
    }

    /// 精确驱动当场给玩家挂一个状态效果（药水箭命中）。`false` 表示不能当场做，
    /// 调用方改成延后的 [`Effect::PlayerEffect`]。
    ///
    /// Applies a potion effect to a player at once in the precise driver.
    /// `false` means this level cannot, and the caller falls back to a deferred
    /// [`Effect::PlayerEffect`].
    fn give_player_effect_now(
        &mut self,
        _target: crate::entity::EntityUid,
        _instance: &crate::entity::mob::effects::EffectInstance,
    ) -> bool {
        false
    }

    /// 精确驱动当场治疗一名玩家（喷溅治疗药水）。`false` 表示不能当场做，
    /// 调用方改成延后的 [`Effect::HealPlayer`]。
    ///
    /// Heals a player at once in the precise driver (splash healing potion).
    /// `false` means this level cannot, and the caller falls back to [`Effect::HealPlayer`].
    fn heal_player_now(&mut self, _target: crate::entity::EntityUid, _amount: f32) -> bool {
        false
    }

    /// 幽匿尖啸体 `tryToWarn`：给这名玩家加一级监守者警告，返回警告后的等级。
    /// `None` 表示没加成（冷却中，或这个世界不存玩家警告等级）。默认不做，
    /// 测试和游戏层各自覆盖。
    fn warn_warden(&mut self, _target: crate::entity::EntityUid) -> Option<i32> {
        None
    }

    /// `ServerPlayer.startSleepInBed` 的怪物查询：以床的 `Vec3.atBottomCenterOf(pos)` 为中心、
    /// 水平 ±8、竖直 ±5 的盒，`getEntitiesOfClass(Monster.class, box,
    /// m -> m.isPreventingPlayerRest(level, player))` 非空。
    ///
    /// 僵尸和尸壳都不覆盖 `isPreventingPlayerRest`（缺省恒真）。官方的实体查询只看碰撞箱和
    /// 盒相交（严格不等，只碰到表面不算），不看死活：倒地动画里的僵尸还在区块里，照样
    /// 挡着；被移除（`discard`）之后才不算。
    ///
    /// The monster query of `ServerPlayer.startSleepInBed`: any monster whose box
    /// intersects the ±8 / ±5 / ±8 box around the bed's bottom centre. Dying monsters
    /// still count, as in vanilla; discarded ones do not.
    fn monster_prevents_rest(&self, pos: BlockPos) -> bool {
        monster_prevents_rest_in(self, pos)
    }

    /// `getDifficulty().getId()`：0 和平、1 简单、2 普通、3 困难。
    /// 权威副本在 `GlobalView`，方块世界和并行只读视图都要给同一份答案。
    /// 默认是普通，和没有难度配置时的旧行为一致。
    ///
    /// `getDifficulty().getId()`: 0 peaceful, 1 easy, 2 normal, 3 hard.
    /// The authoritative value lives in `GlobalView`; both the block world and
    /// the parallel read-only view must answer alike. Normal by default.
    fn difficulty_id(&self) -> i32 {
        2
    }

    /// 官方 `RandomizableContainer.unpackLootTable` 的调用点。有战利品表的容器在
    /// 第一次被读或写之前先填好内容。默认空实现用于没有战利品表的假世界。
    ///
    /// The hook for vanilla `RandomizableContainer.unpackLootTable`. A container with a
    /// loot table is filled before its first read or write. Fake worlds leave this empty.
    fn unpack_loot_table(&mut self, _pos: BlockPos) {}

    /// 官方 `ContainerEntity.unpackChestVehicleLootTable(null)`：漏斗读实体容器
    /// （运输船）的槽位之前先填好战利品表。默认空实现用于没有战利品表的假世界。
    ///
    /// The hook for `ContainerEntity.unpackChestVehicleLootTable(null)`: a hopper
    /// fills the table before reading a container entity's slots. Fake worlds
    /// leave this empty.
    fn unpack_entity_container_loot_table(&mut self, _entity: i32) {}

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

    /// 多区域 tick 第 2 步的越界统计开着没有。`World::new` 问一次并记成布尔值，
    /// 之后每次读写只有一次布尔判断；关着时连 `observe` 的虚调用都没有。
    ///
    /// Whether step-2 out-of-unit statistics are on. `World::new` asks once and
    /// caches the answer, so a read costs only one bool test when they are off.
    fn observe_partition_enabled(&self) -> bool {
        false
    }
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
    /// `CrafterBlockEntity.craftingTicksRemaining`，只记大于 0 的。官方的 ticker 每刻减 1，
    /// 区块不能 tick 或在世界边界外时 ticker 不跑、倒计时停住，所以存剩余刻数而不是
    /// 到期时刻。条目只在亮起后的几刻里存在，按位置有序，每刻整表走一遍。
    ///
    /// `CrafterBlockEntity.craftingTicksRemaining`, positive values only. Stored as remaining
    /// ticks because the vanilla ticker pauses in frozen chunks and outside the world border.
    crafter_crafting: BTreeMap<BlockPos, i32>,
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
    /// 火把开关记录的全局入队序号。`absorb` 靠它把拆出去的那份按原来的
    /// （时刻、先后）顺序插回，和从没拆过逐条相同。
    toggle_order: Vec<u64>,
    next_toggle_order: u64,
}

/// 把拆出去的条目并回一张表。
fn absorb_positions<C, V>(into: &mut C, from: &mut C)
where
    C: PositionalMap<V>,
{
    for (pos, value) in from.drain_all() {
        into.insert_at(pos, value);
    }
}

trait PositionalMap<V> {
    fn drain_all(&mut self) -> Vec<(BlockPos, V)>;
    fn insert_at(&mut self, pos: BlockPos, value: V);
}

macro_rules! positional_map {
    ($ty:ty) => {
        impl<V> PositionalMap<V> for $ty {
            fn drain_all(&mut self) -> Vec<(BlockPos, V)> {
                std::mem::take(self).into_iter().collect()
            }

            fn insert_at(&mut self, pos: BlockPos, value: V) {
                self.insert(pos, value);
            }
        }
    };
}

positional_map!(HashMap<BlockPos, V>);
positional_map!(BTreeMap<BlockPos, V>);

impl PositionalMap<()> for HashSet<BlockPos> {
    fn drain_all(&mut self) -> Vec<(BlockPos, ())> {
        std::mem::take(self)
            .into_iter()
            .map(|pos| (pos, ()))
            .collect()
    }

    fn insert_at(&mut self, pos: BlockPos, _value: ()) {
        self.insert(pos);
    }
}

impl RedstoneMemory {
    /// 只给改坏确认用：从按区块的索引里漏掉一格。
    ///
    /// Break-it checks only: drop one cell from the per-chunk index.
    #[doc(hidden)]
    pub fn forget_chunk_index_for_test(&mut self, pos: BlockPos) {
        if let Some(positions) = self.by_chunk.get_mut(&pos.chunk()) {
            positions.remove(&pos);
        }
    }

    /// 六张按位置的表的每个键都在按区块的索引里（调试构建的断言用）。
    fn index_covers_tables(&self) -> bool {
        let covered = |pos: &BlockPos| {
            self.by_chunk
                .get(&pos.chunk())
                .is_some_and(|positions| positions.contains(pos))
        };
        self.comparator_outputs.keys().all(covered)
            && self.bookshelf_slots.keys().all(covered)
            && self.jukebox_playing.iter().all(covered)
            && self.crafter_disabled.keys().all(covered)
            && self.crafter_crafting.keys().all(covered)
            && self.lecterns.keys().all(covered)
    }

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

    /// `CrafterBlockEntity.setCraftingTicksRemaining`。0 及以下不留记录。
    ///
    /// `CrafterBlockEntity.setCraftingTicksRemaining`. Zero or less is not recorded.
    pub fn set_crafter_crafting(&mut self, pos: BlockPos, remaining: i32) {
        if remaining > 0 {
            if self.crafter_crafting.insert(pos, remaining).is_none() {
                self.note_pos(pos);
            }
        } else {
            self.crafter_crafting.remove(&pos);
        }
    }

    /// 全部还在倒数的合成器和剩余刻数，按位置排序。
    ///
    /// Every running crafter countdown with its remaining ticks, ordered by position.
    pub fn crafter_crafting(&self) -> Vec<(BlockPos, i32)> {
        self.crafter_crafting
            .iter()
            .map(|(&pos, &remaining)| (pos, remaining))
            .collect()
    }

    /// `CrafterBlockEntity.craftingTicksRemaining`，没有记录是 0。
    ///
    /// `CrafterBlockEntity.craftingTicksRemaining`; 0 when absent.
    pub fn crafter_crafting_at(&self, pos: BlockPos) -> i32 {
        self.crafter_crafting.get(&pos).copied().unwrap_or(0)
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
        self.toggle_order.clear();
    }

    /// 扔掉 `when` 之前的开关记录，官方在红石火把的计划刻开头做这件事。
    pub fn forget_torch_toggles_before(&mut self, when: i64) {
        let keep = self
            .torch_toggles
            .iter()
            .position(|&(_, toggled_at)| toggled_at >= when)
            .unwrap_or(self.torch_toggles.len());
        self.torch_toggles.drain(..keep);
        self.toggle_order.drain(..keep);
    }

    /// 岛合并前把火把开关记录的时刻平移 `delta`。只平移不重排：所有记录同加一个数，
    /// 时刻顺序和入队序号都不变。其余几张表存的不是时刻。
    ///
    /// Shift the torch-toggle instants by `delta` before an island merge. Every
    /// entry moves by the same amount, so their order and sequence numbers are
    /// unchanged. The other tables hold no instants.
    pub fn rebase(&mut self, delta: i64) {
        for (_, when) in &mut self.torch_toggles {
            *when = when.saturating_add(delta);
        }
    }

    /// 记下一次开关。
    pub fn record_torch_toggle(&mut self, pos: BlockPos, when: i64) {
        self.torch_toggles.push((pos, when));
        self.toggle_order.push(self.next_toggle_order);
        self.next_toggle_order += 1;
    }

    /// 这一格还留着几次开关记录。
    pub fn torch_toggle_count(&self, pos: BlockPos) -> usize {
        self.torch_toggles
            .iter()
            .filter(|&&(toggled, _)| toggled == pos)
            .count()
    }

    /// 拆出 `chunks` 这些区块的红石记忆，造出另一个单元的表。
    ///
    /// 六张按位置的表按 `by_chunk` 里的位置超集拆开（删掉的格子两边都查不到
    /// 值，结果与不拆一致），只碰要搬的区块的那些位置，不扫整张表；火把开关记录
    /// 按位置拆，各自仍按时刻升序（这份记录只留最近的开关，很短）。
    /// `pending_lectern` 是一次交互内的瞬时量，tick 边界上一定是 `None`。
    ///
    /// The six positional tables split by the position superset in
    /// `by_chunk`, touching only the moved chunks' positions; torch toggles
    /// (only recent ones, a short list) split by position in time order.
    pub fn split_off(&mut self, chunks: &std::collections::BTreeSet<ChunkPos>) -> Self {
        debug_assert!(self.pending_lectern.is_none());
        debug_assert!(self.index_covers_tables(), "红石记忆的按区块索引漏了位置");
        let mut out = Self::default();
        // 要搬的区块比索引里的区块少时逐个取，否则扫一遍索引；结果相同。
        // Take chunk by chunk when fewer chunks move than are indexed, else
        // scan the index once; same result.
        let moved: Vec<(ChunkPos, HashSet<BlockPos>)> = if chunks.len() < self.by_chunk.len() {
            chunks
                .iter()
                .filter_map(|chunk| Some((*chunk, self.by_chunk.remove(chunk)?)))
                .collect()
        } else {
            let (moved, kept): (HashMap<_, _>, HashMap<_, _>) = self
                .by_chunk
                .drain()
                .partition(|(chunk, _)| chunks.contains(chunk));
            self.by_chunk = kept;
            moved.into_iter().collect()
        };
        for (chunk, positions) in moved {
            for &pos in &positions {
                if let Some(signal) = self.comparator_outputs.remove(&pos) {
                    out.comparator_outputs.insert(pos, signal);
                }
                if let Some(slot) = self.bookshelf_slots.remove(&pos) {
                    out.bookshelf_slots.insert(pos, slot);
                }
                if self.jukebox_playing.remove(&pos) {
                    out.jukebox_playing.insert(pos);
                }
                if let Some(mask) = self.crafter_disabled.remove(&pos) {
                    out.crafter_disabled.insert(pos, mask);
                }
                if let Some(remaining) = self.crafter_crafting.remove(&pos) {
                    out.crafter_crafting.insert(pos, remaining);
                }
                if let Some(facts) = self.lecterns.remove(&pos) {
                    out.lecterns.insert(pos, facts);
                }
            }
            out.by_chunk.insert(chunk, positions);
        }
        let mut kept = Vec::with_capacity(self.torch_toggles.len());
        for (index, &(pos, when)) in self.torch_toggles.iter().enumerate() {
            let seq = self
                .toggle_order
                .get(index)
                .copied()
                .unwrap_or(index as u64);
            if chunks.contains(&pos.chunk()) {
                out.torch_toggles.push((pos, when));
                out.toggle_order.push(seq);
            } else {
                kept.push(((pos, when), seq));
            }
        }
        self.torch_toggles = kept.iter().map(|(entry, _)| *entry).collect();
        self.toggle_order = kept.iter().map(|(_, seq)| *seq).collect();
        out.next_toggle_order = self.next_toggle_order;
        out
    }

    /// 测试用：有记录的全部位置（各张表和火把记录），按位置排序去重。
    ///
    /// For tests: every position with a record, sorted and deduplicated.
    #[doc(hidden)]
    pub fn positions(&self) -> Vec<BlockPos> {
        let mut out: Vec<BlockPos> = self
            .comparator_outputs
            .keys()
            .chain(self.bookshelf_slots.keys())
            .chain(self.jukebox_playing.iter())
            .chain(self.crafter_disabled.keys())
            .chain(self.crafter_crafting.keys())
            .chain(self.lecterns.keys())
            .copied()
            .chain(self.torch_toggles.iter().map(|(pos, _)| *pos))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// 测试和差分用：全部记录按位置排好的文本，火把记录按原来的先后、带绝对时刻。
    ///
    /// For tests: every record sorted by position; torch toggles in their
    /// original order with absolute instants.
    #[doc(hidden)]
    pub fn debug_rows(&self) -> String {
        fn sorted<K: Ord + std::fmt::Debug, V: std::fmt::Debug>(
            rows: impl Iterator<Item = (K, V)>,
        ) -> String {
            let mut rows: Vec<(K, V)> = rows.collect();
            rows.sort_by(|a, b| a.0.cmp(&b.0));
            format!("{rows:?}")
        }
        let mut jukebox: Vec<&BlockPos> = self.jukebox_playing.iter().collect();
        jukebox.sort_unstable();
        format!(
            "comparators {} bookshelves {} jukebox {jukebox:?} crafter {} crafting {:?} lecterns {} torches {:?} pending {:?}",
            sorted(self.comparator_outputs.iter()),
            sorted(self.bookshelf_slots.iter()),
            sorted(self.crafter_disabled.iter()),
            self.crafter_crafting,
            sorted(self.lecterns.iter()),
            self.torch_toggles,
            self.pending_lectern,
        )
    }

    /// 并回一个单元的红石记忆：位置表直接合并，火把记录按全局入队序号稳定
    /// 归并（先按时刻，同一时刻按原来的先后），结果等于从没拆过。
    pub fn absorb(&mut self, mut other: Self) {
        debug_assert!(self.pending_lectern.is_none() && other.pending_lectern.is_none());
        absorb_positions(&mut self.comparator_outputs, &mut other.comparator_outputs);
        absorb_positions(&mut self.bookshelf_slots, &mut other.bookshelf_slots);
        absorb_positions(&mut self.jukebox_playing, &mut other.jukebox_playing);
        absorb_positions(&mut self.crafter_disabled, &mut other.crafter_disabled);
        absorb_positions(&mut self.crafter_crafting, &mut other.crafter_crafting);
        absorb_positions(&mut self.lecterns, &mut other.lecterns);
        for (chunk, positions) in std::mem::take(&mut other.by_chunk) {
            self.by_chunk.entry(chunk).or_default().extend(positions);
        }
        let mut toggles: Vec<(i64, u64, BlockPos)> = self
            .torch_toggles
            .drain(..)
            .zip(self.toggle_order.drain(..))
            .map(|((pos, when), seq)| (when, seq, pos))
            .collect();
        for (index, &(pos, when)) in other.torch_toggles.iter().enumerate() {
            let seq = other
                .toggle_order
                .get(index)
                .copied()
                .unwrap_or(index as u64);
            toggles.push((when, seq, pos));
        }
        toggles.sort_by_key(|&(when, seq, _)| (when, seq));
        self.torch_toggles = toggles.iter().map(|&(when, _, pos)| (pos, when)).collect();
        self.toggle_order = toggles.iter().map(|&(_, seq, _)| seq).collect();
        self.next_toggle_order = self.next_toggle_order.max(other.next_toggle_order);
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
                // 表里本来就是剩余刻数，和时钟无关，原样搬。
                if let Some(remaining) = self.crafter_crafting.remove(&pos) {
                    memory.crafter_crafting.push((pos, remaining));
                }
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
        let mut kept_toggles = Vec::with_capacity(self.torch_toggles.len());
        let mut kept_orders = Vec::with_capacity(self.toggle_order.len());
        for (index, &entry) in self.torch_toggles.iter().enumerate() {
            if entry.0.chunk() != chunk {
                kept_toggles.push(entry);
                kept_orders.push(
                    self.toggle_order
                        .get(index)
                        .copied()
                        .unwrap_or(index as u64),
                );
            }
        }
        self.torch_toggles = kept_toggles;
        self.toggle_order = kept_orders;
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
                if let Some(&remaining) = self.crafter_crafting.get(&pos) {
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
            self.set_crafter_crafting(pos, remaining);
        }
        for (pos, facts) in memory.lecterns {
            self.set_lectern(pos, facts);
        }
        for (pos, age) in memory.torch_toggles {
            self.torch_toggles.push((pos, now.saturating_sub(age)));
            self.toggle_order.push(self.next_toggle_order);
            self.next_toggle_order += 1;
        }
        // 稳定排序：原有条目在前，放入的在后，整体仍按时刻升序。
        self.torch_toggles
            .sort_by_key(|&(_, toggled_at)| toggled_at);
    }
}

/// 一个区块的全部红石记忆，按位置排序。火把开关记录存的是相对年龄。
#[derive(Debug, Clone, Default)]
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

/// `Player.BedSleepingProblem` 里床的右键会遇到的几种失败原因。
///
/// The bed failures in `Player.BedSleepingProblem` that a right click can hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BedProblem {
    /// `NOT_POSSIBLE_NOW`：`canSleep = when_dark` 且不是夜里 / 雷暴天。
    NotPossibleNow,
    /// `NOT_SAFE`：附近有会阻止休息的怪物。
    NotSafe,
    /// `TOO_FAR_AWAY`：床不在够得到的范围内。
    TooFarAway,
    /// `OBSTRUCTED`：床头或床尾上方被挡住。
    Obstructed,
    /// `OTHER_PROBLEM`：玩家已经睡着或者不在人世。
    OtherProblem,
}

impl BedProblem {
    /// 官方 `BedSleepingProblem` 挂在玩家快捷栏上方的翻译键。
    ///
    /// The action-bar translation key of official `BedSleepingProblem`.
    pub fn message(self) -> &'static str {
        match self {
            Self::NotPossibleNow => "block.minecraft.bed.no_sleep",
            Self::NotSafe => "block.minecraft.bed.not_safe",
            Self::TooFarAway => "block.minecraft.bed.too_far_away",
            Self::Obstructed => "block.minecraft.bed.obstructed",
            Self::OtherProblem => "block.minecraft.bed.occupied",
        }
    }
}

/// [`Level::hurt_player_now`] 的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerHurtNow {
    /// 官方 `hurtServer` 的返回值。
    pub hurt: bool,
    /// 这一下把玩家打死时，`ServerPlayer.die` 要在世界里生成的掉落。
    pub death_drops: Option<PlayerDeathDrops>,
}

/// `Player.dropEquipment` + `LivingEntity.dropExperience`：死亡那一刻要生成的物品和经验。
/// 游戏层算好，由 [`crate::World::spawn_player_death_drops`] 在世界里生成。
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerDeathDrops {
    /// 玩家死时的位置（经验球的落点）。
    pub position: mc_bamboo_core::Vec3,
    /// 按背包顺序掉出的物品。
    pub items: Vec<mc_bamboo_protocol::item::ItemStack>,
    /// 经验值，0 表示不掉。
    pub experience: i32,
}

/// [`Level::monster_prevents_rest`] 的缺省实现，单独拿出来让覆盖它的假世界也能调到。
///
/// The default body of [`Level::monster_prevents_rest`], callable from overrides.
pub fn monster_prevents_rest_in<L: Level + ?Sized>(level: &L, pos: BlockPos) -> bool {
    let x = f64::from(pos.x) + 0.5;
    let y = f64::from(pos.y);
    let z = f64::from(pos.z) + 0.5;
    let area = mc_bamboo_core::Aabb::new(x - 8.0, y - 5.0, z - 8.0, x + 8.0, y + 5.0, z + 8.0);
    let mut found = false;
    level.for_each_entity_in(area, &mut |public| {
        if !public.removed
            && let crate::entity::KindPublic::Mob(mob) = &public.kind
            && mob.mob_type.is_monster()
        {
            found = true;
        }
    });
    found
}

/// 一次操作留下的、方块以外的结果，由调用方发给玩家。
///
/// The non-block results of one action, handed to the caller for delivery.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// `AbstractBoat.tickBubbleColumn`：给跟踪者广播实体事件，并甩下载具的乘客。
    ///
    /// `AbstractBoat.tickBubbleColumn`: broadcast an entity event and eject the
    /// vehicle's passengers.
    BoatBubbleEvent {
        entity: i32,
        event: u8,
        ejected: Vec<i32>,
    },
    /// `ShoulderRidingEntity.setEntityOnShoulder`：把整只鹦鹉的 NBT 存进玩家的
    /// 左 / 右肩膀槽，并把这只鹦鹉从世界移除。游戏层负责改玩家状态和实体表。
    ///
    /// `ShoulderRidingEntity.setEntityOnShoulder`: store the parrot's NBT in the
    /// player's shoulder slot and remove the parrot from the world. The game layer
    /// updates the player state and the entity table.
    PlayerShoulder {
        player: i32,
        tag: mc_bamboo_nbt::Tag,
    },
    /// `levelEvent(2001, pos, state)`。`except_initiator` 表示不用发给发起者，它自己已经预测过了。
    Destroyed {
        pos: BlockPos,
        state: BlockState,
        except_initiator: bool,
    },
    /// 振动飞行粒子：`VibrationParticleOption(BlockPositionSource(pos), travel)`。
    /// 游戏层按位置源补上目标格，广播给 32 格内的玩家。
    VibrationParticle {
        pos: BlockPos,
        x: f64,
        y: f64,
        z: f64,
        travel: i32,
    },
    /// `levelEvent(1502, pos, 0)`：红石火把开关得太频繁，烧坏时冒的那股烟。
    TorchBurnout { pos: BlockPos },
    /// 一次繁殖落地了幼崽。游戏层按 `lover` 找出求爱者，触发 `BRED_ANIMALS`。
    ///
    /// A breeding produced offspring. The game layer credits the `lover` player's
    /// `BRED_ANIMALS` trigger.
    BredAnimals {
        father: &'static str,
        mother: &'static str,
        child: &'static str,
        lover: Option<[u8; 16]>,
        position: mc_bamboo_core::Vec3,
    },
    /// 一次召唤落地了实体：铁 / 雪 / 铜傀儡、凋灵。游戏层按半径给附近玩家触发。
    ///
    /// A golem or wither was summoned. The game layer credits nearby players' triggers.
    SummonedEntity {
        entity: &'static str,
        position: mc_bamboo_core::Vec3,
        radius: f64,
    },
    /// `CelebrateVillagersSurvivedRaid` 生成的烟花火箭。携带构造好的烟花物品（`FIREWORKS`
    /// 组件），只记录、不落地实体（烟花火箭实体另开任务）。
    ///
    /// A firework rocket spawned by `CelebrateVillagersSurvivedRaid`, carrying the built
    /// firework item (`FIREWORKS` component); recorded but not spawned (the entity is a
    /// separate task).
    Firework {
        position: mc_bamboo_core::Vec3,
        item: ItemStack,
    },
    /// 箭击中并杀死了实体：游戏层给射手触发 `KILLED_BY_ARROW`。
    ///
    /// An arrow killed an entity; the game layer credits the shooter's trigger.
    KilledByArrow {
        shooter: Option<[u8; 16]>,
        /// 被击杀的实体类型名字（最多 5 个，超过按 5 计）。
        victims: [&'static str; 5],
        weapon: &'static str,
    },
    /// 合成器合成成功。游戏层按半径给附近玩家触发 `CRAFTER_RECIPE_CRAFTED`。
    ///
    /// A crafter crafted a recipe. The game layer credits nearby players' triggers.
    CrafterCrafted {
        recipe: &'static str,
        position: mc_bamboo_core::Vec3,
    },
    /// `ThrownEnderpearl.onHit`：把扔出珍珠的玩家传送到 `position`（珍珠命中前的
    /// `oldPosition()`）。游戏层走正式的 `movement::teleport` 并清下落距离。
    ///
    /// `ThrownEnderpearl.onHit`: teleport the pearl's owner to `position` (the
    /// projectile's `oldPosition()` before impact). The game layer uses the
    /// normal `movement::teleport` and clears fall distance.
    EnderPearlTeleport {
        target: crate::entity::EntityUid,
        position: mc_bamboo_core::Vec3,
    },
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
    /// `ServerLevel.explode` 发给范围内玩家的 `ClientboundExplodePacket`。
    /// 游戏层按 `hit_players` 给每名玩家带上自己那份击退向量。
    ///
    /// `ServerLevel.explode` broadcasts `ClientboundExplodePacket` to players in range.
    /// The game layer picks each player's own knockback out of `hit_players`.
    Explode {
        center: mc_bamboo_core::Vec3,
        radius: f32,
        block_count: i32,
        /// 被击退的玩家 `(uid, 击退向量)`，和 `ServerExplosion.getHitPlayers` 对应。
        ///
        /// The knocked-back players `(uid, knockback)`, mirroring `getHitPlayers`.
        hit_players: Vec<(crate::entity::EntityUid, mc_bamboo_core::Vec3)>,
        /// `ServerExplosion.isSmall()`：小爆炸用 `EXPLOSION` 粒子。
        is_small: bool,
        /// `playSound`：来源实体不静音。
        play_sound: bool,
    },
    /// `levelEvent`：客户端自己放粒子和声音。传播范围是 64 格，坐标是方块整数坐标。
    LevelEvent {
        event: i32,
        pos: BlockPos,
        data: i32,
        except_initiator: bool,
    },
    /// `ServerLevel.globalLevelEvent`：发给全服所有玩家，不看距离；协议包的
    /// `global` 标志为真（客户端无条件播放）。
    ///
    /// `ServerLevel.globalLevelEvent`: sent to every player regardless of distance,
    /// with the `global` flag set so the client plays it unconditionally.
    GlobalLevelEvent {
        event: i32,
        pos: BlockPos,
        data: i32,
    },
    /// `ServerLevel.sendParticles`：一个简单粒子包（幽匿催发体的 `sculk_soul`）。
    /// 游戏层发给 32 格内的玩家。
    ///
    /// A simple `ServerLevel.sendParticles` packet (the catalyst's `sculk_soul`).
    Particles {
        particle: i32,
        position: mc_bamboo_core::Vec3,
        offset: mc_bamboo_core::Vec3,
        speed: f32,
        count: i32,
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
    /// 一次生物对玩家的近战伤害。算阶段记意图，同一刻的实体循环里按实体顺序
    /// 调玩家的 `hurtServer`（游戏层接正式入口）。
    HurtPlayer {
        target: EntityUid,
        source: DamageSource,
        amount: f32,
    },
    /// `ServerLevel.destroyBlockProgress(breaker, pos, progress)`：32 格内、不是 `breaker`
    /// 自己的玩家收到裂纹包。生物破门用它；`progress` 为 -1 时清掉。
    DestroyProgress {
        breaker: i32,
        pos: BlockPos,
        progress: i32,
    },
    /// `Zombie.doHurtTarget` 的着火传染打到玩家：`igniteForTicks(ticks)`。
    IgnitePlayer { target: EntityUid, ticks: i32 },
    /// 药水箭 / 光灵箭命中玩家：`LivingEntity.addEffect`。瞬间效果由游戏层当场结算。
    ///
    /// A tipped or spectral arrow hit a player: `LivingEntity.addEffect`.
    /// The game layer settles instantaneous effects at once.
    PlayerEffect {
        target: EntityUid,
        effect: i32,
        duration: i32,
        amplifier: i32,
        ambient: bool,
        visible: bool,
        show_icon: bool,
    },
    /// 喷溅治疗药水按距离缩放给玩家回血：`LivingEntity.heal(amount)`。
    ///
    /// A splash healing potion heals a player by a distance-scaled amount.
    HealPlayer { target: EntityUid, amount: f32 },
    /// `ElderGuardian.customServerAiStep`：只给挂上挖掘疲劳的那名玩家发
    /// `GUARDIAN_ELDER_EFFECT` 游戏事件包。
    ///
    /// `ElderGuardian.customServerAiStep`: send the `GUARDIAN_ELDER_EFFECT` game event
    /// packet only to the player who received mining fatigue.
    GuardianElderEffect { target: EntityUid, silent: bool },
    /// `transformBadOmenIntoTrialOmen`：把玩家的不祥之兆换成试炼之兆。游戏层按
    /// `player` 找玩家、移除 `BAD_OMEN`、加上 `TRIAL_OMEN`（时长 `duration`）。
    ///
    /// `transformBadOmenIntoTrialOmen`: replace the player's bad omen with trial omen.
    /// The game layer finds the player by UUID, removes `BAD_OMEN` and adds `TRIAL_OMEN`.
    TrialSpawnerTransformBadOmen { player: [u8; 16], duration: i32 },
    /// `Raider.die`：一名袭击者死亡。游戏层按位置找活跃袭击、把击杀玩家记进
    /// `heroesOfTheVillage`，并从袭击名册里移除这只实体。
    ///
    /// `Raider.die`: a raider died. The game layer finds the active raid at the
    /// position, credits the killing player as a hero and removes the raider.
    RaiderDied {
        uid: i32,
        position: mc_bamboo_core::Vec3,
        killer: Option<[u8; 16]>,
    },
    /// `Raid.setLeader(wave, raider)`：袭击者捡起灾厄旗帜后登记为这一波的队长。
    ///
    /// `Raid.setLeader(wave, raider)`: a raider picked up the ominous banner and
    /// became this wave's leader.
    RaidSetLeader {
        id: i32,
        wave: i32,
        leader: crate::entity::EntityUid,
    },
    /// 官方 `ServerPlayer.startSleepInBed` 的结果。方块侧已经把床的 `occupied`
    /// 置位（成功时）或没动（失败时），游戏层接着设置玩家睡姿、重生点、提示和包。
    ///
    /// The result of official `ServerPlayer.startSleepInBed`. Block logic has
    /// already set the bed's `occupied` flag on success; the game layer sets
    /// the player's sleeping pose, spawn point, message and packets.
    SleepInBed {
        pos: BlockPos,
        /// `BedRule.canSetSpawn`：普通床为真，草垫为假。
        set_spawn: bool,
        /// `Some` 是失败原因，`None` 是成功入睡。
        problem: Option<BedProblem>,
    },
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
        fn set_crafter_crafting(&mut self, _pos: BlockPos, _remaining: i32) {
            unimplemented!()
        }
        fn crafter_crafting(&self) -> Vec<(BlockPos, i32)> {
            unimplemented!()
        }
        fn crafter_crafting_at(&self, _pos: BlockPos) -> i32 {
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
        memory.set_crafter_crafting(A1, 4);
        memory.set_lectern(A1, FACTS);
        memory.set_comparator_output(B1, 9);
        memory.set_crafter_disabled(B1, 0b11);
        memory.record_torch_toggle(A1, 100);
        memory.record_torch_toggle(B1, 101);
        memory.record_torch_toggle(A1, 102);
    }

    /// 所有能观察到的读，用来比较搬前搬后。倒计时取走后原样放回。
    fn observed(memory: &mut RedstoneMemory, now: i64) -> Vec<String> {
        let countdowns = memory.crafter_crafting();
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
        // `forget_torch_toggles_before` 也依赖时间顺序，搬完以后它仍要按时刻工作。
        memory.forget_torch_toggles_before(now);
        out.push(format!("toggles:{}", memory.torch_toggles.len()));
        out
    }

    /// 拆成两个单元再并回来：所有可读的状态、倒计时顺序、火把抖动表的
    /// （时刻、先后）顺序都和从没拆过逐条相同。
    #[test]
    fn redstone_split_and_absorb_keep_every_observation() {
        let mut reference = RedstoneMemory::default();
        let mut split = RedstoneMemory::default();
        fill(&mut reference);
        fill(&mut split);
        // 同一时刻的开关，先后由入队顺序决定，拆合要保住这个相对顺序。
        split.record_torch_toggle(B1, 101);
        split.record_torch_toggle(A1, 101);
        reference.record_torch_toggle(B1, 101);
        reference.record_torch_toggle(A1, 101);
        let wanted: std::collections::BTreeSet<ChunkPos> = [CHUNK_A].into_iter().collect();
        let unit = split.split_off(&wanted);
        // 拆出去那份只剩 0 号区块的记忆。
        assert_eq!(unit.comparator_output(A1), 7);
        assert_eq!(unit.comparator_output(B1), 0);
        assert_eq!(split.comparator_output(A1), 0);
        assert_eq!(split.comparator_output(B1), 9);
        assert_eq!(unit.torch_toggle_count(A1), 3);
        assert_eq!(split.torch_toggle_count(B1), 2);

        split.absorb(unit);
        let before = observed(&mut reference, 101);
        let after = observed(&mut split, 101);
        assert_eq!(after, before);
        // 火把表的顺序不能变成「按区块再按位置」：和一份不拆的参照逐条比。
        let mut expected = RedstoneMemory::default();
        fill(&mut expected);
        expected.record_torch_toggle(B1, 101);
        expected.record_torch_toggle(A1, 101);
        // 官方的开关表按时刻升序；同一时刻的先后按入队顺序（稳定排序）。
        expected.torch_toggles.sort_by_key(|&(_, when)| when);
        let mut full = RedstoneMemory::default();
        fill(&mut full);
        full.record_torch_toggle(B1, 101);
        full.record_torch_toggle(A1, 101);
        let unit = full.split_off(&wanted);
        full.absorb(unit);
        assert_eq!(full.torch_toggles, expected.torch_toggles);
        // 对照组：按区块再按位置排出来的顺序确实不同。
        let mut chunked = expected.torch_toggles.clone();
        chunked.sort_by_key(|(pos, when)| (pos.chunk(), *pos, *when));
        assert_ne!(full.torch_toggles, chunked);
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
        assert_eq!(memory.crafter_crafting().len(), 0);

        memory.insert_chunk(boxed, 102);
        assert_eq!(memory.comparator_output(A1), 7);
        assert_eq!(memory.last_bookshelf_slot(A1), Some(3));
        assert!(memory.is_jukebox_playing(A1));
        assert_eq!(memory.crafter_disabled(A1), 0b101);
        assert_eq!(memory.lectern(A1), Some(FACTS));
        assert_eq!(memory.torch_toggle_count(A1), 2);
        assert_eq!(memory.crafter_crafting(), [(A1, 4)]);
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

    /// 火把开关记录的随机脚本：每刻按官方火把计划刻的口径先丢掉 60 刻之前的记录、
    /// 再随机记几次开关，观测每格还剩几次（`isToggledTooFrequently` 看的就是它）。
    /// `shift` 给出时第 `mid` 刻后平移 `delta`（`apply` 为假时故意不平移）。
    ///
    /// Random torch-toggle script; observes the per-cell count the burnout
    /// check reads. With `shift`, rebase by `delta` after step `mid` (or
    /// deliberately skip it when `apply` is false).
    fn torch_script(seed: u64, base: i64, shift: Option<(i64, i64, bool)>) -> Vec<Vec<usize>> {
        const TORCHES: [BlockPos; 3] = [
            BlockPos::new(0, 64, 0),
            BlockPos::new(20, 64, 0),
            BlockPos::new(40, 64, 7),
        ];
        let mut state = seed | 1;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut memory = RedstoneMemory::default();
        let mut offset = 0;
        let mut out = Vec::new();
        for step in 1..=200_i64 {
            let now = base + offset + step;
            memory.forget_torch_toggles_before(now - 60);
            for _ in 0..next() % 3 {
                let pos = TORCHES[(next() % 3) as usize];
                memory.record_torch_toggle(pos, now);
            }
            out.push(
                TORCHES
                    .iter()
                    .map(|&pos| memory.torch_toggle_count(pos))
                    .collect(),
            );
            if let Some((mid, delta, apply)) = shift
                && step == mid
            {
                if apply {
                    memory.rebase(delta);
                }
                offset = delta;
            }
        }
        out
    }

    /// 红石火把开关记录一类的平移：「第 100 刻后平移 d 再跑」等于「时钟本来就差 d」；
    /// 改坏确认：不平移就会被抓到。
    ///
    /// Torch toggles: shifting mid-run equals a clock that was always d apart;
    /// skipping the shift is caught.
    #[test]
    fn rebasing_torch_toggles_equals_a_clock_that_was_always_apart() {
        let mut caught = false;
        for seed in 1..=30_u64 {
            let base = (seed as i64 * 977) % 5000 - 2500;
            let delta = (seed as i64 * 7919) % 9000 - 4500;
            let apart = torch_script(seed, base + delta, None);
            assert_eq!(
                torch_script(seed, base, Some((100, delta, true))),
                apart,
                "seed {seed} base {base} delta {delta}"
            );
            caught |= torch_script(seed, base, Some((100, delta, false))) != apart;
        }
        assert!(caught, "skipping the torch-toggle rebase went unnoticed");
    }
}
