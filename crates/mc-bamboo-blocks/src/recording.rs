//! 录制一次触发读到的状态和产生的效果，供离线算缓存命中率。
//!
//! [`Recording`] 包住一个 `&mut dyn Level`，自己实现 [`Level`] 和 [`LevelSideState`]：
//! 每个方法先转发给里面的世界，再把这次读（位置、类别、值）或写记进当前触发。
//! 方块逻辑里不经过 `Level` 的事件由 [`Level::observe`] 补进来，见 [`Observation`]。
//!
//! 录制只记不改：转发的返回值、调用顺序和副作用都和原来一样。驱动循环在触发边界
//! 调 `observe`，于是每条记录的范围就是「一次触发加它引起的全部连锁更新」。
//!
//! Records what a trigger reads and produces, for offline cache-hit-rate analysis.
//! [`Recording`] wraps a `&mut dyn Level` and implements [`Level`] and
//! [`LevelSideState`], forwarding every method and logging the read (position,
//! category, value) or write. Events that bypass `Level` arrive via
//! [`Level::observe`]. Recording never changes behaviour.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    time::Instant,
};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, sign::SignContent};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    BlockState, Container, HopperEntity, Level, LevelRandom,
    behaviour::machines::{
        BrewingStandEntity, CampfireEntity, FurnaceEntity, PistonMoving, ShulkerBoxEntity,
    },
    block_entity::{BlockEntity, BlockEvent},
    cache::{GuardRead, ReadMethod, guard::trigger_pos},
    cache::{ReplayEffect, RuntimeRecord, SectionKey},
    entity::{Entity, EntityPublic, EntityState, EntityStore, EntityUid},
    gamerules::BlockGameRules,
    level::{
        Effect, EquipTarget, LecternFacts, LevelEntities, LevelSideState, Observation, Trigger,
    },
    shape::Aabb,
    ticks::TickPriority,
    weather::{BlockWeather, PrecipitationFacts},
};

/// 一次读：位置（世界级的量没有位置）、读的是哪一类值、值的哈希。
///
/// 精确键用全部读项；段哈希键只用位置，值取整段内容哈希。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Read {
    pub pos: Option<BlockPos>,
    pub kind: u8,
    pub value: u64,
}

/// 一次写，按触发里的先后顺序排好。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Write {
    Block {
        pos: BlockPos,
        state: BlockState,
        flags: u32,
        applied: bool,
    },
    Redstone {
        pos: Option<BlockPos>,
        kind: u8,
        value: u64,
    },
    BlockEvent {
        event: BlockEvent,
        popped: Option<BlockEvent>,
    },
    BlockEntity {
        pos: BlockPos,
        kind: u8,
        value: u64,
    },
    Container {
        pos: BlockPos,
        kind: u8,
        value: u64,
    },
    Sign {
        pos: BlockPos,
        value: u64,
        applied: bool,
    },
    TickScheduled {
        pos: BlockPos,
        kind: i64,
        fluid: bool,
        delay: i32,
        priority: TickPriority,
    },
    Effect(u64),
    Ticker(u8),
}

impl Write {
    /// 这次写落在哪一格；没有位置的写返回 `None`。
    pub const fn pos(self) -> Option<BlockPos> {
        match self {
            Self::Block { pos, .. }
            | Self::Redstone { pos: Some(pos), .. }
            | Self::BlockEvent {
                event: BlockEvent { pos, .. },
                ..
            }
            | Self::BlockEntity { pos, .. }
            | Self::Container { pos, .. }
            | Self::Sign { pos, .. }
            | Self::TickScheduled { pos, .. } => Some(pos),
            Self::Effect(_) | Self::Ticker(_) | Self::Redstone { pos: None, .. } => None,
        }
    }
}

/// 不能缓存的原因。一次触发可能同时占好几个。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Uncacheable {
    Random,
    Entities,
    Time,
}

/// 一个 16³ 的区块段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkSection {
    pub chunk: ChunkPos,
    pub section_y: i32,
}

impl ChunkSection {
    pub const fn of(pos: BlockPos) -> Self {
        Self {
            chunk: pos.chunk(),
            section_y: pos.y >> 4,
        }
    }
}

/// 一次触发的完整记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// 记录发生在第几刻，只用于输出，不进键。
    pub tick: i64,
    pub trigger: Trigger,
    pub reads: Vec<Read>,
    pub writes: Vec<Write>,
    pub uncacheable: Vec<Uncacheable>,
    /// 触发开始时，读集合涉及各段的整段内容哈希，按段排序。
    pub sections: Vec<(ChunkSection, u128)>,
    /// 这次触发本身花了多少纳秒（含录制开销）。
    pub nanos: u64,
}

impl Record {
    /// 精确键：触发本身 + 读到的全部（位置，类别，值），按读的先后顺序。
    ///
    /// 顺序进键是刻意的：读集合相同但顺序不同说明执行路径不同。
    pub fn exact_key(&self, strict_time: bool) -> u128 {
        let mut hasher = StableHasher::new();
        self.trigger.hash(&mut hasher);
        for read in &self.reads {
            if strict_time || (read.kind != K_TIME && read.kind != K_ENTITY_TIME) {
                read.hash(&mut hasher);
            }
        }
        hasher.finish128()
    }

    /// 纯段哈希键（设计稿 9.3 第一版）：触发本身 + 读集合涉及的各段的整段方块状态哈希，
    /// 再加那些「不在方块状态里」的读项的精确（位置，类别，值）。
    ///
    /// 方块状态类读项（`K_BLOCK`）只进段哈希；红石记忆、方块实体内容、容器内容、
    /// 计划刻查询、亮度、告示牌等仍按精确值进键。段内容全相同则读到的方块状态一定
    /// 相同，所以它天然不粗于精确键，不必把整份精确读集合塞进去。
    pub fn pure_section_key(&self, strict_time: bool) -> u128 {
        let mut hasher = StableHasher::new();
        self.trigger.hash(&mut hasher);
        for (section, hash) in &self.sections {
            section.hash(&mut hasher);
            hash.hash(&mut hasher);
        }
        for read in &self.reads {
            if (strict_time || read.kind != K_TIME && read.kind != K_ENTITY_TIME)
                && read.kind != K_BLOCK
            {
                read.hash(&mut hasher);
            }
        }
        hasher.finish128()
    }

    /// 纯段哈希键里要存的键部分：每段的 (段坐标, 128 位哈希) 加非方块状态读项。
    pub fn pure_section_key_bytes(&self) -> usize {
        let extra = self
            .reads
            .iter()
            .filter(|read| read.kind != K_BLOCK)
            .count()
            * std::mem::size_of::<Read>();
        self.sections.len() * std::mem::size_of::<(ChunkSection, u128)>() + extra
    }

    /// 混合段哈希键：精确读集合 + 各段的整段状态哈希。留作对照。
    pub fn section_key(&self, strict_time: bool) -> u128 {
        let mut hasher = StableHasher::new();
        self.trigger.hash(&mut hasher);
        for read in &self.reads {
            if strict_time || (read.kind != K_TIME && read.kind != K_ENTITY_TIME) {
                read.hash(&mut hasher);
            }
        }
        for (section, hash) in &self.sections {
            section.hash(&mut hasher);
            hash.hash(&mut hasher);
        }
        hasher.finish128()
    }

    /// 效果：写序列加不可缓存标记。命中要求它和记录时完全相同。
    pub fn effect_key(&self) -> u128 {
        let mut hasher = StableHasher::new();
        self.writes.hash(&mut hasher);
        self.uncacheable.hash(&mut hasher);
        hasher.finish128()
    }

    /// 读集合里去重后的位置数。
    pub fn read_positions(&self) -> usize {
        self.reads
            .iter()
            .filter_map(|read| read.pos)
            .collect::<HashSet<_>>()
            .len()
    }

    /// 写集合里去重后的位置数。
    pub fn write_positions(&self) -> usize {
        self.writes
            .iter()
            .filter_map(|write| write.pos())
            .collect::<HashSet<_>>()
            .len()
    }

    /// 这条记录自身占的内存估算（字节）。
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.reads.len() * std::mem::size_of::<Read>()
            + self.writes.len() * std::mem::size_of::<Write>()
            + self.uncacheable.len() * std::mem::size_of::<Uncacheable>()
            + self.sections.len() * std::mem::size_of::<(ChunkSection, u128)>()
    }

    /// 这次触发能不能进缓存（严格口径 / 乐观口径）。
    pub fn cacheable(&self, strict_time: bool) -> bool {
        if strict_time {
            self.uncacheable.is_empty()
        } else {
            !self.uncacheable.contains(&Uncacheable::Random)
                && !self.uncacheable.contains(&Uncacheable::Entities)
        }
    }
}

/// 录制用的 `Level` 包装。
///
/// 用 [`Self::new`] 借一个 `&mut dyn Level`，用 [`Self::take_records`] 取出记录。
/// 录制状态放在 `RefCell` 里，因为 [`Level::observe`] 的接收者是 `&self`。
pub struct Recording<'a> {
    level: &'a mut dyn Level,
    inner: RefCell<Inner>,
    /// 记账模式放在 `Cell` 里：`Off` 时每次转发只是一次非原子的加载和比较，
    /// 不用借 `RefCell`，这是「不缓存的触发点常驻开销接近 0」的关键。
    mode: Cell<crate::level::RecordMode>,
    /// `Count` 模式下的读写项数同样只是一次 `Cell` 加法。
    counted: Cell<u64>,
}

struct Inner {
    records: Vec<Record>,
    current: Option<Frame>,
    /// 驱动循环在两次触发边界之间做的读写，归到下一次触发。
    pending_reads: Vec<Read>,
    pending_writes: Vec<Write>,
    pending_uncacheable: Vec<Uncacheable>,
    /// 驱动循环在两次触发边界之间做的运行时守卫读项，归到下一次触发。
    pending_runtime_reads: Vec<GuardRead>,
    /// 边界外的运行时读项里有没有守卫阶段无法重算的（比如 `next_ticker`）。
    pending_rejected: bool,
    /// 每段现在的内容哈希，写方块时增量更新（设计稿 9.3 的做法）。
    section_hashes: HashMap<ChunkSection, u128>,
    runtime: Vec<RuntimeRecord>,
    /// 两次触发边界之间发生过写，下一次触发的运行时记录作废。
    boundary_writes: bool,
    tick: i64,
}

