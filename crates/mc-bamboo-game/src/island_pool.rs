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

/// 主控和 tick 线程共用的派发状态，一把锁守着。只在派岛和线程取岛的那一下拿锁，岛跑
/// 的时候不拿。
///
/// The dispatch state shared by the master and the tick threads behind one
/// lock, held only while handing an island over or taking one, never while an
/// island runs.
#[derive(Default)]
struct Queues {
    /// 每条线程自己的队列（指名给它的岛）。
    own: Vec<VecDeque<Box<Island>>>,
    /// 公共队列：全部线程都忙着时派出的岛，谁先空出来谁跑。
    shared: VecDeque<Box<Island>>,
    /// 每条线程手里的岛数：排在它自己队列里的加上正在跑的。
    busy: Vec<usize>,
    /// 还活着的线程数。
    alive: usize,
    /// 池子关了，线程把手里的跑完就退出。
    closed: bool,
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
                alive: threads,
                closed: false,
            }),
            wake: Condvar::new(),
        });
        let mut handles = Vec::with_capacity(threads);
        for index in 0..threads {
            let replies = reply_sender.clone();
            let shared = Arc::clone(&dispatch);
            let spawned = thread::Builder::new()
                .name(format!("bamboo-island-{index}"))
                .stack_size(ISLAND_THREAD_STACK)
                .spawn(move || run_worker(index, &shared, &replies, body));
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
            #[cfg(test)]
            no_shared_queue: false,
        })
    }

    /// 线程数。
    #[cfg(test)]
    pub(crate) fn threads(&self) -> usize {
        self.handles.len()
    }

    /// 把岛交给一条线程。`hint` 是上次跑它的线程：它空着就还用它，不然用一条空着的线程；
    /// 全都忙着时进公共队列，哪条先空出来哪条跑。线程已经全没了时把岛原样退回。
    ///
    /// Hand an island to a thread: the `hint` thread (the one that ran it
    /// last) if it is idle, otherwise an idle one; when all are busy it joins
    /// the shared queue for whichever frees up first. If every thread is gone
    /// the island is handed back.
    pub(crate) fn dispatch(&mut self, hint: usize, island: Box<Island>) -> Result<(), Box<Island>> {
        let mut queues = self.dispatch.lock();
        if queues.alive == 0 {
            return Err(island);
        }
        let hint = hint.min(queues.busy.len() - 1);
        #[cfg_attr(not(test), allow(unused_mut))]
        let mut idle = if queues.busy[hint] == 0 {
            Some(hint)
        } else {
            (0..queues.busy.len()).find(|&index| queues.busy[index] == 0)
        };
        #[cfg(test)]
        if idle.is_none() && self.no_shared_queue {
            idle = (0..queues.busy.len()).min_by_key(|&index| (queues.busy[index], index));
        }
        match idle {
            Some(index) => {
                queues.busy[index] += 1;
                queues.own[index].push_back(island);
            }
            None => queues.shared.push_back(island),
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
        let busy = {
            let mut queues = self.dispatch.lock();
            queues.closed = true;
            queues.busy.clone()
        };
        self.dispatch.wake.notify_all();
        for (index, handle) in self.handles.iter_mut().enumerate() {
            let Some(handle) = handle.take() else {
                continue;
            };
            if busy.get(index).copied().unwrap_or(0) > 0 {
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

/// 线程退出时（包括意外 panic）把自己从活着的线程里减掉。
struct Alive<'a>(&'a Dispatch);

impl Drop for Alive<'_> {
    fn drop(&mut self) {
        self.0.lock().alive -= 1;
    }
}

/// 一条 tick 线程：取一个岛（先取指名给自己的，再取公共队列的）、跑一刻、交回，直到
/// 池子关掉或者没人收交回。
///
/// One tick thread: take an island (its own queue first, then the shared
/// one), run its tick and hand it back, until the pool closes or nobody takes
/// the hand-backs.
fn run_worker(index: usize, dispatch: &Dispatch, replies: &Sender<HandBack>, body: IslandBody) {
    let _alive = Alive(dispatch);
    loop {
        let mut island = {
            let mut queues = dispatch.lock();
            loop {
                if let Some(island) = queues.own[index].pop_front() {
                    break island;
                }
                if let Some(island) = queues.shared.pop_front() {
                    queues.busy[index] += 1;
                    break island;
                }
                if queues.closed {
                    return;
                }
                queues = dispatch
                    .wake
                    .wait(queues)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        let started = Instant::now();
        // 岛的状态在 panic 之后可能不一致，但主控拿回它只是为了照原来的崩溃路径
        // 重新抛出、不再跑它，所以这里断言可以安全地跨过 unwind。
        // The island may be inconsistent after a panic; the master only takes
        // it back to re-raise on the existing crash path and never runs it again.
        let outcome = catch_unwind(AssertUnwindSafe(|| body(&mut island)));
        let finished = Instant::now();
        {
            let mut queues = dispatch.lock();
            queues.busy[index] = queues.busy[index].saturating_sub(1);
        }
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
