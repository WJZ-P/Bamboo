//! 世界线程的节拍：用 `thread::sleep` 睡到下一刻。
//!
//! 等待逻辑不直接碰系统时钟，而是走 [`Timing`]，测试用假时钟精确推进。

use std::{
    thread,
    time::{Duration, Instant},
};

/// 等待时碰到的系统操作。测试钉死时间后手动推进，不依赖真实睡眠快慢。
pub(crate) trait Timing {
    fn now(&self) -> Instant;
    /// 睡 `duration`；假时钟直接把时间推进到期望醒来的时刻。
    fn sleep(&mut self, duration: Duration);
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
}

/// 世界线程独占的节拍器。
#[derive(Debug, Default)]
pub(crate) struct Pacer;

impl Pacer {
    /// 睡到 `deadline`，返回时一定不早于截止时刻；追赶中的刻（等待时间为 0）不睡。
    pub(crate) fn wait_until<T: Timing>(&mut self, deadline: Instant, timing: &mut T) {
        self.sleep_until(deadline, timing);
    }

    /// 反复补睡，直到 `deadline`，防系统提前唤醒。
    fn sleep_until<T: Timing>(&self, deadline: Instant, timing: &mut T) {
        loop {
            let now = timing.now();
            if now >= deadline {
                return;
            }
            timing.sleep(deadline.saturating_duration_since(now));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 假时钟：`sleep` 按请求时长再加固定的超时量推进，记录每次请求的睡眠时长。
    #[derive(Debug)]
    struct FakeTiming {
        now: Instant,
        oversleep: Duration,
        sleep_requests: Vec<Duration>,
    }

    impl FakeTiming {
        fn new(oversleep: Duration) -> Self {
            Self {
                now: Instant::now(),
                oversleep,
                sleep_requests: Vec::new(),
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
    }

    #[test]
    fn sleeps_the_whole_wait() {
        let mut pacer = Pacer;
        let mut timing = FakeTiming::new(Duration::from_millis(4));
        let deadline = timing.now() + Duration::from_millis(50);
        pacer.wait_until(deadline, &mut timing);
        assert_eq!(timing.sleep_requests, [Duration::from_millis(50)]);
        assert_eq!(timing.now(), deadline + Duration::from_millis(4));
    }

    #[test]
    fn catching_up_does_not_wait_at_all() {
        let mut pacer = Pacer;
        let mut timing = FakeTiming::new(Duration::from_millis(1));
        let now = timing.now();
        pacer.wait_until(now, &mut timing);
        pacer.wait_until(now - Duration::from_millis(5), &mut timing);
        assert!(timing.sleep_requests.is_empty());
        assert_eq!(timing.now(), now);
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
    }

    #[test]
    fn early_wakeups_are_followed_by_another_sleep() {
        let mut pacer = Pacer;
        let mut timing = EarlyTiming::new();
        let deadline = timing.now + Duration::from_millis(50);
        pacer.wait_until(deadline, &mut timing);
        assert_eq!(timing.requests.len(), 2, "提前醒后要补睡");
        assert_eq!(timing.now, deadline);
    }

    #[test]
    fn real_clock_never_returns_early() {
        let mut pacer = Pacer;
        let mut timing = WallTiming;
        let deadline = Instant::now() + Duration::from_millis(2);
        pacer.wait_until(deadline, &mut timing);
        assert!(Instant::now() >= deadline);
    }
}
