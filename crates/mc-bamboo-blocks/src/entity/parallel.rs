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

use std::time::{Duration, Instant};

use mc_bamboo_core::ChunkPos;

use crate::{
    entity::{
        Entity, EntityPublic, EntityRow, EntityState, EntityStore, EntityUid,
        ctx::{self, BlockView, Command, CommandBuffer, EntityCtx},
        store::section_range,
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
/// 每刻起线程的开销大，只用于实验；常驻线程池留给任务系统（设计稿第 12 节）。
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

    /// 只访问范围覆盖到的那几个格子，不再遍历整张索引。
    fn for_each_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic)) {
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
            .filter(|entry| entry.public.bounding_box().intersects(area))
            .map(|entry| (entry.public.uid, &entry.public))
            .collect();
        ordered.sort_unstable_by_key(|(uid, _)| *uid);
        for (_, public) in ordered {
            visit(public);
        }
    }
}

/// 并行阶段的一个任务：只读公开快照、只写自己的公开和私有行、命令进自己的缓冲。
struct ParallelCtx<'a> {
    uid: EntityUid,
    shared: &'a Shared<'a>,
    commands: &'a mut CommandBuffer,
}

impl EntityCtx for ParallelCtx<'_> {
    fn uid(&self) -> EntityUid {
        self.uid
    }

    fn blocks(&self) -> &dyn BlockView {
        self.shared.blocks
    }

    fn other(&self, uid: EntityUid) -> Option<EntityPublic> {
        self.shared.public_of(uid).cloned()
    }

    fn entity_uids(&self) -> Vec<EntityUid> {
        self.shared.uids.to_vec()
    }

    fn for_each_other_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic)) {
        self.shared.for_each_in(area, visit);
    }

    fn for_each_other_uid_in(&self, area: Aabb, visit: &mut dyn FnMut(EntityUid)) {
        self.shared
            .for_each_in(area, &mut |public| visit(public.uid));
    }

    fn emit(&mut self, command: Command, _me: &mut Entity) {
        self.commands.push(command);
    }
}

/// 一块连续的行：算阶段只写自己的这几行，别的都是只读共享。
struct Task<'a> {
    first: usize,
    /// 只属于本任务的起点公开数据，逐行移进 `Entity`，不再克隆第二遍。
    public: Vec<EntityPublic>,
    state: Vec<EntityState>,
    results: &'a mut [RowResult],
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
    let clone_start = Instant::now();
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
    let parts = executor.parallelism().max(1).min(count.max(1));
    let chunk_size = count.div_ceil(parts);
    let compute_start = Instant::now();

    // 用 split_at_mut 把三份平行数组切成不相交的可变切片，任务里不再上锁。
    let mut results: Vec<RowResult> = (0..count).map(|_| RowResult::default()).collect();
    let mut tasks: Vec<Task<'_>> = Vec::with_capacity(parts);
    let mut public_left: Vec<EntityPublic> = starting_public;
    let mut state_left: Vec<EntityState> = starting_state;
    let mut results_left: &mut [RowResult] = &mut results;
    let mut first = 0;
    while !public_left.is_empty() {
        let size = chunk_size.min(public_left.len());
        let public: Vec<_> = public_left.drain(..size).collect();
        let state: Vec<_> = state_left.drain(..size).collect();
        let (result, result_rest) = std::mem::take(&mut results_left).split_at_mut(size);
        results_left = result_rest;
        tasks.push(Task {
            first,
            public,
            state,
            results: result,
        });
        first += size;
    }

    let mut jobs: Vec<Box<dyn FnOnce() + Send + '_>> = Vec::with_capacity(tasks.len());
    for slice in tasks.chunks_mut(1) {
        let shared_ref: &Shared<'_> = &shared;
        jobs.push(Box::new(move || run_task(&mut slice[0], shared_ref)));
    }
    executor.run(jobs);
    drop(tasks);
    times.compute = compute_start.elapsed();

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

/// 一个任务里的所有实体按顺序算完；任务之间互不相交。
fn run_task(task: &mut Task<'_>, shared: &Shared<'_>) {
    let mut public = std::mem::take(&mut task.public);
    let mut state = std::mem::take(&mut task.state);
    for (within, (public, state)) in public.drain(..).zip(state.drain(..)).enumerate() {
        let row = task.first + within;
        let uid = shared.uids[row];
        let mut me = EntityRow { public, state }.into_entity();
        let result = &mut task.results[within];
        let mut ctx = ParallelCtx {
            uid,
            shared,
            commands: &mut result.commands,
        };
        // 所在区块不在实体 tick 范围时跳过这一刻：实体仍留在快照里，位置和年龄不动，
        // 不发命令，别的实体照样查得到它。判断从只读方块视图来，和精确驱动一致。
        if shared
            .blocks
            .chunk_ticks_entities(ChunkPos::containing(me.position.x, me.position.z))
        {
            // `Entity::tick` 内部会跑 `common_tick`，这里不能重复。
            me.tick(&mut ctx);
        }
        let split = me.split();
        result.removed = split.public.removed;
        result.public = Some(split.public);
        result.state = Some(split.state);
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
    // 这个标记是算阶段顺手记下的，不用再对所有 uid 逐个查表。
    let mut publish_rows = Vec::with_capacity(rows.len());
    let mut publish_public = Vec::with_capacity(public.len());
    let mut publish_state = Vec::with_capacity(state.len());
    for (index, ((row, public), state)) in rows.iter().copied().zip(public).zip(state).enumerate() {
        if results[index].removed {
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
        if result.commands.commands.is_empty() || result.removed {
            continue;
        }
        let uid = uids[index];
        // 前面的命令可能已经把这个实体拿掉；`take_entity` 返回 None 就整批跳过。
        let Some(mut me) = world.entities_mut().take_entity(uid) else {
            continue;
        };
        for command in result.commands.commands.drain(..) {
            ctx::apply(command, &mut me, world);
        }
        if !me.removed {
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
mod tests;
