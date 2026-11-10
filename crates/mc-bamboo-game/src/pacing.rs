//! 世界线程的节拍：怎么睡到下一刻、这条线程用什么优先级。
//!
//! 开关在 [`Pacer::from_env`] 里读一次环境变量，默认保持原来的 `thread::sleep` 行为。
//! 等待逻辑不直接碰系统时钟，而是走 [`Timing`]，测试用假时钟精确推进。
//!
//! 各平台线程优先级的现状：macOS 可以在世界线程里调
//! `pthread_set_qos_class_self_np` 把自己设成用户交互级 QoS；Linux 和 Windows
//! 这次没有实现，行为没有验证。这些平台上的 `set_thread_priority` 是空操作。
//! Linux 上 `thread::sleep` 的精度本来就在百微秒量级（本次没有验证），
//! Windows 留给以后。

use std::{
    env,
    hint::spin_loop,
    thread,
    time::{Duration, Instant},
};

use tracing::{info, warn};

/// 怎么睡到下一刻。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SleepStrategy {
    /// 一直 `thread::sleep` 到截止时刻。
    #[default]
    Plain,
    /// 先睡到「截止时刻 − 自适应余量」，剩下的时间自旋到截止时刻。
    Hybrid,
}

impl SleepStrategy {
    fn name(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Hybrid => "hybrid",
        }
    }
}

/// 世界线程的优先级设置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ThreadPriority {
    /// 不动当前线程的优先级。
    #[default]
    Default,
    /// macOS：用户交互级 QoS。其它平台上是空操作。
    Interactive,
}

impl ThreadPriority {
    fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Interactive => "interactive",
        }
    }
}

const SLEEP_ENV: &str = "BAMBOO_TICK_SLEEP";
const SPIN_MAX_ENV: &str = "BAMBOO_TICK_SPIN_MAX_US";
const QOS_ENV: &str = "BAMBOO_TICK_QOS";
const DEFAULT_SPIN_MAX_US: u64 = 6000;
/// 余量的下限：即使最近没睡过头，也留这么一段给自旋，好让余量能跟着时钟变化。
const MIN_MARGIN: Duration = Duration::from_micros(250);
/// 最近睡过头记录的最大值之上再加一点，免得刚好卡在上一次的超时时限上。
const MARGIN_SLACK: Duration = Duration::from_micros(250);
/// 记录最近这么多次睡眠的超时量。
const OVERSLEEP_HISTORY: usize = 64;
/// 创建世界时读到的节拍配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PacingConfig {
    pub(crate) sleep: SleepStrategy,
    /// hybrid 每刻最多愿意自旋多久；0 等价于 plain。
    pub(crate) spin_max: Duration,
    pub(crate) priority: ThreadPriority,
}

impl Default for PacingConfig {
    fn default() -> Self {
        Self {
            sleep: SleepStrategy::Plain,
            spin_max: Duration::from_micros(DEFAULT_SPIN_MAX_US),
            priority: ThreadPriority::Default,
        }
    }
}

impl PacingConfig {
    /// 从环境变量读一次；不认识的值记警告并回落到缺省值。
    fn from_env() -> Self {
        let read = |name| env::var(name).ok();
        Self::parse(
            read(SLEEP_ENV).as_deref(),
            read(SPIN_MAX_ENV).as_deref(),
            read(QOS_ENV).as_deref(),
        )
    }

    /// 解析三个开关。环境变量没设（`None`）和值不认识都走缺省值，但后者要记警告。
    fn parse(sleep: Option<&str>, spin_max: Option<&str>, qos: Option<&str>) -> Self {
        let mut config = Self::default();
        match sleep {
            Some("plain") => config.sleep = SleepStrategy::Plain,
            Some("hybrid") => config.sleep = SleepStrategy::Hybrid,
            Some(value) => warn!(
                value,
                env = SLEEP_ENV,
                "unknown tick sleep strategy, using plain"
            ),
            None => {}
        }
        if let Some(value) = spin_max {
            match value.trim().parse::<u64>() {
                Ok(micros) => config.spin_max = Duration::from_micros(micros),
                Err(_) => warn!(
                    value,
                    env = SPIN_MAX_ENV,
                    "invalid tick spin limit, using {} us",
                    DEFAULT_SPIN_MAX_US
                ),
            }
        }
        match qos {
            Some("default") => config.priority = ThreadPriority::Default,
            Some("interactive") => config.priority = ThreadPriority::Interactive,
            Some(value) => warn!(value, env = QOS_ENV, "unknown tick QoS, using default"),
            None => {}
        }
        config
    }

