//! 两张等级平面的增量传播。
//!
//! 不动点是 `level(p) = min(source(p), min over 8 neighbors (level(n) + 1))`，钳到 `[0, limit]`。
//! 官方用 `DynamicGraphMinFixedPoint` 的分级优先队列维护它；这里用等价的两阶段算法：
//!
//! 1. **升高**：所有等级变大（或被撤掉）的源一起做一次多源 BFS，候选值
//!    `min(旧源等级 + 切比雪夫距离)`。当前等级恰好等于这个候选值的区块，原来靠
//!    这些源取得等级，等级可能要涨；把它们清空。等级更低的地方由没变的源撑着，剪枝。
//! 2. **降低**：把新源/加强的源、清空区自己的源、以及清空区外圈的边界值当种子，
//!    按等级从小到大松弛（和官方 `LeveledPriorityQueue` 一样的桶队列）。只有
//!    "候选值 < 当前等级"才继续往邻居走，所以不会遍历整个视野。
//!
//! 哈希图只用来查找，不用来产生输出：变化列表按 (x, z) 排序，只包含等级真的变了的位置。
//! 清空区、访问标记和变化列表都按发现顺序或坐标顺序遍历，辅助结构的块每轮回收到一个
//! 有上限的池子，数量只和一次更新覆盖的区域有关，和历史上走过多少区块无关。

use std::{
    collections::{HashMap, VecDeque},
    mem,
};

use mc_bamboo_core::ChunkPos;

use super::{
    level::{MAX_VIEW_DISTANCE, PLAYER_TICKET_LEVEL, UNLOADED_LEVEL},
    profile::{Guard, count},
    table::ChunkHasherBuilder,
};

/// 每个块 64×64 个区块。邻居绝大多数落在同一块里，块内直接数组下标。
const BLOCK_SHIFT: i32 = 6;
const BLOCK_SIZE: i32 = 1 << BLOCK_SHIFT;
const BLOCK_MASK: i32 = BLOCK_SIZE - 1;
const BLOCK_CELLS: usize = (BLOCK_SIZE * BLOCK_SIZE) as usize;
/// 等级从玩家票的 `PLAYER_TICKET_LEVEL` 最多向外传播到无票，所以一次更新的
/// 覆盖半径不超过「视距 + 传播半径」。按最大视距算一个固定上限：跨度换成
/// 64 格一块后两端各留一块余量，再平方。超过上限的块用完就释放，池子不再
/// 记住历史峰值。
#[doc(hidden)]
pub const SCRATCH_POOL_CAP: usize = {
    let radius = MAX_VIEW_DISTANCE + (UNLOADED_LEVEL - PLAYER_TICKET_LEVEL);
    let span = 2 * radius + 1;
    let blocks_per_side = ((span + BLOCK_SIZE - 1) / BLOCK_SIZE + 2) as usize;
    blocks_per_side * blocks_per_side
};
const NEIGHBOR_OFFSETS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];
type Block = [i32; BLOCK_CELLS];
type BlockKey = (i32, i32);
type BlockMap<T> = HashMap<BlockKey, Box<T>, ChunkHasherBuilder>;

fn block_key(pos: ChunkPos) -> BlockKey {
    (pos.x >> BLOCK_SHIFT, pos.z >> BLOCK_SHIFT)
}

/// 按坐标分块的稠密等级表。缺省值（limit）也照常写在数组里，只是不计入 `count`。
/// 已分配的块不回收：全世界玩家活动过的范围是有界的，回收反而让热路径多一次判断。
#[derive(Debug)]
struct LevelGrid {
    blocks: BlockMap<Block>,
    default: i32,
    count: usize,
}

impl LevelGrid {
    fn new(default: i32) -> Self {
        Self {
            blocks: HashMap::default(),
            default,
            count: 0,
        }
    }

    fn get(&self, pos: ChunkPos) -> i32 {
        self.block(block_key(pos))
            .map_or(self.default, |block| block[cell_index(pos)])
    }

    /// 按块号取块；热循环里相邻的区块绝大多数落在同一块，用游标或块级访问省掉重复哈希查找。
    fn block(&self, key: BlockKey) -> Option<&Block> {
        self.blocks.get(&key).map(|block| &**block)
    }

