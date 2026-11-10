//! 世界线程每一刻的耗时直方图。世界线程独占，记录时既不分配内存也不加锁。
//!
//! `Histogram` 的桶是固定数组：0..[`LINEAR_BUCKETS`] 每纳秒一个桶，之上每个 2 的幂
//! 再分 [`SUB_BUCKETS`] 个子桶。分位数返回所在桶的下界，是故意偏保守的估计。

use std::{
    env,
    fmt::Write,
    time::{Duration, Instant},
};

/// 环境变量给的是报告间隔的秒数；没设、`0`、负数或解析不了都当成没设置。
const PROFILE_ENV: &str = "BAMBOO_PROFILE";
/// 相邻两刻的起点理想上相隔 50 ms，和 `world.rs` 里的一刻等长。
const TICK: Duration = Duration::from_millis(50);
/// 0..15 ns 逐纳秒记，小到一次 lap 的耗时也能分辨。
const LINEAR_BUCKETS: usize = 16;
/// 每个 2 的幂内再分 8 份，相对误差不超过 12.5%。
const SUB_BUCKETS: usize = 8;
const SUB_SHIFT: u32 = SUB_BUCKETS.ilog2();
/// 指数从 4 开始，正好接在 0..15 的线性桶后面。
const MIN_EXPONENT: u32 = SUB_SHIFT + 1;
/// 最大的对数区间是 2^47..2^48，约 3 天，比任何一次 tick 都长几个数量级。
const MAX_EXPONENT: u32 = 47;
/// 末尾再多一个桶，装所有不小于 `2^(MAX_EXPONENT + 1)` 的值。
const BUCKETS: usize =
    LINEAR_BUCKETS + (MAX_EXPONENT - MIN_EXPONENT + 1) as usize * SUB_BUCKETS + 1;
const MAX_NANOS: u64 = 1_u64 << (MAX_EXPONENT + 1);

/// 一个桶的下界，用来报分位数。
fn bucket_lower_bound(index: usize) -> u64 {
    if index < LINEAR_BUCKETS {
        return index as u64;
    }
    let within = index - LINEAR_BUCKETS;
    if within == (MAX_EXPONENT - MIN_EXPONENT + 1) as usize * SUB_BUCKETS {
        return MAX_NANOS;
    }
    let exponent = MIN_EXPONENT + (within / SUB_BUCKETS) as u32;
    let sub = (within % SUB_BUCKETS) as u64;
    (1_u64 << exponent) + (sub << (exponent - SUB_SHIFT))
}

/// 纳秒值落在哪个桶。
fn bucket_index(nanos: u64) -> usize {
    if nanos < LINEAR_BUCKETS as u64 {
        return nanos as usize;
    }
    if nanos >= MAX_NANOS {
        return BUCKETS - 1;
    }
    // nanos >= 16，所以最高位至少是 4。
    let exponent = u64::BITS - 1 - nanos.leading_zeros();
    let sub = ((nanos - (1_u64 << exponent)) >> (exponent - SUB_SHIFT)) as usize;
    LINEAR_BUCKETS + (exponent - MIN_EXPONENT) as usize * SUB_BUCKETS + sub
}

/// 固定桶、对数刻度的直方图。也用来记数量，此时值的单位由调用方决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Histogram {
    buckets: [u64; BUCKETS],
    count: u64,
    sum: u128,
    max: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            buckets: [0; BUCKETS],
            count: 0,
            sum: 0,
            max: 0,
        }
    }
}

impl Histogram {
    /// 记一个数。只做一次下标计算和几次整数加法。
    pub(crate) fn record(&mut self, value: u64) {
        let index = bucket_index(value);
        self.buckets[index] = self.buckets[index].saturating_add(1);
        self.count = self.count.saturating_add(1);
        self.sum = self.sum.saturating_add(u128::from(value));
        self.max = self.max.max(value);
    }

    /// 记一个时长，纳秒超过 u64 时截断。
    fn record_duration(&mut self, value: Duration) {
        self.record(value.as_nanos().min(u128::from(u64::MAX)) as u64);
    }

    pub(crate) fn count(&self) -> u64 {
        self.count
    }

    pub(crate) fn sum(&self) -> u128 {
        self.sum
    }

    pub(crate) fn max(&self) -> u64 {
        self.max
    }

