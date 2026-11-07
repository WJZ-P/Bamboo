//! 计划刻队列，对应官方的 `LevelTicks`。
//!
//! 官方按区块分容器，这里的世界不卸载区块，一个全局队列的执行顺序和官方相同：
//! 先比到期时间，再比优先级，最后比加入的先后。

use std::collections::{BTreeMap, HashSet, VecDeque};

use mc_bamboo_core::BlockPos;

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

/// 排队的一个刻。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledTick {
    pub pos: BlockPos,
    /// 方块的注册表 ID。到期时这一格换成了别的方块，这个刻就不执行。
    pub block: usize,
    pub trigger_tick: i64,
    pub priority: TickPriority,
}

/// 全部排队的计划刻。
#[derive(Debug, Default)]
pub struct BlockTicks {
    /// 现在的 `gameTime`，由 [`run_scheduled_ticks`](crate::run_scheduled_ticks) 更新。
    now: i64,
    next_order: u64,
    queue: BTreeMap<TickOrder, (BlockPos, usize)>,
    /// 同一格同一方块排队时只留第一个，`LevelChunkTicks.ticksPerPosition`。
    scheduled: HashSet<(BlockPos, usize)>,
    /// 这一刻已经取出、还没有执行的计划刻，`willTickThisTick` 看的就是它们。
    running: VecDeque<(BlockPos, usize)>,
}

impl BlockTicks {
    pub fn new() -> Self {
        Self::default()
    }

    /// 现在的 `gameTime`。
    pub const fn now(&self) -> i64 {
        self.now
    }

    /// 排一个刻。这一格这个方块已经排着时什么也不做。
    pub fn schedule(&mut self, pos: BlockPos, block: usize, delay: i32, priority: TickPriority) {
        if !self.scheduled.insert((pos, block)) {
            return;
        }
        let order = TickOrder {
            trigger_tick: self.now + i64::from(delay),
            priority,
            sub_tick_order: self.next_order,
        };
        self.next_order += 1;
        self.queue.insert(order, (pos, block));
    }

    /// `hasScheduledTick`。
    pub fn has_scheduled_tick(&self, pos: BlockPos, block: usize) -> bool {
        self.scheduled.contains(&(pos, block))
    }

    /// `willTickThisTick`：已经到期、这一刻还没轮到的计划刻。
    pub fn will_tick_this_tick(&self, pos: BlockPos, block: usize) -> bool {
        self.running.contains(&(pos, block))
    }

    /// 全部排队的计划刻，按执行顺序。oracle 用它比较结果。
    pub fn pending(&self) -> impl Iterator<Item = ScheduledTick> {
        self.queue
            .iter()
            .map(|(order, &(pos, block))| ScheduledTick {
                pos,
                block,
                trigger_tick: order.trigger_tick,
                priority: order.priority,
            })
    }

    /// `LevelTicks.collectTicks`：把到期的计划刻取出来等着执行，之后排的要等下一刻。
    pub(crate) fn collect_due(&mut self, now: i64) {
        self.now = now;
        while let Some(entry) = self.queue.first_entry() {
            if entry.key().trigger_tick > now {
                break;
            }
            let (pos, block) = entry.remove();
            self.scheduled.remove(&(pos, block));
            self.running.push_back((pos, block));
        }
    }

    /// 下一个要执行的计划刻。
    pub(crate) fn next_due(&mut self) -> Option<(BlockPos, usize)> {
        self.running.pop_front()
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
}
