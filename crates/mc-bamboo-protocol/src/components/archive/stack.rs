//! `ItemStack.CODEC`、`ItemStackTemplate.CODEC` 和 `DataComponentPatch.CODEC`。
//!
//! 物品堆的 `count` 用 `optionalAlwaysPresentFieldOf`，1 也要写出来。模板的 `count`
//! 用 `optionalFieldOf`，等于 1 就省略。`components` 也是 `optionalFieldOf`，但是
//! 官方在编码前用补丁对象和空补丁做 `Objects.equals`：只含瞬态组件的补丁仍然写出
//! `components`，补丁编码器再把瞬态丢掉，留下空复合标签。补丁的键是组件注册名，
//! 移除项前面加 `!`，值是空复合标签。哈希不跳过瞬态：算不出来就算不匹配。
//!
//! `ItemStack.CODEC`, `ItemStackTemplate.CODEC`, and `DataComponentPatch.CODEC`.
//!
//! A stack's `count` is `optionalAlwaysPresentFieldOf`, so 1 is still written. A
//! template's `count` is `optionalFieldOf` and is omitted when it is 1. `components`
//! is also `optionalFieldOf`, but the official codec compares the patch object with
//! the empty patch before encoding. A patch that only holds transient components is
//! still written; the patch codec then drops those components and leaves `{}`. Patch
//! keys are component ids, removals use a `!` prefix, and a removal's value is an
//! empty compound. Hashing does not skip transients: a value that cannot be hashed
//! does not match.

use std::collections::BTreeSet;

use mc_bamboo_data::{items::AIR, registries::data_component_type as kinds};
use mc_bamboo_nbt::{Accounter, DEFAULT_QUOTA, Reader, Tag};

use super::{
    components,
    dynamic::Dyn,
    read::{self, Error, Result},
    write::{self, Fields},
};
use crate::components::{
    ComponentPatch, PatchEntry,
    cursor::{MAX_STACK_DEPTH, Registry},
    stack::ItemStackTemplate,
};

/// 官方 `HashedPatchMap` 的两项：加上的组件是（类型 ID，哈希），另一项是移除的类型 ID。
///
/// The two lists in `HashedPatchMap`: added components are (type id, hash), and the
/// other list is the removed type ids.
pub struct HashedComponents {
    pub added: Vec<(i32, i32)>,
    pub removed: Vec<i32>,
}

/// 官方编码再由我们解码后写回去的物品堆，外加 `HashedPatchMap` 的两项。
///
/// An item stack after a Bamboo decode/encode, plus the two `HashedPatchMap` lists.
pub struct StackNbt {
    pub bytes: Vec<u8>,
    pub added: Vec<(i32, i32)>,
    pub removed: Vec<i32>,
}

/// 再往下嵌一层物品。和网络编解码一样，最多 [`MAX_STACK_DEPTH`] 层。
///
/// One more nested item. Same cap as the network codecs, [`MAX_STACK_DEPTH`].
pub(super) fn deeper(depth: u32) -> Result<u32> {
    if depth >= MAX_STACK_DEPTH {
        Err(Error::new("item stack nested too deeply"))
    } else {
        Ok(depth + 1)
    }
}

/// 官方 `ItemStack.CODEC`。
pub fn encode_stack(item: i32, count: i32, patch: &ComponentPatch) -> Result<Dyn> {
    encode_item(item, count, patch, 0, true)
}

/// 官方 `ItemStack.CODEC` 的解码。
pub fn decode_stack(tag: &Tag) -> Result<(i32, i32, ComponentPatch)> {
    decode_item(tag, 0)
}

pub(super) fn encode_template(template: &ItemStackTemplate, depth: u32) -> Result<Dyn> {
    let depth = deeper(depth)?;
    encode_item(
        template.item,
        template.count,
        &template.components,
        depth,
        false,
    )
}

pub(super) fn decode_template(tag: &Tag, depth: u32) -> Result<ItemStackTemplate> {
    let depth = deeper(depth)?;
    if let Tag::String(_) = tag {
        let item = read::registry_id(Registry::ITEM, tag)?;
        if item == AIR {
            return Err(Error::new("item stack is empty"));
        }
        return Ok(ItemStackTemplate {
            item,
            count: 1,
            components: ComponentPatch::new(),
        });
    }
    let (item, count, components) = decode_item(tag, depth)?;
    Ok(ItemStackTemplate {
        item,
        count,
        components,
    })
}

pub(super) fn encode_patch(patch: &ComponentPatch, depth: u32) -> Result<Dyn> {
    let mut entries = Vec::new();
    for (kind, entry) in patch.entries() {
        if is_transient(*kind) {
            continue;
        }
        let name = read::id_name(Registry::DATA_COMPONENT_TYPE, *kind)?;
        match entry {
            PatchEntry::Removed => entries.push((format!("!{name}"), Dyn::empty_map())),
            PatchEntry::Set(value) => {
                entries.push((name.to_owned(), components::encode_value(value, depth)?));
            }
        }
    }
    Ok(Dyn::Map(entries))
}

