//! 只在本 crate 的测试里编译的分段计时：每头牛每刻花在移动（行走、方块效果、推挤）、
//! 目标、导航和寻路上的时间。
//! 正式构建里 [`span!`] 展开成空，不占任何开销。
//!
//! Test-only split timing of a cow's tick into movement, goals, navigation and
//! pathfinding. In real builds [`span!`] expands to nothing.
//!
//! 压测另外记「按生物种类」「按离最近玩家的距离档」「寻路请求与展开节点」「推挤命令
//! 与挤压伤害」这几组计数，见 [`imp::counters`]。探针全部只在 `cfg(test)` 下存在，
//! 关掉时正式构建连一次原子操作都没有。

/// 压测的 A/B 开关：`MOB_STRESS_NAIVE=1` 让三项优化都走朴素路径，
/// 同一个二进制就能量到「改之前 / 改之后」。正式构建里恒为假。
///
/// A/B switch for benchmarks: `MOB_STRESS_NAIVE=1` sends all three optimisations
/// down their naive paths, so one binary measures both sides. Always false in
/// production builds.
#[cfg(test)]
pub(crate) fn stress_naive() -> bool {
    static NAIVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *NAIVE.get_or_init(|| std::env::var_os("MOB_STRESS_NAIVE").is_some())
}

#[cfg(not(test))]
#[inline(always)]
pub(crate) const fn stress_naive() -> bool {
    false
}

#[cfg(test)]
pub(crate) mod imp {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::time::Instant;

