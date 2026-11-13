//! 红石热点缓存：录下一次触发读到的状态和产生的效果，命中时直接套用。
//!
//! 第一版只接**方块计划刻**和**方块事件**两种触发，而且只缓存白名单里的读和写：
//! 读 `K_BLOCK` / `K_MIN_Y` / `K_MAX_Y` / `K_REDSTONE` / `K_TICK_QUERY`，
//! 写 `Block` / `TickScheduled` / `Redstone(kind 0)` / `BlockEvent`。
//! 白名单集中在 [`supported_read`] 和 [`supported_write`] 两个函数，第 4 步扩活塞时
//! 只改这两处。守卫不过就退回解释执行，守卫阶段零副作用。
//!
//! 段的整段方块状态哈希由 `mc-bamboo-world::Chunk` 维护
//! （`docs/performance-optimizations.md` 7h），这里通过 [`Level::section_hash`] 读它。

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

use mc_bamboo_core::BlockPos;

use crate::{
    BlockState,
    block_entity::BlockEvent,
    level::Trigger,
    recording::{ChunkSection, Uncacheable, Write},
    ticks::TickPriority,
};

pub mod guard;

pub use guard::{GuardRead, ReadMethod};

/// 缓存的总开关和参数。`enabled = false` 时所有入口都只解释执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheConfig {
    pub enabled: bool,
    /// 触发点累计解释耗时达到这个纳秒数才开始录制；0 表示第一次就录（方块对拍用）。
    pub hot_nanos: u64,
    /// 两秒内触发数的兜底阈值。
    pub hot_triggers_per_2s: u32,
    /// 连续未命中多少次、且已存多少条记录后停录。
    pub stop_miss_streak: u32,
    pub stop_min_records: usize,
    /// 停录后的冷却刻数，每次停录翻倍，上限 3200 刻。
    pub stop_cooldown_ticks: i64,
    /// 每个缓存实例的内存上限（字节）。
    pub memory_limit: usize,
    /// 单条记录的最大字节数。
    pub record_limit: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hot_nanos: 2_000_000,
            hot_triggers_per_2s: 16,
            stop_miss_streak: 64,
            stop_min_records: 32,
            stop_cooldown_ticks: 200,
            memory_limit: 32 * 1024 * 1024,
            record_limit: 1024 * 1024,
        }
    }
}

impl CacheConfig {
    /// 方块对拍用：第一次就录，其余参数保持默认。
    pub fn immediate() -> Self {
        Self {
            enabled: true,
            hot_nanos: 0,
            ..Self::default()
        }
    }
}

/// 累计统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub triggers: u64,
    pub replays: u64,
    pub misses: u64,
    pub recorded: u64,
    pub rejected: u64,
    pub evictions: u64,
    pub stopped_sites: u64,
}

/// 一个 16³ 段的整段方块状态哈希，录制时和重放前都要对上。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SectionKey {
    pub section: ChunkSection,
    pub hash: u128,
}

/// 一次可重放的效果，带真实参数（实验的 `Write` 只存哈希，不能用来重放）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReplayEffect {
    Block {
        pos: BlockPos,
        state: BlockState,
        flags: u32,
        applied: bool,
    },
    Comparator {
        pos: BlockPos,
        signal: i32,
    },
    BlockEvent(BlockEvent),
    /// `Effect::BlockEvent`，方块事件成功时发给附近玩家的包。
    BlockEventPacket {
        pos: BlockPos,
        block: usize,
        param_a: i32,
        param_b: i32,
    },
    ScheduledTick {
        pos: BlockPos,
        block: usize,
        delay: i32,
        priority: TickPriority,
    },
    ScheduledFluidTick {
        pos: BlockPos,
        fluid: i32,
        delay: i32,
    },
}

