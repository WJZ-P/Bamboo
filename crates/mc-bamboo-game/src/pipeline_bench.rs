//! 只编进基准的独占阶段计时与计算池空队列积分。
//!
//! Benchmark-only exclusive phase timing and pool empty-queue integration.

use std::{
    cell::Cell,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub(crate) enum Phase {
    Dispatch,
    Assemble,
    InstallLight,
    Spawn,
    Deliver,
    DeliverStorage,
}

const PHASES: usize = Phase::DeliverStorage as usize + 1;

#[derive(Clone, Copy, Default)]
pub struct WorldTimes([Duration; PHASES]);

impl WorldTimes {
    pub fn fields(self) -> [(&'static str, Duration); PHASES] {
        [
            ("dispatch", self.0[Phase::Dispatch as usize]),
            ("assemble_rest", self.0[Phase::Assemble as usize]),
            ("install_light", self.0[Phase::InstallLight as usize]),
            ("spawn", self.0[Phase::Spawn as usize]),
            ("deliver_rest", self.0[Phase::Deliver as usize]),
            ("deliver_storage", self.0[Phase::DeliverStorage as usize]),
        ]
    }
}

#[derive(Clone, Copy, Default)]
struct Measurement {
    totals: WorldTimes,
    active: Option<(Phase, Instant)>,
}

impl Measurement {
    fn switch(&mut self, phase: Option<Phase>, now: Instant) -> Option<Phase> {
        let previous = std::mem::replace(&mut self.active, phase.map(|p| (p, now)));
        previous.map(|(p, started)| {
            self.totals.0[p as usize] += now - started;
            p
        })
    }
}

thread_local! {
    static MEASUREMENT: Cell<Option<Measurement>> = const { Cell::new(None) };
}

pub fn start() {
    MEASUREMENT.set(Some(Measurement::default()));
}

pub fn stop() -> WorldTimes {
    let measure = MEASUREMENT.take().expect("measurement not started");
    assert!(measure.active.is_none());
    measure.totals
}

pub(crate) struct Scope {
    previous: Option<Phase>,
    enabled: bool,
}

pub(crate) fn enter(phase: Phase) -> Scope {
    let mut scope = Scope {
        previous: None,
        enabled: false,
    };
    // 用 get/set 而不是 Cell::update：后者 1.99 才稳定，超过项目 MSRV 1.96。
    // Use get/set instead of Cell::update, stabilized in 1.99, past the 1.96 MSRV.
    MEASUREMENT.with(|cell| {
        if let Some(mut m) = cell.get() {
            scope.previous = m.switch(Some(phase), Instant::now());
            scope.enabled = true;
            cell.set(Some(m));
        }
    });
    scope
}

impl Drop for Scope {
    fn drop(&mut self) {
        if self.enabled {
            MEASUREMENT.with(|cell| {
                if let Some(mut m) = cell.get() {
                    m.switch(self.previous, Instant::now());
                    cell.set(Some(m));
                }
            });
        }
    }
}

/// 在队列锁内积分：只累计队列为空且 worker 已进入 condvar 等待的时段。
///
/// Integrate under the queue lock, counting only condition-variable waiters
/// while the queue is empty.
pub(crate) struct QueueMeter {
    empty: bool,
    waiters: u32,
    last: Instant,
    total: Duration,
}

impl Default for QueueMeter {
    fn default() -> Self {
        Self {
            empty: true,
            waiters: 0,
            last: Instant::now(),
            total: Duration::ZERO,
        }
    }
}

impl QueueMeter {
    fn at(&self, now: Instant) -> Duration {
        self.total
            + if self.empty {
                (now - self.last) * self.waiters
            } else {
                Duration::ZERO
            }
    }

    fn advance(&mut self, now: Instant) {
        self.total = self.at(now);
        self.last = now;
    }

    pub(crate) fn set_empty(&mut self, empty: bool) {
        if self.empty != empty {
            self.advance(Instant::now());
            self.empty = empty;
        }
    }

    pub(crate) fn begin_wait(&mut self) {
        self.advance(Instant::now());
        self.waiters += 1;
    }

    pub(crate) fn end_wait(&mut self) {
        self.advance(Instant::now());
        self.waiters -= 1;
    }

    pub(crate) fn empty_wait(&self) -> Duration {
        self.at(Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_phases_are_exclusive() {
        let t0 = Instant::now();
        let at = |n| t0 + Duration::from_millis(n);
        let mut m = Measurement::default();
        assert!(m.switch(Some(Phase::Dispatch), at(0)).is_none());
        let outer = m.switch(Some(Phase::Assemble), at(2));
        m.switch(outer, at(5));
        m.switch(None, at(7));
        assert_eq!(
            m.totals.0[Phase::Dispatch as usize],
            Duration::from_millis(4)
        );
        assert_eq!(
            m.totals.0[Phase::Assemble as usize],
            Duration::from_millis(3)
        );
    }

    #[test]
    fn waiters_do_not_count_while_jobs_are_queued() {
        let t0 = Instant::now();
        let at = |n| t0 + Duration::from_millis(n);
        let mut queue = QueueMeter {
            empty: true,
            waiters: 2,
            last: t0,
            total: Duration::ZERO,
        };
        queue.advance(at(3));
        queue.empty = false;
        queue.advance(at(8));
        queue.empty = true;
        queue.waiters = 1;
        assert_eq!(queue.at(at(10)), Duration::from_millis(8));
    }
}