    /// 启动时打一条日志，免得事后分不清哪份结果是哪种配置。
    fn log(self) {
        info!(
            sleep = self.sleep.name(),
            spin_max_us = self.spin_max.as_micros(),
            qos = self.priority.name(),
            "tick pacing"
        );
    }
}

/// 等待时碰到的系统操作。测试钉死时间后手动推进，不依赖真实睡眠快慢。
pub(crate) trait Timing {
    fn now(&self) -> Instant;
    /// 睡 `duration`；假时钟直接把时间推进到期望醒来的时刻。
    fn sleep(&mut self, duration: Duration);
    /// 自旋循环里的一次迭代。生产实现调 `spin_loop`，测试的假时钟可以顺便推进时间。
    fn spin_once(&mut self);
}

/// 生产环境用的真实单调时钟和真实睡眠。
pub(crate) struct WallTiming;

impl Timing for WallTiming {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&mut self, duration: Duration) {
        thread::sleep(duration);
    }

    fn spin_once(&mut self) {
        spin_loop();
    }
}

/// 世界线程独占的节拍器，保存自适应余量要用的历史。
pub(crate) struct Pacer {
    config: PacingConfig,
    /// 最近 [`OVERSLEEP_HISTORY`] 次 `thread::sleep` 睡过头了多久，环形覆盖。
    oversleeps: [Duration; OVERSLEEP_HISTORY],
    oversleep_count: usize,
    next_oversleep: usize,
}

impl Pacer {
    /// 创建世界时读一次环境变量。
    pub(crate) fn from_env() -> Self {
        let config = PacingConfig::from_env();
        config.log();
        Self::new(config)
    }

    pub(crate) fn new(config: PacingConfig) -> Self {
        Self {
            config,
            oversleeps: [Duration::ZERO; OVERSLEEP_HISTORY],
            oversleep_count: 0,
            next_oversleep: 0,
        }
    }

    /// 世界线程开始跑 `World::run` 时设置自己的优先级。失败只记日志。
    pub(crate) fn set_thread_priority(&self) {
        if self.config.priority == ThreadPriority::Interactive {
            set_interactive_qos();
        }
    }

    /// 睡（必要时自旋）到 `deadline`，返回这一刻自旋了多久。
    /// 返回时一定不早于截止时刻；追赶中的刻（等待时间为 0）不睡也不自旋。
    pub(crate) fn wait_until<T: Timing>(&mut self, deadline: Instant, timing: &mut T) -> Duration {
        let start = timing.now();
        let wait = deadline.saturating_duration_since(start);
        if wait.is_zero() {
            return Duration::ZERO;
        }
        let hybrid = self.config.sleep == SleepStrategy::Hybrid && !self.config.spin_max.is_zero();
        if !hybrid {
            self.sleep_until(deadline, timing);
            return Duration::ZERO;
        }

        let margin = self.margin().min(wait).min(self.config.spin_max);
        let wake_at = deadline - margin;
        // 请求睡到 wake_at；如果提前醒了，继续补睡，直到到点或超时。
        self.sleep_until(wake_at, timing);
        if timing.now() >= deadline {
            // 这次睡过头太多，整段余量都算作自旋占核的时间。
            return margin;
        }

        // 余量已按上限夹过，睡眠超时只会让实际自旋变短，不会再超出预算。
        let spin_budget = self.config.spin_max;
        loop {
            let now = timing.now();
            let spun = now.saturating_duration_since(wake_at);
            if now >= deadline {
                return margin;
            }
            if spun >= spin_budget {
                // 预算用完就退回短睡，仍然保证返回时不早于截止时刻。
                self.sleep_until(deadline, timing);
                return margin;
            }
            timing.spin_once();
        }
    }

