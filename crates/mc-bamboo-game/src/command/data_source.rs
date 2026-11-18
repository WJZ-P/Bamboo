//! `/data` 和 `/execute store/if data` 共用的 NBT 数据源。
//!
//! 实体读写走 `mc-bamboo-blocks::archive::entity`（`Entity.saveWithoutId` / `load`），
//! 方块实体走同一层的容器 / 组件表（`chunk_content` 的存档字段），storage 是全服一份
//! 的 `CommandStorage`（只放在 `Global`，不写存档，和现有计分板一致）。
//!
//! NBT sources shared by `/data` and `/execute store/if data`. Entities use the
//! archive codec, block entities use the same game tables that archive writes,
//! and command storage is one global map like the scoreboard.

use std::collections::BTreeMap;

use mc_bamboo_blocks::{
    archive::{read_entity, write_block_entity_additional, write_entity},
    entity::EntityRow,
};
use mc_bamboo_core::BlockPos;
use mc_bamboo_data::registries::block_entity_type as kinds;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::components::archive::slots;

use crate::{command::select::Target, global::Global, region::Region};

/// 三种数据源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Block,
    Entity,
    Storage,
}

/// 一次读写的目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Source {
    pub kind: Kind,
    pub block: Option<BlockPos>,
    pub entity: Option<Target>,
    pub storage: Option<String>,
}

/// 官方 `CommandStorage`：按名字存整条复合标签。不写存档。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommandStorage {
    tags: BTreeMap<String, Tag>,
}

impl CommandStorage {
    pub fn get(&self, id: &str) -> Tag {
        self.tags
            .get(id)
            .cloned()
            .unwrap_or_else(|| Tag::Compound(Vec::new()))
    }

    pub fn set(&mut self, id: &str, tag: Tag) {
        if matches!(&tag, Tag::Compound(entries) if entries.is_empty()) {
            self.tags.remove(id);
        } else {
            self.tags.insert(id.to_owned(), tag);
        }
    }
}

/// 官方 `DataAccessor.getData()`：整条复合标签。
pub(crate) fn read(global: &Global, region: &Region, source: &Source) -> Result<Tag, &'static str> {
    let view = global.view();
    read_view(&view, region, source)
}

/// 只有 `GlobalView` 的来源（`/execute if data` 在岛上跑）用这一份只读快照。
pub(crate) fn read_view(
    view: &crate::global::GlobalView,
    region: &Region,
    source: &Source,
) -> Result<Tag, &'static str> {
    match source.kind {
        Kind::Storage => Ok(view
            .command_storage
            .get(source.storage.as_deref().unwrap_or(""))),
        Kind::Entity => {
            let target = source.entity.ok_or("command.failed")?;
            match target {
                Target::Player(_) => {
                    // 官方玩家也能 `get`（`NbtPredicate.getEntityTagToCompare`）；
                    // Bamboo 没有玩家存档 NBT，明确报错不假装空。
                    Err("commands.data.entity.invalid")
                }
                Target::World(uid) => {
                    let row = entity_row(region, uid).ok_or("argument.entity.notfound.entity")?;
                    write_entity(&row).map_err(|_| "command.failed")
                }
            }
        }
        Kind::Block => {
            let pos = source.block.ok_or("command.failed")?;
            if !block_entity_exists(region, pos) {
                return Err("commands.data.block.invalid");
            }
            Ok(block_entity_tag(region, pos))
        }
    }
}

/// 官方 `DataAccessor.setData()`：把整条复合标签写回去。
pub(crate) fn write(
    global: &mut Global,
    region: &mut Region,
    source: &Source,
    tag: &Tag,
) -> Result<(), &'static str> {
    let Tag::Compound(_) = tag else {
        return Err("command.failed");
    };
    match source.kind {
        Kind::Storage => {
            global
                .command_storage
                .set(source.storage.as_deref().unwrap_or(""), tag.clone());
            Ok(())
        }
        Kind::Entity => match source.entity.ok_or("command.failed")? {
            // 官方 `EntityDataAccessor.setData` 对玩家抛 `commands.data.entity.invalid`。
            Target::Player(_) => Err("commands.data.entity.invalid"),
            Target::World(uid) => {
                let row = entity_row(region, uid).ok_or("argument.entity.notfound.entity")?;
                let rebuilt = read_entity(tag, uid)
                    .map_err(|_| "command.failed")?
                    .unwrap_or(row);
                let mut rebuilt = rebuilt.into_entity();
                rebuilt.id = uid;
                region.entities.with_mut(uid, |live| {
                    let split = rebuilt.clone().split();
                    *live = split.into_entity();
                });
                Ok(())
            }
        },
        Kind::Block => {
            let pos = source.block.ok_or("command.failed")?;
            if !block_entity_exists(region, pos) {
                return Err("commands.data.block.invalid");
            }
            write_block_entity_fields(region, pos, tag)
        }
    }
}

fn entity_row(region: &Region, uid: i32) -> Option<EntityRow> {
    let row = region.entities.row_of(uid)?;
    region.entities.row_at(row).cloned()
}

/// 这一格有没有方块实体：方块层的实例，或者游戏层四张表里任一项。
fn block_entity_exists(region: &Region, pos: BlockPos) -> bool {
    region.block_entities.find_dyn(pos).is_some()
        || region.containers.contains_key(&pos)
        || region.block_items.contains_key(&pos)
        || region.command_blocks.contains_key(&pos)
        || region.lecterns.books.contains_key(&pos)
        || region
            .chunks
            .block(pos)
            .is_some_and(|state| mc_bamboo_blocks::state::BlockState(state).has_block_entity())
}