    fn cursor(&self) -> BlockCursor<'_> {
        BlockCursor {
            grid: self,
            key: None,
            block: None,
        }
    }

    fn set(&mut self, pos: ChunkPos, level: i32) {
        let key = block_key(pos);
        let index = cell_index(pos);
        let block = self
            .blocks
            .entry(key)
            .or_insert_with(|| Box::new([self.default; BLOCK_CELLS]));
        let old = block[index];
        if old != level {
            if old == self.default {
                self.count += 1;
            }
            if level == self.default {
                self.count -= 1;
            }
            block[index] = level;
        }
    }
}

/// 连续读等级时的块缓存：同一个 64×64 块内直接数组下标，只有跨块才查一次哈希图。
struct BlockCursor<'a> {
    grid: &'a LevelGrid,
    key: Option<BlockKey>,
    block: Option<&'a Block>,
}

impl BlockCursor<'_> {
    fn get(&mut self, pos: ChunkPos) -> i32 {
        let key = block_key(pos);
        if self.key != Some(key) {
            self.key = Some(key);
            self.block = self.grid.block(key);
        }
        self.block
            .map_or(self.grid.default, |block| block[cell_index(pos)])
    }
}

fn cell_index(pos: ChunkPos) -> usize {
    ((pos.x & BLOCK_MASK) as usize) * BLOCK_SIZE as usize + (pos.z & BLOCK_MASK) as usize
}

/// 8 邻域，固定顺序（官方 `ChunkTracker.checkNeighborsAfterUpdate` 的 dx/dz 双重循环）。
/// 用 `saturating_add` 保证极端坐标也不会溢出。
fn neighbors(pos: ChunkPos) -> [ChunkPos; 8] {
    NEIGHBOR_OFFSETS
        .map(|(dx, dz)| ChunkPos::new(pos.x.saturating_add(dx), pos.z.saturating_add(dz)))
}

/// 一次更新里某个 64×64 块的全部辅助数据，合成一个结构以便一次哈希查找拿全。
///
/// `stamp` 是本轮的 epoch；不等于当前 epoch 的格子当作"本轮没碰过"，所以块复用时
/// 不必逐格清零。`flags` 的三位分别记"被 BFS 见过 / 被清空 / 旧等级已记下"。
#[derive(Debug)]
struct ScratchBlock {
    stamp: [u64; BLOCK_CELLS],
    flags: [u8; BLOCK_CELLS],
    old: [i32; BLOCK_CELLS],
    /// 位图属于哪一轮；换新轮时整块清掉。
    bitmap_epoch: u64,
    /// 每格一位记"本轮记过旧等级"；收变化列表时按字跳过大片没记过的格子。
    recorded: [u64; BLOCK_CELLS / 64],
}

impl ScratchBlock {
    const SEEN: u8 = 1;
    const RESET: u8 = 2;
    const RECORDED: u8 = 4;

    fn new() -> Box<Self> {
        Box::new(Self {
            stamp: [0; BLOCK_CELLS],
            flags: [0; BLOCK_CELLS],
            old: [0; BLOCK_CELLS],
            bitmap_epoch: 0,
            recorded: [0; BLOCK_CELLS / 64],
        })
    }
}

/// 每轮更新的辅助状态：旧等级、清空标记和清空位置。
///
/// 块只在一次 `update` 期间存在，结束时回收到有上限的池子；既不每轮分配/释放几十 KiB
/// 的块，也不让块数随玩家走过的总距离一直涨。清空区按 BFS 发现顺序记在 Vec 里，
/// 遍历不依赖哈希图顺序；变化列表按块号和块内下标排序产生。
#[derive(Debug)]
struct Scratch {
    blocks: HashMap<BlockKey, Box<ScratchBlock>, ChunkHasherBuilder>,
    pool: Vec<Box<ScratchBlock>>,
    /// 清空位置按 BFS 发现顺序记，遍历它不依赖任何哈希图的顺序。
    reset_order: Vec<ChunkPos>,
    /// 本轮用到过的块号，收变化列表时按坐标排。
    used_blocks: Vec<BlockKey>,
    epoch: u64,
}

