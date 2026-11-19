//! 单元的拆合：把一组区块的带位置状态整盒拆出去、跑完再合回来。
//!
//! 见 `docs/multi-region-tick-design.md` 4.8：现在只有 region 一个单元，存储仍是
//! 全局形状；这些接口只在单元拓扑变化时用，不在 tick 热路径上。实体表和
//! `entity_tracking` 的玩家配对这次不拆，等第 5 步的岛再说。

use std::collections::BTreeSet;

use mc_bamboo_blocks::{BlockEntities, BlockEvents, BlockTicks, RedstoneMemory};
use mc_bamboo_core::ChunkPos;
use mc_bamboo_protocol::components::ComponentPatch;

use crate::{
    command_block::CommandBlockData, container::ContainerEntry, lectern::Book, pos_map::PosMap,
    region::Region,
};

/// 一个单元自己的那份带位置状态。现在没有人并行跑它，只给测试和下一步用。
pub(crate) struct RegionUnit {
    pub(crate) unit: u64,
    pub(crate) containers: PosMap<ContainerEntry>,
    pub(crate) block_items: PosMap<ComponentPatch>,
    pub(crate) command_blocks: PosMap<CommandBlockData>,
    pub(crate) books: PosMap<Book>,
    pub(crate) block_events: BlockEvents,
    pub(crate) block_entities: BlockEntities,
    pub(crate) redstone: RedstoneMemory,
    pub(crate) ticks: BlockTicks,
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
        let containers = self.containers.split_off_chunks(chunks);
        let block_items = self.block_items.split_off_chunks(chunks);
        let command_blocks = self.command_blocks.split_off_chunks(chunks);
        let books = self.lecterns.books.split_off_chunks(chunks);
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
        self.containers.absorb(containers);
        self.block_items.absorb(block_items);
        self.command_blocks.absorb(command_blocks);
        self.lecterns.books.absorb(books);
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
