//! 两张等级平面的增量传播。
//!
//! 不动点是 `level(p) = min(source(p), min over 8 neighbors (level(n) + 1))`，钳到 `[0, limit]`。
//! 官方用 `DynamicGraphMinFixedPoint` 的分级优先队列维护它；这里用等价的两阶段算法：
//!
//! 1. **升高**：所有等级变大（或被撤掉）的源一起做一次多源 BFS，候选值
//!    `min(旧源等级 + 切比雪夫距离)`。当前等级恰好等于这个候选值的区块，说明它原来是
//!    靠这些源取得等级的，等级可能要涨；把它们清空。等级比候选值更低的区块由没变的源
//!    撑着，在那里剪枝。
//! 2. **降低**：把新源/加强的源、清空区自己的源、以及清空区外圈的边界值当种子，
//!    按等级从小到大松弛（和官方 `LeveledPriorityQueue` 一样的桶队列）。只有
//!    "候选值 < 当前等级"才继续往邻居走，所以不会遍历整个视野。
//!
//! 哈希图只用来查找，不用来产生输出：变化列表按 (x, z) 排序，只包含等级真的变了的位置。

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use mc_bamboo_core::ChunkPos;

use super::{profile::Guard, table::ChunkHasherBuilder};

/// 每个块 64×64 个区块。邻居绝大多数落在同一块里，块内直接数组下标。
const BLOCK_SHIFT: i32 = 6;
const BLOCK_SIZE: i32 = 1 << BLOCK_SHIFT;
const BLOCK_MASK: i32 = BLOCK_SIZE - 1;
const BLOCK_CELLS: usize = (BLOCK_SIZE * BLOCK_SIZE) as usize;
type Block = [i32; BLOCK_CELLS];

