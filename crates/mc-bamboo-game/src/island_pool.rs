//! 岛的 tick 线程池（岛的实施计划第 6 节第 7 步）。
//!
//! 主控把停放着的岛整个（`Box<Island>`，只搬一个指针）交给池里的一条线程，线程跑完
//! 这个岛的一刻再把它交回来；岛在线程上跑的时候主控手里没有它，碰不到它。主控这时
//! 只做自己的事（Director 推进），做完在交回通道上等。
//!
//! - 线程常驻，每条显式给 [`ISLAND_THREAD_STACK`] 的栈（调试构建里一刻的调用很深）。
//! - 岛尽量回到上次跑它的那条线程（[`IslandPool::dispatch`] 的 `hint`）。
//! - 岛的一刻里 panic 不会让线程悄悄死掉：线程接住它，连同岛一起交回，由主控在
//!   自己的线程上重新抛出，走世界线程原来的崩溃处理（`docs/world-model-design.md` 第 14 节）。
//! - 线程本身没了（交回通道断开）主控也能发现，同样当成崩溃。
//! - 池子关掉时不等还没交回的岛：卡住的岛不能把关服拖死。
//!
//! The island tick-thread pool (island plan, section 6, step 7). The master
//! hands a parked island whole (`Box<Island>`, one pointer) to a pool thread,
//! which runs the island's tick and hands it back; while it runs the master
//! does not hold it. Threads are long-lived with an explicit stack, an island
//! goes back to the thread that ran it last when possible, a panic is caught
//! and handed back with the island so the master re-raises it on its own
//! thread (the world thread's existing crash path), a vanished thread shows up
//! as a disconnected hand-back channel, and dropping the pool never waits for
//! an island that has not come back.

use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use tracing::error;

use crate::island::Island;

/// tick 线程的栈。调试构建里方块逻辑、实体和命令的调用链很深，测试线程默认的 2 MB
/// 不够用；这里只是保留虚拟地址，用多少提交多少。
///
/// The tick thread's stack. Debug builds call deep through block logic,
/// entities and commands, past the 2 MB test-thread default; this only
/// reserves address space.
pub(crate) const ISLAND_THREAD_STACK: usize = 64 * 1024 * 1024;

/// 池子的线程数上限，防住配置文件里的巨值。
///
/// Upper bound on the pool size, against huge values in the config file.
pub const MAX_ISLAND_THREADS: usize = 64;

/// 在 tick 线程上跑岛的一刻的函数。
pub(crate) type IslandBody = fn(&mut Island);

/// 岛跑完一刻交回的东西。
///
/// What an island hands back after its tick.
pub(crate) struct HandBack {
    pub(crate) island: Box<Island>,
    /// 跑它的线程。
    pub(crate) worker: usize,
    /// 这一刻在 tick 线程上开始 / 结束的时刻。
    pub(crate) started: Instant,
    pub(crate) finished: Instant,
    /// 跑的时候 panic 了：载荷，主控在自己线程上重新抛出。
    pub(crate) panic: Option<Box<dyn Any + Send>>,
}

/// 等交回的结果。
pub(crate) enum Wait {
    Back(Box<HandBack>),
    /// 这一段时间里还没回来。
    Pending,
    /// 所有 tick 线程都没了，岛不会再回来。
    Lost,
}

struct Worker {
    jobs: Option<Sender<Box<Island>>>,
    handle: Option<JoinHandle<()>>,
    /// 交给它、还没交回的岛数。
    outstanding: usize,
}

/// 岛的 tick 线程池。只有主控用它。
///
/// The island tick-thread pool, used only by the master.
pub(crate) struct IslandPool {
    workers: Vec<Worker>,
    replies: Receiver<HandBack>,
}

