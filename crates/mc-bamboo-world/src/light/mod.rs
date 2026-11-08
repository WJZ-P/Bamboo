//! 光照，官方的 `LevelLightEngine`、`ThreadedLevelLightEngine` 和它们下面的整套光照引擎。
//!
//! 天空光和方块光各有一个引擎和一份按区块段存的数据，比世界多上下各一段。
//! 改方块时记下要检查的格子和变空、变满的区块段，攒到一起算一轮：官方在另一个线程上做，
//! 每批最多 1000 个任务；这里在世界线程上同步做，分批的规则相同。
//!
//! 每个区块都已经生成并且算过光照，没改过的区块柱的光照和模板一样，见 `LevelLight::for_template`。

mod block;
mod data_layer;
mod engine;
mod properties;
mod sky;
pub(crate) mod sources;
mod storage;

use std::collections::{BTreeMap, HashMap};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};

pub use data_layer::{DataLayer, LAYER_BYTES};
use engine::LightEngine;
pub use properties::has_different_light_properties;
pub use sources::{NEGATIVE_INFINITY, SkyLightSources};
use storage::{LightColumn, LightStorage};

use crate::{MIN_Y, SECTION_COUNT, chunk::Chunk};

/// 光照比区块段多两段：世界底部下面和顶部上面各一段。
pub const LIGHT_SECTION_COUNT: usize = SECTION_COUNT + 2;
/// 最低的光照段，`getMinLightSection`。
pub const MIN_LIGHT_SECTION: i32 = (MIN_Y >> 4) - 1;
/// `ThreadedLevelLightEngine` 一批最多跑这么多个任务，攒够了就马上跑一批。
const TASKS_PER_BATCH: usize = 1000;
/// 算模板时，模板区块四周各铺这么多个区块。光最多走 15 格，中间那个区块就和无穷大的世界里一样了。
const TEMPLATE_RADIUS: i32 = 2;

/// 区块段坐标，官方的 `SectionPos`。`y` 也是以段为单位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SectionPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl SectionPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// 包含这个方块的段。
    pub const fn of(pos: BlockPos) -> Self {
        Self::new(pos.x >> 4, pos.y >> 4, pos.z >> 4)
    }

    /// 这一段所在的区块。
    pub const fn chunk(self) -> ChunkPos {
        ChunkPos::new(self.x, self.z)
    }

    pub const fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }

    pub const fn above(self) -> Self {
        self.offset(0, 1, 0)
    }
}

/// 光照引擎读区块的地方，官方的 `LightChunkGetter`。超平坦世界里每个区块都在。
#[derive(Debug, Clone, Copy)]
pub(crate) struct Chunks<'a> {
    template: &'a Chunk,
    edited: &'a HashMap<ChunkPos, Chunk>,
}

impl<'a> Chunks<'a> {
    pub(crate) fn new(template: &'a Chunk, edited: &'a HashMap<ChunkPos, Chunk>) -> Self {
        Self { template, edited }
    }

    pub(crate) fn chunk(self, pos: ChunkPos) -> &'a Chunk {
        self.edited.get(&pos).unwrap_or(self.template)
    }

    /// `getState`。世界外是空气。
    pub(crate) fn state(self, pos: BlockPos) -> u32 {
        self.chunk(pos.chunk())
            .block_or_air((pos.x & 15) as usize, pos.y, (pos.z & 15) as usize)
    }

    /// `SkyLightEngine.getLowestSourceY`，世界坐标。
    pub(crate) fn lowest_source_y(self, x: i32, z: i32) -> i32 {
        self.chunk(ChunkPos::new(x >> 4, z >> 4))
            .sky_light_sources()
            .lowest_source_y((x & 15) as usize, (z & 15) as usize)
    }
}

/// 一个区块光照变了的段，`ChunkHolder` 的两个 `BitSet`。第 n 位是从 [`MIN_LIGHT_SECTION`] 数起的第 n 段。
///
/// 和官方一样，顶上那段再往上一段也记，它是第 [`LIGHT_SECTION_COUNT`] 位，发包时不看。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LightChanges {
    pub sky: u32,
    pub block: u32,
}

impl LightChanges {
    /// 这一层的位。
    pub fn of(self, layer: LightLayer) -> u32 {
        match layer {
            LightLayer::Sky => self.sky,
            LightLayer::Block => self.block,
        }
    }

    /// `ChunkHolder.sectionLightChanged`：只记从最低光照段到最高光照段再往上一段的范围。
    fn mark(&mut self, layer: LightLayer, section_y: i32) {
        let index = section_y - MIN_LIGHT_SECTION;
        if !(0..=LIGHT_SECTION_COUNT as i32).contains(&index) {
            return;
        }
        let bits = match layer {
            LightLayer::Sky => &mut self.sky,
            LightLayer::Block => &mut self.block,
        };
        *bits |= 1 << index;
    }
}

/// 整个世界的光照。
#[derive(Debug, Clone)]
pub struct LevelLight {
    sky: LightEngine,
    block: LightEngine,
    /// 上一轮之后排了多少个任务：检查一格，或者一个区块段变空、变满。
    pending_tasks: usize,
    changes: BTreeMap<ChunkPos, LightChanges>,
}

impl LevelLight {
    fn new(sky: LightStorage, block: LightStorage) -> Self {
        Self {
            sky: LightEngine::new(sky),
            block: LightEngine::new(block),
            pending_tasks: 0,
            changes: BTreeMap::new(),
        }
    }

