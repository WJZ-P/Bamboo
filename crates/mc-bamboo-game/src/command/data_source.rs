//! `/data` 和 `/execute store/if data` 共用的 NBT 数据源。
//!
//! 实体读写走 `mc-bamboo-blocks::archive::entity`（`Entity.saveWithoutId` / `load`），
//! 方块实体走同一层的容器 / 组件表和 `archive::create_block_entity` 工厂，
//! storage 是全服一份的 `CommandStorage`：权威副本在 `Global`，按命名空间写进
//! `data/<ns>/command_storage.dat`，开服读回（见 `SaveSession`）。
//!
//! NBT sources shared by `/data` and `/execute store/if data`. Entities use the
//! archive codec, block entities use the same game tables and factory that
//! archive writes, and command storage lives in `Global` and is persisted per
//! namespace by `SaveSession`.

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_blocks::{
    archive::{
        block_entity::create_block_entity, read_entity, write_block_entity_additional,
        write_entity_tree,
    },
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

/// 官方 `CommandStorage`：按命名空间分文件，每个文件存该命名空间下所有路径的
/// 复合标签。每个命名空间单独标脏，存盘时只写改过的文件。
///
/// Official `CommandStorage`: one saved-data file per namespace, holding every
/// path's compound. Each namespace is dirtied separately and only dirty files
/// are written.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommandStorage {
    tags: BTreeMap<String, Tag>,
    dirty: BTreeSet<String>,
}

impl CommandStorage {
    pub fn get(&self, id: &str) -> Tag {
        self.tags
            .get(id)
            .cloned()
            .unwrap_or_else(|| Tag::Compound(Vec::new()))
    }

    pub fn set(&mut self, id: &str, tag: Tag) {
        let namespace = id
            .split_once(':')
            .map_or("minecraft", |(ns, _)| ns)
            .to_owned();
        if matches!(&tag, Tag::Compound(entries) if entries.is_empty()) {
            self.tags.remove(id);
        } else {
            self.tags.insert(id.to_owned(), tag);
        }
        self.dirty.insert(namespace);
    }

    /// 官方 `SavedDataStorage.readSavedData`：从磁盘读回来的 `contents` 套进运行时表。
    /// 整份坏数据作废（官方 `resultOrPartial` 返回空），不静默丢单条。
    pub fn load_namespace(&mut self, namespace: &str, tag: &Tag) {
        let Tag::Compound(entries) = tag else {
            return;
        };
        let Some((_, Tag::Compound(contents))) = entries.iter().find(|(k, _)| k == "contents")
        else {
            return;
        };
        for (path, value) in contents {
            if !matches!(value, Tag::Compound(_)) || path.contains('/') || path.contains('.') {
                return;
            }
        }
        for (path, value) in contents {
            self.tags
                .insert(format!("{namespace}:{path}"), value.clone());
        }
    }

    /// 没有任何内容的命名空间：官方删掉空 container 的键，文件里留一份空
    /// `contents`；Bamboo 显式写空文件，重启读回来仍是空。
    pub fn namespaces(&self) -> Vec<String> {
        let mut out: BTreeSet<String> = self
            .tags
            .keys()
            .map(|id| {
                id.split_once(':')
                    .map_or("minecraft", |(ns, _)| ns)
                    .to_owned()
            })
            .collect();
        out.extend(self.dirty.iter().cloned());
        out.into_iter().collect()
    }

