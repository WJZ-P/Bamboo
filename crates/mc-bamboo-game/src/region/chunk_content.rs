//! `RegionColumn` ⇄ 「区块 NBT + 实体 NBT」的组装，见 `docs/m3-storage.md` 5.4 / 5.5。
//!
//! 方块层写自己拥有的键（`mc-bamboo-blocks::archive`），这一层写自己四张表的键
//! （容器、`block_items`、命令方块、讲台），`mc-bamboo-storage::chunk_content` 把两边
//! 按位置合并并原样保留不认识的键。不读写世界目录，也不接进加载 / 卸载流程。
//! 输入都当成不可信数据：所有分支返回 [`Error`] 或把坏条目记进问题清单，不 panic。

use std::collections::BTreeMap;

use mc_bamboo_blocks::state::TRIGGERED;
use mc_bamboo_blocks::{
    BlockState, ChunkBlockEntities, ChunkRedstone, ChunkTicks,
    archive::{
        block_entity::{create_block_entity, items_tag},
        default_additional,
        entity::{passenger_uids, read_entity_tree, write_entity_tree},
        redstone::{RedstoneFacts, read_redstone, write_redstone},
        resave_additional,
        ticks::{decode_chunk_ticks, encode_chunk_ticks},
    },
    entity::{ChunkEntities, EntityRow, EntityUid, FIRST_ENTITY_ID},
};
use mc_bamboo_core::{BlockPos, ChunkPos, Direction};
use mc_bamboo_data::registries::block_entity_type as kinds;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{
        ComponentPatch, ComponentValue, PatchEntry,
        archive::{components, slots, text},
    },
    item::ItemStack,
};
use mc_bamboo_storage::{
    chunk_content::{
        BoundKeys, ChunkContentError, EntityFile, ModeledBlockEntity, merge_block_entities,
        merge_tick_lists,
    },
    chunk_nbt::{self, ChunkEncodeExtra, DecodedChunk as DecodedColumn, field},
};
use mc_bamboo_world::{column::ChunkColumn, ticket::ChunkStatus};

use super::column::{PackedContainer, RegionColumn, RegionColumnParts};
use crate::{
    command_block::{ArchiveCommandBlock, CommandBlockData},
    container::{ContainerEntry, ContainerKind},
    lectern::Book,
};

/// 组装失败的原因。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("chunk NBT: {0}")]
    Column(#[from] chunk_nbt::ChunkNbtError),
    #[error("blocks archive: {0}")]
    Blocks(String),
    #[error("chunk content: {0}")]
    Content(#[from] ChunkContentError),
}

/// 解码跳过的条目等，交给调用方记日志。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub at: String,
    pub message: String,
}

/// 一个区块写出的两份 NBT。`entities` 为 `None` 表示没有实体，不写实体那一条。
#[derive(Debug, Clone, PartialEq)]
pub struct EncodedChunk {
    pub chunk: Tag,
    pub entities: Option<Tag>,
}

/// 读回来的区块束和问题清单。`persisted_status` 是盘上 `Status` 对应的生成阶段，
/// 非 `full` 时 Director 要从这个阶段接着跑生成流水线（官方
/// `SerializableChunkData.read` + `ChunkStep.apply`）。
///
/// Decoded bundle, problems and the on-disk generation stage. A stage below
/// `full` tells the Director where to resume generation (vanilla
/// `SerializableChunkData.read` + `ChunkStep.apply`).
#[derive(Debug)]
pub struct DecodedChunk {
    pub column: Box<RegionColumn>,
    pub problems: Vec<Problem>,
    pub persisted_status: ChunkStatus,
}

/// 存档编码需要的只读部件。`RegionColumn`（卸载存盘）和 `BorrowedParts`
/// （已加载区块的只读快照，不移动盒子，见 `region::content_snapshot`）都实现它，
/// 共用 [`encode_parts`] 这一条编码路径（补 11）。
/// Read-only parts needed to encode a save. Both `RegionColumn` (unload save)
/// and `BorrowedParts` (read-only snapshot of a still-loaded column, item 11)
/// implement it and share the single `encode_parts` path.
pub(crate) trait EncodeParts {
    fn chunk_column(&self) -> &ChunkColumn;
    fn ticks(&self) -> Option<&ChunkTicks>;
    /// 活方块实体按世界坐标给出的 `saveAdditional` 键。卸载盒子从自己那份
    /// `ChunkBlockEntities` 里查；只读快照从 region 的借用状态里查，都不需要
    /// 拥有 `Box<dyn BlockEntity>`。
    fn live_additional(&self, pos: BlockPos) -> Option<Result<Vec<(String, Tag)>, String>>;
    fn redstone(&self) -> &ChunkRedstone;
    fn container(&self, pos: BlockPos) -> Option<&ContainerEntry>;
    fn block_item(&self, pos: BlockPos) -> Option<&ComponentPatch>;
    fn command_block(&self, pos: BlockPos) -> Option<&CommandBlockData>;
    fn book(&self, pos: BlockPos) -> Option<&Book>;
    fn entity_rows(&self) -> &[EntityRow];
    fn preserved_entities(&self) -> &[Tag];
    /// 这一列的 POI 段；`None` 表示没有 POI 数据（不写 `poi/` 那一格）。
    fn poi(&self) -> Option<&mc_bamboo_world::poi::PoiColumn>;
}

