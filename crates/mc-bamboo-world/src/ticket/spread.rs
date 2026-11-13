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
    env, mem,
    sync::atomic::{AtomicU8, Ordering},
};

#[cfg(test)]
use std::cell::Cell;

use mc_bamboo_core::ChunkPos;

use super::{
    profile::{Guard, count},
    table::ChunkHasherBuilder,
};

/// 每个块 64×64 个区块。邻居绝大多数落在同一块里，块内直接数组下标。
const BLOCK_SHIFT: i32 = 6;
const BLOCK_SIZE: i32 = 1 << BLOCK_SHIFT;
const BLOCK_MASK: i32 = BLOCK_SIZE - 1;
const BLOCK_CELLS: usize = (BLOCK_SIZE * BLOCK_SIZE) as usize;
/// 池子至少留这么多块：视距 8 的一次更新用不了几块，留 16 块省掉冷启动的分配。
#[doc(hidden)]
pub const SCRATCH_POOL_MIN: usize = 16;
/// 每张平面的硬上限：32 MiB / 每块约 53 KiB。两张平面合计不超过 64 MiB。
#[doc(hidden)]
pub const SCRATCH_POOL_CAP: usize = 32 * 1024 * 1024 / (BLOCK_CELLS * 17);
/// 近期峰值窗口：只记最近 64 次 `update` 里单次用到的最多块数，峰值过去后回收。
pub const PEAK_WINDOW: usize = 64;
/// 峰值之外留一点余量，避免刚好卡在边界时每次都重新分配一块。
pub const PEAK_MARGIN: usize = 4;
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

/// 单窗口快路径的硬上限：每张平面 1 M 格（等级用 `u8` 存，约 1 MiB）。
const DENSE_MAX_CELLS: usize = 1 << 20;
/// 每个有效源变化最多摊到这么多窗口格，超过就说明窗口里大片区域白扫。
const DENSE_CELLS_PER_CHANGE: usize = 2048;
/// 影响球并集至少覆盖窗口的 1/4 个 64×64 块，否则算稀疏场景。
const DENSE_COVERAGE_DIVISOR: usize = 4;

thread_local! {
    /// 只给测试用：强制每次 `update` 都走稠密窗口快路径（跳过稀疏/超界回退判据）。
    /// 生产构建里不存在这个变量；窗口本身的内存硬上限依然生效。
    #[cfg(test)]
    static FORCE_DENSE: Cell<bool> = const { Cell::new(false) };
    /// 累计命中快路径的平面轮数。线程局部，避免并行测试互相干扰。
    #[cfg(test)]
    static DENSE_HITS: Cell<u64> = const { Cell::new(0) };
}

/// 只给测试用：累计命中快路径的平面轮数（不受 `TICKET_PROFILE` 开关影响）。
#[cfg(test)]
pub fn dense_hits() -> u64 {
    DENSE_HITS.with(|hits| hits.get())
}

/// 测试期间强制走快路径；返回一个还原原值的 guard。
#[cfg(test)]
pub fn force_dense(on: bool) -> ForceDenseGuard {
    let previous = FORCE_DENSE.with(|flag| flag.replace(on));
    ForceDenseGuard(previous)
}

#[cfg(test)]
pub struct ForceDenseGuard(bool);

#[cfg(test)]
impl Drop for ForceDenseGuard {
    fn drop(&mut self) {
        FORCE_DENSE.with(|flag| flag.set(self.0));
    }
}

/// 0 = 还没查过环境变量，1 = 关，2 = 开。测试里可以强制打开快路径。
static DENSE_ALLOWED: AtomicU8 = AtomicU8::new(0);

/// 默认开启；`BAMBOO_TICKET_DENSE=0` 可以整条关掉，用来交替测量。
fn dense_allowed() -> bool {
    match DENSE_ALLOWED.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let on = env::var_os("BAMBOO_TICKET_DENSE").is_none_or(|value| value != "0");
            DENSE_ALLOWED.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
    }
}