struct Frame {
    record: Record,
    runtime: RuntimeFrame,
    started: Instant,
    /// 触发开始时各段的哈希快照。
    section_start: HashMap<ChunkSection, u128>,
}

/// 一次触发的运行时录制，和实验的 `Record` 并行产出。
#[derive(Debug, Default)]
struct RuntimeFrame {
    trigger: Option<Trigger>,
    trigger_state: u32,
    tick: i64,
    reads: Vec<GuardRead>,
    effects: Vec<ReplayEffect>,
    uncacheable: Vec<Uncacheable>,
    boundary_writes: bool,
    /// 录制到了白名单以外的读或写，整条运行时记录作废。
    rejected: bool,
    /// 这一帧里「必须被守卫覆盖」的实验读项数；每记一条守卫读项加一，
    /// 结束时和实验读项数核对，漏一个读就整条作废。
    guarded_reads: usize,
    /// 这一帧有没有 `Observation::PistonPitch`：只有它伴随的 `seed: None`
    /// 音效才允许重放，别把别的音效当成活塞音效放过。
    piston_pitch_seen: bool,
}

impl<'a> Recording<'a> {
    /// 包住一个世界。`tick` 由驱动方在每个世界刻开头更新，只用来给记录标时刻。
    pub fn new(level: &'a mut dyn Level) -> Self {
        Self {
            level,
            inner: RefCell::new(Inner {
                records: Vec::new(),
                current: None,
                pending_reads: Vec::new(),
                pending_writes: Vec::new(),
                pending_uncacheable: Vec::new(),
                pending_runtime_reads: Vec::new(),
                pending_rejected: false,
                section_hashes: HashMap::new(),
                runtime: Vec::new(),
                boundary_writes: false,
                tick: 0,
            }),
            mode: Cell::new(crate::level::RecordMode::Full),
            counted: Cell::new(0),
        }
    }

    /// 把时钟拨到 `tick`，丢掉上一刻遗留的边界读写。
    ///
    /// 边界外的写会留在 `boundary_writes` 里，让下一次触发的运行时记录作废
    /// （设计稿 §4.2）：实验的 `Record` 仍然把它归到下一次触发，历史数字不变。
    pub fn set_tick(&self, tick: i64) {
        let mut inner = self.inner.borrow_mut();
        inner.tick = tick;
        inner.pending_reads.clear();
        inner.pending_writes.clear();
        inner.pending_uncacheable.clear();
        inner.pending_runtime_reads.clear();
        inner.pending_rejected = false;
        inner.boundary_writes = false;
    }

    /// 切换记账模式。守卫和重放阶段必须是 `Off`：它们产生的读写不是解释执行的结果，
    /// 不能进读集合或效果序列。实验驱动不调用这个方法，行为不变。
    pub fn set_capture(&self, capture: bool) {
        self.set_record_mode(if capture {
            crate::level::RecordMode::Full
        } else {
            crate::level::RecordMode::Off
        });
    }

    /// 按 [`crate::level::RecordMode`] 切换。计数由 `take_counted_reads` 单独取走。
    pub fn set_record_mode(&self, mode: crate::level::RecordMode) {
        self.mode.set(mode);
    }

    /// 取走目前为止的全部记录，清空。
    pub fn take_records(&self) -> Vec<Record> {
        std::mem::take(&mut self.inner.borrow_mut().records)
    }

    /// 取走运行时记录（带真实值的守卫读项和效果），实验的 `Record` 不受影响。
    pub fn take_runtime_records(&self) -> Vec<RuntimeRecord> {
        std::mem::take(&mut self.inner.borrow_mut().runtime)
    }

    /// 实验录制层的一读或一写：`Count` 模式只累加一个计数器，于是计数和
    /// `RuntimeRecord::work()` 的读项数 + 写项数一一对应。`Off` 时只加载一次 `Cell`。
    fn accounting(&self) -> bool {
        match self.mode.get() {
            crate::level::RecordMode::Full => true,
            crate::level::RecordMode::Count => {
                self.counted.set(self.counted.get().saturating_add(1));
                false
            }
            crate::level::RecordMode::Off => false,
        }
    }

    /// 运行时记录专用：不重复计数（它和实验的读/写是同一次底层操作）。
    fn full_only(&self) -> bool {
        self.mode.get() == crate::level::RecordMode::Full
    }