    /// 百分位：返回累计占比首次达到 `percent` 的那个桶的下界。
    /// 分位数因此永远不超过真实值，且不会高于 [`Self::max`]。
    pub(crate) fn percentile(&self, percent: usize) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let rank = (u128::from(self.count) * percent as u128).div_ceil(100);
        let mut seen: u128 = 0;
        for (index, &bucket) in self.buckets.iter().enumerate() {
            seen += u128::from(bucket);
            if seen >= rank {
                return bucket_lower_bound(index).min(self.max);
            }
        }
        self.max
    }

    pub(crate) fn p50(&self) -> u64 {
        self.percentile(50)
    }

    pub(crate) fn p95(&self) -> u64 {
        self.percentile(95)
    }

    pub(crate) fn p99(&self) -> u64 {
        self.percentile(99)
    }

    /// 把另一份直方图并进来，以后多个 region 的报告要合起来看。
    pub(crate) fn merge(&mut self, other: &Self) {
        for (bucket, &value) in self.buckets.iter_mut().zip(&other.buckets) {
            *bucket += value;
        }
        self.count += other.count;
        self.sum += other.sum;
        self.max = self.max.max(other.max);
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

/// `World::tick` 的一段。顺序和 `world.rs` 里的调用顺序一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Phase {
    Inbox,
    LightUpdates,
    WeatherAndTime,
    ScheduledTicks,
    RandomTicks,
    BlockEvents,
    /// 精确驱动时整个实体阶段；并行驱动时不用。
    Entities,
    /// 并行驱动的算阶段：`compute`，只读方块和实体表。
    EntityCompute,
    /// 并行驱动的落地阶段：`commit`，发布状态并按 uid 升序应用命令。
    EntityCommit,
    BlockEntities,
    PlayerMisc,
    Views,
    BlockChangeBroadcast,
    EntitySyncAndTracking,
    ChunkSending,
    Flush,
    /// tick 末尾的关服检查、名单变更发布等零碎工作。
    TickTail,
}

impl Phase {
    const ALL: [Self; 17] = [
        Self::Inbox,
        Self::LightUpdates,
        Self::WeatherAndTime,
        Self::ScheduledTicks,
        Self::RandomTicks,
        Self::BlockEvents,
        Self::Entities,
        Self::EntityCompute,
        Self::EntityCommit,
        Self::BlockEntities,
        Self::PlayerMisc,
        Self::Views,
        Self::BlockChangeBroadcast,
        Self::EntitySyncAndTracking,
        Self::ChunkSending,
        Self::Flush,
        Self::TickTail,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::LightUpdates => "light_updates",
            Self::WeatherAndTime => "weather_and_time",
            Self::ScheduledTicks => "scheduled_ticks",
            Self::RandomTicks => "random_ticks",
            Self::BlockEvents => "block_events",
            Self::Entities => "entities",
            Self::EntityCompute => "entity_compute",
            Self::EntityCommit => "entity_commit",
            Self::BlockEntities => "block_entities",
            Self::PlayerMisc => "player_misc",
            Self::Views => "views",
            Self::BlockChangeBroadcast => "block_change_broadcast",
            Self::EntitySyncAndTracking => "entity_sync_and_tracking",
            Self::ChunkSending => "chunk_sending",
            Self::Flush => "flush",
            Self::TickTail => "tick_tail",
        }
    }
}

/// 不是阶段耗时的那些指标。计数的直方图记的是「每刻多少个」，不是时间。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Metric {
    TickTotal,
    /// 实际开始 − 应该开始。
    TickStartLateness,
    /// 相邻两刻实际起点的间隔和 50ms 网格差多少，只看正常按网格调度的一对。
    TickIntervalDeviation,
    InboundMessages,
    OnlinePlayers,
    DueScheduledTicks,
    Entities,
    ChunksSent,
    ChunkLatencyUs,
    FlushBytes,
    /// 因为落后太多而放弃追赶的次数，只有累计值，没有直方图。
    SkippedCatchUps,
}

