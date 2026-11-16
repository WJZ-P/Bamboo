//! 多区域 tick 第 2 步的统计：单元划分本身、单元形状、越界触发，以及按
//! `halo = 1 / 2` 各算一遍的理论上限。只在 `BAMBOO_PROFILE_PARTITION=1` 时接进
//! `World`；开关关着时世界循环只多一次 `Option` 判断，方块读写完全不走这里。
//!
//! Step-2 statistics for multi-region tick: partition cost, unit shapes,
//! out-of-unit triggers, and the theoretical speed-up for halo 1 and 2.

use std::{collections::HashMap, time::Instant};

use mc_bamboo_blocks::{Observation, Trigger};
use mc_bamboo_core::BlockPos;

use crate::region::partition::{Partition, Unit, distance_to_area};

/// 两种缓冲半径一起统计，见设计稿 4.3。
pub(crate) const HALOS: [i32; 2] = [1, 2];
/// 理论上限按 8 条线程算，和第 0 步离线估计同一个口径。
const THREADS: f64 = 8.0;
/// 设计稿 4.4 的「单元太小就不并行」门槛（微秒）。
const UNIT_THRESHOLD_US: f64 = 50.0;
/// 一份报告窗口里某个 halo 的越界累计。
#[derive(Debug, Default, Clone)]
pub(crate) struct HaloStats {
    /// 越界的触发数（一次触发只记一次）。
    pub(crate) out_triggers: u64,
    /// 触发位置不在任何单元忙区块里的次数（设计 4.3 第 2 种）。
    pub(crate) unassigned: u64,
    /// 越界涉及的单元数去重后的累计（每次触发记一个单元）。
    pub(crate) out_units: u64,
    /// 越界时碰到的最远距离（按区块算，切比雪夫距离）。
    pub(crate) max_distance: u32,
    /// 每刻越界的触发数直方图。
    pub(crate) out_per_tick: crate::profiler::Histogram,
    /// 每刻越界涉及的单元数直方图。
    pub(crate) units_per_tick: crate::profiler::Histogram,
    /// 越界来源：按「读 / 写」和触发方块的注册名分组。
    pub(crate) reads: u64,
    pub(crate) writes: u64,
    pub(crate) by_block: HashMap<&'static str, u64>,
    /// 越界单元这一刻合计耗时（纳秒），用来算串行尾段占三阶段的比例。
    pub(crate) tail_nanos: u128,
}

impl HaloStats {
    fn finish_tick(&mut self, out: u64, units: u64) {
        self.out_per_tick.record(out);
        self.units_per_tick.record(units);
    }

    fn reset(&mut self) {
        self.out_triggers = 0;
        self.unassigned = 0;
        self.out_units = 0;
        self.max_distance = 0;
        self.out_per_tick.reset();
        self.units_per_tick.reset();
        self.reads = 0;
        self.writes = 0;
        self.by_block.clear();
        self.tail_nanos = 0;
    }
}

/// 一次正在跑的触发：位置、方块种类、它属于哪个单元（halo 1/2 各一份）、
/// 以及这次触发已经越界了没有。越界只报一次，但「最远距离」要一直更新。
#[derive(Debug, Clone)]
struct OpenTrigger {
    pos: BlockPos,
    block: &'static str,
    /// 每个 halo：触发所在的单元下标，`None` 是第 2 种越界。
    unit: [Option<usize>; 2],
    reported: [bool; 2],
    started: Instant,
}

