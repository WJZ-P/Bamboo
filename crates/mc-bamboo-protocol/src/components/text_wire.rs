//! 官方 `ComponentSerialization.CODEC` 的解码和重编码。
//!
//! 解出来的值按 `Codec` 再写回去：丢掉不认识的键，默认值不写，布尔写成字节，
//! 没有样式的纯文本塌成字符串。键序是声明顺序放进新的 `HashMap` 之后的桶序。
//!
//! Decode and re-encode `ComponentSerialization.CODEC`. Unknown keys are dropped,
//! defaults are omitted, booleans become bytes, and plain text with an empty style
//! collapses to a string. Keys are emitted in Java `HashMap` bucket order.

use mc_bamboo_data::component_registries::registry_names::ENTITY_TYPE;
use mc_bamboo_nbt::Tag;
use uuid::Uuid;

use super::{
    archive::{
        self,
        read::{self, identifier},
        stack::{decode_template_tag, encode_template_tag},
    },
    dialog,
    java_hash::{hash_order, iteration_order},
    nbt_path, selector,
    syntax::block_pos,
    value::{GameProfile, PartialProfile, ResolvableProfile},
};
use crate::{codec::DecodeError, text::is_allowed_chat_character};

const MAX_DEPTH: u32 = 512;
const COLORS: [&str; 16] = [
    "black",
    "dark_blue",
    "dark_green",
    "dark_aqua",
    "dark_red",
    "dark_purple",
    "gold",
    "gray",
    "dark_gray",
    "blue",
    "green",
    "aqua",
    "red",
    "light_purple",
    "yellow",
    "white",
];

/// 把一段文本解出来，再写成官方 `Codec` 会写出的标签。
///
/// Decode one component and write the tag the official codec would emit.
pub(crate) fn canonical(tag: &Tag, depth: u32) -> Result<Tag, DecodeError> {
    crate::stack_hop::guard(|| {
        if depth >= MAX_DEPTH {
            return Err(bad("nested too deep"));
        }
        Ok(encode(decode(tag, depth)?))
    })
}

/// `show_item` 和对话框里的物品，按 `ItemStackTemplate.MAP_CODEC` 重写。
///
/// An item inside `show_item` or a dialog, rewritten through `ItemStackTemplate.MAP_CODEC`.
pub(crate) fn item_template(tag: &Tag) -> Result<Tag, DecodeError> {
    let template = decode_template_tag(tag).map_err(|_| bad("show_item is not an item"))?;
    let encoded = encode_template_tag(&template).map_err(|_| bad("show_item is not an item"))?;
    Ok(hash_order(encoded))
}

struct Piece {
    content: Vec<(String, Tag)>,
    extra: Vec<Tag>,
    style: Vec<(String, Tag)>,
}

fn decode(tag: &Tag, depth: u32) -> Result<Piece, DecodeError> {
    crate::stack_hop::guard(|| decode_frame(tag, depth))
}

fn decode_frame(tag: &Tag, depth: u32) -> Result<Piece, DecodeError> {
    match tag {
        Tag::String(text) => Ok(Piece {
            content: vec![("text".to_owned(), Tag::String(text.clone()))],
            extra: Vec::new(),
            style: Vec::new(),
        }),
        Tag::List(items) if !items.is_empty() => {
            let mut parts = Vec::with_capacity(items.len());
            for item in items {
                parts.push(decode(item, depth + 1)?);
            }
            let mut first = parts.remove(0);
            for rest in parts {
                first.extra.push(encode(rest));
            }
            Ok(first)
        }
        Tag::List(_) => Err(bad("empty component list")),
        Tag::Compound(entries) => decode_compound(entries, depth),
        _ => Err(bad("not a string, list or compound")),
    }
}

fn encode(piece: Piece) -> Tag {
    let plain = matches!(piece.content.as_slice(), [(key, Tag::String(_))] if key == "text")
        && piece.extra.is_empty()
        && piece.style.is_empty();
    if plain {
        return piece.content.into_iter().next().unwrap().1;
    }
    let mut fields = piece.content;
    if !piece.extra.is_empty() {
        fields.push(("extra".to_owned(), Tag::List(piece.extra)));
    }
    fields.extend(piece.style);
    finish(fields)
}