impl Metric {
    const ALL: [Self; 10] = [
        Self::TickTotal,
        Self::TickStartLateness,
        Self::TickIntervalDeviation,
        Self::InboundMessages,
        Self::OnlinePlayers,
        Self::DueScheduledTicks,
        Self::Entities,
        Self::ChunksSent,
        Self::ChunkLatencyUs,
        Self::FlushBytes,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::TickTotal => "tick_total",
            Self::TickStartLateness => "tick_start_lateness",
            Self::TickIntervalDeviation => "tick_interval_deviation",
            Self::InboundMessages => "inbound_messages",
            Self::OnlinePlayers => "online_players",
            Self::DueScheduledTicks => "due_scheduled_ticks",
            Self::Entities => "entities",
            Self::ChunksSent => "chunks_sent",
            Self::ChunkLatencyUs => "chunk_latency_us",
            Self::FlushBytes => "flush_bytes",
            Self::SkippedCatchUps => "skipped_catch_ups",
        }
    }

    /// 报告里数量的单位；时间的单位单独标 ms。
    fn unit(self) -> &'static str {
        match self {
            Self::OnlinePlayers => "players",
            Self::Entities => "entities",
            Self::InboundMessages => "messages",
            Self::DueScheduledTicks => "ticks",
            Self::ChunksSent => "chunks",
            Self::ChunkLatencyUs => "us",
            Self::FlushBytes => "bytes",
            Self::SkippedCatchUps => "times",
            Self::TickTotal | Self::TickStartLateness | Self::TickIntervalDeviation => "ms",
        }
    }

    fn is_duration(self) -> bool {
        matches!(
            self,
            Self::TickTotal | Self::TickStartLateness | Self::TickIntervalDeviation
        )
    }
}

/// 世界线程取「现在」的地方。生产环境读单调时钟，测试可以钉死再手动推进，
/// 区块延迟这类墙上时间的指标因此不依赖真实时钟跑多快。
#[derive(Debug, Clone)]
pub(crate) enum Clock {
    Wall,
    /// 固定起点加上可推进的偏移，只给测试用。
    #[cfg(test)]
    Manual {
        origin: Instant,
        offset: Duration,
    },
}

impl Clock {
    pub(crate) fn now(&self) -> Instant {
        match self {
            Self::Wall => Instant::now(),
            #[cfg(test)]
            Self::Manual { origin, offset } => *origin + *offset,
        }
    }

    /// 测试里推进钉死的时钟，生产时钟不受影响。
    #[cfg(test)]
    pub(crate) fn advance(&mut self, by: Duration) {
        if let Self::Manual { offset, .. } = self {
            *offset += by;
        }
    }
}

/// 一个 region 的剖析器。所有记录接口都只碰自己的数组，世界线程独占。
pub(crate) struct Profiler {
    phases: [Histogram; Phase::ALL.len()],
    metrics: [Histogram; Metric::ALL.len()],
    skipped_catch_ups: u64,
    /// `send_chunks` 这一刻发出的区块数，阶段末尾记一条样本。
    chunks_this_tick: u64,
    /// 上一次 `lap` 或 `begin_tick` 的时刻。
    last_lap: Instant,
    tick_start: Instant,
    /// 上一刻的实际起点。报告和 `begin_tick` 之间不分家，所以也在这里。
    previous_tick_start: Option<Instant>,
    ticks: u64,
    /// 世界线程取「现在」的地方；测试可以钉死并推进。
    clock: Clock,
    /// 没设环境变量时是 `None`：只累计、不打印。
    report_interval: Option<Duration>,
    last_report: Instant,
}

impl Profiler {
    fn series_index(series: Series) -> (bool, usize) {
        match series {
            Series::Phase(phase) => (false, phase as usize),
            Series::Metric(metric) => (true, metric as usize),
        }
    }

    fn with_report_interval(interval: Option<Duration>) -> Self {
        Self::with_clock(interval, Clock::Wall)
    }

    fn with_clock(interval: Option<Duration>, clock: Clock) -> Self {
        let now = clock.now();
        Self {
            phases: std::array::from_fn(|_| Histogram::default()),
            metrics: std::array::from_fn(|_| Histogram::default()),
            skipped_catch_ups: 0,
            chunks_this_tick: 0,
            last_lap: now,
            tick_start: now,
            previous_tick_start: None,
            ticks: 0,
            clock,
            report_interval: interval,
            last_report: now,
        }
    }
}

impl Default for Profiler {
    fn default() -> Self {
        Self::with_report_interval(None)
    }
}

/// 取单个直方图时用的键：要么是阶段耗时，要么是别的指标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Series {
    Phase(Phase),
    Metric(Metric),
}

