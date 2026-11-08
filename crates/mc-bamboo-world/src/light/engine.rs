//! 两种光照共用的部分，官方的 `LightEngine`。
//!
//! 一轮更新分三步：先看每个改过的格子（`checkNode`），再把变暗的地方一圈圈往外清零（减少队列），
//! 最后从还亮着的边界和光源重新往里照（增加队列）。两个队列都是先进先出。

use std::collections::{HashSet, VecDeque};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};

use super::{Chunks, storage::LightStorage};

/// `MAX_LEVEL`。
pub(crate) const MAX_LEVEL: u8 = 15;

/// 队列里的一项附带的数据，官方的 `LightEngine.QueueEntry`：
/// 低 4 位是等级，接着 6 位是要传播的方向，再往上两位是两个标记。
pub(crate) mod entry {
    use mc_bamboo_core::Direction;

    const LEVEL_MASK: u64 = 15;
    const DIRECTIONS_MASK: u64 = 0b11_1111 << 4;
    const FROM_EMPTY_SHAPE: u64 = 1 << 10;
    const FROM_EMISSION: u64 = 1 << 11;

    const fn with_level(entry: u64, level: u8) -> u64 {
        entry & !LEVEL_MASK | (level as u64 & LEVEL_MASK)
    }

    const fn direction_bit(direction: Direction) -> u64 {
        1 << (direction.id() + 4)
    }

    const fn flag(set: bool, flag: u64) -> u64 {
        if set { flag } else { 0 }
    }

    /// `decreaseSkipOneDirection`。
    pub(crate) const fn decrease_skip_one_direction(level: u8, skip: Direction) -> u64 {
        with_level(DIRECTIONS_MASK & !direction_bit(skip), level)
    }

    /// `decreaseAllDirections`。
    pub(crate) const fn decrease_all_directions(level: u8) -> u64 {
        with_level(DIRECTIONS_MASK, level)
    }

    /// `increaseLightFromEmission`。
    pub(crate) const fn increase_light_from_emission(level: u8, from_empty_shape: bool) -> u64 {
        let entry = DIRECTIONS_MASK | FROM_EMISSION | flag(from_empty_shape, FROM_EMPTY_SHAPE);
        with_level(entry, level)
    }

    /// `increaseSkipOneDirection`。
    pub(crate) const fn increase_skip_one_direction(
        level: u8,
        from_empty_shape: bool,
        skip: Direction,
    ) -> u64 {
        let entry =
            DIRECTIONS_MASK & !direction_bit(skip) | flag(from_empty_shape, FROM_EMPTY_SHAPE);
        with_level(entry, level)
    }

    /// `increaseOnlyOneDirection`。
    pub(crate) const fn increase_only_one_direction(
        level: u8,
        from_empty_shape: bool,
        direction: Direction,
    ) -> u64 {
        let entry = direction_bit(direction) | flag(from_empty_shape, FROM_EMPTY_SHAPE);
        with_level(entry, level)
    }

    /// `increaseSkySourceInDirections`：等级 15，只往这几个方向照。
    pub(crate) const fn increase_sky_source_in_directions(
        down: bool,
        north: bool,
        south: bool,
        west: bool,
        east: bool,
    ) -> u64 {
        let entry = flag(down, direction_bit(Direction::Down))
            | flag(north, direction_bit(Direction::North))
            | flag(south, direction_bit(Direction::South))
            | flag(west, direction_bit(Direction::West))
            | flag(east, direction_bit(Direction::East));
        with_level(entry, 15)
    }

    /// `getFromLevel`。
    pub(crate) const fn from_level(entry: u64) -> u8 {
        (entry & LEVEL_MASK) as u8
    }

    /// `isFromEmptyShape`。
    pub(crate) const fn is_from_empty_shape(entry: u64) -> bool {
        entry & FROM_EMPTY_SHAPE != 0
    }

    /// `isIncreaseFromEmission`。
    pub(crate) const fn is_increase_from_emission(entry: u64) -> bool {
        entry & FROM_EMISSION != 0
    }

    /// `shouldPropagateInDirection`。
    pub(crate) const fn should_propagate_in_direction(entry: u64, direction: Direction) -> bool {
        entry & direction_bit(direction) != 0
    }
}

/// `PULL_LIGHT_IN_ENTRY`：自己不亮，只让亮着的邻居重新往这里照。
pub(crate) const PULL_LIGHT_IN_ENTRY: u64 = entry::decrease_all_directions(1);

