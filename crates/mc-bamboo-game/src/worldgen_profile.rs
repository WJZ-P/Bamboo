//! 真实 Director 流水线的测量插桩：阶段耗时、快照命中、地物结果缓存命中。
//!
//! 平时所有计数器都关着，热路径只多一次 Relaxed 的布尔读；只有
//! `examples/director_pipeline_bench` 显式 `reset`/开启后才计数和记时。测量结果从
//! 计算线程汇总到进程级原子量，最后在世界线程读一次。
//!
//! Instrumentation for the real Director pipeline: stage timings, snapshot hit
//! ratio and feature-run cache behaviour. All counters are off unless the
//! benchmark example explicitly enables them; the hot path then costs one
//! Relaxed boolean load. Workers aggregate into process-wide atomics.

use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

/// 阶段计时开关；只有测量进程会打开。
/// Master switch; only the measurement process turns it on.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// 每个指标一个原子量，单位纳秒或次数。
/// One atomic per metric, nanoseconds or counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProfileStats {
    pub terrain_ns: u64,
    pub features_ns: u64,
    pub light_ns: u64,
    pub terrain_calls: u64,
    pub features_calls: u64,
    pub light_calls: u64,
    pub build_view_ns: u64,
    pub run_features_ns: u64,
    pub view_resolve_ns: u64,
    /// 拼 3×3 视图时落到 Director 快照 / 现算的格子数。
    pub view_cells: u64,
    pub snapshot_some: u64,
    pub snapshot_none: u64,
    /// 取视图时问过地形缓存的格子数（`build_view` 的 9 格 + 补缺的格子）。
    pub terrain_lookups: u64,
    pub terrain_hits: u64,
    pub terrain_misses: u64,
    /// `resolve_view`（Director 快照不足时）与 `build_view` 各自的未命中数。
    pub view_terrain_misses: u64,
    pub build_view_terrain_misses: u64,
    /// `cached_feature_run` 总调用 / 命中 / 现算。
    pub feature_run_calls: u64,
    pub feature_run_hits: u64,
    pub feature_run_misses: u64,
    /// 真正现算时，有多少次走了 `build_view` 的 9 格。
    pub build_view_calls: u64,
    pub build_view_cells: u64,
    /// `run_features` 的独立调用次数（含中心自己的 features 阶段）。
    pub run_features_calls: u64,
    /// 每个中心被 `cached_feature_run` 以多少个不同中心调用过（去重后的分布）。
    /// 由 `Centers` 测量器在插桩打开的进程里按中心累计。
    pub distinct_feature_centers: u64,
    pub feature_centers_called_once: u64,
    pub feature_centers_max_calls: u64,
}

macro_rules! counters {
    ($($upper:ident => $lower:ident),* $(,)?) => {
        $(pub(crate) static $upper: AtomicU64 = AtomicU64::new(0);)*

        /// 读一份累计快照。
        /// Read the accumulated snapshot.
        pub fn stats() -> ProfileStats {
            let mut out = ProfileStats {
                $($lower: $upper.load(Ordering::Relaxed),)*
                ..ProfileStats::default()
            };
            let centers = feature_centers_snapshot();
            out.distinct_feature_centers = centers.count as u64;
            out.feature_centers_called_once = centers.called_once as u64;
            out.feature_centers_max_calls = centers.max_calls as u64;
            out
        }

        /// 清零全部计数并打开插桩。
        /// Reset every counter and turn instrumentation on.
        pub fn reset() {
            $($upper.store(0, Ordering::Relaxed);)*
            FEATURE_CENTERS.lock().unwrap_or_else(|e| e.into_inner()).clear();
            ENABLED.store(true, Ordering::Relaxed);
        }

        /// 关闭插桩（测量结束时调，避免进程里别的活继续付计时开销）。
        /// Turn instrumentation off after a measurement run.
        pub fn disable() {
            ENABLED.store(false, Ordering::Relaxed);
        }
    };
}

counters!(
    TERRAIN_NS => terrain_ns,
    FEATURES_NS => features_ns,
    LIGHT_NS => light_ns,
    TERRAIN_CALLS => terrain_calls,
    FEATURES_CALLS => features_calls,
    LIGHT_CALLS => light_calls,
    BUILD_VIEW_NS => build_view_ns,
    RUN_FEATURES_NS => run_features_ns,
    VIEW_RESOLVE_NS => view_resolve_ns,
    VIEW_CELLS => view_cells,
    SNAPSHOT_SOME => snapshot_some,
    SNAPSHOT_NONE => snapshot_none,
    TERRAIN_LOOKUPS => terrain_lookups,
    TERRAIN_HITS => terrain_hits,
    TERRAIN_MISSES => terrain_misses,
    VIEW_TERRAIN_MISSES => view_terrain_misses,
    BUILD_VIEW_TERRAIN_MISSES => build_view_terrain_misses,
    FEATURE_RUN_CALLS => feature_run_calls,
    FEATURE_RUN_HITS => feature_run_hits,
    FEATURE_RUN_MISSES => feature_run_misses,
    BUILD_VIEW_CALLS => build_view_calls,
    BUILD_VIEW_CELLS => build_view_cells,
    RUN_FEATURES_CALLS => run_features_calls,
);

/// 每个中心被 `cached_feature_run` 问过多少次（插桩打开时才记）。
/// How many times each center was requested from `cached_feature_run`.
type FeatureCenters = Mutex<HashMap<(i64, i32, i32), u64>>;
static FEATURE_CENTERS: std::sync::LazyLock<FeatureCenters> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

struct CenterSnapshot {
    count: usize,
    called_once: usize,
    max_calls: u64,
}