/// 读环境变量，构造剖析器。世界创建时调用一次。
fn report_interval_from_env() -> Option<Duration> {
    let value = env::var(PROFILE_ENV).ok()?;
    let seconds: f64 = value.trim().parse().ok()?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    Some(Duration::from_secs_f64(seconds))
}

impl Profiler {
    /// 世界创建时读一次 `BAMBOO_PROFILE`。`World::new` 及其几个入口都会走到这里。
    pub(crate) fn on_world_start(&mut self) {
        self.report_interval = report_interval_from_env();
        self.last_report = self.clock.now();
    }

    /// 世界线程统一的「现在」。区块入队等事件用这个值打时间戳。
    pub(crate) fn now(&self) -> Instant {
        self.clock.now()
    }

    /// 钉死时钟的剖析器，让墙上时间的测试不依赖真实时钟跑多快。
    #[cfg(test)]
    pub(crate) fn with_manual_clock() -> Self {
        Self::with_clock(
            None,
            Clock::Manual {
                origin: Instant::now(),
                offset: Duration::ZERO,
            },
        )
    }

    /// 只推进钉死的时钟。
    #[cfg(test)]
    pub(crate) fn advance_clock(&mut self, by: Duration) {
        self.clock.advance(by);
    }

    fn histogram_mut(&mut self, series: Series) -> &mut Histogram {
        let (is_metric, index) = Self::series_index(series);
        if is_metric {
            &mut self.metrics[index]
        } else {
            &mut self.phases[index]
        }
    }

    /// 取单个直方图的只读引用，测试和以后的汇总用。
    pub(crate) fn histogram(&self, series: Series) -> &Histogram {
        let (is_metric, index) = Self::series_index(series);
        if is_metric {
            &self.metrics[index]
        } else {
            &self.phases[index]
        }
    }

    /// 这一刻开始。提前返回也不会留下脏状态：下一刻会重新设 `last_lap`，
    /// 也不会把被跳过阶段的耗时算到下一刻。
    pub(crate) fn begin_tick(&mut self) {
        let now = self.clock.now();
        self.tick_start = now;
        self.last_lap = now;
    }

    /// 记下「从上一次 lap 到现在」的耗时，归到 `phase`。hot path 上只做一次
    /// 取时钟、一次时长换算和一次直方图记录。
    pub(crate) fn lap(&mut self, phase: Phase) {
        let now = self.clock.now();
        self.phases[phase as usize].record_duration(now.saturating_duration_since(self.last_lap));
        self.last_lap = now;
    }

    /// 实际开始 − 应该开始。`schedule` 提供的 `expected` 可能晚于 `actual`（提前醒了），
    /// 此时记 0；`skipped` 表示上一刻结束时放弃过追赶。
    ///
    /// `catch_up` 是这一刻有没有在追赶（上一刻没睡、连续跑）。刻间隔偏差只统计
    /// 「上一刻和这一刻都按 50ms 网格正常调度」的相邻对：追赶中的刻之间不睡，
    /// 间隔由 tick 耗时而不是调度精度决定，混进来会把指标撑成几十毫秒。
    /// 追赶结束后的第一对也不记，因为它的前一刻没有按网格睡。
    pub(crate) fn begin_scheduled_tick(
        &mut self,
        expected: Instant,
        actual: Instant,
        skipped: bool,
        catch_up: bool,
    ) {
        self.histogram_mut(Series::Metric(Metric::TickStartLateness))
            .record_duration(actual.saturating_duration_since(expected));
        if skipped {
            self.skipped_catch_ups = self.skipped_catch_ups.saturating_add(1);
        }
        if !catch_up
            && !skipped
            && let Some(previous) = self.previous_tick_start
        {
            let interval = actual.saturating_duration_since(previous);
            let deviation = interval.abs_diff(TICK);
            self.histogram_mut(Series::Metric(Metric::TickIntervalDeviation))
                .record_duration(deviation);
        }
        // 追赶中的刻不能当下一对的基准；放弃追赶后重新按网格起步，也不接着上一次比。
        self.previous_tick_start = (!catch_up && !skipped).then_some(actual);
    }