/// 一次触发的运行时记录：守卫读项 + 可重放效果。
///
/// 和实验的 [`Record`] 并行产出，实验那套一行不改；这里的读项带真实值，
/// 守卫能按 [`ReadMethod`] 重算并逐项复核。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRecord {
    pub trigger: Trigger,
    pub tick: i64,
    pub nanos: u64,
    pub reads: Vec<GuardRead>,
    pub effects: Vec<ReplayEffect>,
    pub sections: Vec<SectionKey>,
    pub uncacheable: Vec<Uncacheable>,
    /// 两次触发边界之间发生过写，这条记录不能缓存，见设计稿 §4.2。
    pub boundary_writes: bool,
    /// 录到了白名单以外的读或写。
    pub rejected: bool,
}

impl RuntimeRecord {
    /// 这条记录是不是能进缓存（白名单 + 无不可缓存标记 + 边界外没有写）。
    pub fn supported(&self) -> bool {
        !self.rejected
            && !self.boundary_writes
            && self.uncacheable.is_empty()
            && self.reads.iter().all(|read| supported_read(read.method))
            && self.effects.iter().all(supported_effect)
            && matches!(
                self.trigger,
                Trigger::ScheduledTick { .. } | Trigger::BlockEvent { .. }
            )
    }
}

/// 实验录制层的类别编号到运行时白名单的映射。
pub const fn supported_kind(kind: u8) -> bool {
    matches!(kind, 1 | 6 | 7 | 9 | 10 | 29)
}

/// 白名单：第一版只支持这些读入口。
pub const fn supported_read(method: ReadMethod) -> bool {
    matches!(
        method,
        ReadMethod::Block
            | ReadMethod::Trigger
            | ReadMethod::MinY
            | ReadMethod::MaxY
            | ReadMethod::Comparator(_)
            | ReadMethod::TickQuery(_, _, _)
            | ReadMethod::Dedup(_, _)
            | ReadMethod::BlockEventDedup(_)
    )
}

/// 白名单：第一版只支持这些写。
pub const fn supported_write(write: Write) -> bool {
    matches!(
        write,
        Write::Block { .. }
            | Write::TickScheduled { .. }
            | Write::Redstone { kind: 0, .. }
            | Write::BlockEvent { popped: None, .. }
    )
}

/// 白名单：从实验 `Write` 转成的效果。
pub const fn supported_effect(effect: &ReplayEffect) -> bool {
    matches!(
        effect,
        ReplayEffect::Block { .. }
            | ReplayEffect::ScheduledTick { .. }
            | ReplayEffect::ScheduledFluidTick { .. }
            | ReplayEffect::Comparator { .. }
            | ReplayEffect::BlockEvent(_)
            | ReplayEffect::BlockEventPacket { .. }
    )
}

/// 完整键原料，128 位哈希碰撞靠逐项比对挡住。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct KeyParts {
    trigger: Trigger,
    sections: Vec<SectionKey>,
    reads: Vec<GuardRead>,
}

/// 一条缓存记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    parts: KeyParts,
    effects: Vec<ReplayEffect>,
    bytes: usize,
    last_used: i64,
}

/// 每个触发点（归一化后的 `Trigger`）的统计。
#[derive(Debug, Clone, Copy, Default)]
struct Site {
    interpreted_nanos: u64,
    misses: u32,
    records: usize,
    stopped_until: i64,
    cooldown: i64,
    window_start: i64,
    window_count: u32,
}

/// 红石缓存。一个 region（或实验用假世界）一份。
#[derive(Debug)]
pub struct RedstoneCache {
    config: CacheConfig,
    /// 触发点 → 键列表；守卫时按它们逐个查表。
    site_keys: HashMap<u128, Vec<u128>>,
    entries: HashMap<u128, Vec<Entry>>,
    used: HashMap<u128, i64>,
    sites: HashMap<u128, Site>,
    bytes: usize,
    now: i64,
    stats: CacheStats,
    site: u128,
}

