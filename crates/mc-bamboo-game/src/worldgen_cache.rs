//! 世界生成两份全进程缓存（CARVERS 地形、地物结果）的公共策略：字节上限、
//! 空闲释放、按依赖释放。
//!
//! 两份缓存存的都是纯函数的结果，删掉任何一条只会让下一次查询现算，**不改变
//! 生成结果**。这里只管「什么时候删」：
//!
//! * **字节上限**：`bamboo.toml` 的 `worldgen_terrain_cache_mb` /
//!   `worldgen_feature_cache_mb`，超了按插入顺序淘汰（各缓存自己做）。
//! * **按依赖释放**：一条缓存只被它周围几个区块的 `features` / `light` 阶段用到。
//!   世界线程在区块完工组装时调 [`chunk_finished`]，地物缓存在算完一个中心时调
//!   [`feature_run_computed`]；每条缓存记一张「哪些使用者已经用完」的位图，位图
//!   满了就立刻删。使用者还没被请求的条目（已生成区域的最外几圈）位图永远不满，
//!   留给字节上限和空闲释放处理。
//! * **空闲释放**：[`release_if_idle`] 由世界线程每刻调一次；超过
//!   `worldgen_cache_idle_seconds` 秒没有任何缓存查询，就把两份缓存清空。
//!
//! Shared policy for the two process-wide worldgen caches (CARVERS terrain and
//! feature results): byte limits, idle release and dependency release.
//!
//! Both caches hold results of pure functions, so dropping an entry only makes
//! the next lookup recompute it and **never changes generation output**. This
//! module only decides when to drop:
//!
//! * **Byte limits**: `worldgen_terrain_cache_mb` / `worldgen_feature_cache_mb`
//!   in `bamboo.toml`; each cache evicts in insertion order past its limit.
//! * **Dependency release**: an entry is only read by the `features` / `light`
//!   stages of a few surrounding chunks. The world thread calls
//!   [`chunk_finished`] when a chunk is assembled and the feature cache calls
//!   [`feature_run_computed`] when a center is done; each entry keeps a bitmap
//!   of consumers that are finished and is dropped as soon as it is full.
//!   Entries whose consumers were never requested (the outer rings of the
//!   generated area) never fill up and are left to the byte limit and the idle
//!   release.
//! * **Idle release**: the world thread calls [`release_if_idle`] every tick;
//!   after `worldgen_cache_idle_seconds` without any cache lookup both caches
//!   are emptied.

use std::{
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Instant,
};

use mc_bamboo_core::ChunkPos;

use crate::world_options::WorldGenConfig;

const MIB: usize = 1024 * 1024;

/// 地形缓存默认上限（MiB），包括未压缩的热层和调色板压缩的冷层。
/// 依据见 `docs/performance-optimizations.md` 第 23 节的实测：64 MiB 在 17×17
/// 区块的突发请求里要把热层容量压到工作集以下，重算约 100 个区块；128 MiB
/// 与基线同等吞吐，峰值仍低约 180 MiB。
///
/// Default terrain cache limit in MiB, including the raw hot tier and
/// palette-compressed cold tier. See section 23 of `performance-optimizations.md`:
/// at 64 MiB a 17x17 burst pushes the hot tier below the working set and
/// recomputes about 100 chunks, while 128 MiB matches the baseline throughput
/// with peak RSS still about 180 MiB lower.
pub const DEFAULT_TERRAIN_CACHE_MB: usize = 128;
/// 地物结果缓存默认上限（MiB）；更小的预算会反复计算仍被光照使用的地物。
///
/// Default feature-result cache limit in MiB; smaller budgets repeatedly
/// recompute features that lighting still needs.
pub const DEFAULT_FEATURE_CACHE_MB: usize = 64;
/// 默认空闲释放秒数；0 表示不做空闲释放。
/// Default idle-release delay in seconds; 0 disables the idle release.
pub const DEFAULT_IDLE_SECONDS: u64 = 30;

/// 配置文件里三个键允许的最大值。
/// Largest values the three config keys accept.
pub const MAX_TERRAIN_CACHE_MB: usize = 4096;
pub const MAX_FEATURE_CACHE_MB: usize = 1024;
pub const MAX_IDLE_SECONDS: u64 = 3600;

/// 两份世界生成缓存的设置，对应 `bamboo.toml` 的三个键。
///
/// Settings of the two worldgen caches; the three `bamboo.toml` keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldgenCacheSettings {
    /// 地形缓存上限（MiB）；0 = 不留任何条目。
    /// Terrain cache limit in MiB; 0 keeps no entry.
    pub terrain_mb: usize,
    /// 地物结果缓存上限（MiB）；0 = 不留任何条目。
    /// Feature-result cache limit in MiB; 0 keeps no entry.
    pub feature_mb: usize,
    /// 多少秒没有缓存查询就清空两份缓存；0 = 不清。
    /// Seconds without a cache lookup before both caches are emptied; 0 = never.
    pub idle_seconds: u64,
}