    /// 记一个数量指标。`SkippedCatchUps` 只有累计值，不进直方图。
    pub(crate) fn record(&mut self, metric: Metric, value: u64) {
        if metric == Metric::SkippedCatchUps {
            self.skipped_catch_ups = self.skipped_catch_ups.saturating_add(value);
        } else {
            self.histogram_mut(Series::Metric(metric)).record(value);
        }
    }

    /// 记一个区块从进入待发送集合到写进出站缓冲的墙上延迟，单位微秒。
    pub(crate) fn record_chunk_latency(&mut self, entered: Instant, sent: Instant) {
        let micros = sent
            .saturating_duration_since(entered)
            .as_micros()
            .min(u128::from(u64::MAX)) as u64;
        self.histogram_mut(Series::Metric(Metric::ChunkLatencyUs))
            .record(micros);
        self.chunks_this_tick = self.chunks_this_tick.saturating_add(1);
    }

    /// 一笔「每刻发出的区块数」样本。即使这一刻一个区块都没发也要记 0。
    pub(crate) fn finish_chunk_sending(&mut self) {
        let chunks = self.chunks_this_tick;
        self.histogram_mut(Series::Metric(Metric::ChunksSent))
            .record(chunks);
        self.chunks_this_tick = 0;
    }

    /// tick 结束：记总耗时，到点就打一份报告并清零。
    pub(crate) fn end_tick(&mut self) {
        let now = self.clock.now();
        let elapsed = now.saturating_duration_since(self.tick_start);
        self.histogram_mut(Series::Metric(Metric::TickTotal))
            .record_duration(elapsed);
        self.ticks = self.ticks.saturating_add(1);
        if let Some(interval) = self.report_interval
            && now.duration_since(self.last_report) >= interval
        {
            tracing::info!("{}", self.report());
            self.reset_samples();
            self.last_report = now;
        }
    }

    fn phase_line(out: &mut String, phase: Phase, histogram: &Histogram) {
        let _ = writeln!(
            out,
            "  {:<24} count={:<8} mean={:>9.3} p50={:>9.3} p95={:>9.3} p99={:>9.3} max={:>9.3}",
            phase.name(),
            histogram.count(),
            millis(histogram.sum(), histogram.count()),
            nanos_to_millis(histogram.p50()),
            nanos_to_millis(histogram.p95()),
            nanos_to_millis(histogram.p99()),
            nanos_to_millis(histogram.max()),
        );
    }

    fn metric_line(out: &mut String, metric: Metric, histogram: &Histogram) {
        let (mean, p50, p95, p99, max) = if metric.is_duration() {
            (
                millis(histogram.sum(), histogram.count()),
                nanos_to_millis(histogram.p50()),
                nanos_to_millis(histogram.p95()),
                nanos_to_millis(histogram.p99()),
                nanos_to_millis(histogram.max()),
            )
        } else {
            // 数量指标原样报，不换算成毫秒。
            let as_f64 = |value: u64| value as f64;
            (
                mean_count(histogram.sum(), histogram.count()),
                as_f64(histogram.p50()),
                as_f64(histogram.p95()),
                as_f64(histogram.p99()),
                as_f64(histogram.max()),
            )
        };
        let _ = writeln!(
            out,
            "  {:<24} samples={:<8} mean={mean:>9.3} p50={p50:>9.3} p95={p95:>9.3} p99={p99:>9.3} max={max:>9.3} unit={}",
            metric.name(),
            histogram.count(),
            metric.unit(),
        );
    }

    /// 一份多行报告：阶段按平均耗时降序，之后是总耗时、调度偏差和各计数指标。
    pub(crate) fn report(&self) -> String {
        let mut out = format!(
            "tick profile: {} ticks, phase times in ms\nphases (by mean):\n",
            self.ticks
        );
        let mut phases: Vec<(Phase, &Histogram)> = Phase::ALL
            .iter()
            .map(|&phase| (phase, self.histogram(Series::Phase(phase))))
            .collect();
        phases.sort_by(|a, b| {
            mean_nanos(b.1)
                .cmp(&mean_nanos(a.1))
                .then_with(|| a.0.cmp(&b.0))
        });
        for (phase, histogram) in phases {
            Self::phase_line(&mut out, phase, histogram);
        }
        out.push_str("metrics (times in ms; count histograms are per tick, otherwise per item):\n");
        for metric in Metric::ALL {
            Self::metric_line(&mut out, metric, self.histogram(Series::Metric(metric)));
        }
        let _ = writeln!(
            out,
            "  {:<24} total={:<8} unit={}",
            Metric::SkippedCatchUps.name(),
            self.skipped_catch_ups,
            Metric::SkippedCatchUps.unit(),
        );
        out.pop();
        out
    }

