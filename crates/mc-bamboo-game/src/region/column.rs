//! 一个区块位置上这一层知道的全部内容，见 `docs/world-model-design.md` 第 4.2 节。
//!
//! [`RegionColumn`] 是 region 之间搬运的唯一单位：方块和两层光照装在
//! [`mc_bamboo_world::column::ChunkColumn`] 里，其余带位置的状态各装一份按区块的盒子。
//! region 里只留可以重建的索引和随时能丢的缓存。
//!
//! 区块平面和多 region 是后面的步骤，现在还没有调用方，只有测试会用到这些接口。

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    ops::Bound,
    sync::OnceLock,
};

use mc_bamboo_blocks::{ChunkBlockEntities, ChunkRedstone, ChunkTicks, entity::ChunkEntities};
use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_protocol::components::ComponentPatch;
use mc_bamboo_world::{column::ChunkColumn, director::ChunkBundle};

use crate::{
    command_block::CommandBlockData, container::ContainerEntry, lectern::Book, profiler::SubPhase,
    region::Region,
};

/// 剖析器用的全局计时开关：连接层没有 `Profiler`，`is_pristine` 在世界线程里
/// 逐段比模板，用一对原子计数器把耗时报进 `pristine_check` 子段。
static PRISTINE_NANOS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 现在不能搬区块束：光照没算完，或者某张表正在 tick 中途。
///
/// 调用方要等到 tick 边界再来，见 [`Region::ready_to_move_columns`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotReady;

impl fmt::Display for NotReady {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the region is not ready to move chunk columns")
    }
}

impl std::error::Error for NotReady {}

/// 一个区块位置上这一层知道的全部内容。
///
/// 区块包缓存、待发的方块/光照变化、按玩家索引的菜单和链路都不在这里：前者可以随时丢，
/// 后者跟着玩家走。方块实体里的 `dead` 条目、开着的菜单指向的位置、告示牌的编辑者
/// 也不单独处理 —— `dead` 和 `recheck_at` 跟着容器条目进盒子，菜单和 `lecterns.views`
/// 留在 region 按玩家索引，告示牌内容（含 `editor`）本来就在 [`ChunkColumn`] 里。
/// 这样「玩家和他附近的区块总是一起搬」就成了本类型唯一需要的跨表不变量：
/// 只要满足它，菜单指向的容器、开合计数和 `dead` 标记放回后仍然对得上。
/// 跨区块双箱的两半各按自己的 `pos.chunk()` 装进不同盒子，所以一批要搬的区块还得是
/// 一整片互相相邻的区块，和下层 `sectionStates` 邻居段计数的要求一致。
#[derive(Debug)]
pub struct RegionColumn {
    /// 方块、高度图、方块实体类型、告示牌、`update tag` 和两层光照。
    column: Box<ChunkColumn>,
    /// 方块和流体的计划刻。`None` 表示这个区块没排着刻。
    ticks: Option<ChunkTicks>,
    /// 活塞、熔炉、漏斗这类会动的方块实体。
    block_entities: ChunkBlockEntities,
    /// 比较器输出、书架槽位、讲台页码、火把开关记录。
    redstone: ChunkRedstone,
    /// 容器表按区块切出来的那些条目。
    containers: BTreeMap<BlockPos, PackedContainer>,
    /// 方块实体从物品带来的组件。
    block_items: BTreeMap<BlockPos, ComponentPatch>,
    /// 命令方块里保存的命令。
    command_blocks: BTreeMap<BlockPos, CommandBlockData>,
    /// 讲台上的书。
    books: BTreeMap<BlockPos, Book>,
    /// 这个区块里的下落方块、点燃的 TNT 和掉落物。
    entities: ChunkEntities,
}