    /// 反复补睡，直到 `deadline`，防系统提前唤醒。每次睡眠的超时量记进历史。
    fn sleep_until<T: Timing>(&mut self, deadline: Instant, timing: &mut T) {
        loop {
            let now = timing.now();
            if now >= deadline {
                return;
            }
            let requested = deadline.saturating_duration_since(now);
            timing.sleep(requested);
            let woke = timing.now();
            self.record_oversleep(woke.saturating_duration_since(deadline));
            if woke >= deadline {
                return;
            }
        }
    }

    fn record_oversleep(&mut self, oversleep: Duration) {
        self.oversleeps[self.next_oversleep] = oversleep;
        self.next_oversleep = (self.next_oversleep + 1) % OVERSLEEP_HISTORY;
        self.oversleep_count = self.oversleep_count.saturating_add(1);
    }

    /// 最近睡过头量的最大值加一点余量，夹在下限和每刻自旋上限之间。
    fn margin(&self) -> Duration {
        let upper = self.config.spin_max;
        let lower = MIN_MARGIN.min(upper);
        let recent_max = self.oversleeps[..self.oversleep_count.min(OVERSLEEP_HISTORY)]
            .iter()
            .copied()
            .max()
            .unwrap_or(Duration::ZERO);
        recent_max
            .saturating_add(MARGIN_SLACK)
            .clamp(lower, upper.max(lower))
    }
}

/// 把当前线程设成 macOS 的用户交互级 QoS。
///
/// # Safety
///
/// 这个函数自己没有 unsafe 前置条件；内部唯一的 `unsafe` 是调用 libc 的
/// `pthread_set_qos_class_self_np`：两个参数都是按值传入的常量，不涉及指针、
/// 生命周期或跨线程共享状态，函数只改调用它的这条线程的 QoS；返回非 0 时
/// 只记一条警告，不影响世界运行。
#[cfg(target_os = "macos")]
fn set_interactive_qos() {
    let result = unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
    };
    if result != 0 {
        warn!(error = result, "could not raise the world thread QoS");
    }
}