/// 官方 `BlockEntity.saveWithoutMetadata` 的字段：方块层 `saveAdditional` + 游戏层四张表。
/// 这里不另写一套 codec，键名和 `chunk_content::write_game_fields` 一致。
fn block_entity_tag(region: &Region, pos: BlockPos) -> Tag {
    let mut entries: Vec<(String, Tag)> = Vec::new();
    let state = mc_bamboo_blocks::state::BlockState(region.chunks.block(pos).unwrap_or(0));
    if let Some(id) = state.block().block_entity_type
        && let Some(name) = kinds::name_of(id)
    {
        entries.push(("id".to_owned(), Tag::String(name.to_owned())));
    }
    if let Some(entity) = region.block_entities.find_dyn(pos)
        && let Ok(fields) = write_block_entity_additional(entity)
    {
        for (key, value) in fields {
            put(&mut entries, key, value);
        }
    }
    if let Some(container) = region.containers.get(&pos) {
        let items: Vec<_> = container
            .slots
            .iter()
            .enumerate()
            .filter(|(_, stack)| !stack.is_empty())
            .map(|(slot, stack)| slots::SlottedItem {
                slot: slot as i32,
                stack: stack.clone(),
            })
            .collect();
        if let Ok(tag) = slots::encode_list(&items) {
            put(&mut entries, "Items".to_owned(), tag);
        }
        if let Some(table) = &container.loot_table {
            put(
                &mut entries,
                "LootTable".to_owned(),
                Tag::String(table.clone()),
            );
            if container.loot_table_seed != 0 {
                put(
                    &mut entries,
                    "LootTableSeed".to_owned(),
                    Tag::Long(container.loot_table_seed),
                );
            }
        }
    }
    if let Some(name) = crate::block_items::custom_name(region, pos) {
        put(&mut entries, "CustomName".to_owned(), name.tag().clone());
    }
    if let Some(command) = region.command_blocks.get(&pos) {
        put(
            &mut entries,
            "Command".to_owned(),
            Tag::String(command.command.clone()),
        );
        put(
            &mut entries,
            "SuccessCount".to_owned(),
            Tag::Int(command.archive().success_count),
        );
    }
    entries.push(("x".to_owned(), Tag::Int(pos.x)));
    entries.push(("y".to_owned(), Tag::Int(pos.y)));
    entries.push(("z".to_owned(), Tag::Int(pos.z)));
    Tag::Compound(entries)
}

/// `/data merge` / `modify` 以后把改过的方块实体字段写回运行时表；
/// 不认识的键明确报错，不静默丢掉。
fn write_block_entity_fields(
    region: &mut Region,
    pos: BlockPos,
    tag: &Tag,
) -> Result<(), &'static str> {
    let Tag::Compound(entries) = tag else {
        return Err("command.failed");
    };
    for (key, value) in entries {
        match key.as_str() {
            "id" | "x" | "y" | "z" => continue,
            "Items" => {
                let Some(container) = region.containers.get_mut(&pos) else {
                    continue;
                };
                let decoded = slots::decode_list(value, container.slots.len());
                for item in decoded.items {
                    if let Some(slot) = container.slots.get_mut(item.slot as usize) {
                        *slot = item.stack;
                    }
                }
            }
            "LootTable" => {
                if let (Some(container), Tag::String(name)) =
                    (region.containers.get_mut(&pos), value)
                {
                    container.loot_table = Some(name.clone());
                }
            }
            "LootTableSeed" => {
                if let (Some(container), Tag::Long(seed)) = (region.containers.get_mut(&pos), value)
                {
                    container.loot_table_seed = *seed;
                }
            }
            "Command" => {
                if let (Some(command), Tag::String(text)) =
                    (region.command_blocks.get_mut(&pos), value)
                {
                    command.command = text.clone();
                }
            }
            "SuccessCount" => {
                if let (Some(command), Tag::Int(count)) =
                    (region.command_blocks.get_mut(&pos), value)
                {
                    command.set_success_count(*count);
                }
            }
            "CustomName" => {
                // 组件表里的自定义名由 `block_items` 统一管，避免另写一套组件 codec。
                let mut patch = region.block_items.get(&pos).cloned().unwrap_or_default();
                patch.put(
                    mc_bamboo_data::registries::data_component_type::CUSTOM_NAME,
                    mc_bamboo_protocol::components::PatchEntry::Set(
                        mc_bamboo_protocol::components::ComponentValue::CustomName(
                            mc_bamboo_protocol::text::Component::trusted(value.clone()),
                        ),
                    ),
                );
                region.block_items.insert(pos, patch);
                crate::block_items::refresh_update_tag(region, pos);
            }
            _ => {
                // 活方块实体自己的 `saveAdditional` 键由方块层解析；这里没有
                // `read_additional` 的统一入口，暂不回写，交回明确的错误。
                return Err("command.failed");
            }
        }
    }
    Ok(())
}

fn put(entries: &mut Vec<(String, Tag)>, key: String, value: Tag) {
    match entries.iter_mut().find(|(name, _)| *name == key) {
        Some((_, slot)) => *slot = value,
        None => entries.push((key, value)),
    }
}

/// 给 `Region` 里放一个能存数据的容器，供测试和 `/data` 的方块目标使用。
#[cfg(test)]
pub(crate) fn put_test_chest(region: &mut Region, pos: BlockPos) {
    region.chunks.set_block(pos, mc_bamboo_data::blocks::CHEST);
    region.containers.insert(
        pos,
        crate::container::ContainerEntry::new(crate::container::ContainerKind::Chest),
    );
}
