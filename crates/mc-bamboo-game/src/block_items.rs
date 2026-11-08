//! 方块实体从物品上带来的数据，官方 `BlockEntity.applyComponents` 和 `collectComponents`。
//!
//! 官方每种方块实体用 `applyImplicitComponents` 挑自己认的几项组件，没被挑走的原样留在方块实体
//! 的组件表里（`BlockEntity.components`）。这里只实现认组件的那四类：旗帜、头颅、饰纹陶罐，
//! 还有有物品栏的容器（官方 `BaseContainerBlockEntity`）。多出来的组件我们不留，已知限制见 README。
//!
//! 挑出来的那几项存在 [`Region::block_items`] 里，就是官方 `collectComponents` 的结果，
//! 中键选取方块时原样还给物品。容器的内容不在这里，它在 [`crate::container`]。
//!
//! 客户端要渲染的那几种（旗帜、头颅、陶罐）还要把 `getUpdateTag` 编成 NBT 存进区块：
//! 区块包和 `block_entity_data` 都发它，做法和告示牌一样，见 [`crate::sign`]。

use mc_bamboo_core::{BlockPos, sign::DyeColor};
use mc_bamboo_data::{
    block_of_state,
    registries::{SYNCED_REGISTRIES, block_entity_type as entities, data_component_type as kinds},
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{
        ComponentPatch, ComponentValue, PatchEntry,
        cursor::Holder,
        defaults,
        stack::ItemStackTemplate,
        validate,
        value::{BannerLayer, ResolvableProfile},
    },
    item::ItemStack,
    text::Component,
};
use tracing::debug;
use uuid::Uuid;

use crate::{container, region::Region, sign};

/// 旗帜的 `applyImplicitComponents` 和 `collectImplicitComponents` 读写的组件，按官方的顺序。
const BANNER: [i32; 2] = [kinds::BANNER_PATTERNS, kinds::CUSTOM_NAME];
/// 头颅的那几项。
const SKULL: [i32; 3] = [kinds::PROFILE, kinds::NOTE_BLOCK_SOUND, kinds::CUSTOM_NAME];
/// 饰纹陶罐：官方还读一项 `container`，见 [`apply`]。
const DECORATED_POT: [i32; 1] = [kinds::POT_DECORATIONS];
/// 容器：官方还读一项 `container`，内容进方块实体的物品栏。
const CONTAINER: [i32; 2] = [kinds::CUSTOM_NAME, kinds::LOCK];

/// 官方用 `getOrDefault` 读的那几项：物品上没有时也要留一份空值，`collectImplicitComponents`
/// 又会把它写回物品，所以中键选取一面空白旗帜拿到的物品带着一项空的 `banner_patterns`。
///
/// 用 `get` 读的那几项（名字、档案、音符音效、锁）没有就是没有。
fn empty_value(component: i32) -> Option<ComponentValue> {
    match component {
        kinds::BANNER_PATTERNS => Some(ComponentValue::BannerPatterns(Vec::new())),
        kinds::POT_DECORATIONS => Some(ComponentValue::PotDecorations {
            back: None,
            left: None,
            right: None,
            front: None,
        }),
        _ => None,
    }
}

/// 官方 `LockCode.NO_LOCK`：一个什么都不要求的物品谓词，NBT 里是个空的复合标签。
///
/// 官方 `collectImplicitComponents` 只在 `isLocked()` 时写 `lock`，所以这一项要当没有。
fn is_no_lock(component: i32, value: &ComponentValue) -> bool {
    matches!(
        (component, value),
        (kinds::LOCK, ComponentValue::Lock(Tag::Compound(entries))) if entries.is_empty()
    )
}

/// 这一格的方块实体类型，`minecraft:block_entity_type` 的注册表 ID。
fn block_entity_kind(region: &Region, pos: BlockPos) -> Option<i32> {
    region
        .chunks
        .block(pos)
        .and_then(block_of_state)?
        .block_entity_type
}