impl RedstoneCache {
    pub fn new(config: CacheConfig) -> Self {
        Self {
            config,
            site_keys: HashMap::new(),
            entries: HashMap::new(),
            used: HashMap::new(),
            sites: HashMap::new(),
            bytes: 0,
            now: 0,
            stats: CacheStats::default(),
            site: 0,
        }
    }

    pub const fn config(&self) -> &CacheConfig {
        &self.config
    }

    pub const fn stats(&self) -> CacheStats {
        self.stats
    }

    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn entry_count(&self) -> usize {
        self.entries.values().map(Vec::len).sum()
    }

    /// 每刻开头由驱动调用。
    pub fn set_time(&mut self, now: i64) {
        self.now = now;
    }

    /// 守卫通过时返回该重放的效果序列，调用方不再解释执行。
    ///
    /// `guard` 由调用方提供：它同时借着 `Level` 和 `BlockTicks`，见设计稿 §3.1。
    /// `sections` 也由调用方在命中前复核，见 [`crate::cache_run::run_trigger`]。
    pub fn try_replay(
        &mut self,
        trigger: Trigger,
        guard: &mut dyn FnMut(&GuardRead) -> Option<u64>,
        section: &dyn Fn(ChunkSection) -> Option<u128>,
    ) -> Option<Vec<ReplayEffect>> {
        if !self.config.enabled {
            return None;
        }
        let site = site_key(trigger);
        self.site = site;
        self.stats.triggers += 1;
        let keys = self.site_keys.get(&site).cloned().unwrap_or_default();
        for key in keys {
            let Some(bucket) = self.entries.get(&key) else {
                continue;
            };
            for entry in bucket {
                if entry.parts.trigger != trigger {
                    continue;
                }
                if !guard_matches(entry, guard) {
                    continue;
                }
                // 段哈希覆盖整段方块状态，必须逐段复核缓存记录里的每一段。
                if !entry
                    .parts
                    .sections
                    .iter()
                    .all(|s| section(s.section) == Some(s.hash))
                {
                    continue;
                }
                self.used.insert(key, self.now);
                self.sites.entry(site).and_modify(|s| s.misses = 0);
                self.stats.replays += 1;
                return Some(entry.effects.clone());
            }
        }
        self.stats.misses += 1;
        self.sites.entry(site).and_modify(|s| s.misses += 1);
        None
    }

