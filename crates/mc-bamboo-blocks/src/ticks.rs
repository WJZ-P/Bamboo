//! 计划刻队列，对应官方的 `LevelTicks`。方块和流体各有一条，`ServerLevel.tick` 先跑方块的，再跑流体的。
//!
//! 官方按区块分容器，这里的世界不卸载区块，一个全局队列的执行顺序和官方相同：
//! 先比到期时间，再比优先级，最后比加入的先后。

use std::{
    collections::{BTreeMap, BTreeSet, HashSet, VecDeque},
    fmt,
    hash::Hash,
};

use mc_bamboo_core::{BlockPos, ChunkPos};

/// `ServerLevel.tick` 给每条队列的上限：一刻最多跑这么多个，剩下的留到下一刻。
const MAX_TICKS_PER_TICK: usize = 65_536;

/// 一张表正在 tick 中途，或者有待放回的方块实体，现在不能整块搬走。
///
/// 只有 [`BlockTicks::take_chunk`] 和 `BlockEntities::take_chunk` 会返回它；
/// 调用方等到 tick 边界再试。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Busy;

impl fmt::Display for Busy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the table is in the middle of a tick and cannot move a chunk")
    }
}

impl std::error::Error for Busy {}

/// 官方 `TickPriority`，值越小越先执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TickPriority {
    ExtremelyHigh = -3,
    VeryHigh = -2,
    High = -1,
    Normal = 0,
    Low = 1,
    VeryLow = 2,
    ExtremelyLow = 3,
}

/// 队列的排序键，`ScheduledTick.DRAIN_ORDER` 加上最后一项决胜键。
///
/// 子序号是 `i64`：官方是 `long`，`LevelChunkTicks.unpack` 给从磁盘装入的刻分配负数，
/// 让它们排在之后新排的、同一（时刻，优先级）的刻前面。运行时排的刻仍从 0 递增。
///
/// 位置是 Bamboo 自己定的决胜规则。不同区块装入的刻可能拿到完全相同的
/// （时刻，优先级，子序号）；官方 `LevelTicks.CONTAINER_DRAIN_ORDER` 只比到子序号，
/// 这种先后不确定。这里按位置定死，让顺序可重现。方块和流体各有自己的队列，
/// 同一条队列里不需要再区分种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TickOrder {
    trigger_tick: i64,
    priority: TickPriority,
    sub_tick_order: i64,
    pos: BlockPos,
}

/// 排队的一个方块的刻。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledTick {
    pub pos: BlockPos,
    /// 方块的注册表 ID。到期时这一格换成了别的方块，这个刻就不执行。
    pub block: usize,
    pub trigger_tick: i64,
    pub priority: TickPriority,
}

/// 排队的一个流体的刻。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledFluidTick {
    pub pos: BlockPos,
    /// 流体的注册表 ID，源头和流动的水是两种。到期时这一格的流体不是这一种，这个刻就不执行。
    pub fluid: i32,
    pub trigger_tick: i64,
    pub priority: TickPriority,
}

/// 区块能不能 tick 的判断，由调用方从 [`crate::Level::chunk_ticks_blocks`] 传进来。
type ChunkTickCheck<'a> = dyn Fn(ChunkPos) -> bool + 'a;

/// 一条队列，`T` 是方块或者流体的注册表 ID。
#[derive(Debug)]
struct TickQueue<T> {
    queue: BTreeMap<TickOrder, (BlockPos, T)>,
    /// 同一格同一种排队时只留第一个，`LevelChunkTicks.ticksPerPosition`。
    /// 搁置的刻也留在里面：官方 `sortContainersToTick` 不放行时条目仍在区块容器里。
    scheduled: HashSet<(BlockPos, T)>,
    /// 这一刻已经取出、还没有执行的计划刻，`willTickThisTick` 看的就是它们。
    running: VecDeque<(BlockPos, T)>,
    /// 到期时所在区块不能 tick 的刻，按区块分组，每条都留着完整的排序键。
    ///
    /// 恢复时整组放回 `queue`，和别的刻按（时刻，优先级，子序号，位置）合在一起执行；
    /// 恢复这一步只扫「有搁置刻的区块数」，不扫搁置的刻数。
    deferred: BTreeMap<ChunkPos, Vec<(TickOrder, BlockPos, T)>>,
}

impl<T> Default for TickQueue<T> {
    fn default() -> Self {
        Self {
            queue: BTreeMap::new(),
            scheduled: HashSet::new(),
            running: VecDeque::new(),
            deferred: BTreeMap::new(),
        }
    }
}

impl<T: Copy + Eq + Hash> TickQueue<T> {
    fn schedule(&mut self, pos: BlockPos, kind: T, order: TickOrder) -> bool {
        if !self.scheduled.insert((pos, kind)) {
            return false;
        }
        self.queue.insert(order, (pos, kind));
        true
    }

    fn has_scheduled(&self, pos: BlockPos, kind: T) -> bool {
        self.scheduled.contains(&(pos, kind))
    }

    fn will_tick_this_tick(&self, pos: BlockPos, kind: T) -> bool {
        self.running.contains(&(pos, kind))
    }

    /// 全部排着的刻，按执行顺序：主队列和搁置表里都算，按完整排序键归并。
    ///
    /// 搁置表为空（默认情况）时直接返回主队列的迭代器，不做任何额外工作；
    /// 非空时只把少数搁置条目排好，和主队列做两路归并。
    fn pending(&self) -> impl Iterator<Item = (&TickOrder, BlockPos, T)> {
        let main = self
            .queue
            .iter()
            .map(|(order, &(pos, kind))| (order, pos, kind));
        if self.deferred.is_empty() {
            return Either::Main(main);
        }
        let mut deferred: Vec<_> = self
            .deferred
            .values()
            .flatten()
            .map(|(order, pos, kind)| (order, *pos, *kind))
            .collect();
        deferred.sort_unstable_by_key(|(order, _, _)| **order);
        Either::Merged(merge_pending(main, deferred.into_iter()))
    }

    /// `LevelTicks.sortContainersToTick` 加 `collectTicks`：把到期的计划刻取出来等着执行，
    /// 之后排的要等下一刻。不能 tick 的区块里到期的刻整组搁置，保留原来的排序键。
    ///
    /// `can_tick` 由调用方从 [`crate::Level::chunk_ticks_blocks`] 传进来：搁置表里的
    /// 每个区块问一次，主队列里每条到期的刻问一次。没到期的刻一条都不碰，
    /// 排刻路径一行都不改。
    fn collect_due(&mut self, now: i64, can_tick: &ChunkTickCheck<'_>) {
        // 先把现在能 tick 的搁置区块整组放回主队列。开销和「有搁置刻的区块数」成正比。
        if !self.deferred.is_empty() {
            let ready: Vec<ChunkPos> = self
                .deferred
                .keys()
                .copied()
                .filter(|&chunk| can_tick(chunk))
                .collect();
            for chunk in ready {
                let entries = self
                    .deferred
                    .remove(&chunk)
                    .expect("key came from the deferred map");
                for (order, pos, kind) in entries {
                    self.queue.insert(order, (pos, kind));
                }
            }
        }

        while self.running.len() < MAX_TICKS_PER_TICK
            && let Some(entry) = self.queue.first_entry()
            && entry.key().trigger_tick <= now
        {
            let order = *entry.key();
            let (pos, kind) = entry.remove();
            if can_tick(pos.chunk()) {
                self.scheduled.remove(&(pos, kind));
                self.running.push_back((pos, kind));
            } else {
                // 条目只移一次：从此不再经过主队列，直到区块恢复。
                self.deferred
                    .entry(pos.chunk())
                    .or_default()
                    .push((order, pos, kind));
            }
        }
    }

    fn next_due(&mut self) -> Option<(BlockPos, T)> {
        self.running.pop_front()
    }
}

/// `pending()` 的两种返回形状：搁置表为空时是主队列的迭代器，否则是两路归并。
enum Either<A, B> {
    Main(A),
    Merged(B),
}