/// 一层光照的引擎和它的存储。
#[derive(Debug, Clone)]
pub(crate) struct LightEngine {
    pub(crate) storage: LightStorage,
    /// `blockNodesToCheck`。官方是哈希集合，这里按加入的顺序检查，结果和顺序无关。
    nodes_to_check: VecDeque<BlockPos>,
    checking: HashSet<BlockPos>,
    decrease: VecDeque<(BlockPos, u64)>,
    increase: VecDeque<(BlockPos, u64)>,
}

impl LightEngine {
    pub(crate) fn new(storage: LightStorage) -> Self {
        Self {
            storage,
            nodes_to_check: VecDeque::new(),
            checking: HashSet::new(),
            decrease: VecDeque::new(),
            increase: VecDeque::new(),
        }
    }

    fn layer(&self) -> LightLayer {
        self.storage.layer_kind()
    }

    /// `checkBlock`：这一格的光照性质变了，下一轮更新时检查它。
    pub(crate) fn check_block(&mut self, pos: BlockPos) {
        if self.checking.insert(pos) {
            self.nodes_to_check.push_back(pos);
        }
    }

    /// `hasLightWork`。
    pub(crate) fn has_light_work(&self) -> bool {
        self.storage.has_inconsistencies()
            || !self.nodes_to_check.is_empty()
            || !self.decrease.is_empty()
            || !self.increase.is_empty()
    }

    /// `runLightUpdates`，可以分几次做完：先检查记下的格子，再传播变暗，最后传播变亮。
    ///
    /// 每处理一项 `work` 加一，到了 `budget` 就停，下次从停下的地方接着做。前一步的队列空了才做下一步，
    /// 中间也没有别人改这一层的数据，所以和一口气做完的结果相同。全部做完以后丢掉不再存光照的段，返回 `true`。
    pub(crate) fn run_light_updates(
        &mut self,
        chunks: Chunks<'_>,
        work: &mut usize,
        budget: usize,
    ) -> bool {
        while *work < budget
            && let Some(node) = self.nodes_to_check.pop_front()
        {
            *work += 1;
            self.check_node(chunks, node);
        }
        while *work < budget
            && let Some((node, data)) = self.decrease.pop_front()
        {
            *work += 1;
            self.propagate_decrease(chunks, node, data);
        }
        while *work < budget
            && let Some((node, data)) = self.increase.pop_front()
        {
            *work += 1;
            self.propagate_increase(chunks, node, data);
        }
        if !self.nodes_to_check.is_empty() || !self.decrease.is_empty() || !self.increase.is_empty()
        {
            return false;
        }
        self.checking.clear();
        self.storage.mark_new_inconsistencies();
        true
    }

    /// `propagateLightSources`：一个刚生成的区块第一次算光照。
    pub(crate) fn propagate_light_sources(&mut self, chunks: Chunks<'_>, pos: ChunkPos) {
        match self.layer() {
            LightLayer::Sky => self.propagate_sky_light_sources(chunks, pos),
            LightLayer::Block => self.propagate_block_light_sources(chunks, pos),
        }
    }

    fn check_node(&mut self, chunks: Chunks<'_>, node: BlockPos) {
        match self.layer() {
            LightLayer::Sky => self.check_sky_node(chunks, node),
            LightLayer::Block => self.check_block_node(chunks, node),
        }
    }

    pub(crate) fn enqueue_decrease(&mut self, node: BlockPos, data: u64) {
        self.decrease.push_back((node, data));
    }

    pub(crate) fn enqueue_increase(&mut self, node: BlockPos, data: u64) {
        self.increase.push_back((node, data));
    }

    fn propagate_decrease(&mut self, chunks: Chunks<'_>, node: BlockPos, data: u64) {
        match self.layer() {
            LightLayer::Sky => self.propagate_sky_decrease(node, data),
            LightLayer::Block => self.propagate_block_decrease(chunks, node, data),
        }
    }

    /// 一项增加只在这一格还是入队时的等级时才传播，后来变得更亮的话会有另一项。
    /// 光源的那一项例外：它先把这一格点亮到自己的等级。
    fn propagate_increase(&mut self, chunks: Chunks<'_>, node: BlockPos, data: u64) {
        let mut level = self.storage.stored_level(node);
        let target = entry::from_level(data);
        if entry::is_increase_from_emission(data) && level < target {
            self.storage.set_stored_level(node, target);
            level = target;
        }
        if level == target {
            match self.layer() {
                LightLayer::Sky => self.propagate_sky_increase(chunks, node, data, level),
                LightLayer::Block => self.propagate_block_increase(chunks, node, data, level),
            }
        }
    }
}
