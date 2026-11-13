//! 光照的任务队列，官方 `ThreadedLevelLightEngine` 的 `lightTasks`。
//!
//! 改方块只是把任务记下来。任务攒够 1000 个，或者到了两个 tick 之间，就排成一轮；算一轮时先执行它的任务，
//! 再把光照传播到稳定，算完才换上新的光照。官方在光照线程上一轮轮地算，主线程从不等它。
//! 这里在世界线程上算，每 tick 只做有限的工作量，一轮也可以停在半路，剩下的留到下一 tick：
//! 一次改很多方块时光照晚几 tick 才出来，tick 却不会被拖住。
//!
//! 一轮读到的方块是它排好那一刻的样子，和光照线程一直跟得上时一样，所以晚算不改变结果。
//! 前面还有没算完的轮次时改方块，先给这个区块留一份改之前的样子，算到哪一轮就把那一轮排好之前的改动补进去。
//!
//! 以后要搬到单独的光照线程上，形状不用变：世界线程按顺序发出任务、方块改动和排轮次这三种消息，
//! 光照线程用自己的区块副本补上改动再算，算完把变过的段发回来，世界线程抄进给别人读的那一份。

use std::{collections::VecDeque, mem};

use mc_bamboo_core::BlockPos;

use super::{Chunks, SectionPos, TASKS_PER_BATCH, write_block};
use crate::chunk::Chunk;

/// 一个光照任务。两种任务都不读方块。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Task {
    /// `checkBlock`：这一格的光照性质变了。
    CheckBlock(BlockPos),
    /// `updateSectionStatus`：这个区块段变空了（`true`），或者从空的变得有方块了。
    SectionStatus(SectionPos, bool),
}

/// 排好的一轮，官方 `runUpdate` 一次拿走的任务。
#[derive(Debug, Clone, Default)]
struct Round {
    /// 攒这一轮的时候改的方块。只在前面还有排着的轮次时才记，算这一轮之前补进副本。
    blocks: Vec<(BlockPos, u32)>,
    tasks: Vec<Task>,
}

/// 排着还没算的光照任务。
#[derive(Debug, Clone, Default)]
pub(super) struct TaskQueue {
    /// 排好了还没开始算的轮次，先排的在前。
    rounds: VecDeque<Round>,
    /// 有一轮开始算了还没算完。
    running: bool,
    /// 正在攒的下一轮。
    forming: Round,
    /// 前面还有没算完的轮次时改过的区块，内容是正在算的那一轮排好那一刻的样子。光照追上以后清空。
    past: crate::store::ChunkMap<Chunk>,
}

impl TaskQueue {
    /// `addTask`：攒够一批就排成一轮。
    pub(super) fn push(&mut self, task: Task) {
        self.forming.tasks.push(task);
        if self.forming.tasks.len() >= TASKS_PER_BATCH {
            self.close_round();
        }
    }

    /// `pos` 要换成 `state` 了，`chunk` 是它所在的区块改之前的样子。
    ///
    /// 没算完的轮次不能看到这次改动：先留一份这个区块，改动记在正在攒的那一轮上，算到那一轮时再补进去。
    pub(super) fn block_changing(&mut self, chunk: &Chunk, pos: BlockPos, state: u32) {
        if !self.running && self.rounds.is_empty() {
            return;
        }
        self.past
            .entry(pos.chunk())
            .or_insert_with(|| chunk.clone());
        self.forming.blocks.push((pos, state));
    }

    /// `tryScheduleUpdate`：到了两个 tick 之间，剩下的任务排成一轮。
    ///
    /// 官方没有任务时还看 `hasLightWork`。这里的任务要到算的时候才执行，一轮算完引擎里不会剩下工作，
    /// 所以只看任务。
    pub(super) fn schedule(&mut self) {
        if !self.forming.tasks.is_empty() {
            self.close_round();
        }
    }

