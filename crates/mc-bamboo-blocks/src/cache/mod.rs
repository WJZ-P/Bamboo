//! 红石热点缓存：录下一次触发读到的状态和产生的效果，命中时直接套用。
//!
//! 只缓存白名单里的读和写，白名单集中在 [`supported_read`]、[`supported_write`]
//! 和 [`supported_effect`]；每个白名单读项都有一条按 [`ReadMethod`] 重算真实值的守卫，
//! 漏一个读就整条记录作废。守卫不过就退回解释执行，守卫阶段零副作用。
//!
//! 划算不划算按触发点自己算账，不用墙上时间：`RuntimeRecord::work()` 是这次解释执行的
//! 确定性工作量（读写项数），门槛、命中省下的代价、未命中白付的代价都用同一个单位。
//! 见 [`Site`] 和 [`CacheConfig`]。
//!
//! 段的整段方块状态哈希由 `mc-bamboo-world::Chunk` 维护
//! （`docs/performance-optimizations.md` 7h），这里通过 [`Level::section_hash`] 读它。

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

use mc_bamboo_core::BlockPos;
use mc_bamboo_protocol::item::ItemStack;

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
///
/// 工作量单位是 [`RuntimeRecord::work`] 的一次读写项，不含墙上时间；门槛和净收益
/// 都用这同一个单位，所以判定与机器快慢、编译选项、负载无关。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheConfig {
    pub enabled: bool,
    /// 单次解释执行的工作量低于这个值的触发点不录。理由见 `docs/performance-optimizations.md` 7i：
    /// 九台机器里最大的「小电路」触发是 25732 项，最小的红石线阵列触发是
    /// 145105 项；65536 落在两者之间，留约 2.5 倍 / 2.2 倍的余量。
    /// 0 表示不设门槛（方块对拍用）。
    pub min_work: u64,
    /// 试探阶段允许的触发次数；这么多次里没有一次达到 `min_work` 就判为不划算，
    /// 按冷却规则过一阵再试。
    pub probe_triggers: u32,
    /// 每个结算窗口的触发次数。
    pub ledger_window_triggers: u32,
    /// 窗口内净收益低于这个阈值（工作量单位）就停掉该触发点。
    /// 默认 0：收支相抵也算不划算，因为还白占了内存。设成 `i64::MIN` 可关掉这条规则。
    pub ledger_min_gain: i64,
    /// 窗口结算不划算时，至少已经命中过这么多次才停；给新点一点热起来的时间。
    pub ledger_min_hits: u64,
    /// 停掉一个触发点后的冷却刻数，每次停掉翻倍，上限 3200 刻。
    pub cooldown_ticks: i64,
    /// 每个缓存实例的内存上限（字节）。
    pub memory_limit: usize,
    /// 单条记录的最大字节数。
    pub record_limit: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_work: 65_536,
            probe_triggers: 8,
            ledger_window_triggers: 128,
            ledger_min_gain: 0,
            ledger_min_hits: 8,
            cooldown_ticks: 200,
            memory_limit: 32 * 1024 * 1024,
            record_limit: 1024 * 1024,
        }
    }
}

impl CacheConfig {
    /// 正式配置：总开关打开，其余按默认门槛。
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    /// 方块对拍用：第一次就录，且不因净收益停录，其余参数保持默认。
    pub fn immediate() -> Self {
        Self {
            enabled: true,
            min_work: 0,
            ledger_min_gain: i64::MIN,
            ..Self::default()
        }
    }
}