impl Scratch {
    fn new() -> Self {
        Self {
            blocks: HashMap::default(),
            pool: Vec::new(),
            reset_order: Vec::new(),
            used_blocks: Vec::new(),
            epoch: 0,
        }
    }

    /// 开始一轮：清空顺序列表，上一轮用过的块已在结束时回到池子。
    fn begin(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            // u64 回绕（实际不可达）：把池子也清掉，免得旧 epoch 的标记被误认。
            self.pool.clear();
            self.epoch = 1;
        }
        self.reset_order.clear();
        self.used_blocks.clear();
    }

    /// 一轮结束：块回收，只保留固定个供下轮复用，多出来的直接释放。
    fn recycle(&mut self) {
        let keep = SCRATCH_POOL_CAP.saturating_sub(self.pool.len());
        for (_, block) in self.blocks.drain().take(keep) {
            self.pool.push(block);
        }
    }

    /// 辅助结构当前占着多少个块（含池子）；公开给上层测试检查内存不随历史距离增长。
    fn block_count(&self) -> usize {
        self.blocks.len() + self.pool.len()
    }

    /// 记下一个区块在本次更新开始前的等级；同一轮里只记第一次。
    fn record(&mut self, pos: ChunkPos, old: i32) {
        let key = block_key(pos);
        let index = cell_index(pos);
        let epoch = self.epoch;
        let block = match self.blocks.entry(key) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let block = self.pool.pop().unwrap_or_else(ScratchBlock::new);
                self.used_blocks.push(key);
                entry.insert(block)
            }
        };
        if block.stamp[index] != epoch {
            block.stamp[index] = epoch;
            block.flags[index] = 0;
            block.old[index] = 0;
        }
        if block.bitmap_epoch != epoch {
            block.bitmap_epoch = epoch;
            block.recorded.fill(0);
        }
        if block.flags[index] & ScratchBlock::RECORDED == 0 {
            block.flags[index] |= ScratchBlock::RECORDED;
            block.old[index] = old;
            block.recorded[index / 64] |= 1 << (index % 64);
        }
    }

    /// BFS 里一次完成"见过没有 / 记旧等级 / 标清空"，只查一次块。
    fn visit_and_reset(&mut self, pos: ChunkPos, old: i32) -> bool {
        let key = block_key(pos);
        let index = cell_index(pos);
        let epoch = self.epoch;
        let block = match self.blocks.entry(key) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let block = self.pool.pop().unwrap_or_else(ScratchBlock::new);
                self.used_blocks.push(key);
                entry.insert(block)
            }
        };
        if block.stamp[index] != epoch {
            block.stamp[index] = epoch;
            block.flags[index] = 0;
            block.old[index] = 0;
        }
        if block.flags[index] & ScratchBlock::SEEN != 0 {
            return false;
        }
        block.flags[index] |= ScratchBlock::SEEN | ScratchBlock::RESET;
        if block.bitmap_epoch != epoch {
            block.bitmap_epoch = epoch;
            block.recorded.fill(0);
        }
        if block.flags[index] & ScratchBlock::RECORDED == 0 {
            block.flags[index] |= ScratchBlock::RECORDED;
            block.old[index] = old;
            block.recorded[index / 64] |= 1 << (index % 64);
        }
        true
    }

    /// 把一个位置记进清空区（调用前已经 visit 过）。
    fn push_reset_order(&mut self, pos: ChunkPos) {
        self.reset_order.push(pos);
    }

    /// 连续问"这个邻居在不在清空区"时的块缓存，等价于 `BlockCursor` 但读的是标记块。
    fn reset_cursor(&self) -> ResetCursor<'_> {
        ResetCursor {
            scratch: self,
            key: None,
            block: None,
        }
    }
}

struct ResetCursor<'a> {
    scratch: &'a Scratch,
    key: Option<BlockKey>,
    block: Option<&'a ScratchBlock>,
}

impl ResetCursor<'_> {
    fn is_reset(&mut self, pos: ChunkPos) -> bool {
        let key = block_key(pos);
        if self.key != Some(key) {
            self.key = Some(key);
            self.block = self.scratch.blocks.get(&key).map(|block| &**block);
        }
        let index = cell_index(pos);
        self.block.is_some_and(|block| {
            block.stamp[index] == self.scratch.epoch
                && block.flags[index] & ScratchBlock::RESET != 0
        })
    }
}