/// 组装盒子之前攒齐的几张表。`ChunkBlockEntities` / `ChunkRedstone` 没有 `Default`，
/// 用 `Option` 占位，凑齐了才建 [`RegionColumn`]，不会造出又丢掉的空盒子。
#[derive(Default)]
struct Parts {
    column: Option<Box<ChunkColumn>>,
    ticks: Option<ChunkTicks>,
    block_entities: Option<ChunkBlockEntities>,
    redstone: Option<ChunkRedstone>,
    containers: BTreeMap<BlockPos, PackedContainer>,
    block_items: BTreeMap<BlockPos, ComponentPatch>,
    command_blocks: BTreeMap<BlockPos, CommandBlockData>,
    books: BTreeMap<BlockPos, Book>,
    entities: Option<ChunkEntities>,
}

impl Parts {
    fn finish(self) -> Box<RegionColumn> {
        Box::new(RegionColumn {
            column: self.column.expect("区块柱已经取出"),
            ticks: self.ticks,
            block_entities: self.block_entities.expect("方块实体盒子已经取出"),
            redstone: self.redstone.expect("红石记忆盒子已经取出"),
            containers: self.containers,
            block_items: self.block_items,
            command_blocks: self.command_blocks,
            books: self.books,
            entities: self.entities.expect("实体盒子已经取出"),
        })
    }
}

/// 容器条目里 `recheck_at` 换成相对剩余刻数（绝对时刻在盒子里清掉），
/// 跨到时钟不同的 region 时保持剩余寿命。其余字段原样带走。
#[derive(Debug, Clone)]
pub struct PackedContainer {
    pub(crate) entry: ContainerEntry,
    pub(crate) recheck_in: Option<u64>,
}

impl PackedContainer {
    /// 存档解码用：官方不存开合计数和 `recheck_at`，按新建条目填默认值。
    pub fn new(entry: ContainerEntry) -> Self {
        Self {
            entry,
            recheck_in: None,
        }
    }

    /// 这一格的容器内容。
    pub fn entry(&self) -> &ContainerEntry {
        &self.entry
    }
}

/// 从存档组装一个区块束所需的部件。
#[derive(Debug)]
pub struct RegionColumnParts {
    pub column: Box<ChunkColumn>,
    pub ticks: Option<ChunkTicks>,
    pub block_entities: ChunkBlockEntities,
    pub redstone: ChunkRedstone,
    pub containers: BTreeMap<BlockPos, PackedContainer>,
    pub block_items: BTreeMap<BlockPos, ComponentPatch>,
    pub command_blocks: BTreeMap<BlockPos, CommandBlockData>,
    pub books: BTreeMap<BlockPos, Book>,
    pub entities: ChunkEntities,
}

/// [`RegionColumn::parts`] 的只读视图。
pub struct RegionColumnView<'a> {
    pub column: &'a ChunkColumn,
    pub ticks: Option<&'a ChunkTicks>,
    pub block_entities: &'a ChunkBlockEntities,
    pub redstone: &'a ChunkRedstone,
    pub containers: &'a BTreeMap<BlockPos, PackedContainer>,
    pub block_items: &'a BTreeMap<BlockPos, ComponentPatch>,
    pub command_blocks: &'a BTreeMap<BlockPos, CommandBlockData>,
    pub books: &'a BTreeMap<BlockPos, Book>,
    pub entities: &'a ChunkEntities,
}

/// 判断「原样」用的生成器输出盒子。全局缓存一份，构造时算一次模板光照。
static TEMPLATE_COLUMN: OnceLock<ChunkColumn> = OnceLock::new();

fn template_column() -> &'static ChunkColumn {
    TEMPLATE_COLUMN.get_or_init(ChunkColumn::from_template)
}

impl ChunkBundle for Box<RegionColumn> {
    fn is_pristine(&self) -> bool {
        let started = std::time::Instant::now();
        let result = (**self).is_pristine();
        let elapsed = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        PRISTINE_NANOS.fetch_add(elapsed, std::sync::atomic::Ordering::Relaxed);
        result
    }
}

/// 取走并清零 `is_pristine` 累计的纳秒，给世界线程的剖析器用。
pub(crate) fn take_pristine_cost() -> u64 {
    PRISTINE_NANOS.swap(0, std::sync::atomic::Ordering::Relaxed)
}

