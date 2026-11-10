//! 并行驱动：所有实体对着阶段开始时的快照算，命令进缓冲，之后按实体顺序落地。
//!
//! 见 `docs/world-model-design.md` 8.2 和第 13 节。结果和线程数无关：算的时候只读
//! 不变的公开快照和自己那一行，落地的顺序固定为（uid 升序，实体内序号）。
//!
//! 和精确模式的差异只有「看到的别的实体是这一阶段开始时的样子」这一类，见
//! 设计稿第 13 节第 5 条。

use std::collections::BTreeMap;

use crate::{
    entity::{
        Entity, EntityPublic, EntityRow, EntityState, EntityUid,
        ctx::{self, Command, CommandBuffer, EntityCtx},
    },
    shape::Aabb,
    world::World,
};

/// 一个把「对每个下标区间跑一个闭包」分发出去的执行器。
///
/// 内联实现和 [`ScopedThreads`] 各一份；真正的任务系统以后在上层实现这个 trait。
pub trait Executor {
    fn run_chunks(&self, len: usize, work: &(dyn Fn(usize, usize) + Sync));
}

/// 一个线程也不开，按区间顺序跑。
pub struct InlineExecutor;

impl Executor for InlineExecutor {
    fn run_chunks(&self, len: usize, work: &(dyn Fn(usize, usize) + Sync)) {
        work(0, len);
    }
}

/// `std::thread::scope` 起 `threads` 个线程，把下标均分给它们。
pub struct ScopedThreads {
    pub threads: usize,
}

impl Executor for ScopedThreads {
    fn run_chunks(&self, len: usize, work: &(dyn Fn(usize, usize) + Sync)) {
        let threads = self.threads.max(1).min(len.max(1));
        if threads == 1 {
            work(0, len);
            return;
        }
        std::thread::scope(|scope| {
            for index in 0..threads {
                let start = len * index / threads;
                let end = len * (index + 1) / threads;
                scope.spawn(move || work(start, end));
            }
        });
    }
}

/// 按 16³ 段分格的行号索引；算阶段只读，由阶段开始时的 public 建好。
struct EntityGrid {
    cells: BTreeMap<(i32, i32, i32), Vec<usize>>,
}

impl EntityGrid {
    fn build(public: &[EntityPublic]) -> Self {
        let mut cells: BTreeMap<(i32, i32, i32), Vec<usize>> = BTreeMap::new();
        for (row, entity) in public.iter().enumerate() {
            for key in sections_of(entity.bounding_box()) {
                cells.entry(key).or_default().push(row);
            }
        }
        Self { cells }
    }

    fn rows_in(&self, area: Aabb) -> Vec<usize> {
        let (first, last) = section_range(&area);
        let mut rows = Vec::new();
        for ((x, y, z), set) in &self.cells {
            if (first.0..=last.0).contains(x)
                && (first.1..=last.1).contains(y)
                && (first.2..=last.2).contains(z)
            {
                rows.extend(set.iter().copied());
            }
        }
        rows.sort_unstable();
        rows.dedup();
        rows
    }
}

/// 并行阶段的一个任务：只读公开快照、只写自己的公开和私有行、命令进自己的缓冲。
struct ParallelCtx<'a> {
    uid: EntityUid,
    blocks: &'a (dyn ctx::BlockView + Sync),
    public: &'a [EntityPublic],
    uids: &'a [EntityUid],
    grid: &'a EntityGrid,
    commands: &'a mut CommandBuffer,
}

impl EntityCtx for ParallelCtx<'_> {
    fn uid(&self) -> EntityUid {
        self.uid
    }

    fn blocks(&self) -> &dyn ctx::BlockView {
        self.blocks
    }

    fn other(&self, uid: EntityUid) -> Option<EntityPublic> {
        self.uids
            .iter()
            .position(|&candidate| candidate == uid)
            .and_then(|row| self.public.get(row).cloned())
    }

    fn entity_uids(&self) -> Vec<EntityUid> {
        self.uids.to_vec()
    }

    fn for_each_other_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic)) {
        for row in self.grid.rows_in(area) {
            let Some(public) = self.public.get(row) else {
                continue;
            };
            if public.uid != self.uid && public.bounding_box().intersects(area) {
                visit(public);
            }
        }
    }

    fn emit(&mut self, command: Command, _me: &mut Entity) {
        self.commands.push(command);
    }
}

