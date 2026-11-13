//! 常驻的后台计算池：一组线程跑纯计算任务，回执经信箱送回世界线程。
//!
//! 池子本身不认识区块：提交的是一个实现了 [`PoolJob`] 的任务和它的编号、
//! 优先级，收回的是 `(编号, 回执)`。区块的解析、生成阶段、序列化由
//! `director_host` 包成任务交进来（设计稿 5.3、第 12 节）。实体并行驱动现在
//! 每刻新起线程，以后也可以改成往这里交任务。
//!
//! - 队列：一个 `Mutex<BinaryHeap>` 加 `Condvar`，按 `(优先级, 编号)` 从小到大出队，
//!   同优先级先提交的先跑。
//! - 取消：只对还在队列里的任务生效，出队时改调 [`PoolJob::cancelled`]；已经
//!   开始跑的照常跑完。每个提交过的任务**恰好回一次执**，调用方据此记账。
//! - 任务 panic 被 `catch_unwind` 接住，交给 [`PoolJob::finish`] 变成失败回执，
//!   线程不退出。
//! - 回执到达的先后取决于线程调度；调用方自己负责按编号排好再用。
//!
//! A resident background compute pool. It knows nothing about chunks: callers
//! submit a [`PoolJob`] with an id and a priority and receive `(id, reply)`.
//! Queued jobs can be cancelled; every submitted job replies exactly once. A
//! panicking job becomes a failure reply instead of killing the worker. Reply
//! order depends on scheduling, so callers must order replies themselves.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashSet},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// 池子里跑的一个任务。
///
/// `run` 拿 `&mut self`，这样任务 panic 时它手里的数据（例如要存盘的区块束）
/// 还在，`finish` 能把它原样放进失败回执里交还，不会丢。
pub(crate) trait PoolJob: Send + 'static {
    /// 计算的结果。
    type Output: Send + 'static;
    /// 送回提交方的回执。
    type Reply: Send + 'static;

    /// 真正的计算。可能 panic。
    fn run(&mut self) -> Self::Output;
    /// 把计算结果（或 panic 的说明）连同任务里剩下的数据变成回执。
    fn finish(self, output: Result<Self::Output, String>) -> Self::Reply;
    /// 还没开始就被取消了。
    fn cancelled(self) -> Self::Reply;
}

/// 队列里的一项。`BinaryHeap` 是大顶堆，键套 `Reverse` 变成小的先出。
struct Queued<J> {
    key: Reverse<(u64, u64)>,
    job: J,
}

impl<J> PartialEq for Queued<J> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<J> Eq for Queued<J> {}

impl<J> PartialOrd for Queued<J> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<J> Ord for Queued<J> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key.cmp(&other.key)
    }
}

struct Queue<J> {
    heap: BinaryHeap<Queued<J>>,
    /// 堆里所有任务的编号，取消时 O(1) 判断任务还在不在队列里。
    queued_ids: HashSet<u64>,
    /// 已经取消、但还在堆里的任务编号。
    cancelled: HashSet<u64>,
    stopping: bool,
}

struct Shared<J> {
    queue: Mutex<Queue<J>>,
    ready: Condvar,
    /// 还活着的工作线程数。只有线程本身出了 `catch_unwind` 接不住的错才会少。
    alive: AtomicUsize,
}