    /// 每个区块都和 `template` 一样、都算好了光照的世界。
    ///
    /// 模板的光照照官方生成区块的流程算：先给一片区块登记有方块的段（`initializeLight`），
    /// 再逐个打开光照、点亮光源（`lightChunk`），取中间那个区块柱的结果。
    pub(crate) fn for_template(template: &Chunk) -> Self {
        let edited = HashMap::new();
        let chunks = Chunks::new(template, &edited);
        let empty = |layer| LightStorage::new(layer, LightColumn::empty(), i32::MAX);
        let mut light = Self::new(empty(LightLayer::Sky), empty(LightLayer::Block));

        let grid = template_grid();
        for &pos in &grid {
            light.initialize_chunk(template, pos);
        }
        light.run_light_updates(chunks);
        for &pos in &grid {
            light.block.propagate_light_sources(chunks, pos);
            light.sky.propagate_light_sources(chunks, pos);
        }
        light.run_light_updates(chunks);

        let origin = ChunkPos::new(0, 0);
        let lit = |engine: &LightEngine, layer| {
            let column = engine.storage.column_at(origin).lit();
            LightStorage::new(layer, column, engine.storage.lowest_section())
        };
        Self::new(
            lit(&light.sky, LightLayer::Sky),
            lit(&light.block, LightLayer::Block),
        )
    }

    /// `initializeLight` 的前半：登记区块里有方块的段。
    fn initialize_chunk(&mut self, chunk: &Chunk, pos: ChunkPos) {
        for (index, section) in chunk.sections().iter().enumerate() {
            if !section.is_empty() {
                let section_pos =
                    SectionPos::new(pos.x, MIN_LIGHT_SECTION + 1 + index as i32, pos.z);
                self.block.storage.update_section_status(section_pos, false);
                self.sky.storage.update_section_status(section_pos, false);
            }
        }
    }

    /// `checkBlock`：这一格的光照性质变了。
    pub(crate) fn check_block(&mut self, chunks: Chunks<'_>, pos: BlockPos) {
        self.block.check_block(pos);
        self.sky.check_block(pos);
        self.task_added(chunks);
    }

    /// `updateSectionStatus`：一个区块段变空了，或者从空的变得有方块了。
    pub(crate) fn update_section_status(
        &mut self,
        chunks: Chunks<'_>,
        section: SectionPos,
        empty: bool,
    ) {
        self.block.storage.update_section_status(section, empty);
        self.sky.storage.update_section_status(section, empty);
        self.task_added(chunks);
    }

    /// 任务攒够一批就马上算一轮。
    ///
    /// 官方的任务要等到算的时候才执行，这里当场执行。两种任务都不读方块，执行的先后也没变，
    /// 而新建的段读出来的值和它建起来之前一样，所以结果相同。
    fn task_added(&mut self, chunks: Chunks<'_>) {
        self.pending_tasks += 1;
        if self.pending_tasks >= TASKS_PER_BATCH {
            self.run_light_updates(chunks);
        }
    }

    /// 把排着的任务算完，官方在两个 tick 之间做的事。
    pub(crate) fn run_updates(&mut self, chunks: Chunks<'_>) {
        if self.pending_tasks > 0 || self.block.has_light_work() || self.sky.has_light_work() {
            self.run_light_updates(chunks);
        }
    }

    /// `runLightUpdates`：先方块光，再天空光，记下写过的段。
    fn run_light_updates(&mut self, chunks: Chunks<'_>) {
        self.pending_tasks = 0;
        self.block.run_light_updates(chunks);
        self.collect_changes(LightLayer::Block);
        self.sky.run_light_updates(chunks);
        self.collect_changes(LightLayer::Sky);
    }

    /// `swapSectionMap` 里的 `onLightUpdate`。
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

    /// 上次取走以后光照变了的区块和段。
    pub fn take_changes(&mut self) -> BTreeMap<ChunkPos, LightChanges> {
        std::mem::take(&mut self.changes)
    }

    fn storage(&self, layer: LightLayer) -> &LightStorage {
        match layer {
            LightLayer::Sky => &self.sky.storage,
            LightLayer::Block => &self.block.storage,
        }
    }

    /// `getLightValue`：一格的天空光或者方块光。
    pub fn light_value(&self, layer: LightLayer, pos: BlockPos) -> u8 {
        self.storage(layer).light_value(pos)
    }

    /// `getDataLayerData`：一个光照段存的数据，没存时是 `None`。
    pub fn section(&self, layer: LightLayer, section: SectionPos) -> Option<&DataLayer> {
        self.storage(layer).layer(section)
    }

    /// 这个区块柱的光照写过，可能和模板不同。
    pub fn is_materialized(&self, pos: ChunkPos) -> bool {
        self.sky.storage.is_materialized(pos) || self.block.storage.is_materialized(pos)
    }

    /// 这个区块柱两层光照的版本号，天空光在前。每写一次都换成没用过的新值，没写过的柱是 0。
    pub fn revision(&self, pos: ChunkPos) -> [u64; 2] {
        [
            self.sky.storage.revision(pos),
            self.block.storage.revision(pos),
        ]
    }
}

/// 模板区块四周的一片区块，x 在外层。
fn template_grid() -> Vec<ChunkPos> {
    let range = -TEMPLATE_RADIUS..=TEMPLATE_RADIUS;
    range
        .clone()
        .flat_map(|x| range.clone().map(move |z| ChunkPos::new(x, z)))
        .collect()
}

#[cfg(test)]
mod tests;