/// 这种方块实体认哪几项组件。不认组件的方块实体返回空。
fn implicit_components(kind: i32) -> &'static [i32] {
    match kind {
        entities::BANNER => &BANNER,
        entities::SKULL => &SKULL,
        entities::DECORATED_POT => &DECORATED_POT,
        _ if is_container(kind) => &CONTAINER,
        _ => &[],
    }
}

/// 官方 `BaseContainerBlockEntity` 的子类：自带物品栏、认 `custom_name`、`lock` 和 `container`。
///
/// 末影箱的方块实体不是它的子类（格子在玩家身上），所以不认组件。
fn is_container(kind: i32) -> bool {
    matches!(
        kind,
        entities::CHEST
            | entities::TRAPPED_CHEST
            | entities::BARREL
            | entities::SHULKER_BOX
            | entities::HOPPER
            | entities::DISPENSER
            | entities::DROPPER
    )
}

/// 官方 `BlockItem.updateBlockEntityComponents`：把放下去的物品上的组件搬进方块实体。
///
/// 官方在这之前还有一步 `updateCustomBlockEntityTag`（物品上的 `block_entity_data`），
/// 那一步要解 NBT 形式的方块实体存档，我们不做，见 README 的已知限制。
pub(crate) fn apply(region: &mut Region, pos: BlockPos, stack: &ItemStack) {
    let Some(kind) = block_entity_kind(region, pos) else {
        return;
    };
    let patch = stack.patch();
    let mut implicit = ComponentPatch::new();
    for &component in implicit_components(kind) {
        let value = defaults::resolve(stack.item, &patch, component)
            .cloned()
            .or_else(|| empty_value(component));
        // 官方 `lock` 的默认值 `NO_LOCK` 就是一个空的物品谓词，和没锁一样。
        if let Some(value) = value.filter(|value| !is_no_lock(component, value)) {
            implicit.put(component, PatchEntry::Set(value));
        }
    }

    // 容器的 `container` 组件直接变成方块实体的物品栏，官方 `ItemContainerContents.copyInto`。
    if is_container(kind) {
        let contents = match defaults::resolve(stack.item, &patch, kinds::CONTAINER) {
            Some(ComponentValue::Container(slots)) => slots.as_slice(),
            _ => &[],
        };
        copy_into_container(region, pos, contents);
    }

    if implicit.entries().is_empty() {
        region.block_items.remove(&pos);
    } else {
        region.block_items.insert(pos, implicit);
    }
    refresh_update_tag(region, pos);
}

/// 把 `container` 组件里的物品放进这一格的容器，官方 `copyInto`：每一格都写，多出来的格清空。
fn copy_into_container(region: &mut Region, pos: BlockPos, contents: &[Option<ItemStackTemplate>]) {
    let Some(container) = container::kind_at(region, pos) else {
        return;
    };
    if contents.is_empty() && !region.containers.contains_key(&pos) {
        return;
    }
    let entry = container::entry_at(region, pos, container);
    for slot in 0..entry.slots.len() {
        entry.slots[slot] = contents
            .get(slot)
            .and_then(Option::as_ref)
            .map_or_else(ItemStack::empty, create_stack);
    }
}

/// 官方 `ItemStackTemplate.create`：组件不合法时是空物品堆。
pub(crate) fn create_stack(template: &ItemStackTemplate) -> ItemStack {
    let (added, removed) = template.components.to_opaque();
    let stack = ItemStack {
        item: template.item,
        count: template.count,
        added,
        removed,
    };
    if validate::validate_stack(&stack).is_err() {
        return ItemStack::empty();
    }
    stack
}

/// 官方 `ItemContainerContents.fromItems`：末尾的空格子不写。
fn container_contents(slots: &[ItemStack]) -> ComponentValue {
    let last = slots.iter().rposition(|stack| !stack.is_empty());
    let items = last.map_or_else(Vec::new, |last| {
        slots[..=last]
            .iter()
            .map(|stack| {
                (!stack.is_empty()).then(|| ItemStackTemplate {
                    item: stack.item,
                    count: stack.count,
                    components: stack.patch(),
                })
            })
            .collect()
    });
    ComponentValue::Container(items)
}