/// 把一个区块束整个写成官方的两份 NBT。
///
/// 盒子里的计划刻已经是相对延迟，不需要当前时刻；`last_update` / `inhabited_time`
/// 读盘时挂在 `ChunkColumn` 里，没有就用 0。
/// 把一个已经从 region 收回的盒子编码成两份 NBT（卸载存盘路径）。
/// Encode a box taken out of the region (unload-save path).
pub fn encode(column: &RegionColumn, pos: ChunkPos) -> Result<EncodedChunk, Error> {
    encode_parts(column, pos)
}

/// 从只读部件编码两份 NBT。卸载存盘和已加载区块的只读快照都走这里，
/// 只是前者传 `RegionColumn`、后者传 `BorrowedParts`（补 11）。
/// Encode from read-only parts: unload saves pass a `RegionColumn`, loaded-chunk
/// snapshots pass `BorrowedParts` (item 11); the NBT is byte-identical.
pub(crate) fn encode_parts(parts: &impl EncodeParts, pos: ChunkPos) -> Result<EncodedChunk, Error> {
    let column = parts.chunk_column();
    let mut problems = Vec::new();
    let mut modeled_ids: Vec<&str> = Vec::new();

    // 1. 按位置把每种方块实体建模的键攒起来。
    let mut modeled = Vec::new();
    for entity in column.chunk().block_entities() {
        let name = kinds::name_of(entity.kind).unwrap_or("minecraft:unknown");
        modeled_ids.push(name);
        let x = pos.x * 16 + entity.x as i32;
        let z = pos.z * 16 + entity.z as i32;
        let world_pos = BlockPos::new(x, entity.y, z);
        let mut entry = ModeledBlockEntity::new(name, x, entity.y, z);
        // 只保留存档态的方块实体：按官方 `loadStatic → saveAdditional` 的规则把读盘
        // 条目往返一遍。没有读盘条目时补构造器默认值；调用方后面写的游戏表 / 活实例
        // 键优先，不用读盘旧值盖掉运行时改过的内容。
        let raw = column
            .chunk()
            .preserved_block_entity(entity.x, entity.y, entity.z);
        let state = BlockState(column.chunk().block(entity.x, entity.y, entity.z));
        let empty = Tag::Compound(Vec::new());
        let saved = resave_additional(name, state, raw.unwrap_or(&empty));
        if let Some(saved) = &saved {
            for key in &saved.omit {
                entry.omit.push((*key).to_owned());
            }
        }
        // 会动的方块实体自己的键。有活实例时由活实例全权负责，避免读盘旧值盖掉已清空的可选键；
        // 没有活实例时才回落到 `resave_additional` 或官方默认值。
        match parts.live_additional(world_pos) {
            Some(Ok(additional)) => {
                for (key, value) in additional {
                    entry.put(&key, value);
                }
            }
            Some(Err(error)) => return Err(Error::Blocks(error)),
            None => {
                if let Some(saved) = &saved {
                    for (key, value) in &saved.fields {
                        entry.put(key, value.clone());
                    }
                } else if let Some(fields) = default_additional(name) {
                    for (key, value) in fields {
                        entry.put(&key, value);
                    }
                } else {
                    write_default_block_entity_fields(&mut entry, name);
                }
            }
        }
        // 告示牌的文字在 `ChunkColumn::signs` 里，是运行时内容；按官方
        // `SignBlockEntity.saveAdditional` 重新编码，覆盖 `resave_additional` 的读盘旧值。
        if matches!(name, "minecraft:sign" | "minecraft:hanging_sign") {
            let content = column
                .chunk()
                .sign(entity.x, entity.y, entity.z)
                .cloned()
                .unwrap_or(mc_bamboo_core::sign::SignContent::BLANK);
            write_sign_text(&mut entry, &content);
        }
        // 游戏层拥有的键（容器、组件补丁、命令方块、讲台）覆盖 `resave_additional` 的静态值。
        write_game_fields(&mut entry, &mut problems, name, world_pos, parts, pos);
        // 红石记忆里属于这一格的那几项。
        for (key, value) in write_redstone(
            world_pos,
            name,
            &parts.redstone().archive_entries(),
            redstone_facts(name, world_pos, parts, pos, raw),
        )
        .map_err(|e| Error::Blocks(e.0))?
        {
            entry.put(&key, value);
        }
        modeled.push(entry);
    }
    // 原始条目挂在 ChunkColumn 里，按位置跟类型表走。
    // 官方读盘时把坐标和方块状态不符的条目留在 pendingBlockEntities，
    // 写盘时仍整条列出；下面的合并规则会原样带回这些条目。
    // 不认识的 id、以及官方 pending 的孤立条目要整条原样写回；
    // 只有当前确实建模的 id 才参与「方块没了就丢」的规则。
    let raw: Vec<Tag> = decode_raw_block_entities(column, pos)
        .into_iter()
        .filter(|tag| {
            column.chunk().block_entities().any(|entity| {
                let x = pos.x * 16 + entity.x as i32;
                let z = pos.z * 16 + entity.z as i32;
                Some((x, entity.y, z)) == tag_position_of(tag)
            }) || !modeled_ids.contains(&id_of_raw(tag).unwrap_or(""))
        })
        .collect();
    let merged = merge_block_entities(modeled, &raw, &modeled_ids)?;

    // 2. 计划刻：两条列表混进 ChunkEncodeExtra。
    let mut extra = ChunkEncodeExtra {
        chunk_pos: pos,
        status: Some(column.chunk().archive().status),
        ..Default::default()
    };
    let archive = column.chunk().archive();
    extra.last_update = archive.last_update;
    extra.inhabited_time = archive.inhabited_time;
    let empty_ticks = ChunkTicks::empty();
    let lists = encode_chunk_ticks(parts.ticks().unwrap_or(&empty_ticks))
        .map_err(|e| Error::Blocks(e.0))?;
    problems.extend((0..lists.clamped).map(|_| Problem {
        at: "ticks".to_owned(),
        message: "delay was clamped to i32".to_owned(),
    }));
    merge_tick_lists(lists.blocks, lists.fluids, &[], &[], &mut extra);
    extra.raw_block_entities = Some(merged);
    let chunk = chunk_nbt::encode(column, &extra)?;

    // 3. 实体文件：三种建模实体 + 不认识的类型整条原样保留。乘客嵌在载具的
    //    `Passengers` 里（官方 `Entity.save`），不单独成行。
    let mut file = EntityFile::empty(pos);
    let rows = parts.entity_rows();
    let by_uid: BTreeMap<EntityUid, EntityRow> = rows
        .iter()
        .map(|row| (row.public.uid, row.clone()))
        .collect();
    // 和 `write_entity_tree` 用同一份乘客名单：它把谁嵌进 `Passengers`，这里就不再把谁
    // 单独写一行。只认生物载具的话，船上的乘客会嵌一次、单独再写一次，读回来多一只。
    // The same passenger list `write_entity_tree` nests from: whoever it puts under
    // `Passengers` is not written as a row again. Looking at mob vehicles only wrote a
    // boat passenger twice, nested and on its own, and it came back doubled.
    let written_under_vehicle = |row: &EntityRow| {
        row.public.vehicle.is_some_and(|vehicle| {
            by_uid
                .get(&vehicle)
                .is_some_and(|parent| passenger_uids(&parent.public.kind).contains(&row.public.uid))
        })
    };
    for row in rows {
        // 官方 `Entity.shouldBeSaved`：已经移除的实体不存；整棵树里恰好一名玩家的根载具
        // 也不存，它连同别的乘客写在那名玩家存档的 `RootVehicle` 里，区块里再存一份
        // 就是两份。
        // Vanilla `Entity.shouldBeSaved`: a removed entity is not saved, and neither is
        // a root whose tree holds exactly one player. That root and its other
        // passengers live in the player file's `RootVehicle`; a chunk copy would be a
        // second one.
        if row.public.removed || players_in_tree(row, &by_uid, 0) == 1 {
            continue;
        }
        if written_under_vehicle(row)
            || matches!(
                row.public.kind,
                mc_bamboo_blocks::entity::KindPublic::LightningBolt(_)
                    | mc_bamboo_blocks::entity::KindPublic::FishingHook(_)
            )
        {
            continue;
        }
        let tag = write_entity_tree(row, &|uid| by_uid.get(&uid).cloned());
        match tag {
            Ok(tag) => file.entities.push(tag),
            Err(error) => problems.push(Problem {
                at: format!("entity {}", row.public.uid),
                message: error.0,
            }),
        }
    }
    file.preserved_entities
        .extend(parts.preserved_entities().iter().cloned());
    Ok(EncodedChunk {
        chunk,
        entities: file.encode(),
    })
}