/// 累计统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// 进入缓存引擎（还值得查一次表）的触发数。
    pub triggers: u64,
    /// 完全不走缓存路径、直接解释执行的触发数。
    pub bypassed: u64,
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
///
/// 不实现 `Hash`：`ItemStack` 没有 `Hash`，而效果序列只按值比较、不进键。
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// `PistonMoving` 的全部字段。重放时按这些值造一个实体，走原始 `insert`。
    PistonMoving {
        pos: BlockPos,
        moved_state: BlockState,
        direction: u8,
        extending: bool,
        source: bool,
        progress: u32,
        progress_o: u32,
    },
    /// 方块实体 tick 跑完后的 `PistonMoving` 状态：原地替换表里那一个，
    /// 保留 ticker 的编号和顺序（设计稿 §3.1 第 2 条）。
    RestorePistonMoving {
        pos: BlockPos,
        moved_state: BlockState,
        direction: u8,
        extending: bool,
        source: bool,
        progress: u32,
        progress_o: u32,
    },
    /// 移除这一格的方块实体，走原始 `remove`。
    RemoveBlockEntity {
        pos: BlockPos,
    },
    /// 把这一格的槽位表整个换成 `slots`，走原始 `with_slots`。
    ContainerSlots {
        pos: BlockPos,
        slots: Vec<ItemStack>,
    },
    /// 清掉这一格的容器，走原始 `clear_container`。
    ClearContainer {
        pos: BlockPos,
    },
    /// 换方块时清掉比较器输出等红石记忆，走原始 `forget_block_entity_memory`。
    ForgetBlockEntityMemory {
        pos: BlockPos,
    },
    /// 解包这一格的战利品表，走原始 `unpack_loot_table`。
    UnpackLootTable {
        pos: BlockPos,
    },
    /// 方块实体 tick 阶段记下「这一格本刻 tick 过」，见 `BlockTicks::note_block_entity_tick`。
    NoteBlockEntityTick {
        pos: BlockPos,
    },
    /// 活塞伸出/收回的音效。pitch 重放时照样从世界随机源取一个数算，
    /// 取数的次数和时机与解释执行相同（设计稿 §3.2）。
    PistonSound {
        pos: BlockPos,
        sound: i32,
        source: u8,
        volume: u32,
        scale: u32,
        base: u32,
        except_initiator: bool,
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
    /// 这次解释执行真正跑过的读项数（实验 `Record` 的 `reads.len()`，含重复读同一格）。
    /// 守卫读项只覆盖「不在方块状态里」的那一小部分，不能拿它当解释代价的代理。
    pub interpreted_items: u64,
    /// 这次解释执行产生的写项数（实验 `Record` 的 `writes.len()`）。
    pub written_items: u64,
    pub reads: Vec<GuardRead>,
    pub effects: Vec<ReplayEffect>,
    pub sections: Vec<SectionKey>,
    pub uncacheable: Vec<Uncacheable>,
    /// 两次触发边界之间发生过写，这条记录不能缓存，见设计稿 §4.2。
    pub boundary_writes: bool,
    /// 录到了白名单以外的读或写。
    pub rejected: bool,
}

/// 守卫阶段一步能否决的代价单位：每查一个段哈希、每重读一条守卫项各算一项。
pub const GUARD_READ_WORK: u64 = 1;
/// 录制包装的常驻代价，按每次转发多记一笔折成一份工作量。
pub const RECORD_OVERHEAD_PER_ITEM: u64 = 1;
/// 建键、克隆记录、插表这些与读写项数无关的固定代价。
pub const RECORD_BASE_WORK: u64 = 8;

impl RuntimeRecord {
    /// 一次解释执行的确定性工作量：读项数 + 写项数 + 段哈希项数。
    ///
    /// 用读写项数而不是墙上时间：它和这次解释执行真正跑过的底层操作数量成正比，
    /// 同一台机器在任何速度的 CPU、任何编译优化下都得到同一个数，测试可复现。
    /// 命中时省下的就是这份工作量，未命中时白付的守卫/录制也按这个量纲折算。
    pub const fn work(&self) -> u64 {
        self.interpreted_items
            .saturating_add(self.written_items)
            .saturating_add(self.sections.len() as u64)
    }

    /// 这条记录是不是能进缓存（白名单 + 无不可缓存标记 + 边界外没有写）。
    pub fn supported(&self) -> bool {
        !self.rejected
            && !self.boundary_writes
            && self.uncacheable.is_empty()
            && self.reads.iter().all(|read| supported_read(read.method))
            && self.effects.iter().all(supported_effect)
            && matches!(
                self.trigger,
                Trigger::ScheduledTick { .. }
                    | Trigger::BlockEvent { .. }
                    | Trigger::BlockEntityTick { .. }
            )
    }
}