impl ChunkBundle for RegionColumn {
    /// 盒子还能不能丢掉、下次用模板重新生成。
    ///
    /// 方块、告示牌、`update tag` 和两层光照要和生成器输出逐项相同，
    /// 并且这一层知道的所有上层内容都得是空的（计划刻、方块实体、红石记忆、
    /// 容器、物品组件、命令方块、讲台上的书、实体）。
    /// 邻居的光会照进这一柱，只比方块不够；`ChunkColumn::is_pristine` 把光照一起比了。
    /// 拿不准时返回 `false`，让 Director 把盒子整份停放。
    fn is_pristine(&self) -> bool {
        self.ticks.is_none()
            && self.block_entities.is_empty()
            && self.redstone.is_empty()
            && self.containers.is_empty()
            && self.block_items.is_empty()
            && self.command_blocks.is_empty()
            && self.books.is_empty()
            && self.entities.is_empty()
            && self.column.is_pristine(template_column())
    }
}

// 区块束要被 move 到别的线程（设计稿 4.4）。
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<RegionColumn>();
    assert_send::<Box<RegionColumn>>();
    assert_send::<NotReady>();
};

impl RegionColumn {
    /// 从生成器模板造一个空的区块束：上层的所有盒子都是空的。
    /// 已加载模式用它加载一个从没来过的位置。
    pub(crate) fn from_template() -> Self {
        Self {
            column: Box::new(ChunkColumn::from_template()),
            ticks: None,
            block_entities: ChunkBlockEntities::default(),
            redstone: ChunkRedstone::default(),
            containers: BTreeMap::new(),
            block_items: BTreeMap::new(),
            command_blocks: BTreeMap::new(),
            books: BTreeMap::new(),
            entities: ChunkEntities::default(),
        }
    }

    /// 从部件造一个区块束。存档解码和 Director 的加载流水线用；字段仍保持私有。
    pub fn from_parts(parts: RegionColumnParts) -> Box<Self> {
        Box::new(Self {
            column: parts.column,
            ticks: parts.ticks,
            block_entities: parts.block_entities,
            redstone: parts.redstone,
            containers: parts.containers,
            block_items: parts.block_items,
            command_blocks: parts.command_blocks,
            books: parts.books,
            entities: parts.entities,
        })
    }

    /// 存档编码用的只读部件视图。
    pub fn parts(&self) -> RegionColumnView<'_> {
        RegionColumnView {
            column: &self.column,
            ticks: self.ticks.as_ref(),
            block_entities: &self.block_entities,
            redstone: &self.redstone,
            containers: &self.containers,
            block_items: &self.block_items,
            command_blocks: &self.command_blocks,
            books: &self.books,
            entities: &self.entities,
        }
    }

    /// 这个区块里的方块和光照，给测试和以后组装区块包用。
    #[cfg(test)]
    pub(crate) fn column(&self) -> &ChunkColumn {
        &self.column
    }

    /// 有没有排着计划刻，给测试用。
    #[cfg(test)]
    pub(crate) fn has_ticks(&self) -> bool {
        self.ticks.is_some()
    }

    /// 盒子里按方块位置索引的几张表，给测试用。
    #[cfg(test)]
    pub(crate) fn containers(&self) -> &BTreeMap<BlockPos, PackedContainer> {
        &self.containers
    }

    /// 盒子里这一格的容器条目，给测试用。
    #[cfg(test)]
    pub(crate) fn container(&self, pos: BlockPos) -> Option<&ContainerEntry> {
        self.containers.get(&pos).map(|packed| &packed.entry)
    }

    #[cfg(test)]
    pub(crate) fn block_items(&self) -> &BTreeMap<BlockPos, ComponentPatch> {
        &self.block_items
    }

    #[cfg(test)]
    pub(crate) fn command_blocks(&self) -> &BTreeMap<BlockPos, CommandBlockData> {
        &self.command_blocks
    }

    #[cfg(test)]
    pub(crate) fn books(&self) -> &BTreeMap<BlockPos, Book> {
        &self.books
    }
}

