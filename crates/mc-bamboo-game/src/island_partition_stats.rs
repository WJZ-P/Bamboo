//! 岛的实施计划第 8 步的统计：岛数、两种大小分布、合并 / 拆分频率、
//! 分区器自己每刻的耗时，以及不变量失败次数。
//! `BAMBOO_PROFILE_ISLANDS=1` 打开，`=2` 还打开每刻全量对照；默认关。
//!
//! Step-8 statistics for island partitioning: island count, two size
//! distributions, merge/split rates, partitioner cost per tick, and
//! invariant failures. Enabled by `BAMBOO_PROFILE_ISLANDS=1/2`.

use std::{collections::BTreeMap, fmt::Write};

use crate::{
    island_partition::{IslandPartition, TickResult},
    profiler::Histogram,
};

/// 每秒按 20 个世界刻算。
const TICKS_PER_SECOND: f64 = 20.0;

#[derive(Debug, Default)]
pub(crate) struct IslandPartitionStats {
    island_count: Histogram,
    section_size: Histogram,
    chunk_size: Histogram,
    merges: Histogram,
    splits: Histogram,
    pending_merges: Histogram,
    pending_splits: Histogram,
    cancelled_splits: Histogram,
    desired_chunks: Histogram,
    held_chunks: Histogram,
    partition_ns: Histogram,
    bookkeeping_ns: Histogram,
    verify_ns: Histogram,
    /// 报告窗口内每个「岛数」出现的刻数，报告里列出中位数和最大值。
    count_distribution: BTreeMap<u64, u64>,
    /// 每个 section 大小的岛出现的次数。
    section_distribution: BTreeMap<u64, u64>,
    /// 每个区块大小的岛出现的次数。
    chunk_distribution: BTreeMap<u64, u64>,
    window_merges: u64,
    window_splits: u64,
    window_cancelled: u64,
    window_ticks: u64,
    invariant_failures: u64,
}

impl IslandPartitionStats {
    /// 一刻的分区器结果。样本数就是这个报告窗口里 `World::tick` 的次数。
    pub(crate) fn record_tick(
        &mut self,
        partition: &IslandPartition,
        result: TickResult,
        bookkeeping_nanos: u64,
    ) {
        self.island_count.record(result.island_count as u64);
        *self
            .count_distribution
            .entry(result.island_count as u64)
            .or_default() += 1;
        for size in partition.island_sizes() {
            self.section_size.record(size as u64);
            *self.section_distribution.entry(size as u64).or_default() += 1;
        }
        for size in partition.island_chunk_sizes() {
            self.chunk_size.record(size as u64);
            *self.chunk_distribution.entry(size as u64).or_default() += 1;
        }
        self.merges.record(result.merges);
        self.splits.record(result.splits);
        self.pending_merges.record(result.pending_merges);
        self.pending_splits.record(result.pending_splits);
        self.cancelled_splits.record(result.cancelled_splits);
        self.desired_chunks.record(partition.desired_len() as u64);
        self.held_chunks.record(partition.held_len() as u64);
        self.partition_ns.record(result.nanos);
        self.bookkeeping_ns.record(bookkeeping_nanos);
        self.verify_ns.record(result.verify_nanos);
        self.window_merges = self.window_merges.saturating_add(result.merges);
        self.window_splits = self.window_splits.saturating_add(result.splits);
        self.window_cancelled = self
            .window_cancelled
            .saturating_add(result.cancelled_splits);
        self.window_ticks = self.window_ticks.saturating_add(1);
        self.invariant_failures = self
            .invariant_failures
            .saturating_add(result.invariant_failures);
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// 报告末尾的一段，和单元划分统计的报告并列。
    pub(crate) fn report(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "island partition (step 8, computed but not used):");
        let _ = writeln!(
            out,
            "  island_partition_ns mean={:.1} p50={:.1} p95={:.1} max={:.1} bookkeeping_ns mean={:.1} p95={:.1} verify_ns mean={:.1}",
            mean(&self.partition_ns),
            mean_p(&self.partition_ns, 50),
            mean_p(&self.partition_ns, 95),
            self.partition_ns.max(),
            mean(&self.bookkeeping_ns),
            mean_p(&self.bookkeeping_ns, 95),
            mean(&self.verify_ns),
        );
        let ticks = self.window_ticks.max(1) as f64;
        let _ = writeln!(
            out,
            "  islands mean={:.3} p50={:.1} p95={:.1} median_window={:.1} max_window={:.1}",
            mean(&self.island_count),
            mean_p(&self.island_count, 50),
            mean_p(&self.island_count, 95),
            median(&self.count_distribution),
            self.count_distribution.keys().copied().max().unwrap_or(0),
        );
        let _ = writeln!(
            out,
            "  island_size sections mean={:.3} chunks mean={:.3} desired={:.1} held={:.1}",
            mean(&self.section_size),
            mean(&self.chunk_size),
            mean(&self.desired_chunks),
            mean(&self.held_chunks),
        );
        distribution(&mut out, "island-size-sections", &self.section_distribution);
        distribution(&mut out, "island-size-chunks", &self.chunk_distribution);
        let _ = writeln!(
            out,
            "  merges total={} per_tick={:.4} per_second={:.3} pending={:.4}/tick",
            self.window_merges,
            mean(&self.merges),
            self.window_merges as f64 / ticks * TICKS_PER_SECOND,
            mean(&self.pending_merges),
        );
        let _ = writeln!(
            out,
            "  splits total={} per_tick={:.4} per_second={:.3} pending={:.4}/tick cancelled={} ({:.4}/tick)",
            self.window_splits,
            mean(&self.splits),
            self.window_splits as f64 / ticks * TICKS_PER_SECOND,
            mean(&self.pending_splits),
            self.window_cancelled,
            mean(&self.cancelled_splits),
        );
        let _ = writeln!(
            out,
            "  invariant_failures={} window_ticks={}",
            self.invariant_failures, self.window_ticks,
        );
        out
    }
}

fn mean(histogram: &Histogram) -> f64 {
    histogram.sum() as f64 / histogram.count().max(1) as f64
}

fn mean_p(histogram: &Histogram, percent: usize) -> f64 {
    histogram.percentile(percent) as f64
}

/// 加权中位数：分布里的值是大小，权重是出现次数。
fn median(distribution: &BTreeMap<u64, u64>) -> f64 {
    let total: u64 = distribution.values().sum();
    if total == 0 {
        return 0.0;
    }
    let mut rank = total.div_ceil(2);
    for (&value, &count) in distribution {
        if rank <= count {
            return value as f64;
        }
        rank -= count;
    }
    distribution.keys().copied().max().unwrap_or(0) as f64
}

fn distribution(out: &mut String, label: &str, values: &BTreeMap<u64, u64>) {
    for (&size, &count) in values {
        let _ = writeln!(out, "  {label} {size} {count}");
    }
}