/// 这棵骑乘树里的玩家数，官方 `Entity.countPlayerPassengers`（连间接乘客一起数）。
/// 玩家不在实体表里，名单上小于 [`FIRST_ENTITY_ID`] 的就是玩家。只看同一份存档里的行。
///
/// Players in this riding tree, vanilla `Entity.countPlayerPassengers` (indirect
/// passengers included). Players are not entity rows; ids below [`FIRST_ENTITY_ID`] in
/// a passenger list are players. Only rows of the same save are followed.
fn players_in_tree(
    row: &EntityRow,
    by_uid: &BTreeMap<EntityUid, EntityRow>,
    depth: usize,
) -> usize {
    /// 和 `write_entity_tree` 的嵌套上限一样，坏数据成环也会停。
    ///
    /// The nesting limit of `write_entity_tree`, so a corrupt cycle still ends.
    const MAX_DEPTH: usize = 64;
    passenger_uids(&row.public.kind)
        .iter()
        .map(|&uid| {
            if uid < FIRST_ENTITY_ID {
                1
            } else if depth < MAX_DEPTH {
                by_uid
                    .get(&uid)
                    .filter(|child| child.public.vehicle == Some(row.public.uid))
                    .map_or(0, |child| players_in_tree(child, by_uid, depth + 1))
            } else {
                0
            }
        })
        .sum()
}