/// 红石火把的开关记录带世界时间，取放时按 `BlockTicks::now()` 换算相对年龄。
/// 等模拟时间和世界时间分开（设计稿 6.3）时这里要改成区块自己的时间。
impl Region {
    /// 现在能不能搬区块束：在 tick 边界、光照已经全部算完、各张表都不在 tick 中途。
    ///
    /// 只是只读地问一声，不会偷偷把光照跑完。一次要搬几张表的调用方可以先问，
    /// 好让任何一个前提不满足时整批都不动。
    #[allow(dead_code)] // 区块平面接上之前没有调用方
    pub(crate) fn ready_to_move_columns(&self) -> bool {
        self.chunks.ready_to_move_columns()
            && self.block_ticks.ready_to_move_chunks()
            && self.block_entities.ready_to_move_chunks()
            && self.entities.ready_to_move_chunks()
    }

    /// 一次取出一批区块。任何一个前提不满足时整批都不动。
    ///
    /// 四张按方块位置索引的表（`containers`、`block_items`、`command_blocks`、讲台上的书）
    /// 仍按整张 `BTreeMap` 存，这里按 `BlockPos` 的 x 带（`Ord` 以 x 为第一关键字）
    /// 用 `range` 把扫描限制在要搬的区块那 16 格宽的 x 带里，再按区块坐标过滤；
    /// **表本身和遍历顺序都不变**，所以发包顺序不变。扫描开销和 x 带里的条目数成正比，
    /// 和 [`mc_bamboo_blocks::BlockTicks::take_chunks`] 一样要求调用方把一次要搬的区块
    /// 攒成一批，每刻最多调一次。
    ///
    /// 取出后这些坐标在 `ChunkStore` 里读成模板；`block_ticks` / `block_entities` / `redstone`
    /// 里属于它们的条目、区块包缓存和还没发出去的光照/方块变化一并丢掉。别的区块不受影响。
    /// 发区块包、`forget_level_chunk` 和重算视图都是调用方的事，这里不通知客户端。
    ///
    /// 计划刻按相对延迟装盒，容器条目的 `recheck_at` 按相对剩余刻数装盒，
    /// 所以放进一个模拟时间不同的 region 也能保持剩余寿命。
    /// 只在 tick 边界调用：红石记忆里「这次右键要放上的书」是一次交互内部的瞬时量，
    /// 边界上一定是 `None`，它不跟着区块走。
    #[allow(dead_code)] // 区块平面接上之前没有调用方
    pub(crate) fn take_columns(
        &mut self,
        chunks: &BTreeSet<ChunkPos>,
    ) -> Result<BTreeMap<ChunkPos, Box<RegionColumn>>, NotReady> {
        if !self.ready_to_move_columns() {
            return Err(NotReady);
        }
        if chunks.is_empty() {
            return Ok(BTreeMap::new());
        }

        // 前提全部检查完，从这里开始每一步都不会失败。
        let now = self.block_ticks.now();
        self.profiler.start_sub(SubPhase::ReclaimStorage);
        let mut parts: BTreeMap<ChunkPos, Parts> = chunks
            .iter()
            .map(|&pos| {
                let column = self
                    .chunks
                    .take_column(pos)
                    .unwrap_or_else(|_| unreachable!("前提已经检查过"));
                (
                    pos,
                    Parts {
                        column: Some(column),
                        ..Parts::default()
                    },
                )
            })
            .collect();
        self.profiler.end_sub();

        // 四张按位置的表：按 x 带扫，抽出的条目按区块分给各自的盒子。
        self.profiler.start_sub(SubPhase::ReclaimTables);
        take_positions(&mut self.containers, chunks, |pos, mut entry| {
            let recheck_in = entry.recheck_at.take().map(|at| self.tick.remaining(at));
            let packed = PackedContainer { recheck_in, entry };
            parts
                .get_mut(&pos.chunk())
                .unwrap()
                .containers
                .insert(pos, packed);
        });
        take_positions(&mut self.block_items, chunks, |pos, patch| {
            parts
                .get_mut(&pos.chunk())
                .unwrap()
                .block_items
                .insert(pos, patch);
        });
        take_positions(&mut self.command_blocks, chunks, |pos, data| {
            parts
                .get_mut(&pos.chunk())
                .unwrap()
                .command_blocks
                .insert(pos, data);
        });
        take_positions(&mut self.lecterns.books, chunks, |pos, book| {
            parts.get_mut(&pos.chunk()).unwrap().books.insert(pos, book);
        });

        self.profiler.end_sub();

        // 计划刻、方块实体和红石记忆各自用下层的按区块接口。各张表互不依赖，
        // 分几次按坐标有序地走同样的位置集合，测量时才能把各表的耗时分开。
        self.profiler.start_sub(SubPhase::ReclaimTicks);
        let mut tick_boxes = self
            .block_ticks
            .take_chunks(chunks)
            .unwrap_or_else(|_| unreachable!("前提已经检查过"));
        for (&pos, part) in &mut parts {
            part.ticks = tick_boxes.remove(&pos);
        }
        self.profiler.end_sub();
        self.profiler.start_sub(SubPhase::ReclaimBlockEntities);
        for (&pos, part) in &mut parts {
            part.block_entities = Some(
                self.block_entities
                    .take_chunk(pos)
                    .unwrap_or_else(|_| unreachable!("前提已经检查过")),
            );
        }
        self.profiler.end_sub();
        self.profiler.start_sub(SubPhase::ReclaimTables);
        for (&pos, part) in &mut parts {
            part.redstone = Some(self.redstone.take_chunk(pos, now));
        }
        self.profiler.end_sub();
        self.profiler.start_sub(SubPhase::ReclaimEntities);
        for (&pos, part) in &mut parts {
            part.entities = Some(
                self.entities
                    .take_chunk(pos)
                    .unwrap_or_else(|_| unreachable!("前提已经检查过")),
            );
        }
        self.profiler.end_sub();

        // 丢掉属于这些区块的、还没发出去的变化和区块包的缓存。
        self.profiler.start_sub(SubPhase::ReclaimTables);
        for &pos in chunks {
            self.light_changes.remove(&pos);
        }
        self.block_changes
            .changed
            .retain(|pos| !chunks.contains(&pos.chunk()));
        self.chunk_cache.retain(|pos| !chunks.contains(&pos));
        self.profiler.end_sub();

        Ok(parts
            .into_iter()
            .map(|(pos, part)| (pos, part.finish()))
            .collect())
    }