impl IslandPool {
    /// 起 `threads` 条常驻线程（至少一条），每条都用 `body` 跑交给它的岛。
    ///
    /// Start `threads` long-lived threads (at least one), each running the
    /// islands handed to it with `body`.
    pub(crate) fn new(threads: usize, body: IslandBody) -> std::io::Result<Self> {
        let threads = threads.clamp(1, MAX_ISLAND_THREADS);
        let (reply_sender, replies) = mpsc::channel();
        let mut workers = Vec::with_capacity(threads);
        for index in 0..threads {
            let (jobs, inbox) = mpsc::channel::<Box<Island>>();
            let replies = reply_sender.clone();
            let handle = thread::Builder::new()
                .name(format!("bamboo-island-{index}"))
                .stack_size(ISLAND_THREAD_STACK)
                .spawn(move || run_worker(index, &inbox, &replies, body))?;
            workers.push(Worker {
                jobs: Some(jobs),
                handle: Some(handle),
                outstanding: 0,
            });
        }
        Ok(Self { workers, replies })
    }

    /// 线程数。
    #[cfg(test)]
    pub(crate) fn threads(&self) -> usize {
        self.workers.len()
    }

    /// 把岛交给一条线程。`hint` 是上次跑它的线程：它空着就还用它，不然挑最闲的。
    /// 返回实际交给的线程。线程已经没了时把岛原样退回。
    ///
    /// Hand an island to a thread: the `hint` thread (the one that ran it
    /// last) if it is idle, otherwise the least loaded one. Returns the chosen
    /// thread, or the island itself if that thread is gone.
    pub(crate) fn dispatch(
        &mut self,
        hint: usize,
        island: Box<Island>,
    ) -> Result<usize, Box<Island>> {
        let hint = hint.min(self.workers.len() - 1);
        let index = if self.workers[hint].outstanding == 0 {
            hint
        } else {
            (0..self.workers.len())
                .min_by_key(|&index| (self.workers[index].outstanding, index))
                .unwrap_or(hint)
        };
        let worker = &mut self.workers[index];
        let Some(jobs) = &worker.jobs else {
            return Err(island);
        };
        match jobs.send(island) {
            Ok(()) => {
                worker.outstanding += 1;
                Ok(index)
            }
            Err(mpsc::SendError(island)) => Err(island),
        }
    }

    /// 等一个岛交回，最多等 `timeout`。
    ///
    /// Wait up to `timeout` for an island to come back.
    pub(crate) fn wait(&mut self, timeout: Duration) -> Wait {
        match self.replies.recv_timeout(timeout) {
            Ok(back) => {
                if let Some(worker) = self.workers.get_mut(back.worker) {
                    worker.outstanding = worker.outstanding.saturating_sub(1);
                }
                Wait::Back(Box::new(back))
            }
            Err(RecvTimeoutError::Timeout) => Wait::Pending,
            Err(RecvTimeoutError::Disconnected) => Wait::Lost,
        }
    }
}

impl Drop for IslandPool {
    /// 关掉每条线程的任务通道；空着的线程等它退出，还拿着岛的线程（卡住了）不等。
    ///
    /// Close every job channel; wait for idle threads, but never for one that
    /// still holds an island (it may be stuck).
    fn drop(&mut self) {
        for worker in &mut self.workers {
            worker.jobs = None;
        }
        for (index, worker) in self.workers.iter_mut().enumerate() {
            let Some(handle) = worker.handle.take() else {
                continue;
            };
            if worker.outstanding > 0 {
                error!(
                    thread = index,
                    "an island tick thread still holds its island; leaving it behind"
                );
                continue;
            }
            let _ = handle.join();
        }
    }
}

/// 一条 tick 线程：收一个岛、跑一刻、交回，直到任务通道关掉或者没人收交回。
fn run_worker(
    index: usize,
    inbox: &Receiver<Box<Island>>,
    replies: &Sender<HandBack>,
    body: IslandBody,
) {
    while let Ok(mut island) = inbox.recv() {
        let started = Instant::now();
        // 岛的状态在 panic 之后可能不一致，但主控拿回它只是为了照原来的崩溃路径
        // 重新抛出、不再跑它，所以这里断言可以安全地跨过 unwind。
        // The island may be inconsistent after a panic; the master only takes
        // it back to re-raise on the existing crash path and never runs it again.
        let outcome = catch_unwind(AssertUnwindSafe(|| body(&mut island)));
        let finished = Instant::now();
        let back = HandBack {
            island,
            worker: index,
            started,
            finished,
            panic: outcome.err(),
        };
        if replies.send(back).is_err() {
            break;
        }
    }
}