impl Scratch {
    /// 按块号、块内下标遍历；负块号下块号字典序和 (x, z) 字典序不一致，最后再排一次。
    fn collect(&mut self, levels: &LevelGrid, limit: i32, result: &mut Vec<(ChunkPos, i32, i32)>) {
        result.clear();
        self.used_blocks
            .sort_unstable_by_key(|&(bx, bz)| (bx << BLOCK_SHIFT, bz << BLOCK_SHIFT));
        for &key in &self.used_blocks {
            let block = &self.blocks[&key];
            let level_block = levels.block(key);
            count(super::profile::C_COLLECT_SCAN, BLOCK_CELLS as u64);
            for (word_index, &word) in block.recorded.iter().enumerate() {
                if word == 0 {
                    continue;
                }
                for bit in 0..64 {
                    if word & (1 << bit) == 0 {
                        continue;
                    }
                    let index = word_index * 64 + bit;
                    let old = block.old[index];
                    let new = level_block.map_or(levels.default, |levels| levels[index]);
                    if old != new {
                        let pos = ChunkPos::new(
                            (key.0 << BLOCK_SHIFT) + (index as i32 / BLOCK_SIZE),
                            (key.1 << BLOCK_SHIFT) + (index as i32 % BLOCK_SIZE),
                        );
                        result.push((pos, old, new));
                    }
                }
            }
        }
        // 负块号下块号的字典序和 (x, z) 的字典序不一致，最后按区块坐标排一次。
        result.sort_unstable_by_key(|&(pos, _, _)| (pos.x, pos.z));
        debug_assert!(
            result
                .windows(2)
                .all(|w| (w[0].0.x, w[0].0.z) < (w[1].0.x, w[1].0.z))
        );
        debug_assert!(result.iter().all(|&(_, _, new)| new <= limit));
        count(super::profile::C_CHANGES, result.len() as u64);
    }
}

/// 一张等级平面。
#[derive(Debug)]
pub struct Plane {
    /// 位置 → 该位置同标志票里最小的等级（小于 limit；没有就是 limit）。
    sources: HashMap<ChunkPos, i32, ChunkHasherBuilder>,
    /// 无票等级，也是传播的上限。加载平面 45，模拟平面 33。
    limit: i32,
    /// 当前等级。
    levels: LevelGrid,
    /// 跨轮复用的辅助结构。
    scratch: Scratch,
    /// 跨轮复用的分桶队列；每轮清空但不释放桶的容量。
    queue: LevelQueue,
    /// 跨轮复用的源变化列表。
    buffers: Buffers,
}

/// `update` 里每轮都要用、但跨轮可以复用容量的几个 `Vec`。
#[derive(Debug, Default)]
struct Buffers {
    folded: Vec<((i32, i32), usize, i32, i32)>,
    effective: Vec<(ChunkPos, i32, i32)>,
    frontier: Vec<Vec<ChunkPos>>,
}

impl Buffers {
    fn new(limit: i32) -> Self {
        Self {
            folded: Vec::new(),
            effective: Vec::new(),
            frontier: (0..=limit.max(0)).map(|_| Vec::new()).collect(),
        }
    }
}

impl Plane {
    pub fn new(limit: i32) -> Self {
        Self {
            sources: HashMap::default(),
            limit,
            levels: LevelGrid::new(limit),
            scratch: Scratch::new(),
            queue: LevelQueue::new(limit),
            buffers: Buffers::new(limit),
        }
    }

    /// 当前等级；没有记录就是无票。
    pub fn level(&self, pos: ChunkPos) -> i32 {
        self.levels.get(pos)
    }

    /// 表里非默认等级的区块数。
    pub fn len(&self) -> usize {
        self.levels.count
    }

    /// 辅助结构占的块数，测试用来检查长期移动不会让内存一直涨。
    #[doc(hidden)]
    pub fn scratch_blocks(&self) -> usize {
        self.scratch.block_count()
    }