    /// 放入一个区块束。失败时把盒子原样还回来，region 一点没变。
    ///
    /// 前提和 [`Self::take_columns`] 一样；调用方保证目标位置原本没有这个区块的内容
    /// （先 [`Self::take_columns`]，或者这是个新 region）。
    /// 计划刻按这张表自己的 `now` 从相对延迟算回绝对时刻，容器条目的 `recheck_at`
    /// 按 region 自己的 `tick` 算回绝对时刻，所以两边模拟时间不同也能对上。
    /// 放入不产生发给客户端的方块/光照变化，也不发区块包；区块包缓存里这个坐标的条目
    /// 一并丢掉，下层会用目标 store 自己的版本号重新编码。
    #[allow(dead_code)] // 区块平面接上之前没有调用方
    pub(crate) fn insert_column(
        &mut self,
        pos: ChunkPos,
        column: Box<RegionColumn>,
    ) -> Result<(), (NotReady, Box<RegionColumn>)> {
        if !self.ready_to_move_columns() {
            return Err((NotReady, column));
        }
        self.insert_column_unchecked(pos, column);
        Ok(())
    }

    /// 不检查前提地放入一个区块束，前提由调用方保证。
    fn insert_column_unchecked(&mut self, pos: ChunkPos, column: Box<RegionColumn>) {
        let now = self.block_ticks.now();
        let RegionColumn {
            column: world,
            ticks,
            block_entities,
            redstone,
            containers,
            block_items,
            command_blocks,
            books,
            entities,
        } = *column;
        self.profiler.start_sub(SubPhase::DeliverStorage);
        self.chunks
            .insert_column(pos, world)
            .unwrap_or_else(|_| unreachable!("前提已经检查过"));
        self.profiler.end_sub();

        self.profiler.start_sub(SubPhase::DeliverTables);
        for (block_pos, mut packed) in containers {
            packed.entry.recheck_at = packed.recheck_in.map(|remaining| self.tick.plus(remaining));
            self.containers.insert(block_pos, packed.entry);
        }
        self.block_items.extend(block_items);
        self.command_blocks.extend(command_blocks);
        self.lecterns.books.extend(books);
        if let Some(ticks) = ticks {
            self.block_ticks.insert_chunk(ticks);
        }
        self.block_entities.insert_chunk(block_entities);
        self.redstone.insert_chunk(redstone, now);
        self.entities.insert_chunk(entities);

        self.light_changes.remove(&pos);
        self.block_changes
            .changed
            .retain(|changed| changed.chunk() != pos);
        self.chunk_cache.retain(|cached| cached != pos);
        self.profiler.end_sub();
    }