/// 从两份 NBT 组装区块束。`entities` 为 `None` 表示没有实体文件。
pub fn decode(pos: ChunkPos, chunk: &Tag, entities: Option<&Tag>) -> Result<DecodedChunk, Error> {
    let mut problems = Vec::new();
    let decoded: DecodedColumn = chunk_nbt::decode(chunk)?;
    let mut parts = RegionColumnParts {
        column: decoded.column,
        ticks: None,
        block_entities: ChunkBlockEntities::default(),
        redstone: ChunkRedstone::default(),
        containers: BTreeMap::new(),
        block_items: BTreeMap::new(),
        command_blocks: BTreeMap::new(),
        books: BTreeMap::new(),
        entities: ChunkEntities::default(),
        poi: Default::default(),
        poi_version_changed: false,
    };

    // 1. 计划刻。
    let (ticks, tick_problems) = decode_chunk_ticks(
        &Tag::List(decoded.block_ticks),
        &Tag::List(decoded.fluid_ticks),
    )
    .map_err(|e| Error::Blocks(e.0))?;
    problems.extend(tick_problems.into_iter().map(|message| Problem {
        at: "ticks".to_owned(),
        message,
    }));
    parts.ticks = Some(ticks);

    // 2. 方块实体条目：按 id 建具体类型，游戏层四张表各取自己那部分键。
    install_block_entities(&mut parts, pos, &decoded.block_entities, &mut problems);

    // 3. 实体文件：认识的三种交给 blocks 解码，不认识的原样保留。
    let mut rows: Vec<EntityRow> = Vec::new();
    let mut preserved_entities = Vec::new();
    if let Some(root) = entities {
        let file = EntityFile::decode(root, pos)?;
        problems.extend(file.problems.into_iter().map(|message| Problem {
            at: "entities".to_owned(),
            message,
        }));
        // 每一条都交给 blocks 试着读：读得出来的进实体表，`id` 不认识的整条原样保留。
        // 以前这里先按一张写死的名单筛，名单外的（村民、马、船、矿车……）即使读得出来
        // 也只当原始数据留着，区块一重新加载它们就从世界里消失了。读坏的也原样保留，
        // 不丢数据。
        // Offer every entry to the blocks decoder: what it reads becomes a live entity
        // and an unknown `id` is preserved verbatim. A fixed allowlist used to filter
        // first, so everything outside it (villagers, horses, boats, minecarts, ...)
        // stayed raw even though it could be read, and vanished from the world when
        // its chunk reloaded. Entries that fail to decode are preserved too.
        let mut next_uid = FIRST_UID;
        // `EntityFile` 已经按那张名单把条目分成两堆，两堆都要试。
        // `EntityFile` already split the entries by that list; both halves are tried.
        let entries = file.entities.into_iter().chain(file.preserved_entities);
        for (index, tag) in entries.enumerate() {
            match read_entity_tree(&tag, &mut next_uid) {
                Ok(Some(tree)) => rows.extend(tree),
                Ok(None) => preserved_entities.push(tag),
                Err(error) => {
                    problems.push(Problem {
                        at: format!("entities[{index}]"),
                        message: error.0,
                    });
                    preserved_entities.push(tag);
                }
            }
        }
    }
    rows.sort_by_key(|row| row.public.uid);
    parts.entities = ChunkEntities::from_parts(rows, preserved_entities);

    Ok(DecodedChunk {
        column: RegionColumn::from_parts(parts),
        problems,
        persisted_status: decoded.persisted_status,
    })
}