pub(super) fn decode_patch(tag: &Tag, depth: u32) -> Result<ComponentPatch> {
    let entries = read::compound(tag)?;
    let mut patch = ComponentPatch::new();
    for (key, value) in entries {
        let (removed, raw) = match key.strip_prefix('!') {
            Some(raw) => (true, raw),
            None => (false, key.as_str()),
        };
        let name = crate::components::cursor::parse_identifier(raw)
            .map_err(|_| Error::new(format!("{raw} is not a resource location")))?;
        let kind = read::name_id(Registry::DATA_COMPONENT_TYPE, &name)
            .ok_or_else(|| Error::new(format!("no component {name}")))?;
        if is_transient(kind) {
            return Err(Error::new(format!("{name} is not a persistent component")));
        }
        if removed {
            if !matches!(value, Tag::Compound(_)) {
                return Err(Error::new(format!(
                    "removed component {name} is not a compound"
                )));
            }
            patch.put(kind, PatchEntry::Removed);
        } else {
            patch.put(
                kind,
                PatchEntry::Set(components::decode_value(kind, value, depth)?),
            );
        }
    }
    Ok(patch)
}

/// 官方 `HashedPatchMap.matches`。算不出哈希（瞬态组件，或者这种值没有存档 `Codec`）
/// 时返回假，调用方会多同步一次槽位。
///
/// `HashedPatchMap.matches`. A value that cannot be hashed does not match, so the
/// slot is sent again.
pub fn matches_hashed(patch: &ComponentPatch, added: &[(i32, i32)], removed: &[i32]) -> bool {
    let Ok(ours) = hashes(patch) else {
        return false;
    };
    if ours.added.len() != added.len() || ours.removed.len() != removed.len() {
        return false;
    }
    let removed_set: BTreeSet<i32> = ours.removed.into_iter().collect();
    if removed_set.len() != removed.len() || removed.iter().any(|kind| !removed_set.contains(kind))
    {
        return false;
    }
    let added_set: BTreeSet<(i32, i32)> = ours.added.into_iter().collect();
    if added_set.len() != added.len() {
        return false;
    }
    added.iter().all(|entry| added_set.contains(entry))
}

/// 官方 `HashedPatchMap.create` 里每一项的哈希。算不出来（瞬态组件，或没有存档
/// `Codec`）时返回 [`None`]。
///
/// The per-component hashes from `HashedPatchMap.create`. [`None`] when a value
/// cannot be hashed, such as a transient component.
pub fn component_hashes(patch: &ComponentPatch) -> Option<HashedComponents> {
    hashes(patch).ok()
}

/// 把官方 `DataComponentPatch.CODEC` 的网络 NBT 解出来再编回去。
///
/// Decode and re-encode a `DataComponentPatch.CODEC` network tag.
pub fn transcode_patch(bytes: &[u8]) -> Result<Vec<u8>> {
    let tag = read_nbt(bytes)?;
    let patch = decode_patch(&tag, 0)?;
    let mut encoded = Vec::new();
    encode_patch(&patch, 0)?
        .to_tag()
        .write_network(&mut encoded);
    Ok(encoded)
}

/// 把官方 `ItemStack.CODEC` 的网络 NBT 解出来再编回去，并算出组件哈希。
///
/// Decode and re-encode an `ItemStack.CODEC` network tag, and hash its components.
pub fn transcode_stack(bytes: &[u8]) -> Result<StackNbt> {
    let tag = read_nbt(bytes)?;
    let (item, count, patch) = decode_stack(&tag)?;
    let mut encoded = Vec::new();
    encode_stack(item, count, &patch)?
        .to_tag()
        .write_network(&mut encoded);
    let hashed = hashes(&patch)?;
    Ok(StackNbt {
        bytes: encoded,
        added: hashed.added,
        removed: hashed.removed,
    })
}

/// 两段网络 NBT 是不是同一棵标签。复合标签的键序不算。
///
/// Whether two network tags are the same value. Compound key order is ignored.
pub fn nbt_equal(left: &[u8], right: &[u8]) -> bool {
    match (read_nbt(left), read_nbt(right)) {
        (Ok(left), Ok(right)) => tags_equal(&left, &right),
        _ => false,
    }
}

