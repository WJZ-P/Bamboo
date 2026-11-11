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
        block_entity::{create_block_entity, items_tag, write_block_entity_additional},
        entity::{read_entity, write_entity},
        redstone::{RedstoneFacts, read_redstone, write_redstone},
        ticks::{decode_chunk_ticks, encode_chunk_ticks},
    },
    entity::{ChunkEntities, EntityRow},
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
        merge_tick_lists, split_entity_entries,
    },
    chunk_nbt::{self, ChunkEncodeExtra, DecodedChunk as DecodedColumn, field},
};

use super::column::{PackedContainer, RegionColumn, RegionColumnParts};
use crate::{
    command_block::ArchiveCommandBlock,
    container::{ContainerEntry, ContainerKind},
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

/// 读回来的区块束和问题清单，可直接交给 `Region::deliver_column`。
#[derive(Debug)]
pub struct DecodedChunk {
    pub column: Box<RegionColumn>,
    pub problems: Vec<Problem>,
}

/// 把一个区块束整个写成官方的两份 NBT。
///
/// 盒子里的计划刻已经是相对延迟，不需要当前时刻；`last_update` / `inhabited_time`
/// 读盘时挂在 `ChunkColumn` 里，没有就用 0。
pub fn encode(column: &RegionColumn, pos: ChunkPos) -> Result<EncodedChunk, Error> {
    let view = column.parts();
    let mut problems = Vec::new();
    let mut modeled_ids: Vec<&str> = Vec::new();

    // 1. 按位置把每种方块实体建模的键攒起来。
    let mut modeled = Vec::new();
    for entity in view.column.chunk().block_entities() {
        let name = kinds::name_of(entity.kind).unwrap_or("minecraft:unknown");
        modeled_ids.push(name);
        let x = pos.x * 16 + entity.x as i32;
        let z = pos.z * 16 + entity.z as i32;
        let world_pos = BlockPos::new(x, entity.y, z);
        let mut entry = ModeledBlockEntity::new(name, x, entity.y, z);
        write_game_fields(&mut entry, &mut problems, name, world_pos, &view, pos);
        // 会动的方块实体自己的键。
        let live = view
            .block_entities
            .entities()
            .iter()
            .find(|live| live.pos() == world_pos);
        if let Some(live) = live {
            for (key, value) in
                write_block_entity_additional(live.as_ref()).map_err(|e| Error::Blocks(e.0))?
            {
                entry.put(&key, value);
            }
        } else {
            // 官方必写但我们没有实例的键：按官方默认值补齐。
            write_default_block_entity_fields(&mut entry, name);
        }
        // 红石记忆里属于这一格的那几项。
        for (key, value) in write_redstone(
            world_pos,
            name,
            &view.redstone.archive_entries(),
            redstone_facts(name, world_pos, &view, pos),
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
            view.column.chunk().block_entities().any(|entity| {
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
        ..Default::default()
    };
    let archive = view.column.chunk().archive();
    extra.last_update = archive.last_update;
    extra.inhabited_time = archive.inhabited_time;
    let empty_ticks = ChunkTicks::empty();
    let lists =
        encode_chunk_ticks(view.ticks.unwrap_or(&empty_ticks)).map_err(|e| Error::Blocks(e.0))?;
    problems.extend((0..lists.clamped).map(|_| Problem {
        at: "ticks".to_owned(),
        message: "delay was clamped to i32".to_owned(),
    }));
    merge_tick_lists(lists.blocks, lists.fluids, &[], &[], &mut extra);
    extra.raw_block_entities = Some(merged);
    let chunk = chunk_nbt::encode(view.column, &extra)?;

    // 3. 实体文件：三种建模实体 + 不认识的类型整条原样保留。
    let mut file = EntityFile::empty(pos);
    for row in view.entities.rows() {
        match write_entity(row) {
            Ok(tag) => file.entities.push(tag),
            Err(error) => problems.push(Problem {
                at: format!("entity {}", row.public.uid),
                message: error.0,
            }),
        }
    }
    file.preserved_entities
        .extend(view.entities.preserved().iter().cloned());
    Ok(EncodedChunk {
        chunk,
        entities: file.encode(),
    })
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
    let mut live = Vec::new();
    let mut redstone_entries = Vec::new();
    for tag in &decoded.block_entities {
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
        read_game_fields(&mut parts, &mut problems, id, world_pos, tag, state);
        redstone_entries.extend(read_redstone(world_pos, id, tag));
    }
    parts.block_entities = ChunkBlockEntities::from_entities(live);
    for entry in redstone_entries {
        parts.redstone.push_archive_entry(entry);
    }

    // 3. 实体文件：认识的三种交给 blocks 解码，不认识的原样保留。
    let mut rows: Vec<EntityRow> = Vec::new();
    let mut preserved_entities = Vec::new();
    if let Some(root) = entities {
        let file = EntityFile::decode(root, pos)?;
        problems.extend(file.problems.into_iter().map(|message| Problem {
            at: "entities".to_owned(),
            message,
        }));
        let (modeled, unknown) = split_entity_entries(file.entities);
        for (index, tag) in modeled.iter().enumerate() {
            match read_entity(tag, FIRST_UID + index as i32) {
                Ok(Some(row)) => rows.push(row),
                Ok(None) => problems.push(Problem {
                    at: format!("entities[{index}]"),
                    message: "unknown entity id".to_owned(),
                }),
                Err(error) => problems.push(Problem {
                    at: format!("entities[{index}]"),
                    message: error.0,
                }),
            }
        }
        preserved_entities.extend(unknown);
        preserved_entities.extend(file.preserved_entities);
    }
    rows.sort_by_key(|row| row.public.uid);
    parts.entities = ChunkEntities::from_parts(rows, preserved_entities);

    Ok(DecodedChunk {
        column: RegionColumn::from_parts(parts),
        problems,
    })
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

fn decode_raw_block_entities(column: &RegionColumn, pos: ChunkPos) -> Vec<Tag> {
    let mut out = Vec::new();
    for (x, y, z, _, tag) in column.parts().column.chunk().preserved_block_entities() {
        let _ = (x, y, z, pos);
        out.push(tag.clone());
    }
    out
}

fn write_game_fields(
    entry: &mut ModeledBlockEntity,
    problems: &mut Vec<Problem>,
    id: &str,
    pos: BlockPos,
    view: &super::column::RegionColumnView<'_>,
    _chunk: ChunkPos,
) {
    // 容器：Items、LootTable、LootTableSeed。
    if let Some(packed) = view.containers.get(&pos) {
        let game = packed.entry();
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
    if let Some(patch) = view.block_items.get(&pos) {
        write_component_fields(entry, problems, id, pos, patch);
    }
    // 命令方块。
    if id == "minecraft:command_block" {
        if let Some(data) = view.command_blocks.get(&pos) {
            let data = data.archive();
            put_string(entry, "Command", data.command);
            put_int(entry, "SuccessCount", data.success_count);
            put_bool(entry, "TrackOutput", data.track_output);
            put_bool(entry, "UpdateLastExecution", data.update_last_execution);
            put_bool(entry, "powered", data.powered);
            put_bool(entry, "conditionMet", data.condition_met);
            put_bool(entry, "auto", data.automatic);
        }
        // 官方 TrackOutput 为真时写 LastOutput（可省），我们没建模，删保留值。
        entry.omit.push("LastOutput".to_owned());
        entry.omit.push("LastExecution".to_owned());
    }
    // 讲台上的书。
    if id == "minecraft:lectern" {
        if let Some(book) = view.books.get(&pos) {
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
            (
                "minecraft:chest"
                | "minecraft:trapped_chest"
                | "minecraft:barrel"
                | "minecraft:shulker_box"
                | "minecraft:hopper"
                | "minecraft:dispenser"
                | "minecraft:dropper",
                mc_bamboo_data::registries::data_component_type::CUSTOM_NAME,
                ComponentValue::CustomName(component),
            ) => text::encode(component, 0)
                .ok()
                .map(|v| ("CustomName", v.to_tag())),
            (
                _,
                mc_bamboo_data::registries::data_component_type::LOCK,
                ComponentValue::Lock(tag),
            ) => Some(("lock", tag.clone())),
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
    view: &super::column::RegionColumnView<'_>,
    chunk: ChunkPos,
) -> RedstoneFacts {
    let state = view.column.chunk().block(
        (pos.x - chunk.x * 16) as usize,
        pos.y,
        (pos.z - chunk.z * 16) as usize,
    );
    let state = BlockState(state);
    let has_record = view
        .containers
        .get(&pos)
        .and_then(|packed| packed.entry().slots.first())
        .is_some_and(|stack| !stack.is_empty());
    RedstoneFacts {
        jukebox_has_record: id == "minecraft:jukebox" && has_record,
        crafter_triggered: id == "minecraft:crafter" && state.try_get(TRIGGERED) == Some(true),
        song_ticks: None,
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
    let is_container = container_kind(id).is_some();
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