#[cfg(test)]
fn force_dense_enabled() -> bool {
    FORCE_DENSE.with(|flag| flag.get())
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
    /// 最近每次更新用到的块数（环形窗口），用来算「近期峰值」。
    recent_usage: [usize; PEAK_WINDOW],
    next_usage: usize,
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
            recent_usage: [0; PEAK_WINDOW],
            next_usage: 0,
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

    /// 一轮结束：按最近 [`PEAK_WINDOW`] 次更新的峰值决定保留多少块，
    /// 多出来的直接释放。峰值滑出窗口之后池子会自己缩小；硬上限按内存算，
    /// 不随玩家历史走过的距离增长。
    fn recycle(&mut self) {
        self.recent_usage[self.next_usage] = self.blocks.len();
        self.advance_window();
        let cap = self.pool_cap();
        let keep = cap.saturating_sub(self.pool.len());
        for (_, block) in self.blocks.drain().take(keep) {
            self.pool.push(block);
        }
        self.pool.truncate(cap);
    }

    /// 这一轮没用到辅助块：窗口照样往前走，旧峰值过期后把多余的块释放掉。
    fn note_idle_round(&mut self) {
        self.recent_usage[self.next_usage] = 0;
        self.advance_window();
        if self.pool.len() > SCRATCH_POOL_MIN {
            self.pool.truncate(self.pool_cap());
        }
    }

    fn advance_window(&mut self) {
        self.next_usage = (self.next_usage + 1) % PEAK_WINDOW;
    }

    fn pool_cap(&self) -> usize {
        let peak = self.recent_usage.iter().copied().max().unwrap_or(0);
        peak.saturating_add(PEAK_MARGIN)
            .clamp(SCRATCH_POOL_MIN, SCRATCH_POOL_CAP)
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

/// 有界窗口快路径的等级缓冲：按 x 主序铺开一个坐标长方体，跨轮复用容量。
///
/// 沿用 7k 的「近 [`PEAK_WINDOW`] 轮峰值」口径：峰值过去后把过大的缓冲整块释放，
/// 免得一次大批玩家移动留下的缓冲一直占着内存。`scratch_blocks()` 按 64×64
/// 块折算上报，两个内存回收测试对两条路径都成立。
#[derive(Debug)]
struct DenseWorkspace {
    cells: Vec<u8>,
    recent_usage: [usize; PEAK_WINDOW],
    next_usage: usize,
}

impl DenseWorkspace {
    fn new() -> Self {
        Self {
            cells: Vec::new(),
            recent_usage: [0; PEAK_WINDOW],
            next_usage: 0,
        }
    }

    /// 记下这一轮铺开的格数，并在峰值过去后收掉过大的缓冲。
    fn note_round(&mut self, used: usize) {
        self.recent_usage[self.next_usage] = used;
        self.next_usage = (self.next_usage + 1) % PEAK_WINDOW;
        let peak = self.recent_usage.iter().copied().max().unwrap_or(0);
        // 留一倍滞后，稳态时不会因为刚好过一点就每轮重新分配。
        if self.cells.capacity() > peak.saturating_mul(2).saturating_add(BLOCK_CELLS) {
            self.cells = Vec::new();
        }
    }

    fn block_count(&self) -> usize {
        self.cells.capacity().div_ceil(BLOCK_CELLS)
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
    /// 稠密窗口快路径的等级缓冲。
    dense: DenseWorkspace,
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
            dense: DenseWorkspace::new(),
        }
    }

    /// 当前等级；没有记录就是无票。
    pub fn level(&self, pos: ChunkPos) -> i32 {
        self.levels.get(pos)
    }

    /// 这一轮没有源变化：推进两条路径各自的近期峰值窗口。
    pub fn note_idle_round(&mut self) {
        self.scratch.note_idle_round();
        self.dense.note_round(0);
    }

    /// 表里非默认等级的区块数。
    pub fn len(&self) -> usize {
        self.levels.count
    }

    /// 辅助结构占的块数，测试用来检查长期移动不会让内存一直涨。
    #[doc(hidden)]
    pub fn scratch_blocks(&self) -> usize {
        self.scratch.block_count() + self.dense.block_count()
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
            self.scratch.note_idle_round();
            self.dense.note_round(0);
            return Vec::new();
        }
        drop(_fold_guard);

        // 影响球并集落在一个有界窗口里时，用一次稠密光栅扫描替代两阶段算法的四遍遍历。
        // 不满足判据就原样走回退路径；两条路径算的是同一个不动点，见下文注释。
        if let Some(result) = self.try_dense_update(&effective) {
            self.buffers.effective = effective;
            return result;
        }
        count(super::profile::C_DENSE_FALLBACK, 1);
        self.dense.note_round(0);
        self.scratch.begin();

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

    /// 有界窗口快路径：把影响球并集铺成一张稠密 `u8` 表，两遍光栅扫描直接算新不动点。
    ///
    /// 官方 `DynamicGraphMinFixedPoint` 的稳定解是
    /// `L(p) = min( min_s(S(s) + chebyshev(s, p)), limit )`（字节码 `getComputedLevel` /
    /// `computeLevelFromNeighbor` / `runUpdates` 已核）。等级为 `v` 的源只可能改变
    /// `chebyshev <= limit - 1 - v` 的格子，所以每个有效变化的 `old`、`new` 取较小者
    /// 得出影响球半径，包围盒再加两格余量。加余量后窗口最外一圈（halo）上的
    /// `L_old == L_new`：任何外部源到窗口内的最短 8 邻路径必经这一圈，而坐标长方体
    /// 内部的最短路径不会再出去。把 halo 钉在旧值、窗口内部按**新**源初始化，
    /// 算出的 min-plus 不动点就是全局新不动点。
    ///
    /// 切比雪夫最短路可以按 x 单调取，所以一遍 x↑z↑（看上/左上/右上/左 +1）
    /// 加一遍 x↓z↓（看下/左下/右下/右 +1）就收敛，不需要优先队列。窗口外一格不变，
    /// 按 x 主序扫一遍旧/新值即可产出恰好全部变化、且天然按 `(x, z)` 有序。
    ///
    /// 返回 `None` 表示这轮不满足判据，调用方原样走两阶段回退路径。
    fn try_dense_update(
        &mut self,
        effective: &[(ChunkPos, i32, i32)],
    ) -> Option<Vec<(ChunkPos, i32, i32)>> {
        if !dense_allowed() {
            return None;
        }
        let limit = self.limit;
        let limit_u8 = u8::try_from(limit).ok()?;
        let forced = {
            #[cfg(test)]
            {
                force_dense_enabled()
            }
            #[cfg(not(test))]
            {
                false
            }
        };
        // 每个变化源的影响球，坐标用 i64 算，极端输入也不会溢出。
        let mut x0 = i64::MAX;
        let mut x1 = i64::MIN;
        let mut z0 = i64::MAX;
        let mut z1 = i64::MIN;
        let mut coverage = 0usize;
        for &(pos, old, new) in effective {
            let radius = i64::from(limit) - 1 - i64::from(old.min(new));
            let cx = i64::from(pos.x);
            let cz = i64::from(pos.z);
            x0 = x0.min(cx - radius - 2);
            x1 = x1.max(cx + radius + 2);
            z0 = z0.min(cz - radius - 2);
            z1 = z1.max(cz + radius + 2);
            // 影响球覆盖的 64×64 块数，用来判稀疏。
            let bx0 = (cx - radius).div_euclid(BLOCK_SIZE as i64);
            let bx1 = (cx + radius).div_euclid(BLOCK_SIZE as i64);
            let bz0 = (cz - radius).div_euclid(BLOCK_SIZE as i64);
            let bz1 = (cz + radius).div_euclid(BLOCK_SIZE as i64);
            coverage = coverage.saturating_add(
                (bx1 - bx0 + 1).unsigned_abs() as usize * (bz1 - bz0 + 1).unsigned_abs() as usize,
            );
        }
        // 窗口就是包围盒本身；等级表按 64 分块，但拷入/拷出用块游标，不需要对齐。
        // 对齐只会把窗口白白放大到四倍。
        let width = (x1 - x0 + 1) as u64;
        let height = (z1 - z0 + 1) as u64;
        let area = width
            .checked_mul(height)
            .and_then(|n| usize::try_from(n).ok())?;
        let window_blocks = width.div_ceil(BLOCK_SIZE as u64) as usize
            * height.div_ceil(BLOCK_SIZE as u64) as usize;
        // 强制开关只跳过「稀疏 / 每变化源面积」两条启发式判据；窗口内存硬上限仍生效。
        if area > DENSE_MAX_CELLS
            || (!forced
                && (area > DENSE_CELLS_PER_CHANGE.saturating_mul(effective.len())
                    || coverage.saturating_mul(DENSE_COVERAGE_DIVISOR) < window_blocks))
        {
            return None;
        }
        let (x0, z0, x1, z1) = (
            i32::try_from(x0).ok()?,
            i32::try_from(z0).ok()?,
            i32::try_from(x1).ok()?,
            i32::try_from(z1).ok()?,
        );
        let width = width as usize;
        let height = height as usize;
        let _setup_guard = Guard::new(super::profile::DENSE_SETUP);
        count(super::profile::C_DENSE_HIT, 1);
        count(super::profile::C_DENSE_CELLS, area as u64);
        #[cfg(test)]
        DENSE_HITS.with(|hits| hits.set(hits.get() + 1));

        let mut cells = mem::take(&mut self.dense.cells);
        cells.clear();
        cells.resize(area, limit_u8);
        // 内部格按**新**源等级初始化：source 表里除了 effective 涉及的位置都还是新值，
        // effective 覆盖的位置以它的最终值为准（含被撤掉的源，不能留在表里）。
        for (&pos, &level) in &self.sources {
            if pos.x > x0 && pos.x < x1 && pos.z > z0 && pos.z < z1 {
                let index = (pos.x - x0) as usize * height + (pos.z - z0) as usize;
                cells[index] = cells[index].min(level.clamp(0, limit) as u8);
            }
        }
        for &(pos, _old, new) in effective {
            if pos.x > x0 && pos.x < x1 && pos.z > z0 && pos.z < z1 {
                let index = (pos.x - x0) as usize * height + (pos.z - z0) as usize;
                cells[index] = if new >= limit {
                    // 源被撤掉，或降到无票等级：该位置从 limit 重新被邻居传播。
                    limit_u8
                } else {
                    new.clamp(0, limit) as u8
                };
            }
        }
        // halo 钉在旧等级上；按行读，用块游标跨块才查一次哈希。
        {
            let mut cursor = self.levels.cursor();
            for ix in 0..width {
                for iz in 0..height {
                    if ix == 0 || ix + 1 == width || iz == 0 || iz + 1 == height {
                        let pos = ChunkPos::new(x0 + ix as i32, z0 + iz as i32);
                        cells[ix * height + iz] = cursor.get(pos).clamp(0, limit) as u8;
                    }
                }
            }
        }
        drop(_setup_guard);
        // 两遍光栅扫描：3×3 邻域的切比雪夫距离变换，对按 x 单调的最短路是精确的。
        let _sweep_guard = Guard::new(super::profile::DENSE_SWEEP);
        for ix in 1..width - 1 {
            for iz in 1..height - 1 {
                let index = ix * height + iz;
                let best = cells[index]
                    .min(cells[index - height - 1].saturating_add(1))
                    .min(cells[index - height].saturating_add(1))
                    .min(cells[index - height + 1].saturating_add(1))
                    .min(cells[index - 1].saturating_add(1));
                cells[index] = best.min(limit_u8);
            }
        }
        for ix in (1..width - 1).rev() {
            for iz in (1..height - 1).rev() {
                let index = ix * height + iz;
                let best = cells[index]
                    .min(cells[index + height - 1].saturating_add(1))
                    .min(cells[index + height].saturating_add(1))
                    .min(cells[index + height + 1].saturating_add(1))
                    .min(cells[index + 1].saturating_add(1));
                cells[index] = best.min(limit_u8);
            }
        }
        drop(_sweep_guard);
        let _commit_guard = Guard::new(super::profile::DENSE_COMMIT);

        // 应用新源表（和回退路径的顺序一致），再按 x↑z↑ 扫一遍收变化并写回持久表。
        for &(pos, _old, new) in effective {
            if new >= limit {
                self.sources.remove(&pos);
            } else {
                self.sources.insert(pos, new);
            }
        }
        let mut result = Vec::new();
        {
            let mut cursor = self.levels.cursor();
            for ix in 1..width - 1 {
                for iz in 1..height - 1 {
                    let pos = ChunkPos::new(x0 + ix as i32, z0 + iz as i32);
                    let index = ix * height + iz;
                    let old = cursor.get(pos);
                    let new = i32::from(cells[index]).min(limit);
                    if old != new {
                        result.push((pos, old, new));
                    }
                }
            }
        }
        for &(pos, old, new) in &result {
            debug_assert_ne!(old, new);
            self.levels.set(pos, new);
        }
        debug_assert!(
            result
                .windows(2)
                .all(|w| (w[0].0.x, w[0].0.z) < (w[1].0.x, w[1].0.z))
        );
        count(super::profile::C_CHANGES, result.len() as u64);
        let used = cells.len();
        self.dense.cells = cells;
        self.dense.note_round(used);
        drop(_commit_guard);
        // 快路径没用到两阶段辅助块；峰值窗口照样推进，旧峰值过期后回收。
        self.scratch.note_idle_round();
        Some(result)
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
