//! 并行驱动：所有实体对着阶段开始时的快照算，命令进缓冲，之后按实体顺序落地。
//!
//! 见 `docs/world-model-design.md` 8.2 和第 13 节。结果和线程数无关：算的时候只读
//! 不变的公开快照和自己那一行，落地的顺序固定为（uid 升序，实体内序号）。
//!
//! 和精确模式的差异只有「看到的别的实体是这一阶段开始时的样子」这一类，见
//! 设计稿第 13 节第 5 条。
//!
//! 接口分两段：[`compute`] 要只读方块视图和实体表，不需要完整的 `World`；
//! [`commit`] 把算好的命令按序落地，需要完整的 `World`。游戏层在两段之间
//! 可以重新构造 `World`（区块表被 `RegionLevel` 独占借着，没法同时借出只读视图）。

mod riding;

use std::time::{Duration, Instant};

use mc_bamboo_core::ChunkPos;

use crate::{
    entity::{
        Entity, EntityPublic, EntityRow, EntityState, EntityStore, EntityUid,
        ctx::{self, BlockView, Command, CommandBuffer, EntityCtx, ThrottlePlan},
        mob::throttle,
        store::{KindState, section_range},
    },
    shape::Aabb,
    world::World,
};

/// 一批不相交的任务。执行器跑完这一批才返回；任务之间没有先后要求。
///
/// 任务闭包带 `'a` 生命周期，调用方可以把 `&'a mut [..]` 的不相交切片直接搬进去，
/// 不需要锁、不需要 `unsafe`。实现必须是对象安全的，游戏层以后可以换成常驻线程池。
pub trait Executor: Sync {
    /// 希望分成几份。驱动自己按这个数切任务；分几份不影响结果。
    fn parallelism(&self) -> usize;
    /// 把这批任务跑完才返回。任务之间没有先后要求。
    fn run<'a>(&self, jobs: Vec<Box<dyn FnOnce() + Send + 'a>>);
}

/// 一个线程也不开，按数组顺序跑。
pub struct InlineExecutor;

impl Executor for InlineExecutor {
    fn parallelism(&self) -> usize {
        1
    }

    fn run<'a>(&self, jobs: Vec<Box<dyn FnOnce() + Send + 'a>>) {
        for job in jobs {
            job();
        }
    }
}

/// `std::thread::scope` 每刻起 `threads - 1` 个线程，主线程也跑一份。
///
/// 正式服务端已经换成长驻池；这里只留给压测做「每刻起线程」的 A/B 对照。
pub struct ScopedThreads {
    pub threads: usize,
}

impl Executor for ScopedThreads {
    fn parallelism(&self) -> usize {
        self.threads.max(1)
    }

    fn run<'a>(&self, jobs: Vec<Box<dyn FnOnce() + Send + 'a>>) {
        let count = jobs.len();
        if count <= 1 {
            for job in jobs {
                job();
            }
            return;
        }
        let mut jobs = jobs;
        std::thread::scope(|scope| {
            // 最后一份留给当前线程，少起一个线程。
            for job in jobs.drain(..count - 1) {
                scope.spawn(job);
            }
            for job in jobs {
                job();
            }
        });
    }
}

/// 服务端启动时建好、按刻派活的常驻线程池。主线程也跑最后一份，所以
/// `threads - 1` 条后台线程加当前线程就是 `threads` 份活。
///
/// 每个后台线程有自己的信箱：派活时把「这一刻属于它的那一份」直接放进去，
/// 跑完置一个原子完成标记。不共用任务队列，小场景里没有逐任务的锁竞争。
/// 闭包借用算阶段的只读快照，派活线程在 `run` 返回前等所有线程跑完，
/// 借用不会越过这一刻。结果和线程数无关，执行器只决定谁跑哪一份。
pub struct WorkerPool {
    workers: Vec<Worker>,
    /// 并行份数：后台线程各一份，当前线程最后一份。
    threads: usize,
    /// 同一时刻只能有一批任务在跑。正式服务端是一刻一刻串行派活的；
    /// 分离驱动下多个岛万一同时进来，锁保证它们排队而不是互相覆盖。
    dispatch: std::sync::Mutex<()>,
    generation: std::sync::atomic::AtomicU64,
}

/// 一条后台线程和它独占的信箱。
struct Worker {
    slot: std::sync::Arc<WorkerSlot>,
    handle: Option<std::thread::JoinHandle<()>>,
}

/// 一份活。`job` 被取走跑时信箱里仍留着这一项，派活线程据此等完成标记。
struct WorkerJob {
    generation: u64,
    job: Option<Box<dyn FnOnce() + Send + 'static>>,
    panic: Option<Box<dyn std::any::Any + Send + 'static>>,
}