    pub fn namespace_tag(&self, namespace: &str) -> Tag {
        let entries: Vec<_> = self
            .tags
            .iter()
            .filter(|(id, _)| id.split_once(':').map_or("minecraft", |(ns, _)| ns) == namespace)
            .map(|(id, tag)| {
                let path = id.split_once(':').map_or(id.as_str(), |(_, p)| p);
                (path.to_owned(), tag.clone())
            })
            .collect();
        Tag::Compound(vec![("contents".to_owned(), Tag::Compound(entries))])
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
                    let root = entity_row(region, uid).ok_or("argument.entity.notfound.entity")?;
                    // 官方 `Entity.saveWithoutId` 给载具写嵌套 `Passengers`：乘客要按
                    // 同一棵树序列化，不能只写这一行。
                    // Vanilla `Entity.saveWithoutId` writes nested `Passengers`
                    // for a vehicle, so serialize the whole riding tree.
                    let lookup = |passenger: i32| {
                        let row = region.entities.row_of(passenger)?;
                        region.entities.row_at(row).cloned()
                    };
                    write_entity_tree(&root, &lookup).map_err(|_| "command.failed")
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
pub(crate) fn block_entity_tag(region: &Region, pos: BlockPos) -> Tag {
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
    // 告示牌文字、讲台上的书：都在游戏层的运行时表里，读出来给 `/data get`。
    if let Some(sign) = region.chunks.sign(pos) {
        put(
            &mut entries,
            "front_text".to_owned(),
            sign_text(&sign.front),
        );
        put(&mut entries, "back_text".to_owned(), sign_text(&sign.back));
        put(
            &mut entries,
            "is_waxed".to_owned(),
            Tag::Byte(i8::from(sign.waxed)),
        );
    }
    // 活方块实体（熔炉、酿造台、漏斗、营火、活塞等）的 `saveAdditional` 键
    // 由存档层的 `write_additional` 提供，直接并入可读标签，不另写一套 codec。
    if let Some(entity) = region.block_entities.find_dyn(pos)
        && let Ok(additional) = write_block_entity_additional(entity)
    {
        for (key, value) in additional {
            put(&mut entries, key, value);
        }
    }
    if let Some(book) = region.lecterns.books.get(&pos) {
        let (stack, page) = book.archive();
        if let Ok(tag) = slots::encode_item(stack) {
            put(&mut entries, "Book".to_owned(), tag);
        }
        put(&mut entries, "Page".to_owned(), Tag::Int(page));
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
            // 活方块实体（熔炉、酿造台、漏斗、营火、活塞等）走
            // `archive::create_block_entity` 的读盘工厂：它不是另写一套 codec，
            // 而是把改过的整条标签重新交给存档层解一遍，再放回 `BlockEntities`。
            // 游戏层容器 / 组件表也共享这批键，上面的分支已优先处理。
            _ if LIVE_BLOCK_ENTITY_KEYS.contains(&key.as_str()) => {
                let id = block_entity_id(region, pos)?;
                let state =
                    mc_bamboo_blocks::state::BlockState(region.chunks.block(pos).unwrap_or(0));
                let rebuilt = create_block_entity(id, pos, state, tag)
                    .map_err(|_| "command.failed")?
                    .ok_or("command.failed")?;
                region.block_entities.insert(rebuilt);
            }
            // 告示牌文字与讲台书页由游戏层的运行时表拥有。
            "front_text" | "back_text" | "is_waxed" => {
                write_sign(region, pos, tag)?;
            }
            "Book" | "Page" => {
                write_lectern(region, pos, tag)?;
            }
            // 读出来但不该由 `/data` 回写的键（组件表里的 lock / 旗帜图案 /
            // 头颅 profile 等）明确报错，不静默丢。
            _ => return Err("command.failed"),
        }
    }
    Ok(())
}

/// 官方 `saveAdditional` 里活跃方块实体会写的键；出现这些键就走存档工厂回写。
const LIVE_BLOCK_ENTITY_KEYS: &[&str] = &[
    "TransferCooldown",
    "cooking_time_spent",
    "cooking_total_time",
    "lit_time_remaining",
    "lit_total_time",
    "speed_multiplier",
    "RecipesUsed",
    "BrewTime",
    "total_brew_time",
    "Fuel",
    "total_fuel",
    "CookingTimes",
    "CookingTotalTimes",
    "blockState",
    "facing",
    "extending",
    "source",
    "progress",
];

fn block_entity_id(region: &Region, pos: BlockPos) -> Result<&str, &'static str> {
    let state = mc_bamboo_blocks::state::BlockState(region.chunks.block(pos).unwrap_or(0));
    let id = state
        .block()
        .block_entity_type
        .and_then(mc_bamboo_data::registries::block_entity_type::name_of)
        .ok_or("commands.data.block.invalid")?;
    Ok(id)
}

fn write_sign(region: &mut Region, pos: BlockPos, tag: &Tag) -> Result<(), &'static str> {
    use mc_bamboo_core::sign::SignContent;
    let mut content = region.chunks.sign(pos).unwrap_or(SignContent::BLANK);
    for (key, value) in tag_entries(tag) {
        match key.as_str() {
            "is_waxed" => content.waxed = matches!(value, Tag::Byte(v) if *v != 0),
            "front_text" => content.front = parse_sign_text(value)?,
            "back_text" => content.back = parse_sign_text(value)?,
            _ => {}
        }
    }
    region.chunks.set_sign(pos, content);
    Ok(())
}

fn parse_sign_text(value: &Tag) -> Result<mc_bamboo_core::sign::SignText, &'static str> {
    use mc_bamboo_core::sign::{DyeColor, SignText};
    let mut out = SignText::EMPTY;
    for (key, value) in tag_entries(value) {
        match key.as_str() {
            "messages" => {
                let Tag::List(items) = value else {
                    return Err("command.failed");
                };
                for (index, item) in items.iter().take(4).enumerate() {
                    if let Tag::String(text) = item {
                        out.messages[index] = text.clone();
                    }
                }
            }
            "color" => {
                if let Tag::String(name) = value
                    && let Some(color) = DyeColor::ALL.iter().copied().find(|c| c.name() == name)
                {
                    out.color = color;
                }
            }
            "has_glowing_text" => out.glowing = matches!(value, Tag::Byte(v) if *v != 0),
            _ => {}
        }
    }
    Ok(out)
}

fn write_lectern(region: &mut Region, pos: BlockPos, tag: &Tag) -> Result<(), &'static str> {
    let mut stack = mc_bamboo_protocol::item::ItemStack::empty();
    let mut page = 0;
    for (key, value) in tag_entries(tag) {
        match key.as_str() {
            "Book" => stack = slots::decode_item(value).map_err(|_| "command.failed")?,
            "Page" => {
                if let Tag::Int(value) = value {
                    page = *value;
                }
            }
            _ => {}
        }
    }
    let facing = mc_bamboo_blocks::state::BlockState(region.chunks.block(pos).unwrap_or(0))
        .try_get(mc_bamboo_blocks::state::FACING)
        .unwrap_or(mc_bamboo_core::Direction::North);
    region
        .lecterns
        .books
        .insert(pos, crate::lectern::Book::from_archive(stack, page, facing));
    Ok(())
}

fn tag_entries(tag: &Tag) -> &[(String, Tag)] {
    match tag {
        Tag::Compound(entries) => entries,
        _ => &[],
    }
}

/// 官方 `SignText.CODEC`：`messages` / `color` / `has_glowing_text`。
fn sign_text(text: &mc_bamboo_core::sign::SignText) -> Tag {
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