/// 三个方块侧阶段里，每个触发实际碰到的区块，和划分结果比对得到的越界统计。
///
/// 关闭时 `Profiler` 不挂它，`RegionLevel::observe` 只有一次 `Option` 判断。
#[derive(Debug, Default)]
pub(crate) struct PartitionStats {
    halos: [HaloStats; HALOS.len()],
    open: Option<OpenTrigger>,
    /// 这一刻每个 halo 越界的触发数 / 单元数。
    tick_out: [u64; HALOS.len()],
    tick_units: [u64; HALOS.len()],
    /// 这一刻的触发总数（含没越界的），报告里算越界占比。
    tick_triggers: u64,
    /// 划分阶段这一刻的耗时（纳秒）。
    partition_nanos: u64,
    partition_hist: crate::profiler::Histogram,
    /// 全量朴素对照的耗时（纳秒），默认 0。
    verify_nanos: u64,
    verify_hist: crate::profiler::Histogram,
    /// 每刻的单元数、大小分布和稳定性。
    unit_count: crate::profiler::Histogram,
    unit_size: crate::profiler::Histogram,
    merges: crate::profiler::Histogram,
    splits: crate::profiler::Histogram,
    /// 每刻忙区块数。
    busy_chunks: crate::profiler::Histogram,
    /// 每刻三阶段总耗时（纳秒）和「过 50 µs 门槛的单元」的三阶段耗时之和，
    /// 用来按实际划分重算理论上限。
    three_nanos: crate::profiler::Histogram,
    tail_share: crate::profiler::Histogram,
    /// 每刻的单元数样本，报告里列大小分布用。
    size_distribution: HashMap<usize, u64>,
    /// 报告窗口内每个 halo、每个单元的三阶段累计耗时（纳秒）。
    /// 单元下标只在划分不变期间有效；划分一变就把旧单元折进 `other_nanos`。
    unit_nanos: [HashMap<usize, u128>; HALOS.len()],
    /// 折进来的旧单元耗时（无法再按新划分归类的部分），算上限时按串行处理。
    other_nanos: [u128; HALOS.len()],
    /// 上一刻看到的单元下标集合；变了就折旧开新。
    unit_generation: [usize; HALOS.len()],
    /// 划分变化的代数，每刻由调用方传进来才知道下标有没有换主人（见 `record_units`）。
    last_unit_keys: [Vec<usize>; HALOS.len()],
}

/// 一个触发的方块名字。注册表 ID 来自方块逻辑，越界时用名字归类。
fn trigger_block(trigger: Trigger) -> (BlockPos, &'static str) {
    let (pos, id) = match trigger {
        Trigger::ScheduledTick { pos, kind } => (pos, kind),
        Trigger::ScheduledFluidTick { pos, kind } => {
            return (
                pos,
                mc_bamboo_data::registries::fluid::name_of(kind).unwrap_or("fluid"),
            );
        }
        Trigger::BlockEvent { pos, kind, .. } => (pos, kind),
        Trigger::BlockEntityTick { pos, kind } => (pos, kind),
    };
    let name = mc_bamboo_data::blocks::BLOCKS
        .get(id)
        .map(|block| block.name)
        .unwrap_or("unknown");
    (pos, name)
}

impl PartitionStats {
    /// 三个方块侧阶段之一开始。划分统计只在这三个阶段里认触发边界。
    pub(crate) fn begin_phase(&mut self) {
        self.open = None;
    }

    pub(crate) fn end_phase(&mut self) {
        self.open = None;
    }

    /// 记录划分阶段自己的耗时，和全量对照的耗时。
    pub(crate) fn record_partition(&mut self, nanos: u64, verify_nanos: u64) {
        self.partition_nanos = self.partition_nanos.saturating_add(nanos);
        self.verify_nanos = self.verify_nanos.saturating_add(verify_nanos);
    }

    /// 划分完成后把单元形状记进这一刻的样本。
    pub(crate) fn record_units(&mut self, busy: usize, units: &[Unit], merges: u64, splits: u64) {
        self.busy_chunks.record(busy as u64);
        self.unit_count.record(units.len() as u64);
        self.merges.record(merges);
        self.splits.record(splits);
        for unit in units {
            self.unit_size.record(unit.busy.len() as u64);
            *self.size_distribution.entry(unit.busy.len()).or_default() += 1;
        }
        // 单元槽位会被复用：划分拓扑一变，旧下标累计的耗时折进「其它」，
        // 不能让新单元继承旧单元的耗时。
        let keys: Vec<usize> = units.iter().map(|unit| unit.busy.len()).collect();
        for index in 0..HALOS.len() {
            if keys != self.last_unit_keys[index].as_slice() {
                self.unit_generation[index] = self.unit_generation[index].wrapping_add(1);
                for (_, nanos) in self.unit_nanos[index].drain() {
                    self.other_nanos[index] = self.other_nanos[index].saturating_add(nanos);
                }
            }
            self.last_unit_keys[index] = keys.clone();
        }
    }

    /// 一次触发开始。两套 halo 的划分都要查它属于哪个单元。
    pub(crate) fn trigger_begin(
        &mut self,
        trigger: Trigger,
        partitions: &[Partition],
        now: Instant,
    ) {
        let (pos, block) = trigger_block(trigger);
        let chunk = pos.chunk();
        let mut unit = [None; 2];
        for (index, partition) in partitions.iter().enumerate() {
            unit[index] = partition.unit_index_for(chunk);
        }
        self.tick_triggers = self.tick_triggers.saturating_add(1);
        self.open = Some(OpenTrigger {
            pos,
            block,
            unit,
            reported: [false; 2],
            started: now,
        });
    }

