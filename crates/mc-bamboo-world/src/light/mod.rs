//! 光照，官方的 `LevelLightEngine`、`ThreadedLevelLightEngine` 和它们下面的整套光照引擎。
//!
//! 天空光和方块光各有一个引擎和一份按区块段存的数据，比世界多上下各一段。
//! 改方块时记下要检查的格子和变空、变满的区块段，攒成一轮一轮地算：官方在另一个线程上算，
//! 每批最多 1000 个任务；这里在世界线程上算，分批的规则相同，每 tick 只做有限的工作量，见 [`queue`] 和 [`level`]。
//!
//! 每个区块都已经生成并且算过光照，没改过的区块柱的光照和模板一样，见 `LevelLight::for_template`。

mod block;
mod data_layer;
mod engine;
mod level;
mod properties;
mod queue;
mod sky;
pub(crate) mod sources;
mod storage;

use std::collections::{BTreeMap, HashMap};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};

pub use data_layer::{DataLayer, LAYER_BYTES};
use engine::LightEngine;
use level::{Engines, Visible};
pub use properties::has_different_light_properties;
use queue::{Task, TaskQueue};
pub use sources::{NEGATIVE_INFINITY, SkyLightSources};
pub(crate) use storage::LightColumn;
use storage::LightStorage;

use crate::{MIN_Y, SECTION_COUNT, chunk::Chunk};

/// 光照比区块段多两段：世界底部下面和顶部上面各一段。
pub const LIGHT_SECTION_COUNT: usize = SECTION_COUNT + 2;
/// 最低的光照段，`getMinLightSection`。
pub const MIN_LIGHT_SECTION: i32 = (MIN_Y >> 4) - 1;
/// `ThreadedLevelLightEngine` 一批最多跑这么多个任务，攒够了就排成一轮。
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
    /// 光照落后时，正在算的这一轮排好之后又改过的区块在那一刻的样子，先读它们。
    past: Option<&'a HashMap<ChunkPos, Chunk>>,
}

impl<'a> Chunks<'a> {
    pub(crate) fn new(template: &'a Chunk, edited: &'a HashMap<ChunkPos, Chunk>) -> Self {
        Self {
            template,
            edited,
            past: None,
        }
    }

    /// 同一个世界，但 `past` 里有的区块读 `past` 里的。
    fn with_past<'b>(self, past: &'b HashMap<ChunkPos, Chunk>) -> Chunks<'b>
    where
        'a: 'b,
    {
        Chunks {
            template: self.template,
            edited: self.edited,
            past: (!past.is_empty()).then_some(past),
        }
    }

    pub(crate) fn chunk(self, pos: ChunkPos) -> &'a Chunk {
        if let Some(chunk) = self.past.and_then(|past| past.get(&pos)) {
            return chunk;
        }
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

/// 整个世界的光照，官方的 `ThreadedLevelLightEngine`：两个引擎、排给它们还没算的任务，和给别人读的那一份光照。
///
/// 别人读到的光照总是上一轮算完的样子：一轮可以分几个 tick 算，算完才换上，和官方读 `visibleSectionData` 一样。
#[derive(Debug, Clone)]
pub struct LevelLight {
    engines: Engines,
    visible: Visible,
    queue: TaskQueue,
}

impl LevelLight {
    /// 每个区块都和 `template` 一样、都算好了光照的世界。
    ///
    /// 模板的光照照官方生成区块的流程算：先给一片区块登记有方块的段（`initializeLight`），
    /// 再逐个打开光照、点亮光源（`lightChunk`），取中间那个区块柱的结果。
    pub(crate) fn for_template(template: &Chunk) -> Self {
        let edited = HashMap::new();
        let chunks = Chunks::new(template, &edited);
        let empty = |layer| LightStorage::new(layer, LightColumn::empty(), i32::MAX);
        let mut engines = Engines::new(empty(LightLayer::Sky), empty(LightLayer::Block));

        let grid = template_grid();
        for &pos in &grid {
            engines.initialize_chunk(template, pos);
        }
        engines.run_light_updates(chunks);
        for &pos in &grid {
            engines.block.propagate_light_sources(chunks, pos);
            engines.sky.propagate_light_sources(chunks, pos);
        }
        engines.run_light_updates(chunks);

        let origin = ChunkPos::new(0, 0);
        let lit = |engine: &LightEngine, layer| {
            let column = engine.storage.column_at(origin).lit();
            LightStorage::new(layer, column, engine.storage.lowest_section())
        };
        let sky = lit(&engines.sky, LightLayer::Sky);
        let block = lit(&engines.block, LightLayer::Block);
        Self {
            engines: Engines::new(sky.clone(), block.clone()),
            visible: Visible::new(sky, block),
            queue: TaskQueue::default(),
        }
    }

    /// `checkBlock`：这一格的光照性质变了。
    pub(crate) fn check_block(&mut self, pos: BlockPos) {
        self.queue.push(Task::CheckBlock(pos));
    }

