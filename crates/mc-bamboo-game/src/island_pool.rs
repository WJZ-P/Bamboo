//! 岛的 tick 线程池（岛的实施计划第 6 节第 7 步）。
//!
//! 主控把停放着的岛整个（`Box<Island>`，只搬一个指针）交给池里的一条线程，线程跑完
//! 这个岛的一刻再把它交回来；岛在线程上跑的时候主控手里没有它，碰不到它。主控这时
//! 只做自己的事（Director 推进），做完在交回通道上等。
//!
//! - 线程常驻，每条显式给 [`ISLAND_THREAD_STACK`] 的栈（调试构建里一刻的调用很深）。
//! - 岛尽量回到上次跑它的那条线程（[`IslandPool::dispatch`] 的 `hint`）；它忙着就给一条
//!   空着的线程；全都忙着时进公共队列，哪条线程先空出来哪条跑。岛从不排在某一条特定的
//!   忙线程后面：混合驱动下（岛的实施计划 5.5）那条线程上可能正跑着一个很慢或者卡死的
//!   岛，排在它后面的健康的岛就被它拖住了。
//! - 岛的一刻里 panic 不会让线程悄悄死掉：线程接住它，连同岛一起交回，由主控在
//!   自己的线程上重新抛出，走世界线程原来的崩溃处理（`docs/world-model-design.md` 第 14 节）。
//! - 线程本身没了（交回通道断开）主控也能发现，同样当成崩溃。
//! - 池子关掉时不等还没交回的岛：卡住的岛不能把关服拖死。
//! - 每次派出带一个派出号，池子记着每条线程手里那一刻是哪个派出号、什么时候开跑的。
//!   主控判一个岛死了（岛的实施计划 5.6）就放弃那条线程（[`IslandPool::abandon`]）：
//!   不再给它派岛、不再算进活着的线程数，同时补一条新线程，最多补
//!   [`MAX_ABANDONED_THREADS`] 条。
//!
//! The island tick-thread pool (island plan, section 6, step 7). The master
//! hands a parked island whole (`Box<Island>`, one pointer) to a pool thread,
//! which runs the island's tick and hands it back; while it runs the master
//! does not hold it. Threads are long-lived with an explicit stack, an island
//! goes back to the thread that ran it last when possible, a panic is caught
//! and handed back with the island so the master re-raises it on its own
//! thread (the world thread's existing crash path), a vanished thread shows up
//! as a disconnected hand-back channel, and dropping the pool never waits for
//! an island that has not come back. When the hinted thread is busy an idle
//! one is used, and when all are busy the island joins a shared queue served
//! by whichever thread frees up first: an island never queues behind one
//! particular busy thread, which under the hybrid drive (island plan 5.5) may
//! be running a very slow or stuck island.