fn encode_item(
    item: i32,
    count: i32,
    patch: &ComponentPatch,
    depth: u32,
    always_count: bool,
) -> Result<Dyn> {
    if item == AIR {
        return Err(Error::new("item stack is empty"));
    }
    let count = write::range_i32(count, 1, 99)?;
    // 官方 `optionalFieldOf("components", EMPTY)` 比的是编码前的补丁对象。
    // 只含瞬态组件时对象不是空补丁，字段照样写；编码再把瞬态丢掉，值变成 `{}`。
    //
    // `optionalFieldOf("components", EMPTY)` compares the patch object before
    // encoding. A transient-only patch is not empty, so the field is written, and
    // encoding then drops those components and leaves `{}`.
    let mut fields = Fields::new().put("id", write::holder_name(Registry::ITEM, item)?);
    if !patch.entries().is_empty() {
        fields = fields.put("components", encode_patch(patch, depth)?);
    }
    Ok(if always_count {
        fields.always("count", Dyn::Int(count))
    } else {
        fields.default("count", Dyn::Int(count), &Dyn::Int(1))
    }
    .build())
}

fn decode_item(tag: &Tag, depth: u32) -> Result<(i32, i32, ComponentPatch)> {
    let entries = read::compound(tag)?;
    let item = read::registry_id(Registry::ITEM, read::required(entries, "id")?)?;
    if item == AIR {
        return Err(Error::new("item stack is empty"));
    }
    let count = match read::field(entries, "count") {
        Some(tag) => write::range_i32(read::int(tag)?, 1, 99)?,
        None => 1,
    };
    let patch = match read::field(entries, "components") {
        Some(tag) => decode_patch(tag, depth)?,
        None => ComponentPatch::new(),
    };
    Ok((item, count, patch))
}

fn hashes(patch: &ComponentPatch) -> Result<HashedComponents> {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    for (kind, entry) in patch.entries() {
        match entry {
            PatchEntry::Removed => removed.push(*kind),
            PatchEntry::Set(value) => {
                if is_transient(*kind) {
                    return Err(Error::new("transient component cannot be hashed"));
                }
                added.push((*kind, components::encode_value(value, 0)?.hash_int()));
            }
        }
    }
    Ok(HashedComponents { added, removed })
}

pub(super) fn is_transient(kind: i32) -> bool {
    matches!(
        kind,
        kinds::CREATIVE_SLOT_LOCK | kinds::ADDITIONAL_TRADE_COST | kinds::MAP_POST_PROCESSING
    )
}

fn read_nbt(bytes: &[u8]) -> Result<Tag> {
    let mut reader = Reader::new(bytes, Accounter::new(DEFAULT_QUOTA));
    let tag = reader
        .tag()
        .map_err(|error| Error::new(error.to_string()))?;
    if reader.read() != bytes.len() {
        return Err(Error::new("trailing nbt"));
    }
    Ok(tag)
}

fn tags_equal(left: &Tag, right: &Tag) -> bool {
    match (left, right) {
        (Tag::Float(left), Tag::Float(right)) => left.to_bits() == right.to_bits(),
        (Tag::Double(left), Tag::Double(right)) => left.to_bits() == right.to_bits(),
        (Tag::List(left), Tag::List(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| tags_equal(left, right))
        }
        (Tag::Compound(left), Tag::Compound(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, value)| {
                    right
                        .iter()
                        .find(|(other, _)| other == key)
                        .is_some_and(|(_, other)| tags_equal(value, other))
                })
        }
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::ComponentValue;
    use mc_bamboo_data::items::STONE;

    #[test]
    fn transient_patch_still_writes_empty_components() {
        let mut patch = ComponentPatch::new();
        patch.put(
            kinds::CREATIVE_SLOT_LOCK,
            PatchEntry::Set(ComponentValue::CreativeSlotLock),
        );
        let Tag::Compound(entries) = encode_stack(STONE, 1, &patch).unwrap().to_tag() else {
            panic!("not a compound");
        };
        let components = entries
            .iter()
            .find(|(key, _)| key == "components")
            .map(|(_, value)| value);
        assert_eq!(components, Some(&Tag::Compound(Vec::new())));
    }

    #[test]
    fn present_empty_components_do_not_round_trip() {
        // 解出来是空补丁，再编码时官方也会把这个键省掉。
        // Decoding yields an empty patch, and encoding that omits the key.
        let tag = Tag::Compound(vec![
            (
                String::from("id"),
                Tag::String(String::from("minecraft:stone")),
            ),
            (String::from("components"), Tag::Compound(Vec::new())),
            (String::from("count"), Tag::Int(1)),
        ]);
        let (item, count, patch) = decode_stack(&tag).unwrap();
        assert!(patch.entries().is_empty());
        let Tag::Compound(entries) = encode_stack(item, count, &patch).unwrap().to_tag() else {
            panic!("not a compound");
        };
        assert!(entries.iter().all(|(key, _)| key != "components"));
    }
}