impl<J> Shared<J> {
    /// 锁中毒只可能来自本模块里不会 panic 的几行；真中毒了也照样拿数据，
    /// 队列的不变量不依赖持锁期间完成的半截操作。
    fn lock(&self) -> MutexGuard<'_, Queue<J>> {
        self.queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// 计算池。drop 时通知线程退出并等它们结束；队列里没跑的任务直接丢掉，
/// 所以要等任务做完的调用方先用 [`ComputePool::in_flight`] 排空。
pub(crate) struct ComputePool<J: PoolJob> {
    shared: Arc<Shared<J>>,
    replies: Receiver<(u64, J::Reply)>,
    workers: Vec<JoinHandle<()>>,
    threads: usize,
    /// 提交了、还没被 `try_recv` / `recv_timeout` 收走回执的任务数。
    in_flight: usize,
}

impl<J: PoolJob> ComputePool<J> {
    /// 起 `threads` 条线程（至少 1 条），线程名是 `{name}-{序号}`。
    pub(crate) fn new(name: &str, threads: usize) -> std::io::Result<Self> {
        let threads = threads.max(1);
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                heap: BinaryHeap::new(),
                queued_ids: HashSet::new(),
                cancelled: HashSet::new(),
                stopping: false,
            }),
            ready: Condvar::new(),
            alive: AtomicUsize::new(0),
        });
        let (sender, replies) = mpsc::channel();
        let mut pool = Self {
            shared,
            replies,
            workers: Vec::with_capacity(threads),
            threads,
            in_flight: 0,
        };
        for index in 0..threads {
            let shared = Arc::clone(&pool.shared);
            let sender = sender.clone();
            pool.shared.alive.fetch_add(1, Ordering::SeqCst);
            let spawned = thread::Builder::new()
                .name(format!("{name}-{index}"))
                .spawn(move || worker(&shared, &sender));
            match spawned {
                Ok(handle) => pool.workers.push(handle),
                Err(error) => {
                    pool.shared.alive.fetch_sub(1, Ordering::SeqCst);
                    // 已经起来的线程由 drop 收拾。
                    return Err(error);
                }
            }
        }
        Ok(pool)
    }

    /// 提交一个任务。`priority` 小的先跑，同优先级按 `id` 从小到大。
    /// `id` 由调用方保证不重复。
    pub(crate) fn submit(&mut self, id: u64, priority: u64, job: J) {
        let mut queue = self.shared.lock();
        queue.queued_ids.insert(id);
        queue.heap.push(Queued {
            key: Reverse((priority, id)),
            job,
        });
        drop(queue);
        self.in_flight += 1;
        self.shared.ready.notify_one();
    }

    /// 取消一个还没开始的任务。已经在跑或已经跑完的不受影响。
    /// 被取消的任务仍然会回一次执（[`PoolJob::cancelled`]）。
    pub(crate) fn cancel(&mut self, id: u64) {
        let mut queue = self.shared.lock();
        if queue.queued_ids.contains(&id) {
            queue.cancelled.insert(id);
        }
    }

    /// 线程数。
    pub(crate) fn threads(&self) -> usize {
        self.threads
    }

    /// 还没收回回执的任务数。
    pub(crate) fn in_flight(&self) -> usize {
        self.in_flight
    }

    /// 队列里还没开始的任务数。
    #[cfg(test)]
    pub(crate) fn queued(&self) -> usize {
        self.shared.lock().heap.len()
    }

    /// 有没有线程意外退出。任务的 panic 被接住，不算在这里。
    pub(crate) fn worker_died(&self) -> bool {
        self.shared.alive.load(Ordering::SeqCst) < self.threads
    }

    /// 不阻塞地收一个回执。
    pub(crate) fn try_recv(&mut self) -> Option<(u64, J::Reply)> {
        let reply = self.replies.try_recv().ok()?;
        self.in_flight = self.in_flight.saturating_sub(1);
        Some(reply)
    }

    /// 最多等 `timeout` 收一个回执。只给关服和测试排空用。
    pub(crate) fn recv_timeout(&mut self, timeout: Duration) -> Option<(u64, J::Reply)> {
        match self.replies.recv_timeout(timeout) {
            Ok(reply) => {
                self.in_flight = self.in_flight.saturating_sub(1);
                Some(reply)
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => None,
        }
    }
}

impl<J: PoolJob> Drop for ComputePool<J> {
    fn drop(&mut self) {
        {
            let mut queue = self.shared.lock();
            queue.stopping = true;
            queue.heap.clear();
            queue.queued_ids.clear();
            queue.cancelled.clear();
        }
        self.shared.ready.notify_all();
        for handle in self.workers.drain(..) {
            let _ = handle.join();
        }
    }
}