impl Default for WorldgenCacheSettings {
    fn default() -> Self {
        Self {
            terrain_mb: DEFAULT_TERRAIN_CACHE_MB,
            feature_mb: DEFAULT_FEATURE_CACHE_MB,
            idle_seconds: DEFAULT_IDLE_SECONDS,
        }
    }
}

static TERRAIN_BYTES: AtomicUsize = AtomicUsize::new(DEFAULT_TERRAIN_CACHE_MB * MIB);
static FEATURE_BYTES: AtomicUsize = AtomicUsize::new(DEFAULT_FEATURE_CACHE_MB * MIB);
static IDLE_SECONDS: AtomicU64 = AtomicU64::new(DEFAULT_IDLE_SECONDS);
/// 按依赖释放的总开关：只给压测对比用，正式配置没有这个键。
/// Dependency-release switch: for benchmark comparisons only, not a config key.
static DEPENDENCY_RELEASE: AtomicBool = AtomicBool::new(true);
/// 冷层（压缩条目）的总开关：同样只给压测对比用，正式配置没有这个键。
/// Cold-tier switch: again for benchmark comparisons only, not a config key.
static COLD_TIER: AtomicBool = AtomicBool::new(true);
/// 最近查询的毫秒数加 1；0 表示上次释放后没有查询。
/// Last lookup time in milliseconds plus 1; zero means no lookup since release.
static LAST_USE_MS: AtomicU64 = AtomicU64::new(0);
static IDLE_RELEASES: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// 应用设置。缓存是全进程一份，所以设置也是：后建的 `World` 覆盖先建的。
/// 调小上限不会立刻淘汰，下一次插入时生效。
///
/// Apply settings. The caches are process-wide and so are the settings: a later
/// `World` overrides an earlier one. A smaller limit takes effect at the next
/// insertion rather than immediately.
pub fn configure(settings: WorldgenCacheSettings) {
    TERRAIN_BYTES.store(settings.terrain_mb.saturating_mul(MIB), Ordering::Relaxed);
    FEATURE_BYTES.store(settings.feature_mb.saturating_mul(MIB), Ordering::Relaxed);
    IDLE_SECONDS.store(settings.idle_seconds, Ordering::Relaxed);
}

/// 压测对比用：关掉 / 打开按依赖释放。
/// Benchmark comparisons only: switch the dependency release off or on.
#[doc(hidden)]
pub fn set_dependency_release(enabled: bool) {
    DEPENDENCY_RELEASE.store(enabled, Ordering::Relaxed);
}

/// 压测对比用：关掉 / 打开地形缓存的冷层（压缩条目）。
/// Benchmark comparisons only: switch the cold (compacted) tier off or on.
#[doc(hidden)]
pub fn set_cold_tier(enabled: bool) {
    COLD_TIER.store(enabled, Ordering::Relaxed);
}

/// 冷层是否打开（`worldgen.rs` 按它决定冷层预算）。
/// Whether the cold tier is on (`worldgen.rs` sizes its budget from this).
pub(crate) fn cold_tier_enabled() -> bool {
    COLD_TIER.load(Ordering::Relaxed)
}

/// 空闲释放一共触发过几次（压测 / 测试用）。
/// How many idle releases have happened (benchmarks / tests).
#[doc(hidden)]
pub fn idle_releases() -> u64 {
    IDLE_RELEASES.load(Ordering::Relaxed)
}

pub(crate) fn terrain_byte_limit() -> usize {
    TERRAIN_BYTES.load(Ordering::Relaxed)
}

pub(crate) fn feature_byte_limit() -> usize {
    FEATURE_BYTES.load(Ordering::Relaxed)
}

/// 每次缓存查询都调：记下「刚刚还有生成在用缓存」。
/// Called by every cache lookup: generation used the caches just now.
pub(crate) fn touch() {
    LAST_USE_MS.fetch_max(now_ms() + 1, Ordering::Relaxed);
}

/// 世界线程每刻调一次：空闲够久就清空两份缓存，返回这次有没有清。
/// 平时只是两次原子读；清空时才拿分片锁（锁内只有查表，没有 IO）。
///
/// Called once per tick by the world thread: empty both caches after a long
/// enough idle period; returns whether it did. Normally just two atomic loads;
/// the shard locks (which only ever guard map operations, never IO) are taken
/// only when releasing.
pub fn release_if_idle() -> bool {
    let idle_ms = IDLE_SECONDS.load(Ordering::Relaxed).saturating_mul(1000);
    let last = LAST_USE_MS.load(Ordering::Relaxed);
    if !idle_elapsed(idle_ms, last, now_ms()) {
        return false;
    }
    // 仅消费观察到的空闲时刻，不能覆盖刚发生的查询。
    // Claim only the observed idle timestamp, without erasing a fresh lookup.
    if LAST_USE_MS
        .compare_exchange(last, 0, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return false;
    }
    crate::worldgen::release_terrain_cache();
    crate::worldgen_features::release_feature_cache();
    crate::worldgen_light::clear_final_blocks_cache();
    IDLE_RELEASES.fetch_add(1, Ordering::Relaxed);
    true
}