/// 官方 `BlockEntity.collectComponents`：方块实体身上的组件，中键选取时还给物品。
pub(crate) fn collect(region: &Region, pos: BlockPos) -> ComponentPatch {
    let mut patch = region.block_items.get(&pos).cloned().unwrap_or_default();
    let Some(kind) = block_entity_kind(region, pos) else {
        return patch;
    };
    if is_container(kind) {
        let slots = region
            .containers
            .get(&pos)
            .map_or(&[][..], |entry| entry.slots.as_slice());
        patch.put(kinds::CONTAINER, PatchEntry::Set(container_contents(slots)));
    }
    // 陶罐里的物品我们不存，官方这里放的是罐子里那一格。
    if kind == entities::DECORATED_POT {
        patch.put(
            kinds::CONTAINER,
            PatchEntry::Set(ComponentValue::Container(Vec::new())),
        );
    }
    patch
}

/// 这一格方块实体的自定义名，官方 `Nameable.getCustomName`。
pub(crate) fn custom_name(region: &Region, pos: BlockPos) -> Option<&Component> {
    match region.block_items.get(&pos)?.get(kinds::CUSTOM_NAME) {
        Some(ComponentValue::CustomName(name)) => Some(name),
        _ => None,
    }
}

/// 方块换了以后忘掉这一格的数据。区块里那份 NBT 由 `Chunk::set_block` 自己丢。
pub(crate) fn forget(region: &mut Region, pos: BlockPos) {
    region.block_items.remove(&pos);
}

/// 重算这一格要发给客户端的 NBT 并写进区块。
pub(crate) fn refresh_update_tag(region: &mut Region, pos: BlockPos) {
    let tag = update_tag(region, pos).map(|tag| {
        let mut out = Vec::new();
        tag.write_network(&mut out);
        out
    });
    region.chunks.set_update_tag(pos, tag);
}

/// 官方 `BlockEntity.getUpdateTag`：客户端要渲染的那几种方块实体发这段 NBT。
///
/// 别的方块实体官方发的是空标签，区块包里写一个 `TAG_End`，所以返回 `None`。
fn update_tag(region: &Region, pos: BlockPos) -> Option<Tag> {
    let kind = block_entity_kind(region, pos)?;
    let patch = region.block_items.get(&pos)?;
    match kind {
        entities::BANNER => Some(banner_tag(patch)),
        entities::SKULL => skull_tag(patch),
        entities::DECORATED_POT => pot_tag(patch),
        _ => None,
    }
}

/// 旗帜的 `getUpdateTag` 走 `saveWithoutMetadata`，所以末尾还有方块实体自己的组件表。
fn banner_tag(patch: &ComponentPatch) -> Tag {
    let mut entries = Vec::new();
    if let Some(ComponentValue::BannerPatterns(layers)) = patch.get(kinds::BANNER_PATTERNS)
        && !layers.is_empty()
    {
        match banner_patterns_tag(layers) {
            Some(tag) => entries.push(("patterns".to_owned(), tag)),
            None => debug!("skipping banner patterns: an unknown pattern cannot be written"),
        }
    }
    if let Some(name) = custom_name_tag(patch) {
        entries.push(("CustomName".to_owned(), name));
    }
    // 方块实体自己的组件表。我们不留物品上多出来的组件，所以这张表永远是空的。
    entries.push(("components".to_owned(), Tag::Compound(Vec::new())));
    Tag::Compound(entries)
}

/// 官方 `BannerPatternLayers.CODEC`：一串 `{pattern, color}`。
fn banner_patterns_tag(layers: &[BannerLayer]) -> Option<Tag> {
    let mut list = Vec::with_capacity(layers.len());
    for layer in layers {
        let pattern = match &layer.pattern {
            Holder::Id(id) => Tag::String(registry_entry("minecraft:banner_pattern", *id)?),
            Holder::Inline(pattern) => Tag::Compound(vec![
                ("asset_id".to_owned(), Tag::String(pattern.asset_id.clone())),
                (
                    "translation_key".to_owned(),
                    Tag::String(pattern.translation_key.clone()),
                ),
            ]),
        };
        list.push(Tag::Compound(vec![
            ("pattern".to_owned(), pattern),
            (
                "color".to_owned(),
                Tag::String(DyeColor::from_id(layer.color).name().to_owned()),
            ),
        ]));
    }
    Some(Tag::List(list))
}

