//! 已加载区块的「只读存档快照」部件（补 11）和每刻的内容指纹（补 9）。
//!
//! `BorrowedParts` 从 `&Region` 借用/克隆出一柱存档编码需要的部件：不搬盒子、
//! 不算完光照、不发任何包，写出的 NBT 和卸载存盘逐字节一致（共用
//! `chunk_content::encode_parts`）。`content_fingerprints` 一次扫过上层内容表，
//! 让 D6 认出「方块没变但箱子里的东西变了」这种真实的脏。
//!
//! Read-only save-snapshot parts (item 11) and per-tick content fingerprints
//! (item 9). `BorrowedParts` borrows/clones from `&Region` without moving the
//! box, settling light or sending packets; the NBT is byte-identical to an
//! unload save (shared `encode_parts`). `content_fingerprints` scans the upper
//! tables once so D6 can see content-only edits.

use std::collections::{BTreeMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

use mc_bamboo_blocks::{
    ChunkRedstone, ChunkTicks,
    archive::block_entity::write_block_entity_additional,
    entity::{ChunkEntities, EntityRow},
};
use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::components::ComponentPatch;
use mc_bamboo_world::column::ChunkColumn;

use super::{Region, chunk_content::EncodeParts, column::PackedContainer};
use crate::{command_block::CommandBlockData, container::ContainerEntry, lectern::Book};

/// 一柱已加载区块的只读存档部件。全部是拥有值，生命周期和 region 无关。
/// Owned read-only save parts for one loaded column; independent of `&Region`'s lifetime.
pub(crate) struct BorrowedParts {
    column: ChunkColumn,
    ticks: Option<ChunkTicks>,
    live_additional: BTreeMap<BlockPos, Vec<(String, Tag)>>,
    redstone: ChunkRedstone,
    containers: BTreeMap<BlockPos, PackedContainer>,
    block_items: BTreeMap<BlockPos, ComponentPatch>,
    command_blocks: BTreeMap<BlockPos, CommandBlockData>,
    books: BTreeMap<BlockPos, Book>,
    entities: ChunkEntities,
}

impl BorrowedParts {
    /// 只读地组装一柱。任何表在 tick 中途（有在途刻/实体）就返回 `None`，
    /// 调用方下一刻重试；绝不改 region、绝不提前算完光照、绝不动区块包缓存。
    /// Assemble a column read-only. If any table is mid-tick, return `None`
    /// (retry next tick). Never mutates the region, settles light or touches
    /// the chunk-packet cache.
    pub(crate) fn build(region: &Region, pos: ChunkPos) -> Option<Self> {
        let column = region.chunks.column_clone(pos)?;
        let ticks = region.block_ticks.snapshot_chunk(pos);
        let entities = region.entities.snapshot_chunk(pos)?;

        // 活方块实体的 `saveAdditional` 键，按世界坐标收集；不拥有 Box<dyn>。
        let mut live_additional = BTreeMap::new();
        region.block_entities.visit(|at, entity| {
            if at.chunk() == pos
                && let Ok(fields) = write_block_entity_additional(entity)
            {
                live_additional.insert(at, fields);
            }
        });

        let redstone = region
            .redstone
            .snapshot_chunk(pos, region.block_ticks.now());
        let in_chunk = |at: &BlockPos| at.chunk() == pos;
        let containers = region
            .containers
            .iter()
            .filter(|(at, _)| in_chunk(at))
            .map(|(&at, entry)| (at, PackedContainer::new(entry.clone())))
            .collect();
        let block_items = clone_in_chunk(&region.block_items, pos);
        let command_blocks = clone_in_chunk(&region.command_blocks, pos);
        let books = clone_in_chunk(&region.lecterns.books, pos);

        Some(Self {
            column,
            ticks,
            live_additional,
            redstone,
            containers,
            block_items,
            command_blocks,
            books,
            entities,
        })
    }
}

fn clone_in_chunk<T: Clone>(table: &BTreeMap<BlockPos, T>, pos: ChunkPos) -> BTreeMap<BlockPos, T> {
    table
        .iter()
        .filter(|(at, _)| at.chunk() == pos)
        .map(|(&at, value)| (at, value.clone()))
        .collect()
}

impl EncodeParts for BorrowedParts {
    fn chunk_column(&self) -> &ChunkColumn {
        &self.column
    }

    fn ticks(&self) -> Option<&ChunkTicks> {
        self.ticks.as_ref()
    }

    fn live_additional(&self, pos: BlockPos) -> Option<Result<Vec<(String, Tag)>, String>> {
        self.live_additional.get(&pos).cloned().map(Ok)
    }

    fn redstone(&self) -> &ChunkRedstone {
        &self.redstone
    }

    fn container(&self, pos: BlockPos) -> Option<&ContainerEntry> {
        self.containers.get(&pos).map(|packed| packed.entry())
    }

    fn block_item(&self, pos: BlockPos) -> Option<&ComponentPatch> {
        self.block_items.get(&pos)
    }

    fn command_block(&self, pos: BlockPos) -> Option<&CommandBlockData> {
        self.command_blocks.get(&pos)
    }

    fn book(&self, pos: BlockPos) -> Option<&Book> {
        self.books.get(&pos)
    }

    fn entity_rows(&self) -> &[EntityRow] {
        self.entities.rows()
    }

    fn preserved_entities(&self) -> &[Tag] {
        self.entities.preserved()
    }
}

/// 每刻把「会存盘的上层内容」按区块散成一个 u64。方块/光照由 `ChunkStore`
/// 的版本号负责，这里补上容器、方块实体、实体、计划刻、红石记忆等上层内容
/// （补 9：箱子里放东西不改方块也必须被 D6 写下去）。
/// Hash per chunk of the upper-layer saved content. The lower block/light
/// revision is handled by `ChunkStore`; this covers containers, block entities,
/// entities, scheduled ticks and redstone memory (item 9: a chest edit with no
/// block change must still be saved by D6).
pub(crate) fn content_fingerprints(region: &Region) -> BTreeMap<ChunkPos, u64> {
    let mut entries: BTreeMap<ChunkPos, Vec<String>> = BTreeMap::new();
    let mut push = |at: BlockPos, text: String| {
        entries.entry(at.chunk()).or_default().push(text);
    };

    for (&at, entry) in &region.containers {
        push(
            at,
            format!(
                "container {:?} {:?} {:?} {}",
                entry.kind, entry.slots, entry.loot_table, entry.loot_table_seed
            ),
        );
    }
    for (&at, patch) in &region.block_items {
        push(at, format!("block_item {patch:?}"));
    }
    for (&at, data) in &region.command_blocks {
        push(at, format!("command_block {data:?}"));
    }
    for (&at, book) in &region.lecterns.books {
        push(at, format!("book {book:?}"));
    }
    for tick in region.block_ticks.pending() {
        push(tick.pos, format!("tick {tick:?}"));
    }
    for tick in region.block_ticks.pending_fluids() {
        push(tick.pos, format!("fluid_tick {tick:?}"));
    }
    region.block_entities.visit(|at, entity| {
        if let Ok(fields) = write_block_entity_additional(entity) {
            push(at, format!("block_entity {fields:?}"));
        }
    });
    for entity in region.entities.iter() {
        push(entity.block_position(), format!("entity {entity:?}"));
    }
    for preserved in region.entities.preserved() {
        let [x, _, z] = mc_bamboo_blocks::archive::tag::vec3(preserved, "Pos", [0.0; 3]);
        push(
            BlockPos::new(x.floor() as i32, 0, z.floor() as i32),
            format!("preserved {preserved:?}"),
        );
    }
    // 红石记忆按已加载区块各取一份只读快照；只有真的有内容才进指纹，
    // 否则每个区块都会凭空多一条，原样区块会被误判成脏。
    for (pos, _, _) in region.chunks.loaded_states() {
        let memory = region
            .redstone
            .snapshot_chunk(pos, region.block_ticks.now());
        if !memory.is_empty() {
            push(
                BlockPos::new(pos.x * 16, 0, pos.z * 16),
                format!("redstone {memory:?}"),
            );
        }
    }

    entries
        .into_iter()
        .map(|(pos, mut texts)| {
            texts.sort();
            let mut hasher = DefaultHasher::new();
            texts.hash(&mut hasher);
            (pos, hasher.finish())
        })
        .collect()
}
