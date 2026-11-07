//! 计划刻队列，对应官方的 `LevelTicks`。方块和流体各有一条，`ServerLevel.tick` 先跑方块的，再跑流体的。
//!
//! 官方按区块分容器，这里的世界不卸载区块，一个全局队列的执行顺序和官方相同：
//! 先比到期时间，再比优先级，最后比加入的先后。

use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    hash::Hash,
};

use mc_bamboo_core::BlockPos;

/// `ServerLevel.tick` 给每条队列的上限：一刻最多跑这么多个，剩下的留到下一刻。
const MAX_TICKS_PER_TICK: usize = 65_536;

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

/// 队列的排序键，`ScheduledTick.DRAIN_ORDER`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TickOrder {
    trigger_tick: i64,
    priority: TickPriority,
    sub_tick_order: u64,
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

/// 一条队列，`T` 是方块或者流体的注册表 ID。
#[derive(Debug)]
struct TickQueue<T> {
    queue: BTreeMap<TickOrder, (BlockPos, T)>,
    /// 同一格同一种排队时只留第一个，`LevelChunkTicks.ticksPerPosition`。
    scheduled: HashSet<(BlockPos, T)>,
    /// 这一刻已经取出、还没有执行的计划刻，`willTickThisTick` 看的就是它们。
    running: VecDeque<(BlockPos, T)>,
}

impl<T> Default for TickQueue<T> {
    fn default() -> Self {
        Self {
            queue: BTreeMap::new(),
            scheduled: HashSet::new(),
            running: VecDeque::new(),
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

    fn pending(&self) -> impl Iterator<Item = (&TickOrder, BlockPos, T)> {
        self.queue
            .iter()
            .map(|(order, &(pos, kind))| (order, pos, kind))
    }

    /// `LevelTicks.collectTicks`：把到期的计划刻取出来等着执行，之后排的要等下一刻。
    fn collect_due(&mut self, now: i64) {
        while self.running.len() < MAX_TICKS_PER_TICK
            && let Some(entry) = self.queue.first_entry()
            && entry.key().trigger_tick <= now
        {
            let (pos, kind) = entry.remove();
            self.scheduled.remove(&(pos, kind));
            self.running.push_back((pos, kind));
        }
    }

    fn next_due(&mut self) -> Option<(BlockPos, T)> {
        self.running.pop_front()
    }
}

/// 全部排队的计划刻：方块一条队列，流体一条队列。
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

    /// 现在的 `gameTime`。
    pub const fn now(&self) -> i64 {
        self.now
    }

    fn order(&mut self, delay: i32, priority: TickPriority) -> TickOrder {
        let order = TickOrder {
            trigger_tick: self.now + i64::from(delay),
            priority,
            sub_tick_order: self.next_order,
        };
        self.next_order += 1;
        order
    }

    /// 给方块排一个刻。这一格这个方块已经排着时什么也不做。
    pub fn schedule(&mut self, pos: BlockPos, block: usize, delay: i32, priority: TickPriority) {
        if !self.blocks.has_scheduled(pos, block) {
            let order = self.order(delay, priority);
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
            let order = self.order(delay, priority);
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

    /// 全部排队的方块计划刻，按执行顺序。oracle 用它比较结果。
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

    /// 全部排队的流体计划刻，按执行顺序。
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
    pub(crate) fn collect_due(&mut self, now: i64) {
        self.now = now;
        self.blocks.collect_due(now);
    }

    /// 下一个要执行的方块计划刻。
    pub(crate) fn next_due(&mut self) -> Option<(BlockPos, usize)> {
        self.blocks.next_due()
    }

    /// 方块的刻都跑完以后，取出这一刻到期的流体计划刻。
    pub(crate) fn collect_due_fluids(&mut self) {
        self.fluids.collect_due(self.now);
    }

    /// 下一个要执行的流体计划刻。
    pub(crate) fn next_fluid_due(&mut self) -> Option<(BlockPos, i32)> {
        self.fluids.next_due()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POS: BlockPos = BlockPos::new(1, 2, 3);
    const OTHER: BlockPos = BlockPos::new(4, 5, 6);

    fn drain(ticks: &mut BlockTicks, now: i64) -> Vec<(BlockPos, usize)> {
        ticks.collect_due(now);
        std::iter::from_fn(|| ticks.next_due()).collect()
    }

    fn drain_fluids(ticks: &mut BlockTicks, now: i64) -> Vec<(BlockPos, i32)> {
        ticks.collect_due(now);
        ticks.collect_due_fluids();
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
        ticks.collect_due(1);
        assert_eq!(ticks.next_due(), Some((POS, 1)));
        assert!(!ticks.will_tick_this_tick(POS, 1));
        assert!(ticks.will_tick_this_tick(OTHER, 1));
    }

    #[test]
    fn ticks_scheduled_while_running_wait_for_the_next_collection() {
        let mut ticks = BlockTicks::new();
        ticks.schedule(POS, 1, 1, TickPriority::Normal);
        ticks.collect_due(1);
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
}