    /// 一次触发结束：两套 halo 各自记一次越界与否，把越界触发的耗时算进尾段。
    pub(crate) fn trigger_end(&mut self, now: Instant) {
        let Some(open) = self.open.take() else {
            return;
        };
        let nanos = u128::from(
            now.saturating_duration_since(open.started)
                .as_nanos()
                .min(u128::from(u64::MAX)) as u64,
        );
        for index in 0..HALOS.len() {
            let out = open.unit[index].is_none() || open.reported[index];
            if let Some(unit) = open.unit[index] {
                *self.unit_nanos[index].entry(unit).or_default() += nanos;
            } else {
                self.other_nanos[index] = self.other_nanos[index].saturating_add(nanos);
            }
            if !out {
                continue;
            }
            let halo = &mut self.halos[index];
            halo.out_triggers = halo.out_triggers.saturating_add(1);
            halo.tail_nanos = halo.tail_nanos.saturating_add(nanos);
            self.tick_out[index] = self.tick_out[index].saturating_add(1);
            if open.unit[index].is_none() {
                halo.unassigned = halo.unassigned.saturating_add(1);
            } else {
                halo.out_units = halo.out_units.saturating_add(1);
                self.tick_units[index] = self.tick_units[index].saturating_add(1);
            }
        }
    }