fn decode_compound(entries: &[(String, Tag)], depth: u32) -> Result<Piece, DecodeError> {
    let content = match field(entries, "type") {
        Some(Tag::String(kind)) => decode_typed(kind, entries, depth)?,
        Some(_) => return Err(bad("type must be a string")),
        None => decode_fuzzy(entries, depth)?,
    };
    let extra = match field(entries, "extra") {
        Some(Tag::List(items)) if !items.is_empty() => items
            .iter()
            .map(|item| canonical(item, depth + 1))
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(bad("extra must be a non-empty component list")),
        None => Vec::new(),
    };
    Ok(Piece {
        content,
        extra,
        style: style(entries, depth)?,
    })
}

fn decode_typed(
    kind: &str,
    entries: &[(String, Tag)],
    depth: u32,
) -> Result<Vec<(String, Tag)>, DecodeError> {
    match kind {
        "text" | "translatable" | "keybind" | "score" | "selector" | "nbt" | "object" => {
            content(kind, entries, depth)
        }
        _ => Err(bad("unknown content type")),
    }
}

fn decode_fuzzy(entries: &[(String, Tag)], depth: u32) -> Result<Vec<(String, Tag)>, DecodeError> {
    for kind in [
        "text",
        "translatable",
        "keybind",
        "score",
        "selector",
        "nbt",
        "object",
    ] {
        if let Ok(fields) = content(kind, entries, depth) {
            return Ok(fields);
        }
    }
    Err(bad("no matching content type"))
}

fn content(
    kind: &str,
    entries: &[(String, Tag)],
    depth: u32,
) -> Result<Vec<(String, Tag)>, DecodeError> {
    match kind {
        "text" => Ok(vec![(
            "text".to_owned(),
            Tag::String(string_field(entries, "text")?),
        )]),
        "translatable" => translatable(entries, depth),
        "keybind" => Ok(vec![(
            "keybind".to_owned(),
            Tag::String(string_field(entries, "keybind")?),
        )]),
        "score" => score(entries),
        "selector" => selector_content(entries, depth),
        "nbt" => nbt_content(entries, depth),
        "object" => object_content(entries, depth),
        _ => Err(bad("unknown content type")),
    }
}

fn translatable(entries: &[(String, Tag)], depth: u32) -> Result<Vec<(String, Tag)>, DecodeError> {
    let mut fields = vec![(
        "translate".to_owned(),
        Tag::String(string_field(entries, "translate")?),
    )];
    if let Some(Tag::String(fallback)) = field(entries, "fallback") {
        fields.push(("fallback".to_owned(), Tag::String(fallback.clone())));
    }
    if let Some(with) = field(entries, "with") {
        fields.push(("with".to_owned(), arguments(with, depth)?));
    }
    Ok(fields)
}

fn arguments(tag: &Tag, depth: u32) -> Result<Tag, DecodeError> {
    let items = read::list(tag).map_err(|_| bad("with must be a list"))?;
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        values.push(argument(&item, depth)?);
    }
    Ok(Tag::List(values))
}

fn argument(tag: &Tag, depth: u32) -> Result<Tag, DecodeError> {
    if is_number(tag) || matches!(tag, Tag::String(_)) {
        return Ok(tag.clone());
    }
    let encoded = canonical(tag, depth + 1)?;
    Ok(encoded)
}

fn score(entries: &[(String, Tag)]) -> Result<Vec<(String, Tag)>, DecodeError> {
    let Some(Tag::Compound(score)) = field(entries, "score") else {
        return Err(bad("score must be a compound"));
    };
    let name = string_field(score, "name")?;
    let objective = string_field(score, "objective")?;
    Ok(vec![(
        "score".to_owned(),
        finish(vec![
            ("name".to_owned(), Tag::String(name)),
            ("objective".to_owned(), Tag::String(objective)),
        ]),
    )])
}

fn selector_content(
    entries: &[(String, Tag)],
    depth: u32,
) -> Result<Vec<(String, Tag)>, DecodeError> {
    let selector = string_field(entries, "selector")?;
    if !selector::parse(&selector) {
        return Err(bad("selector is invalid"));
    }
    let mut fields = vec![("selector".to_owned(), Tag::String(selector))];
    if let Some(separator) = field(entries, "separator") {
        fields.push(("separator".to_owned(), canonical(separator, depth + 1)?));
    }
    Ok(fields)
}

