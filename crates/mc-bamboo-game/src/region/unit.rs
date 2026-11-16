//! 单元的拆合：把一组区块的带位置状态整盒拆出去、跑完再合回来。
//!
//! 见 `docs/multi-region-tick-design.md` 4.8：现在只有 region 一个单元，存储仍是
//! 全局形状；这些接口只在单元拓扑变化时用，不在 tick 热路径上。实体表和
//! `entity_tracking` 的玩家配对这次不拆，等第 5 步的岛再说。

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_blocks::{BlockEntities, BlockEvents, BlockTicks, RedstoneMemory};
use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_protocol::components::ComponentPatch;

use crate::{
    command_block::CommandBlockData, container::ContainerEntry, lectern::Book, region::Region,
};

/// 一个单元自己的那份带位置状态。现在没有人并行跑它，只给测试和下一步用。
pub(crate) struct RegionUnit {
    pub(crate) unit: u64,
    pub(crate) containers: BTreeMap<BlockPos, ContainerEntry>,
    pub(crate) block_items: BTreeMap<BlockPos, ComponentPatch>,
    pub(crate) command_blocks: BTreeMap<BlockPos, CommandBlockData>,
    pub(crate) books: BTreeMap<BlockPos, Book>,
    pub(crate) block_events: BlockEvents,
    pub(crate) block_entities: BlockEntities,
    pub(crate) redstone: RedstoneMemory,
    pub(crate) ticks: BlockTicks,
}

/// 把 `table` 里位置落在 `chunks` 里的条目移出来，收集成一张按位置有序的小表。
///
/// 只看每个区块所在的那条 16 格宽的 x 带，和 `take_columns` 的取盒路径同一套做法；
/// 这是拆合时才走的冷路径，不追求比扫表更快。
fn split_positions<T>(
    table: &mut BTreeMap<BlockPos, T>,
    chunks: &BTreeSet<ChunkPos>,
) -> BTreeMap<BlockPos, T> {
    let mut out = BTreeMap::new();
    for &chunk in chunks {
        // 区块坐标可能来自客户端，乘 16 会溢出时用饱和算术，不让它触发 panic。
        let first_x = chunk.x.saturating_mul(16);
        let from = BlockPos::new(first_x, i32::MIN, i32::MIN);
        let to = BlockPos::new(first_x.saturating_add(16), i32::MIN, i32::MIN);
        let taken: Vec<BlockPos> = table
            .range((
                std::ops::Bound::Included(from),
                std::ops::Bound::Excluded(to),
            ))
            .map(|(&pos, _)| pos)
            .filter(|pos| pos.chunk() == chunk)
            .collect();
        for pos in taken {
            if let Some(value) = table.remove(&pos) {
                out.insert(pos, value);
            }
        }
    }
    out
}

impl Region {
    /// 拆出 `chunks` 这些区块的带位置状态，造出另一个单元。只在 tick 边界调用。
    ///
    /// 四张表按 x 带取；计划刻、方块事件、方块实体、红石记忆各自按区块分表拆。
    /// 菜单、`lecterns.views` / `pending` 按玩家索引，留在 region；跨区块双箱
    /// 和开着的菜单要求相邻区块一起拆（和 `take_columns` 同一条不变量）。
    /// 关注关系跨区块时 `BlockEntities::watchers_cross_chunks` 会报告，下一步
    /// 划单元时必须把两侧合成一个单元。
    #[allow(dead_code, reason = "单元并行建起来之前只有测试调用")]
    pub(crate) fn split_off_unit(
        &mut self,
        chunks: &BTreeSet<ChunkPos>,
        unit: u64,
    ) -> Result<RegionUnit, super::column::NotReady> {
        if !self.ready_to_move_columns() {
            return Err(super::column::NotReady);
        }
        let containers = split_positions(&mut self.containers, chunks);
        let block_items = split_positions(&mut self.block_items, chunks);
        let command_blocks = split_positions(&mut self.command_blocks, chunks);
        let books = split_positions(&mut self.lecterns.books, chunks);
        let block_events = self.block_events.split_off(chunks);
        let block_entities = self
            .block_entities
            .split_off(chunks, unit)
            .map_err(|_| super::column::NotReady)?;
        let redstone = self.redstone.split_off(chunks);
        let ticks = self.block_ticks.split_off(chunks, unit);
        Ok(RegionUnit {
            unit,
            containers,
            block_items,
            command_blocks,
            books,
            block_events,
            block_entities,
            redstone,
            ticks,
        })
    }

    /// 把一个单元的状态并回来，结果等于从没拆过：四张表按位置插回，其余各自归并。
    #[allow(dead_code, reason = "单元并行建起来之前只有测试调用")]
    pub(crate) fn absorb_unit(&mut self, unit: RegionUnit) {
        let RegionUnit {
            unit: id,
            containers,
            block_items,
            command_blocks,
            books,
            block_events,
            block_entities,
            redstone,
            ticks,
        } = unit;
        let _ = id;
        self.containers.extend(containers);
        self.block_items.extend(block_items);
        self.command_blocks.extend(command_blocks);
        self.lecterns.books.extend(books);
        self.block_events.absorb(block_events);
        self.block_entities.absorb(block_entities);
        self.redstone.absorb(redstone);
        self.block_ticks.absorb(ticks);
    }

    /// 下一步的忙区块候选：本刻排过计划刻 / 排过方块事件的区块，加上有醒着
    /// ticker 的区块。全部是增量或按集合大小成正比，不扫任何大表。
    #[allow(dead_code, reason = "下一步的划分阶段才调用")]
    pub(crate) fn busy_chunks(&self) -> BTreeSet<ChunkPos> {
        let mut out: BTreeSet<ChunkPos> =
            self.block_ticks.touched_chunks().iter().copied().collect();
        out.extend(self.block_events.touched_chunks().iter().copied());
        out.extend(self.block_entities.awake_chunks());
        out
    }
}