    /// `updateSectionStatus`：一个区块段变空了，或者从空的变得有方块了。
    pub(crate) fn update_section_status(&mut self, section: SectionPos, empty: bool) {
        self.queue.push(Task::SectionStatus(section, empty));
    }

    /// `pos` 要换成 `state` 了，`chunk` 是它所在的区块改之前的样子。还有排着的轮次时要留一份。
    pub(crate) fn block_changing(&mut self, chunk: &Chunk, pos: BlockPos, state: u32) {
        self.queue.block_changing(chunk, pos, state);
    }

    /// 官方光照线程在两个 tick 之间做的事：攒着的任务排成一轮，再一轮轮地算。
    ///
    /// 工作量是任务数加上传播的步数，到了 `budget` 就停，哪怕一轮算到一半，下次接着算。
    /// 按工作量而不是按时间停，同样的改动在哪台机器上都在同一个 tick 算完。返回这次的工作量。
    pub(crate) fn run_updates(&mut self, chunks: Chunks<'_>, budget: usize) -> usize {
        self.queue.schedule();
        // 至少做一点，不然给 0 的时候永远停在原地。
        let budget = budget.max(1);
        let mut work = 0;
        while work < budget {
            if !self.engines.in_round() {
                let Some(tasks) = self.queue.start_round() else {
                    break;
                };
                work += self.engines.start_round(tasks);
            }
            let view = self.queue.view(chunks);
            if self.engines.continue_round(view, &mut work, budget) {
                self.engines.publish(&mut self.visible);
                self.queue.finish_round();
            }
        }
        work
    }

    /// 还有没算完的光照。
    pub(crate) fn has_pending_updates(&self) -> bool {
        self.queue.has_work()
    }

    /// 可以搬区块柱了：没有排着队的任务，也没有算到一半的轮次和引擎里剩下的工作。
    /// 这时引擎和给别人读的那两份光照内容相同，盒子只装一份就够。
    pub(crate) fn ready_to_take_column(&self) -> bool {
        !self.queue.has_work()
            && !self.engines.in_round()
            && !self.engines.sky.has_light_work()
            && !self.engines.block.has_light_work()
    }

    /// 把 `pos` 这一列的天空光和方块光整个拿走，这个位置回到模板柱。
    pub(crate) fn take_column(&mut self, pos: ChunkPos) -> [LightColumn; 2] {
        debug_assert!(self.ready_to_take_column(), "只在光照全部算完时搬区块柱");
        let engines = self.engines.take_column(pos);
        let visible = self.visible.take_column(pos);
        debug_assert!(
            engines[0].has_same_data(&visible[0]) && engines[1].has_same_data(&visible[1]),
            "算完以后引擎和别人读到的光照数据相同"
        );
        engines
    }

    /// 把一个区块柱的天空光和方块光同时放进引擎和给别人读的那份。
    pub(crate) fn insert_column(&mut self, pos: ChunkPos, [sky, block]: [LightColumn; 2]) {
        self.engines
            .insert_column(pos, [sky, block], &mut self.visible);
    }

    /// 上次取走以后光照变了的区块和段。
    pub fn take_changes(&mut self) -> BTreeMap<ChunkPos, LightChanges> {
        self.engines.take_changes()
    }

    /// `getLightValue`：一格的天空光或者方块光。
    pub fn light_value(&self, layer: LightLayer, pos: BlockPos) -> u8 {
        self.visible.storage(layer).light_value(pos)
    }

    /// `getDataLayerData`：一个光照段存的数据，没存时是 `None`。
    pub fn section(&self, layer: LightLayer, section: SectionPos) -> Option<&DataLayer> {
        self.visible.storage(layer).layer(section)
    }

    /// 这个区块柱的光照写过，可能和模板不同。
    pub fn is_materialized(&self, pos: ChunkPos) -> bool {
        let visible = &self.visible;
        visible.storage(LightLayer::Sky).is_materialized(pos)
            || visible.storage(LightLayer::Block).is_materialized(pos)
    }

    /// 这个区块柱两层光照给别人读的那一份的版本号，天空光在前。每发布一次都换成没用过的新值，没写过的柱是 0。
    pub fn revision(&self, pos: ChunkPos) -> [u64; 2] {
        let visible = &self.visible;
        [
            visible.storage(LightLayer::Sky).revision(pos),
            visible.storage(LightLayer::Block).revision(pos),
        ]
    }
}

/// `LevelChunk.setBlockState` 里和光照有关的部分：写一格，光照性质变了时更新这一列的天空光源。
/// 返回光照性质变没变。
pub(crate) fn write_block(chunk: &mut Chunk, pos: BlockPos, state: u32) -> bool {
    let (x, z) = ((pos.x & 15) as usize, (pos.z & 15) as usize);
    let previous = chunk.block(x, pos.y, z);
    chunk.set_block(x, pos.y, z, state);
    let changed = has_different_light_properties(previous, state);
    if changed {
        chunk.update_sky_light_sources(x, pos.y, z);
    }
    changed
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