    /// 把另一个 region 的样本并进来，供主控汇总多个 region 的报告。
    /// 现在只有一个 region，暂未接线；测试覆盖它。
    #[allow(dead_code)]
    pub(crate) fn merge(&mut self, other: &Self) {
        for (histogram, source) in self.phases.iter_mut().zip(&other.phases) {
            histogram.merge(source);
        }
        for (histogram, source) in self.metrics.iter_mut().zip(&other.metrics) {
            histogram.merge(source);
        }
        self.skipped_catch_ups = self
            .skipped_catch_ups
            .saturating_add(other.skipped_catch_ups);
        self.ticks = self.ticks.saturating_add(other.ticks);
    }

    /// 打印之后清零，只清样本，不动环境变量和报告时刻。
    pub(crate) fn reset_samples(&mut self) {
        for histogram in &mut self.phases {
            histogram.reset();
        }
        for histogram in &mut self.metrics {
            histogram.reset();
        }
        self.skipped_catch_ups = 0;
        self.chunks_this_tick = 0;
        self.ticks = 0;
        // 清零后不再有「上一刻」可比，第一刻和报告之间的间隔不算样本。
        self.previous_tick_start = None;
    }
}

fn mean_nanos(histogram: &Histogram) -> u128 {
    if histogram.count() == 0 {
        0
    } else {
        histogram.sum() / u128::from(histogram.count())
    }
}

fn nanos_to_millis(nanos: u64) -> f64 {
    nanos as f64 / 1_000_000.0
}

fn millis(sum: u128, count: u64) -> f64 {
    nanos_to_millis((sum.min(u128::from(u64::MAX))) as u64) / count.max(1) as f64
}