impl<I, A, B> Iterator for Either<A, B>
where
    A: Iterator<Item = I>,
    B: Iterator<Item = I>,
{
    type Item = I;

    fn next(&mut self) -> Option<I> {
        match self {
            Self::Main(iter) => iter.next(),
            Self::Merged(iter) => iter.next(),
        }
    }
}

/// 两个都按排序键排好的迭代器归并，键相同的时候主队列在前（和主队列里的顺序一致）。
fn merge_pending<'a, T: Copy + 'a>(
    mut main: impl Iterator<Item = (&'a TickOrder, BlockPos, T)> + 'a,
    mut deferred: impl Iterator<Item = (&'a TickOrder, BlockPos, T)> + 'a,
) -> impl Iterator<Item = (&'a TickOrder, BlockPos, T)> + 'a {
    let mut left = main.next();
    let mut right = deferred.next();
    std::iter::from_fn(move || match (left, right) {
        (Some(a), Some(b)) => {
            if *a.0 <= *b.0 {
                left = main.next();
                Some(a)
            } else {
                right = deferred.next();
                Some(b)
            }
        }
        (Some(a), None) => {
            left = main.next();
            Some(a)
        }
        (None, Some(b)) => {
            right = deferred.next();
            Some(b)
        }
        (None, None) => None,
    })
}

/// 全部排队的计划刻：方块一条队列，流体一条队列。
///
/// 每张队列另有一张「搁置表」：到期时所在区块不能 tick 的刻按区块收起来，
/// 保留原来的完整排序键；区块恢复时整组放回主队列，和别的刻合在一起执行。
#[derive(Debug, Default)]
pub struct BlockTicks {
    /// 现在的 `gameTime`，由 [`run_scheduled_ticks`](crate::run_scheduled_ticks) 更新。
    now: i64,
    /// `Level.nextSubTickCount`，两条队列共用。
    next_order: u64,
    blocks: TickQueue<usize>,
    fluids: TickQueue<i32>,
}

impl BlockTicks {
    pub fn new() -> Self {
        Self::default()
    }

    /// 现在能不能按区块搬计划刻：两条队列里都没有取出、还没跑完的刻。
    ///
    /// [`Self::take_chunks`] 和 [`Self::take_chunk`] 的前提，可以在动手之前只读地问一声，
    /// 好让一次要搬几张表的调用方先检查全部前提。
    pub fn ready_to_move_chunks(&self) -> bool {
        self.blocks.running.is_empty() && self.fluids.running.is_empty()
    }

    /// 现在的 `gameTime`。
    pub const fn now(&self) -> i64 {
        self.now
    }

    /// 把时钟拨到 `now`，不取出也不运行任何计划刻。
    ///
    /// 官方的假世界直接从一个游戏时间开始，没有先跑一刻；重放动作之前也只要把时钟对上。
    pub fn set_time(&mut self, now: i64) {
        self.now = now;
    }

    fn order(&mut self, pos: BlockPos, delay: i32, priority: TickPriority) -> TickOrder {
        let order = TickOrder {
            trigger_tick: self.now.saturating_add(i64::from(delay)),
            priority,
            sub_tick_order: self.next_order as i64,
            pos,
        };
        self.next_order += 1;
        order
    }

    /// 给方块排一个刻。这一格这个方块已经排着时什么也不做。
    pub fn schedule(&mut self, pos: BlockPos, block: usize, delay: i32, priority: TickPriority) {
        if !self.blocks.has_scheduled(pos, block) {
            let order = self.order(pos, delay, priority);
            self.blocks.schedule(pos, block, order);
        }
    }

    /// 给流体排一个刻，`fluid` 是流体的注册表 ID。这一格这种流体已经排着时什么也不做。
    pub fn schedule_fluid(
        &mut self,
        pos: BlockPos,
        fluid: i32,
        delay: i32,
        priority: TickPriority,
    ) {
        if !self.fluids.has_scheduled(pos, fluid) {
            let order = self.order(pos, delay, priority);
            self.fluids.schedule(pos, fluid, order);
        }
    }

    /// `getBlockTicks().hasScheduledTick`。
    pub fn has_scheduled_tick(&self, pos: BlockPos, block: usize) -> bool {
        self.blocks.has_scheduled(pos, block)
    }

    /// `getBlockTicks().willTickThisTick`：已经到期、这一刻还没轮到的计划刻。
    pub fn will_tick_this_tick(&self, pos: BlockPos, block: usize) -> bool {
        self.blocks.will_tick_this_tick(pos, block)
    }

    /// 全部排队的方块计划刻，按执行顺序，搁置的也算。oracle 用它比较结果。
    pub fn pending(&self) -> impl Iterator<Item = ScheduledTick> {
        self.blocks
            .pending()
            .map(|(order, pos, block)| ScheduledTick {
                pos,
                block,
                trigger_tick: order.trigger_tick,
                priority: order.priority,
            })
    }

    /// 全部排队的流体计划刻，按执行顺序，搁置的也算。
    pub fn pending_fluids(&self) -> impl Iterator<Item = ScheduledFluidTick> {
        self.fluids
            .pending()
            .map(|(order, pos, fluid)| ScheduledFluidTick {
                pos,
                fluid,
                trigger_tick: order.trigger_tick,
                priority: order.priority,
            })
    }

    /// 把这一刻到期的方块计划刻取出来，同时把时钟拨到 `now`。
    ///
    /// `can_tick` 是调用方从 [`crate::Level::chunk_ticks_blocks`] 拿来的判断。
    pub(crate) fn collect_due(&mut self, now: i64, can_tick: &ChunkTickCheck<'_>) {
        self.now = now;
        self.blocks.collect_due(now, can_tick);
    }

    /// 下一个要执行的方块计划刻。
    pub(crate) fn next_due(&mut self) -> Option<(BlockPos, usize)> {
        self.blocks.next_due()
    }

    /// 方块的刻都跑完以后，取出这一刻到期的流体计划刻。
    pub(crate) fn collect_due_fluids(&mut self, can_tick: &ChunkTickCheck<'_>) {
        self.fluids.collect_due(self.now, can_tick);
    }

    /// 下一个要执行的流体计划刻。
    pub(crate) fn next_fluid_due(&mut self) -> Option<(BlockPos, i32)> {
        self.fluids.next_due()
    }

    /// 把一批区块里所有排着的计划刻装进盒子搬走。只在 tick 边界调用。
    ///
    /// 每条队列只扫一遍：开销和队列长度成正比，和要取多少个区块无关。
    /// **这里刻意不维护按区块的索引**：微基准里，每排一条刻多一次哈希和一次
    /// Vec push 会让「排完再跑完一百万条刻」从 350 ms 变 440 ms、滚动场景从
    /// 164 ms 变 226 ms，而排刻是红石和流体每刻都在做的热路径，取出区块只在
    /// 卸载和 region 拆合时发生。调用方要把一整刻要卸载的区块攒成一批，
    /// 每刻最多调一次 [`Self::take_chunks`]。
    ///
    /// 返回的 map 只含有排着刻的区块；`chunks` 里没有刻的区块不给键，也不给空盒子。
    /// 这一刻已经取出、还没跑完的清单不为空时返回 [`Err`]，因为那些刻已经不在队列里，
    /// 没法跟着区块走。别的区块的刻原地不动。
    pub fn take_chunks(
        &mut self,
        chunks: &BTreeSet<ChunkPos>,
    ) -> Result<BTreeMap<ChunkPos, ChunkTicks>, Busy> {
        if !self.ready_to_move_chunks() {
            return Err(Busy);
        }
        let blocks = self.blocks.take_chunks(chunks, self.now);
        let fluids = self.fluids.take_chunks(chunks, self.now);
        let mut boxes: BTreeMap<ChunkPos, ChunkTicks> = blocks
            .into_iter()
            .map(|(chunk, blocks)| {
                (
                    chunk,
                    ChunkTicks {
                        blocks,
                        fluids: Vec::new(),
                    },
                )
            })
            .collect();
        for (chunk, fluids) in fluids {
            boxes
                .entry(chunk)
                .or_insert_with(|| ChunkTicks {
                    blocks: Vec::new(),
                    fluids: Vec::new(),
                })
                .fluids = fluids;
        }
        Ok(boxes)
    }

    /// 把 `chunk` 里所有排着的计划刻装进盒子搬走。只在 tick 边界调用。
    ///
    /// 见 [`Self::take_chunks`]：这也是一次全队列扫描，一次要搬多个区块时用批量接口。
    pub fn take_chunk(&mut self, chunk: ChunkPos) -> Result<ChunkTicks, Busy> {
        let chunks = [chunk].into_iter().collect();
        let mut boxes = self.take_chunks(&chunks)?;
        Ok(boxes.remove(&chunk).unwrap_or_else(|| ChunkTicks {
            blocks: Vec::new(),
            fluids: Vec::new(),
        }))
    }

    /// 把盒子里的计划刻放回表里。只在 tick 边界调用。
    ///
    /// 调用方保证表里已经没有 `chunk` 的条目。到期时刻按这张表的 `now` 加上盒子里存的
    /// 相对延迟算出，所以从另一张时钟不同的表搬过来也能保持原来的剩余时间。
    pub fn insert_chunk(&mut self, ticks: ChunkTicks) {
        // 调用方保证表里已经没有这个区块的条目；盒子本身只装一个区块。
        debug_assert!(
            ticks
                .blocks
                .first()
                .map(|first| first.pos.chunk())
                .is_none_or(|chunk| {
                    ticks.blocks.iter().all(|tick| tick.pos.chunk() == chunk)
                        && ticks.fluids.iter().all(|tick| tick.pos.chunk() == chunk)
                })
        );
        self.blocks.insert_chunk(ticks.blocks, self.now);
        self.fluids.insert_chunk(ticks.fluids, self.now);
    }
}

/// 一个区块的全部计划刻，方块和流体各一条，按各自原来的执行顺序排。
///
/// 每条都存相对延迟而不是绝对到期时刻：region 合并拆分、区块存盘和加载时
/// 两边的模拟时间不一定相同（见 `docs/world-model-design.md` 6.3 节）。
#[derive(Debug)]
pub struct ChunkTicks {
    blocks: Vec<SavedTick<usize>>,
    fluids: Vec<SavedTick<i32>>,
}

/// 官方 `SavedTick`：位置、种类、优先级和相对延迟。
#[derive(Debug, Clone, Copy)]
struct SavedTick<T> {
    pos: BlockPos,
    kind: T,
    priority: TickPriority,
    delay: i64,
}

impl<T: Copy + Eq + Hash> TickQueue<T> {
    /// 扫一遍队列，按区块把要搬走的刻分组。每个盒子里的条目天然是执行顺序。
    ///
    /// 搁置表里这个区块的刻也一起装盒：它们的到期时刻已经过去，相对延迟是负数或 0，
    /// 排在这个区块其它刻前面，彼此仍按原来的排序键。
    fn take_chunks(
        &mut self,
        chunks: &BTreeSet<ChunkPos>,
        now: i64,
    ) -> BTreeMap<ChunkPos, Vec<SavedTick<T>>> {
        if chunks.is_empty() {
            return BTreeMap::new();
        }
        let mut boxes: BTreeMap<ChunkPos, Vec<SavedTick<T>>> = BTreeMap::new();
        // 先放搁置的刻，按完整排序键排好；它们全都已经到期，延迟不会大于 0。
        for chunk in chunks {
            let Some(entries) = self.deferred.remove(chunk) else {
                continue;
            };
            let mut entries = entries;
            entries.sort_unstable_by_key(|(order, _, _)| *order);
            for (order, pos, kind) in entries {
                self.scheduled.remove(&(pos, kind));
                boxes.entry(*chunk).or_default().push(SavedTick {
                    pos,
                    kind,
                    priority: order.priority,
                    delay: order.trigger_tick.saturating_sub(now),
                });
            }
        }
        let mut taken: Vec<(ChunkPos, TickOrder, BlockPos, T)> = Vec::new();
        // `extract_if` 在 Rust 1.96 之前不稳定，用 retain 一次扫描完成删除和分组。
        self.queue.retain(|&order, &mut (pos, kind)| {
            let chunk = pos.chunk();
            if !chunks.contains(&chunk) {
                return true;
            }
            taken.push((chunk, order, pos, kind));
            false
        });
        for (chunk, order, pos, kind) in taken {
            self.scheduled.remove(&(pos, kind));
            boxes.entry(chunk).or_default().push(SavedTick {
                pos,
                kind,
                priority: order.priority,
                delay: order.trigger_tick.saturating_sub(now),
            });
        }
        boxes
    }

    /// `LevelChunkTicks.unpack`：子序号从 `-条目数` 开始递增的负数。
    /// 盒子里的条目按（到期时刻，优先级，子序号）排好，只有（时刻，优先级）相同的
    /// 两条刻才会比到子序号，所以按这个顺序重新分配递增编号和官方按子序号存等价。
    fn insert_chunk(&mut self, saved: Vec<SavedTick<T>>, now: i64) {
        let first = -(saved.len() as i64);
        for (sub_tick_order, entry) in (first..).zip(saved) {
            let order = TickOrder {
                trigger_tick: now.saturating_add(entry.delay),
                priority: entry.priority,
                sub_tick_order,
                pos: entry.pos,
            };
            debug_assert!(order.sub_tick_order < 0);
            self.schedule(entry.pos, entry.kind, order);
        }
    }
}

/// 三个盒子都要求 `Send + Debug`：区块束会被 move 到别的线程（4.4 节）。
const _: fn() = || {
    fn assert_send_debug<T: Send + fmt::Debug>() {}
    assert_send_debug::<ChunkTicks>();
};

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        collections::{HashMap, HashSet},
    };

    use super::*;

    const POS: BlockPos = BlockPos::new(1, 2, 3);
    const OTHER: BlockPos = BlockPos::new(4, 5, 6);

    /// 默认行为：所有区块都能 tick。
    fn all_chunks_tick(_: ChunkPos) -> bool {
        true
    }

    /// 一组可变的「现在哪些区块冻结」，用来给两次 collect 之间换状态。
    struct FrozenMask(Cell<u32>);

    impl FrozenMask {
        fn new(mask: u32) -> Self {
            Self(Cell::new(mask))
        }

        fn set(&self, mask: u32) {
            self.0.set(mask);
        }

        fn mask(&self) -> u32 {
            self.0.get()
        }

        fn check(&self) -> impl Fn(ChunkPos) -> bool + '_ {
            |chunk| (self.0.get() & (1_u32 << chunk.x.rem_euclid(32) as u32)) == 0
        }
    }

    fn drain(ticks: &mut BlockTicks, now: i64) -> Vec<(BlockPos, usize)> {
        ticks.collect_due(now, &all_chunks_tick);
        std::iter::from_fn(|| ticks.next_due()).collect()
    }

    fn drain_fluids(ticks: &mut BlockTicks, now: i64) -> Vec<(BlockPos, i32)> {
        ticks.collect_due(now, &all_chunks_tick);
        ticks.collect_due_fluids(&all_chunks_tick);
        std::iter::from_fn(|| ticks.next_fluid_due()).collect()
    }

    #[test]
    fn ticks_run_when_they_are_due() {
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 7, 2, TickPriority::Normal);
        assert!(ticks.has_scheduled_tick(POS, 7));
        assert_eq!(drain(&mut ticks, 1), []);
        assert_eq!(drain(&mut ticks, 2), [(POS, 7)]);
        assert!(!ticks.has_scheduled_tick(POS, 7));
    }

    #[test]
    fn the_same_block_is_only_queued_once() {
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 7, 5, TickPriority::Normal);
        ticks.schedule(POS, 7, 1, TickPriority::Normal);
        ticks.schedule(POS, 8, 5, TickPriority::Normal);
        assert_eq!(ticks.pending().count(), 2);
        // 第二次排队被丢弃，到期时间还是第一次的。
        assert_eq!(drain(&mut ticks, 1), []);
    }

    #[test]
    fn order_is_time_then_priority_then_arrival() {
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 1, 2, TickPriority::Normal);
        ticks.schedule(POS, 2, 1, TickPriority::Low);
        ticks.schedule(POS, 3, 1, TickPriority::High);
        ticks.schedule(OTHER, 3, 1, TickPriority::High);
        assert_eq!(
            drain(&mut ticks, 2),
            [(POS, 3), (OTHER, 3), (POS, 2), (POS, 1)]
        );
    }

    #[test]
    fn collected_ticks_are_visible_until_they_run() {
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 1, 1, TickPriority::Normal);
        ticks.schedule(OTHER, 1, 1, TickPriority::Normal);
        ticks.collect_due(1, &all_chunks_tick);
        assert_eq!(ticks.next_due(), Some((POS, 1)));
        assert!(!ticks.will_tick_this_tick(POS, 1));
        assert!(ticks.will_tick_this_tick(OTHER, 1));
    }

    #[test]
    fn ticks_scheduled_while_running_wait_for_the_next_collection() {
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 1, 1, TickPriority::Normal);
        ticks.collect_due(1, &all_chunks_tick);
        assert_eq!(ticks.next_due(), Some((POS, 1)));
        // 执行时再排一个 0 延迟的刻，它不属于这一刻的这一批。
        ticks.schedule(POS, 1, 0, TickPriority::Normal);
        assert_eq!(ticks.next_due(), None);
        assert_eq!(drain(&mut ticks, 2), [(POS, 1)]);
    }

    #[test]
    fn fluids_have_their_own_queue() {
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 2, 5, TickPriority::Normal);
        ticks.schedule_fluid(POS, 2, 5, TickPriority::Normal);
        ticks.schedule_fluid(POS, 2, 1, TickPriority::Normal);
        ticks.schedule_fluid(POS, 1, 5, TickPriority::Normal);
        assert_eq!(ticks.pending().count(), 1);
        assert_eq!(ticks.pending_fluids().count(), 2);
        assert_eq!(drain_fluids(&mut ticks, 4), []);
        assert_eq!(drain_fluids(&mut ticks, 5), [(POS, 2), (POS, 1)]);
        // 方块的那一个还在自己的队列里等着。
        assert_eq!(ticks.next_due(), Some((POS, 2)));
        assert_eq!(ticks.pending_fluids().count(), 0);
    }

    #[test]
    fn one_tick_runs_at_most_the_official_number_of_ticks() {
        let mut ticks = BlockTicks::new();
        let count = MAX_TICKS_PER_TICK + 3;
        for x in 0..count {
            let pos = BlockPos::new(x as i32, 0, 0);
            ticks.schedule_fluid(pos, 1, 1, TickPriority::Normal);
        }
        assert_eq!(drain_fluids(&mut ticks, 1).len(), MAX_TICKS_PER_TICK);
        assert_eq!(drain_fluids(&mut ticks, 2).len(), 3);
    }

    /// 两个不同区块的位置。`POS` / `OTHER` 都在 0 号区块。
    const CHUNK_A: ChunkPos = ChunkPos::new(0, 0);
    const AWAY: BlockPos = BlockPos::new(20, 2, 3);
    const AWAY_2: BlockPos = BlockPos::new(21, 2, 3);
    const AWAY_3: BlockPos = BlockPos::new(22, 2, 3);

    /// 一个区块里所有待跑的刻，把流体也一起按各自的时刻收进来。
    fn all_due(ticks: &mut BlockTicks, now: i64) -> Vec<(BlockPos, i64)> {
        let mut out = Vec::new();
        ticks.collect_due(now, &all_chunks_tick);
        while let Some((pos, block)) = ticks.next_due() {
            out.push((pos, block as i64));
        }
        ticks.collect_due_fluids(&all_chunks_tick);
        while let Some((pos, fluid)) = ticks.next_fluid_due() {
            out.push((pos, i64::from(fluid)));
        }
        out
    }

    fn block_keys(ticks: &BlockTicks) -> Vec<ScheduledTick> {
        ticks.pending().collect()
    }

    fn fluid_keys(ticks: &BlockTicks) -> Vec<ScheduledFluidTick> {
        ticks.pending_fluids().collect()
    }

    #[test]
    fn taking_a_chunk_leaves_no_trace_and_keeps_other_chunks() {
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 7, 3, TickPriority::High);
        ticks.schedule(OTHER, 8, 3, TickPriority::Low);
        ticks.schedule_fluid(POS, 2, 4, TickPriority::Normal);
        ticks.schedule(AWAY, 9, 3, TickPriority::High);
        ticks.schedule_fluid(AWAY, 3, 4, TickPriority::Normal);

        let boxed = ticks.take_chunk(CHUNK_A).unwrap();
        assert!(!ticks.has_scheduled_tick(POS, 7));
        assert!(!ticks.has_scheduled_tick(OTHER, 8));
        assert!(!ticks.will_tick_this_tick(POS, 7));
        assert!(ticks.has_scheduled_tick(AWAY, 9));
        assert_eq!(ticks.pending().count(), 1);
        assert_eq!(ticks.pending_fluids().count(), 1);
        assert!(format!("{boxed:?}").contains("ChunkTicks"));

        // 别的区块的到期时刻、优先级、种类都没变。
        assert_eq!(block_keys(&ticks)[0].trigger_tick, 3);
        assert_eq!(fluid_keys(&ticks)[0].trigger_tick, 4);
        assert_eq!(all_due(&mut ticks, 9), [(AWAY, 9), (AWAY, 3)]);

        // 取出的盒子放进新表，两条队列的条目和相对延迟都在。
        ticks.insert_chunk(boxed);
        assert!(ticks.has_scheduled_tick(POS, 7));
        assert!(ticks.has_scheduled_tick(OTHER, 8));
        assert_eq!(ticks.pending().count(), 2);
        assert_eq!(ticks.pending_fluids().count(), 1);
    }

    #[test]
    fn taking_and_putting_back_keeps_the_same_due_ticks() {
        let mut untouched = BlockTicks::new();
        let mut moved = BlockTicks::new();
        let entries = [
            (POS, 1, 2, TickPriority::High),
            (OTHER, 2, 2, TickPriority::High),
            (POS, 3, 5, TickPriority::Normal),
            (AWAY, 4, 2, TickPriority::High),
            (AWAY_2, 5, 2, TickPriority::Normal),
        ];
        for &(pos, block, delay, priority) in &entries {
            untouched.schedule(pos, block, delay, priority);
            moved.schedule(pos, block, delay, priority);
        }
        untouched.schedule_fluid(POS, 1, 2, TickPriority::Normal);
        moved.schedule_fluid(POS, 1, 2, TickPriority::Normal);
        untouched.schedule_fluid(AWAY, 2, 2, TickPriority::Normal);
        moved.schedule_fluid(AWAY, 2, 2, TickPriority::Normal);

        let boxed = moved.take_chunk(CHUNK_A).unwrap();
        assert!(moved.take_chunk(CHUNK_A).unwrap().blocks.is_empty());
        moved.insert_chunk(boxed);

        // 取出立刻原样放回：每一刻到期的刻和没搬过完全一样。
        for now in 0..8 {
            assert_eq!(all_due(&mut untouched, now), all_due(&mut moved, now));
        }
        assert_eq!(block_keys(&untouched), block_keys(&moved));
        assert_eq!(fluid_keys(&untouched), fluid_keys(&moved));
    }

    #[test]
    fn inserted_ticks_go_before_later_scheduled_ticks_of_the_same_time_and_priority() {
        let mut moved = BlockTicks::new();
        moved.schedule(POS, 1, 3, TickPriority::Normal);
        moved.schedule(OTHER, 2, 3, TickPriority::Normal);
        let boxed = moved.take_chunk(CHUNK_A).unwrap();

        // 同一（时刻，优先级）的刻在搬走之后才排：放入的要排在它前面。
        let mut target = BlockTicks::new();
        target.schedule(AWAY_3, 3, 3, TickPriority::Normal);
        target.insert_chunk(boxed);
        assert_eq!(
            target.pending().map(|tick| tick.pos).collect::<Vec<_>>(),
            [POS, OTHER, AWAY_3]
        );
    }

    #[test]
    fn a_boxes_relative_delay_survives_a_different_clock() {
        let mut source = BlockTicks::new();
        source.set_time(100);
        source.schedule(POS, 7, 5, TickPriority::Normal);
        source.schedule_fluid(OTHER, 2, 6, TickPriority::High);
        let boxed = source.take_chunk(CHUNK_A).unwrap();
        assert_eq!(boxed.blocks[0].delay, 5);
        assert_eq!(boxed.fluids[0].delay, 6);

        // 目标表的时钟晚 900 刻，剩余延迟仍是 5 / 6。
        let mut target = BlockTicks::new();
        target.set_time(1000);
        target.insert_chunk(boxed);
        let blocks = block_keys(&target);
        let fluids = fluid_keys(&target);
        assert_eq!(blocks[0].trigger_tick, 1005);
        assert_eq!(fluids[0].trigger_tick, 1006);
        assert_eq!(all_due(&mut target, 1004), []);
        assert_eq!(all_due(&mut target, 1006), [(POS, 7), (OTHER, 2)]);
    }

    #[test]
    fn take_chunks_boxes_several_chunks_in_one_scan() {
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 1, 2, TickPriority::High);
        ticks.schedule(OTHER, 2, 2, TickPriority::Normal);
        ticks.schedule(AWAY, 3, 2, TickPriority::Low);
        ticks.schedule(AWAY_2, 4, 2, TickPriority::Low);
        ticks.schedule_fluid(POS, 5, 2, TickPriority::Normal);
        ticks.schedule_fluid(AWAY, 6, 2, TickPriority::Normal);
        let wanted: BTreeSet<ChunkPos> = [CHUNK_A, ChunkPos::new(1, 0), ChunkPos::new(9, 9)]
            .into_iter()
            .collect();

        let mut boxes = ticks.take_chunks(&wanted).unwrap();
        // 没有刻的区块不给键，也不给空盒子。
        assert_eq!(boxes.len(), 2);
        assert!(boxes.contains_key(&CHUNK_A));
        assert!(boxes.contains_key(&ChunkPos::new(1, 0)));
        assert_eq!(boxes.remove(&CHUNK_A).unwrap().blocks.len(), 2);
        let away = boxes.remove(&ChunkPos::new(1, 0)).unwrap();
        assert_eq!(away.blocks.len(), 2);
        assert_eq!(away.fluids.len(), 1);

        // 队列里什么都没剩下，别的区块的条目也一起被搬走了。
        assert_eq!(ticks.pending().count(), 0);
        assert_eq!(ticks.pending_fluids().count(), 0);
        assert!(!ticks.has_scheduled_tick(POS, 1));
        assert!(!ticks.has_scheduled_tick(AWAY, 3));

        // 空集合一次扫描都不做。
        assert!(ticks.take_chunks(&BTreeSet::new()).unwrap().is_empty());
    }

    /// 只读查询和 `take_chunks` 用的是同一个前提，tick 中途问一次就能整批不动。
    #[test]
    fn the_ready_query_tracks_due_ticks_running() {
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 1, 1, TickPriority::Normal);
        ticks.schedule_fluid(POS, 1, 1, TickPriority::Normal);
        assert!(ticks.ready_to_move_chunks());

        ticks.collect_due(1, &all_chunks_tick);
        assert!(!ticks.ready_to_move_chunks());
        assert!(ticks.next_due().is_some());
        assert!(ticks.ready_to_move_chunks());

        ticks.collect_due(1, &all_chunks_tick);
        ticks.collect_due_fluids(&all_chunks_tick);
        assert!(!ticks.ready_to_move_chunks());
        assert!(ticks.next_fluid_due().is_some());
        assert!(ticks.ready_to_move_chunks());

        let boxed = ticks.take_chunk(CHUNK_A).unwrap();
        ticks.insert_chunk(boxed);
        assert!(ticks.ready_to_move_chunks());
    }

    #[test]
    fn take_chunks_is_busy_while_due_ticks_are_running() {
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 1, 1, TickPriority::Normal);
        ticks.collect_due(1, &all_chunks_tick);
        assert!(matches!(
            ticks.take_chunks(&[CHUNK_A].into_iter().collect()),
            Err(Busy)
        ));
    }

    #[test]
    fn an_empty_box_round_trips() {
        let mut ticks = BlockTicks::new();
        let empty = ticks.take_chunk(CHUNK_A).unwrap();
        ticks.insert_chunk(empty);
        assert_eq!(ticks.pending().count(), 0);
        assert_eq!(ticks.pending_fluids().count(), 0);
        assert_eq!(drain(&mut ticks, 1), []);
    }

    #[test]
    fn taking_a_chunk_is_busy_while_due_ticks_are_running() {
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 1, 1, TickPriority::Normal);
        ticks.schedule(AWAY, 1, 1, TickPriority::Normal);
        ticks.collect_due(1, &all_chunks_tick);
        assert!(matches!(ticks.take_chunk(CHUNK_A), Err(Busy)));
        // 跑完这一批之后就能搬了。
        assert!(ticks.next_due().is_some());
        assert!(ticks.next_due().is_some());
        assert!(ticks.take_chunk(CHUNK_A).is_ok());

        let mut ticks = BlockTicks::new();
        ticks.schedule_fluid(POS, 1, 1, TickPriority::Normal);
        ticks.collect_due(1, &all_chunks_tick);
        ticks.collect_due_fluids(&all_chunks_tick);
        assert!(matches!(ticks.take_chunk(CHUNK_A), Err(Busy)));
        assert_eq!(ticks.next_fluid_due(), Some((POS, 1)));
        assert!(ticks.take_chunk(CHUNK_A).is_ok());
    }

    /// `collect_due` 的便捷包装：判断直接来自这一刻的冻结位图。
    fn drain_blocks(
        ticks: &mut BlockTicks,
        now: i64,
        frozen: &FrozenMask,
    ) -> Vec<(BlockPos, usize)> {
        ticks.collect_due(now, &frozen.check());
        std::iter::from_fn(|| ticks.next_due()).collect()
    }

    fn drain_all_fluids(ticks: &mut BlockTicks, frozen: &FrozenMask) -> Vec<(BlockPos, i32)> {
        ticks.collect_due_fluids(&frozen.check());
        std::iter::from_fn(|| ticks.next_fluid_due()).collect()
    }

    /// 冻结期间到期的刻不执行、不丢、不改到期时刻；恢复后按原来的排序键
    /// 和别的区块的刻合在一起执行，到期时刻早的先执行。
    #[test]
    fn frozen_chunks_defer_due_ticks_and_merge_them_back_in_order() {
        let frozen = FrozenMask::new(1 << 0);
        let mut ticks = BlockTicks::new();
        // 区块 A 冻结，B 能 tick。A 有 1 / 2 刻到期的刻，B 有 2 / 3 刻到期的刻。
        ticks.schedule(POS, 10, 1, TickPriority::Normal);
        ticks.schedule(OTHER, 11, 2, TickPriority::Normal);
        ticks.schedule(AWAY, 20, 2, TickPriority::High);
        ticks.schedule(AWAY_2, 21, 3, TickPriority::Normal);
        assert_eq!(drain_blocks(&mut ticks, 1, &frozen), []);
        assert_eq!(drain_blocks(&mut ticks, 2, &frozen), [(AWAY, 20)]);
        assert_eq!(drain_blocks(&mut ticks, 3, &frozen), [(AWAY_2, 21)]);
        // 搁置的刻仍然排着：has_scheduled_tick 为真，重复排刻被去重，
        // will_tick_this_tick 为假，pending 里还看得见、到期时刻不变。
        assert!(ticks.has_scheduled_tick(POS, 10));
        assert!(ticks.has_scheduled_tick(OTHER, 11));
        assert!(!ticks.will_tick_this_tick(POS, 10));
        assert!(!ticks.will_tick_this_tick(OTHER, 11));
        ticks.schedule(POS, 10, 1, TickPriority::ExtremelyHigh);
        assert_eq!(ticks.pending().count(), 2);
        assert_eq!(
            ticks
                .pending()
                .map(|tick| tick.trigger_tick)
                .collect::<Vec<_>>(),
            [1, 2]
        );

        // 恢复的那一刻，A 里 1 / 2 刻到期的两条和 B 里同一刻到期的一条合在一起，
        // 按（到期时刻，优先级，子序号）执行：旧的 1 刻先跑。
        ticks.schedule(AWAY_3, 30, 0, TickPriority::ExtremelyLow);
        frozen.set(0);
        assert_eq!(
            drain_blocks(&mut ticks, 3, &frozen),
            [(POS, 10), (OTHER, 11), (AWAY_3, 30)]
        );
        assert_eq!(ticks.pending().count(), 0);
    }

    /// 搁置表和主队列混在一起时，`pending()` 仍按（时刻，优先级，子序号）输出，
    /// 且是惰性的：只取开头几条不会碰后面的条目。
    #[test]
    fn pending_merges_deferred_and_main_queues_in_sort_order() {
        let frozen = FrozenMask::new(1 << 0);
        let mut ticks = BlockTicks::new();
        // 冻结区块 A 的 1 / 3 刻，区块 B 的 2 / 4 刻；分别有到期和未到期的。
        ticks.schedule(POS, 1, 1, TickPriority::Normal);
        ticks.schedule(OTHER, 2, 3, TickPriority::Normal);
        ticks.schedule(AWAY, 3, 2, TickPriority::Normal);
        ticks.schedule(AWAY_2, 4, 4, TickPriority::Normal);
        assert_eq!(drain_blocks(&mut ticks, 2, &frozen), [(AWAY, 3)]);
        // A 里的 1 到期被搁置、3 还在主队列；B 里的 4 也在主队列。
        let keys: Vec<_> = ticks
            .pending()
            .map(|tick| (tick.trigger_tick, tick.pos))
            .collect();
        assert_eq!(keys, [(1, POS), (3, OTHER), (4, AWAY_2)]);
        // 只取第一条也不用把整条队列排一遍。
        assert_eq!(
            ticks.pending().next().map(|tick| tick.trigger_tick),
            Some(1)
        );
    }

    /// 冻结期间新排的刻到期后也被搁置；流体有自己那张搁置表。
    #[test]
    fn ticks_scheduled_while_frozen_are_deferred_when_they_become_due() {
        let frozen = FrozenMask::new(1 << 0);
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 1, 1, TickPriority::Normal);
        assert_eq!(drain_blocks(&mut ticks, 1, &frozen), []);
        // 冻结期间排的新刻进主队列，到期时才被搁置。
        ticks.schedule(OTHER, 2, 1, TickPriority::High);
        ticks.schedule_fluid(POS, 3, 1, TickPriority::Normal);
        ticks.schedule_fluid(AWAY, 4, 1, TickPriority::Normal);
        assert_eq!(drain_blocks(&mut ticks, 2, &frozen), []);
        assert_eq!(drain_all_fluids(&mut ticks, &frozen), [(AWAY, 4)]);
        assert_eq!(ticks.pending().count(), 2);
        assert_eq!(ticks.pending_fluids().count(), 1);
        assert!(ticks.has_scheduled_tick(OTHER, 2));

        frozen.set(0);
        assert_eq!(drain_blocks(&mut ticks, 2, &frozen), [(POS, 1), (OTHER, 2)]);
        assert_eq!(drain_all_fluids(&mut ticks, &frozen), [(POS, 3)]);
        assert_eq!(ticks.pending().count(), 0);
        assert_eq!(ticks.pending_fluids().count(), 0);
    }

    /// 装盒把搁置的刻一起带走：相对延迟是负数或 0，排在这个区块其它刻前面，
    /// 彼此的相对顺序保持；放回后到期时如果区块又冻结，会再次被搁置。
    #[test]
    fn deferred_ticks_survive_the_chunk_box_and_keep_their_order() {
        let frozen = FrozenMask::new(1 << 0);
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 1, 1, TickPriority::Normal);
        ticks.schedule(OTHER, 2, 2, TickPriority::Normal);
        ticks.schedule_fluid(POS, 3, 1, TickPriority::Normal);
        // 1 刻到期时区块 A 冻结，POS 的方块刻/流体刻都被搁置；OTHER 还没到期。
        assert_eq!(drain_blocks(&mut ticks, 1, &frozen), []);
        assert_eq!(drain_all_fluids(&mut ticks, &frozen), []);
        assert!(!ticks.will_tick_this_tick(POS, 1));

        let boxed = ticks.take_chunk(CHUNK_A).unwrap();
        assert!(!ticks.has_scheduled_tick(POS, 1));
        assert_eq!(ticks.pending().count(), 0);
        assert_eq!(ticks.pending_fluids().count(), 0);
        // 盒子里四条刻：两条搁置的（延迟 0）在前，两条还没到期的（延迟 1）在后。
        assert_eq!(boxed.blocks.len(), 2);
        assert_eq!(boxed.fluids.len(), 1);
        assert_eq!(boxed.blocks[0].pos, POS);
        assert_eq!(boxed.blocks[0].delay, 0);
        assert_eq!(boxed.blocks[1].pos, OTHER);
        assert_eq!(boxed.blocks[1].delay, 1);

        // 放回时全部进主队列；区块还冻结，到期的两条再次被搁置。
        ticks.insert_chunk(boxed);
        assert_eq!(ticks.pending().count(), 2);
        assert_eq!(ticks.pending_fluids().count(), 1);
        assert_eq!(drain_blocks(&mut ticks, 2, &frozen), []);
        assert_eq!(drain_all_fluids(&mut ticks, &frozen), []);
        assert_eq!(ticks.pending().count(), 2);
        frozen.set(0);
        assert_eq!(drain_blocks(&mut ticks, 2, &frozen), [(POS, 1), (OTHER, 2)]);
        assert_eq!(drain_all_fluids(&mut ticks, &frozen), [(POS, 3)]);
    }

    /// 固定种子的 xorshift，随机测不用引入依赖。
    struct Rng(u64);

    impl Rng {
        fn new(seed: u64) -> Self {
            Self(seed)
        }

        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    fn random_priority(value: u64) -> TickPriority {
        match value % 7 {
            0 => TickPriority::ExtremelyHigh,
            1 => TickPriority::VeryHigh,
            2 => TickPriority::High,
            3 => TickPriority::Normal,
            4 => TickPriority::Low,
            5 => TickPriority::VeryLow,
            _ => TickPriority::ExtremelyLow,
        }
    }

    /// 一条随机排的刻，位置由下标唯一确定，保证每条都真的排上。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Entry {
        pos: BlockPos,
        kind: i64,
        delay: i32,
        priority: TickPriority,
        fluid: bool,
    }

    fn random_entries(rng: &mut Rng, count: usize) -> Vec<Entry> {
        (0..count)
            .map(|index| {
                let value = rng.next();
                // 下标本身保证位置唯一，4 个区块轮流用。
                let pos = BlockPos::new(
                    ((index as i32 % 4) * 16) + (index as i32 / 4 % 16),
                    (index / 64) as i32,
                    (index / 1024) as i32,
                );
                let fluid = index % 3 == 0;
                Entry {
                    pos,
                    kind: (1 + index % 5) as i64,
                    delay: (value >> 14 & 15) as i32,
                    priority: random_priority(value >> 18),
                    fluid,
                }
            })
            .collect()
    }

    fn schedule_entries(ticks: &mut BlockTicks, entries: &[Entry]) {
        for entry in entries {
            if entry.fluid {
                ticks.schedule_fluid(entry.pos, entry.kind as i32, entry.delay, entry.priority);
            } else {
                ticks.schedule(entry.pos, entry.kind as usize, entry.delay, entry.priority);
            }
        }
    }

    /// 随机排一批刻，随机取出放回若干区块，再把所有刻跑完。
    /// 性质：每条刻恰好跑一次，到期时刻不变；同一个区块内部的相对顺序不变；
    /// 和别的区块的刻之间只允许「放入的排在同（时刻，优先级）的刻前面」这一种差别。
    #[test]
    fn random_take_and_insert_keeps_every_tick_once_in_order() {
        const COUNT: usize = 3000;
        let entries = random_entries(&mut Rng::new(0x9e37_79b9_7f4a_7c15), COUNT);
        let mut untouched = BlockTicks::new();
        let mut moved = BlockTicks::new();
        schedule_entries(&mut untouched, &entries);
        schedule_entries(&mut moved, &entries);

        let mut rng = Rng::new(0x2545_f491_4f6c_dd1d);
        let mut moved_chunks = HashSet::new();
        for _ in 0..12 {
            let chunk = ChunkPos::new((rng.next() & 3) as i32, 0);
            let boxed = moved.take_chunk(chunk).unwrap();
            moved.insert_chunk(boxed);
            moved_chunks.insert(chunk);
        }

        assert_eq!(block_keys(&moved).len(), block_keys(&untouched).len());
        assert_eq!(fluid_keys(&moved).len(), fluid_keys(&untouched).len());

        // 逐刻跑到全部到期：两条队列都取干净。
        let mut left = Vec::new();
        let mut right = Vec::new();
        for now in 0..18 {
            left.extend(all_due(&mut untouched, now));
            right.extend(all_due(&mut moved, now));
        }
        assert_eq!(left.len(), COUNT);
        assert_eq!(right.len(), COUNT);

        // 每条刻按位置、种类、到期时刻对得上（位置唯一，没有同格查重的干扰）。
        let mut left_by_pos: HashMap<BlockPos, i64> = HashMap::new();
        let mut right_by_pos: HashMap<BlockPos, i64> = HashMap::new();
        for &(pos, kind) in &left {
            assert!(left_by_pos.insert(pos, kind).is_none());
        }
        for &(pos, kind) in &right {
            assert!(right_by_pos.insert(pos, kind).is_none());
        }
        assert_eq!(left_by_pos, right_by_pos);

        // 同一个被搬过的区块内部，相对顺序不变。
        for &chunk in &moved_chunks {
            let in_chunk = |run: &[(BlockPos, i64)]| {
                run.iter()
                    .filter(|&&(pos, _)| pos.chunk() == chunk)
                    .copied()
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                in_chunk(&left),
                in_chunk(&right),
                "区块 {chunk:?} 的内部顺序"
            );
        }

        // 没被搬过的区块，和所有其它区块一起在整条执行序列里的相对顺序不变。
        let untouched_chunks = |run: &[(BlockPos, i64)]| {
            run.iter()
                .filter(|&&(pos, _)| !moved_chunks.contains(&pos.chunk()))
                .copied()
                .collect::<Vec<_>>()
        };
        assert_eq!(untouched_chunks(&left), untouched_chunks(&right));

        // 剩下的差别只可能在同（时刻，优先级）的刻之间：按（时刻，优先级）给每条刻
        // 编号后，没搬过的序列和搬过的序列里这个编号序列必须完全相同。
        let by_entry: HashMap<BlockPos, &Entry> = entries.iter().map(|e| (e.pos, e)).collect();
        let key_sequence = |run: &[(BlockPos, i64)]| {
            run.iter()
                .map(|&(pos, _)| {
                    let entry = by_entry[&pos];
                    (entry.delay, entry.priority, entry.fluid)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(key_sequence(&left), key_sequence(&right));
    }

    /// 朴素的参照实现：每个区块一条按排序键排好的队列，每刻对能 tick 的区块
    /// 取出到期的刻，全局按排序键归并。方块和流体各跑一遍。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct RefTick {
        order: TickOrder,
        pos: BlockPos,
        kind: i64,
    }

    /// 一条队列的参照状态：按区块分好，每条留完整排序键。
    #[derive(Default)]
    struct RefQueue {
        by_chunk: BTreeMap<ChunkPos, Vec<RefTick>>,
    }

    impl RefQueue {
        fn schedule(&mut self, tick: RefTick) {
            self.by_chunk
                .entry(tick.pos.chunk())
                .or_default()
                .push(tick);
        }

        fn due(&mut self, now: i64, frozen: u32, out: &mut Vec<RefTick>) {
            for (&chunk, queue) in &mut self.by_chunk {
                // 冻结位图按 x 的位取，和 `Frozen::check` 的判定一致。
                if frozen & (1_u32 << chunk.x as u32) != 0 {
                    continue;
                }
                let mut rest = Vec::with_capacity(queue.len());
                for &tick in queue.iter() {
                    if tick.order.trigger_tick <= now {
                        out.push(tick);
                    } else {
                        rest.push(tick);
                    }
                }
                *queue = rest;
            }
            for queue in self.by_chunk.values_mut() {
                queue.sort_unstable_by_key(|tick| tick.order);
            }
        }

        fn is_empty(&self) -> bool {
            self.by_chunk.values().all(Vec::is_empty)
        }
    }

    /// 随机冻结 / 恢复四个区块、随机排刻、随机推进时间，逐刻和 [`BlockTicks`]
    /// 比较方块和流体的执行序列。种子固定，几秒内跑完。
    #[test]
    fn random_freeze_and_thaw_matches_a_naive_per_chunk_reference() {
        const CHUNKS: i32 = 4;
        const STEPS: usize = 900;
        let mut rng = Rng::new(0x51ed_270b_c1f4_a83d);
        let mut ticks = BlockTicks::new();
        let mut reference = [RefQueue::default(), RefQueue::default()];
        let frozen = FrozenMask::new(0);
        let mut now = 0_i64;

        for step in 0..STEPS {
            // 随机冻结或恢复一个区块；四个区块的冻结位落在低四位。
            let changed = (rng.next() % CHUNKS as u64) as i32;
            let bit = 1_u32 << changed;
            let mask = frozen.mask();
            frozen.set(if rng.next() & 1 == 0 {
                mask | bit
            } else {
                mask & !bit
            });
            // 每次都把时钟拨到 now 再排刻，这样代码里的 scheduled/参照里的 order 一致。
            ticks.set_time(now);

            // 每刻随机排若干条刻，位置由 (step, index) 唯一确定，避免同格查重。
            // y 限制在 0..16，x/z 都是非负的，所以 chunk.x 不会是负数。
            let count = (rng.next() % 4) as usize;
            for index in 0..count {
                let value = rng.next();
                let chunk = (value % CHUNKS as u64) as i32;
                let local = (step as i32 * 5 + index as i32) % 16;
                let pos = BlockPos::new(chunk * 16 + local, (step % 16) as i32, (step / 16) as i32);
                let fluid = value >> 3 & 1 == 1;
                let delay = (value >> 4 & 7) as i32;
                let priority = random_priority(value >> 8);
                let kind = (1 + (value >> 12 & 3)) as i64;
                let queue = usize::from(fluid);
                let before = ticks.next_order;
                if fluid {
                    ticks.schedule_fluid(pos, kind as i32, delay, priority);
                } else {
                    ticks.schedule(pos, kind as usize, delay, priority);
                }
                // 同格同种排队被去重时 `next_order` 不动，参照也别收这一条。
                if ticks.next_order != before {
                    let order = TickOrder {
                        trigger_tick: now.saturating_add(i64::from(delay)),
                        priority,
                        sub_tick_order: before as i64,
                        pos,
                    };
                    reference[queue].schedule(RefTick { order, pos, kind });
                }
            }

            // 参照：两条队列各自按 (时刻, 优先级, 子序号, 位置) 归并。
            let mask = frozen.mask();
            let mut expected = [Vec::new(), Vec::new()];
            reference[0].due(now, mask, &mut expected[0]);
            reference[1].due(now, mask, &mut expected[1]);
            expected[0].sort_unstable_by_key(|tick| tick.order);
            expected[1].sort_unstable_by_key(|tick| tick.order);

            // 被测：方块刻、流体刻各收一次，判断是同一份冻结位图。
            ticks.collect_due(now, &frozen.check());
            let mut ran_blocks = Vec::new();
            while let Some((pos, kind)) = ticks.next_due() {
                ran_blocks.push((pos, kind as i64));
            }
            ticks.collect_due_fluids(&frozen.check());
            let mut ran_fluids = Vec::new();
            while let Some((pos, kind)) = ticks.next_fluid_due() {
                ran_fluids.push((pos, i64::from(kind)));
            }

            let expected_blocks: Vec<_> = expected[0]
                .iter()
                .map(|tick| (tick.pos, tick.kind))
                .collect();
            let expected_fluids: Vec<_> = expected[1]
                .iter()
                .map(|tick| (tick.pos, tick.kind))
                .collect();
            assert_eq!(
                ran_blocks, expected_blocks,
                "第 {step} 刻的方块刻，now={now}"
            );
            assert_eq!(
                ran_fluids, expected_fluids,
                "第 {step} 刻的流体刻，now={now}"
            );
            now += (rng.next() % 3) as i64;
        }

        // 最后放开全部区块，跑到所有刻都执行完，两边一个不剩。
        frozen.set(0);
        let mut final_blocks = Vec::new();
        let mut final_fluids = Vec::new();
        for _ in 0..12 {
            let mut expected = [Vec::new(), Vec::new()];
            reference[0].due(now, 0, &mut expected[0]);
            reference[1].due(now, 0, &mut expected[1]);
            final_blocks.extend(expected[0].iter().map(|tick| (tick.pos, tick.kind)));
            final_fluids.extend(expected[1].iter().map(|tick| (tick.pos, tick.kind)));
            ticks.collect_due(now, &frozen.check());
            while ticks.next_due().is_some() {}
            ticks.collect_due_fluids(&frozen.check());
            while ticks.next_fluid_due().is_some() {}
            now += 1;
        }
        assert!(reference.iter().all(RefQueue::is_empty));
        assert_eq!(ticks.pending().count(), 0);
        assert_eq!(ticks.pending_fluids().count(), 0);
        assert!(!final_blocks.is_empty() && !final_fluids.is_empty());
    }

    /// 粗基准，用 `cargo test --release -p mc-bamboo-blocks ticks_bench -- --ignored --nocapture` 跑。
    /// 排满再跑完，以及每刻排一批跑一批；输入固定，改动前后对比耗时。
    #[test]
    #[ignore]
    fn ticks_bench() {
        use std::{
            hint::black_box,
            time::{Duration, Instant},
        };

        const TOTAL: usize = 1_000_000;
        const PER_TICK: usize = 1000;
        const TICKS: i64 = 1000;

        /// 固定种子的 xorshift，避免给这个 crate 加依赖。
        struct Rng(u64);

        impl Rng {
            fn next(&mut self) -> u64 {
                self.0 ^= self.0 << 13;
                self.0 ^= self.0 >> 7;
                self.0 ^= self.0 << 17;
                self.0
            }
        }

        fn priority(value: u64) -> TickPriority {
            match value % 7 {
                0 => TickPriority::ExtremelyHigh,
                1 => TickPriority::VeryHigh,
                2 => TickPriority::High,
                3 => TickPriority::Normal,
                4 => TickPriority::Low,
                5 => TickPriority::VeryLow,
                _ => TickPriority::ExtremelyLow,
            }
        }

        fn schedule(ticks: &mut BlockTicks, rng: &mut Rng, first: usize, count: usize) {
            for index in first..first + count {
                let value = rng.next();
                let pos = BlockPos::new(
                    (value & 0xffff) as i32 - 32768,
                    (value >> 16 & 0xff) as i32,
                    (value >> 24 & 0xffff) as i32 - 32768,
                );
                ticks.schedule(
                    pos,
                    (value >> 40 & 0xff) as usize,
                    (index % 17) as i32,
                    priority(value >> 48),
                );
            }
        }

        fn run_due(ticks: &mut BlockTicks, now: i64) -> usize {
            ticks.collect_due(now, &all_chunks_tick);
            let mut ran = 0;
            while let Some((pos, block)) = ticks.next_due() {
                black_box((pos, block));
                ran += 1;
            }
            ran
        }

        fn measure(mut run: impl FnMut() -> usize) -> (Duration, usize) {
            let warmup = run();
            let mut best = Duration::MAX;
            let mut count = 0;
            for _ in 0..5 {
                let started = Instant::now();
                count = run();
                best = best.min(started.elapsed());
            }
            assert_eq!(warmup, count);
            (best, count)
        }

        let (batch_time, batch_count) = measure(|| {
            let mut ticks = BlockTicks::new();
            let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
            schedule(&mut ticks, &mut rng, 0, TOTAL);
            let mut ran = 0;
            for now in 0..17 {
                ran += run_due(&mut ticks, now);
            }
            assert_eq!(ran, TOTAL, "每条刻都应该恰好跑一次");
            ran
        });

        let (rolling_time, rolling_count) = measure(|| {
            let mut ticks = BlockTicks::new();
            let mut rng = Rng(0x2545_f491_4f6c_dd1d);
            let mut ran = 0;
            for now in 0..TICKS {
                schedule(&mut ticks, &mut rng, (now as usize) * PER_TICK, PER_TICK);
                ran += run_due(&mut ticks, now);
            }
            for now in TICKS..TICKS + 17 {
                ran += run_due(&mut ticks, now);
            }
            assert_eq!(ran, TOTAL, "每条刻都应该恰好跑一次");
            ran
        });

        println!(
            "batch:   {batch_count} ticks, best of 5: {:.3} ms ({:.0} ns/tick)",
            batch_time.as_secs_f64() * 1000.0,
            batch_time.as_nanos() as f64 / batch_count as f64
        );
        println!(
            "rolling: {rolling_count} ticks, best of 5: {:.3} ms ({:.0} ns/tick)",
            rolling_time.as_secs_f64() * 1000.0,
            rolling_time.as_nanos() as f64 / rolling_count as f64
        );

        /// 队列里有 `count` 条刻时，一次取出 16 个区块的耗时。
        fn take_16(count: usize) -> Duration {
            let chunks: BTreeSet<ChunkPos> = (0..16)
                .map(|index| ChunkPos::new(index % 4, index / 4))
                .collect();
            let mut best = Duration::MAX;
            for _ in 0..5 {
                let mut ticks = BlockTicks::new();
                let mut rng = Rng(0x6a09_e667_f3bc_c909);
                // 位置由下标唯一确定，均匀落在 4×4 个区块里，避免同格查重吞掉条目。
                for index in 0..count {
                    let value = rng.next();
                    let index = index as i32;
                    let pos = BlockPos::new(index & 63, (index >> 12) & 255, (index >> 6) & 63);
                    ticks.schedule(
                        pos,
                        (value >> 40 & 0xff) as usize,
                        20,
                        priority(value >> 48),
                    );
                }
                let started = Instant::now();
                let boxes = black_box(ticks.take_chunks(&chunks).unwrap());
                best = best.min(started.elapsed());
                assert_eq!(boxes.len(), 16);
            }
            best
        }

        let take_100k = take_16(100_000);
        let take_1m = take_16(1_000_000);
        println!(
            "take_chunks 16 chunks, queue 100k: {:.3} ms",
            take_100k.as_secs_f64() * 1000.0
        );
        println!(
            "take_chunks 16 chunks, queue   1M: {:.3} ms",
            take_1m.as_secs_f64() * 1000.0
        );
    }
}