/// 把一组完整的方块实体条目（带 `id` 和 `x`/`y`/`z`）装进区块部件：按 id 建具体
/// 类型，游戏层四张表（容器、命令方块、讲台、组件补丁）和红石记忆各取自己那部分键。
/// 读盘和世界生成交付共用这一条，生成的箱子和读回的箱子走同一套规则。
///
/// Install complete block entity entries (with `id` and `x`/`y`/`z`) into the
/// column parts: build the concrete kinds and let the game tables and redstone
/// memory take their keys. Disk loads and generated chunks share this path.
pub(crate) fn install_block_entities(
    parts: &mut RegionColumnParts,
    pos: ChunkPos,
    tags: &[Tag],
    problems: &mut Vec<Problem>,
) {
    let mut live = Vec::new();
    let mut redstone_entries = Vec::new();
    for tag in tags {
        let Some((x, y, z)) = tag_position(tag) else {
            problems.push(Problem {
                at: "block_entities".to_owned(),
                message: "entry has no position".to_owned(),
            });
            continue;
        };
        let world_pos = BlockPos::new(x, y, z);
        let Some(id) = id_of(tag) else {
            problems.push(Problem {
                at: format!("{world_pos:?}"),
                message: "entry has no id".to_owned(),
            });
            continue;
        };
        let local_x = x - pos.x * 16;
        let local_z = z - pos.z * 16;
        let state = if (0..16).contains(&local_x)
            && (0..16).contains(&local_z)
            && (mc_bamboo_world::MIN_Y..mc_bamboo_world::MIN_Y + mc_bamboo_world::WORLD_HEIGHT)
                .contains(&y)
        {
            BlockState(
                parts
                    .column
                    .chunk()
                    .block(local_x as usize, y, local_z as usize),
            )
        } else {
            BlockState::AIR
        };
        match create_block_entity(id, world_pos, state, tag) {
            Ok(Some(entity)) => live.push(entity),
            Ok(None) => {}
            Err(error) => problems.push(Problem {
                at: format!("{world_pos:?} {id}"),
                message: error.0,
            }),
        }
        read_game_fields(parts, problems, id, world_pos, tag, state);
        redstone_entries.extend(read_redstone(world_pos, id, tag));
    }
    parts.block_entities = ChunkBlockEntities::from_entities(live);
    for entry in redstone_entries {
        parts.redstone.push_archive_entry(entry);
    }
}

/// 读盘时给实体重新分配的第一号 ID，和 `EntityStore` 的默认一致。
const FIRST_UID: i32 = 1 << 24;

fn id_of_raw(tag: &Tag) -> Option<&str> {
    match field(tag, "id") {
        Some(Tag::String(value)) => Some(value),
        _ => None,
    }
}

fn tag_position_of(tag: &Tag) -> Option<(i32, i32, i32)> {
    let x = match field(tag, "x")? {
        Tag::Int(v) => *v,
        _ => return None,
    };
    let y = match field(tag, "y")? {
        Tag::Int(v) => *v,
        _ => return None,
    };
    let z = match field(tag, "z")? {
        Tag::Int(v) => *v,
        _ => return None,
    };
    Some((x, y, z))
}

fn decode_raw_block_entities(column: &ChunkColumn, pos: ChunkPos) -> Vec<Tag> {
    let _ = pos;
    column
        .chunk()
        .preserved_block_entities()
        .map(|(_, _, _, _, tag)| tag.clone())
        .collect()
}

fn write_game_fields(
    entry: &mut ModeledBlockEntity,
    problems: &mut Vec<Problem>,
    id: &str,
    pos: BlockPos,
    parts: &impl EncodeParts,
    _chunk: ChunkPos,
) {
    // 容器：Items、LootTable、LootTableSeed。
    if let Some(game) = parts.container(pos) {
        let slots: Vec<_> = game
            .slots
            .iter()
            .enumerate()
            .map(|(slot, stack)| slots::SlottedItem {
                slot: slot as i32,
                stack: stack.clone(),
            })
            .collect();
        match slots::encode_list(&slots) {
            Ok(items) => {
                if game.loot_table.is_none() {
                    entry.put("Items", items);
                }
            }
            Err(error) => problems.push(Problem {
                at: format!("{pos:?} Items"),
                message: error.to_string(),
            }),
        }
        if let Some(table) = &game.loot_table {
            entry.put("LootTable", Tag::String(table.clone()));
            if game.loot_table_seed != 0 {
                entry.put("LootTableSeed", Tag::Long(game.loot_table_seed));
            } else {
                entry.omit.push("LootTableSeed".to_owned());
            }
        } else {
            entry.omit.push("LootTable".to_owned());
            entry.omit.push("LootTableSeed".to_owned());
        }
    }
    // 方块实体从物品带来的组件：CustomName / lock / 旗帜 / 头颅 / 陶罐。
    if let Some(patch) = parts.block_item(pos) {
        write_component_fields(entry, problems, id, pos, patch);
    }
    // 命令方块。
    if id == "minecraft:command_block"
        && let Some(data) = parts.command_block(pos)
    {
        let data = data.archive();
        put_string(entry, "Command", data.command);
        put_int(entry, "SuccessCount", data.success_count);
        put_bool(entry, "TrackOutput", data.track_output);
        put_bool(entry, "UpdateLastExecution", data.update_last_execution);
        put_bool(entry, "powered", data.powered);
        put_bool(entry, "conditionMet", data.condition_met);
        put_bool(entry, "auto", data.automatic);
    }
    // `LastOutput` / `LastExecution` 由 `archive::resave::command_block` 按
    // 官方条件写回（没有运行时表拥有它们），不能在这里删掉。
    // Those two keys have no runtime owner; the archive resave codec writes
    // them conditionally and they must not be dropped here.
    // 讲台上的书。
    if id == "minecraft:lectern" {
        if let Some(book) = parts.book(pos) {
            let (stack, page) = book.archive();
            if !stack.is_empty() {
                match slots::encode_item(stack) {
                    Ok(tag) => {
                        entry.put("Book", tag);
                        put_int(entry, "Page", page);
                    }
                    Err(error) => problems.push(Problem {
                        at: format!("{pos:?} Book"),
                        message: error.to_string(),
                    }),
                }
            }
        }
        entry.omit.push("Book".to_owned());
        entry.omit.push("Page".to_owned());
        // Book 变了之后 Page 不该沿用旧值。
        entry.invalidate.push(BoundKeys {
            watch: "Book".to_owned(),
            bound: vec!["Page".to_owned()],
        });
    }
}