/// 头颅的 `getUpdateTag` 走 `saveCustomOnly`，没有组件表。三项都没有时官方发的是空标签。
fn skull_tag(patch: &ComponentPatch) -> Option<Tag> {
    let mut entries = Vec::new();
    if let Some(ComponentValue::Profile(profile)) = patch.get(kinds::PROFILE) {
        entries.push(("profile".to_owned(), profile_tag(profile)));
    }
    if let Some(ComponentValue::NoteBlockSound(sound)) = patch.get(kinds::NOTE_BLOCK_SOUND) {
        entries.push(("note_block_sound".to_owned(), Tag::String(sound.clone())));
    }
    if let Some(name) = custom_name_tag(patch) {
        entries.push(("custom_name".to_owned(), name));
    }
    (!entries.is_empty()).then_some(Tag::Compound(entries))
}

/// 官方 `ResolvableProfile.CODEC`：档案和皮肤补丁写在同一个复合标签里。
fn profile_tag(profile: &ResolvableProfile) -> Tag {
    let mut entries = Vec::new();
    match &profile.profile {
        // `ExtraCodecs.STORED_GAME_PROFILE`：UUID 和名字都是必填的。
        Ok(full) => {
            entries.push(("id".to_owned(), uuid_tag(full.id)));
            entries.push(("name".to_owned(), Tag::String(full.name.clone())));
            if !full.properties.is_empty() {
                entries.push(("properties".to_owned(), properties_tag(&full.properties)));
            }
        }
        // `ResolvableProfile.Partial.MAP_CODEC`：名字和 UUID 都是可选的。
        Err(partial) => {
            if let Some(name) = &partial.name {
                entries.push(("name".to_owned(), Tag::String(name.clone())));
            }
            if let Some(id) = partial.id {
                entries.push(("id".to_owned(), uuid_tag(id)));
            }
            if !partial.properties.is_empty() {
                entries.push(("properties".to_owned(), properties_tag(&partial.properties)));
            }
        }
    }

    // `PlayerSkin.Patch.MAP_CODEC`，字段名和组件里的顺序不同：`body` 存的键是 `texture`。
    let skin = &profile.skin_patch;
    for (key, asset) in [
        ("texture", &skin.body),
        ("cape", &skin.cape),
        ("elytra", &skin.elytra),
    ] {
        if let Some(asset) = asset {
            entries.push((key.to_owned(), Tag::String(asset.clone())));
        }
    }
    if let Some(slim) = skin.slim {
        let model = if slim { "slim" } else { "wide" };
        entries.push(("model".to_owned(), Tag::String(model.to_owned())));
    }
    Tag::Compound(entries)
}

/// 官方 `ExtraCodecs.PROPERTY_MAP` 写出来的那一份：一串 `{name, value, signature?}`。
fn properties_tag(properties: &[mc_bamboo_protocol::components::value::Property]) -> Tag {
    Tag::List(
        properties
            .iter()
            .map(|property| {
                let mut entries = vec![
                    ("name".to_owned(), Tag::String(property.name.clone())),
                    ("value".to_owned(), Tag::String(property.value.clone())),
                ];
                if let Some(signature) = &property.signature {
                    entries.push(("signature".to_owned(), Tag::String(signature.clone())));
                }
                Tag::Compound(entries)
            })
            .collect(),
    )
}

/// 官方 `UUIDUtil.CODEC`：四个 int，从高位开始。
fn uuid_tag(uuid: Uuid) -> Tag {
    let (high, low) = uuid.as_u64_pair();
    Tag::IntArray(vec![
        (high >> 32) as i32,
        high as i32,
        (low >> 32) as i32,
        low as i32,
    ])
}