    /// 计时的几段。每段记「自身」时间，里面嵌套的段从外层扣掉。
    ///
    /// Timed sections. Each records self time; nested sections are deducted from the outer one.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Part {
        /// 整个生物 tick 里没归到下面几段的部分（基础 tick、受伤、年龄……）。
        Other,
        /// `travel`：走、重力、碰撞。
        Travel,
        /// `applyEffectsFromBlocks`：踩到和身处的方块效果。
        BlockEffects,
        /// `pushEntities`。
        Push,
        /// 目标选择器（感知、挑攻击目标、视线）。
        Targeting,
        /// 目标 / 大脑决策（`GoalSelector`）。
        Goals,
        /// 导航跟随、移动 / 视线控制（不含重算路径的寻路）。
        Navigation,
        /// `PathFinder.findPath`。
        Path,
        /// 大脑传感器扫描（`Brain::tick_by` 里的传感器循环）。
        BrainSensors,
        /// 大脑行为启动与推进（`startEachNonRunning` + `tickEachRunning`）。
        BrainBehaviors,
        /// POI 查询（`PoiStore` 查询接口）。
        Poi,
    }

    pub(crate) const PARTS: usize = 11;

    /// 离最近玩家的水平距离档，对应设计稿 4.2 的三档。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum DistanceBand {
        Within32,
        From32To64,
        Beyond64,
    }

    pub(crate) const BANDS: usize = 3;

    impl DistanceBand {
        pub(crate) fn of(distance_sqr: f64) -> Self {
            if distance_sqr <= 32.0 * 32.0 {
                Self::Within32
            } else if distance_sqr <= 64.0 * 64.0 {
                Self::From32To64
            } else {
                Self::Beyond64
            }
        }

        pub(crate) fn index(self) -> usize {
            match self {
                Self::Within32 => 0,
                Self::From32To64 => 1,
                Self::Beyond64 => 2,
            }
        }
    }

    /// 一次寻路请求的去重键：起点格、终点、体型（宽高）和导航类型。
    /// 官方的 `createPath` 还会用 `target_pos` 复用整条路径；这里只统计真正进了
    /// `find_path` 的请求，正是以后独立寻路阶段要批量算的那些。
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub(crate) struct PathKey {
        pub start: (i32, i32, i32),
        pub target: (i32, i32, i32),
        pub size: (u32, u32),
        pub nav: u16,
    }

    /// 一次寻路的结果计数；`nodes` 是 `PathFinder.run` 里弹出的节点数。
    #[derive(Debug, Clone, Copy, Default)]
    pub(crate) struct PathStats {
        pub requests: u64,
        pub succeeded: u64,
        pub failed: u64,
        pub nodes: u64,
        pub max_nodes: u64,
        /// 按请求发出顺序、单任务内去重能省掉的请求数（保守下界）。
        pub duplicates: u64,
        /// 全刻全局去重能省掉的请求数。
        pub global_duplicates: u64,
    }

    impl PathStats {
        const fn new() -> Self {
            Self {
                requests: 0,
                succeeded: 0,
                failed: 0,
                nodes: 0,
                max_nodes: 0,
                duplicates: 0,
                global_duplicates: 0,
            }
        }
    }

    /// 每种生物的分段时间和实体刻数；`MobType` 用注册表 ID 当键，避免和表里的顺序耦合。
    #[derive(Debug, Clone, Copy, Default)]
    pub(crate) struct KindStats {
        pub ticks: u64,
        pub parts: [u64; PARTS],
    }

    /// 一个距离档里这一刻参与 tick 的生物数、按段拆的时间。
    #[derive(Debug, Clone, Copy, Default)]
    pub(crate) struct BandStats {
        pub ticks: u64,
        pub parts: [u64; PARTS],
    }

    impl BandStats {
        const fn new() -> Self {
            Self {
                ticks: 0,
                parts: [0; PARTS],
            }
        }
    }

    #[derive(Default)]
    struct Counters {
        kinds: HashMap<i32, KindStats>,
        bands: [BandStats; BANDS],
        paths: PathStats,
        /// 这一刻的寻路去重键；每次 `new_tick` 清空。并行驱动下各算阶段线程各持一份，
        /// 去重数因此是「单任务内」的保守下界，不是全局精确值。
        path_keys: HashMap<PathKey, u64>,
        /// `pushEntities` 发出的 `PushMob` 命令数。
        push_commands: u64,
        /// 挤压伤害（`maxEntityCramming`）真正触发的次数。
        cramming_hits: u64,
        /// 大脑执行次数（`Brain::tick_by`）。
        brain_ticks: u64,
        /// 传感器倒计时归零后真正执行扫描的次数。
        sensor_scans: u64,
        /// 大脑成功启动的行为次数。
        behavior_starts: u64,
        /// POI 查询次数。
        poi_queries: u64,
        /// 结构刷怪覆盖查询次数和累计耗时（纳秒）。
        structure_queries: u64,
        structure_query_ns: u64,
        /// 目标搜索真正访问到的候选数。
        target_candidates: u64,
        /// 视线射线次数、缓存命中次数。
        sight_rays: u64,
        sight_cache_hits: u64,
    }

    #[derive(Default)]
    struct State {
        totals: [u64; PARTS],
        /// 打开着的段：(哪段, 开始时刻, 子段已用纳秒)。
        stack: Vec<(usize, Instant, u64)>,
        /// 当前实体：注册表的生物类型 ID、离最近玩家的距离档。
        /// `None` 表示这个 tick 不该记账（不是生物，或者计时没打开）。
        current: Option<(i32, DistanceBand)>,
        counters: Counters,
    }

    thread_local! {
        static STATE: RefCell<State> = RefCell::default();
    }

    /// 离开作用域时结束一段。
    pub(crate) struct Span(());

    pub(crate) fn begin(part: Part) -> Span {
        STATE.with(|state| {
            state
                .borrow_mut()
                .stack
                .push((part as usize, Instant::now(), 0));
        });
        Span(())
    }

    impl Drop for Span {
        fn drop(&mut self) {
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                let (part, start, children) = state.stack.pop().expect("span stack");
                let elapsed = start.elapsed().as_nanos() as u64;
                let own = elapsed.saturating_sub(children);
                state.totals[part] += own;
                // 只有最外层 `Other` 结束（整头生物 tick 完）时才记一次实体刻；
                // 内层段的时间在各自的 `Span` 里单独记，避免重复计数。
                if let Some((kind, band)) = state.current {
                    let kind_stats = state.counters.kinds.entry(kind).or_default();
                    if part == Part::Other as usize {
                        kind_stats.ticks += 1;
                    }
                    kind_stats.parts[part] += own;
                    state.counters.bands[band.index()].parts[part] += own;
                    if part == Part::Other as usize {
                        state.counters.bands[band.index()].ticks += 1;
                    }
                }
                if let Some(parent) = state.stack.last_mut() {
                    parent.2 += elapsed;
                }
            });
        }
    }

    /// 每个被测实体 tick 开头调一次：记下类型和到最近玩家的距离档。
    /// `distance_sqr` 是水平距离平方；没有玩家时按 64 格外算（`INFINITY`）。
    pub(crate) fn begin_mob(kind: i32, distance_sqr: f64) {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.current = Some((kind, DistanceBand::of(distance_sqr)));
        });
    }

    /// 每个实体 tick 结尾调一次，清掉当前类型标记。
    pub(crate) fn end_mob() {
        STATE.with(|state| {
            state.borrow_mut().current = None;
        });
    }

    /// 一刻开头清空全局去重集合；别把跨刻的请求当成重复。
    pub(crate) fn new_tick() {
        STATE.with(|state| {
            state.borrow_mut().counters.path_keys.clear();
        });
        *TICK_KEYS.lock().expect("path key mutex") = Some(std::collections::HashSet::new());
    }

    /// 一条进入 A\* 的寻路请求。`key` 相同的只算一次，用于估计去重的上限。
    pub(crate) fn record_path(key: PathKey, nodes: u64, succeeded: bool) {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let counters = &mut state.counters;
            let paths = &mut counters.paths;
            paths.requests += 1;
            if succeeded {
                paths.succeeded += 1;
            } else {
                paths.failed += 1;
            }
            paths.nodes += nodes;
            paths.max_nodes = paths.max_nodes.max(nodes);
            let seen = counters.path_keys.entry(key).or_insert(0);
            if *seen > 0 {
                paths.duplicates += 1;
            }
            *seen += 1;
            // 另一套全局口径：跨算阶段任务也能去重。两条都记，报告里用全局值。
            if let Some(all) = TICK_KEYS.lock().expect("path key mutex").as_mut()
                && !all.insert(key)
            {
                paths.global_duplicates += 1;
            }
        });
    }

    /// `pushEntities` 发出的 `PushMob` 命令条数。
    pub(crate) fn record_push(commands: u64) {
        STATE.with(|state| {
            state.borrow_mut().counters.push_commands += commands;
        });
    }

    /// 挤压伤害触发一次。
    pub(crate) fn record_cramming() {
        STATE.with(|state| {
            state.borrow_mut().counters.cramming_hits += 1;
        });
    }

    /// 大脑运行一刻。
    pub(crate) fn record_brain_tick() {
        STATE.with(|state| {
            state.borrow_mut().counters.brain_ticks += 1;
        });
    }

    /// 传感器执行一次真实扫描。
    pub(crate) fn record_sensor_scan() {
        STATE.with(|state| {
            state.borrow_mut().counters.sensor_scans += 1;
        });
    }

    /// 大脑成功启动一条行为。
    pub(crate) fn record_behavior_start() {
        STATE.with(|state| {
            state.borrow_mut().counters.behavior_starts += 1;
        });
    }

    /// 执行一次 POI 查询。
    pub(crate) fn record_poi_query() {
        STATE.with(|state| {
            state.borrow_mut().counters.poi_queries += 1;
        });
    }

    /// 一次结构刷怪覆盖查询及其耗时（纳秒）。
    pub(crate) fn record_structure_query(nanos: u64) {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.counters.structure_queries += 1;
            state.counters.structure_query_ns += nanos;
        });
    }

    /// 目标搜索访问到的候选实体数。
    pub(crate) fn record_target_candidates(count: u64) {
        STATE.with(|state| {
            state.borrow_mut().counters.target_candidates += count;
        });
    }

    /// 一次视线检查：`hit` 为真表示复用了跨刻缓存，没有真跑射线。
    pub(crate) fn record_sight(hit: bool) {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.counters.sight_rays += 1;
            if hit {
                state.counters.sight_cache_hits += 1;
            }
        });
    }

    /// 取走全部线程累计的自身时间（纳秒），按 [`Part`] 的顺序。
    /// 并行算阶段各线程的时间在 [`flush_counters`] 里并进来，所以这里是 CPU 时间之和，
    /// 不是墙上时间。
    pub(crate) fn take() -> [u64; PARTS] {
        let mut totals = ALL_TOTALS.lock().expect("timing mutex");
        std::mem::take(&mut *totals)
    }

    /// 把当前线程的分段时间和附加计数并进全局汇总，供并行驱动的多个算阶段线程共用。
    pub(crate) fn flush_counters() {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let own = std::mem::take(&mut state.totals);
            let counters = std::mem::take(&mut state.counters);
            let mut totals = ALL_TOTALS.lock().expect("timing mutex");
            for (to, from) in totals.iter_mut().zip(own) {
                *to += from;
            }
            drop(totals);
            let mut all = ALL_COUNTERS.lock().expect("counter mutex");
            all.absorb(counters);
        });
    }

    /// 取走全部线程的附加计数并清空（不清空分段时间，`take` 单独调用）。
    /// 基准在每一刻并行算阶段结束、算阶段线程都收工之后调。
    pub(crate) fn take_all_counters() -> Snapshot {
        let mut all = ALL_COUNTERS.lock().expect("counter mutex");
        std::mem::take(&mut *all)
    }

    static ALL_COUNTERS: std::sync::Mutex<Snapshot> = std::sync::Mutex::new(Snapshot::new());
    /// 一刻内所有算阶段任务共享的去重键；`new_tick` 每刻清空。
    static TICK_KEYS: std::sync::Mutex<Option<std::collections::HashSet<PathKey>>> =
        std::sync::Mutex::new(None);
    static ALL_TOTALS: std::sync::Mutex<[u64; PARTS]> = std::sync::Mutex::new([0; PARTS]);

    #[derive(Debug, Default)]
    pub(crate) struct Snapshot {
        /// `None` 表示静态初始值（空表还没实例化）；压测读它时用 `kinds()`。
        pub kinds: Option<HashMap<i32, KindStats>>,
        pub bands: [BandStats; BANDS],
        pub paths: PathStats,
        pub push_commands: u64,
        pub cramming_hits: u64,
        pub brain_ticks: u64,
        pub sensor_scans: u64,
        pub behavior_starts: u64,
        pub poi_queries: u64,
        pub structure_queries: u64,
        pub structure_query_ns: u64,
        pub target_candidates: u64,
        pub sight_rays: u64,
        pub sight_cache_hits: u64,
    }

    impl Snapshot {
        const fn new() -> Self {
            Self {
                // `HashMap::new` 不是 const；缺省构造出来的空表放在 `Option` 里，
                // 第一次 `absorb` 时初始化。静态初始化因此不跑任何代码。
                kinds: None,
                bands: [BandStats::new(); BANDS],
                paths: PathStats::new(),
                push_commands: 0,
                cramming_hits: 0,
                brain_ticks: 0,
                sensor_scans: 0,
                behavior_starts: 0,
                poi_queries: 0,
                structure_queries: 0,
                structure_query_ns: 0,
                target_candidates: 0,
                sight_rays: 0,
                sight_cache_hits: 0,
            }
        }

        pub(crate) fn kinds(&self) -> impl Iterator<Item = (&i32, &KindStats)> {
            self.kinds.iter().flatten()
        }

        fn absorb(&mut self, other: Counters) {
            for (kind, stats) in other.kinds {
                let entry = self
                    .kinds
                    .get_or_insert_with(HashMap::new)
                    .entry(kind)
                    .or_default();
                entry.ticks += stats.ticks;
                for (to, from) in entry.parts.iter_mut().zip(stats.parts) {
                    *to += from;
                }
            }
            for (to, from) in self.bands.iter_mut().zip(other.bands) {
                to.ticks += from.ticks;
                for (a, b) in to.parts.iter_mut().zip(from.parts) {
                    *a += b;
                }
            }
            let to = &mut self.paths;
            let from = other.paths;
            to.requests += from.requests;
            to.succeeded += from.succeeded;
            to.failed += from.failed;
            to.nodes += from.nodes;
            to.max_nodes = to.max_nodes.max(from.max_nodes);
            to.duplicates += from.duplicates;
            to.global_duplicates += from.global_duplicates;
            self.push_commands += other.push_commands;
            self.cramming_hits += other.cramming_hits;
            self.brain_ticks += other.brain_ticks;
            self.sensor_scans += other.sensor_scans;
            self.behavior_starts += other.behavior_starts;
            self.poi_queries += other.poi_queries;
            self.structure_queries += other.structure_queries;
            self.structure_query_ns += other.structure_query_ns;
            self.target_candidates += other.target_candidates;
            self.sight_rays += other.sight_rays;
            self.sight_cache_hits += other.sight_cache_hits;
        }
    }
}

