//! 两层光照一起算一轮，官方的 `LevelLightEngine`；和给别人读的那一份光照，官方的 `visibleSectionData`。
//!
//! 一轮先执行它的任务，再先方块光后天空光地传播到稳定。传播可以停在任意一步，下次接着做：
//! 中间没有别人改引擎的数据，方块也读这一轮排好那一刻的样子，所以和一口气做完的结果相同。
//! 一轮做完才把变过的段抄进给别人读的那一份，算到一半时读到的还是上一轮的结果。

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_core::{ChunkPos, LightLayer};

use super::{
    Chunks, LightChanges, MIN_LIGHT_SECTION, SectionPos,
    engine::LightEngine,
    queue::Task,
    storage::{LightColumn, LightStorage},
};
use crate::chunk::Chunk;

/// 天空光和方块光两个引擎。
#[derive(Debug, Clone)]
pub(super) struct Engines {
    pub(super) sky: LightEngine,
    pub(super) block: LightEngine,
    /// 上次取走以后光照变了的区块和段。
    changes: Changes,
    /// 正在算的一轮算到了哪一层。`None` 表示没有算到一半的轮次。
    round: Option<LightLayer>,
}

impl Engines {
    pub(super) fn new(sky: LightStorage, block: LightStorage) -> Self {
        Self {
            sky: LightEngine::new(sky),
            block: LightEngine::new(block),
            changes: Changes(BTreeMap::new()),
            round: None,
        }
    }

    /// `initializeLight` 的前半：登记区块里有方块的段。
    pub(super) fn initialize_chunk(&mut self, chunk: &Chunk, pos: ChunkPos) {
        for (index, section) in chunk.sections().iter().enumerate() {
            if !section.is_empty() {
                let section_pos =
                    SectionPos::new(pos.x, MIN_LIGHT_SECTION + 1 + index as i32, pos.z);
                self.block.storage.update_section_status(section_pos, false);
                self.sky.storage.update_section_status(section_pos, false);
            }
        }
    }

    /// `runLightUpdates`，一口气算完。算模板时用。
    pub(super) fn run_light_updates(&mut self, chunks: Chunks<'_>) {
        let mut work = 0;
        self.block.run_light_updates(chunks, &mut work, usize::MAX);
        self.sky.run_light_updates(chunks, &mut work, usize::MAX);
    }

    /// 有一轮算到一半。
    pub(super) fn in_round(&self) -> bool {
        self.round.is_some()
    }

    /// `runUpdate` 的前半：按顺序执行这一轮的任务。返回任务数。
    pub(super) fn start_round(&mut self, tasks: Vec<Task>) -> usize {
        let count = tasks.len();
        for task in tasks {
            match task {
                Task::CheckBlock(pos) => {
                    self.block.check_block(pos);
                    self.sky.check_block(pos);
                }
                Task::SectionStatus(section, empty) => {
                    self.block.storage.update_section_status(section, empty);
                    self.sky.storage.update_section_status(section, empty);
                }
            }
        }
        self.round = Some(LightLayer::Block);
        count
    }

    /// `runUpdate` 的后半：接着传播，先方块光再天空光，`work` 到了 `budget` 就停。这一轮算完时返回 `true`。
    pub(super) fn continue_round(
        &mut self,
        chunks: Chunks<'_>,
        work: &mut usize,
        budget: usize,
    ) -> bool {
        if self.round == Some(LightLayer::Block) {
            if !self.block.run_light_updates(chunks, work, budget) {
                return false;
            }
            self.round = Some(LightLayer::Sky);
        }
        if !self.sky.run_light_updates(chunks, work, budget) {
            return false;
        }
        self.round = None;
        // 排轮次时只看任务，靠的就是这一条。
        debug_assert!(
            !self.block.has_light_work() && !self.sky.has_light_work(),
            "一轮算完，引擎里不该剩下工作"
        );
        true
    }

    /// `swapSectionMap`：一轮算完，记下光照变了的段，再把变过的数据抄进 `visible`。
    pub(super) fn publish(&mut self, visible: &mut Visible) {
        self.collect_changes(LightLayer::Block);
        self.collect_changes(LightLayer::Sky);
        self.block.storage.publish(&mut visible.block);
        self.sky.storage.publish(&mut visible.sky);
    }

    /// 上次取走以后光照变了的区块和段。
    pub(super) fn take_changes(&mut self) -> BTreeMap<ChunkPos, LightChanges> {
        self.changes.take()
    }