fn write_component_fields(
    entry: &mut ModeledBlockEntity,
    problems: &mut Vec<Problem>,
    id: &str,
    pos: BlockPos,
    patch: &ComponentPatch,
) {
    for (kind, value) in patch.entries() {
        let PatchEntry::Set(value) = value else {
            continue;
        };
        let result: Option<(&str, Tag)> = match (id, *kind, value) {
            // `CopperGolemStatueBlockEntity` 不覆写 `saveAdditional`：自定义名只写在
            // `components` 组件表里（官方 `{"minecraft:custom_name":...}`），没有
            // `CustomName` 旧键。雕像的组件表里只有这一项。
            // A statue overrides no `saveAdditional`: the custom name goes only into
            // the `components` map, with no legacy `CustomName` key.
            (
                "minecraft:copper_golem_statue",
                mc_bamboo_data::registries::data_component_type::CUSTOM_NAME,
                ComponentValue::CustomName(component),
            ) => text::encode(component, 0).ok().map(|v| {
                (
                    "components",
                    Tag::Compound(vec![("minecraft:custom_name".to_owned(), v.to_tag())]),
                )
            }),
            (
                "minecraft:chest"
                | "minecraft:trapped_chest"
                | "minecraft:barrel"
                | "minecraft:shulker_box"
                | "minecraft:hopper"
                | "minecraft:dispenser"
                | "minecraft:dropper"
                | "minecraft:beacon",
                mc_bamboo_data::registries::data_component_type::CUSTOM_NAME,
                ComponentValue::CustomName(component),
            ) => text::encode(component, 0)
                .ok()
                .map(|v| ("CustomName", v.to_tag())),
            (
                _,
                mc_bamboo_data::registries::data_component_type::LOCK,
                ComponentValue::Lock(tag),
            ) => {
                if matches!(tag, Tag::Compound(entries) if entries.is_empty()) {
                    continue;
                }
                Some(("lock", tag.clone()))
            }
            (_, mc_bamboo_data::registries::data_component_type::BANNER_PATTERNS, _) => {
                components::encode_value(value, 0)
                    .ok()
                    .map(|v| ("patterns", v.to_tag()))
            }
            (_, mc_bamboo_data::registries::data_component_type::PROFILE, _) => {
                components::encode_value(value, 0)
                    .ok()
                    .map(|v| ("profile", v.to_tag()))
            }
            (_, mc_bamboo_data::registries::data_component_type::NOTE_BLOCK_SOUND, _) => {
                components::encode_value(value, 0)
                    .ok()
                    .map(|v| ("note_block_sound", v.to_tag()))
            }
            (_, mc_bamboo_data::registries::data_component_type::POT_DECORATIONS, _) => {
                components::encode_value(value, 0)
                    .ok()
                    .map(|v| ("sherds", v.to_tag()))
            }
            _ => None,
        };
        match result {
            Some((key, tag)) => entry.put(key, tag),
            None => problems.push(Problem {
                at: format!("{pos:?} component {kind}"),
                message: "component has no archive field".to_owned(),
            }),
        }
    }
}

/// 官方 `SignText.CODEC` 的三个字段；离线模式没有过滤后的文字。
fn sign_text_tag(text: &mc_bamboo_core::sign::SignText) -> Tag {
    Tag::Compound(vec![
        (
            "messages".to_owned(),
            Tag::List(text.messages.iter().cloned().map(Tag::String).collect()),
        ),
        (
            "color".to_owned(),
            Tag::String(text.color.name().to_owned()),
        ),
        (
            "has_glowing_text".to_owned(),
            Tag::Byte(i8::from(text.glowing)),
        ),
    ])
}

/// 用运行时告示牌内容覆盖读盘条目里的 `front_text` / `back_text` / `is_waxed`。
fn write_sign_text(entry: &mut ModeledBlockEntity, content: &mc_bamboo_core::sign::SignContent) {
    entry.put("front_text", sign_text_tag(&content.front));
    entry.put("back_text", sign_text_tag(&content.back));
    entry.put("is_waxed", Tag::Byte(i8::from(content.waxed)));
}