    fn source(&self, pos: ChunkPos) -> i32 {
        self.sources.get(&pos).copied().unwrap_or(self.limit)
    }

    fn set_level(&mut self, pos: ChunkPos, level: i32) {
        self.levels.set(pos, level.min(self.limit));
    }

    /// 应用一批源变化（旧等级、新等级都用 limit 表示"没有源"），返回这一平面变了的位置。
    pub fn update(
        &mut self,
        changes: &[(ChunkPos, i32, i32)],
        stages: [usize; 5],
    ) -> Vec<(ChunkPos, i32, i32)> {
        let limit = self.limit;
        let _fold_guard = Guard::new(stages[4]);
        // 同一个位置可能在一步里被改了多次（玩家移动时增删交错），折叠成"最初旧值 → 最终新值"。
        // 先按 (坐标, 到达顺序) 排：同一位置的若干条变化挨在一起且保持先后，
        // 第一条的旧值配最后一条的新值。排序是稳定的口径，不依赖哈希图。
        let mut folded = mem::take(&mut self.buffers.folded);
        folded.clear();
        folded.extend(changes.iter().enumerate().map(|(order, &(pos, old, new))| {
            (
                (pos.x, pos.z),
                order,
                old.clamp(0, limit),
                new.clamp(0, limit),
            )
        }));
        folded.sort_unstable_by_key(|&(key, order, _, _)| (key, order));
        let mut effective = mem::take(&mut self.buffers.effective);
        effective.clear();
        for &((x, z), _, old, new) in &folded {
            let pos = ChunkPos::new(x, z);
            match effective.last_mut() {
                Some(last) if last.0 == pos => last.2 = new,
                _ => effective.push((pos, old, new)),
            }
        }
        effective.retain(|&(_, old, new)| old != new);
        self.buffers.folded = folded;
        if effective.is_empty() {
            self.buffers.effective = effective;
            return Vec::new();
        }
        self.scratch.begin();
        drop(_fold_guard);

        // 阶段一：多源 BFS 找升高锥，只看旧的等级表，不改表。
        let _bfs_guard = Guard::new(stages[0]);
        self.collect_increases(&effective);
        drop(_bfs_guard);

        // 改源表，把升高锥里的等级清空。
        for &(pos, _old, new) in &effective {
            if new >= limit {
                self.sources.remove(&pos);
            } else {
                self.sources.insert(pos, new);
            }
            self.scratch.record(pos, self.level(pos));
        }

        let reset_order = std::mem::take(&mut self.scratch.reset_order);
        for &pos in &reset_order {
            self.scratch.record(pos, self.level(pos));
            self.levels.set(pos, limit);
        }

        // 阶段二：按等级从小到大松弛，用和官方 LeveledPriorityQueue 一样的桶队列。
        let _seed_guard = Guard::new(stages[1]);
        let mut queue = std::mem::replace(&mut self.queue, LevelQueue::empty());
        queue.clear();
        for &(pos, old, new) in &effective {
            if new < old && new < limit {
                queue.push(new, pos);
            }
        }
        self.scratch.reset_order = reset_order;
        let mut cursor = self.levels.cursor();
        let mut reset_cursor = self.scratch.reset_cursor();
        for &pos in &self.scratch.reset_order {
            let mut boundary = self.source(pos);
            // 清空区外一圈的区块没被清空，仍保留旧等级。取最小的边界值当候选，
            // 每个清空区块只排一次，而不是给每个邻居排一次。
            count(super::profile::C_SEED_NEIGHBOR, 8);
            for neighbor in neighbors(pos) {
                if !reset_cursor.is_reset(neighbor) {
                    boundary = boundary.min(cursor.get(neighbor).saturating_add(1));
                }
            }
            if boundary < limit {
                queue.push(boundary, pos);
            }
        }

        drop(_seed_guard);
        let _relax_guard = Guard::new(stages[2]);
        let mut cursor = self.levels.cursor();
        while let Some((candidate, pos)) = queue.pop() {
            count(super::profile::C_RELAX_POP, 1);
            let current = cursor.get(pos);
            if candidate >= current {
                continue;
            }
            count(super::profile::C_RELAX_UPDATE, 1);
            self.scratch.record(pos, current);
            // 写入后游标里的旧块内容失效，下一轮重建（不分配，只是清掉缓存）。
            self.set_level(pos, candidate);
            let next = candidate + 1;
            if next < limit {
                cursor = self.levels.cursor();
                for neighbor in neighbors(pos) {
                    if next < cursor.get(neighbor) {
                        count(super::profile::C_RELAX_PUSH, 1);
                        queue.push(next, neighbor);
                    }
                }
            } else {
                cursor = self.levels.cursor();
            }
        }

        self.queue = queue;
        drop(_relax_guard);
        self.buffers.effective = effective;
        let _collect_guard = Guard::new(stages[3]);
        let mut result = Vec::new();
        self.scratch.collect(&self.levels, limit, &mut result);
        self.scratch.recycle();
        drop(_collect_guard);
        result
    }

