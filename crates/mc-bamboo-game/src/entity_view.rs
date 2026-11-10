//! 实体算阶段用的只读方块视图：只借 `&ChunkStore` 和几样算阶段需要的只读事实。
//!
//! `RegionLevel` 独占借着区块表（要记方块改动），落地阶段才需要它；算阶段不能
//! 同时借出一个可变世界和一个只读世界，所以这里单独做一份 `BlockView + Sync`。
//! 它和精确模式的 `World` 共用 `mc_bamboo_blocks::entity::view` 的查询实现。
//!
//! 方块实体表不是 `Sync`（里面是 `Box<dyn BlockEntity>`），所以算阶段只带
//! 「哪些格子的潜影盒是开着的」这一份按位置拷出来的只读事实，不借整张表。

use std::collections::BTreeSet;

use mc_bamboo_blocks::{
    BlockState, ShulkerBoxEntity,
    entity::view::{BlockSource, SourceView, ViewFacts},
};
use mc_bamboo_core::{BlockPos, LightLayer};
use mc_bamboo_data::{BlockAncestor, blocks};
use mc_bamboo_world::{MIN_Y, SECTION_SIZE, palette::PaletteView, store::ChunkStore};

/// 算阶段只读的世界事实。字段都在 tick 开始时按值拷进来，不借用 `Region` 的可变部分。
pub(crate) struct ReadOnlyWorld<'a> {
    pub(crate) chunks: &'a ChunkStore,
    pub(crate) facts: ViewFacts,
    /// 盖子开着的潜影盒位置。`isFaceSturdy` 要读，别的方块实体数据算阶段用不到。
    pub(crate) open_shulker_boxes: BTreeSet<BlockPos>,
    /// 世界里漏斗方块的位置。没有漏斗时物品不发 `SuckIntoHopper` 命令。
    pub(crate) hopper_positions: BTreeSet<BlockPos>,
}

impl BlockSource for ReadOnlyWorld<'_> {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        BlockState(self.chunks.block(pos).unwrap_or(blocks::VOID_AIR))
    }

    fn facts(&self) -> ViewFacts {
        self.facts
    }

    fn brightness(&self, layer: LightLayer, pos: BlockPos) -> i32 {
        i32::from(self.chunks.light_value(layer, pos))
    }

    fn shulker_box_open(&self, pos: BlockPos) -> bool {
        self.open_shulker_boxes.contains(&pos)
    }

    fn any_hopper_in(&self, min: BlockPos, max: BlockPos) -> bool {
        ReadOnlyWorld::any_hopper_in(self, min, max)
    }
}

impl ReadOnlyWorld<'_> {
    /// 这片格子里有没有漏斗。位置集合是算阶段开头从区块表拷出来的，只读且 `Sync`。
    /// `BlockPos` 的 Ord 先比 x，所以范围查询只能先缩小到 x 段，再逐项核 y/z。
    fn any_hopper_in(&self, min: BlockPos, max: BlockPos) -> bool {
        // Ord 先比 x，所以先按 x 段扫一段有序集合，再逐项核 y/z 和 x。
        let first = BlockPos::new(min.x, i32::MIN, i32::MIN);
        let last = BlockPos::new(max.x, i32::MAX, i32::MAX);
        self.hopper_positions.range(first..=last).any(|pos| {
            (min.x..=max.x).contains(&pos.x)
                && (min.y..=max.y).contains(&pos.y)
                && (min.z..=max.z).contains(&pos.z)
        })
    }
}

/// 给算阶段用的只读视图。
pub(crate) type EntityBlockView<'a> = SourceView<ReadOnlyWorld<'a>>;

/// 构造只读视图时借的字段和 `RegionLevel` 借的字段不相交，所以调用方按字段拆借即可。
/// 这个断言钉住区块表是 `Sync` 的：算阶段的线程会共享同一个引用。
const _: () = {
    const fn assert_sync<T: Sync>() {}
    assert_sync::<ChunkStore>();
};

/// 算阶段开头从区块表拷出所有漏斗方块的位置。只扫改过的区块段调色板，
/// 没有漏斗时物品不会发 `SuckIntoHopper` 命令；精确模式仍然逐格扫。
pub(crate) fn hopper_positions(chunks: &ChunkStore) -> BTreeSet<BlockPos> {
    let mut found = BTreeSet::new();
    let mut scan = |chunk_pos: mc_bamboo_core::ChunkPos, chunk: &mc_bamboo_world::chunk::Chunk| {
        for (section_index, section) in chunk.sections().iter().enumerate() {
            let has_hopper = match section.blocks().palette() {
                // 全局调色板里存的是全局状态 ID，没法只看表就排除，逐格扫。
                PaletteView::Global => true,
                PaletteView::Single(state) => BlockState(state).is_a(BlockAncestor::HopperBlock),
                PaletteView::Indirect(table) => table
                    .iter()
                    .any(|&state| BlockState(state).is_a(BlockAncestor::HopperBlock)),
            };
            if !has_hopper {
                continue;
            }
            let base_y = MIN_Y + section_index as i32 * SECTION_SIZE;
            let origin_x = chunk_pos.x * SECTION_SIZE;
            let origin_z = chunk_pos.z * SECTION_SIZE;
            for y in 0..SECTION_SIZE {
                for z in 0..SECTION_SIZE {
                    for x in 0..SECTION_SIZE {
                        let index = ((y as usize) << 8) | ((z as usize) << 4) | x as usize;
                        let state = section.blocks().get(index);
                        if BlockState(state).is_a(BlockAncestor::HopperBlock) {
                            found.insert(BlockPos::new(origin_x + x, base_y + y, origin_z + z));
                        }
                    }
                }
            }
        }
    };
    scan(mc_bamboo_core::ChunkPos::new(0, 0), chunks.template());
    for (pos, chunk) in chunks.edited_chunks() {
        scan(pos, chunk);
    }
    found
}

/// 从方块实体表里拷出盖子开着的潜影盒位置。只在算阶段开头跑一次。
pub(crate) fn open_shulker_boxes(entities: &mc_bamboo_blocks::BlockEntities) -> BTreeSet<BlockPos> {
    let mut open = BTreeSet::new();
    entities.visit(|pos, entity| {
        if entity
            .as_any()
            .downcast_ref::<ShulkerBoxEntity>()
            .is_some_and(|box_| !box_.is_closed())
        {
            open.insert(pos);
        }
    });
    open
}