fn write_default_block_entity_fields(entry: &mut ModeledBlockEntity, id: &str) {
    match id {
        "minecraft:hopper" => put_int(entry, "TransferCooldown", -1),
        "minecraft:furnace" | "minecraft:blast_furnace" | "minecraft:smoker" => {
            put_int(entry, "cooking_time_spent", 0);
            put_int(entry, "cooking_total_time", 0);
            put_int(entry, "lit_time_remaining", 0);
            put_int(entry, "lit_total_time", 0);
            entry.put("speed_multiplier", Tag::Float(1.0));
            entry.put("Items", items(&[]));
            entry.put("RecipesUsed", Tag::Compound(Vec::new()));
        }
        "minecraft:brewing_stand" => {
            put_int(entry, "BrewTime", 0);
            put_int(entry, "total_brew_time", 400);
            put_int(entry, "Fuel", 0);
            put_int(entry, "total_fuel", 20);
            entry.put("speed_multiplier", Tag::Float(1.0));
            entry.put("Items", items(&[]));
        }
        "minecraft:campfire" => {
            entry.put("Items", items(&[]));
            entry.put("CookingTimes", Tag::IntArray(vec![0; 4]));
            entry.put("CookingTotalTimes", Tag::IntArray(vec![0; 4]));
        }
        "minecraft:piston" => {
            entry.put("blockState", Tag::String("minecraft:air".to_owned()));
            entry.put("facing", Tag::Byte(0));
            entry.put("progress", Tag::Float(0.0));
            put_bool(entry, "extending", false);
            put_bool(entry, "source", false);
        }
        _ => {}
    }
}

fn items(slots: &[ItemStack]) -> Tag {
    items_tag(slots).unwrap_or(Tag::List(Vec::new()))
}

fn redstone_facts(
    id: &str,
    pos: BlockPos,
    parts: &impl EncodeParts,
    chunk: ChunkPos,
    raw: Option<&Tag>,
) -> RedstoneFacts {
    let state = parts.chunk_column().chunk().block(
        (pos.x - chunk.x * 16) as usize,
        pos.y,
        (pos.z - chunk.z * 16) as usize,
    );
    let state = BlockState(state);
    let has_record = parts
        .container(pos)
        .and_then(|game| game.slots.first())
        .is_some_and(|stack| !stack.is_empty());
    // 唱片机没有运行时进度表：读盘的 `ticks_since_song_started` 交给红石键写回，
    // 免得被新方块实体的 0 盖掉；没有读盘条目时才按新建写 0。
    // The jukebox has no runtime progress table: keep the disk ticks when present,
    // defaulting to 0 only for a freshly generated block entity.
    let song_ticks = raw
        .and_then(|tag| field(tag, "ticks_since_song_started"))
        .and_then(|tag| match tag {
            Tag::Long(value) => Some(*value),
            Tag::Int(value) => Some(i64::from(*value)),
            _ => None,
        });
    RedstoneFacts {
        jukebox_has_record: id == "minecraft:jukebox" && has_record,
        crafter_triggered: id == "minecraft:crafter" && state.try_get(TRIGGERED) == Some(true),
        song_ticks,
    }
}