/// 实验录制层的类别编号到运行时白名单的映射。
///
/// 编号见 `recording.rs`：1 方块、5 方块实体、6 红石记忆、7 方块事件、9/10 高度、
/// 19 容器、24 槽位、26 战利品解包、29 计划刻查询、32 ticker、33 实体时刻、
/// 34 世界边界。每个白名单里的读项都在 `finish_record` 里数一遍守卫，
/// 守卫数量对不上就整条记录作废。
pub const fn supported_kind(kind: u8) -> bool {
    matches!(
        kind,
        1 | 5 | 6 | 7 | 9 | 10 | 19 | 24 | 26 | 29 | 32 | 33 | 34
    )
}

/// 白名单：每个变体都在 `guard_value` 里有对应的按类别重算。
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
            | ReadMethod::BlockEntity(_, _, _)
            | ReadMethod::EntityTime(_)
            | ReadMethod::Slots(_, _)
            | ReadMethod::Ticker(0 | 1, _)
            | ReadMethod::LootUnpack(_)
            | ReadMethod::WorldBorder
            | ReadMethod::Redstone(_, 0 | 1 | 2 | 3 | 6)
    )
}

/// 白名单：每个变体都有对应的按值重放效果。
pub const fn supported_write(write: Write) -> bool {
    matches!(
        write,
        Write::Block { .. }
            | Write::TickScheduled { .. }
            | Write::Redstone { kind: 0 | 6, .. }
            | Write::BlockEvent { popped: None, .. }
            | Write::BlockEntity { kind: 0 | 1, .. }
            | Write::Container {
                kind: 0 | 1 | 2,
                ..
            }
            | Write::Effect(_)
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
            | ReplayEffect::PistonMoving { .. }
            | ReplayEffect::RestorePistonMoving { .. }
            | ReplayEffect::RemoveBlockEntity { .. }
            | ReplayEffect::ContainerSlots { .. }
            | ReplayEffect::ClearContainer { .. }
            | ReplayEffect::ForgetBlockEntityMemory { .. }
            | ReplayEffect::UnpackLootTable { .. }
            | ReplayEffect::NoteBlockEntityTick { .. }
            | ReplayEffect::PistonSound { .. }
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
    /// 命中这条记录能省下的解释执行工作量。
    saved_work: u64,
}

impl Entry {
    /// 命中或失配都要付的守卫代价。先比段哈希，失配时只读这些段。
    const fn section_work(&self) -> u64 {
        GUARD_READ_WORK * self.parts.sections.len() as u64
    }

    const fn read_work(&self) -> u64 {
        GUARD_READ_WORK * self.parts.reads.len() as u64
    }
}

/// 每个触发点（归一化后的 `Trigger`）的一本账。
///
/// `gate` 是三态：`Probing` 还在试探要不要录、`Active` 正在录、`Stopped` 停到某个刻。
/// `gain` 是当前窗口内「命中省下的工作量 − 未命中白付的守卫/录制工作量」，
/// 全是确定性计数，不含墙上时间。
#[derive(Debug, Clone, Copy, Default)]
struct Site {
    gate: Gate,
    probe_triggers: u32,
    /// 当前窗口的净收益（工作量单位，可为负）。
    gain: i64,
    window_triggers: u32,
    /// 从打开录制到现在累计命中次数，给新点一点热起来的时间。
    hits: u64,
    /// 连续录到白名单以外的记录的次数。这种触发点重放不了，别一直付录制开销。
    unsupported_streak: u32,
    stopped_until: i64,
    cooldown: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Gate {
    #[default]
    Probing,
    Active,
    Stopped,
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
    /// 第一次跑缓存驱动的时刻，用来给试探留一个窗口。
    started_at: Option<i64>,
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
            started_at: None,
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

    /// 这一刻整个缓存是不是都不用碰。
    ///
    /// 有活跃触发点时永远不休眠，保证大电路的每一刻都能命中；否则在开机后的
    /// 64 刻试探窗口、以及任何冷却到点的触发点上放行一次，其余时刻驱动直接走
    /// 不带录制包装的原路径，常驻开销真的是 0。`min_work = 0` 的诊断配置
    /// （`immediate()`）永不停歇。
    pub fn dormant(&mut self, now: i64) -> bool {
        if self.config.min_work == 0 {
            return false;
        }
        let start = *self.started_at.get_or_insert(now);
        if now.saturating_sub(start) < 64 {
            return false;
        }
        !self.sites.values().any(|state| {
            state.gate == Gate::Active
                || (state.gate == Gate::Stopped && state.stopped_until <= now)
        })
    }

    /// 这个触发点现在值不值得碰缓存。
    ///
    /// 返回 `false` 时调用方直接解释执行，不套录制包装、不建触发帧、不发守卫读项，
    /// 常驻开销只剩这一次查表和计数。冷却结束的触发点会重新进入试探。
    pub fn admits(&mut self, trigger: Trigger) -> bool {
        if !self.config.enabled {
            self.stats.bypassed += 1;
            return false;
        }
        let site = site_key(trigger);
        self.site = site;
        let state = self.sites.entry(site).or_default();
        if state.gate == Gate::Stopped && state.stopped_until <= self.now {
            state.gate = Gate::Probing;
            state.probe_triggers = 0;
        }
        if state.gate == Gate::Stopped {
            self.stats.bypassed += 1;
            false
        } else {
            self.stats.triggers += 1;
            true
        }
    }

    /// 解释执行前问一句：这个触发点现在要不要打开完整录制包装。
    ///
    /// 试探阶段用 `Count` 模式估计读写项数（[`Self::note_probe_work`]），
    /// 单次越过 `min_work` 才切成 `Active`；`immediate()` 把 `min_work` 设成 0，
    /// 于是第一次就录。
    pub fn begin_probe(&mut self) -> bool {
        let site = self.site;
        let state = self.sites.entry(site).or_default();
        if self.config.min_work == 0 && state.gate == Gate::Probing {
            state.gate = Gate::Active;
        }
        state.gate == Gate::Active
    }

    /// 试探阶段一次解释执行结束，把这次的读写项数记进触发点的账。
    ///
    /// `Count` 模式只累加一个计数器，不建帧、不哈希，所以这里是这个触发点
    /// 唯一的常驻开销。单次越过 `min_work` 就切成 `Active`，下次触发开始完整录制；
    /// 连续 `probe_triggers` 次都够不到门槛就判为不划算。
    pub fn note_probe_work(&mut self, work: u64) {
        let site = self.site;
        let config = self.config;
        let Some(state) = self.sites.get_mut(&site) else {
            return;
        };
        if state.gate != Gate::Probing {
            return;
        }
        state.probe_triggers = state.probe_triggers.saturating_add(1);
        if work >= config.min_work {
            state.gate = Gate::Active;
        } else if state.probe_triggers >= config.probe_triggers {
            self.stop_site(site);
        }
    }

    /// 守卫通过时返回该重放的效果序列，调用方不再解释执行。
    ///
    /// `guard` 由调用方提供：它同时借着 `Level` 和 `BlockTicks`，见设计稿 §3.1。
    /// `sections` 也由调用方在命中前复核，见 [`crate::cache_run::run_cached_trigger`]。
    /// 先比段哈希这类一步就能否决的，失配立刻放弃，不再逐项重读。
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
        let mut guard_cost = 0_u64;
        let mut found = None;
        let keys = self.site_keys.get(&site).cloned().unwrap_or_default();
        for key in keys {
            let Some(bucket) = self.entries.get(&key) else {
                continue;
            };
            for entry in bucket {
                if entry.parts.trigger != trigger {
                    continue;
                }
                // 段哈希覆盖整段方块状态：一次查表就能否决绝大多数失配，
                // 不要先逐项重读（那才是旧版最贵的一块常驻开销）。
                guard_cost = guard_cost.saturating_add(entry.section_work());
                if !entry
                    .parts
                    .sections
                    .iter()
                    .all(|s| section(s.section) == Some(s.hash))
                {
                    continue;
                }
                guard_cost = guard_cost.saturating_add(entry.read_work());
                if !guard_matches(entry, guard) {
                    continue;
                }
                self.used.insert(key, self.now);
                found = Some((entry.effects.clone(), entry.saved_work));
                break;
            }
            if found.is_some() {
                break;
            }
        }
        self.note_trigger(guard_cost, found.as_ref().map(|(_, saved)| *saved));
        found.map(|(effects, _)| effects)
    }

    /// 把一条解释执行得到的运行时记录收进缓存。返回是否收下了。
    pub fn record(&mut self, record: &RuntimeRecord) -> bool {
        if !self.config.enabled {
            return false;
        }
        let site = site_key(record.trigger);
        self.site = site;
        let config = self.config;
        let work = record.work();
        let record_cost = RECORD_OVERHEAD_PER_ITEM
            .saturating_mul(work)
            .saturating_add(RECORD_BASE_WORK);
        self.note_cost(record_cost);
        let unsupported = !record.supported();
        let stopped = self
            .sites
            .get(&site)
            .is_some_and(|s| s.gate == Gate::Stopped);
        if work < config.min_work || unsupported || stopped {
            self.stats.rejected += 1;
            // 白名单以外的记录重放不了；连续几次都这样就不该继续为它付完整录制的开销。
            // `immediate()` 是诊断配置（`min_work = 0`），保留它逐题录制、逐题拒绝的行为。
            if unsupported && config.min_work > 0 {
                let mut streak = 0;
                self.sites.entry(site).and_modify(|s| {
                    s.unsupported_streak = s.unsupported_streak.saturating_add(1);
                    streak = s.unsupported_streak;
                });
                if streak >= config.probe_triggers {
                    self.stop_site(site);
                }
            } else if let Some(state) = self.sites.get_mut(&site) {
                state.unsupported_streak = 0;
            }
            return false;
        }
        let mut sections = record.sections.clone();
        sections.sort_by_key(|s| s.section);
        let bytes = entry_bytes(&record.reads, &record.effects, &sections);
        if bytes > config.record_limit {
            self.stats.rejected += 1;
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
            old.saved_work = work;
            self.bytes = self.bytes.saturating_add(delta);
        } else {
            bucket.push(Entry {
                parts,
                effects: record.effects.clone(),
                bytes,
                last_used: self.now,
                saved_work: work,
            });
            self.bytes = self.bytes.saturating_add(bytes);
            self.stats.recorded += 1;
            let keys = self.site_keys.entry(site).or_default();
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        self.used.insert(key, self.now);
        self.trim_memory();
        true
    }

    /// 一次触发记账：命中把省下的工作量记成收益，失配把白付的守卫记成亏损。
    fn note_trigger(&mut self, guard_cost: u64, saved: Option<u64>) {
        let site = self.site;
        let delta = if let Some(saved) = saved {
            self.stats.replays += 1;
            i64::try_from(saved.saturating_sub(guard_cost)).unwrap_or(i64::MAX)
        } else {
            self.stats.misses += 1;
            -i64::try_from(guard_cost).unwrap_or(i64::MAX)
        };
        let state = self.sites.entry(site).or_default();
        state.gain = state.gain.saturating_add(delta);
        state.window_triggers = state.window_triggers.saturating_add(1);
        if saved.is_some() {
            state.hits = state.hits.saturating_add(1);
        }
        if state.window_triggers >= self.config.ledger_window_triggers {
            self.settle_ledger(site);
        }
    }

    /// 录制包装本身的常驻代价，按工作量折成亏损。
    ///
    /// 一次触发最多由 `note_trigger` 结算一次窗口，这里只加账不推进触发计数：
    /// 没命中时守卫和录制是同一次触发的两笔开销。
    fn note_cost(&mut self, cost: u64) {
        let state = self.sites.entry(self.site).or_default();
        state.gain = state
            .gain
            .saturating_sub(i64::try_from(cost).unwrap_or(i64::MAX));
    }

    /// 一个窗口结算：净收益不达门槛就停掉这个触发点，按冷却规则过一阵再试。
    fn settle_ledger(&mut self, site: u128) {
        let Some(state) = self.sites.get(&site).copied() else {
            return;
        };
        self.sites.entry(site).and_modify(|s| {
            s.gain = 0;
            s.window_triggers = 0;
        });
        if state.gate == Gate::Active
            && state.gain < self.config.ledger_min_gain
            && state.hits >= self.config.ledger_min_hits
        {
            self.stop_site(site);
        }
    }

    fn stop_site(&mut self, site: u128) {
        let Some(state) = self.sites.get(&site).copied() else {
            return;
        };
        if state.gate == Gate::Stopped {
            return;
        }
        let base = self.config.cooldown_ticks.clamp(1, 3200);
        let cooldown = base.max(state.cooldown).min(3200);
        self.sites.entry(site).and_modify(|s| {
            s.gate = Gate::Stopped;
            s.stopped_until = self.now + cooldown;
            s.cooldown = (cooldown * 2).min(3200);
            s.gain = 0;
            s.window_triggers = 0;
            s.hits = 0;
            s.unsupported_streak = 0;
            s.probe_triggers = 0;
        });
        self.stats.stopped_sites += 1;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn trigger(index: i32) -> Trigger {
        Trigger::ScheduledTick {
            pos: BlockPos::new(index, 64, 0),
            kind: 1,
        }
    }

    /// 造一条读项数为 `reads`、写项数为 `effects` 的记录，工作量 = reads + effects + sections。
    fn record(trigger: Trigger, reads: usize, effects: usize, value: u64) -> RuntimeRecord {
        RuntimeRecord {
            trigger,
            tick: 0,
            nanos: 0,
            interpreted_items: reads as u64,
            written_items: effects as u64,
            reads: (0..reads)
                .map(|i| GuardRead {
                    method: ReadMethod::MinY,
                    pos: None,
                    value: value.wrapping_add(i as u64),
                })
                .collect(),
            effects: (0..effects)
                .map(|_| ReplayEffect::Comparator {
                    pos: BlockPos::new(0, 64, 0),
                    signal: 1,
                })
                .collect(),
            sections: Vec::new(),
            uncacheable: Vec::new(),
            boundary_writes: false,
            rejected: false,
        }
    }

    /// 一次守卫/重放：所有守卫项都给 `value`，段哈希全给 0。
    fn trigger_once(cache: &mut RedstoneCache, trigger: Trigger, value: u64) -> bool {
        cache
            .try_replay(trigger, &mut |_| Some(value), &|_| Some(0))
            .is_some()
    }

    /// 便宜的触发点不录：试探窗口里每次解释执行只有几条读项，攒不够门槛。
    #[test]
    fn cheap_sites_are_never_recorded() {
        let mut cache = RedstoneCache::new(CacheConfig {
            enabled: true,
            min_work: 100,
            probe_triggers: 4,
            ledger_min_gain: i64::MIN,
            ..CacheConfig::default()
        });
        for _ in 0..4 {
            assert!(cache.admits(trigger(0)));
            assert!(!cache.begin_probe(), "便宜的触发点不该打开录制包装");
            cache.note_probe_work(3);
        }
        assert_eq!(cache.stats().recorded, 0);
        assert!(!cache.admits(trigger(0)), "试探失败后应进冷却");
        assert!(cache.stats().bypassed > 0);
    }

    /// 够贵的触发点开录，并且只录工作量超过 min_work 的记录。
    #[test]
    fn expensive_sites_open_recording_and_gate_each_record() {
        let mut cache = RedstoneCache::new(CacheConfig {
            enabled: true,
            min_work: 10,
            ledger_min_gain: i64::MIN,
            ..CacheConfig::default()
        });
        assert!(cache.admits(trigger(0)));
        assert!(!cache.begin_probe(), "第一次还在试探");
        cache.note_probe_work(20);
        assert!(cache.admits(trigger(0)));
        assert!(cache.begin_probe(), "累计够贵就该开录");
        assert!(!cache.record(&record(trigger(0), 2, 0, 1)), "单条太便宜");
        assert!(cache.record(&record(trigger(0), 20, 0, 1)));
        assert_eq!(cache.stats().recorded, 1);
    }

    /// 净收益为负的触发点会停，冷却结束后重新试探。
    #[test]
    fn a_site_with_negative_gain_stops_and_retries_after_cooldown() {
        let mut cache = RedstoneCache::new(CacheConfig {
            enabled: true,
            min_work: 0,
            ledger_window_triggers: 4,
            ledger_min_gain: 0,
            ledger_min_hits: 0,
            cooldown_ticks: 5,
            ..CacheConfig::default()
        });
        // 一条很便宜的记录：守卫代价（1 项）和录制代价（8+1）都比命中省下的（1）大。
        assert!(cache.admits(trigger(0)));
        assert!(cache.begin_probe(), "immediate 配置第一次就录");
        assert!(cache.record(&record(trigger(0), 1, 0, 1)));
        for now in 1..=4 {
            cache.set_time(now);
            assert!(cache.admits(trigger(0)), "第 {now} 次还没到窗口结算");
            assert!(cache.begin_probe());
            assert!(
                !trigger_once(&mut cache, trigger(0), 999),
                "守卫换值才能失配"
            );
            let _ = cache.record(&record(trigger(0), 1, 0, 1));
        }
        assert_eq!(cache.stats().stopped_sites, 1);
        assert!(!cache.admits(trigger(0)), "停掉后直接解释执行");
        cache.set_time(10);
        assert!(cache.admits(trigger(0)), "冷却结束重新试探");
    }

    /// 净收益为正的触发点一直留着。
    #[test]
    fn a_site_with_positive_gain_stays_active() {
        let mut cache = RedstoneCache::new(CacheConfig {
            enabled: true,
            min_work: 0,
            ledger_window_triggers: 4,
            ledger_min_gain: -100_000,
            ..CacheConfig::default()
        });
        assert!(cache.admits(trigger(0)));
        assert!(cache.begin_probe());
        cache.record(&record(trigger(0), 1000, 0, 7));
        for now in 1..=8 {
            cache.set_time(now);
            assert!(cache.admits(trigger(0)));
            assert!(cache.begin_probe());
            assert!(
                cache
                    .try_replay(trigger(0), &mut |read| Some(read.value), &|_| Some(0))
                    .is_some()
            );
        }
        assert_eq!(cache.stats().stopped_sites, 0);
        assert_eq!(cache.stats().replays, 8);
    }

    /// 段哈希失配时不再逐项重读；守卫项数就是段数。
    #[test]
    fn section_mismatch_skips_itemized_guard_reads() {
        let mut cache = RedstoneCache::new(CacheConfig {
            enabled: true,
            min_work: 0,
            ledger_min_gain: i64::MIN,
            ..CacheConfig::default()
        });
        assert!(cache.admits(trigger(0)));
        assert!(cache.begin_probe());
        let mut r = record(trigger(0), 3, 0, 7);
        r.sections.push(SectionKey {
            section: ChunkSection {
                chunk: mc_bamboo_core::ChunkPos::new(0, 0),
                section_y: 4,
            },
            hash: 42,
        });
        assert!(cache.record(&r));
        let mut reads = 0;
        assert!(
            cache
                .try_replay(
                    trigger(0),
                    &mut |_| {
                        reads += 1;
                        Some(7)
                    },
                    &|_| Some(0)
                )
                .is_none(),
            "段哈希不匹配必须退回解释"
        );
        assert_eq!(reads, 0, "段哈希一步否决，不该逐项重读");
    }

    /// 超过内存上限按最近使用淘汰，降到 80%。
    #[test]
    fn memory_limit_evicts_least_recently_used_entries() {
        let mut cache = RedstoneCache::new(CacheConfig {
            enabled: true,
            min_work: 0,
            memory_limit: 1000,
            record_limit: usize::MAX,
            ledger_min_gain: i64::MIN,
            ..CacheConfig::default()
        });
        for index in 0_u64..40 {
            cache.set_time(index as i64);
            let t = trigger(index as i32);
            cache.admits(t);
            cache.begin_probe();
            cache.record(&record(t, 4, 0, index));
        }
        assert!(cache.bytes() <= 800);
        assert!(cache.stats().evictions > 0);
        assert!(cache.entry_count() < 40);
    }

    /// 键原料逐项比对：128 位哈希相同但读项不同的两条记录不会互相覆盖。
    #[test]
    fn key_parts_are_compared_in_full() {
        let mut cache = RedstoneCache::new(CacheConfig {
            enabled: true,
            min_work: 0,
            ledger_min_gain: i64::MIN,
            ..CacheConfig::default()
        });
        cache.admits(trigger(0));
        cache.begin_probe();
        cache.record(&record(trigger(0), 1, 0, 1));
        cache.record(&record(trigger(0), 1, 0, 2));
        assert_eq!(cache.entry_count(), 2);
    }
}