fn nbt_content(entries: &[(String, Tag)], depth: u32) -> Result<Vec<(String, Tag)>, DecodeError> {
    let path = string_field(entries, "nbt")?;
    if !nbt_path::parse(&path) {
        return Err(bad("nbt path is invalid"));
    }
    let interpret = lenient_bool(field(entries, "interpret"));
    let plain = lenient_bool(field(entries, "plain"));
    if interpret && plain {
        return Err(bad("interpret and plain cannot both be set"));
    }
    let mut fields = vec![("nbt".to_owned(), Tag::String(path))];
    if interpret {
        fields.push(("interpret".to_owned(), Tag::Byte(1)));
    }
    if plain {
        fields.push(("plain".to_owned(), Tag::Byte(1)));
    }
    if let Some(separator) = field(entries, "separator")
        && let Ok(separator) = canonical(separator, depth + 1)
    {
        fields.push(("separator".to_owned(), separator));
    }
    fields.extend(data_source(entries)?);
    Ok(fields)
}

fn data_source(entries: &[(String, Tag)]) -> Result<Vec<(String, Tag)>, DecodeError> {
    let source = field(entries, "source").and_then(|tag| match tag {
        Tag::String(value) => Some(value.as_str()),
        _ => None,
    });
    let try_entity = source.is_none() || source == Some("entity");
    let try_block = source.is_none() || source == Some("block");
    let try_storage = source.is_none() || source == Some("storage");
    if try_entity
        && let Some(entity) = string_field_opt(entries, "entity")
        && selector::parse(&entity)
    {
        return Ok(vec![("entity".to_owned(), Tag::String(entity))]);
    }
    if try_block
        && let Some(block) = string_field_opt(entries, "block")
        && block_pos(&block)
    {
        return Ok(vec![("block".to_owned(), Tag::String(block))]);
    }
    if try_storage
        && let Some(storage) = string_field_opt(entries, "storage")
        && let Ok(storage) = identifier(&Tag::String(storage))
    {
        return Ok(vec![("storage".to_owned(), Tag::String(storage))]);
    }
    if source.is_some() {
        return Err(bad("unknown data source"));
    }
    Err(bad("nbt content needs a data source"))
}