    fn close_round(&mut self) {
        self.rounds.push_back(mem::take(&mut self.forming));
    }

    /// 还有没算完的任务：算到一半的、排好的和正在攒的都算。
    pub(super) fn has_work(&self) -> bool {
        self.running || !self.rounds.is_empty() || !self.forming.tasks.is_empty()
    }

    /// 开始算排好的下一轮：先把它排好之前的方块改动补进副本，返回它的任务。没有排好的轮次时返回 `None`。
    pub(super) fn start_round(&mut self) -> Option<Vec<Task>> {
        let round = self.rounds.pop_front()?;
        for (pos, state) in round.blocks {
            let chunk = self
                .past
                .get_mut(&pos.chunk())
                .expect("记下改动之前先留了这个区块");
            write_block(chunk, pos, state);
        }
        self.running = true;
        Some(round.tasks)
    }

    /// 正在算的这一轮该读的世界。
    pub(super) fn view<'a>(&'a self, chunks: Chunks<'a>) -> Chunks<'a> {
        chunks.with_past(&self.past)
    }

    /// 正在算的这一轮算完了。
    pub(super) fn finish_round(&mut self) {
        self.running = false;
        if self.rounds.is_empty() {
            // 追上了：之后排的轮次在排好时就读世界本身，留着的副本和改动都用不上了。
            self.past.clear();
            self.forming.blocks.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::blocks;

    use super::*;
    use crate::flat;

    const POS: BlockPos = BlockPos::new(3, -60, 3);

    fn check(index: i32) -> Task {
        Task::CheckBlock(BlockPos::new(index, -60, 0))
    }

    #[test]
    fn tasks_form_a_round_every_thousand_and_the_rest_waits_for_the_schedule() {
        let mut queue = TaskQueue::default();
        for index in 0..2500 {
            queue.push(check(index));
        }
        assert_eq!(queue.rounds.len(), 2);
        assert_eq!(queue.forming.tasks.len(), 500);

        queue.schedule();
        let mut sizes = Vec::new();
        while let Some(tasks) = queue.start_round() {
            sizes.push(tasks.len());
            queue.finish_round();
        }
        assert_eq!(sizes, [1000, 1000, 500]);
        assert!(!queue.has_work());
    }

    #[test]
    fn changes_are_kept_only_while_a_round_is_not_finished() {
        let template = flat::generate_chunk();
        let mut queue = TaskQueue::default();
        queue.block_changing(&template, POS, blocks::STONE);
        assert!(queue.past.is_empty());
        assert!(queue.forming.blocks.is_empty());

        queue.push(check(0));
        queue.schedule();
        queue.start_round();
        // 开始算了还没算完，照样要留。
        queue.block_changing(&template, POS, blocks::STONE);
        assert!(queue.past.contains_key(&POS.chunk()));
        assert_eq!(queue.forming.blocks, [(POS, blocks::STONE)]);
        assert!(queue.has_work());
    }

    #[test]
    fn a_late_round_reads_the_blocks_from_when_it_was_formed() {
        let template = flat::generate_chunk();
        let mut queue = TaskQueue::default();
        queue.push(check(0));
        queue.schedule();
        queue.block_changing(&template, POS, blocks::STONE);
        queue.push(check(1));
        queue.schedule();

        let mut edited = crate::store::ChunkMap::default();
        edited.insert(POS.chunk(), {
            let mut chunk = template.clone();
            write_block(&mut chunk, POS, blocks::STONE);
            chunk
        });
        let mut seen = Vec::new();
        while queue.start_round().is_some() {
            seen.push(queue.view(Chunks::new(&template, &edited)).state(POS));
            queue.finish_round();
        }
        assert_eq!(seen, [blocks::AIR, blocks::STONE]);
        // 追上以后副本没了，之后直接读世界。
        assert!(queue.past.is_empty());
        assert!(queue.forming.blocks.is_empty());
    }
}