    /// 把一条解释执行得到的运行时记录收进缓存。返回是否收下了。
    pub fn record(&mut self, record: &RuntimeRecord) -> bool {
        if !self.config.enabled {
            return false;
        }
        let site = self.site;
        let config = self.config;
        let state = self.sites.entry(site).or_default();
        state.interpreted_nanos = state.interpreted_nanos.saturating_add(record.nanos);
        if self.now - state.window_start >= 20 {
            state.window_start = self.now;
            state.window_count = 0;
        }
        state.window_count += 1;
        let hot = config.hot_nanos == 0
            || state.interpreted_nanos >= config.hot_nanos
            || state.window_count >= config.hot_triggers_per_2s;
        let stopped = state.stopped_until > self.now;
        if !hot || stopped || !record.supported() {
            self.stats.rejected += 1;
            self.update_stop(site);
            return false;
        }
        let mut sections = record.sections.clone();
        sections.sort_by_key(|s| s.section);
        let bytes = entry_bytes(&record.reads, &record.effects, &sections);
        if bytes > config.record_limit {
            self.stats.rejected += 1;
            self.update_stop(site);
            return false;
        }
        let parts = KeyParts {
            trigger: record.trigger,
            sections,
            reads: record.reads.clone(),
        };
        let key = hash_parts(&parts);
        let bucket = self.entries.entry(key).or_default();
        if let Some(old) = bucket.iter_mut().find(|old| old.parts == parts) {
            let delta = bytes.saturating_sub(old.bytes);
            old.effects = record.effects.clone();
            old.last_used = self.now;
            self.bytes = self.bytes.saturating_add(delta);
        } else {
            bucket.push(Entry {
                parts,
                effects: record.effects.clone(),
                bytes,
                last_used: self.now,
            });
            self.bytes = self.bytes.saturating_add(bytes);
            self.sites.entry(site).and_modify(|s| s.records += 1);
            self.stats.recorded += 1;
            let keys = self.site_keys.entry(site).or_default();
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        self.used.insert(key, self.now);
        self.sites.entry(site).and_modify(|s| s.misses = 0);
        self.trim_memory();
        true
    }

    fn update_stop(&mut self, site: u128) {
        let Some(state) = self.sites.get(&site).copied() else {
            return;
        };
        if state.misses >= self.config.stop_miss_streak
            && state.records >= self.config.stop_min_records
        {
            let base = self.config.stop_cooldown_ticks.clamp(1, 3200);
            let cooldown = base.max(state.cooldown);
            self.sites.entry(site).and_modify(|s| {
                s.stopped_until = self.now + cooldown;
                s.cooldown = (cooldown * 2).min(3200);
                s.misses = 0;
            });
            self.stats.stopped_sites += 1;
        }
    }

    fn trim_memory(&mut self) {
        if self.bytes <= self.config.memory_limit {
            return;
        }
        let target = self.config.memory_limit * 80 / 100;
        let mut order: Vec<(i64, u128)> = self.used.iter().map(|(&k, &t)| (t, k)).collect();
        order.sort_unstable();
        for (_, key) in order {
            if self.bytes <= target {
                break;
            }
            if let Some(bucket) = self.entries.remove(&key) {
                for entry in bucket {
                    self.bytes = self.bytes.saturating_sub(entry.bytes);
                }
                self.used.remove(&key);
                self.stats.evictions += 1;
            }
        }
        self.site_keys.retain(|_, keys| {
            keys.retain(|key| self.entries.contains_key(key));
            !keys.is_empty()
        });
    }
}

/// 守卫：逐项重算 `GuardRead` 的哈希。段哈希由 [`guard::SectionGuard`] 单独复核。
fn guard_matches(entry: &Entry, guard: &mut dyn FnMut(&GuardRead) -> Option<u64>) -> bool {
    entry.parts.reads.iter().all(|read| match guard(read) {
        Some(value) => value == read.value,
        None => false,
    })
}

fn entry_bytes(reads: &[GuardRead], effects: &[ReplayEffect], sections: &[SectionKey]) -> usize {
    std::mem::size_of::<Entry>()
        + std::mem::size_of_val(reads)
        + std::mem::size_of_val(effects)
        + std::mem::size_of_val(sections)
}

fn hash_parts(parts: &KeyParts) -> u128 {
    let mut hasher = KeyHasher::new();
    parts.hash(&mut hasher);
    hasher.finish128()
}

fn site_key(trigger: Trigger) -> u128 {
    let mut hasher = KeyHasher::new();
    let (tag, pos) = match trigger {
        Trigger::ScheduledTick { pos, .. } => (0_u8, pos),
        Trigger::ScheduledFluidTick { pos, .. } => (1_u8, pos),
        Trigger::BlockEvent { pos, .. } => (2_u8, pos),
        Trigger::BlockEntityTick { pos, .. } => (3_u8, pos),
    };
    (tag, pos).hash(&mut hasher);
    hasher.finish128()
}

/// 和实验录制层同一个固定种子算法，键在不同进程里可比。
struct KeyHasher {
    lo: u64,
    hi: u64,
}

impl KeyHasher {
    const fn new() -> Self {
        Self {
            lo: 0x243f_6a88_85a3_08d3,
            hi: 0x1319_8a2e_0370_7344,
        }
    }

    fn finish128(self) -> u128 {
        self.lo as u128 | (self.hi as u128) << 64
    }
}

impl Hasher for KeyHasher {
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

/// guard 模块按真实值重算哈希时用同一个稳定算法。
pub(crate) fn hash_value(value: &impl Hash) -> u64 {
    crate::recording::stable_hash(value)
}
