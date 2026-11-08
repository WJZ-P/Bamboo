//! 文本组件的存档编码。
//!
//! 网络上的文本是已经按 `ComponentSerialization.CODEC` 写好的 NBT，键序必须原样留着
//! 才能逐字节往返。存档哈希不能直接拿这份 NBT 去算：样式里的布尔在 NBT 里是字节，
//! 在 `HashOps` 里是布尔；`show_item` 里的物品组件也要走组件自己的 `Codec`。
//! 所以这里沿着已知的布尔和嵌套物品把标签抬成 [`super::Dyn`]，其余字段原样保留。
//!
//! Archive encoding for chat components.
//!
//! The stored tag is already the NBT `ComponentSerialization.CODEC` produced, and its
//! key order has to survive a byte-for-byte network round trip. Hashing that tag
//! directly is wrong: style booleans are bytes in NBT and booleans in `HashOps`,
//! and the item inside `show_item` has to go through the item-component codecs.
//! Known booleans and nested items are lifted into [`super::Dyn`]; every other
//! field is kept as it was stored.

use mc_bamboo_nbt::Tag;

use super::{
    dynamic::Dyn,
    read::{self, Error, Result},
    stack,
};

/// 把一段文本编成存档中间值。
///
/// Encode one component into the archive value.
pub fn encode(component: &crate::text::Component, depth: u32) -> Result<Dyn> {
    crate::stack_hop::guard(|| encode_tag(component.tag(), depth))
}

/// 从存档标签还原一段文本。只接受字符串、非空列表或复合标签，内容和官方的
/// `ComponentSerialization.CODEC` 一样。
///
/// Decode a component tag. A string, a non-empty list, or a compound, matching
/// `ComponentSerialization.CODEC`.
pub fn decode(tag: &Tag) -> Result<crate::text::Component> {
    match tag {
        Tag::String(_) | Tag::Compound(_) => Ok(crate::text::Component::from_tag(tag.clone())),
        Tag::List(items) if !items.is_empty() => Ok(crate::text::Component::from_tag(tag.clone())),
        Tag::List(_) => Err(Error::new("text component list is empty")),
        _ => Err(Error::new("text component has the wrong shape")),
    }
}

fn encode_tag(tag: &Tag, depth: u32) -> Result<Dyn> {
    crate::stack_hop::guard(|| encode_tag_frame(tag, depth))
}

fn encode_tag_frame(tag: &Tag, depth: u32) -> Result<Dyn> {
    match tag {
        Tag::String(text) => Ok(Dyn::String(text.clone())),
        Tag::List(items) => {
            if items.is_empty() {
                return Err(Error::new("text component list is empty"));
            }
            let values = items
                .iter()
                .map(|item| encode_tag(item, depth))
                .collect::<Result<Vec<_>>>()?;
            Ok(Dyn::List(values))
        }
        Tag::Compound(entries) => encode_compound(entries, depth),
        _ => Err(Error::new("text component has the wrong shape")),
    }
}

fn encode_compound(entries: &[(String, Tag)], depth: u32) -> Result<Dyn> {
    let mut out = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let encoded = match key.as_str() {
            "bold" | "italic" | "underlined" | "strikethrough" | "obfuscated" | "interpret"
            | "plain" | "hat" => Dyn::Bool(read::bool_value(value)?),
            "extra" => encode_tag(value, depth)?,
            "with" => encode_arguments(value, depth)?,
            "separator" | "fallback" => encode_tag(value, depth)?,
            "hover_event" => encode_hover(value, depth)?,
            "click_event" | "score" => Dyn::from_tag(value),
            _ => Dyn::from_tag(value),
        };
        out.push((key.clone(), encoded));
    }
    Ok(Dyn::Map(out))
}

fn encode_arguments(tag: &Tag, depth: u32) -> Result<Dyn> {
    let items = read::list(tag)?;
    let mut values = Vec::with_capacity(items.len());
    for item in &items {
        values.push(match item {
            Tag::Compound(_) | Tag::List(_) => encode_tag(item, depth)?,
            other => Dyn::from_tag(other),
        });
    }
    Ok(Dyn::List(values))
}

fn encode_hover(tag: &Tag, depth: u32) -> Result<Dyn> {
    let entries = read::compound(tag)?;
    let action = read::string(read::required(entries, "action")?)?;
    match action {
        "show_item" => encode_show_item(entries, depth),
        "show_text" => rewrite(entries, "value", |value| encode_tag(value, depth)),
        "show_entity" => rewrite(entries, "name", |value| encode_tag(value, depth)),
        _ => Ok(Dyn::from_tag(tag)),
    }
}

/// `show_item` 用的是 `ItemStackTemplate.MAP_CODEC`，数量为 1 和空补丁都省略。
/// 补丁里的组件要重新走存档 `Codec`，布尔才会变成 `HashOps` 的布尔。
///
/// `show_item` uses `ItemStackTemplate.MAP_CODEC`: count 1 and an empty patch are
/// omitted. The patch is re-encoded so its booleans become `HashOps` booleans.
fn encode_show_item(entries: &[(String, Tag)], depth: u32) -> Result<Dyn> {
    let nested = stack::deeper(depth)?;
    let mut out = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        match key.as_str() {
            "count" => {
                let count = read::int(value)?;
                if !(1..=99).contains(&count) {
                    return Err(Error::new(format!("item count {count} is not in [1;99]")));
                }
                if count != 1 {
                    out.push((key.clone(), Dyn::Int(count)));
                }
            }
            "components" => {
                let patch = stack::decode_patch(value, nested)?;
                let encoded = stack::encode_patch(&patch, nested)?;
                if !encoded.same(&Dyn::empty_map()) {
                    out.push((key.clone(), encoded));
                }
            }
            _ => out.push((key.clone(), Dyn::from_tag(value))),
        }
    }
    Ok(Dyn::Map(out))
}

fn rewrite(
    entries: &[(String, Tag)],
    key: &str,
    encode: impl Fn(&Tag) -> Result<Dyn>,
) -> Result<Dyn> {
    let mut out = Vec::with_capacity(entries.len());
    for (name, value) in entries {
        if name == key {
            out.push((name.clone(), encode(value)?));
        } else {
            out.push((name.clone(), Dyn::from_tag(value)));
        }
    }
    Ok(Dyn::Map(out))
}