/// 并行地跑一刻。精确驱动在 [`World::tick_entities`](crate::World::tick_entities)。
///
/// 步骤：取快照 → 并行算 → 发布新的公开状态 → 按（uid 升序，实体内序号）落地命令
/// → 写回被命令改过的实体 → 并进这一刻新生成的实体。
pub fn tick_entities_parallel(
    blocks: &(dyn ctx::BlockView + Sync),
    world: &mut World<'_>,
    executor: &dyn Executor,
) {
    let _ = tick_entities_parallel_timed(blocks, world, executor);
}

/// 同 [`tick_entities_parallel`]，另外报出「算」「发布」「落地」各段的耗时。
pub fn tick_entities_parallel_timed(
    blocks: &(dyn ctx::BlockView + Sync),
    world: &mut World<'_>,
    executor: &dyn Executor,
) -> PhaseTimes {
    use std::time::Instant;
    let mut times = PhaseTimes::default();
    let snapshot_start = Instant::now();
    world.entities_mut().begin_entity_tick();
    let mut snapshot = world.level().entity_snapshot();
    times.clone = snapshot.clone_time;
    times.publish += snapshot_start.elapsed();
    let grid = EntityGrid::build(&snapshot.public);
    let uids: Vec<EntityUid> = snapshot.uids.clone();
    let public: Vec<EntityPublic> = snapshot.public.clone();
    let len = uids.len();

    // 每行的命令缓冲预分配好，任务只碰自己那一个。
    let mut commands: Vec<CommandBuffer> = (0..len).map(|_| CommandBuffer::default()).collect();
    // 执行器按自己的线程数把下标分成若干连续区间；每个区间拿到三份不相交的可变切片。
    // 先按顺序收集边界，再用 split_at_mut 逐段切出来，不需要 unsafe。
    let ranges = std::sync::Mutex::new(Vec::new());
    executor.run_chunks(len, &|start, end| {
        ranges.lock().expect("ranges mutex").push((start, end))
    });
    let mut ranges = ranges.into_inner().expect("ranges mutex");
    ranges.sort_by_key(|(start, _)| *start);

    struct Task<'a> {
        start: usize,
        end: usize,
        next: &'a mut [EntityPublic],
        state: &'a mut [EntityState],
        commands: &'a mut [CommandBuffer],
    }
    let mut tasks: Vec<Task<'_>> = Vec::with_capacity(ranges.len());
    let mut remaining_next: &mut [EntityPublic] = &mut snapshot.next_public;
    let mut remaining_state: &mut [EntityState] = &mut snapshot.state;
    let mut remaining_commands: &mut [CommandBuffer] = &mut commands;
    let mut consumed = 0;
    for (start, end) in &ranges {
        let skip = *start - consumed;
        let size = end - start;
        let (head, tail) = remaining_next.split_at_mut(skip);
        let (mine, rest) = tail.split_at_mut(size);
        remaining_next = rest;
        let _ = head;
        let (head, tail) = remaining_state.split_at_mut(skip);
        let (mine_state, rest) = tail.split_at_mut(size);
        remaining_state = rest;
        let _ = head;
        let (head, tail) = remaining_commands.split_at_mut(skip);
        let (mine_commands, rest) = tail.split_at_mut(size);
        remaining_commands = rest;
        let _ = head;
        tasks.push(Task {
            start: *start,
            end: *end,
            next: mine,
            state: mine_state,
            commands: mine_commands,
        });
        consumed = *end;
    }

    // 每个任务独占自己那几行；public/uids/blocks/grid 只读共享。
    // 用 split_at_mut 把 tasks 按执行器的区间切成不相交的可变切片，线程里不再上锁。
    let compute_start = Instant::now();
    let uids_ref: &[EntityUid] = &uids;
    let public_ref: &[EntityPublic] = &public;
    let grid_ref = &grid;
    let blocks_ref: &(dyn ctx::BlockView + Sync) = blocks;
    let task_ranges: Vec<(usize, usize)> = {
        let collected = std::sync::Mutex::new(Vec::new());
        executor.run_chunks(tasks.len(), &|start, end| {
            collected.lock().expect("ranges mutex").push((start, end));
        });
        let mut ranges = collected.into_inner().expect("ranges mutex");
        ranges.sort_by_key(|(start, _)| *start);
        ranges
    };
    let mut remaining: &mut [Task<'_>] = &mut tasks;
    let mut consumed_task = 0;
    let mut slices: Vec<Option<&mut [Task<'_>]>> = Vec::with_capacity(task_ranges.len());
    for (start, end) in &task_ranges {
        let skip = start - consumed_task;
        let (_head, tail) = remaining.split_at_mut(skip);
        let (mine, rest) = tail.split_at_mut(end - start);
        remaining = rest;
        slices.push(Some(mine));
        consumed_task = *end;
    }
    let slices = std::sync::Mutex::new(slices);
    executor.run_chunks(task_ranges.len(), &|start, end| {
        // 每个区间只取自己那一份，锁在取出的瞬间释放。
        let mut all = slices.lock().expect("slices mutex");
        let mut mine: Vec<Option<&mut [Task<'_>]>> =
            (start..end).map(|index| all[index].take()).collect();
        drop(all);
        for slice in mine.iter_mut().flatten() {
            for task in slice.iter_mut() {
                for (offset, row) in (task.start..task.end).enumerate() {
                    let uid = uids_ref[row];
                    let mut me = EntityRow {
                        public: public_ref[row].clone(),
                        state: task.state[offset].clone(),
                    }
                    .into_entity();
                    // `Entity::tick` 内部会跑 `common_tick`，这里不能重复。
                    let mut ctx = ParallelCtx {
                        uid,
                        blocks: blocks_ref,
                        public: public_ref,
                        uids: uids_ref,
                        grid: grid_ref,
                        commands: &mut task.commands[offset],
                    };
                    me.tick(&mut ctx);
                    let split = me.split();
                    task.next[offset] = split.public;
                    task.state[offset] = split.state;
                }
            }
        }
        // 放回去，保证 `tasks` 仍然完整。
        let mut all = slices.lock().expect("slices mutex");
        for (offset, slot) in mine.drain(..).enumerate() {
            all[start + offset] = slot;
        }
    });
    times.compute = compute_start.elapsed();

    // 发布新的公开和私有数据，再按（uid 升序，实体内序号）串行落地命令。
    // 命令处理函数改的是表里的实体，所以先发布再落地；落地后把改动写回 snapshot。
    let publish_start = Instant::now();
    world.entities_mut().entity_publish(snapshot);
    // 算阶段就 `discard` 的实体不留在表里，和精确模式「tick 过就不 put 回去」一致。
    let removed: Vec<EntityUid> = world
        .level()
        .entity_uids()
        .into_iter()
        .filter(|&uid| {
            world
                .level()
                .entity_public(uid)
                .is_some_and(|public| public.removed)
        })
        .collect();
    for uid in removed {
        world.entities_mut().remove_entity(uid);
    }
    times.publish += publish_start.elapsed();
    let apply_start = Instant::now();
    for (row, buffer) in commands.iter_mut().enumerate() {
        let uid = uids[row];
        if world.level().entity_public(uid).is_none() {
            continue;
        }
        let Some(mut me) = world.entities_mut().take_entity(uid) else {
            continue;
        };
        for command in buffer.commands.drain(..) {
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
    pub clone: std::time::Duration,
    pub compute: std::time::Duration,
    pub publish: std::time::Duration,
    pub apply: std::time::Duration,
}

fn sections_of(bounds: Aabb) -> Vec<(i32, i32, i32)> {
    let (first, last) = section_range(&bounds);
    let mut keys = Vec::new();
    for x in first.0..=last.0 {
        for y in first.1..=last.1 {
            for z in first.2..=last.2 {
                keys.push((x, y, z));
            }
        }
    }
    keys
}

fn section_range(area: &Aabb) -> ((i32, i32, i32), (i32, i32, i32)) {
    const SIZE: i32 = 16;
    (
        (
            (area.min_x.floor() as i32).div_euclid(SIZE),
            (area.min_y.floor() as i32).div_euclid(SIZE),
            (area.min_z.floor() as i32).div_euclid(SIZE),
        ),
        (
            (area.max_x.floor() as i32).div_euclid(SIZE),
            (area.max_y.floor() as i32).div_euclid(SIZE),
            (area.max_z.floor() as i32).div_euclid(SIZE),
        ),
    )
}
#[cfg(test)]
mod tests;