fn read_game_fields(
    parts: &mut RegionColumnParts,
    problems: &mut Vec<Problem>,
    id: &str,
    pos: BlockPos,
    tag: &Tag,
    state: BlockState,
) {
    // 容器：有 Items 或 LootTable 就建条目，其它字段按新建默认。
    // 这四类官方不写 `Items`（末影箱没有自己的存档键，唱片机 / 潜影盒 / 陶罐
    // 用 `RecordItem` / `item` 等键）：读盘时不建空条目，免得存盘多写 `Items:[]`。
    let is_container = container_kind(id).is_some()
        && !matches!(
            id,
            "minecraft:ender_chest"
                | "minecraft:jukebox"
                | "minecraft:shulker_box"
                | "minecraft:decorated_pot"
        );
    if is_container {
        let kind = container_kind(id).unwrap_or(ContainerKind::Chest);
        let mut entry = ContainerEntry::new(kind);
        if let Some(table) = match field(tag, "LootTable") {
            Some(Tag::String(value)) => Some(value.clone()),
            _ => None,
        } {
            entry.loot_table = Some(table);
            entry.loot_table_seed = match field(tag, "LootTableSeed") {
                Some(Tag::Long(value)) => *value,
                Some(Tag::Int(value)) => i64::from(*value),
                _ => 0,
            };
        } else if let Some(Tag::List(_)) = field(tag, "Items") {
            let decoded = slots::decode_list(field(tag, "Items").unwrap(), kind.slots());
            for item in decoded.items {
                if let Some(slot) = entry.slots.get_mut(item.slot as usize) {
                    *slot = item.stack;
                }
            }
            problems.extend(decoded.problems.into_iter().map(|problem| Problem {
                at: format!("{pos:?} Items"),
                message: problem.message,
            }));
        }
        parts.containers.insert(pos, PackedContainer::new(entry));
    }
    // 命令方块。
    if id == "minecraft:command_block" {
        parts.command_blocks.insert(
            pos,
            crate::command_block::CommandBlockData::from_archive(ArchiveCommandBlock {
                command: string(tag, "Command"),
                track_output: boolean(tag, "TrackOutput", true),
                automatic: boolean(tag, "auto", false),
                success_count: integer(tag, "SuccessCount", 0),
                powered: boolean(tag, "powered", false),
                condition_met: boolean(tag, "conditionMet", false),
                update_last_execution: boolean(tag, "UpdateLastExecution", true),
            }),
        );
    }
    // 讲台：Book + Page，页数和是否有内容从书重算。
    if id == "minecraft:lectern"
        && let Some(book) = field(tag, "Book")
    {
        match slots::decode_item(book) {
            Ok(stack) => {
                let page = integer(tag, "Page", 0);
                let facing = state
                    .try_get(mc_bamboo_blocks::state::FACING)
                    .unwrap_or(Direction::North);
                parts
                    .books
                    .insert(pos, crate::lectern::Book::from_archive(stack, page, facing));
            }
            Err(error) => problems.push(Problem {
                at: format!("{pos:?} Book"),
                message: error.to_string(),
            }),
        }
    }
    // 旗帜 / 头颅 / 陶罐 / 容器的名字和锁：还原成 block_items 的组件补丁，
    // 之后 update tag 用放置方块时的同一条路径重新生成。
    let mut patch = ComponentPatch::new();
    for (key, kind) in [
        (
            "CustomName",
            mc_bamboo_data::registries::data_component_type::CUSTOM_NAME,
        ),
        (
            "lock",
            mc_bamboo_data::registries::data_component_type::LOCK,
        ),
        (
            "patterns",
            mc_bamboo_data::registries::data_component_type::BANNER_PATTERNS,
        ),
        (
            "profile",
            mc_bamboo_data::registries::data_component_type::PROFILE,
        ),
        (
            "note_block_sound",
            mc_bamboo_data::registries::data_component_type::NOTE_BLOCK_SOUND,
        ),
        (
            "sherds",
            mc_bamboo_data::registries::data_component_type::POT_DECORATIONS,
        ),
    ] {
        if let Some(value) = field(tag, key) {
            if key == "lock" && matches!(value, Tag::Compound(entries) if entries.is_empty()) {
                continue;
            }
            let decoded = if key == "CustomName" {
                text::decode(value).map(ComponentValue::CustomName)
            } else {
                components::decode_value(kind, value, 0)
            };
            match decoded {
                Ok(value) => patch.put(kind, PatchEntry::Set(value)),
                Err(error) => problems.push(Problem {
                    at: format!("{pos:?} {key}"),
                    message: error.to_string(),
                }),
            }
        }
    }
    if !patch.entries().is_empty() {
        parts.block_items.insert(pos, patch);
    }
}

fn container_kind(id: &str) -> Option<ContainerKind> {
    Some(match id {
        "minecraft:chest" | "minecraft:trapped_chest" => ContainerKind::Chest,
        "minecraft:barrel" => ContainerKind::Barrel,
        "minecraft:ender_chest" => ContainerKind::EnderChest,
        "minecraft:shulker_box" => ContainerKind::ShulkerBox,
        "minecraft:hopper" => ContainerKind::Hopper,
        "minecraft:dispenser" => ContainerKind::Dispenser,
        "minecraft:dropper" => ContainerKind::Dropper,
        "minecraft:chiseled_bookshelf" => ContainerKind::ChiseledBookshelf,
        "minecraft:jukebox" => ContainerKind::Jukebox,
        "minecraft:decorated_pot" => ContainerKind::DecoratedPot,
        "minecraft:crafter" => ContainerKind::Crafter,
        "minecraft:shelf" => ContainerKind::Shelf,
        _ => return None,
    })
}

fn put_string(entry: &mut ModeledBlockEntity, key: &str, value: String) {
    entry.put(key, Tag::String(value));
}
fn put_int(entry: &mut ModeledBlockEntity, key: &str, value: i32) {
    entry.put(key, Tag::Int(value));
}
fn put_bool(entry: &mut ModeledBlockEntity, key: &str, value: bool) {
    entry.put(key, Tag::Byte(i8::from(value)));
}
fn string(tag: &Tag, key: &str) -> String {
    match field(tag, key) {
        Some(Tag::String(value)) => value.clone(),
        _ => String::new(),
    }
}
fn integer(tag: &Tag, key: &str, default: i32) -> i32 {
    match field(tag, key) {
        Some(Tag::Int(value)) => *value,
        _ => default,
    }
}
fn boolean(tag: &Tag, key: &str, default: bool) -> bool {
    match field(tag, key) {
        Some(Tag::Byte(value)) => *value != 0,
        _ => default,
    }
}
fn tag_position(tag: &Tag) -> Option<(i32, i32, i32)> {
    let x = match field(tag, "x")? {
        Tag::Int(v) => *v,
        _ => return None,
    };
    let y = match field(tag, "y")? {
        Tag::Int(v) => *v,
        _ => return None,
    };
    let z = match field(tag, "z")? {
        Tag::Int(v) => *v,
        _ => return None,
    };
    Some((x, y, z))
}
fn id_of(tag: &Tag) -> Option<&str> {
    match field(tag, "id") {
        Some(Tag::String(value)) => Some(value),
        _ => None,
    }
}