fn idle_elapsed(idle_ms: u64, last_use: u64, now: u64) -> bool {
    idle_ms > 0 && last_use != 0 && now.saturating_sub(last_use - 1) >= idle_ms
}

/// 3×3 邻域里 `(dx, dz)` 的位下标（行优先）。
///
/// Bit index of `(dx, dz)` in a 3×3 neighborhood (row-major).
fn neighborhood_bit(dx: i32, dz: i32) -> u32 {
    ((dz + 1) * 3 + (dx + 1)) as u32
}

fn dependency_release_applies(config: &WorldGenConfig) -> bool {
    // flat 世界的地形只有一条共用条目，不按位置释放。
    // Flat worlds share a single terrain entry; nothing is released by position.
    DEPENDENCY_RELEASE.load(Ordering::Relaxed) && !config.level_type.is_flat()
}

/// 区块 `pos` 完工组装了：它的 `features` / `light` 阶段不会再读缓存。
///
/// * `light` 直接读 3×3 的地形 → 给 `pos ± 1` 的地形条目记「这个使用者完工」；
/// * `features` 和局部 `light` 都只读 3×3 个地物中心 → 给 `pos ± 1` 的地物条目记一位。
///
/// 由世界线程在 `Task::Assemble` 里调；只拿分片锁做查表，不做 IO。
///
/// Chunk `pos` has been assembled, so its `features` / `light` stages will not
/// read the caches again. `light` reads the 3×3 terrain directly, so terrain
/// entries at `pos ± 1` get this consumer's bit; `features` and local `light`
/// both read 3×3 feature results, so feature entries at `pos ± 1` get one. Called by
/// the world thread in `Task::Assemble`; it only takes shard locks for map
/// operations, never IO.
pub fn chunk_finished(config: &WorldGenConfig, pos: ChunkPos) {
    if !dependency_release_applies(config) {
        return;
    }
    for dz in -1..=1 {
        for dx in -1..=1 {
            // 条目在 `pos + d`，从条目看使用者在 `-d`。
            // The entry sits at `pos + d`; seen from it the consumer is at `-d`.
            let bit = neighborhood_bit(-dx, -dz);
            crate::worldgen::mark_terrain_chunk_finished(config, pos.x + dx, pos.z + dz, bit);
            crate::worldgen_features::mark_feature_chunk_finished(
                config,
                pos.x + dx,
                pos.z + dz,
                bit,
            );
        }
    }
}

/// 以 `(cx, cz)` 为中心的地物刚算完并进了地物缓存：它读过的 3×3 地形少了一个
/// 使用者。由算完这个中心的计算线程调。
///
/// The feature run centered on `(cx, cz)` was just computed and cached: the 3×3
/// terrain it read has one consumer fewer. Called by the computing thread.
pub(crate) fn feature_run_computed(config: &WorldGenConfig, cx: i32, cz: i32) {
    if !dependency_release_applies(config) {
        return;
    }
    for dz in -1..=1 {
        for dx in -1..=1 {
            let bit = neighborhood_bit(-dx, -dz);
            crate::worldgen::mark_terrain_feature_run(config, cx + dx, cz + dz, bit);
        }
    }
}

/// 3×3 使用者全部完成时的位图。
/// Bitmap value when all 3×3 consumers are done.
pub(crate) const ALL_3X3: u32 = (1 << 9) - 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighborhood_bits_are_distinct_and_fill_the_mask() {
        let mut mask = 0_u32;
        for dz in -1..=1 {
            for dx in -1..=1 {
                let bit = 1 << neighborhood_bit(dx, dz);
                assert_eq!(mask & bit, 0, "bit reused at ({dx},{dz})");
                mask |= bit;
            }
        }
        assert_eq!(mask, ALL_3X3);
    }

    #[test]
    fn idle_needs_a_delay_a_lookup_and_enough_time() {
        assert!(!idle_elapsed(0, 6, 1_000_000), "0 disables it");
        assert!(!idle_elapsed(30_000, 0, 1_000_000), "nothing to drop");
        assert!(!idle_elapsed(30_000, 6, 30_004));
        assert!(idle_elapsed(30_000, 6, 30_005));
        assert!(idle_elapsed(30_000, 1, 30_000), "lookup at time zero");
    }
}