/// 工作线程的主循环：取一个任务，跑完把回执发回去；收到停止信号就退出。
fn worker<J: PoolJob>(shared: &Shared<J>, replies: &Sender<(u64, J::Reply)>) {
    loop {
        let (id, job, cancelled) = {
            let mut queue = shared.lock();
            loop {
                if queue.stopping {
                    shared.alive.fetch_sub(1, Ordering::SeqCst);
                    return;
                }
                if let Some(queued) = queue.heap.pop() {
                    let id = queued.key.0.1;
                    queue.queued_ids.remove(&id);
                    let cancelled = queue.cancelled.remove(&id);
                    break (id, queued.job, cancelled);
                }
                queue = shared
                    .ready
                    .wait(queue)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        };
        let reply = if cancelled {
            job.cancelled()
        } else {
            let mut job = job;
            let output = catch_unwind(AssertUnwindSafe(|| job.run())).map_err(panic_message);
            job.finish(output)
        };
        // 接收端没了说明池子正在 drop，回执没人要了。
        let _ = replies.send((id, reply));
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        format!("compute job panicked: {message}")
    } else if let Some(message) = payload.downcast_ref::<String>() {
        format!("compute job panicked: {message}")
    } else {
        "compute job panicked".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Barrier},
        time::{Duration, Instant},
    };

    use super::*;

    /// 一个把数字平方的任务；`gate` 非空时先等在栅栏上，用来卡住线程。
    struct Square {
        value: u64,
        gate: Option<Arc<Barrier>>,
        boom: bool,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Reply {
        Done(u64),
        Failed(u64, String),
        Cancelled(u64),
    }

    impl PoolJob for Square {
        type Output = u64;
        type Reply = Reply;

        fn run(&mut self) -> u64 {
            if let Some(gate) = &self.gate {
                gate.wait();
            }
            assert!(!self.boom, "boom");
            self.value * self.value
        }

        fn finish(self, output: Result<u64, String>) -> Reply {
            match output {
                Ok(value) => Reply::Done(value),
                Err(message) => Reply::Failed(self.value, message),
            }
        }

        fn cancelled(self) -> Reply {
            Reply::Cancelled(self.value)
        }
    }

    fn square(value: u64) -> Square {
        Square {
            value,
            gate: None,
            boom: false,
        }
    }

    fn collect(pool: &mut ComputePool<Square>) -> Vec<(u64, Reply)> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut out = Vec::new();
        while pool.in_flight() > 0 && Instant::now() < deadline {
            if let Some(reply) = pool.recv_timeout(Duration::from_millis(50)) {
                out.push(reply);
            }
        }
        out.sort_by_key(|(id, _)| *id);
        out
    }

    #[test]
    fn every_job_replies_exactly_once() {
        let mut pool = ComputePool::new("test-pool", 4).unwrap();
        for id in 0..100 {
            pool.submit(id, id % 7, square(id));
        }
        let replies = collect(&mut pool);
        assert_eq!(replies.len(), 100);
        for (id, reply) in replies {
            assert_eq!(reply, Reply::Done(id * id));
        }
        assert_eq!(pool.in_flight(), 0);
        assert!(!pool.worker_died());
    }

    #[test]
    fn a_panicking_job_becomes_a_failure_and_the_worker_survives() {
        let mut pool = ComputePool::new("test-pool", 1).unwrap();
        pool.submit(
            1,
            0,
            Square {
                value: 3,
                gate: None,
                boom: true,
            },
        );
        pool.submit(2, 0, square(4));
        let replies = collect(&mut pool);
        assert!(matches!(&replies[0], (1, Reply::Failed(3, message)) if message.contains("boom")));
        assert_eq!(replies[1], (2, Reply::Done(16)));
        assert!(!pool.worker_died(), "任务 panic 不带走线程");
    }

    #[test]
    fn queued_jobs_run_by_priority_and_can_be_cancelled() {
        let mut pool = ComputePool::new("test-pool", 1).unwrap();
        // 先用一个卡在栅栏上的任务占住唯一的线程，后面的都留在队列里。
        let gate = Arc::new(Barrier::new(2));
        pool.submit(
            0,
            0,
            Square {
                value: 0,
                gate: Some(Arc::clone(&gate)),
                boom: false,
            },
        );
        // 等它真的开始跑。
        while pool.queued() > 0 {
            std::thread::yield_now();
        }
        pool.submit(1, 30, square(1));
        pool.submit(2, 10, square(2));
        pool.submit(3, 20, square(3));
        pool.cancel(3);
        pool.cancel(99);
        gate.wait();
        let mut order = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while pool.in_flight() > 0 && Instant::now() < deadline {
            if let Some((id, reply)) = pool.recv_timeout(Duration::from_millis(50)) {
                order.push((id, reply));
            }
        }
        assert_eq!(
            order,
            vec![
                (0, Reply::Done(0)),
                (2, Reply::Done(4)),
                (3, Reply::Cancelled(3)),
                (1, Reply::Done(1)),
            ],
            "单线程时按优先级出队，取消的也回一次执"
        );
    }

    #[test]
    fn dropping_the_pool_joins_its_threads() {
        let mut pool = ComputePool::new("test-pool", 3).unwrap();
        for id in 0..10 {
            pool.submit(id, 0, square(id));
        }
        drop(pool);
    }
}