/// `span!(Movement)`：测试构建里计时到作用域结束，正式构建里什么也不做。
#[cfg(test)]
macro_rules! span {
    ($part:ident) => {
        let _span =
            $crate::entity::mob::timing::imp::begin($crate::entity::mob::timing::imp::Part::$part);
    };
}

/// `begin_mob!(kind, distance_sqr)`：每头生物 tick 开头记类型和距离档。
#[cfg(test)]
macro_rules! begin_mob {
    ($kind:expr, $distance_sqr:expr) => {
        $crate::entity::mob::timing::imp::begin_mob($kind, $distance_sqr);
    };
}

/// `end_mob!()`：每头生物 tick 结尾清掉类型标记。
#[cfg(test)]
macro_rules! end_mob {
    () => {
        $crate::entity::mob::timing::imp::end_mob();
    };
}

/// `record_path!(key, nodes, succeeded)`：一条进入 A\* 的请求的结果。
#[cfg(test)]
macro_rules! record_path {
    ($key:expr, $nodes:expr, $succeeded:expr) => {
        $crate::entity::mob::timing::imp::record_path($key, $nodes, $succeeded);
    };
}

/// `record_push!(n)`：`pushEntities` 发出的推挤命令数。
#[cfg(test)]
macro_rules! record_push {
    ($n:expr) => {
        $crate::entity::mob::timing::imp::record_push($n);
    };
}