fn feature_centers_snapshot() -> CenterSnapshot {
    let Ok(map) = FEATURE_CENTERS.lock() else {
        return CenterSnapshot {
            count: 0,
            called_once: 0,
            max_calls: 0,
        };
    };
    CenterSnapshot {
        count: map.len(),
        called_once: map.values().filter(|&&n| n == 1).count(),
        max_calls: map.values().copied().max().unwrap_or(0),
    }
}

/// 记一次「以 `key` 为中心」的地物结果查询，用来统计重复调用分布。
/// Record one feature-result lookup by center, for the duplicate-call distribution.
#[inline]
pub fn record_feature_center(seed: i64, x: i32, z: i32) {
    if !enabled() {
        return;
    }
    if let Ok(mut map) = FEATURE_CENTERS.lock() {
        *map.entry((seed, x, z)).or_default() += 1;
    }
}

/// 插桩是否已打开；关闭时所有 `count` / `time` 都只是这一次布尔读。
/// Whether instrumentation is on; when off `count` / `time` cost only this load.
#[inline]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// 给一个指标加一。
/// Add one to a metric.
#[inline]
pub fn count(metric: &'static AtomicU64) {
    if enabled() {
        metric.fetch_add(1, Ordering::Relaxed);
    }
}

/// 给一个指标加 `n`。
/// Add `n` to a metric.
#[inline]
pub fn count_by(metric: &'static AtomicU64, n: u64) {
    if enabled() && n != 0 {
        metric.fetch_add(n, Ordering::Relaxed);
    }
}

/// 返回一个在 drop 时把经过的纳秒加进 `metric` 的计时器；关插桩时不读时钟。
/// Return a guard that adds elapsed nanoseconds to `metric` on drop. When
/// instrumentation is off no clock is read.
#[inline]
pub fn timer(metric: &'static AtomicU64) -> Option<Timer> {
    enabled().then(|| Timer {
        metric,
        started: Instant::now(),
    })
}

/// [`timer`] 的计时器；`Drop` 时把耗时记进对应指标。
/// Guard returned by [`timer`]; records elapsed time on drop.
pub struct Timer {
    metric: &'static AtomicU64,
    started: Instant,
}

impl Drop for Timer {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        self.metric.fetch_add(elapsed, Ordering::Relaxed);
    }
}

/// 让测量脚手架能按名字取到指标原子量（测试用，正式路径不查字符串）。
/// Expose the metric atomics by name for the benchmark and tests.
pub const METRICS: &[(&str, &AtomicU64)] = &[
    ("terrain_ns", &TERRAIN_NS),
    ("features_ns", &FEATURES_NS),
    ("light_ns", &LIGHT_NS),
    ("terrain_calls", &TERRAIN_CALLS),
    ("features_calls", &FEATURES_CALLS),
    ("light_calls", &LIGHT_CALLS),
    ("build_view_ns", &BUILD_VIEW_NS),
    ("run_features_ns", &RUN_FEATURES_NS),
    ("view_resolve_ns", &VIEW_RESOLVE_NS),
    ("view_cells", &VIEW_CELLS),
    ("snapshot_some", &SNAPSHOT_SOME),
    ("snapshot_none", &SNAPSHOT_NONE),
    ("terrain_lookups", &TERRAIN_LOOKUPS),
    ("terrain_hits", &TERRAIN_HITS),
    ("terrain_misses", &TERRAIN_MISSES),
    ("view_terrain_misses", &VIEW_TERRAIN_MISSES),
    ("build_view_terrain_misses", &BUILD_VIEW_TERRAIN_MISSES),
    ("feature_run_calls", &FEATURE_RUN_CALLS),
    ("feature_run_hits", &FEATURE_RUN_HITS),
    ("feature_run_misses", &FEATURE_RUN_MISSES),
    ("build_view_calls", &BUILD_VIEW_CALLS),
    ("build_view_cells", &BUILD_VIEW_CELLS),
    ("run_features_calls", &RUN_FEATURES_CALLS),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// 关插桩时计数不变；打开后 `count` / `count_by` / `timer` 都记进去。
    /// With instrumentation off nothing changes; on, every helper records.
    #[test]
    fn counters_only_move_when_enabled() {
        // 这个测试进程里没有别的测量在跑；用完立刻关掉。
        // No other measurement runs in this process; turn it off right away.
        disable();
        let before = FEATURE_RUN_CALLS.load(Ordering::Relaxed);
        count(&FEATURE_RUN_CALLS);
        count_by(&FEATURE_RUN_CALLS, 3);
        assert_eq!(FEATURE_RUN_CALLS.load(Ordering::Relaxed), before);
        reset();
        count(&FEATURE_RUN_CALLS);
        count_by(&FEATURE_RUN_CALLS, 3);
        assert!(timer(&TERRAIN_NS).is_some());
        assert_eq!(FEATURE_RUN_CALLS.load(Ordering::Relaxed), before + 4);
        assert!(stats().terrain_ns > 0);
        record_feature_center(7, 1, 2);
        record_feature_center(7, 1, 2);
        record_feature_center(7, 3, 4);
        let centers = stats();
        assert_eq!(centers.distinct_feature_centers, 2);
        assert_eq!(centers.feature_centers_called_once, 1);
        assert_eq!(centers.feature_centers_max_calls, 2);
        disable();
        let after = FEATURE_RUN_CALLS.load(Ordering::Relaxed);
        count(&FEATURE_RUN_CALLS);
        assert_eq!(FEATURE_RUN_CALLS.load(Ordering::Relaxed), after);
    }
}