fn mean_count(sum: u128, count: u64) -> f64 {
    sum.min(u128::from(u64::MAX)) as f64 / count.max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK_FOR_TEST: Duration = Duration::from_millis(50);

    #[test]
    fn bucket_bounds_are_monotonic_and_cover_the_range() {
        assert_eq!(bucket_index(0), 0);
        assert_eq!(bucket_index(15), 15);
        assert_eq!(bucket_index(16), LINEAR_BUCKETS);
        assert_eq!(bucket_index(u64::MAX), BUCKETS - 1);
        for index in 1..BUCKETS {
            assert!(
                bucket_lower_bound(index) > bucket_lower_bound(index - 1),
                "bucket {index}"
            );
        }
        for value in [
            0,
            1,
            7,
            8,
            9,
            15,
            16,
            17,
            1_000_000,
            1_u64 << MAX_EXPONENT,
            u64::MAX / 2,
            u64::MAX,
        ] {
            let index = bucket_index(value);
            assert!(bucket_lower_bound(index) <= value, "{value}");
        }
    }

    #[test]
    fn histogram_matches_exact_statistics() {
        let values = [0_u64, 1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 100, 1_000_000];
        let mut histogram = Histogram::default();
        for value in values {
            histogram.record(value);
        }
        assert_eq!(histogram.count(), values.len() as u64);
        assert_eq!(histogram.sum(), values.iter().map(|&v| u128::from(v)).sum());
        assert_eq!(histogram.max(), 1_000_000);
    }

    /// 100 个值：0..99 纳秒。p50/p95/p99 分别落在 50、95、99 所在的桶里。
    #[test]
    fn percentiles_land_in_the_expected_buckets() {
        let mut histogram = Histogram::default();
        for value in 0..100_u64 {
            histogram.record(value);
        }
        for (percentile, expected) in [(50, 50_u64), (95, 95), (99, 99)] {
            let lower = bucket_lower_bound(bucket_index(expected));
            let got = histogram.percentile(percentile);
            assert!(
                got == lower,
                "p{percentile}: {got} 应该在 {expected} 所在的桶 {lower}"
            );
            assert!(got <= expected);
            assert!(got > expected - expected / 8 - 1);
        }
        assert_eq!(histogram.p50(), histogram.percentile(50));
        assert_eq!(histogram.p95(), histogram.percentile(95));
        assert_eq!(histogram.p99(), histogram.percentile(99));
        assert_eq!(Histogram::default().p99(), 0);
    }

    #[test]
    fn merge_adds_up_and_reset_clears() {
        let mut left = Histogram::default();
        left.record(10);
        left.record(1_000);
        let mut right = Histogram::default();
        right.record(20);
        right.record(5_000);
        right.record(5_000);

        left.merge(&right);
        assert_eq!(left.count(), 5);
        assert_eq!(left.sum(), 10 + 1_000 + 20 + 5_000 + 5_000);
        assert_eq!(left.max(), 5_000);
        // 合并后中位数是第 3 小值 1000，必须落在它所在的桶里。
        assert_eq!(left.p50(), bucket_lower_bound(bucket_index(1_000)));
        left.reset();
        assert_eq!(left, Histogram::default());
        assert_eq!(left.count(), 0);
        assert_eq!(left.p99(), 0);
    }

    #[test]
    fn lap_charges_time_to_the_named_phase() {
        let mut profiler = Profiler::with_report_interval(None);
        profiler.begin_tick();
        for phase in Phase::ALL {
            std::thread::sleep(Duration::from_micros(50));
            profiler.lap(phase);
        }
        profiler.end_tick();
        for phase in Phase::ALL {
            assert_eq!(
                profiler.histogram(Series::Phase(phase)).count(),
                1,
                "{}",
                phase.name()
            );
        }
        assert_eq!(
            profiler
                .histogram(Series::Metric(Metric::TickTotal))
                .count(),
            1
        );
    }

    #[test]
    fn an_abandoned_tick_does_not_poison_the_next_one() {
        let mut profiler = Profiler::with_report_interval(None);
        profiler.begin_tick();
        std::thread::sleep(Duration::from_millis(2));
        // 模拟 process_inbox 提前返回：没有 lap、没有 end_tick。
        profiler.begin_tick();
        profiler.lap(Phase::Inbox);
        profiler.end_tick();
        assert_eq!(profiler.histogram(Series::Phase(Phase::Inbox)).count(), 1);
        assert_eq!(
            profiler
                .histogram(Series::Phase(Phase::LightUpdates))
                .count(),
            0
        );
        assert_eq!(
            profiler
                .histogram(Series::Metric(Metric::TickTotal))
                .count(),
            1
        );
    }

    #[test]
    fn report_lists_every_phase_and_metric_in_order() {
        let mut profiler = Profiler::with_report_interval(None);
        profiler.begin_tick();
        profiler.record(Metric::OnlinePlayers, 2);
        profiler.begin_scheduled_tick(Instant::now(), Instant::now(), true, false);
        profiler.lap(Phase::Inbox);
        profiler.end_tick();
        let report = profiler.report();
        for phase in Phase::ALL {
            assert!(report.contains(phase.name()), "{}", phase.name());
        }
        for metric in Metric::ALL {
            assert!(report.contains(metric.name()), "{}", metric.name());
        }
        assert!(report.contains(Metric::SkippedCatchUps.name()));
        assert!(report.contains("mean=") && report.contains("p99=") && report.contains("max="));
        let phases_at = report.find("phases (by mean):").unwrap();
        let first = report.find(Phase::Inbox.name()).unwrap();
        assert!(phases_at < first);
        assert_eq!(
            report.lines().count(),
            2 + Phase::ALL.len() + 1 + Metric::ALL.len() + 1
        );
    }

    #[test]
    fn scheduled_tick_records_lateness_and_skipped_catch_ups() {
        let start = Instant::now();
        let mut profiler = Profiler::with_report_interval(None);
        profiler.begin_scheduled_tick(start, start + Duration::from_millis(3), true, false);
        profiler.begin_scheduled_tick(start + TICK_FOR_TEST, start, false, false);
        let lateness = profiler.histogram(Series::Metric(Metric::TickStartLateness));
        assert_eq!(lateness.count(), 2);
        assert_eq!(lateness.sum(), 3_000_000);
        assert_eq!(lateness.max(), 3_000_000);
        assert!(profiler.report().contains("total=1"));
    }

    #[test]
    fn interval_deviation_uses_only_regular_grid_pairs() {
        let start = Instant::now();
        let mut profiler = Profiler::with_report_interval(None);
        let deviation = |profiler: &Profiler| {
            profiler
                .histogram(Series::Metric(Metric::TickIntervalDeviation))
                .clone()
        };

        // 第一刻没有「上一刻」可比；第二刻和第三刻都稳定晚 4ms，间隔正好 50ms。
        profiler.begin_scheduled_tick(start, start + Duration::from_millis(4), false, false);
        assert_eq!(deviation(&profiler).count(), 0);
        profiler.begin_scheduled_tick(
            start + TICK_FOR_TEST,
            start + TICK_FOR_TEST + Duration::from_millis(4),
            false,
            false,
        );
        let first = deviation(&profiler);
        assert_eq!(first.count(), 1);
        assert_eq!(first.sum(), 0, "稳定的 4ms 起点偏差不是刻间隔抖动");

        // 第四刻醒来早了 1ms，和上一刻相隔 45ms，偏差 5ms。
        profiler.begin_scheduled_tick(
            start + 2 * TICK_FOR_TEST,
            start + 2 * TICK_FOR_TEST - Duration::from_millis(1),
            false,
            false,
        );
        let second = deviation(&profiler);
        assert_eq!(second.count(), 2);
        assert_eq!(second.sum(), 5_000_000);
        assert_eq!(second.max(), 5_000_000);

        // 追赶中的刻不记样本，也不能当下一对的基准。
        profiler.begin_scheduled_tick(
            start + 3 * TICK_FOR_TEST,
            start + 3 * TICK_FOR_TEST + Duration::from_millis(20),
            false,
            true,
        );
        assert_eq!(deviation(&profiler).count(), 2);
        profiler.begin_scheduled_tick(
            start + 4 * TICK_FOR_TEST,
            start + 4 * TICK_FOR_TEST - Duration::from_millis(1),
            false,
            false,
        );
        assert_eq!(deviation(&profiler).count(), 2, "追赶后的第一对也不记");
        // 恢复网格后再记一对：这一刻比上一刻正好晚 50ms。
        profiler.begin_scheduled_tick(
            start + 5 * TICK_FOR_TEST,
            start + 5 * TICK_FOR_TEST - Duration::from_millis(1),
            false,
            false,
        );
        let resumed = deviation(&profiler);
        assert_eq!(resumed.count(), 3);
        assert_eq!(resumed.sum(), 5_000_000);
    }

    #[test]
    fn chunk_latency_uses_the_injected_clock() {
        let mut profiler = Profiler::with_manual_clock();
        let entered = profiler.now();
        profiler.advance_clock(Duration::from_millis(3));
        profiler.record_chunk_latency(entered, profiler.now());
        let latency = profiler.histogram(Series::Metric(Metric::ChunkLatencyUs));
        assert_eq!(latency.count(), 1);
        assert_eq!(latency.sum(), 3_000);
        assert_eq!(latency.max(), 3_000);
        profiler.finish_chunk_sending();
        assert_eq!(
            profiler.histogram(Series::Metric(Metric::ChunksSent)).sum(),
            1
        );
    }

    #[test]
    fn profiler_merge_adds_both_regions() {
        let mut left = Profiler::with_report_interval(None);
        left.begin_tick();
        left.record(Metric::OnlinePlayers, 3);
        left.lap(Phase::Inbox);
        left.end_tick();
        let mut right = Profiler::with_report_interval(None);
        right.begin_tick();
        right.record(Metric::OnlinePlayers, 5);
        right.lap(Phase::Inbox);
        right.end_tick();

        left.merge(&right);
        assert_eq!(left.histogram(Series::Phase(Phase::Inbox)).count(), 2);
        assert_eq!(left.histogram(Series::Metric(Metric::TickTotal)).count(), 2);
        assert_eq!(
            left.histogram(Series::Metric(Metric::OnlinePlayers)).sum(),
            8
        );
        assert!(left.report().contains("2 ticks"));
    }

    #[test]
    fn reset_samples_keeps_the_interval() {
        let mut profiler = Profiler::with_report_interval(Some(Duration::from_secs(60)));
        profiler.begin_tick();
        profiler.end_tick();
        profiler.reset_samples();
        assert_eq!(profiler.report_interval, Some(Duration::from_secs(60)));
        assert!(profiler.report().contains("0 ticks"));
    }
}