use std::{
    any::Any,
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use tracing::error;

use crate::island::Island;
use crate::island_beacon::{self, Beacon, Progress};

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

/// 线程池最多为被放弃的线程补几条新线程（岛的实施计划 5.6.4）。每放弃一条，在它回来
/// 之前泄漏一条线程和它手里那个岛的内存；补到这个数以后不再补，活着的线程数从此只减
/// 不增，减到 0 时派岛失败、走崩溃路径。
///
/// How many replacement threads the pool starts for abandoned ones (island
/// plan 5.6.4). Each abandoned thread leaks itself and its island's memory
/// until it returns; past this number no replacement is started, the live
/// thread count only goes down, and at 0 dispatching fails into the crash path.
pub(crate) const MAX_ABANDONED_THREADS: usize = 8;

/// 在 tick 线程上跑岛的一刻的函数。
pub(crate) type IslandBody = fn(&mut Island);

/// 岛跑完一刻交回的东西。
///
/// What an island hands back after its tick.
pub(crate) struct HandBack {
    pub(crate) island: Box<Island>,
    /// 这次派出的派出号（[`IslandPool::dispatch`] 的参数）。主控拿它认岛：岛的编号会被
    /// 复用，判死以后同一个编号可能已经是另一个岛。
    ///
    /// The ticket of this dispatch (given to [`IslandPool::dispatch`]). The
    /// master identifies the island by it: island ids are reused, and after
    /// a death the same id may be a different island.
    pub(crate) ticket: u64,
    /// 跑它的线程。
    pub(crate) worker: usize,
    /// 这一刻在 tick 线程上开始 / 结束的时刻。
    pub(crate) started: Instant,
    pub(crate) finished: Instant,
    /// 派出时全部线程都忙着、岛进了公共队列：开跑之前等过别的岛让出线程。
    ///
    /// Every thread was busy at dispatch and the island went to the shared
    /// queue: it waited for another island to free a thread before starting.
    pub(crate) queued: bool,
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

/// 主控和 tick 线程共用的派发状态，一把锁守着。只在派岛和线程取岛的那一下拿锁，岛跑
/// 的时候不拿。
///
/// The dispatch state shared by the master and the tick threads behind one
/// lock, held only while handing an island over or taking one, never while an
/// island runs.
#[derive(Default)]
struct Queues {
    /// 每条线程自己的队列（指名给它的岛）。
    own: Vec<VecDeque<Job>>,
    /// 公共队列：全部线程都忙着时派出的岛，谁先空出来谁跑。
    shared: VecDeque<Job>,
    /// 每条线程手里的岛数：排在它自己队列里的加上正在跑的。
    busy: Vec<usize>,
    /// 每条线程正在跑的那一刻：派出号和开跑的时刻。空着时是 `None`。
    ///
    /// What each thread is running: the dispatch ticket and when it started;
    /// `None` while idle.
    running: Vec<Option<Running>>,
    /// 每条线程的进度信标，下标和线程号一致。
    ///
    /// Each thread's progress beacon, indexed by thread number.
    beacons: Vec<Arc<Beacon>>,
    /// 被放弃的线程（它手里的岛被判死了）：不再派岛给它，它跑完手里的就退出。
    ///
    /// Abandoned threads (their island was declared dead): nothing more is
    /// dispatched to them and they exit once their island's tick ends.
    retired: Vec<bool>,
    /// 还活着的线程数（不含被放弃的）。
    alive: usize,
    /// 池子关了，线程把手里的跑完就退出。
    closed: bool,
}

/// 排着等线程的一次派出。
struct Job {
    island: Box<Island>,
    ticket: u64,
    /// 进的是公共队列。
    shared: bool,
}

#[derive(Clone, Copy)]
struct Running {
    ticket: u64,
    started: Instant,
}

struct Dispatch {
    queues: Mutex<Queues>,
    wake: Condvar,
}

impl Dispatch {
    fn lock(&self) -> std::sync::MutexGuard<'_, Queues> {
        // 线程在 `catch_unwind` 外面 panic 才会毒化；状态只是几条队列，照常用。
        // Poisoned only if a thread panics outside `catch_unwind`; the state
        // is a few queues and stays usable.
        self.queues.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// 岛的 tick 线程池。只有主控用它。
///
/// The island tick-thread pool, used only by the master.
pub(crate) struct IslandPool {
    dispatch: Arc<Dispatch>,
    handles: Vec<Option<JoinHandle<()>>>,
    replies: Receiver<HandBack>,
    /// 交回通道的发端，补新线程时给它一份。池子自己留着一份，所以线程全没了时通道不会
    /// 断开，[`Self::wait`] 改看活着的线程数。
    ///
    /// The hand-back sender, cloned for replacement threads. The pool keeps
    /// one, so the channel never disconnects when every thread is gone;
    /// [`Self::wait`] looks at the live thread count instead.
    reply_sender: Sender<HandBack>,
    body: IslandBody,
    /// 已经放弃了几条线程。
    abandoned: usize,
    /// 测试专用：全部线程都忙着时不进公共队列，排到手里岛最少的那条线程后面（改坏确认）。
    ///
    /// Test only: with every thread busy, queue behind the least loaded one
    /// instead of using the shared queue (break-it check).
    #[cfg(test)]
    pub(crate) no_shared_queue: bool,
}

impl IslandPool {
    /// 起 `threads` 条常驻线程（至少一条），每条都用 `body` 跑交给它的岛。
    ///
    /// Start `threads` long-lived threads (at least one), each running the
    /// islands handed to it with `body`.
    pub(crate) fn new(threads: usize, body: IslandBody) -> std::io::Result<Self> {
        let threads = threads.clamp(1, MAX_ISLAND_THREADS);
        let (reply_sender, replies) = mpsc::channel();
        let dispatch = Arc::new(Dispatch {
            queues: Mutex::new(Queues {
                own: (0..threads).map(|_| VecDeque::new()).collect(),
                shared: VecDeque::new(),
                busy: vec![0; threads],
                running: vec![None; threads],
                beacons: (0..threads).map(|_| Arc::default()).collect(),
                retired: vec![false; threads],
                alive: threads,
                closed: false,
            }),
            wake: Condvar::new(),
        });
        let mut handles = Vec::with_capacity(threads);
        for index in 0..threads {
            let spawned = spawn_worker(index, &dispatch, &reply_sender, body);
            match spawned {
                Ok(handle) => handles.push(Some(handle)),
                Err(error) => {
                    // 起不来：让已经起来的线程退出。
                    // Could not start: let the threads already running exit.
                    dispatch.lock().closed = true;
                    dispatch.wake.notify_all();
                    return Err(error);
                }
            }
        }
        Ok(Self {
            dispatch,
            handles,
            replies,
            reply_sender,
            body,
            abandoned: 0,
            #[cfg(test)]
            no_shared_queue: false,
        })
    }

    /// 线程数（含被放弃的和补上的）。
    #[cfg(test)]
    pub(crate) fn threads(&self) -> usize {
        self.handles.len()
    }

    /// 还活着的线程数（不含被放弃的）。
    #[cfg(test)]
    pub(crate) fn live_threads(&self) -> usize {
        self.dispatch.lock().alive
    }

    /// 在线程上开跑以后已经过了 `limit` 的派出号（岛的实施计划 5.6.1）。还排在队列里等
    /// 线程的不算：它们没有卡住，只是没轮到。
    ///
    /// Tickets whose tick has been running on a thread for at least `limit`
    /// (island plan 5.6.1). Dispatches still queued for a thread do not
    /// count: they are waiting, not stuck.
    pub(crate) fn overdue(&self, limit: Duration) -> Vec<u64> {
        let queues = self.dispatch.lock();
        queues
            .running
            .iter()
            .zip(&queues.retired)
            .filter(|(_, retired)| !**retired)
            .filter_map(|(running, _)| *running)
            .filter(|running| running.started.elapsed() >= limit)
            .map(|running| running.ticket)
            .collect()
    }

    /// 在线程上已经跑了至少 `limit` 的派出号，连同已跑时长和它那条线程信标上的进度。
    ///
    /// Tickets running on a thread for at least `limit`, with how long they
    /// have run and the progress on that thread's beacon.
    pub(crate) fn slow(&self, limit: Duration) -> Vec<(u64, Duration, Progress)> {
        let queues = self.dispatch.lock();
        queues
            .running
            .iter()
            .zip(&queues.beacons)
            .zip(&queues.retired)
            .filter(|(_, retired)| !**retired)
            .filter_map(|((running, beacon), _)| running.map(|running| (running, beacon)))
            .filter(|(running, _)| running.started.elapsed() >= limit)
            .map(|(running, beacon)| (running.ticket, running.started.elapsed(), beacon.progress()))
            .collect()
    }

    /// 派出号 `ticket` 所在线程信标上的进度；没在跑时是 `None`。
    ///
    /// The progress on the beacon of the thread running `ticket`; `None` if it
    /// is not running.
    pub(crate) fn progress_of(&self, ticket: u64) -> Option<Progress> {
        let queues = self.dispatch.lock();
        queues
            .running
            .iter()
            .zip(&queues.beacons)
            .find(|(running, _)| running.is_some_and(|running| running.ticket == ticket))
            .map(|(_, beacon)| beacon.progress())
    }

    /// 派出号 `ticket` 这一刻在线程上已经跑了多久；没在跑时是 `None`。
    ///
    /// How long dispatch `ticket` has been running on a thread; `None` if it
    /// is not running.
    pub(crate) fn running_for(&self, ticket: u64) -> Option<Duration> {
        let queues = self.dispatch.lock();
        queues
            .running
            .iter()
            .flatten()
            .find(|running| running.ticket == ticket)
            .map(|running| running.started.elapsed())
    }

    /// 把还排在队列里、没开跑的派出号 `ticket` 拿回来。已经开跑或者跑完了时是 `None`。
    ///
    /// Take back dispatch `ticket` if it is still queued and has not started;
    /// `None` once it started or finished.
    pub(crate) fn withdraw(&mut self, ticket: u64) -> Option<Box<Island>> {
        let mut queues = self.dispatch.lock();
        if let Some(at) = queues.shared.iter().position(|job| job.ticket == ticket) {
            return queues.shared.remove(at).map(|job| job.island);
        }
        for index in 0..queues.own.len() {
            if let Some(at) = queues.own[index]
                .iter()
                .position(|job| job.ticket == ticket)
            {
                queues.busy[index] = queues.busy[index].saturating_sub(1);
                return queues.own[index].remove(at).map(|job| job.island);
            }
        }
        None
    }

    /// 派出号 `ticket` 是不是已经在某条线程上开跑、还没跑完。
    ///
    /// Whether dispatch `ticket` has started on a thread and not finished.
    #[cfg(test)]
    pub(crate) fn is_running(&self, ticket: u64) -> bool {
        let queues = self.dispatch.lock();
        queues
            .running
            .iter()
            .flatten()
            .any(|running| running.ticket == ticket)
    }

    /// 放弃正在跑派出号 `ticket` 的那条线程（岛的实施计划 5.6.4）：不再给它派岛、不再把它
    /// 算进活着的线程数，指名排在它后面的岛改进公共队列；还没补满
    /// [`MAX_ABANDONED_THREADS`] 条时补一条新线程。它哪天跑完了照常交回（主控按派出号
    /// 认出来丢弃），然后退出。那一刻已经跑完、或者还没开跑时什么都不做，返回 `false`。
    ///
    /// Abandon the thread running dispatch `ticket` (island plan 5.6.4):
    /// nothing more is dispatched to it, it no longer counts as alive, the
    /// islands queued behind it move to the shared queue, and a replacement
    /// starts unless [`MAX_ABANDONED_THREADS`] were already replaced. If it
    /// ever finishes it hands back as usual (the master discards by ticket)
    /// and exits. Returns `false`, doing nothing, if that tick already
    /// finished or has not started.
    pub(crate) fn abandon(&mut self, ticket: u64) -> bool {
        let replace = {
            let mut queues = self.dispatch.lock();
            let Some(index) = queues
                .running
                .iter()
                .position(|running| running.is_some_and(|running| running.ticket == ticket))
            else {
                return false;
            };
            if queues.retired[index] {
                return false;
            }
            queues.retired[index] = true;
            queues.alive = queues.alive.saturating_sub(1);
            let waiting: Vec<Job> = queues.own[index].drain(..).collect();
            queues.busy[index] = queues.busy[index].saturating_sub(waiting.len());
            queues.shared.extend(waiting);
            self.abandoned += 1;
            self.abandoned <= MAX_ABANDONED_THREADS
        };
        if replace {
            let index = {
                let mut queues = self.dispatch.lock();
                queues.own.push(VecDeque::new());
                queues.busy.push(0);
                queues.running.push(None);
                queues.beacons.push(Arc::default());
                queues.retired.push(false);
                queues.alive += 1;
                queues.own.len() - 1
            };
            match spawn_worker(index, &self.dispatch, &self.reply_sender, self.body) {
                Ok(handle) => self.handles.push(Some(handle)),
                Err(error) => {
                    error!(%error, "could not start a replacement island tick thread");
                    let mut queues = self.dispatch.lock();
                    queues.retired[index] = true;
                    queues.alive = queues.alive.saturating_sub(1);
                    self.handles.push(None);
                }
            }
        } else {
            error!(
                abandoned = self.abandoned,
                "too many island tick threads were abandoned; not starting another"
            );
        }
        self.dispatch.wake.notify_all();
        true
    }

    /// 测试专用：把派出号 `ticket` 的开跑时刻往前拨 `by`（让它看起来已经跑了这么久）。
    ///
    /// Test only: move the start of dispatch `ticket` back by `by`, as if it
    /// had been running that long.
    #[cfg(test)]
    pub(crate) fn backdate(&self, ticket: u64, by: Duration) -> bool {
        let mut queues = self.dispatch.lock();
        for running in queues.running.iter_mut().flatten() {
            if running.ticket == ticket {
                running.started = running.started.checked_sub(by).unwrap_or(running.started);
                return true;
            }
        }
        false
    }

    /// 把岛交给一条线程。`hint` 是上次跑它的线程：它空着就还用它，不然用一条空着的线程；
    /// 全都忙着时进公共队列，哪条先空出来哪条跑。线程已经全没了时把岛原样退回。
    ///
    /// Hand an island to a thread: the `hint` thread (the one that ran it
    /// last) if it is idle, otherwise an idle one; when all are busy it joins
    /// the shared queue for whichever frees up first. If every thread is gone
    /// the island is handed back.
    pub(crate) fn dispatch(
        &mut self,
        hint: usize,
        island: Box<Island>,
        ticket: u64,
    ) -> Result<(), Box<Island>> {
        let mut queues = self.dispatch.lock();
        if queues.alive == 0 {
            return Err(island);
        }
        let hint = hint.min(queues.busy.len() - 1);
        let free =
            |queues: &Queues, index: usize| queues.busy[index] == 0 && !queues.retired[index];
        #[cfg_attr(not(test), allow(unused_mut))]
        let mut idle = if free(&queues, hint) {
            Some(hint)
        } else {
            (0..queues.busy.len()).find(|&index| free(&queues, index))
        };
        #[cfg(test)]
        if idle.is_none() && self.no_shared_queue {
            idle = (0..queues.busy.len())
                .filter(|&index| !queues.retired[index])
                .min_by_key(|&index| (queues.busy[index], index));
        }
        let mut job = Job {
            island,
            ticket,
            shared: false,
        };
        match idle {
            Some(index) => {
                queues.busy[index] += 1;
                queues.own[index].push_back(job);
            }
            None => {
                job.shared = true;
                queues.shared.push_back(job);
            }
        }
        drop(queues);
        self.dispatch.wake.notify_all();
        Ok(())
    }

    /// 等一个岛交回，最多等 `timeout`。
    ///
    /// Wait up to `timeout` for an island to come back.
    pub(crate) fn wait(&mut self, timeout: Duration) -> Wait {
        match self.replies.recv_timeout(timeout) {
            Ok(back) => Wait::Back(Box::new(back)),
            // 池子自己留着一个发端，通道不会断：线程全没了看活着的线程数。
            // The pool keeps a sender, so the channel never disconnects; a
            // pool with no thread left shows in the live count.
            Err(RecvTimeoutError::Timeout) if self.dispatch.lock().alive == 0 => Wait::Lost,
            Err(RecvTimeoutError::Timeout) => Wait::Pending,
            Err(RecvTimeoutError::Disconnected) => Wait::Lost,
        }
    }
}

impl Drop for IslandPool {
    /// 关掉派发；空着的线程等它退出，还拿着岛的线程（卡住了）不等。
    ///
    /// Close the dispatch; wait for idle threads, but never for one that
    /// still holds an island (it may be stuck).
    fn drop(&mut self) {
        let (busy, retired) = {
            let mut queues = self.dispatch.lock();
            queues.closed = true;
            (queues.busy.clone(), queues.retired.clone())
        };
        self.dispatch.wake.notify_all();
        for (index, handle) in self.handles.iter_mut().enumerate() {
            let Some(handle) = handle.take() else {
                continue;
            };
            if busy.get(index).copied().unwrap_or(0) > 0
                || retired.get(index).copied().unwrap_or(false)
            {
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

/// 起一条 tick 线程。
fn spawn_worker(
    index: usize,
    dispatch: &Arc<Dispatch>,
    replies: &Sender<HandBack>,
    body: IslandBody,
) -> std::io::Result<JoinHandle<()>> {
    let replies = replies.clone();
    let shared = Arc::clone(dispatch);
    thread::Builder::new()
        .name(format!("bamboo-island-{index}"))
        .stack_size(ISLAND_THREAD_STACK)
        .spawn(move || run_worker(index, &shared, &replies, body))
}

/// 线程退出时（包括意外 panic）把自己从活着的线程里减掉。被放弃的线程在放弃的那一刻
/// 已经减过了。
struct Alive<'a>(&'a Dispatch, usize);

impl Drop for Alive<'_> {
    fn drop(&mut self) {
        let mut queues = self.0.lock();
        if !queues.retired[self.1] {
            queues.alive = queues.alive.saturating_sub(1);
            // 意外退出的线程不再接活：指名排给它的岛改进公共队列。
            // A thread that exits unexpectedly takes no more work; islands
            // queued for it move to the shared queue.
            queues.retired[self.1] = true;
            let waiting: Vec<Job> = queues.own[self.1].drain(..).collect();
            queues.busy[self.1] = queues.busy[self.1].saturating_sub(waiting.len());
            queues.shared.extend(waiting);
        }
    }
}

/// tick 线程结束时收掉它自己留的实体工作池（岛的一刻里实体并行用的那几条线程）。在
/// 线程正常退出的路上收，不留给线程本地的析构。
///
/// When a tick thread ends, tear down the entity worker pool it kept for
/// itself (the threads an island's tick uses for parallel entities). Done on
/// the thread's way out, not left to a thread-local destructor.
struct LocalWorkers;

impl Drop for LocalWorkers {
    fn drop(&mut self) {
        crate::interaction::release_local_entity_pool();
    }
}

/// 一条 tick 线程：取一个岛（先取指名给自己的，再取公共队列的）、跑一刻、交回，直到
/// 池子关掉或者没人收交回。
///
/// One tick thread: take an island (its own queue first, then the shared
/// one), run its tick and hand it back, until the pool closes or nobody takes
/// the hand-backs.
fn run_worker(index: usize, dispatch: &Dispatch, replies: &Sender<HandBack>, body: IslandBody) {
    let _alive = Alive(dispatch, index);
    let _workers = LocalWorkers;
    let beacon = Arc::clone(&dispatch.lock().beacons[index]);
    island_beacon::install(Arc::clone(&beacon));
    loop {
        let Job {
            mut island,
            ticket,
            shared,
        } = {
            let mut queues = dispatch.lock();
            let job = loop {
                if queues.retired[index] {
                    return;
                }
                if let Some(job) = queues.own[index].pop_front() {
                    break job;
                }
                if let Some(job) = queues.shared.pop_front() {
                    queues.busy[index] += 1;
                    break job;
                }
                if queues.closed {
                    return;
                }
                queues = dispatch
                    .wake
                    .wait(queues)
                    .unwrap_or_else(PoisonError::into_inner);
            };
            queues.running[index] = Some(Running {
                ticket: job.ticket,
                started: Instant::now(),
            });
            job
        };
        let started = Instant::now();
        beacon.start_tick();
        // 岛的状态在 panic 之后可能不一致，但主控拿回它只是为了照原来的崩溃路径
        // 重新抛出、不再跑它，所以这里断言可以安全地跨过 unwind。
        // The island may be inconsistent after a panic; the master only takes
        // it back to re-raise on the existing crash path and never runs it again.
        let outcome = catch_unwind(AssertUnwindSafe(|| body(&mut island)));
        let finished = Instant::now();
        {
            let mut queues = dispatch.lock();
            queues.busy[index] = queues.busy[index].saturating_sub(1);
            queues.running[index] = None;
        }
        let back = HandBack {
            island,
            ticket,
            worker: index,
            started,
            finished,
            queued: shared,
            panic: outcome.err(),
        };
        if replies.send(back).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::island::IslandTestHook;

    /// 测试用的岛的一刻：带着闸就卡在闸上，直到闸打开。
    fn stall_body(island: &mut Island) {
        if let Some(IslandTestHook::Stall(gate)) = &island.test_hook {
            while !gate.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(1));
            }
        }
    }

    /// 一个只有空 `Region`、带着闸的岛。
    fn stalled_island(gate: &Arc<AtomicBool>) -> Box<Island> {
        let mut island = Island::new(
            mc_bamboo_world::director::HOME_REGION,
            crate::region::Region::new(2, 20, false),
            crate::director_host::MailBudget::default(),
        );
        island.test_hook = Some(IslandTestHook::Stall(Arc::clone(gate)));
        Box::new(island)
    }

    /// 等派出号 `ticket` 在某条线程上开跑。
    fn wait_running(pool: &IslandPool, ticket: u64) {
        let started = Instant::now();
        while !pool.is_running(ticket) {
            assert!(started.elapsed() < Duration::from_secs(60), "没开跑");
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// 放弃线程（岛的实施计划 5.6.4）：前 [`MAX_ABANDONED_THREADS`] 次各补一条新线程，
    /// 活着的线程数不变；之后不再补，活着的线程数一次少一条，到 0 时派岛失败、等岛报
    /// 「线程全没了」。排在被放弃的线程后面的岛改由别的线程跑；只看开跑了多久，排着队的
    /// 不算超时；被放弃的线程跑完以后照常交回（带着派出号）然后退出。
    ///
    /// Abandoning threads (island plan 5.6.4): the first
    /// [`MAX_ABANDONED_THREADS`] each start a replacement and the live count
    /// stays; afterwards none is started, each abandon costs a live thread,
    /// and at 0 dispatching fails and waiting reports every thread gone.
    /// Islands queued behind an abandoned thread run elsewhere; only time
    /// spent running counts as overdue; an abandoned thread that finishes
    /// hands back as usual (with its ticket) and exits.
    #[test]
    fn abandoned_threads_are_replaced_up_to_the_limit() {
        let mut pool = IslandPool::new(2, stall_body).expect("起线程池");
        let mut gates = Vec::new();
        let mut ticket = 0_u64;
        // 两条线程都卡住，第三个岛排在公共队列里。
        // Both threads stuck, a third island waits in the shared queue.
        for _ in 0..2 {
            ticket += 1;
            let gate = Arc::new(AtomicBool::new(false));
            assert!(pool.dispatch(0, stalled_island(&gate), ticket).is_ok());
            wait_running(&pool, ticket);
            gates.push(gate);
        }
        let queued_gate = Arc::new(AtomicBool::new(true));
        ticket += 1;
        let queued = ticket;
        assert!(
            pool.dispatch(0, stalled_island(&queued_gate), queued)
                .is_ok()
        );
        assert!(!pool.is_running(queued), "两条线程都忙着，它排着");
        assert_eq!(pool.running_for(queued), None);
        pool.backdate(1, Duration::from_secs(100));
        assert_eq!(
            pool.overdue(Duration::from_secs(50)),
            vec![1],
            "只有开跑了 50 秒以上的算超时，排着的不算"
        );
        assert!(pool.abandon(1), "放弃正在跑 1 号的线程");
        assert!(!pool.abandon(1), "已经放弃过");
        assert_eq!(pool.live_threads(), 2, "补了一条");
        assert_eq!(pool.threads(), 3);
        // 排着的岛由补上的线程跑完交回。
        // The queued island runs on the replacement and comes back.
        let started = Instant::now();
        loop {
            match pool.wait(Duration::from_millis(50)) {
                Wait::Back(back) => {
                    assert_eq!(back.ticket, queued);
                    break;
                }
                Wait::Pending => assert!(started.elapsed() < Duration::from_secs(60)),
                Wait::Lost => panic!("线程没全没"),
            }
        }
        assert!(
            pool.overdue(Duration::from_secs(50)).is_empty(),
            "被放弃的不再算"
        );
        assert!(pool.abandon(2), "另一条卡住的也放弃");
        assert_eq!(pool.live_threads(), 2, "又补了一条");
        // 接着放弃：补到上限为止。
        // Keep abandoning: replacements stop at the limit.
        let mut live = pool.live_threads();
        let mut abandoned = 2;
        while live > 0 {
            ticket += 1;
            let gate = Arc::new(AtomicBool::new(false));
            assert!(pool.dispatch(0, stalled_island(&gate), ticket).is_ok());
            wait_running(&pool, ticket);
            gates.push(gate);
            assert!(pool.abandon(ticket));
            abandoned += 1;
            let now = pool.live_threads();
            if abandoned <= MAX_ABANDONED_THREADS {
                assert_eq!(now, live, "第 {abandoned} 次放弃补了一条");
            } else {
                assert_eq!(now, live - 1, "第 {abandoned} 次放弃不再补");
            }
            live = now;
        }
        assert_eq!(
            abandoned,
            MAX_ABANDONED_THREADS + 2,
            "补满以后两条线程各放弃一次"
        );
        let gate = Arc::new(AtomicBool::new(true));
        assert!(
            pool.dispatch(0, stalled_island(&gate), ticket + 1).is_err(),
            "没有线程了"
        );
        assert!(matches!(pool.wait(Duration::ZERO), Wait::Lost));
        // 开闸：被放弃的线程把岛交回（带着当时的派出号）然后退出。
        // Open the gates: abandoned threads hand back with their tickets and exit.
        for gate in &gates {
            gate.store(true, Ordering::Release);
        }
        let mut back = Vec::new();
        let started = Instant::now();
        while back.len() < gates.len() {
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "被放弃的线程没交回"
            );
            if let Ok(hand) = pool.replies.recv_timeout(Duration::from_millis(50)) {
                back.push(hand.ticket);
            }
        }
        back.sort_unstable();
        let mut expected: Vec<u64> = (1..=ticket).filter(|&t| t != queued).collect();
        expected.sort_unstable();
        assert_eq!(back, expected);
    }
}
