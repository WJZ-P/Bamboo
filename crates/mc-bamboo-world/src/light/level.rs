//! 两层光照一起算一轮，官方的 `LevelLightEngine`；和给别人读的那一份光照，官方的 `visibleSectionData`。
//!
//! 一轮先执行它的任务，再先方块光后天空光地传播到稳定。传播可以停在任意一步，下次接着做：
//! 中间没有别人改引擎的数据，方块也读这一轮排好那一刻的样子，所以和一口气做完的结果相同。
//! 一轮做完才把变过的段抄进给别人读的那一份，算到一半时读到的还是上一轮的结果。

use std::collections::BTreeMap;

use mc_bamboo_core::{ChunkPos, LightLayer};

use super::{
    Chunks, LightChanges, MIN_LIGHT_SECTION, SectionPos, engine::LightEngine, queue::Task,
    storage::LightStorage,
};
use crate::chunk::Chunk;

/// 天空光和方块光两个引擎。
#[derive(Debug, Clone)]
pub(super) struct Engines {
    pub(super) sky: LightEngine,
    pub(super) block: LightEngine,
    /// 上次取走以后光照变了的区块和段。
    changes: BTreeMap<ChunkPos, LightChanges>,
    /// 正在算的一轮算到了哪一层。`None` 表示没有算到一半的轮次。
    round: Option<LightLayer>,
}

impl Engines {
    pub(super) fn new(sky: LightStorage, block: LightStorage) -> Self {
        Self {
            sky: LightEngine::new(sky),
            block: LightEngine::new(block),
            changes: BTreeMap::new(),
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
        std::mem::take(&mut self.changes)
    }

    /// `onLightUpdate`。
    fn collect_changes(&mut self, layer: LightLayer) {
        let engine = match layer {
            LightLayer::Sky => &mut self.sky,
            LightLayer::Block => &mut self.block,
        };
        for section in engine.storage.take_affected() {
            self.changes
                .entry(section.chunk())
                .or_default()
                .mark(layer, section.y);
        }
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

    pub(super) fn storage(&self, layer: LightLayer) -> &LightStorage {
        match layer {
            LightLayer::Sky => &self.sky,
            LightLayer::Block => &self.block,
        }
    }
}