/// Linux 和 Windows 这次不做任何事，接口留在这里；这些平台的行为没有验证。
#[cfg(not(target_os = "macos"))]
fn set_interactive_qos() {
    warn!("interactive tick QoS is not implemented on this platform");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 假时钟：`sleep` 按请求时长再加固定的超时量推进，`spin_once` 以固定步长
    /// 推进。记录每次请求的睡眠时长和自旋次数。
    const SPIN_STEP: Duration = Duration::from_micros(50);

    #[derive(Debug)]
    struct FakeTiming {
        now: Instant,
        oversleep: Duration,
        sleep_requests: Vec<Duration>,
        spins: usize,
    }

    impl FakeTiming {
        fn new(oversleep: Duration) -> Self {
            Self {
                now: Instant::now(),
                oversleep,
                sleep_requests: Vec::new(),
                spins: 0,
            }
        }
    }

    impl Timing for FakeTiming {
        fn now(&self) -> Instant {
            self.now
        }

        fn sleep(&mut self, duration: Duration) {
            self.sleep_requests.push(duration);
            self.now += duration + self.oversleep;
        }

        fn spin_once(&mut self) {
            self.spins += 1;
            self.now += SPIN_STEP;
        }
    }

    fn hybrid(spin_max_us: u64) -> PacingConfig {
        PacingConfig {
            sleep: SleepStrategy::Hybrid,
            spin_max: Duration::from_micros(spin_max_us),
            ..PacingConfig::default()
        }
    }

    #[test]
    fn env_defaults_match_the_old_behaviour() {
        assert_eq!(
            PacingConfig::parse(None, None, None),
            PacingConfig::default()
        );
        assert_eq!(
            PacingConfig::parse(Some("plain"), Some("6000"), Some("default")),
            PacingConfig::default()
        );
    }

    #[test]
    fn env_overrides_and_invalid_values_fall_back() {
        assert_eq!(
            PacingConfig::parse(Some("hybrid"), Some("2500"), Some("interactive")),
            PacingConfig {
                sleep: SleepStrategy::Hybrid,
                spin_max: Duration::from_micros(2500),
                priority: ThreadPriority::Interactive,
            }
        );
        assert_eq!(
            PacingConfig::parse(Some("other"), Some("abc"), Some("other")),
            PacingConfig::default()
        );
        assert_eq!(
            PacingConfig::parse(Some("hybrid"), Some("0"), Some("interactive")),
            PacingConfig {
                sleep: SleepStrategy::Hybrid,
                spin_max: Duration::ZERO,
                priority: ThreadPriority::Interactive,
            }
        );
    }

    #[test]
    fn plain_sleeps_the_whole_wait_and_never_spins() {
        let mut pacer = Pacer::new(PacingConfig::default());
        let mut timing = FakeTiming::new(Duration::from_millis(4));
        let deadline = timing.now() + Duration::from_millis(50);
        assert_eq!(pacer.wait_until(deadline, &mut timing), Duration::ZERO);
        assert_eq!(timing.sleep_requests, [Duration::from_millis(50)]);
        assert_eq!(timing.spins, 0);
        assert_eq!(timing.now(), deadline + Duration::from_millis(4));
    }

    #[test]
    fn hybrid_margin_grows_with_oversleep_history_and_is_clamped() {
        let mut pacer = Pacer::new(hybrid(6000));
        assert_eq!(pacer.margin(), MIN_MARGIN, "没有记录时用下限");

        // 几次不同的超时量：余量跟着最大值走。
        pacer.record_oversleep(Duration::from_millis(1));
        pacer.record_oversleep(Duration::from_millis(3));
        assert_eq!(pacer.margin(), Duration::from_micros(3250));

        // 更大的超时量把余量顶到上限，超出的部分被夹住。
        pacer.record_oversleep(Duration::from_millis(20));
        assert_eq!(pacer.margin(), Duration::from_micros(6000));

        // 历史只保留最近 64 次，老的大值退出窗口后余量回落。
        for _ in 0..OVERSLEEP_HISTORY {
            pacer.record_oversleep(Duration::from_micros(100));
        }
        assert_eq!(pacer.margin(), Duration::from_micros(350));
    }

    #[test]
    fn margin_obeys_a_tiny_upper_bound() {
        let mut pacer = Pacer::new(hybrid(100));
        assert_eq!(pacer.margin(), Duration::from_micros(100));
        pacer.record_oversleep(Duration::from_millis(9));
        assert_eq!(pacer.margin(), Duration::from_micros(100));
    }

    #[test]
    fn zero_spin_limit_behaves_like_plain() {
        let mut pacer = Pacer::new(hybrid(0));
        let mut timing = FakeTiming::new(Duration::from_millis(2));
        let deadline = timing.now() + Duration::from_millis(50);
        assert_eq!(pacer.wait_until(deadline, &mut timing), Duration::ZERO);
        assert_eq!(timing.sleep_requests, [Duration::from_millis(50)]);
        assert_eq!(timing.spins, 0);
        assert!(timing.now() >= deadline);
    }

    #[test]
    fn catching_up_does_not_wait_at_all() {
        for config in [PacingConfig::default(), hybrid(6000)] {
            let mut pacer = Pacer::new(config);
            let mut timing = FakeTiming::new(Duration::from_millis(1));
            let now = timing.now();
            assert_eq!(pacer.wait_until(now, &mut timing), Duration::ZERO);
            assert_eq!(
                pacer.wait_until(now - Duration::from_millis(5), &mut timing),
                Duration::ZERO
            );
            assert!(timing.sleep_requests.is_empty());
            assert_eq!(timing.spins, 0);
            assert_eq!(timing.now(), now);
        }
    }

    #[test]
    fn hybrid_sleeps_then_spins_and_returns_after_the_deadline() {
        let mut pacer = Pacer::new(hybrid(6000));
        let mut timing = FakeTiming::new(Duration::ZERO);
        let deadline = timing.now() + Duration::from_millis(50);
        let spun = pacer.wait_until(deadline, &mut timing);
        assert_eq!(
            timing.sleep_requests,
            [Duration::from_millis(50) - MIN_MARGIN]
        );
        assert_eq!(spun, MIN_MARGIN);
        assert_eq!(timing.spins, 5);
        assert!(timing.now() >= deadline);
        // 下一次余量按这次睡眠的超时量（0）算，仍是下限。
        assert_eq!(pacer.margin(), MIN_MARGIN);
    }

    #[test]
    fn hybrid_uses_oversleep_history_to_wake_earlier() {
        let mut pacer = Pacer::new(hybrid(6000));
        // 先让 plain 记下 3 ms 的睡过头量。
        for _ in 0..3 {
            pacer.record_oversleep(Duration::from_millis(3));
        }
        pacer.config.sleep = SleepStrategy::Hybrid;
        let mut timing = FakeTiming::new(Duration::ZERO);
        let deadline = timing.now() + Duration::from_millis(50);
        let spun = pacer.wait_until(deadline, &mut timing);
        assert_eq!(
            timing.sleep_requests,
            [Duration::from_millis(50) - Duration::from_micros(3250)]
        );
        assert_eq!(spun, Duration::from_micros(3250));
        assert!(timing.now() >= deadline);
    }

    #[test]
    fn early_wakeups_are_followed_by_another_sleep() {
        let mut pacer = Pacer::new(PacingConfig::default());
        let mut timing = EarlyTiming::new();
        let deadline = timing.now + Duration::from_millis(50);
        assert_eq!(pacer.wait_until(deadline, &mut timing), Duration::ZERO);
        assert_eq!(timing.requests.len(), 2, "提前醒后要补睡");
        assert_eq!(timing.now, deadline);
    }

    /// `sleep` 第一次只推进请求时长的一半，之后按请求时长精确推进。
    struct EarlyTiming {
        now: Instant,
        requests: Vec<Duration>,
    }

    impl EarlyTiming {
        fn new() -> Self {
            Self {
                now: Instant::now(),
                requests: Vec::new(),
            }
        }
    }

    impl Timing for EarlyTiming {
        fn now(&self) -> Instant {
            self.now
        }

        fn sleep(&mut self, duration: Duration) {
            self.requests.push(duration);
            self.now += if self.requests.len() == 1 {
                duration / 2
            } else {
                duration
            };
        }

        fn spin_once(&mut self) {
            self.now += Duration::from_micros(50);
        }
    }

    #[test]
    fn an_overshooting_sleep_still_returns_after_the_deadline() {
        let mut pacer = Pacer::new(hybrid(6000));
        let mut timing = FakeTiming::new(Duration::from_millis(20));
        let deadline = timing.now() + Duration::from_millis(50);
        let spun = pacer.wait_until(deadline, &mut timing);
        assert_eq!(spun, MIN_MARGIN, "睡过头后整段余量都算自旋占核时间");
        assert_eq!(timing.spins, 0, "没机会自旋");
        assert!(timing.now() >= deadline);
        // 睡过头量进了历史，下一次余量被上限夹住。
        assert_eq!(pacer.margin(), Duration::from_micros(6000));
    }

    #[test]
    fn real_clock_never_returns_early() {
        for config in [PacingConfig::default(), hybrid(1000)] {
            let mut pacer = Pacer::new(config);
            let mut timing = WallTiming;
            let deadline = Instant::now() + Duration::from_millis(2);
            let spun = pacer.wait_until(deadline, &mut timing);
            assert!(Instant::now() >= deadline);
            assert!(spun <= Duration::from_millis(20), "自旋时长只是个读数");
        }
    }
}