    /// Director 交付一个完整的区块束到 `pos`（已加载模式）。
    ///
    /// 把世界层和上层内容装进 region（按目标世界的方块重算段状态），
    /// 再把这个位置登记为已加载。
    ///
    /// 交付只往各张表里**插入**，不需要「没有实体被借出去」这个取出才有的前提；
    /// [`Self::take_columns`] 仍用完整的 `ready_to_move_columns`。
    pub(crate) fn deliver_column(
        &mut self,
        pos: ChunkPos,
        column: Box<RegionColumn>,
    ) -> Result<(), (NotReady, Box<RegionColumn>)> {
        let ready = self.chunks.ready_to_move_columns()
            && self.block_ticks.ready_to_move_chunks()
            && self.block_entities.ready_to_move_chunks();
        if !ready {
            return Err((NotReady, column));
        }
        // 先登记已加载，再插盒子：`insert_column` 里的 `rebind_area`
        // 按「这一柱已经在了」把 3×3 段状态一次算对，`mark_loaded` 不再重算第二遍。
        self.chunks.mark_loaded(pos);
        self.insert_column_unchecked(pos, column);
        Ok(())
    }

    /// Director 收回一批区块（已加载模式）。返回盒子与各自的代数。
    pub(crate) fn reclaim_columns(
        &mut self,
        chunks: &BTreeSet<ChunkPos>,
    ) -> Result<BTreeMap<ChunkPos, Box<RegionColumn>>, NotReady> {
        let boxes = self.take_columns(chunks)?;
        self.profiler.start_sub(SubPhase::ReclaimStorage);
        for &pos in chunks {
            let _ = self.chunks.unload(pos);
        }
        self.profiler.end_sub();
        Ok(boxes)
    }
}

/// 把 `table` 里位置落在 `chunks` 里的条目移出来，逐个交给 `emit`。
///
/// 只看每个区块所在的那条 16 格宽的 x 带，再逐个移出，不扫整张表，条目也不复制。
fn take_positions<T>(
    table: &mut BTreeMap<BlockPos, T>,
    chunks: &BTreeSet<ChunkPos>,
    mut emit: impl FnMut(BlockPos, T),
) {
    // `BlockPos` 的 `Ord` 是 (x, y, z)：同一个 x 带里 z 不是连续的一段，所以先按 x 圈出
    // 16 格宽的带，再按 `pos.chunk() == chunk` 过滤 z。
    let mut taken: Vec<BlockPos> = Vec::new();
    for &chunk in chunks {
        // 区块坐标可能来自客户端，乘 16 会溢出时用饱和算术，不让它触发 panic。
        let first_x = chunk.x.saturating_mul(16);
        let from = BlockPos::new(first_x, i32::MIN, i32::MIN);
        let to = BlockPos::new(first_x.saturating_add(16), i32::MIN, i32::MIN);
        taken.extend(
            table
                .range((Bound::Included(from), Bound::Excluded(to)))
                .map(|(&pos, _)| pos)
                .filter(|pos| pos.chunk() == chunk),
        );
    }
    for pos in taken {
        if let Some(value) = table.remove(&pos) {
            emit(pos, value);
        }
    }
}
