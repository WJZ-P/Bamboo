//! 岛的进度信标：tick 线程每跑完一段就记下「刚跑完哪一段、什么时候」，主控在岛跑得太久
//! 时读它，报出岛卡在哪一段。只在岛的 tick 线程上记；世界线程上没有信标，记录是空操作。
//!
//! An island's progress beacon: the tick thread records "which phase just
//! finished, and when" after every phase, and the master reads it when an
//! island runs too long to report where it is stuck. Only island tick threads
//! carry a beacon; on the world thread recording is a no-op.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::profiler::Phase;

/// 「这一刻刚开始，还没跑完任何一段」。
///
/// "This tick has just started; no phase has finished yet."
const TICK_STARTED: u8 = u8::MAX;

/// 一条 tick 线程的信标。两个原子量各自独立更新，读到的可能是相邻两次记录的混合，
/// 只用来写日志，够用。
///
/// One tick thread's beacon. The two atomics update independently, so a read
/// may mix two neighbouring records; it only feeds log lines, which is enough.
#[derive(Debug, Default)]
pub(crate) struct Beacon {
    phase: AtomicU8,
    at_nanos: AtomicU64,
}

/// 读出来的进度：最后跑完的一段（`None` 表示这一刻还没跑完任何一段）和距离那次记录
/// 过了多久。
///
/// A progress reading: the last finished phase (`None` when this tick has not
/// finished any phase yet) and how long ago that was recorded.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Progress {
    pub(crate) last_phase: Option<&'static str>,
    pub(crate) since: Duration,
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Beacon>>> = const { RefCell::new(None) };
}

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn now_nanos() -> u64 {
    epoch().elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
}

impl Beacon {
    fn store(&self, phase: u8) {
        self.phase.store(phase, Ordering::Relaxed);
        self.at_nanos.store(now_nanos(), Ordering::Relaxed);
    }

    /// 一刻开跑时调用：清掉上一刻的记录。
    ///
    /// Called when a tick starts: clears the previous tick's record.
    pub(crate) fn start_tick(&self) {
        self.store(TICK_STARTED);
    }

    /// 读当前进度。
    ///
    /// Read the current progress.
    pub(crate) fn progress(&self) -> Progress {
        let phase = self.phase.load(Ordering::Relaxed);
        let at = self.at_nanos.load(Ordering::Relaxed);
        let since = Duration::from_nanos(now_nanos().saturating_sub(at));
        let last_phase = (phase != TICK_STARTED)
            .then(|| Phase::from_index(phase).map(Phase::name))
            .flatten();
        Progress { last_phase, since }
    }
}

/// 把 `beacon` 装成当前线程的信标（tick 线程开工时调用一次）。
///
/// Install `beacon` as this thread's beacon (once, when a tick thread starts).
pub(crate) fn install(beacon: Arc<Beacon>) {
    CURRENT.with(|current| *current.borrow_mut() = Some(beacon));
}

/// 当前线程刚跑完 `phase`。没装信标的线程上什么也不做。
///
/// This thread has just finished `phase`. Does nothing on a thread without a
/// beacon.
pub(crate) fn mark(phase: Phase) {
    CURRENT.with(|current| {
        if let Some(beacon) = current.borrow().as_ref() {
            beacon.store(phase as u8);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_beacon_reports_the_last_finished_phase() {
        let beacon = Arc::new(Beacon::default());
        beacon.start_tick();
        assert_eq!(beacon.progress().last_phase, None);
        install(Arc::clone(&beacon));
        mark(Phase::RandomTicks);
        assert_eq!(beacon.progress().last_phase, Some("random_ticks"));
        CURRENT.with(|current| *current.borrow_mut() = None);
        mark(Phase::Flush);
        assert_eq!(
            beacon.progress().last_phase,
            Some("random_ticks"),
            "a thread without a beacon records nothing"
        );
    }
}