    /// 记一条「必须与某个实验读项一一对应」的守卫读项。
    fn guarded_read(&self, method: ReadMethod, pos: Option<BlockPos>, value: impl Hash) {
        let guard = GuardRead::new(method, pos, value);
        if !self.full_only() {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        match inner.current.as_mut() {
            Some(frame) => {
                frame.runtime.reads.push(guard);
                frame.runtime.guarded_reads = frame.runtime.guarded_reads.saturating_add(1);
            }
            None => inner.pending_runtime_reads.push(guard),
        }
    }

    /// 一次运行时读。白名单之外的读也记下来，让记录被判为不可缓存。
    fn runtime_read(&self, method: ReadMethod, pos: Option<BlockPos>, value: impl Hash) {
        if !self.full_only() {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        let guard = GuardRead::new(method, pos, value);
        match inner.current.as_mut() {
            Some(frame) => frame.runtime.reads.push(guard),
            None => inner.pending_runtime_reads.push(guard),
        }
    }

    /// 一次运行时效果。白名单之外的写用 `reject_runtime` 作废整条记录。
    fn runtime_effect(&self, effect: ReplayEffect) {
        if !self.full_only() {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        if let Some(frame) = inner.current.as_mut() {
            frame.runtime.effects.push(effect);
        }
    }

    fn reject_runtime(&self) {
        if !self.full_only() {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        match inner.current.as_mut() {
            Some(frame) => frame.runtime.rejected = true,
            None => inner.pending_rejected = true,
        }
    }

    /// 借回里面的世界，供实验程序读状态。录制器还活着时不要用它跑方块逻辑。
    pub fn level(&mut self) -> &mut dyn Level {
        self.level
    }

    /// 一次读进读集合。
    fn read(&self, pos: Option<BlockPos>, kind: u8, value: impl Hash) {
        if !self.accounting() {
            return;
        }
        // 白名单之外的读项让这条运行时记录作废，见 `RuntimeRecord::supported`。
        if !crate::cache::supported_kind(kind) {
            self.reject_runtime();
        }
        // 运行时守卫按「类别 + 真实值」记一笔，白名单之外的读项让记录作废。
        if !crate::cache::supported_kind(kind) {
            self.reject_runtime();
        }
        let value = stable_hash(&value);
        let section = pos.map(ChunkSection::of);
        if let Some(section) = section {
            self.ensure_section(section);
        }
        let entry = Read { pos, kind, value };
        let hash =
            section.and_then(|section| self.inner.borrow().section_hashes.get(&section).copied());
        let mut inner = self.inner.borrow_mut();
        match inner.current.as_mut() {
            Some(frame) => {
                if let Some((section, hash)) = section.zip(hash)
                    && !frame.section_start.contains_key(&section)
                {
                    frame.section_start.insert(section, hash);
                }
                frame.record.reads.push(entry);
            }
            None => inner.pending_reads.push(entry),
        }
    }

    /// ticker 驱动临时拿放实体的读：实验的 `Record` 照旧记两笔（历史数字不变），
    /// 但运行时不产生守卫项，见设计稿 §3.1「内部递增 id、驱动临时拿放实体不进键」。
    fn read_entity_driver(&self, pos: BlockPos, entity: Option<&dyn BlockEntity>, tag: u8) {
        let now = self.inner.borrow().tick;
        let facts = entity.map(entity_facts);
        let time = entity
            .map(|entity| entity_time_facts(entity, now))
            .unwrap_or(0);
        self.read(Some(pos), K_BLOCK_ENTITY, (tag, facts));
        self.read(Some(pos), K_ENTITY_TIME, time);
    }

    /// 一次方块实体读：内容哈希 + 时间派生部分各记一笔。
    fn read_entity(&self, pos: BlockPos, entity: Option<&dyn BlockEntity>, tag: u8) {
        let now = self.inner.borrow().tick;
        let facts = entity.map(entity_facts);
        let time = entity
            .map(|entity| entity_time_facts(entity, now))
            .unwrap_or(0);
        self.read(Some(pos), K_BLOCK_ENTITY, (tag, facts));
        self.read(Some(pos), K_ENTITY_TIME, time);
        self.guarded_read(
            ReadMethod::BlockEntity(pos, tag, 2),
            Some(pos),
            (tag, facts),
        );
        self.guarded_read(ReadMethod::EntityTime(pos), Some(pos), time);
    }

    /// 一次写进效果。
    fn write(&self, entry: Write) {
        if !self.accounting() {
            return;
        }
        // 白名单之外的写让这条运行时记录作废。
        if !crate::cache::supported_write(entry) {
            self.reject_runtime();
        }
        let mut inner = self.inner.borrow_mut();
        match inner.current.as_mut() {
            Some(frame) => frame.record.writes.push(entry),
            None => {
                inner.boundary_writes = true;
                inner.pending_writes.push(entry);
            }
        }
    }

    fn redstone_write(&self, pos: Option<BlockPos>, kind: u8, value: impl Hash) {
        let value = stable_hash(&value);
        // 只有这几个子标签有按值还原的效果；其余子标签出现时整条记录作废。
        match (pos, kind) {
            (Some(pos), 0) | (Some(pos), 6) => {
                if kind == 6 {
                    self.runtime_effect(ReplayEffect::ForgetBlockEntityMemory { pos });
                }
            }
            (Some(_), _) => self.reject_runtime(),
            (None, _) => self.reject_runtime(),
        }
        self.write(Write::Redstone { pos, kind, value });
    }

    /// 实体窄接口上的任意一次调用都算「碰了实体」。
    fn note_entities(&self, which: u8) {
        let _ = which;
        self.uncacheable(Uncacheable::Entities);
    }

    /// 标一个不能缓存的原因，同一次触发里只记一次。
    fn uncacheable(&self, reason: Uncacheable) {
        if !self.accounting() {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        match inner.current.as_mut() {
            Some(frame) => {
                if !frame.record.uncacheable.contains(&reason) {
                    frame.record.uncacheable.push(reason);
                }
                if !frame.runtime.uncacheable.contains(&reason) {
                    frame.runtime.uncacheable.push(reason);
                }
            }
            None => {
                if !inner.pending_uncacheable.contains(&reason) {
                    inner.pending_uncacheable.push(reason);
                }
            }
        }
    }

    /// 把世界时间换成相对量：所有带绝对时刻的记录值都写成「和当前刻的差」。
    fn relative_time(&self, when: i64) -> i64 {
        when.saturating_sub(self.inner.borrow().tick)
    }

    /// 段内容哈希没算过时扫一遍这 16³ 格。只在第一次碰到这一段时发生。
    fn ensure_section(&self, section: ChunkSection) {
        let mut inner = self.inner.borrow_mut();
        if inner.section_hashes.contains_key(&section) {
            return;
        }
        // 存储层维护好的整段哈希；没有实现 `section_hash` 的假世界退回逐格扫描。
        let hash = self
            .level
            .section_hash(section.chunk, section.section_y)
            .unwrap_or_else(|| {
                let mut hash = 0_u128;
                for y in section.section_y * 16..section.section_y * 16 + 16 {
                    for z in section.chunk.z * 16..section.chunk.z * 16 + 16 {
                        for x in section.chunk.x * 16..section.chunk.x * 16 + 16 {
                            let pos = BlockPos::new(x, y, z);
                            hash ^= cell_hash(pos, self.level.block_state(pos));
                        }
                    }
                }
                hash
            });
        inner.section_hashes.insert(section, hash);
    }

    /// 写方块时把旧值和新值从段哈希里异或掉/加进来，不用重扫整段。
    fn note_block_write(&self, pos: BlockPos, old: BlockState, new: BlockState) {
        let section = ChunkSection::of(pos);
        self.ensure_section(section);
        let mut inner = self.inner.borrow_mut();
        let entry = inner.section_hashes.entry(section).or_default();
        *entry ^= cell_hash(pos, old) ^ cell_hash(pos, new);
    }
}

// 读/写的类别编号。键里带上它：同一位置不同种类的值不能混。
const K_BLOCK: u8 = 1;
const K_BRIGHTNESS: u8 = 2;
const K_SIGN: u8 = 3;
const K_NOTE_SOUND: u8 = 4;
const K_BLOCK_ENTITY: u8 = 5;
const K_BLOCK_EVENT: u8 = 7;
const K_REDSTONE: u8 = 6;
const K_UNOBSTRUCTED: u8 = 8;
const K_MIN_Y: u8 = 9;
const K_MAX_Y: u8 = 10;
const K_RANDOM: u8 = 11;
const K_SECTION_TICKING: u8 = 12;
const K_PLAYER_NEAR: u8 = 13;
const K_GAME_RULES: u8 = 14;
const K_WEATHER: u8 = 15;
const K_SKY: u8 = 16;
const K_HEIGHTMAP: u8 = 17;
const K_PRECIPITATION: u8 = 18;
const K_CONTAINER: u8 = 19;
const K_OWN_COUNTS: u8 = 20;
const K_CONTAINER_SIGNAL: u8 = 21;
const K_CONTAINER_POSITIONS: u8 = 22;
const K_HOPPER: u8 = 23;
const K_SLOTS: u8 = 24;
const K_DROPS: u8 = 25;
const K_LOOT_UNPACK: u8 = 26;
const K_TARGETS: u8 = 27;
const K_EQUIP: u8 = 28;
const K_TICK_QUERY: u8 = 29;
const K_ENTITIES: u8 = 30;
/// 读世界时间这一项。乐观口径把它从键里去掉。
pub const K_TIME: u8 = 31;
/// 方块实体内容里由绝对游戏时刻换算出来的那部分（漏斗 `tickedGameTime`、
/// 移动活塞 `lastTicked`）。乐观口径也把它从键里去掉。
pub const K_ENTITY_TIME: u8 = 33;
const K_TICKER: u8 = 32;
/// 方块逻辑读世界边界（床、活塞、桶、龙蛋）时的那一次读。
///
/// The read made when block logic consults the world border (beds, pistons, buckets,
/// dragon eggs).
const K_WORLD_BORDER: u8 = 34;

/// 精确键和效果键用的 128 位哈希，算法固定，跨进程可比。
struct StableHasher {
    lo: u64,
    hi: u64,
}

impl StableHasher {
    const fn new() -> Self {
        Self {
            lo: 0x243f_6a88_85a3_08d3,
            hi: 0x1319_8a2e_0370_7344,
        }
    }

    fn finish128(self) -> u128 {
        u128::from(self.lo) | u128::from(self.hi) << 64
    }
}

impl Hasher for StableHasher {
    fn finish(&self) -> u64 {
        self.lo
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.lo ^= u64::from(byte);
            self.lo = self.lo.wrapping_mul(0x100_0000_01b3);
            self.hi = self.hi.rotate_left(7) ^ self.lo;
        }
    }
}

pub fn stable_hash(value: &impl Hash) -> u64 {
    let mut hasher = StableHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn cell_hash(pos: BlockPos, state: BlockState) -> u128 {
    mc_bamboo_core::section_hash::cell_hash(
        pos.x.rem_euclid(16) as usize,
        pos.y.rem_euclid(16) as usize,
        pos.z.rem_euclid(16) as usize,
        state.0,
    )
}

fn hash_effect(effect: &Effect) -> u64 {
    let mut hasher = StableHasher::new();
    let tag = match effect {
        Effect::Destroyed { .. } => 1_u8,
        Effect::TorchBurnout { .. } => 2,
        Effect::FilledBucket { .. } => 3,
        Effect::EmptiedBucket { .. } => 4,
        Effect::BoneMealUsed { .. } => 5,
        Effect::SignChanged { .. } => 6,
        Effect::SignWaxed { .. } => 7,
        Effect::OpenSignEditor { .. } => 8,
        Effect::Sound { .. } => 9,
        Effect::LevelEvent { .. } => 10,
        Effect::NoteBlockPlayed { .. } => 11,
        Effect::BlockEvent { .. } => 12,
        Effect::ComposterFill { .. } => 13,
        Effect::PageTurned { .. } => 14,
        Effect::DispenserEquipped { .. } => 15,
        Effect::CampfireChanged { .. } => 16,
        Effect::Overlay { .. } => 17,
        Effect::OpenCommandBlock { .. } => 18,
    };
    tag.hash(&mut hasher);
    match *effect {
        Effect::Destroyed {
            pos,
            state,
            except_initiator,
        } => {
            pos.hash(&mut hasher);
            state.0.hash(&mut hasher);
            except_initiator.hash(&mut hasher);
        }
        Effect::TorchBurnout { pos }
        | Effect::BoneMealUsed { pos }
        | Effect::SignChanged { pos }
        | Effect::SignWaxed { pos }
        | Effect::PageTurned { pos }
        | Effect::CampfireChanged { pos }
        | Effect::OpenCommandBlock { pos } => pos.hash(&mut hasher),
        Effect::FilledBucket { item, hand } => {
            item.hash(&mut hasher);
            (hand as u8).hash(&mut hasher);
        }
        Effect::EmptiedBucket { hand } => (hand as u8).hash(&mut hasher),
        Effect::OpenSignEditor { pos, slot } => {
            pos.hash(&mut hasher);
            (slot as u8).hash(&mut hasher);
        }
        Effect::Sound {
            sound,
            source,
            x,
            y,
            z,
            volume,
            pitch,
            except_initiator,
            seed,
        } => {
            sound.hash(&mut hasher);
            source.id().hash(&mut hasher);
            x.to_bits().hash(&mut hasher);
            y.to_bits().hash(&mut hasher);
            z.to_bits().hash(&mut hasher);
            volume.to_bits().hash(&mut hasher);
            pitch.to_bits().hash(&mut hasher);
            except_initiator.hash(&mut hasher);
            seed.hash(&mut hasher);
        }
        Effect::LevelEvent {
            event,
            pos,
            data,
            except_initiator,
        } => {
            event.hash(&mut hasher);
            pos.hash(&mut hasher);
            data.hash(&mut hasher);
            except_initiator.hash(&mut hasher);
        }
        Effect::NoteBlockPlayed {
            pos,
            instrument,
            note,
            pitch,
            seed,
            custom,
        } => {
            pos.hash(&mut hasher);
            instrument.name().hash(&mut hasher);
            note.hash(&mut hasher);
            pitch.to_bits().hash(&mut hasher);
            seed.hash(&mut hasher);
            custom.hash(&mut hasher);
        }
        Effect::BlockEvent {
            pos,
            block,
            param_a,
            param_b,
        } => {
            pos.hash(&mut hasher);
            block.hash(&mut hasher);
            param_a.hash(&mut hasher);
            param_b.hash(&mut hasher);
        }
        Effect::ComposterFill { pos, success } => {
            pos.hash(&mut hasher);
            success.hash(&mut hasher);
        }
        Effect::DispenserEquipped {
            entity_id,
            menu_slot,
        } => {
            entity_id.hash(&mut hasher);
            menu_slot.hash(&mut hasher);
        }
        Effect::Overlay { key, first, second } => {
            key.hash(&mut hasher);
            first.hash(&mut hasher);
            second.hash(&mut hasher);
        }
    }
    hasher.finish()
}

pub fn hash_slots(slots: &[ItemStack]) -> u64 {
    let mut hasher = StableHasher::new();
    slots.len().hash(&mut hasher);
    for stack in slots {
        stack.item.hash(&mut hasher);
        stack.count.hash(&mut hasher);
        for (id, bytes) in &stack.added {
            id.hash(&mut hasher);
            bytes.hash(&mut hasher);
        }
        stack.removed.hash(&mut hasher);
    }
    hasher.finish()
}

/// 缓存守卫重算用：和录制时同一个 `entity_facts` / `entity_time_facts` / `hash_slots`。
pub mod guard_facts {
    pub use super::{entity_facts, entity_time_facts, hash_slots};
}

/// 方块实体里由绝对游戏时刻决定的字段，单独记一笔读。
///
/// 严格口径把它留在键里；乐观口径（「把时间比较改成相对量」）把它去掉，
/// 否则活塞动画里 `lastTicked - now` 每刻都变，门明明周期性开合却永远不命中。
pub fn entity_time_facts(entity: &dyn BlockEntity, _now: i64) -> u64 {
    if let Some(hopper) = entity.as_any().downcast_ref::<HopperEntity>() {
        // 漏斗存的是相对量；`None` 是官方读盘后的初值 0。
        return stable_hash(&hopper.ticked_game_time_age());
    }
    // 移动活塞已不含绝对时刻，见 `PistonMoving::observed_facts`。
    0
}

fn hash_sign(content: &SignContent) -> u64 {
    let mut hasher = StableHasher::new();
    content.front.messages.hash(&mut hasher);
    content.back.messages.hash(&mut hasher);
    (content.front.color as u8).hash(&mut hasher);
    (content.back.color as u8).hash(&mut hasher);
    content.front.glowing.hash(&mut hasher);
    content.back.glowing.hash(&mut hasher);
    content.waxed.hash(&mut hasher);
    content.editor.hash(&mut hasher);
    hasher.finish()
}

/// 方块实体里能观察到的状态。私有字段通过各类自己的公开取值函数拿，
/// 带绝对时刻的字段换成相对值，见 [`Recording::relative_time`]。
pub fn entity_facts(entity: &dyn BlockEntity) -> u64 {
    let mut hasher = StableHasher::new();
    std::any::type_name_of_val(entity).hash(&mut hasher);
    if let Some(furnace) = entity.as_any().downcast_ref::<FurnaceEntity>() {
        hash_slots(furnace.items()).hash(&mut hasher);
        furnace.data_values().hash(&mut hasher);
        furnace.recipe_facts().hash(&mut hasher);
    } else if let Some(stand) = entity.as_any().downcast_ref::<BrewingStandEntity>() {
        hash_slots(stand.items()).hash(&mut hasher);
        stand.data_values().hash(&mut hasher);
        stand.observed_facts().hash(&mut hasher);
    } else if let Some(campfire) = entity.as_any().downcast_ref::<CampfireEntity>() {
        hash_slots(campfire.items()).hash(&mut hasher);
        campfire.data_values().hash(&mut hasher);
        campfire.recipe_facts().hash(&mut hasher);
    } else if let Some(hopper) = entity.as_any().downcast_ref::<HopperEntity>() {
        hopper.cooldown_time().hash(&mut hasher);
    } else if let Some(shulker) = entity.as_any().downcast_ref::<ShulkerBoxEntity>() {
        shulker.open_count().hash(&mut hasher);
        (shulker.status() as u8).hash(&mut hasher);
        shulker.progress(1.0).to_bits().hash(&mut hasher);
        shulker.progress(0.0).to_bits().hash(&mut hasher);
    } else if let Some(piston) = entity.as_any().downcast_ref::<PistonMoving>() {
        piston.observed_facts().hash(&mut hasher);
    }
    hasher.finish()
}

impl LevelSideState for Recording<'_> {
    fn comparator_output(&self, pos: BlockPos) -> i32 {
        let value = self.level.comparator_output(pos);
        self.read(Some(pos), K_REDSTONE, (0_u8, value));
        self.guarded_read(ReadMethod::Comparator(pos), Some(pos), value);
        value
    }

    fn set_comparator_output(&mut self, pos: BlockPos, signal: i32) {
        self.redstone_write(Some(pos), 0, (0_u8, signal));
        self.runtime_effect(ReplayEffect::Comparator { pos, signal });
        self.level.set_comparator_output(pos, signal);
    }

    fn last_bookshelf_slot(&self, pos: BlockPos) -> Option<usize> {
        let value = self.level.last_bookshelf_slot(pos);
        self.read(Some(pos), K_REDSTONE, (1_u8, value));
        self.guarded_read(ReadMethod::Redstone(Some(pos), 1), Some(pos), (1_u8, value));
        value
    }

    fn set_last_bookshelf_slot(&mut self, pos: BlockPos, slot: usize) {
        self.redstone_write(Some(pos), 1, (1_u8, slot));
        self.level.set_last_bookshelf_slot(pos, slot);
    }

    fn is_jukebox_playing(&self, pos: BlockPos) -> bool {
        let value = self.level.is_jukebox_playing(pos);
        self.read(Some(pos), K_REDSTONE, (2_u8, value));
        self.guarded_read(ReadMethod::Redstone(Some(pos), 2), Some(pos), (2_u8, value));
        value
    }

    fn set_jukebox_playing(&mut self, pos: BlockPos, playing: bool) {
        self.redstone_write(Some(pos), 2, (2_u8, playing));
        self.level.set_jukebox_playing(pos, playing);
    }

    fn crafter_disabled(&self, pos: BlockPos) -> u16 {
        let value = self.level.crafter_disabled(pos);
        self.read(Some(pos), K_REDSTONE, (3_u8, value));
        self.guarded_read(ReadMethod::Redstone(Some(pos), 3), Some(pos), (3_u8, value));
        value
    }

    fn set_crafter_disabled(&mut self, pos: BlockPos, mask: u16) {
        self.redstone_write(Some(pos), 3, (3_u8, mask));
        self.level.set_crafter_disabled(pos, mask);
    }

    fn set_crafter_crafting(&mut self, pos: BlockPos, remaining: i32, now: i64) {
        self.redstone_write(Some(pos), 4, (4_u8, remaining));
        self.level.set_crafter_crafting(pos, remaining, now);
    }

    fn take_crafter_crafting(&mut self, now: i64) -> Vec<(BlockPos, i32)> {
        let value = self.level.take_crafter_crafting(now);
        self.read(None, K_REDSTONE, (5_u8, &value));
        value
    }

    fn restore_crafter_crafting(&mut self, entries: Vec<(BlockPos, i32)>, now: i64) {
        self.redstone_write(None, 5, (5_u8, &entries));
        self.level.restore_crafter_crafting(entries, now);
    }

    fn take_due_crafter_crafting(&mut self, now: i64) -> Vec<BlockPos> {
        let value = self.level.take_due_crafter_crafting(now);
        self.read(None, K_REDSTONE, (7_u8, &value));
        value
    }

    fn forget_block_entity_memory(&mut self, pos: BlockPos) {
        self.redstone_write(Some(pos), 6, 6_u8);
        self.level.forget_block_entity_memory(pos);
    }

    fn set_pending_lectern(&mut self, facts: LecternFacts) {
        let value = (facts.page, facts.page_count, facts.has_content);
        self.redstone_write(None, 7, (7_u8, value));
        self.level.set_pending_lectern(facts);
    }

    fn take_pending_lectern(&mut self) -> Option<LecternFacts> {
        let value = self.level.take_pending_lectern();
        self.read(
            None,
            K_REDSTONE,
            (8_u8, value.map(|f| (f.page, f.page_count, f.has_content))),
        );
        value
    }

    fn set_lectern(&mut self, pos: BlockPos, facts: LecternFacts) {
        let value = (facts.page, facts.page_count, facts.has_content);
        self.redstone_write(Some(pos), 8, (8_u8, value));
        self.level.set_lectern(pos, facts);
    }

    fn lectern(&self, pos: BlockPos) -> Option<LecternFacts> {
        let value = self.level.lectern(pos);
        self.read(
            Some(pos),
            K_REDSTONE,
            (9_u8, value.map(|f| (f.page, f.page_count, f.has_content))),
        );
        value
    }

    fn clear_lectern(&mut self, pos: BlockPos) {
        self.redstone_write(Some(pos), 9, 9_u8);
        self.level.clear_lectern(pos);
    }

    fn forget_torch_toggles_before(&mut self, when: i64) {
        self.redstone_write(None, 10, (10_u8, self.relative_time(when)));
        self.level.forget_torch_toggles_before(when);
    }

    fn record_torch_toggle(&mut self, pos: BlockPos, when: i64) {
        self.redstone_write(Some(pos), 10, (10_u8, self.relative_time(when)));
        self.level.record_torch_toggle(pos, when);
    }

    fn torch_toggle_count(&self, pos: BlockPos) -> usize {
        let value = self.level.torch_toggle_count(pos);
        self.read(Some(pos), K_REDSTONE, (11_u8, value));
        self.guarded_read(
            ReadMethod::Redstone(Some(pos), 11),
            Some(pos),
            (11_u8, value),
        );
        value
    }

    fn push_block_event(&mut self, event: BlockEvent) {
        self.write(Write::BlockEvent {
            event,
            popped: None,
        });
        self.runtime_effect(ReplayEffect::BlockEvent(event));
        self.level.push_block_event(event);
    }

    fn pop_block_event(&mut self) -> Option<BlockEvent> {
        // 驱动循环取事件是实现细节；取到的内容进读集合，不进效果。
        let value = self.level.pop_block_event();
        self.read(None, K_BLOCK_EVENT, value);
        value
    }

    fn reschedule_block_events(&mut self, events: Vec<BlockEvent>) {
        // 一轮结束后被推迟的事件按原顺序放回；录制器只转发，和 push 分开记。
        self.level.reschedule_block_events(events);
    }

    fn block_entity_dyn(&self, pos: BlockPos) -> Option<&dyn BlockEntity> {
        let value = self.level.block_entity_dyn(pos);
        self.read_entity(pos, value, 2_u8);
        value
    }

    fn block_entity_sleep_enabled(&self) -> bool {
        self.level.block_entity_sleep_enabled()
    }

    fn block_entity_sleep_from(&mut self, pos: BlockPos, now: i64) -> Option<i64> {
        self.level.block_entity_sleep_from(pos, now)
    }

    fn has_block_entity(&self, pos: BlockPos) -> bool {
        let value = self.level.has_block_entity(pos);
        self.read(Some(pos), K_BLOCK_ENTITY, (0_u8, value));
        self.guarded_read(ReadMethod::BlockEntity(pos, 0, 0), Some(pos), (0_u8, value));
        value
    }

    fn insert_block_entity(&mut self, entity: Box<dyn BlockEntity>) {
        let pos = entity.pos();
        let facts = entity_facts(entity.as_ref());
        // 读的是刚要写入的实体本身，是「果」不是「因」，不作守卫。
        self.read_entity_driver(pos, Some(entity.as_ref()), 6_u8);
        // 运行时只支持 `PistonMoving`：其余实体类型没有按值还原的路径，
        // 见到就作废整条记录，别只放宽白名单。
        if let Some(piston) = entity.as_any().downcast_ref::<PistonMoving>() {
            let (moved, direction, extending, source, progress, progress_o) =
                piston.observed_facts();
            self.runtime_effect(ReplayEffect::PistonMoving {
                pos,
                moved_state: BlockState(moved),
                direction,
                extending,
                source,
                progress,
                progress_o,
            });
        } else {
            self.reject_runtime();
        }
        self.write(Write::BlockEntity {
            pos,
            kind: 0,
            value: facts,
        });
        self.level.insert_block_entity(entity);
    }

    fn remove_block_entity(&mut self, pos: BlockPos) -> Option<Box<dyn BlockEntity>> {
        let value = self.level.remove_block_entity(pos);
        self.read_entity(pos, value.as_deref(), 7_u8);
        self.runtime_effect(ReplayEffect::RemoveBlockEntity { pos });
        self.write(Write::BlockEntity {
            pos,
            kind: 1,
            value: stable_hash(&value.as_ref().map(|entity| entity_facts(entity.as_ref()))),
        });
        value
    }

    fn take_block_entity_at(&mut self, pos: BlockPos) -> Option<(u64, Box<dyn BlockEntity>)> {
        // 驱动把实体临时拿出去不是「效果」，只是实现细节；记一笔读就够了。
        let value = self.level.take_block_entity_at(pos);
        // ticker 的内部 id 是递增的实现细节，不进读集合。
        let entity = value.as_ref().map(|(_, entity)| entity.as_ref());
        self.read_entity(pos, entity, 3_u8);
        value
    }

    fn take_block_entity(&mut self, pos: BlockPos, id: u64) -> Option<Box<dyn BlockEntity>> {
        let value = self.level.take_block_entity(pos, id);
        let _ = id;
        self.read_entity(pos, value.as_deref(), 4_u8);
        value
    }

    fn put_block_entity(&mut self, pos: BlockPos, id: u64, entity: Box<dyn BlockEntity>) {
        // 放回去也不是「效果」：实体改了什么由这次触发里别的读写记录。
        self.read_entity_driver(pos, Some(entity.as_ref()), 5_u8);
        self.level.put_block_entity(pos, id, entity);
    }

    fn is_block_entity_taken(&self, pos: BlockPos) -> bool {
        let value = self.level.is_block_entity_taken(pos);
        self.read(Some(pos), K_BLOCK_ENTITY, (1_u8, value));
        self.guarded_read(ReadMethod::BlockEntity(pos, 1, 1), Some(pos), (1_u8, value));
        value
    }

    fn wake_block_entity(&mut self, pos: BlockPos) {
        self.level.wake_block_entity(pos);
    }

    fn schedule_block_entity_wake(&mut self, pos: BlockPos, wake_at: Option<i64>) {
        self.level.schedule_block_entity_wake(pos, wake_at);
    }

    fn set_chunk_block_entity_ticking(&mut self, chunk: ChunkPos, ticks: bool) {
        self.level.set_chunk_block_entity_ticking(chunk, ticks);
    }

    fn begin_block_entity_tick(&mut self, now: i64) {
        self.level.begin_block_entity_tick(now);
    }

    fn next_block_entity_ticker(&mut self) -> Option<(BlockPos, u64)> {
        // 只记位置：ticker 的内部 id 递增，是实现细节，不进读集合。
        let value = self.level.next_block_entity_ticker();
        self.read(None, K_TICKER, value.map(|(pos, _)| pos));
        // 队列推进有副作用，守卫阶段不能重查；但这一条触发本身就是刚取出的 ticker，
        // 所以守卫只需确认「记录里的位置 == 当前触发的位置」，见 guard_value。
        if let Some((pos, _)) = value {
            self.guarded_read(ReadMethod::Ticker(0, pos), Some(pos), pos);
        }
        value
    }

    fn end_block_entity_tick(&mut self) {
        self.level.end_block_entity_tick();
    }
}

/// 录制包装直接实现实体窄接口（不经过 `EntityTables`）：碰到实体就标记不可缓存，
/// 每个方法都原样转发，返回值和副作用不变。
impl LevelEntities for Recording<'_> {
    fn entity_count(&self) -> usize {
        self.note_entities(0);
        let value = self.level.entity_count();
        self.read(None, K_ENTITIES, (0_u8, value));
        value
    }

    fn entity_uids(&self) -> Vec<EntityUid> {
        self.note_entities(1);
        let value = self.level.entity_uids();
        self.read(None, K_ENTITIES, (1_u8, &value));
        value
    }

    fn entity(&self, uid: EntityUid) -> Option<Entity> {
        self.note_entities(2);
        let value = self.level.entity(uid);
        self.read(None, K_ENTITIES, (2_u8, uid, value.is_some()));
        value
    }

    fn entity_public(&self, uid: EntityUid) -> Option<EntityPublic> {
        self.note_entities(3);
        let value = self.level.entity_public(uid);
        self.read(None, K_ENTITIES, (3_u8, uid, value.is_some()));
        value
    }

    fn for_each_entity_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic)) {
        self.note_entities(4);
        self.read(
            None,
            K_ENTITIES,
            (4_u8, area.min_x.to_bits(), area.max_z.to_bits()),
        );
        self.level.for_each_entity_in(area, visit);
    }

    fn spawn_entity(&mut self, entity: Entity) -> EntityUid {
        self.note_entities(5);
        let uid = self.level.spawn_entity(entity);
        self.read(None, K_ENTITIES, (5_u8, uid));
        uid
    }

    fn with_just_spawned(&mut self, uid: EntityUid, f: &mut dyn FnMut(&mut Entity)) -> bool {
        self.note_entities(6);
        let value = self.level.with_just_spawned(uid, f);
        self.read(None, K_ENTITIES, (6_u8, uid, value));
        value
    }

    fn discard_entity(&mut self, uid: EntityUid) -> bool {
        self.note_entities(7);
        let value = self.level.discard_entity(uid);
        self.write(Write::Effect(stable_hash(&(7_u8, uid, value))));
        value
    }

    fn remove_entity(&mut self, uid: EntityUid) {
        self.note_entities(8);
        self.write(Write::Effect(stable_hash(&(8_u8, uid))));
        self.level.remove_entity(uid);
    }

    fn begin_entity_tick(&mut self) {
        self.level.begin_entity_tick();
    }

    fn end_entity_tick(&mut self) {
        self.level.end_entity_tick();
    }

    fn take_entity(&mut self, uid: EntityUid) -> Option<Entity> {
        self.note_entities(9);
        let value = self.level.take_entity(uid);
        self.read(None, K_ENTITIES, (9_u8, uid, value.is_some()));
        value
    }

    fn put_entity(&mut self, entity: Entity) {
        self.note_entities(10);
        self.level.put_entity(entity);
    }

    fn live_entities(&self) -> Vec<Entity> {
        self.note_entities(11);
        let value = self.level.live_entities();
        self.read(None, K_ENTITIES, (11_u8, value.len()));
        value
    }

    // 下面三个是给并行驱动用的整表入口。录制层不细分，碰了就算“碰了实体”。
    fn entities_table(&self) -> &EntityStore {
        self.note_entities(12);
        self.level.entities_table()
    }

    fn entities_table_mut(&mut self) -> &mut EntityStore {
        self.note_entities(13);
        self.level.entities_table_mut()
    }

    fn publish_entity_rows(
        &mut self,
        rows: &[usize],
        public: Vec<EntityPublic>,
        state: Vec<EntityState>,
    ) {
        self.note_entities(14);
        self.level.publish_entity_rows(rows, public, state);
    }
}

impl Level for Recording<'_> {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        let value = self.level.block_state(pos);
        self.read(Some(pos), K_BLOCK, value.0);
        value
    }

    fn write_block(&mut self, pos: BlockPos, state: BlockState, flags: u32) -> bool {
        let old = self.level.block_state(pos);
        let applied = self.level.write_block(pos, state, flags);
        let new = self.level.block_state(pos);
        self.note_block_write(pos, old, new);
        self.write(Write::Block {
            pos,
            state,
            flags,
            applied,
        });
        self.runtime_effect(ReplayEffect::Block {
            pos,
            state,
            flags,
            applied,
        });
        applied
    }

    fn is_unobstructed(&self, boxes: &[Aabb]) -> bool {
        let value = self.level.is_unobstructed(boxes);
        let pos = boxes.first().map(|box_| {
            BlockPos::new(
                box_.min_x.floor() as i32,
                box_.min_y.floor() as i32,
                box_.min_z.floor() as i32,
            )
        });
        self.read(pos, K_UNOBSTRUCTED, (value, boxes.len()));
        value
    }

    fn min_y(&self) -> i32 {
        let value = self.level.min_y();
        self.read(None, K_MIN_Y, value);
        self.guarded_read(ReadMethod::MinY, None, value);
        value
    }

    fn max_y(&self) -> i32 {
        let value = self.level.max_y();
        self.read(None, K_MAX_Y, value);
        self.guarded_read(ReadMethod::MaxY, None, value);
        value
    }

    fn random(&self) -> &LevelRandom {
        self.uncacheable(Uncacheable::Random);
        self.read(None, K_RANDOM, self.level.random().source().seed());
        self.level.random()
    }

    fn next_random_float(&self) -> f32 {
        // 活塞音高取数由 `Observation::PistonPitch` 记成可重放的效果。
        self.level.next_random_float()
    }

    fn section_hash(&self, chunk: ChunkPos, section_y: i32) -> Option<u128> {
        // 守卫读的是录制器自己维护的整段哈希：它按 `write_block` 增量更新，
        // 和存储层给的初始哈希一致。底层 `Level` 不一定实现 `section_hash`。
        self.inner
            .borrow()
            .section_hashes
            .get(&ChunkSection { chunk, section_y })
            .copied()
            .or_else(|| self.level.section_hash(chunk, section_y))
    }

    fn is_randomly_ticking_section(&self, chunk: ChunkPos, section_y: i32) -> bool {
        let value = self.level.is_randomly_ticking_section(chunk, section_y);
        self.read(None, K_SECTION_TICKING, (chunk, section_y, value));
        value
    }

    fn randomly_ticking_sections(&self, chunk: ChunkPos) -> u32 {
        // 整个区块的答案记成一条读取，不要退回默认实现逐段各记一条。
        let value = self.level.randomly_ticking_sections(chunk);
        self.read(None, K_SECTION_TICKING, (chunk, value));
        value
    }

    fn any_player_close_enough_to(&self, pos: BlockPos, max_distance: i32) -> bool {
        let value = self.level.any_player_close_enough_to(pos, max_distance);
        self.read(Some(pos), K_PLAYER_NEAR, (max_distance, value));
        value
    }

    fn brightness(&self, layer: LightLayer, pos: BlockPos) -> i32 {
        let value = self.level.brightness(layer, pos);
        self.read(Some(pos), K_BRIGHTNESS, (layer, value));
        value
    }

    fn sign(&self, pos: BlockPos) -> Option<SignContent> {
        let value = self.level.sign(pos);
        self.read(Some(pos), K_SIGN, value.as_ref().map(hash_sign));
        value
    }

    fn set_sign(&mut self, pos: BlockPos, content: SignContent) -> bool {
        let value = hash_sign(&content);
        let applied = self.level.set_sign(pos, content);
        self.write(Write::Sign {
            pos,
            value,
            applied,
        });
        applied
    }

    fn note_block_sound(&self, pos: BlockPos) -> Option<String> {
        let value = self.level.note_block_sound(pos);
        self.read(Some(pos), K_NOTE_SOUND, &value);
        value
    }

    fn chunk_ticks_blocks(&self, chunk: ChunkPos) -> bool {
        self.level.chunk_ticks_blocks(chunk)
    }

    fn chunk_ticks_entities(&self, chunk: ChunkPos) -> bool {
        self.level.chunk_ticks_entities(chunk)
    }

    fn runs_block_entity_tick(&self, pos: BlockPos) -> bool {
        let value = self.level.runs_block_entity_tick(pos);
        self.read(Some(pos), K_TICKER, (2_u8, value));
        self.guarded_read(ReadMethod::Ticker(1, pos), Some(pos), (2_u8, value));
        value
    }

    fn block_game_rules(&self) -> BlockGameRules {
        let value = self.level.block_game_rules();
        self.read(
            None,
            K_GAME_RULES,
            (
                value.random_tick_speed,
                value.fire_spread_radius_around_player,
                value.tnt_explodes,
                value.block_drops,
                value.entity_drops,
                value.water_source_conversion,
                value.lava_source_conversion,
                value.spread_vines,
                value.max_block_modifications,
                value.max_snow_accumulation_height,
                value.spawn_mobs,
            ),
        );
        value
    }

    fn block_weather(&self) -> BlockWeather {
        let value = self.level.block_weather();
        self.read(
            None,
            K_WEATHER,
            (value.rain_level.to_bits(), value.thunder_level.to_bits()),
        );
        value
    }

    fn world_border(&self) -> crate::WorldBorder {
        let value = self.level.world_border();
        let facts = (
            value.min_x().to_bits(),
            value.max_x().to_bits(),
            value.min_z().to_bits(),
            value.max_z().to_bits(),
            value.size().to_bits(),
            value.lerp_time(),
            value.lerp_target().to_bits(),
            value.safe_zone().to_bits(),
            value.damage_per_block().to_bits(),
            value.warning_time(),
            value.warning_blocks(),
            value.absolute_max_size(),
        );
        self.read(None, K_WORLD_BORDER, facts);
        self.guarded_read(ReadMethod::WorldBorder, None, facts);
        value
    }

    fn can_see_sky(&self, pos: BlockPos) -> bool {
        let value = self.level.can_see_sky(pos);
        self.read(Some(pos), K_SKY, value);
        value
    }

    fn heightmap_y(&self, pos: BlockPos) -> i32 {
        let value = self.level.heightmap_y(pos);
        self.read(Some(pos), K_HEIGHTMAP, value);
        value
    }

    fn precipitation_facts(&self, pos: BlockPos) -> PrecipitationFacts {
        let value = self.level.precipitation_facts(pos);
        self.read(
            Some(pos),
            K_PRECIPITATION,
            (value.precipitation as u8, value.warm_enough_to_rain),
        );
        value
    }

    fn with_container(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut dyn Container)) -> bool {
        let before = self.level.slots_at(pos);
        let applied = self.level.with_container(pos, f);
        let after = self.level.slots_at(pos);
        self.read(Some(pos), K_CONTAINER, (hash_slots(&before), applied));
        self.reject_runtime(); // with_container 的 before/applied 守卫无法无副作用重算
        if applied {
            self.runtime_effect(ReplayEffect::ContainerSlots {
                pos,
                slots: after.clone(),
            });
        }
        self.write(Write::Container {
            pos,
            kind: 0,
            value: stable_hash(&(hash_slots(&after), applied)),
        });
        applied
    }

    fn own_container_counts(&self, pos: BlockPos) -> Vec<i32> {
        let value = self.level.own_container_counts(pos);
        self.read(Some(pos), K_OWN_COUNTS, &value);
        value
    }

    fn container_signal(&self, pos: BlockPos) -> i32 {
        let value = self.level.container_signal(pos);
        self.read(
            Some(pos),
            K_CONTAINER_SIGNAL,
            (value, hash_slots(&self.level.slots_at(pos))),
        );
        value
    }

    fn container_positions(&self, pos: BlockPos) -> Vec<BlockPos> {
        let value = self.level.container_positions(pos);
        self.read(Some(pos), K_CONTAINER_POSITIONS, &value);
        value
    }

    fn with_hopper(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut HopperEntity)) -> bool {
        let before = self.level.block_entity_dyn(pos).map(entity_facts);
        let applied = self.level.with_hopper(pos, f);
        let after = self.level.block_entity_dyn(pos).map(entity_facts);
        self.read(Some(pos), K_HOPPER, (before, applied));
        self.write(Write::BlockEntity {
            pos,
            kind: 5,
            value: stable_hash(&(after, applied)),
        });
        applied
    }

    fn with_slots(
        &mut self,
        pos: BlockPos,
        len: usize,
        f: &mut dyn FnMut(&mut [ItemStack]),
    ) -> bool {
        let before = self.level.slots_at(pos);
        let applied = self.level.with_slots(pos, len, f);
        let after = self.level.slots_at(pos);
        self.read(Some(pos), K_SLOTS, (len, hash_slots(&before), applied));
        self.guarded_read(
            ReadMethod::Slots(pos, 1, len, applied),
            Some(pos),
            (len, hash_slots(&before), applied),
        );
        if applied {
            self.runtime_effect(ReplayEffect::ContainerSlots {
                pos,
                slots: after.clone(),
            });
        }
        self.write(Write::Container {
            pos,
            kind: 1,
            value: stable_hash(&(hash_slots(&after), applied)),
        });
        applied
    }

    fn slots_at(&self, pos: BlockPos) -> Vec<ItemStack> {
        let value = self.level.slots_at(pos);
        self.read(Some(pos), K_SLOTS, (0_u8, hash_slots(&value)));
        self.guarded_read(
            ReadMethod::Slots(pos, 0, 0, true),
            Some(pos),
            (0_u8, hash_slots(&value)),
        );
        value
    }

    fn clear_container(&mut self, pos: BlockPos) {
        self.runtime_effect(ReplayEffect::ClearContainer { pos });
        self.write(Write::Container {
            pos,
            kind: 2,
            value: 0,
        });
        self.level.clear_container(pos);
    }

    fn explosion_block_drops(&mut self, state: BlockState, pos: BlockPos) -> Vec<ItemStack> {
        let value = self.level.explosion_block_drops(state, pos);
        self.read(Some(pos), K_DROPS, (0_u8, state.0, hash_slots(&value)));
        self.uncacheable(Uncacheable::Random);
        value
    }

    fn block_loot_drops(&mut self, state: BlockState, pos: BlockPos) -> Vec<ItemStack> {
        let value = self.level.block_loot_drops(state, pos);
        self.read(Some(pos), K_DROPS, (1_u8, state.0, hash_slots(&value)));
        self.uncacheable(Uncacheable::Random);
        value
    }

    fn unpack_loot_table(&mut self, pos: BlockPos) {
        // 实验 `Record` 的读项恒为 0（历史命中率数字不变）；运行时守卫另用
        // 只读的 `loot_table_pending` 重算「这一格还有没有待解包的表」。
        let pending = self.level.loot_table_pending(pos);
        self.read(Some(pos), K_LOOT_UNPACK, 0_u8);
        self.guarded_read(ReadMethod::LootUnpack(pos), Some(pos), pending);
        self.runtime_effect(ReplayEffect::UnpackLootTable { pos });
        self.level.unpack_loot_table(pos);
    }

    fn loot_table_pending(&self, pos: BlockPos) -> bool {
        self.level.loot_table_pending(pos)
    }

    fn creative_container_drop(&mut self, pos: BlockPos, item: i32) -> Option<ItemStack> {
        let value = self.level.creative_container_drop(pos, item);
        self.read(
            Some(pos),
            K_DROPS,
            (
                2_u8,
                item,
                value
                    .as_ref()
                    .map(|stack| hash_slots(std::slice::from_ref(stack))),
            ),
        );
        value
    }

    fn spawn_after_break_random(&mut self, state: BlockState, pos: BlockPos) {
        let _ = (state, pos);
        self.uncacheable(Uncacheable::Random);
        self.level.spawn_after_break_random(state, pos);
    }

    fn dispenser_targets(&self) -> Vec<EquipTarget> {
        let value = self.level.dispenser_targets();
        self.read(None, K_TARGETS, value.len());
        self.uncacheable(Uncacheable::Entities);
        value
    }

    fn set_dispenser_equipment(
        &mut self,
        entity_id: i32,
        component_slot: i32,
        stack: ItemStack,
    ) -> Option<u16> {
        let key = hash_slots(std::slice::from_ref(&stack));
        let value = self
            .level
            .set_dispenser_equipment(entity_id, component_slot, stack);
        self.read(None, K_EQUIP, (entity_id, component_slot, key, value));
        self.uncacheable(Uncacheable::Entities);
        value
    }

    fn runtime_records(&self) -> Vec<RuntimeRecord> {
        self.take_runtime_records()
    }

    fn take_counted_reads(&self) -> u64 {
        self.counted.replace(0)
    }

    fn observe(&self, event: Observation<'_>) {
        if let Observation::RecordMode(mode) = event {
            self.set_record_mode(mode);
            return;
        }
        // `Off` / `Count` 模式下连触发帧都不建：这是不缓存的触发点最贵的一块开销。
        if self.mode.get() != crate::level::RecordMode::Full {
            return;
        }
        match event {
            Observation::TriggerBegin(trigger) => {
                let mut inner = self.inner.borrow_mut();
                if let Some(frame) = inner.current.take() {
                    let (record, runtime) = finish_record(frame);
                    inner.records.push(record);
                    inner.runtime.push(runtime);
                }
                let record = Record {
                    tick: inner.tick,
                    trigger,
                    reads: std::mem::take(&mut inner.pending_reads),
                    writes: std::mem::take(&mut inner.pending_writes),
                    uncacheable: std::mem::take(&mut inner.pending_uncacheable),
                    sections: Vec::new(),
                    nanos: 0,
                };
                // 驱动循环在两次边界之间读到的位置可能属于还没扫过的段。
                let sections: Vec<ChunkSection> = record
                    .reads
                    .iter()
                    .filter_map(|read| read.pos)
                    .map(ChunkSection::of)
                    .collect();
                inner.current = Some(Frame {
                    runtime: RuntimeFrame {
                        trigger: Some(trigger),
                        trigger_state: self.level.block_state(trigger_pos(trigger)).0,
                        tick: inner.tick,
                        boundary_writes: inner.boundary_writes,
                        ..RuntimeFrame::default()
                    },
                    record,
                    started: Instant::now(),
                    section_start: HashMap::new(),
                });
                // 驱动循环在边界外读到的守卫项归到这一次触发（和实验的 pending_reads 一样）。
                let pending = std::mem::take(&mut inner.pending_runtime_reads);
                let pending_rejected = std::mem::take(&mut inner.pending_rejected);
                if let Some(frame) = inner.current.as_mut() {
                    frame.runtime.reads.extend(pending);
                    frame.runtime.rejected |= pending_rejected;
                }
                drop(inner);
                for section in sections {
                    self.ensure_section(section);
                }
                // 只读触发开始时的段内容，后面新读到别的段也不改快照。
                let mut section_start = self.inner.borrow().section_hashes.clone();
                let used: HashSet<ChunkSection> = self
                    .inner
                    .borrow()
                    .current
                    .as_ref()
                    .expect("刚放进来的触发")
                    .record
                    .reads
                    .iter()
                    .filter_map(|read| read.pos)
                    .map(ChunkSection::of)
                    .collect();
                section_start.retain(|section, _| used.contains(section));
                self.inner
                    .borrow_mut()
                    .current
                    .as_mut()
                    .expect("刚放进来的触发")
                    .section_start = section_start;
            }
            Observation::TriggerEnd => {
                let mut inner = self.inner.borrow_mut();
                if let Some(frame) = inner.current.take() {
                    let (record, runtime) = finish_record(frame);
                    inner.records.push(record);
                    inner.runtime.push(runtime);
                }
                inner.boundary_writes = false;
                inner.pending_runtime_reads.clear();
                inner.pending_rejected = false;
            }
            Observation::TimeRead => {
                self.uncacheable(Uncacheable::Time);
                self.read(None, K_TIME, 0_u8);
            }
            Observation::Effect(effect) => {
                // 方块事件成功的包是确定的，记成可重放的效果；其余对外效果
                // 不能重放，见到就把这条运行时记录作废。
                if let Effect::BlockEvent {
                    pos,
                    block,
                    param_a,
                    param_b,
                } = effect
                {
                    self.runtime_effect(ReplayEffect::BlockEventPacket {
                        pos: *pos,
                        block: *block,
                        param_a: *param_a,
                        param_b: *param_b,
                    });
                } else if !matches!(effect, Effect::Sound { seed: None, .. }) {
                    // 其余对外效果一律作废这条记录。
                    self.reject_runtime();
                } else if !self.full_only() {
                    // 守卫/重放阶段不记账，不该走到这里。
                } else {
                    // `seed: None` 的音效只有被 `PistonPitch` 记成可重放动作才安全。
                    let mut inner = self.inner.borrow_mut();
                    if let Some(frame) = inner.current.as_mut()
                        && !frame.runtime.piston_pitch_seen
                    {
                        frame.runtime.rejected = true;
                    }
                }
                // 活塞伸出/收回的音高和音符盒的 seed 由世界随机源决定，
                // 不是「状态」；效果里只留音效的种类和位置，避免同一键因
                // 随机数不同而被算成「效果不同」。触发本身已标为不可缓存。
                let value = match effect {
                    Effect::Sound {
                        sound,
                        source,
                        x,
                        y,
                        z,
                        volume,
                        except_initiator,
                        ..
                    } => stable_hash(&(
                        9_u8,
                        *sound,
                        source.id(),
                        x.to_bits(),
                        y.to_bits(),
                        z.to_bits(),
                        volume.to_bits(),
                        *except_initiator,
                    )),
                    Effect::NoteBlockPlayed { .. } => stable_hash(&11_u8),
                    _ => hash_effect(effect),
                };
                self.write(Write::Effect(value));
            }
            Observation::TickQuery {
                pos,
                kind,
                will_tick_this_tick,
                result,
            } => {
                self.read(Some(pos), K_TICK_QUERY, (kind, will_tick_this_tick, result));
                self.guarded_read(
                    ReadMethod::TickQuery(pos, kind, will_tick_this_tick),
                    Some(pos),
                    (kind, will_tick_this_tick, result),
                );
            }
            Observation::TickScheduled {
                pos,
                kind,
                fluid,
                delay,
                priority,
                existed,
            } => {
                // `existed` 为真的写入在重放时仍走原始入口，去重和内部序号自然发生；
                // 这里只把它记成守卫读项，不因此作废记录。
                self.runtime_read(ReadMethod::Dedup(kind, fluid), Some(pos), existed);
                if fluid {
                    if let Ok(fluid_id) = i32::try_from(kind) {
                        self.runtime_effect(ReplayEffect::ScheduledFluidTick {
                            pos,
                            fluid: fluid_id,
                            delay,
                        });
                    }
                } else if let Ok(block) = usize::try_from(kind) {
                    self.runtime_effect(ReplayEffect::ScheduledTick {
                        pos,
                        block,
                        delay,
                        priority,
                    });
                }
                self.write(Write::TickScheduled {
                    pos,
                    kind,
                    fluid,
                    delay,
                    priority,
                });
            }
            Observation::BlockEventQueued { event, existed } => {
                // 真正的 push 走 `LevelSideState::push_block_event`，这里只记去重存在性。
                self.runtime_read(ReadMethod::BlockEventDedup(event), Some(event.pos), existed);
            }
            Observation::PistonPitch {
                pos,
                sound,
                source,
                volume,
                scale,
                base,
                except_initiator,
            } => {
                // 实验的严格口径仍把「取随机数」标成不可缓存（历史数字不变）；
                // 运行时把这次取数记成可重放的动作，重放时照样从世界随机源取一个。
                let mut inner = self.inner.borrow_mut();
                if let Some(frame) = inner.current.as_mut() {
                    frame.runtime.piston_pitch_seen = true;
                    if !frame.record.uncacheable.contains(&Uncacheable::Random) {
                        frame.record.uncacheable.push(Uncacheable::Random);
                    }
                }
                drop(inner);
                self.runtime_effect(ReplayEffect::PistonSound {
                    pos,
                    sound,
                    source: source.id(),
                    volume: volume.to_bits(),
                    scale: scale.to_bits(),
                    base: base.to_bits(),
                    except_initiator,
                });
                self.write(Write::Effect(stable_hash(&(
                    9_u8,
                    sound,
                    source.id(),
                    volume.to_bits(),
                    scale.to_bits(),
                    base.to_bits(),
                    except_initiator,
                ))));
            }
            Observation::NoteBlockEntityTick { pos } => {
                self.runtime_effect(ReplayEffect::NoteBlockEntityTick { pos });
            }
            Observation::PistonTickState {
                pos,
                moved_state,
                direction,
                extending,
                source,
                progress,
                progress_o,
            } => {
                // 只供缓存重放用：实验的 `Record` 不加读写项，历史数字不变。
                self.runtime_effect(ReplayEffect::RestorePistonMoving {
                    pos,
                    moved_state: BlockState(moved_state),
                    direction,
                    extending,
                    source,
                    progress,
                    progress_o,
                });
            }
            Observation::RecordMode(_) => {}
        }
    }
}