    /// 取出一个区块柱在引擎里的天空光和方块光。
    /// 只在光照全部算完时调用，这时引擎和 [`Visible`] 里的两份内容相同。
    pub(super) fn take_column(&mut self, pos: ChunkPos) -> [LightColumn; 2] {
        debug_assert!(!self.in_round(), "一轮算到一半不能搬区块柱");
        debug_assert!(
            !self.block.has_light_work() && !self.sky.has_light_work(),
            "引擎里不该剩下工作"
        );
        let columns = [
            self.sky.storage.take_column(pos),
            self.block.storage.take_column(pos),
        ];
        self.changes.clear(pos);
        columns
    }

    /// 把一个区块柱的天空光和方块光放进引擎。
    pub(super) fn insert_column(
        &mut self,
        pos: ChunkPos,
        [sky, block]: [LightColumn; 2],
        visible: &mut Visible,
    ) {
        self.sky.storage.insert_column(pos, &sky);
        self.block.storage.insert_column(pos, &block);
        visible.sky.insert_column(pos, &sky);
        visible.block.insert_column(pos, &block);
        // 放进来的内容是已经算好的，不产生发给客户端的变化。
        self.changes.clear(pos);
    }

    /// 搬区块柱之后重算引擎这份的 `sectionStates`。邻居柱里补建了数据层的段记成
    /// 要发给客户端的变化：正在看那些区块的玩家手里少这一段。被搬的那一柱自己不记，
    /// 它整个随区块包发出去。返回天空光和方块光各自补建了数据层的段。
    pub(super) fn rebind_area(
        &mut self,
        pos: ChunkPos,
        chunks: super::Chunks<'_>,
    ) -> [Vec<SectionPos>; 2] {
        let sky = self.sky.storage.rebind_area(pos, chunks);
        let block = self.block.storage.rebind_area(pos, chunks);
        for (layer, sections) in [(LightLayer::Sky, &sky), (LightLayer::Block, &block)] {
            self.changes.mark(
                layer,
                sections
                    .iter()
                    .copied()
                    .filter(|section| section.chunk() != pos)
                    .collect(),
            );
        }
        [sky, block]
    }

    /// `onLightUpdate`。
    fn collect_changes(&mut self, layer: LightLayer) {
        let engine = match layer {
            LightLayer::Sky => &mut self.sky,
            LightLayer::Block => &mut self.block,
        };
        self.changes.mark(layer, engine.storage.take_affected());
    }
}

/// 还没发给客户端的光照变化，按区块记下变过的段。
#[derive(Debug, Clone)]
struct Changes(BTreeMap<ChunkPos, LightChanges>);

impl Changes {
    fn mark(&mut self, layer: LightLayer, sections: BTreeSet<SectionPos>) {
        for section in sections {
            self.0
                .entry(section.chunk())
                .or_default()
                .mark(layer, section.y);
        }
    }

    /// 搬走或者整个换掉一个区块时，它还没发出去的变化不用再发了。
    fn clear(&mut self, pos: ChunkPos) {
        self.0.remove(&pos);
    }

    fn take(&mut self) -> BTreeMap<ChunkPos, LightChanges> {
        std::mem::take(&mut self.0)
    }
}

/// 给别人读的光照：方块规则、区块包和 `light_update` 都读这一份。
#[derive(Debug, Clone)]
pub(super) struct Visible {
    sky: LightStorage,
    block: LightStorage,
}

impl Visible {
    pub(super) fn new(sky: LightStorage, block: LightStorage) -> Self {
        Self { sky, block }
    }

    /// 区块束搬进/搬出之后重算别人读的那份里的 `sectionStates`，补建的数据层和引擎那份相同。
    /// 返回天空光和方块光各自补建了数据层的段。
    pub(super) fn rebind_area(
        &mut self,
        pos: ChunkPos,
        chunks: super::Chunks<'_>,
    ) -> [Vec<SectionPos>; 2] {
        [
            self.sky.rebind_area(pos, chunks),
            self.block.rebind_area(pos, chunks),
        ]
    }

    /// 取出一个区块柱在给别人读的那份里的天空光和方块光。
    pub(super) fn take_column(&mut self, pos: ChunkPos) -> [LightColumn; 2] {
        [self.sky.take_column(pos), self.block.take_column(pos)]
    }

    pub(super) fn storage(&self, layer: LightLayer) -> &LightStorage {
        match layer {
            LightLayer::Sky => &self.sky,
            LightLayer::Block => &self.block,
        }
    }
}