/// `record_cramming!()`：挤压伤害触发一次。
#[cfg(test)]
macro_rules! record_cramming {
    () => {
        $crate::entity::mob::timing::imp::record_cramming();
    };
}

/// `new_tick!()`：一刻开头清空寻路去重集合。
#[cfg(test)]
macro_rules! new_tick {
    () => {
        $crate::entity::mob::timing::imp::new_tick();
    };
}

/// `record_brain_tick!()`：大脑执行一刻。
#[cfg(test)]
macro_rules! record_brain_tick {
    () => {
        $crate::entity::mob::timing::imp::record_brain_tick();
    };
}

/// `record_sensor_scan!()`：传感器真正执行一次扫描。
#[cfg(test)]
macro_rules! record_sensor_scan {
    () => {
        $crate::entity::mob::timing::imp::record_sensor_scan();
    };
}

/// `record_behavior_start!()`：大脑成功启动一条行为。
#[cfg(test)]
macro_rules! record_behavior_start {
    () => {
        $crate::entity::mob::timing::imp::record_behavior_start();
    };
}

/// `record_poi_query!()`：执行一次 POI 查询。
#[cfg(test)]
macro_rules! record_poi_query {
    () => {
        $crate::entity::mob::timing::imp::record_poi_query();
    };
}

