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
    cell::RefCell,
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    time::Instant,
};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, sign::SignContent};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    BlockState, Container, HopperEntity, Level, LevelRandom,
    behaviour::machines::{BrewingStandEntity, CampfireEntity, FurnaceEntity, PistonMoving, ShulkerBoxEntity},
    block_entity::{BlockEntity, BlockEvent},
    entity::Entities,
    gamerules::BlockGameRules,
    level::{Effect, EquipTarget, LecternFacts, LevelSideState, Observation, Trigger},
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
            | Self::BlockEvent { event: BlockEvent { pos, .. }, .. }
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
            if strict_time || read.kind != K_TIME {
                read.hash(&mut hasher);
            }
        }
        hasher.finish128()
    }

    /// 段哈希键：触发本身 + 精确读集合 + 读集合涉及各段的整段方块状态哈希。
    ///
    /// 键里带着精确读集合，是为了保证它不比精确键粗：整段哈希里只有方块状态，
    /// 容器、红石记忆、计划刻查询这些非方块状态只能由精确读项补上。因此同一段里
    /// 无关的方块变了会失配，命中率不高于精确键。
    pub fn section_key(&self, strict_time: bool) -> u128 {
        let mut hasher = StableHasher::new();
        self.trigger.hash(&mut hasher);
        for read in &self.reads {
            if strict_time || read.kind != K_TIME {
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
        self.reads.iter().filter_map(|read| read.pos).collect::<HashSet<_>>().len()
    }

    /// 写集合里去重后的位置数。
    pub fn write_positions(&self) -> usize {
        self.writes.iter().filter_map(|write| write.pos()).collect::<HashSet<_>>().len()
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
}

struct Inner {
    records: Vec<Record>,
    current: Option<Frame>,
    /// 驱动循环在两次触发边界之间做的读写，归到下一次触发。
    pending_reads: Vec<Read>,
    pending_writes: Vec<Write>,
    pending_uncacheable: Vec<Uncacheable>,
    /// 每段现在的内容哈希，写方块时增量更新（设计稿 9.3 的做法）。
    section_hashes: HashMap<ChunkSection, u128>,
    tick: i64,
}

struct Frame {
    record: Record,
    started: Instant,
    /// 触发开始时各段的哈希快照。
    section_start: HashMap<ChunkSection, u128>,
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
                section_hashes: HashMap::new(),
                tick: 0,
            }),
        }
    }

    /// 把时钟拨到 `tick`，丢掉上一刻遗留的边界读写。
    pub fn set_tick(&self, tick: i64) {
        let mut inner = self.inner.borrow_mut();
        inner.tick = tick;
        inner.pending_reads.clear();
        inner.pending_writes.clear();
        inner.pending_uncacheable.clear();
    }

    /// 取走目前为止的全部记录，清空。
    pub fn take_records(&self) -> Vec<Record> {
        std::mem::take(&mut self.inner.borrow_mut().records)
    }

    /// 借回里面的世界，供实验程序读状态。录制器还活着时不要用它跑方块逻辑。
    pub fn level(&mut self) -> &mut dyn Level {
        self.level
    }

    /// 一次读进读集合。
    fn read(&self, pos: Option<BlockPos>, kind: u8, value: impl Hash) {
        let value = stable_hash(&value);
        let section = pos.map(ChunkSection::of);
        if let Some(section) = section {
            self.ensure_section(section);
        }
        let entry = Read { pos, kind, value };
        let hash = section.and_then(|section| self.inner.borrow().section_hashes.get(&section).copied());
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

    /// 一次写进效果。
    fn write(&self, entry: Write) {
        let mut inner = self.inner.borrow_mut();
        match inner.current.as_mut() {
            Some(frame) => frame.record.writes.push(entry),
            None => inner.pending_writes.push(entry),
        }
    }

    fn redstone_write(&self, pos: Option<BlockPos>, kind: u8, value: impl Hash) {
        let value = stable_hash(&value);
        self.write(Write::Redstone { pos, kind, value });
    }

    /// 标一个不能缓存的原因，同一次触发里只记一次。
    fn uncacheable(&self, reason: Uncacheable) {
        let mut inner = self.inner.borrow_mut();
        match inner.current.as_mut() {
            Some(frame) => {
                if !frame.record.uncacheable.contains(&reason) {
                    frame.record.uncacheable.push(reason);
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
        let mut hash = 0_u128;
        for y in section.section_y * 16..section.section_y * 16 + 16 {
            for z in section.chunk.z * 16..section.chunk.z * 16 + 16 {
                for x in section.chunk.x * 16..section.chunk.x * 16 + 16 {
                    let pos = BlockPos::new(x, y, z);
                    hash ^= cell_hash(pos, self.level.block_state(pos));
                }
            }
        }
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
const K_TICKER: u8 = 32;

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

fn stable_hash(value: &impl Hash) -> u64 {
    let mut hasher = StableHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn cell_hash(pos: BlockPos, state: BlockState) -> u128 {
    let lo = stable_hash(&(pos, 0_u8, state.0));
    let hi = stable_hash(&(pos, 1_u8, state.0));
    u128::from(lo) | u128::from(hi) << 64
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
        Effect::DispenserEquipped { entity_id, menu_slot } => {
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

fn hash_slots(slots: &[ItemStack]) -> u64 {
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
fn entity_facts(entity: &dyn BlockEntity, now: i64) -> u64 {
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
        hopper.ticked_game_time().saturating_sub(now).hash(&mut hasher);
    } else if let Some(shulker) = entity.as_any().downcast_ref::<ShulkerBoxEntity>() {
        shulker.open_count().hash(&mut hasher);
        (shulker.status() as u8).hash(&mut hasher);
        shulker.progress(1.0).to_bits().hash(&mut hasher);
        shulker.progress(0.0).to_bits().hash(&mut hasher);
    } else if let Some(piston) = entity.as_any().downcast_ref::<PistonMoving>() {
        piston.observed_facts(now).hash(&mut hasher);
    }
    hasher.finish()
}

impl LevelSideState for Recording<'_> {
    fn comparator_output(&self, pos: BlockPos) -> i32 {
        let value = self.level.comparator_output(pos);
        self.read(Some(pos), K_REDSTONE, (0_u8, value));
        value
    }

    fn set_comparator_output(&mut self, pos: BlockPos, signal: i32) {
        self.redstone_write(Some(pos), 0, (0_u8, signal));
        self.level.set_comparator_output(pos, signal);
    }

    fn last_bookshelf_slot(&self, pos: BlockPos) -> Option<usize> {
        let value = self.level.last_bookshelf_slot(pos);
        self.read(Some(pos), K_REDSTONE, (1_u8, value));
        value
    }

    fn set_last_bookshelf_slot(&mut self, pos: BlockPos, slot: usize) {
        self.redstone_write(Some(pos), 1, (1_u8, slot));
        self.level.set_last_bookshelf_slot(pos, slot);
    }

    fn is_jukebox_playing(&self, pos: BlockPos) -> bool {
        let value = self.level.is_jukebox_playing(pos);
        self.read(Some(pos), K_REDSTONE, (2_u8, value));
        value
    }

    fn set_jukebox_playing(&mut self, pos: BlockPos, playing: bool) {
        self.redstone_write(Some(pos), 2, (2_u8, playing));
        self.level.set_jukebox_playing(pos, playing);
    }

    fn crafter_disabled(&self, pos: BlockPos) -> u16 {
        let value = self.level.crafter_disabled(pos);
        self.read(Some(pos), K_REDSTONE, (3_u8, value));
        value
    }

    fn set_crafter_disabled(&mut self, pos: BlockPos, mask: u16) {
        self.redstone_write(Some(pos), 3, (3_u8, mask));
        self.level.set_crafter_disabled(pos, mask);
    }

    fn set_crafter_crafting(&mut self, pos: BlockPos, remaining: i32) {
        self.redstone_write(Some(pos), 4, (4_u8, remaining));
        self.level.set_crafter_crafting(pos, remaining);
    }

    fn take_crafter_crafting(&mut self) -> Vec<(BlockPos, i32)> {
        let value = self.level.take_crafter_crafting();
        self.read(None, K_REDSTONE, (5_u8, &value));
        value
    }

    fn restore_crafter_crafting(&mut self, entries: Vec<(BlockPos, i32)>) {
        self.redstone_write(None, 5, (5_u8, &entries));
        self.level.restore_crafter_crafting(entries);
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
        value
    }

    fn push_block_event(&mut self, event: BlockEvent) {
        self.write(Write::BlockEvent {
            event,
            popped: None,
        });
        self.level.push_block_event(event);
    }

    fn pop_block_event(&mut self) -> Option<BlockEvent> {
        let value = self.level.pop_block_event();
        self.write(Write::BlockEvent {
            event: value.unwrap_or(BlockEvent {
                pos: BlockPos::new(0, 0, 0),
                block: usize::MAX,
                param_a: 0,
                param_b: 0,
            }),
            popped: value,
        });
        value
    }

    fn block_entity_dyn(&self, pos: BlockPos) -> Option<&dyn BlockEntity> {
        let value = self.level.block_entity_dyn(pos);
        let now = self.inner.borrow().tick;
        self.read(Some(pos), K_BLOCK_ENTITY, value.map(|entity| entity_facts(entity, now)));
        value
    }

    fn has_block_entity(&self, pos: BlockPos) -> bool {
        let value = self.level.has_block_entity(pos);
        self.read(Some(pos), K_BLOCK_ENTITY, (0_u8, value));
        value
    }

    fn insert_block_entity(&mut self, entity: Box<dyn BlockEntity>) {
        let pos = entity.pos();
        let now = self.inner.borrow().tick;
        let facts = entity_facts(entity.as_ref(), now);
        self.write(Write::BlockEntity {
            pos,
            kind: 0,
            value: facts,
        });
        self.level.insert_block_entity(entity);
    }

    fn remove_block_entity(&mut self, pos: BlockPos) -> Option<Box<dyn BlockEntity>> {
        let value = self.level.remove_block_entity(pos);
        let now = self.inner.borrow().tick;
        self.write(Write::BlockEntity {
            pos,
            kind: 1,
            value: stable_hash(&value.as_ref().map(|entity| entity_facts(entity.as_ref(), now))),
        });
        value
    }

    fn take_block_entity_at(&mut self, pos: BlockPos) -> Option<(u64, Box<dyn BlockEntity>)> {
        // 驱动把实体临时拿出去不是「效果」，只是实现细节；记一笔读就够了。
        let value = self.level.take_block_entity_at(pos);
        let now = self.inner.borrow().tick;
        self.read(
            Some(pos),
            K_BLOCK_ENTITY,
            value.as_ref().map(|(id, entity)| (*id, entity_facts(entity.as_ref(), now))),
        );
        value
    }

    fn take_block_entity(&mut self, pos: BlockPos, id: u64) -> Option<Box<dyn BlockEntity>> {
        let value = self.level.take_block_entity(pos, id);
        let now = self.inner.borrow().tick;
        self.read(
            Some(pos),
            K_BLOCK_ENTITY,
            (id, value.as_ref().map(|entity| entity_facts(entity.as_ref(), now))),
        );
        value
    }

    fn put_block_entity(&mut self, pos: BlockPos, id: u64, entity: Box<dyn BlockEntity>) {
        // 放回去也不是「效果」：实体改了什么由这次触发里别的读写记录。
        let now = self.inner.borrow().tick;
        self.read(Some(pos), K_BLOCK_ENTITY, (id, entity_facts(entity.as_ref(), now)));
        self.level.put_block_entity(pos, id, entity);
    }

    fn is_block_entity_taken(&self, pos: BlockPos) -> bool {
        let value = self.level.is_block_entity_taken(pos);
        self.read(Some(pos), K_BLOCK_ENTITY, (1_u8, value));
        value
    }

    fn begin_block_entity_tick(&mut self) {
        self.write(Write::Ticker(0));
        self.level.begin_block_entity_tick();
    }

    fn next_block_entity_ticker(&mut self) -> Option<(BlockPos, u64)> {
        let value = self.level.next_block_entity_ticker();
        self.read(None, K_TICKER, value);
        value
    }

    fn end_block_entity_tick(&mut self) {
        self.write(Write::Ticker(1));
        self.level.end_block_entity_tick();
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
        value
    }

    fn max_y(&self) -> i32 {
        let value = self.level.max_y();
        self.read(None, K_MAX_Y, value);
        value
    }

    fn random(&self) -> &LevelRandom {
        self.uncacheable(Uncacheable::Random);
        self.read(None, K_RANDOM, self.level.random().source().seed());
        self.level.random()
    }

    fn is_randomly_ticking_section(&self, chunk: ChunkPos, section_y: i32) -> bool {
        let value = self.level.is_randomly_ticking_section(chunk, section_y);
        self.read(None, K_SECTION_TICKING, (chunk, section_y, value));
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
        self.write(Write::Sign { pos, value, applied });
        applied
    }

    fn note_block_sound(&self, pos: BlockPos) -> Option<String> {
        let value = self.level.note_block_sound(pos);
        self.read(Some(pos), K_NOTE_SOUND, &value);
        value
    }

    fn runs_block_entity_tick(&self, pos: BlockPos) -> bool {
        let value = self.level.runs_block_entity_tick(pos);
        self.read(Some(pos), K_TICKER, (2_u8, value));
        value
    }

    fn entities(&self) -> &Entities {
        self.uncacheable(Uncacheable::Entities);
        self.read(None, K_ENTITIES, 0_u8);
        self.level.entities()
    }

    fn entities_mut(&mut self) -> &mut Entities {
        self.uncacheable(Uncacheable::Entities);
        self.read(None, K_ENTITIES, 1_u8);
        self.level.entities_mut()
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
        self.read(None, K_WEATHER, (value.rain_level.to_bits(), value.thunder_level.to_bits()));
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
        let now = self.inner.borrow().tick;
        let before = self.level.block_entity_dyn(pos).map(|entity| entity_facts(entity, now));
        let applied = self.level.with_hopper(pos, f);
        let after = self.level.block_entity_dyn(pos).map(|entity| entity_facts(entity, now));
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
        value
    }

    fn clear_container(&mut self, pos: BlockPos) {
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
        self.read(Some(pos), K_LOOT_UNPACK, 0_u8);
        self.level.unpack_loot_table(pos);
    }

    fn creative_container_drop(&mut self, pos: BlockPos, item: i32) -> Option<ItemStack> {
        let value = self.level.creative_container_drop(pos, item);
        self.read(
            Some(pos),
            K_DROPS,
            (2_u8, item, value.as_ref().map(|stack| hash_slots(std::slice::from_ref(stack)))),
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

    fn observe(&self, event: Observation<'_>) {
        match event {
            Observation::TriggerBegin(trigger) => {
                let mut inner = self.inner.borrow_mut();
                if let Some(frame) = inner.current.take() {
                    inner.records.push(finish_record(frame));
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
                    record,
                    started: Instant::now(),
                    section_start: HashMap::new(),
                });
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
                self.inner.borrow_mut().current.as_mut().expect("刚放进来的触发").section_start =
                    section_start;
            }
            Observation::TriggerEnd => {
                let mut inner = self.inner.borrow_mut();
                if let Some(frame) = inner.current.take() {
                    inner.records.push(finish_record(frame));
                }
            }
            Observation::TimeRead => {
                self.uncacheable(Uncacheable::Time);
                self.read(None, K_TIME, 0_u8);
            }
            Observation::Effect(effect) => {
                self.write(Write::Effect(hash_effect(effect)));
            }
            Observation::TickQuery {
                pos,
                kind,
                will_tick_this_tick,
                result,
            } => {
                self.read(Some(pos), K_TICK_QUERY, (kind, will_tick_this_tick, result));
            }
            Observation::TickScheduled {
                pos,
                kind,
                fluid,
                delay,
                priority,
            } => {
                self.write(Write::TickScheduled {
                    pos,
                    kind,
                    fluid,
                    delay,
                    priority,
                });
            }
        }
    }
}

fn finish_record(mut frame: Frame) -> Record {
    frame.record.nanos = u64::try_from(frame.started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let used: HashSet<ChunkSection> = frame
        .record
        .reads
        .iter()
        .filter_map(|read| read.pos)
        .map(ChunkSection::of)
        .collect();
    frame.record.sections = frame
        .section_start
        .into_iter()
        .filter(|(section, _)| used.contains(section))
        .collect();
    frame.record.sections.sort_by_key(|(section, _)| *section);
    frame.record
}

const _: fn() = || {
    fn assert_level<T: Level + LevelSideState>() {}
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
    pub section_strict: Hits,
    pub section_optimistic: Hits,
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
    for (index, record) in records.iter().enumerate() {
        let effect = record.effect_key();
        let nanos = u128::from(record.nanos);
        out.total_nanos += nanos;
        let strict = record.cacheable(true);
        let optimistic = record.cacheable(false);
        let count_hit = |map: &mut HashMap<u128, u128>, key: u128, hits: &mut Hits, nanos: u128| {
            match map.get(&key) {
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
        }
        out.exact_strict.records = exact_strict.len();
        out.exact_optimistic.records = exact_optimistic.len();
        out.section_strict.records = section_strict.len();
        out.section_optimistic.records = section_optimistic.len();
        if (index + 1) % growth_step == 0 || index + 1 == records.len() {
            let count = index + 1;
            out.exact_strict.growth.push((count, exact_strict.len(), section_strict.len()));
            out.exact_optimistic
                .growth
                .push((count, exact_optimistic.len(), section_optimistic.len()));
            out.section_strict
                .growth
                .push((count, exact_strict.len(), section_strict.len()));
            out.section_optimistic
                .growth
                .push((count, exact_optimistic.len(), section_optimistic.len()));
        }
    }
    out
}