/// 按坐标分块的稠密等级表。缺省值（limit）也照常写在数组里，只是不计入 `count`。
/// 已分配的块不回收：全世界玩家活动过的范围是有界的，回收反而让热路径多一次判断。
#[derive(Debug)]
struct LevelGrid {
    blocks: HashMap<(i32, i32), Box<Block>, ChunkHasherBuilder>,
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
        let key = (pos.x >> BLOCK_SHIFT, pos.z >> BLOCK_SHIFT);
        let Some(block) = self.blocks.get(&key) else {
            return self.default;
        };
        block[cell_index(pos)]
    }

    fn set(&mut self, pos: ChunkPos, level: i32) {
        let key = (pos.x >> BLOCK_SHIFT, pos.z >> BLOCK_SHIFT);
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

/// 记下一个区块在本次更新开始前的等级。用 i32::MIN 当"还没记"的哨兵，
/// 和 LevelGrid 一样是稠密块，不额外哈希。
fn record(grid: &mut LevelGrid, pos: ChunkPos, old: i32) {
    if grid.get(pos) == grid.default {
        grid.set(pos, old);
    }
}

fn cell_index(pos: ChunkPos) -> usize {
    ((pos.x & BLOCK_MASK) as usize) * BLOCK_SIZE as usize + (pos.z & BLOCK_MASK) as usize
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
}

impl Plane {
    pub fn new(limit: i32) -> Self {
        Self {
            sources: HashMap::default(),
            limit,
            levels: LevelGrid::new(limit),
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
        let mut folded: BTreeMap<ChunkPos, (i32, i32)> = BTreeMap::new();
        for &(pos, old, new) in changes {
            let old = old.clamp(0, limit);
            let new = new.clamp(0, limit);
            folded
                .entry(pos)
                .and_modify(|entry| entry.1 = new)
                .or_insert((old, new));
        }
        let effective: Vec<(ChunkPos, i32, i32)> = folded
            .into_iter()
            .filter(|(_, (old, new))| old != new)
            .map(|(pos, (old, new))| (pos, old, new))
            .collect();
        if effective.is_empty() {
            return Vec::new();
        }
        drop(_fold_guard);

        // 每个被碰过的区块在本次更新开始前的等级，用于最后判定"真的变了"。
        let mut original = LevelGrid::new(i32::MIN);
        // 阶段一：多源 BFS 找升高锥，只看旧的等级表，不改表。
        let _bfs_guard = Guard::new(stages[0]);
        let reset = self.collect_increases(&effective, &mut original);
        drop(_bfs_guard);

        // 改源表，把升高锥里的等级清空。
        for &(pos, _old, new) in &effective {
            if new >= limit {
                self.sources.remove(&pos);
            } else {
                self.sources.insert(pos, new);
            }
            record(&mut original, pos, self.level(pos));
        }
        for &pos in &reset {
            record(&mut original, pos, self.level(pos));
            self.levels.set(pos, limit);
        }

        // 阶段二：按等级从小到大松弛，用和官方 LeveledPriorityQueue 一样的桶队列。
        let _seed_guard = Guard::new(stages[1]);
        let mut queue = LevelQueue::new(limit);
        for &(pos, old, new) in &effective {
            if new < old && new < limit {
                queue.push(new, pos);
            }
        }
        for &pos in &reset {
            let mut boundary = self.source(pos);
            // 清空区外一圈的区块没被清空，仍保留旧等级。取最小的边界值当候选，
            // 每个清空区块只排一次，而不是给每个邻居排一次。
            for neighbor in neighbors(pos) {
                if !reset.contains(&neighbor) {
                    boundary = boundary.min(self.level(neighbor).saturating_add(1));
                }
            }
            if boundary < limit {
                queue.push(boundary, pos);
            }
        }

        drop(_seed_guard);
        let _relax_guard = Guard::new(stages[2]);
        while let Some((candidate, pos)) = queue.pop() {
            let current = self.level(pos);
            if candidate >= current {
                continue;
            }
            record(&mut original, pos, current);
            self.set_level(pos, candidate);
            let next = candidate + 1;
            if next < limit {
                for neighbor in neighbors(pos) {
                    if next < self.level(neighbor) {
                        queue.push(next, neighbor);
                    }
                }
            }
        }

        drop(_relax_guard);
        let _collect_guard = Guard::new(stages[3]);
        // 按块号、块内下标遍历，天然按 (x, z) 排序，不用再排序。
        let mut keys: Vec<_> = original.blocks.keys().copied().collect();
        keys.sort_unstable();
        let mut result = Vec::new();
        for (bx, bz) in keys {
            let block = &original.blocks[&(bx, bz)];
            for index in 0..BLOCK_CELLS {
                let old = block[index];
                if old == original.default {
                    continue;
                }
                let pos = ChunkPos::new(
                    (bx << BLOCK_SHIFT) + (index as i32 / BLOCK_SIZE),
                    (bz << BLOCK_SHIFT) + (index as i32 % BLOCK_SIZE),
                );
                let new = self.level(pos);
                if old != new {
                    result.push((pos, old, new));
                }
            }
        }
        drop(_collect_guard);
        result
    }

    /// 从全部"等级变大/被撤掉"的源一起做 BFS，找等级可能升高的区块。
    ///
    /// 逐层扩张：第 d 层的候选值是 `旧源等级 + d`，当前等级恰好等于它的区块原来靠
    /// 这些源取得等级。等级更低的地方由没变的源撑着，剪掉整个分支。每个区块只访问一次。
    fn collect_increases(
        &self,
        effective: &[(ChunkPos, i32, i32)],
        original: &mut LevelGrid,
    ) -> HashSet<ChunkPos, ChunkHasherBuilder> {
        let limit = self.limit;
        let mut seen: HashSet<ChunkPos, ChunkHasherBuilder> = HashSet::default();
        let mut reset: HashSet<ChunkPos, ChunkHasherBuilder> = HashSet::default();
        // 第 value 层里装"候选值 == value"的待访问区块。
        let mut frontier: Vec<Vec<ChunkPos>> = vec![Vec::new(); limit as usize + 1];
        for &(pos, old, new) in effective {
            if new > old && old < limit {
                frontier[old as usize].push(pos);
            }
        }
        for value in 0..limit {
            let mut index = 0;
            while index < frontier[value as usize].len() {
                let pos = frontier[value as usize][index];
                index += 1;
                if !seen.insert(pos) {
                    continue;
                }
                // 等级低于候选值：这个区块由别的源撑着，不受升高影响，整支剪掉。
                if self.level(pos) != value {
                    continue;
                }
                if reset.insert(pos) {
                    record(original, pos, value);
                }
                let next = value + 1;
                if next < limit {
                    let bucket = &mut frontier[next as usize];
                    for neighbor in neighbors(pos) {
                        bucket.push(neighbor);
                    }
                }
            }
        }
        reset
    }
}

/// 按等级分桶的 FIFO 队列，等价于官方 `LeveledPriorityQueue`：等级小的先出队，
/// 同等级按入队顺序。入队出队都是 O(1)。
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

/// 8 邻域，固定顺序（官方 `ChunkTracker.checkNeighborsAfterUpdate` 的 dx/dz 双重循环）。
fn neighbors(pos: ChunkPos) -> [ChunkPos; 8] {
    [
        ChunkPos::new(pos.x.saturating_sub(1), pos.z.saturating_sub(1)),
        ChunkPos::new(pos.x.saturating_sub(1), pos.z),
        ChunkPos::new(pos.x.saturating_sub(1), pos.z.saturating_add(1)),
        ChunkPos::new(pos.x, pos.z.saturating_sub(1)),
        ChunkPos::new(pos.x, pos.z.saturating_add(1)),
        ChunkPos::new(pos.x.saturating_add(1), pos.z.saturating_sub(1)),
        ChunkPos::new(pos.x.saturating_add(1), pos.z),
        ChunkPos::new(pos.x.saturating_add(1), pos.z.saturating_add(1)),
    ]
}