/// `record_structure_query!(nanos)`：一次结构刷怪覆盖查询。
#[cfg(test)]
macro_rules! record_structure_query {
    ($nanos:expr) => {
        $crate::entity::mob::timing::imp::record_structure_query($nanos);
    };
}

/// `record_target_candidates!(n)`：目标搜索访问到的候选数。
#[cfg(test)]
macro_rules! record_target_candidates {
    ($n:expr) => {
        $crate::entity::mob::timing::imp::record_target_candidates($n);
    };
}

/// `record_sight!(hit)`：一次视线检查及是否命中缓存。
#[cfg(test)]
macro_rules! record_sight {
    ($hit:expr) => {
        $crate::entity::mob::timing::imp::record_sight($hit);
    };
}

/// 正式构建里的同名空宏：每个调用点都展开成一条空语句，零开销、零行为变化。
/// 定义本身没有被用到不算问题。
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! span {
    ($part:ident) => {};
}
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! begin_mob {
    ($kind:expr, $distance_sqr:expr) => {};
}
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! end_mob {
    () => {};
}
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! record_path {
    ($key:expr, $nodes:expr, $succeeded:expr) => {};
}
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! record_push {
    ($n:expr) => {};
}
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! record_cramming {
    () => {};
}
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! new_tick {
    () => {};
}
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! record_brain_tick {
    () => {};
}
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! record_sensor_scan {
    () => {};
}
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! record_behavior_start {
    () => {};
}
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! record_poi_query {
    () => {};
}
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! record_structure_query {
    ($nanos:expr) => {};
}
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! record_target_candidates {
    ($n:expr) => {};
}
#[cfg(not(test))]
#[allow(unused_macros)]
macro_rules! record_sight {
    ($hit:expr) => {};
}

#[allow(unused_imports)]
pub(crate) use begin_mob;
pub(crate) use end_mob;
#[allow(unused_imports)]
pub(crate) use new_tick;
#[allow(unused_imports)]
pub(crate) use record_behavior_start;
#[allow(unused_imports)]
pub(crate) use record_brain_tick;
pub(crate) use record_cramming;
#[allow(unused_imports)]
pub(crate) use record_path;
#[allow(unused_imports)]
pub(crate) use record_poi_query;
pub(crate) use record_push;
#[allow(unused_imports)]
pub(crate) use record_sensor_scan;
#[allow(unused_imports)]
pub(crate) use record_sight;
#[allow(unused_imports)]
pub(crate) use record_structure_query;
#[allow(unused_imports)]
pub(crate) use record_target_candidates;
pub(crate) use span;