    /// 一次触发通过 `Level` 读或写了这一格。两套 halo 都比对单元范围。
    pub(crate) fn chunk_access(&mut self, pos: BlockPos, write: bool, partitions: &[Partition]) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let touched = pos.chunk();
        for (index, partition) in partitions.iter().enumerate() {
            let Some(unit) = partition.unit_for(open.pos.chunk()) else {
                continue; // 第 2 种越界，trigger_end 里统一记
            };
            if unit.area.contains(&touched) {
                continue;
            }
            let distance = distance_to_area(unit, touched).max(1) as u32;
            let halo = &mut self.halos[index];
            halo.max_distance = halo.max_distance.max(distance);
            if write {
                halo.writes = halo.writes.saturating_add(1);
            } else {
                halo.reads = halo.reads.saturating_add(1);
            }
            if !open.reported[index] {
                open.reported[index] = true;
                *halo.by_block.entry(open.block).or_default() += 1;
            }
        }
    }

    /// 处理一个 `Observation`。只认触发边界和区块访问；别的事件直接返回。
    pub(crate) fn observe(
        &mut self,
        event: Observation<'_>,
        partitions: &[Partition],
        now: Instant,
    ) {
        match event {
            Observation::TriggerBegin(trigger) => self.trigger_begin(trigger, partitions, now),
            Observation::TriggerEnd => self.trigger_end(now),
            Observation::ChunkAccess { pos, write } => self.chunk_access(pos, write, partitions),
            _ => {}
        }
    }

    /// 一刻结束：把这一刻的计数并进直方图，报告窗口末清零。
    pub(crate) fn finish_tick(&mut self, now: Instant, three_nanos: u64) {
        let _ = now;
        for index in 0..HALOS.len() {
            self.halos[index].finish_tick(self.tick_out[index], self.tick_units[index]);
        }
        self.partition_hist.record(self.partition_nanos);
        self.verify_hist.record(self.verify_nanos);
        self.three_nanos.record(three_nanos);
        // 尾段占三阶段的比例按 halo=1 算，报告里另列 halo=2。
        let tail_millis =
            (self.halos[0].tail_nanos.min(u128::from(u64::MAX)) as u64).saturating_mul(1_000);
        let share = tail_millis.checked_div(three_nanos).unwrap_or(0);
        self.tail_share.record(share.min(1_000));
        self.tick_out = [0; HALOS.len()];
        self.tick_units = [0; HALOS.len()];
        self.partition_nanos = 0;
        self.verify_nanos = 0;
        self.open = None;
    }

    pub(crate) fn reset(&mut self) {
        for halo in &mut self.halos {
            halo.reset();
        }
        self.open = None;
        self.tick_out = [0; HALOS.len()];
        self.tick_units = [0; HALOS.len()];
        self.tick_triggers = 0;
        self.partition_nanos = 0;
        self.verify_nanos = 0;
        self.partition_hist.reset();
        self.verify_hist.reset();
        self.unit_count.reset();
        self.unit_size.reset();
        self.merges.reset();
        self.splits.reset();
        self.busy_chunks.reset();
        self.three_nanos.reset();
        self.tail_share.reset();
        self.size_distribution.clear();
        for index in 0..HALOS.len() {
            self.unit_nanos[index].clear();
            self.other_nanos[index] = 0;
            self.last_unit_keys[index].clear();
        }
    }

    /// 一段多行文本，接在剖析报告末尾。`halo=1/2` 的越界、单元形状、
    /// 划分耗时、以及按实际划分 + 越界尾段重算的 Amdahl 上限都在这里。
    pub(crate) fn report(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let _ = writeln!(out, "partition (step 2, 3 phases run serially):");
        let _ = writeln!(
            out,
            "  partition_ns mean={:.3} p50={:.3} p95={:.3} max={:.3} verify_ns mean={:.3}",
            mean(&self.partition_hist),
            p(&self.partition_hist, 50),
            p(&self.partition_hist, 95),
            self.partition_hist.max() as f64 / 1_000.0,
            mean(&self.verify_hist),
        );
        let _ = writeln!(
            out,
            "  partition_share={:.4}% tick_mean={:.3}ms three_mean={:.3}ms",
            mean(&self.partition_hist) / mean(&self.three_nanos).max(1.0) * 100.0,
            mean(&self.three_nanos) / 1_000_000.0,
            mean(&self.three_nanos) / 1_000_000.0,
        );
        let _ = writeln!(
            out,
            "  units mean={:.2} p50={:.2} p95={:.2} max={:.2} busy={:.2} size_mean={:.2} merges={:.3}/tick splits={:.3}/tick",
            mean(&self.unit_count),
            p(&self.unit_count, 50),
            p(&self.unit_count, 95),
            self.unit_count.max(),
            mean(&self.busy_chunks),
            mean(&self.unit_size),
            mean(&self.merges),
            mean(&self.splits),
        );
        let mut sizes: Vec<_> = self.size_distribution.iter().collect();
        sizes.sort_unstable_by_key(|&(size, _)| size);
        for (&size, &count) in sizes {
            let _ = writeln!(out, "  unit-size {} {}", size, count);
        }
        let triggers = self.halos[0].out_per_tick.count().max(1);
        for (index, halo) in HALOS.iter().enumerate() {
            let stats = &self.halos[index];
            let _ = writeln!(
                out,
                "  halo {} out_triggers={} unassigned={} out_units={} max_distance={} reads={} writes={} out_per_tick_mean={:.3} tail_ns={}",
                halo,
                stats.out_triggers,
                stats.unassigned,
                stats.out_units,
                stats.max_distance,
                stats.reads,
                stats.writes,
                mean(&stats.out_per_tick),
                stats.tail_nanos,
            );
            let mut blocks: Vec<_> = stats.by_block.iter().collect();
            blocks.sort_unstable_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
            for (name, count) in blocks.iter().take(8) {
                let _ = writeln!(out, "  halo {} out-block {} {}", halo, name, count);
            }
        }
        let _ = writeln!(
            out,
            "  triggers_in_window={} ticks={}",
            self.tick_triggers, triggers
        );
        // 按实际划分重算 8 线程上限：三阶段总量 S，最贵的单元 u，越界 / 没归属
        // 的触发耗时进串行尾段，余下并行；再扣掉「单元太小就不并行」的门槛。
        let three = mean(&self.three_nanos);
        for (index, halo) in HALOS.iter().enumerate() {
            let tail = self.halos[index].tail_nanos as f64;
            let biggest = self.unit_nanos[index].values().copied().max().unwrap_or(0) as f64;
            let other = self.other_nanos[index] as f64;
            let parallel = (three - tail - other).max(0.0);
            let serial_tail = tail + other;
            let per_unit = biggest.max(parallel / THREADS);
            let _ = writeln!(
                out,
                "  halo {} speedup_input three_ns={:.0} tail_ns={:.0} other_ns={:.0} biggest_unit_ns={:.0} per_unit_ns={:.0} unit_threshold_us={:.0}",
                halo, three, serial_tail, other, biggest, per_unit, UNIT_THRESHOLD_US,
            );
        }
        out
    }
}

fn mean(histogram: &crate::profiler::Histogram) -> f64 {
    histogram.sum() as f64 / histogram.count().max(1) as f64
}

fn p(histogram: &crate::profiler::Histogram, percent: usize) -> f64 {
    histogram.percentile(percent) as f64
}