    /// 从全部"等级变大/被撤掉"的源一起做 BFS，找等级可能升高的区块。
    ///
    /// 逐层扩张：第 value 层的候选值是 `旧源等级 + d`，当前等级恰好等于它的区块原来靠
    /// 这些源取得等级。等级更低的地方由没变的源撑着，剪掉整个分支。每个区块只访问一次。
    fn collect_increases(&mut self, effective: &[(ChunkPos, i32, i32)]) {
        let limit = self.limit;
        // 第 value 层里装"候选值 == value"的待访问区块；层向量跨轮复用。
        let mut frontier = mem::take(&mut self.buffers.frontier);
        for bucket in &mut frontier {
            bucket.clear();
        }
        for &(pos, old, new) in effective {
            if new > old && old < limit {
                frontier[old as usize].push(pos);
            }
        }
        let mut cursor = self.levels.cursor();
        for value in 0..limit {
            let mut index = 0;
            while index < frontier[value as usize].len() {
                let pos = frontier[value as usize][index];
                index += 1;
                count(super::profile::C_BFS_VISIT, 1);
                // 等级低于候选值：这个区块由别的源撑着，不受升高影响，整支剪掉。
                if cursor.get(pos) != value || !self.scratch.visit_and_reset(pos, value) {
                    continue;
                }
                count(super::profile::C_BFS_RESET, 1);
                self.scratch.push_reset_order(pos);
                let next = value + 1;
                if next < limit {
                    // 等级低于候选值的邻居必然由别的源撑着，入队前就剪掉；
                    // 也避免同一个位置被多个已清空的邻居反复推入下一层。
                    let bucket = &mut frontier[next as usize];
                    for neighbor in neighbors(pos) {
                        if cursor.get(neighbor) == next {
                            bucket.push(neighbor);
                        }
                    }
                }
            }
        }
        self.buffers.frontier = frontier;
    }
}

/// 按等级分桶的 FIFO 队列，等价于官方 `LeveledPriorityQueue`：等级小的先出队，
/// 同等级按入队顺序。入队出队都是 O(1)。
#[derive(Debug)]
struct LevelQueue {
    buckets: Vec<VecDeque<ChunkPos>>,
    first: usize,
}

impl LevelQueue {
    fn new(limit: i32) -> Self {
        let count = (limit as usize).saturating_add(1);
        Self {
            buckets: (0..count).map(|_| VecDeque::new()).collect(),
            first: count,
        }
    }

    /// 占位值，只在把队列从 `Plane` 里取出来时用，不分配桶。
    fn empty() -> Self {
        Self {
            buckets: Vec::new(),
            first: 0,
        }
    }

    fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
        self.first = self.buckets.len();
    }

    fn push(&mut self, level: i32, pos: ChunkPos) {
        if level < 0 {
            return;
        }
        let index = level as usize;
        if index >= self.buckets.len() {
            return;
        }
        self.buckets[index].push_back(pos);
        if index < self.first {
            self.first = index;
        }
    }

    fn pop(&mut self) -> Option<(i32, ChunkPos)> {
        while self.first < self.buckets.len() && self.buckets[self.first].is_empty() {
            self.first += 1;
        }
        if self.first == self.buckets.len() {
            return None;
        }
        let pos = self.buckets[self.first].pop_front()?;
        Some((self.first as i32, pos))
    }
}