fn object_content(
    entries: &[(String, Tag)],
    depth: u32,
) -> Result<Vec<(String, Tag)>, DecodeError> {
    let object = field(entries, "object").and_then(|tag| match tag {
        Tag::String(value) => Some(value.as_str()),
        _ => None,
    });
    let mut fields = if object.is_none() || object == Some("atlas") {
        match atlas(entries) {
            Some(atlas) => atlas,
            None if object == Some("atlas") => return Err(bad("atlas sprite is invalid")),
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    if fields.is_empty() && (object.is_none() || object == Some("player")) {
        fields = player(entries)?;
    }
    if fields.is_empty() {
        return Err(bad("object content needs an object info"));
    }
    if let Some(fallback) = field(entries, "fallback") {
        fields.push(("fallback".to_owned(), canonical(fallback, depth + 1)?));
    }
    Ok(fields)
}

fn atlas(entries: &[(String, Tag)]) -> Option<Vec<(String, Tag)>> {
    let sprite = identifier(field(entries, "sprite")?).ok()?;
    let atlas = match field(entries, "atlas") {
        Some(tag) => identifier(tag).ok()?,
        None => "minecraft:blocks".to_owned(),
    };
    let mut fields = Vec::new();
    if atlas != "minecraft:blocks" {
        fields.push(("atlas".to_owned(), Tag::String(atlas)));
    }
    fields.push(("sprite".to_owned(), Tag::String(sprite)));
    Some(fields)
}

fn player(entries: &[(String, Tag)]) -> Result<Vec<(String, Tag)>, DecodeError> {
    let Some(profile) = field(entries, "player") else {
        return Ok(Vec::new());
    };
    let decoded =
        archive::decode_profile_tag(profile).map_err(|_| bad("player profile is invalid"))?;
    let mut fields = vec![("player".to_owned(), profile_tag(&decoded))];
    if field(entries, "hat").is_some() {
        let hat = read::bool_value(field(entries, "hat").unwrap())
            .map_err(|_| bad("hat must be a number"))?;
        if !hat {
            fields.push(("hat".to_owned(), Tag::Byte(0)));
        }
    }
    Ok(fields)
}

fn profile_tag(profile: &ResolvableProfile) -> Tag {
    let mut fields = Vec::new();
    match &profile.profile {
        Ok(GameProfile {
            id,
            name,
            properties,
        }) => {
            fields.push(("id".to_owned(), uuid_ints(*id)));
            fields.push(("name".to_owned(), Tag::String(name.clone())));
            if !properties.is_empty() {
                fields.push(("properties".to_owned(), properties_tag(properties)));
            }
        }
        Err(PartialProfile {
            name,
            id,
            properties,
        }) => {
            if let Some(name) = name {
                fields.push(("name".to_owned(), Tag::String(name.clone())));
            }
            if let Some(id) = id {
                fields.push(("id".to_owned(), uuid_ints(*id)));
            }
            if !properties.is_empty() {
                fields.push(("properties".to_owned(), properties_tag(properties)));
            }
        }
    }
    if let Some(texture) = &profile.skin_patch.body {
        fields.push(("texture".to_owned(), Tag::String(texture.clone())));
    }
    if let Some(cape) = &profile.skin_patch.cape {
        fields.push(("cape".to_owned(), Tag::String(cape.clone())));
    }
    if let Some(elytra) = &profile.skin_patch.elytra {
        fields.push(("elytra".to_owned(), Tag::String(elytra.clone())));
    }
    if let Some(slim) = profile.skin_patch.slim {
        fields.push((
            "model".to_owned(),
            Tag::String(if slim { "slim" } else { "wide" }.to_owned()),
        ));
    }
    finish(fields)
}

fn properties_tag(properties: &[super::value::Property]) -> Tag {
    Tag::List(
        properties
            .iter()
            .map(|property| {
                let mut fields = vec![
                    ("name".to_owned(), Tag::String(property.name.clone())),
                    ("value".to_owned(), Tag::String(property.value.clone())),
                ];
                if let Some(signature) = &property.signature {
                    fields.push(("signature".to_owned(), Tag::String(signature.clone())));
                }
                finish(fields)
            })
            .collect(),
    )
}

fn style(entries: &[(String, Tag)], depth: u32) -> Result<Vec<(String, Tag)>, DecodeError> {
    let mut fields = Vec::new();
    if let Some(color) = field(entries, "color") {
        let Tag::String(color) = color else {
            return Err(bad("color must be a string"));
        };
        fields.push(("color".to_owned(), Tag::String(canonical_color(color)?)));
    }
    if let Some(shadow) = field(entries, "shadow_color") {
        fields.push(("shadow_color".to_owned(), Tag::Int(argb(shadow)?)));
    }
    for name in [
        "bold",
        "italic",
        "underlined",
        "strikethrough",
        "obfuscated",
    ] {
        if let Some(flag) = field(entries, name) {
            let value = read::bool_value(flag).map_err(|_| bad("style flag must be a number"))?;
            fields.push((name.to_owned(), Tag::Byte(if value { 1 } else { 0 })));
        }
    }
    if let Some(click) = field(entries, "click_event") {
        fields.push(("click_event".to_owned(), click_event(click, depth)?));
    }
    if let Some(hover) = field(entries, "hover_event") {
        fields.push(("hover_event".to_owned(), hover_event(hover, depth)?));
    }
    if let Some(insertion) = field(entries, "insertion") {
        let Tag::String(insertion) = insertion else {
            return Err(bad("insertion must be a string"));
        };
        fields.push(("insertion".to_owned(), Tag::String(insertion.clone())));
    }
    if let Some(font) = field(entries, "font") {
        let font = identifier(font).map_err(|_| bad("font is not a resource location"))?;
        fields.push(("font".to_owned(), Tag::String(font)));
    }
    Ok(fields)
}

fn canonical_color(color: &str) -> Result<String, DecodeError> {
    if let Some(hex) = color.strip_prefix('#') {
        let value = i64::from_str_radix(hex, 16).map_err(|_| bad("invalid color value"))?;
        if !(0..=0x00ff_ffff).contains(&value) {
            return Err(bad("color value out of range"));
        }
        return Ok(format!("#{value:06X}"));
    }
    if COLORS.contains(&color) {
        Ok(color.to_owned())
    } else {
        Err(bad("invalid color name"))
    }
}

fn argb(tag: &Tag) -> Result<i32, DecodeError> {
    if is_number(tag) {
        return strict_i32(tag);
    }
    let values =
        read::list(tag).map_err(|_| bad("shadow_color must be a number or four floats"))?;
    if values.len() != 4 || !values.iter().all(is_number) {
        return Err(bad("shadow_color must be a number or four floats"));
    }
    let channel = |tag: &Tag| -> Result<i32, DecodeError> {
        let value =
            read::float(tag).map_err(|_| bad("shadow_color must be a number or four floats"))?;
        Ok(value.mul_add(255.0, 0.0).floor() as i32)
    };
    let red = channel(&values[0])?;
    let green = channel(&values[1])?;
    let blue = channel(&values[2])?;
    let alpha = channel(&values[3])?;
    Ok((alpha & 0xff) << 24 | (red & 0xff) << 16 | (green & 0xff) << 8 | (blue & 0xff))
}

fn click_event(tag: &Tag, depth: u32) -> Result<Tag, DecodeError> {
    let entries = read::compound(tag).map_err(|_| bad("click_event must be a compound"))?;
    let action = string_field(entries, "action")?;
    let mut fields = vec![("action".to_owned(), Tag::String(action.clone()))];
    match action.as_str() {
        "open_url" => fields.push((
            "url".to_owned(),
            Tag::String(dialog_url(string_field(entries, "url")?)?),
        )),
        "run_command" | "suggest_command" => {
            let command = string_field(entries, "command")?;
            if !command.chars().all(is_allowed_chat_character) {
                return Err(bad("disallowed chat character"));
            }
            fields.push(("command".to_owned(), Tag::String(command)));
        }
        "show_dialog" => {
            let dialog = field(entries, "dialog")
                .ok_or_else(|| bad("dialog must be a name or a definition"))?;
            fields.push(("dialog".to_owned(), dialog::rewrite(dialog, depth + 1)?));
        }
        "change_page" => {
            let page = field(entries, "page").ok_or_else(|| bad("page must be a number"))?;
            let page = strict_i32(page)?;
            if page < 1 {
                return Err(bad("page must be positive"));
            }
            fields.push(("page".to_owned(), Tag::Int(page)));
        }
        "copy_to_clipboard" => fields.push((
            "value".to_owned(),
            Tag::String(string_field(entries, "value")?),
        )),
        "custom" => {
            fields.push((
                "id".to_owned(),
                Tag::String(
                    identifier(field(entries, "id").ok_or_else(|| bad("missing id"))?)
                        .map_err(|_| bad("field is not a resource location"))?,
                ),
            ));
            if let Some(payload) = field(entries, "payload") {
                fields.push(("payload".to_owned(), payload.clone()));
            }
        }
        _ => return Err(bad("click event action not allowed")),
    }
    Ok(finish(fields))
}

fn hover_event(tag: &Tag, depth: u32) -> Result<Tag, DecodeError> {
    let entries = read::compound(tag).map_err(|_| bad("hover_event must be a compound"))?;
    let action = string_field(entries, "action")?;
    let mut fields = vec![("action".to_owned(), Tag::String(action.clone()))];
    match action.as_str() {
        "show_text" => {
            let value = field(entries, "value").ok_or_else(|| bad("show_text needs a value"))?;
            fields.push(("value".to_owned(), canonical(value, depth + 1)?));
        }
        "show_item" => {
            let mut item = Vec::new();
            if let Some(id) = field(entries, "id") {
                item.push(("id".to_owned(), id.clone()));
            }
            if let Some(count) = field(entries, "count") {
                item.push(("count".to_owned(), count.clone()));
            }
            if let Some(components) = field(entries, "components") {
                item.push(("components".to_owned(), components.clone()));
            }
            let encoded = item_template(&Tag::Compound(item))?;
            if let Tag::Compound(parts) = encoded {
                for (key, value) in parts {
                    if key != "action" {
                        fields.push((key, value));
                    }
                }
            }
        }
        "show_entity" => {
            let id = identifier(field(entries, "id").ok_or_else(|| bad("missing id"))?)
                .map_err(|_| bad("field is not a resource location"))?;
            if !ENTITY_TYPE.contains(&id.as_str()) {
                return Err(bad("unknown entity type"));
            }
            let uuid =
                field(entries, "uuid").ok_or_else(|| bad("uuid must be a string or four ints"))?;
            fields.push(("id".to_owned(), Tag::String(id)));
            fields.push(("uuid".to_owned(), uuid_tag(uuid)?));
            if let Some(name) = field(entries, "name") {
                fields.push(("name".to_owned(), canonical(name, depth + 1)?));
            }
        }
        _ => return Err(bad("unknown hover event action")),
    }
    Ok(finish(fields))
}

fn uuid_tag(tag: &Tag) -> Result<Tag, DecodeError> {
    let uuid = match tag {
        Tag::String(text) => {
            Uuid::parse_str(text).map_err(|_| bad("uuid must be a string or four ints"))?
        }
        Tag::IntArray(values) if values.len() == 4 => uuid_from_ints(values),
        _ => return Err(bad("uuid must be a string or four ints")),
    };
    Ok(uuid_ints(uuid))
}

fn uuid_ints(uuid: Uuid) -> Tag {
    let (most, least) = uuid.as_u64_pair();
    Tag::IntArray(vec![
        (most >> 32) as i32,
        most as i32,
        (least >> 32) as i32,
        least as i32,
    ])
}

fn uuid_from_ints(values: &[i32]) -> Uuid {
    let most = ((values[0] as u32 as u64) << 32) | values[1] as u32 as u64;
    let least = ((values[2] as u32 as u64) << 32) | values[3] as u32 as u64;
    Uuid::from_u64_pair(most, least)
}

fn dialog_url(url: String) -> Result<String, DecodeError> {
    super::uri::check(&url).map_err(bad)?;
    Ok(url)
}

fn strict_i32(tag: &Tag) -> Result<i32, DecodeError> {
    if !is_number(tag) {
        return Err(bad("expected a number"));
    }
    match tag {
        Tag::Float(value) if !value.is_finite() => return Err(bad("expected a number")),
        Tag::Double(value) if !value.is_finite() => return Err(bad("expected a number")),
        _ => {}
    }
    let value = read::double(tag).map_err(|_| bad("expected a number"))?;
    if !(i32::MIN as f64..=i32::MAX as f64).contains(&value) {
        return Err(bad("number is out of range"));
    }
    Ok(value as i32)
}

fn lenient_bool(tag: Option<&Tag>) -> bool {
    tag.and_then(|tag| read::bool_value(tag).ok())
        .unwrap_or(false)
}

fn string_field(entries: &[(String, Tag)], name: &str) -> Result<String, DecodeError> {
    match field(entries, name) {
        Some(Tag::String(value)) => Ok(value.clone()),
        _ => Err(bad("missing string field")),
    }
}

fn string_field_opt(entries: &[(String, Tag)], name: &str) -> Option<String> {
    match field(entries, name) {
        Some(Tag::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn field<'a>(entries: &'a [(String, Tag)], name: &str) -> Option<&'a Tag> {
    entries
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

fn is_number(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Byte(_) | Tag::Short(_) | Tag::Int(_) | Tag::Long(_) | Tag::Float(_) | Tag::Double(_)
    )
}

fn finish(fields: Vec<(String, Tag)>) -> Tag {
    // 把条目搬进新顺序。克隆后再丢掉会按层递归 `Drop`，512 层把栈打穿。
    //
    // Move entries into the new order. Cloning and then dropping recurses per
    // level, and 512 levels overflow the stack.
    let keys: Vec<String> = fields.iter().map(|(key, _)| key.clone()).collect();
    let order = iteration_order(&keys);
    let mut fields: Vec<Option<(String, Tag)>> = fields.into_iter().map(Some).collect();
    Tag::Compound(
        order
            .into_iter()
            .map(|index| fields[index].take().expect("key is ordered once"))
            .collect(),
    )
}

fn bad(reason: &'static str) -> DecodeError {
    DecodeError::InvalidTextComponent(reason)
}