fn finish_record(mut frame: Frame) -> (Record, RuntimeRecord) {
    let nanos = u64::try_from(frame.started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    frame.record.nanos = nanos;
    let used: HashSet<ChunkSection> = frame
        .record
        .reads
        .iter()
        .filter_map(|read| read.pos)
        .map(ChunkSection::of)
        .collect();
    frame.record.sections = frame
        .section_start
        .iter()
        .filter(|(section, _)| used.contains(section))
        .map(|(&section, &hash)| (section, hash))
        .collect();
    frame.record.sections.sort_by_key(|(section, _)| *section);
    let mut sections: Vec<SectionKey> = frame
        .record
        .sections
        .iter()
        .map(|&(section, hash)| SectionKey { section, hash })
        .collect();
    sections.sort_by_key(|s| s.section);
    let trigger = frame.runtime.trigger.unwrap_or(frame.record.trigger);
    // 触发本身这一格的方块状态进守卫，确认它还是记录里那个方块。
    let mut reads = frame.runtime.reads;
    // 触发点的方块状态：录制时记下原始状态 ID，守卫在 World 层重查。
    reads.insert(
        0,
        GuardRead::trigger(trigger_pos(trigger), frame.runtime.trigger_state),
    );
    // 守卫读项必须覆盖每一个白名单里的实验读项：漏一个读就可能误命中。
    // `guarded_reads` 是录制时逐项记下的「这条实验读项有守卫」计数；
    // ticker 驱动的临时拿放实体（设计稿 §3.1）和 K_BLOCK / K_BLOCK_EVENT
    // 由段哈希、Trigger 覆盖，不产生守卫项，也不计入这里。
    // 另有触发本身，以及每次排刻/排事件的去重存在性。
    let expected_guards = frame.runtime.guarded_reads
        + reads
            .iter()
            .filter(|read| {
                matches!(
                    read.method,
                    ReadMethod::Dedup(_, _) | ReadMethod::BlockEventDedup(_)
                )
            })
            .count();
    // 方块实体 tick 的重放必须按 `RestorePistonMoving` 还原 tick 后的实体状态；
    // 没有这条效果（非移动活塞、或者实体被删了）就退回解释，别放它进缓存。
    let piston_tick_replayable = !matches!(trigger, Trigger::BlockEntityTick { .. })
        || frame
            .runtime
            .effects
            .iter()
            .any(|effect| matches!(effect, ReplayEffect::RestorePistonMoving { .. }));
    let rejected = frame.runtime.rejected
        || !piston_tick_replayable
        || reads.len().saturating_sub(1) != expected_guards
        || frame
            .record
            .reads
            .iter()
            .any(|read| !crate::cache::supported_kind(read.kind))
        || frame
            .record
            .writes
            .iter()
            .any(|write| !crate::cache::supported_write(*write));
    let runtime = RuntimeRecord {
        trigger,
        tick: frame.runtime.tick,
        nanos,
        interpreted_items: frame.record.reads.len() as u64,
        written_items: frame.record.writes.len() as u64,
        reads,
        effects: frame.runtime.effects,
        sections,
        uncacheable: frame.runtime.uncacheable.clone(),
        boundary_writes: frame.runtime.boundary_writes,
        rejected,
    };
    (frame.record, runtime)
}

const _: fn() = || {
    fn assert_level<T: Level + LevelSideState + LevelEntities>() {}
    assert_level::<Recording<'static>>();
};

/// 一组记录的命中统计，精确键和段哈希键各一份，严格口径和乐观口径各一份。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Hits {
    pub hits: usize,
    pub first_seen: usize,
    /// 键相同但效果不同的次数。非零说明录制漏了读。
    pub mismatched_effects: usize,
    /// 命中那些触发的耗时之和，单位纳秒。
    pub saved_nanos: u128,
    /// 不同键的数量，也就是要存多少条记录。
    pub records: usize,
    /// 触发数检查点上的不同键数（触发数, 精确, 段哈希）。
    pub growth: Vec<(usize, usize, usize)>,
}