struct WorkerSlot {
    state: std::sync::Mutex<Option<WorkerJob>>,
    ready: std::sync::Condvar,
    /// 这一份活跑完了（结果留在 `state` 里等派活线程取走）。
    done: std::sync::atomic::AtomicBool,
    stop: std::sync::atomic::AtomicBool,
}

impl WorkerSlot {
    fn stopped(&self) -> bool {
        self.stop.load(std::sync::atomic::Ordering::Acquire)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<WorkerJob>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl WorkerPool {
    /// 起 `threads.max(1) - 1` 条常驻线程；起不来时返回错误，调用方退回内联执行器。
    pub fn new(name: &str, threads: usize) -> std::io::Result<Self> {
        let threads = threads.max(1);
        let mut workers = Vec::with_capacity(threads - 1);
        for index in 0..threads - 1 {
            let slot = std::sync::Arc::new(WorkerSlot {
                state: std::sync::Mutex::new(None),
                ready: std::sync::Condvar::new(),
                done: std::sync::atomic::AtomicBool::new(false),
                stop: std::sync::atomic::AtomicBool::new(false),
            });
            let worker_slot = std::sync::Arc::clone(&slot);
            let builder = std::thread::Builder::new().name(format!("{name}-{index}"));
            match builder.spawn(move || worker_loop(&worker_slot)) {
                Ok(handle) => workers.push(Worker {
                    slot,
                    handle: Some(handle),
                }),
                Err(error) => {
                    // 已经起来的线程也要收干净，不能留下空转的常驻线程。
                    for worker in &mut workers {
                        worker.stop_and_join();
                    }
                    return Err(error);
                }
            }
        }
        Ok(Self {
            workers,
            threads,
            dispatch: std::sync::Mutex::new(()),
            generation: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// 常驻线程数（不含当前线程）。
    #[cfg(test)]
    pub(crate) fn worker_count(&self) -> usize {
        self.workers.len()
    }
}

impl Executor for WorkerPool {
    fn parallelism(&self) -> usize {
        self.threads
    }

    fn run<'a>(&self, jobs: Vec<Box<dyn FnOnce() + Send + 'a>>) {
        let _dispatch = self
            .dispatch
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = jobs.len();
        if count <= 1 || self.workers.is_empty() {
            for job in jobs {
                job();
            }
            return;
        }
        // `unsafe` 只用来把闭包的借用期擦成 `'static`：每份活只在这一次
        // `run` 里被对应的线程跑一次，派活线程等所有线程跑完并把信箱清空后才返回；
        // 后台线程从不把闭包存到信箱之外。
        let erase = |job: Box<dyn FnOnce() + Send + 'a>| -> Box<dyn FnOnce() + Send + 'static> {
            unsafe { std::mem::transmute(job) }
        };
        // 派活是串行的，批次号放在池子上递增；信箱里的上一批在 `run` 返回前已取空。
        let generation = self
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        // 一份闭包对应一整块连续的行，不能拆开；最后一份留给当前线程，
        // 其余平均分成 `workers.len()` 组，一条线程一组。组数少于线程数时
        // 有的线程这一刻空着。
        let mut jobs = jobs;
        let mine = jobs.pop().expect("至少两份");
        let groups = self.workers.len().min(jobs.len());
        let per_group = jobs.len().div_ceil(groups);
        let mut pieces: Vec<Vec<Box<dyn FnOnce() + Send + 'static>>> =
            (0..groups).map(|_| Vec::new()).collect();
        for (index, job) in jobs.into_iter().enumerate() {
            pieces[(index / per_group).min(groups - 1)].push(erase(job));
        }
        for (worker, piece) in self.workers.iter().zip(pieces) {
            let mut guard = worker.slot.lock();
            debug_assert!(guard.is_none(), "上一批已经取走");
            worker
                .slot
                .done
                .store(false, std::sync::atomic::Ordering::Release);
            let job: Box<dyn FnOnce() + Send + 'static> = Box::new(move || {
                for job in piece {
                    job();
                }
            });
            *guard = Some(WorkerJob {
                generation,
                job: Some(job),
                panic: None,
            });
            worker.slot.ready.notify_all();
            drop(guard);
        }
        // 当前线程也跑最后一份。
        mine();
        // 等所有派了活的后台线程跑完，把它们信箱里的结果取走。
        let mut panic = None;
        for worker in &self.workers[..groups] {
            let mut guard = worker.slot.lock();
            while !worker.slot.done.load(std::sync::atomic::Ordering::Acquire) {
                guard = worker
                    .slot
                    .ready
                    .wait(guard)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            if let Some(job) = guard.take()
                && job.generation == generation
                && panic.is_none()
            {
                panic = job.panic;
            }
            drop(guard);
        }
        // 某一刻的实体逻辑 panic 了：不在池子里吞掉，交回派活线程。
        if let Some(payload) = panic {
            std::panic::resume_unwind(payload);
        }
    }
}

/// 后台线程：等自己信箱里出现「还没跑过的活」，取走闭包（信箱里留着这一项，
/// 派活线程据此等待），跑完置 `done`，然后等下一刻。
fn worker_loop(slot: &WorkerSlot) {
    loop {
        let body = {
            let mut guard = slot.lock();
            loop {
                if slot.stopped() {
                    return;
                }
                let pending = guard
                    .as_mut()
                    .filter(|job| job.job.is_some())
                    .and_then(|job| job.job.take());
                if let Some(body) = pending {
                    break body;
                }
                guard = slot
                    .ready
                    .wait(guard)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        };
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).err();
        let mut guard = slot.lock();
        if let Some(job) = guard.as_mut() {
            job.panic = caught;
        }
        slot.done.store(true, std::sync::atomic::Ordering::Release);
        slot.ready.notify_all();
    }
}

impl Worker {
    /// 让后台线程退出并等它结束。停止标志必须在持有信箱锁时设置、唤醒：
    /// 后台线程是拿着锁检查 `stopped()` 再原子地放锁等待的，不拿锁设置的话，
    /// 唤醒可能落在「检查完、还没开始等」的空当里丢掉，`join` 就永远等不到。
    ///
    /// Stop the worker thread and wait for it. The stop flag must be set and signalled
    /// while holding the slot lock: the worker checks `stopped()` under the lock and
    /// then waits, releasing it atomically, so an unlocked notify can fall between the
    /// check and the wait, get lost, and leave `join` waiting forever.
    fn stop_and_join(&mut self) {
        {
            let _guard = self.slot.lock();
            self.slot
                .stop
                .store(true, std::sync::atomic::Ordering::Release);
            self.slot.ready.notify_all();
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        for worker in &mut self.workers {
            worker.stop_and_join();
        }
    }
}

/// 一个实体的算阶段产出：新的公开和私有数据、命令、是否在 tick 里被丢弃。
#[derive(Debug, Default)]
struct RowResult {
    public: Option<EntityPublic>,
    state: Option<EntityState>,
    commands: CommandBuffer,
    removed: bool,
}

/// 算阶段的结果。`commit` 消费它；算阶段的只读借用不放在里面，所以两段之间
/// 调用方可以重新构造完整的 `World`。
#[derive(Debug)]
pub struct Computed {
    uids: Vec<EntityUid>,
    rows: Vec<usize>,
    results: Vec<RowResult>,
    times: PhaseTimes,
}

impl Computed {
    /// 这一刻参与算的实体 ID，按 uid 升序。给测试断言用。
    pub fn uids(&self) -> &[EntityUid] {
        &self.uids
    }

    /// 每个实体的命令条数，按 uid 升序。
    pub fn command_counts(&self) -> Vec<usize> {
        self.results
            .iter()
            .map(|result| result.commands.commands.len())
            .collect()
    }

    /// 这一刻被自己的 tick 丢弃的实体数。
    pub fn removed_count(&self) -> usize {
        self.results.iter().filter(|result| result.removed).count()
    }
}

/// 只读公开快照：算阶段共享实体表本身的 `public` 和空间索引，不复制一份。
struct Shared<'a> {
    uids: &'a [EntityUid],
    rows: &'a [usize],
    entities: &'a EntityStore,
    blocks: &'a (dyn BlockView + Sync),
}

impl Shared<'_> {
    /// uid 是升序的，二分查找行号；查到后直接引用表里的那一行。
    fn public_of(&self, uid: EntityUid) -> Option<&EntityPublic> {
        let index = self.uids.binary_search(&uid).ok()?;
        Some(&self.entities.row_at(self.rows[index])?.public)
    }

    /// 只访问范围覆盖到的那几个格子，不再遍历整张索引。`exclude` 是当前正在算的实体：
    /// 官方 `getEntities(this, ...)` 不返回自己，快照里还留着自己那一行是并行模型的
    /// 实现细节，不是可以让别的实体看到的东西。
    fn for_each_in(
        &self,
        area: Aabb,
        exclude: EntityUid,
        limit: usize,
        visit: &mut dyn FnMut(&EntityPublic) -> bool,
    ) {
        let (first, last) = section_range(&area);
        let mut found = Vec::new();
        for x in first.0..=last.0 {
            for y in first.1..=last.1 {
                for z in first.2..=last.2 {
                    if let Some(rows) = self.entities.grid_section((x, y, z)) {
                        found.extend(rows.iter().copied());
                    }
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        // 行号不等于 uid 顺序（槽位会被回收复用），按 uid 排序再访问。
        let mut ordered: Vec<_> = found
            .into_iter()
            .filter_map(|row| self.entities.row_at(row))
            .filter(|entry| {
                entry.public.uid != exclude && entry.public.bounding_box().intersects(area)
            })
            .map(|entry| (entry.public.uid, &entry.public))
            .collect();
        ordered.sort_unstable_by_key(|(uid, _)| *uid);
        let mut taken = 0;
        for (_, public) in ordered {
            if taken >= limit {
                break;
            }
            if visit(public) {
                taken += 1;
            }
        }
    }
}

/// 并行阶段的一个任务：只读公开快照、只写自己的公开和私有行、命令进自己的缓冲。
struct ParallelCtx<'a> {
    uid: EntityUid,
    shared: &'a Shared<'a>,
    commands: &'a mut CommandBuffer,
    plan: ThrottlePlan,
}

impl EntityCtx for ParallelCtx<'_> {
    fn uid(&self) -> EntityUid {
        self.uid
    }

    fn throttle_plan(&self) -> ThrottlePlan {
        self.plan
    }

    fn blocks(&self) -> &dyn BlockView {
        self.shared.blocks
    }

    fn other(&self, uid: EntityUid) -> Option<EntityPublic> {
        // 精确驱动把当前实体从表里拿走了，`other(自己)` 本来就是 `None`。
        (uid != self.uid)
            .then(|| self.shared.public_of(uid).cloned())
            .flatten()
    }

    fn entity_uids(&self) -> Vec<EntityUid> {
        self.shared
            .uids
            .iter()
            .copied()
            .filter(|&uid| uid != self.uid)
            .collect()
    }

    fn for_each_other_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic)) {
        self.shared
            .for_each_in(area, self.uid, usize::MAX, &mut |public| {
                visit(public);
                true
            });
    }

    fn pushable_others(
        &self,
        area: Aabb,
        limit: usize,
        visit: &mut dyn FnMut(&EntityPublic) -> bool,
    ) {
        self.shared.for_each_in(area, self.uid, limit, visit);
    }

    fn for_each_other_uid_in(&self, area: Aabb, visit: &mut dyn FnMut(EntityUid)) {
        self.shared
            .for_each_in(area, self.uid, usize::MAX, &mut |public| {
                visit(public.uid);
                true
            });
    }

    fn emit(&mut self, command: Command, _me: &mut Entity) {
        self.commands.push(command);
    }
}

/// 算：只读方块、可写实体表。不需要 `World`。
///
/// 公开数据和空间索引只读表里的那一份，各行的私有状态移进任务再移回，不克隆
/// （`ItemStack` 在 `KindPublic` 里，`public` 仍然必须克隆一次给 tick 拼完整实体）。
pub fn compute(
    blocks: &(dyn BlockView + Sync),
    entities: &mut EntityStore,
    executor: &dyn Executor,
) -> Computed {
    compute_timed(blocks, entities, executor).0
}

fn compute_timed(
    blocks: &(dyn BlockView + Sync),
    entities: &mut EntityStore,
    executor: &dyn Executor,
) -> (Computed, Duration) {
    let mut times = PhaseTimes::default();
    // 一刻开头清空寻路去重集合（测试构建的压测探针；正式构建是空操作）。
    #[cfg(test)]
    crate::entity::mob::timing::new_tick!();
    let clone_start = Instant::now();
    // 降频开着时按 uid 升序算好这一刻的计划；和精确驱动共用 `plans_for_tick`。
    // 算阶段只能读快照，所以这一步在 `take_states_for_tick` 之前改实体表。
    let config = throttle::Config::of(&blocks.block_game_rules());
    let plans = if config.enabled() {
        let mut players = Vec::new();
        blocks.for_each_player(&mut |player| players.push(*player));
        throttle::plans_for_tick(entities, blocks.game_time(), config, &players)
    } else {
        Vec::new()
    };
    // 独立寻路阶段：上一刻算好的结果先挂回实体表里的导航，算阶段 tick 时统一处理。
    entities.begin_path_tick();
    let (uids, rows, starting_state) = entities.take_states_for_tick();
    // 每个实体一份起点的公开数据：tick 要拿它拼成完整 `Entity`。这是唯一必需的
    // 克隆；别的实体看到的值直接引用表里那一份不变的快照。
    let starting_public: Vec<EntityPublic> = rows
        .iter()
        .map(|&row| {
            entities
                .row_at(row)
                .expect("indexed row is live")
                .public
                .clone()
        })
        .collect();
    times.clone = clone_start.elapsed();

    let shared = Shared {
        uids: &uids,
        rows: &rows,
        entities,
        blocks,
    };
    let count = uids.len();
    let compute_start = Instant::now();

    // 一棵骑乘树就是一份活：根和它的乘客在同一线程里按 `tickNonPassenger` 的顺序算。
    // 没有骑乘关系时每棵树只有一个节点，切法和以前按 uid 连续切片等价。每份活写自己
    // 的局部结果，跑完由主线程按 uid 升序并回总表，落地顺序和以前完全一样。
    let groups = riding::riding_groups(&uids, &starting_public);
    let parts = executor.parallelism().max(1).min(groups.len().max(1));
    let trees_per_task = groups.len().div_ceil(parts).max(1);
    let mut jobs: Vec<Box<dyn FnOnce() + Send + '_>> = Vec::with_capacity(parts);
    type JobSlot = std::sync::Arc<std::sync::Mutex<Vec<(usize, RowResult)>>>;
    let mut slots: Vec<JobSlot> = Vec::with_capacity(parts);
    for group_of_trees in groups.chunks(trees_per_task) {
        let mut inputs: Vec<Vec<TreeRow>> = Vec::with_capacity(group_of_trees.len());
        for tree in group_of_trees {
            inputs.push(build_tree_rows(
                tree,
                &uids,
                &starting_public,
                &starting_state,
                &plans,
            ));
        }
        let slot = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        slots.push(std::sync::Arc::clone(&slot));
        let shared_ref: &Shared<'_> = &shared;
        jobs.push(Box::new(move || {
            for tree in &mut inputs {
                run_tree(tree, shared_ref);
            }
            let mut out = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            for tree in inputs {
                for row in tree {
                    out.push((row.index, row.result));
                }
            }
            // 测试构建里各算阶段线程把分段计数并进全局汇总；正式构建是空操作。
            #[cfg(test)]
            crate::entity::mob::timing::imp::flush_counters();
        }));
    }
    executor.run(jobs);
    let mut results: Vec<RowResult> = (0..count).map(|_| RowResult::default()).collect();
    for slot in slots {
        let mut done =
            std::mem::take(&mut *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
        for (index, result) in done.drain(..) {
            results[index] = result;
        }
    }
    #[cfg(test)]
    crate::entity::mob::timing::imp::flush_counters();
    times.compute = compute_start.elapsed();

    run_pathfinding_phase(blocks, entities, &mut results, executor);

    (
        Computed {
            uids,
            rows,
            results,
            times,
        },
        clone_start.elapsed(),
    )
}

/// 一份活里的一行：`index` 是它在 uid 升序总名单里的位置，`vehicle_slot` 是它的
/// 载具在这棵树里的位置（根为 `None`），`passenger_index` 是它在载具乘客表里的序号。
///
/// One row of a job: `index` is its position in the uid-ascending list,
/// `vehicle_slot` the position of its vehicle in the same tree (`None` for the
/// root), and `passenger_index` its ordinal in the vehicle's passenger list.
struct TreeRow {
    index: usize,
    input: Option<(EntityPublic, EntityState)>,
    plan: ThrottlePlan,
    vehicle_slot: Option<usize>,
    passenger_index: usize,
    result: RowResult,
}

fn build_tree_rows(
    tree: &[usize],
    uids: &[EntityUid],
    public: &[EntityPublic],
    state: &[EntityState],
    plans: &[ThrottlePlan],
) -> Vec<TreeRow> {
    let mut rows = Vec::with_capacity(tree.len());
    for (within, &index) in tree.iter().enumerate() {
        let (vehicle_slot, passenger_index) = if within == 0 {
            (None, 0)
        } else {
            let vehicle = public[index].vehicle;
            let slot = tree
                .iter()
                .take(within)
                .position(|&candidate| Some(uids[candidate]) == vehicle);
            let ordinal = slot
                .and_then(|slot| {
                    let list = match &public[tree[slot]].kind {
                        crate::entity::KindPublic::Mob(mob) => &mob.passengers,
                        _ => return None,
                    };
                    list.iter().position(|&uid| uid == uids[index])
                })
                .unwrap_or(0);
            (slot, ordinal)
        };
        rows.push(TreeRow {
            index,
            input: Some((public[index].clone(), state[index].clone())),
            plan: plans.get(index).copied().unwrap_or(ThrottlePlan::FULL),
            vehicle_slot,
            passenger_index,
            result: RowResult::default(),
        });
    }
    rows
}

/// 独立寻路阶段（并行驱动）：从算阶段的实体状态里收请求，去重、排优先级、
/// 按节点预算挑出后分给线程，结果按固定顺序汇总。开关关着时什么都不做。
///
/// The pathfinding phase in the parallel driver: collect requests from the
/// computed states, deduplicate and rank them, split the selected searches across
/// workers, and merge the results in fixed order. A no-op while the switch is off.
fn run_pathfinding_phase(
    blocks: &(dyn BlockView + Sync),
    entities: &mut EntityStore,
    results: &mut [RowResult],
    executor: &dyn Executor,
) {
    use crate::entity::mob::pathfinding;
    let config = pathfinding::Config::of(&blocks.block_game_rules());
    if !config.enabled {
        return;
    }
    let now = blocks.game_time();
    let mut requests = Vec::new();
    for result in results.iter_mut() {
        let Some(state) = result.state.as_mut() else {
            continue;
        };
        let KindState::Mob(mob) = &mut state.kind else {
            continue;
        };
        let Some(request) = mob.navigation.pending_request.take() else {
            continue;
        };
        // 和精确驱动的 `take_path_requests` 一致：按导航 owner 记账。控制载具的骑手
        // 借载具导航，请求属于载具，key 用 `owner_uid`（载具）而不是本结果行的 uid。
        // Match the precise driver's `take_path_requests`: key by navigation owner.
        // A controlling rider borrows the vehicle's navigation, so the request
        // belongs to the vehicle and is keyed by `owner_uid`.
        let uid = request.nav.owner_uid;
        requests.push(pathfinding::Pending::new(
            uid,
            request,
            now,
            mob.target.is_some(),
            mob.target.unwrap_or(-1),
        ));
    }
    requests.sort_by_key(|pending| (pending.uid, pending.local_serial));
    let mut players = Vec::new();
    blocks.for_each_player(&mut |player| players.push(*player));
    let groups = entities
        .pathfinding_mut()
        .begin(now, config, &players, requests);
    let count = groups.len();
    let mut found: Vec<Option<(Option<crate::entity::mob::path::Path>, u64)>> =
        (0..count).map(|_| None).collect();
    if count > 0 {
        let parts = executor.parallelism().max(1).min(count);
        let chunk = count.div_ceil(parts);
        let mut left: &mut [Option<(Option<crate::entity::mob::path::Path>, u64)>] = &mut found;
        let mut start = 0;
        let mut jobs: Vec<Box<dyn FnOnce() + Send + '_>> = Vec::new();
        while start < count {
            let size = chunk.min(count - start);
            let (head, rest) = std::mem::take(&mut left).split_at_mut(size);
            left = rest;
            let selected: Vec<_> = groups[start..start + size]
                .iter()
                .map(|group| group[0].request)
                .collect();
            jobs.push(Box::new(move || {
                for (slot, request) in head.iter_mut().zip(selected) {
                    *slot = Some(crate::entity::mob::path::find_path_with_nodes(
                        blocks, &request,
                    ));
                }
            }));
            start += size;
        }
        executor.run(jobs);
    }
    let results: Vec<_> = found
        .into_iter()
        .map(|result| result.unwrap_or((None, 0)))
        .collect();
    entities
        .pathfinding_mut()
        .finish(now, config, groups, results);
}

/// 一棵骑乘树：`tree[0]` 是根，按 `ServerLevel.tickNonPassenger` 先 tick 根，再按
/// 乘客表顺序对实体乘客跑 `commonTick + rideTick + positionRider`。乘客看到的是
/// 本刻开头其它实体的只读快照，但定位用的是刚 tick 完的载具这一刻末的位置。
///
/// One riding tree: the root ticks first, then entity passengers run
/// `commonTick + rideTick + positionRider` in boarding order. Passengers read
/// the tick-start snapshot for other entities but are positioned against their
/// just-ticked vehicle.
fn run_tree(tree: &mut [TreeRow], shared: &Shared<'_>) {
    for within in 0..tree.len() {
        let Some((public, state)) = tree[within].input.take() else {
            continue;
        };

        let mut me = EntityRow { public, state }.into_entity();
        let is_root = within == 0;
        let vehicle_slot = tree[within].vehicle_slot;
        if !is_root {
            // `Entity.rideTick` 先把速度清零，再跑自己的 tick。
            me.delta = mc_bamboo_core::Vec3::default();
        }
        // `Mob.getNavigation` / `getMoveControl` 在控制载具时转给载具：整棵树在同一份
        // 活里，直接从已 tick 完的载具结果取它的导航/移动控制与按载具尺寸的快照。
        // Vanilla `Mob.getNavigation` / `getMoveControl` delegate to the vehicle;
        // the whole riding tree is one job, so take the vehicle's post-tick result.
        let borrowing = if !is_root && let Some(slot) = vehicle_slot {
            let vehicle = tree.get(slot).and_then(|row| {
                Some(
                    EntityRow {
                        public: row.result.public.as_ref()?.clone(),
                        state: row.result.state.as_ref()?.clone(),
                    }
                    .into_entity(),
                )
            });
            vehicle.is_some_and(|vehicle| {
                crate::entity::riding::begin_nav_borrow(&mut me, &vehicle, shared.blocks)
            })
        } else {
            false
        };
        {
            let result = &mut tree[within].result;
            let mut ctx = ParallelCtx {
                uid: shared.uids[tree[within].index],
                shared,
                commands: &mut result.commands,
                plan: tree[within].plan,
            };
            // 所在区块不在实体 tick 范围时跳过这一刻：实体仍留在快照里，位置和年龄
            // 不动，不发命令，别的实体照样查得到它。两种驱动共用这个判断。
            if shared
                .blocks
                .chunk_ticks_entities(ChunkPos::containing(me.position.x, me.position.z))
            {
                me.tick(&mut ctx);
            }
        }
        // 把改过的共享导航/移动控制写回载具结果，恢复骑手自己的两份。
        // Write the mutated shared state back to the vehicle's result and restore
        // the rider's own navigation / move control.
        if borrowing
            && let Some((navigation, mover)) = crate::entity::riding::end_nav_borrow(&mut me)
            && let Some(slot) = vehicle_slot
            && let Some(state) = tree[slot].result.state.as_mut()
        {
            crate::entity::riding::put_borrowed_nav_state(state, navigation, mover);
        }
        // `RangedBowAttackGoal.tick()` 的 `vehicle.lookAt`：把这一刻转向目标的请求
        // 落到载具行。精确驱动在借导航写回处做同样的事。
        let vehicle_look = crate::entity::riding::take_vehicle_look(&mut me);
        if let (Some(target_eye), Some(slot)) = (vehicle_look, vehicle_slot) {
            let rebuilt = tree.get(slot).and_then(|row| {
                Some(
                    EntityRow {
                        public: row.result.public.as_ref()?.clone(),
                        state: row.result.state.as_ref()?.clone(),
                    }
                    .into_entity(),
                )
            });
            if let Some(mut vehicle) = rebuilt {
                crate::entity::riding::apply_vehicle_look(&mut vehicle, target_eye);
                let split = vehicle.split();
                tree[slot].result.public = Some(split.public);
                tree[slot].result.state = Some(split.state);
            }
        }
        if !is_root {
            // `LivingEntity.rideTick` 的复位落距，以及 `positionRider`。
            me.fall_distance = 0.0;
            if let Some(vehicle_slot) = vehicle_slot {
                let Some(vehicle_public) = tree[vehicle_slot].result.public.as_ref() else {
                    continue;
                };
                let passenger_public = me.clone().split().public;
                let at = crate::entity::riding::rider_position(
                    vehicle_public,
                    &passenger_public,
                    tree[within].passenger_index,
                );
                me.position = at;
            } else {
                // 载具不在本刻的名单里：按官方 `baseTick` 的分支当场下车。
                crate::entity::riding::dismount(&mut me, None);
            }
            // 官方 `checkDespawn` 对乘客排在 `rideTick` 之后；`tick_mob` 对乘客跳过，
            // 由这里补上。
            // Vanilla runs a passenger's `checkDespawn` after its `rideTick`; the
            // tick path skips it for passengers, so it happens here.
            if !me.removed {
                let result = &mut tree[within].result;
                let mut ctx = ParallelCtx {
                    uid: shared.uids[tree[within].index],
                    shared,
                    commands: &mut result.commands,
                    plan: tree[within].plan,
                };
                crate::entity::mob::living::check_despawn_entity(&mut me, &mut ctx);
            }
        }
        // `AbstractSkeleton.rideTick` / `Drowned.rideTick` 结尾：控制载具的骷髅/溺尸
        // 把载具的 `yBodyRot` 抄成自己的，让蜘蛛骑士朝向一致（精确驱动同样处理）。
        if !is_root && let Some(slot) = vehicle_slot {
            let vehicle = tree.get(slot).and_then(|row| {
                Some(
                    EntityRow {
                        public: row.result.public.as_ref()?.clone(),
                        state: row.result.state.as_ref()?.clone(),
                    }
                    .into_entity(),
                )
            });
            if let Some(vehicle) = vehicle {
                crate::entity::riding::sync_rider_body_rotation(&mut me, &vehicle);
            }
        }
        let split = me.split();
        tree[within].result.removed = split.public.removed;
        tree[within].result.public = Some(split.public);
        tree[within].result.state = Some(split.state);
    }
}

/// 落地：发布新的公开和私有数据，按（uid 升序，实体内序号）串行应用命令。
///
/// 需要一个完整的 `World`：命令处理函数会写方块、生成实体、发效果。
pub fn commit(computed: Computed, world: &mut World<'_>) {
    commit_timed(computed, world);
}

fn commit_timed(computed: Computed, world: &mut World<'_>) -> PhaseTimes {
    let Computed {
        uids,
        rows,
        mut results,
        mut times,
    } = computed;
    let publish_start = Instant::now();
    // 先发布，命令处理函数再改表里的实体；发布只更新位置变了的那几个格子。
    let (public, state): (Vec<_>, Vec<_>) = results
        .iter_mut()
        .map(|result| {
            (
                result.public.take().expect("compute wrote every row"),
                result.state.take().expect("compute wrote every row"),
            )
        })
        .unzip();
    // 算阶段就 `discard` 的实体不发布进表里，和精确模式「tick 过就不 put 回去」一致。
    // 但它这一刻发出的命令仍要落地（闪电最后一刻的引火 / `thunderHit`、命中的鸡蛋等）：
    // 精确模式 `emit` 当场就做了，并行模式若跳过会少一次写世界。保存一份算阶段
    // 结束时的快照，落地时按同一顺序跑命令，跑完不再放回表里。
    let mut publish_rows = Vec::with_capacity(rows.len());
    let mut publish_public = Vec::with_capacity(public.len());
    let mut publish_state = Vec::with_capacity(state.len());
    let mut removed_entities: Vec<Option<Entity>> = (0..rows.len()).map(|_| None).collect();
    for (index, ((row, public), state)) in rows.iter().copied().zip(public).zip(state).enumerate() {
        if results[index].removed {
            removed_entities[index] = Some(EntityRow { public, state }.into_entity());
            continue;
        }
        publish_rows.push(row);
        publish_public.push(public);
        publish_state.push(state);
    }
    world
        .entities_mut()
        .publish_entity_rows(&publish_rows, publish_public, publish_state);
    // 上面是「发布」，还差算阶段自己 `discard` 的那几行：它们仍在表里（精确模式
    // 里从来没 put 回去），按标记逐个拿掉，不用对所有 uid 查表。
    for (index, result) in results.iter().enumerate() {
        if result.removed {
            world.entities_mut().remove_entity(uids[index]);
        }
    }
    times.publish = publish_start.elapsed();

    let apply_start = Instant::now();
    for (index, result) in results.iter_mut().enumerate() {
        if result.commands.commands.is_empty() {
            continue;
        }
        let uid = uids[index];
        let mut me = if result.removed {
            let Some(me) = removed_entities[index].take() else {
                continue;
            };
            me
        } else {
            // 前面的命令可能已经把这个实体拿掉；`take_entity` 返回 None 就整批跳过。
            let Some(me) = world.entities_mut().take_entity(uid) else {
                continue;
            };
            me
        };
        for command in result.commands.commands.drain(..) {
            ctx::apply(command, &mut me, world);
        }
        if me.removed {
            // 落地时才消失的实体（命中的鸡蛋）要把这一刻的实体事件留给同步层。
            // An entity removed while applying (an egg that hit) keeps its events for sync.
            world.entities_mut().keep_departed_mob_events(&me);
        } else if !result.removed {
            world.entities_mut().put_entity(me);
        }
    }
    times.apply = apply_start.elapsed();
    world.entities_mut().end_entity_tick();
    times
}

/// 把「算」和「落地」的耗时分开报，给压力基准用。
#[derive(Debug, Clone, Copy, Default)]
pub struct PhaseTimes {
    pub clone: Duration,
    pub compute: Duration,
    pub publish: Duration,
    pub apply: Duration,
}

/// 「先 compute 再 commit」的便捷包装，给测试和基准用。
///
/// 调用方同时拿得出只读方块视图和 `&mut World` 时才用得上；游戏层要走两段式，
/// 因为 `RegionLevel` 独占借着区块表，没法同时再借一个只读视图。
pub fn tick_entities_parallel(
    blocks: &(dyn BlockView + Sync),
    world: &mut World<'_>,
    executor: &dyn Executor,
) {
    let _ = tick_entities_parallel_timed(blocks, world, executor);
}

/// 同 [`tick_entities_parallel`]，另外报出「复制」「算」「发布」「落地」各段的耗时。
pub fn tick_entities_parallel_timed(
    blocks: &(dyn BlockView + Sync),
    world: &mut World<'_>,
    executor: &dyn Executor,
) -> PhaseTimes {
    let snapshot_start = Instant::now();
    world.begin_entity_tick();
    // 一体包装里同时有 `&mut World`，可以直接把实体表可变借给算阶段。
    let (computed, _) = compute_timed(blocks, world.entities_mut().entities_table_mut(), executor);
    let mut times = commit_timed(computed, world);
    // `begin_entity_tick` 和构造只读名单的开销算进发布这一段。
    times.publish += snapshot_start
        .elapsed()
        .saturating_sub(times.publish + times.clone + times.compute + times.apply);
    times
}

#[cfg(test)]
pub(crate) fn shared_rows_in(
    uids: &[EntityUid],
    rows: &[usize],
    entities: &EntityStore,
    blocks: &(dyn BlockView + Sync),
    area: Aabb,
) -> Vec<Vec<EntityUid>> {
    let shared = Shared {
        uids,
        rows,
        entities,
        blocks,
    };
    uids.iter()
        .map(|&uid| {
            let mut seen = Vec::new();
            shared.for_each_in(area, uid, usize::MAX, &mut |public| {
                seen.push(public.uid);
                true
            });
            seen
        })
        .collect()
}

#[cfg(test)]
mod tests;