/// 饰纹陶罐的 `getUpdateTag`：只有 `sherds`，罐子里的物品我们不存。
fn pot_tag(patch: &ComponentPatch) -> Option<Tag> {
    let ComponentValue::PotDecorations {
        back,
        left,
        right,
        front,
    } = patch.get(kinds::POT_DECORATIONS)?
    else {
        return None;
    };
    let sides = [
        ("back", back),
        ("left", left),
        ("right", right),
        ("front", front),
    ];
    let mut entries = Vec::new();
    for (key, side) in sides {
        let Some(side) = side else { continue };
        let Some(tag) = template_tag(side) else {
            debug!("skipping pot sherds: a sherd carries components we cannot write");
            return None;
        };
        entries.push((key.to_owned(), tag));
    }
    // 官方只在四面都空着的时候不写 `sherds`。
    (!entries.is_empty())
        .then(|| Tag::Compound(vec![("sherds".to_owned(), Tag::Compound(entries))]))
}

/// 官方 `ItemStackTemplate.MAP_CODEC`：`count` 是 1、组件是空的时候都不写。
///
/// 组件不是空的时候返回 `None`：组件的 NBT 形式要整套 DFU 的 `Codec`，我们不写。
fn template_tag(template: &ItemStackTemplate) -> Option<Tag> {
    if !template.components.entries().is_empty() {
        return None;
    }
    let name = mc_bamboo_data::items::ITEMS
        .get(usize::try_from(template.item).ok()?)?
        .name;
    let mut entries = vec![("id".to_owned(), Tag::String(name.to_owned()))];
    if template.count != 1 {
        entries.push(("count".to_owned(), Tag::Int(template.count)));
    }
    Some(Tag::Compound(entries))
}

fn custom_name_tag(patch: &ComponentPatch) -> Option<Tag> {
    match patch.get(kinds::CUSTOM_NAME) {
        Some(ComponentValue::CustomName(name)) => Some(name.tag().clone()),
        _ => None,
    }
}

/// 同步给客户端的注册表里第 `id` 个条目的名字。
fn registry_entry(registry: &str, id: i32) -> Option<String> {
    SYNCED_REGISTRIES
        .iter()
        .find(|synced| synced.name == registry)?
        .entries
        .get(usize::try_from(id).ok()?)
        .map(|&name| name.to_owned())
}

/// 官方 `ServerGamePacketListenerImpl.addBlockDataToItem`：中键选取带上方块实体的数据。
///
/// 先是 `saveCustomOnly` 去掉 `removeComponentsFromTag` 丢掉的键，剩下的进 `block_entity_data`；
/// 再把 `collectComponents` 盖到物品上。我们只有告示牌还剩内容，别的方块实体丢完就空了。
pub(crate) fn add_block_data(region: &Region, pos: BlockPos, stack: &mut ItemStack) {
    let Some(kind) = block_entity_kind(region, pos) else {
        return;
    };
    let mut patch = stack.patch();
    if let Some(data) = block_entity_data(region, pos, kind) {
        patch.put(kinds::BLOCK_ENTITY_DATA, PatchEntry::Set(data));
    }
    for (component, entry) in collect(region, pos).entries() {
        patch.put(*component, entry.clone());
    }
    let (added, removed) = patch.to_opaque();
    stack.added = added;
    stack.removed = removed;
}

/// `saveCustomOnly` 减去 `removeComponentsFromTag` 丢掉的键之后还剩的内容。
fn block_entity_data(region: &Region, pos: BlockPos, kind: i32) -> Option<ComponentValue> {
    if kind != entities::SIGN && kind != entities::HANGING_SIGN {
        return None;
    }
    // 告示牌没有覆盖 `removeComponentsFromTag`，两面文字和上蜡标志都留在 NBT 里。
    let content = region.chunks.sign(pos)?;
    let Tag::Compound(tag) = sign::update_tag_value(&content) else {
        return None;
    };
    Some(ComponentValue::BlockEntityData(
        mc_bamboo_protocol::components::value::TypedEntityData { kind, tag },
    ))
}

#[cfg(test)]
mod tests;