#[cfg(test)]
mod bench {
    use std::time::Instant;

    use mc_bamboo_core::{BlockPos, Vec3};
    use mc_bamboo_data::blocks;

    use super::imp::{self, PARTS};
    use crate::{
        BlockTicks,
        entity::{Entity, ctx::PlayerView},
        state::BlockState,
        testing::TestLevel,
        world::World,
    };

    /// 平地草方块上放 `count` 头牛，先跑 `WARM` 刻让它们散开，再量 `MEASURE` 刻。
    /// 测试世界的方块在一张散列表里，查方块比正式区块存储慢，数字偏保守。
    ///
    /// `count` cows on flat grass; warm up, then measure. The test level keeps
    /// blocks in a hash map, slower than real chunk storage, so numbers lean high.
    fn run(count: usize) {
        const WARM: usize = 200;
        const MEASURE: usize = 600;
        let side = (count as f64).sqrt().ceil() as i32;
        let spacing = 3;
        let half = side * spacing / 2 + 24;
        let mut level = TestLevel::new();
        for x in -half..half {
            for z in -half..half {
                level.set(BlockPos::new(x, 62, z), BlockState(blocks::DIRT));
                level.set(BlockPos::new(x, 63, z), BlockState(blocks::GRASS_BLOCK));
            }
        }
        // 每 32 格一名玩家站着不动：离玩家 32 格以外的牛 `noActionTime` 一直涨，
        // 到 100 就不再闲逛，那样量不到寻路。
        // A still player every 32 blocks; cows farther than 32 blocks from any
        // player stop strolling once `noActionTime` reaches 100.
        for px in (-half..half).step_by(32) {
            for pz in (-half..half).step_by(32) {
                level.players.push(PlayerView {
                    main_arm_right: true,
                    fall_flying: false,
                    shoulder_left: None,
                    shoulder_right: None,
                    uuid: None,
                    uid: 1_000_000 + level.players.len() as i32,
                    yaw: 0.0,
                    pitch: 0.0,
                    position: Vec3::new(f64::from(px) + 16.5, 64.0, f64::from(pz) + 16.5),
                    delta: Vec3::default(),
                    width: 0.6,
                    height: 1.8,
                    eye_height: 1.62,
                    health: 20.0,
                    look_direction: Vec3::default(),
                    gaze_disguise: false,
                    alive: true,
                    spectator: false,
                    game_mode: 0,
                    mining_fatigue: None,
                    creative: false,
                    flying: false,
                    explosion_knockback_resistance: 0.0,
                    sneaking: false,
                    sprinting: false,
                    swimming: false,
                    invisible: false,
                    armor_cover: 0.0,
                    main_hand: 0,
                    off_hand: 0,
                    team: None,
                    combat: Default::default(),
                    effects: Default::default(),
                });
            }
        }
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        for i in 0..count as i32 {
            let x = (i % side) * spacing - side * spacing / 2;
            let z = (i / side) * spacing - side * spacing / 2;
            world.spawn_entity(Entity::cow(Vec3::new(
                f64::from(x) + 0.5,
                64.0,
                f64::from(z) + 0.5,
            )));
        }
        for _ in 0..WARM {
            imp::new_tick();
            world.tick_entities();
        }
        imp::take();
        let _ = imp::take_all_counters();
        let start = Instant::now();
        for _ in 0..MEASURE {
            imp::new_tick();
            world.tick_entities();
        }
        let wall = start.elapsed().as_nanos() as f64;
        let parts = imp::take();
        let per = |ns: f64| ns / (count * MEASURE) as f64 / 1000.0;
        let names = [
            "other",
            "travel",
            "block-effects",
            "push",
            "targeting",
            "goals",
            "navigation",
            "path",
            "brain-sensors",
            "brain-behaviors",
            "poi",
        ];
        let timed: u64 = parts.iter().sum();
        let mut line = format!(
            "{count} cows: wall {:.3} us/cow/tick, timed {:.3}",
            per(wall),
            per(timed as f64)
        );
        for i in 0..PARTS {
            line += &format!(", {} {:.3}", names[i], per(parts[i] as f64));
        }
        println!("{line}");
    }

    /// `cargo test --release -p mc-bamboo-blocks cow_tick_cost -- --ignored --nocapture`
    #[test]
    #[ignore = "timing run, not a check"]
    fn cow_tick_cost() {
        run(100);
        run(1000);
    }
}

#[cfg(test)]
mod stress;