/// 一次离线分析的结果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HitRates {
    pub total_nanos: u128,
    pub exact_strict: Hits,
    pub exact_optimistic: Hits,
    /// 混合段哈希键（精确读集合 + 段哈希），严格 / 乐观。
    pub section_strict: Hits,
    pub section_optimistic: Hits,
    /// 纯段哈希键（9.3 第一版口径），严格 / 乐观。
    pub pure_strict: Hits,
    pub pure_optimistic: Hits,
}

impl HitRates {
    /// 按「触发数」列检查点，四条曲线对齐。
    pub fn growth_points(&self) -> Vec<(usize, usize, usize)> {
        self.exact_strict.growth.clone()
    }
}

/// 离线算命中率。`growth_step` 是记录数曲线的采样间隔。
///
/// 命中 = 这个键在这次运行里之前出现过，而且那次的效果和这次相同。
pub fn hit_rates(records: &[Record], growth_step: usize) -> HitRates {
    let mut out = HitRates::default();
    let mut exact_strict: HashMap<u128, u128> = HashMap::new();
    let mut exact_optimistic: HashMap<u128, u128> = HashMap::new();
    let mut section_strict: HashMap<u128, u128> = HashMap::new();
    let mut section_optimistic: HashMap<u128, u128> = HashMap::new();
    let mut pure_strict: HashMap<u128, u128> = HashMap::new();
    let mut pure_optimistic: HashMap<u128, u128> = HashMap::new();
    for (index, record) in records.iter().enumerate() {
        let effect = record.effect_key();
        let nanos = u128::from(record.nanos);
        out.total_nanos += nanos;
        let strict = record.cacheable(true);
        let optimistic = record.cacheable(false);
        let count_hit =
            |map: &mut HashMap<u128, u128>, key: u128, hits: &mut Hits, nanos: u128| match map
                .get(&key)
            {
                Some(&previous) => {
                    if previous != effect {
                        hits.mismatched_effects += 1;
                    } else {
                        hits.hits += 1;
                        hits.saved_nanos += nanos;
                    }
                }
                None => {
                    map.insert(key, effect);
                    hits.first_seen += 1;
                }
            };
        if strict {
            count_hit(
                &mut exact_strict,
                record.exact_key(true),
                &mut out.exact_strict,
                nanos,
            );
            count_hit(
                &mut section_strict,
                record.section_key(true),
                &mut out.section_strict,
                nanos,
            );
            count_hit(
                &mut pure_strict,
                record.pure_section_key(true),
                &mut out.pure_strict,
                nanos,
            );
        }
        if optimistic {
            count_hit(
                &mut exact_optimistic,
                record.exact_key(false),
                &mut out.exact_optimistic,
                nanos,
            );
            count_hit(
                &mut section_optimistic,
                record.section_key(false),
                &mut out.section_optimistic,
                nanos,
            );
            count_hit(
                &mut pure_optimistic,
                record.pure_section_key(false),
                &mut out.pure_optimistic,
                nanos,
            );
        }
        out.exact_strict.records = exact_strict.len();
        out.exact_optimistic.records = exact_optimistic.len();
        out.section_strict.records = section_strict.len();
        out.section_optimistic.records = section_optimistic.len();
        out.pure_strict.records = pure_strict.len();
        out.pure_optimistic.records = pure_optimistic.len();
        if (index + 1) % growth_step == 0 || index + 1 == records.len() {
            let count = index + 1;
            out.exact_strict
                .growth
                .push((count, exact_strict.len(), section_strict.len()));
            out.exact_optimistic.growth.push((
                count,
                exact_optimistic.len(),
                section_optimistic.len(),
            ));
            out.section_strict
                .growth
                .push((count, exact_strict.len(), section_strict.len()));
            out.section_optimistic.growth.push((
                count,
                exact_optimistic.len(),
                section_optimistic.len(),
            ));
            out.pure_strict
                .growth
                .push((count, exact_strict.len(), pure_strict.len()));
            out.pure_optimistic
                .growth
                .push((count, exact_optimistic.len(), pure_optimistic.len()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        level::{LevelTables, Trigger},
        testing::TestLevel,
    };

    /// 录制包装转发全部读写：同一个场景开/关录制，方块和红石记忆的最终状态相同。
    #[test]
    fn recording_does_not_change_the_world() {
        let mut plain = TestLevel::new();
        let mut recorded = TestLevel::new();
        let pos = BlockPos::new(1, 64, 1);
        for level in [&mut plain, &mut recorded] {
            level.set(pos, BlockState(mc_bamboo_data::blocks::REPEATER));
            level.set(pos.below(), BlockState(mc_bamboo_data::blocks::STONE));
        }
        let mut plain_ticks = crate::BlockTicks::new();
        let mut recorded_ticks = crate::BlockTicks::new();
        let mut recorder = Recording::new(&mut recorded);
        let mut world = crate::World::new(&mut recorder, &mut recorded_ticks);
        world.block_state(pos);
        world.set_comparator_output(pos, 7);
        world.push_effect(Effect::TorchBurnout { pos });
        world.begin_trigger(Trigger::ScheduledTick { pos, kind: 0 });
        world.block_state(pos);
        world.end_trigger();
        let records = recorder.take_records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].reads.len(), 2);
        let mut world = crate::World::new(&mut plain, &mut plain_ticks);
        world.block_state(pos);
        world.set_comparator_output(pos, 7);
        world.push_effect(Effect::TorchBurnout { pos });
        assert_eq!(plain.get(pos), recorded.get(pos));
        assert_eq!(plain.redstone_table().comparator_output(pos), 7);
        assert_eq!(recorded.redstone_table().comparator_output(pos), 7);
    }

    /// 段哈希键包含精确读集合，所以它撞上的键不会比精确键多。
    #[test]
    fn section_key_is_no_coarser_than_exact_key() {
        let mut level = TestLevel::new();
        let mut recorder = Recording::new(&mut level);
        let mut ticks = crate::BlockTicks::new();
        let pos = BlockPos::new(0, 64, 0);
        let world = crate::World::new(&mut recorder, &mut ticks);
        world.begin_trigger(Trigger::ScheduledTick { pos, kind: 0 });
        world.block_state(pos);
        world.block_state(pos);
        world.end_trigger();
        let records = recorder.take_records();
        let rates = hit_rates(&records, 1);
        assert_eq!(rates.exact_strict.mismatched_effects, 0);
        assert_eq!(rates.section_strict.mismatched_effects, 0);
        assert_eq!(records[0].sections.len(), 1);
    }
}
