//! 组件里的文本组件：线上是 NBT，内容要符合官方 `ComponentSerialization.CODEC` 的结构。
//!
//! 官方把这个 NBT 过一遍 DFU 的 `Codec`，解出 `Component` 再按 `Component` 写回去。
//! 我们不复刻 DFU，而是原样保留读到的标签，只校验它的结构。
//! 这样官方写出来的字节读完再写回去是逐字节相同的，键序也不会变。
//!
//! 校验是「宁可放过，不要错杀」：官方能接受的一定接受。里面有几处官方会再往下解析
//! （实体选择器、NBT 路径、对话框、`show_item` 里的组件表），我们只看字段类型。

use mc_bamboo_nbt::Tag;

use crate::{
    codec::DecodeError,
    components::cursor::{Cursor, parse_identifier},
    text::Component,
};

/// 文本组件的嵌套层数上限。NBT 自己的深度上限是 512，一层组件至少占一层 NBT。
const MAX_DEPTH: u32 = 512;

/// 官方 `ComponentSerialization.STREAM_CODEC`：读一个 NBT 标签，按 `Component` 校验。
pub fn read(cursor: &mut Cursor<'_>) -> Result<Component, DecodeError> {
    let tag = cursor.nbt_tag()?;
    validate(&tag, 0)?;
    Ok(Component::from_tag(tag))
}

/// 官方 `ComponentSerialization.OPTIONAL_STREAM_CODEC`。
pub fn read_optional(cursor: &mut Cursor<'_>) -> Result<Option<Component>, DecodeError> {
    cursor.optional(read)
}

/// 按网络格式写回去。
pub fn write(out: &mut Vec<u8>, component: &Component) {
    component.write(out);
}

fn bad(reason: &'static str) -> DecodeError {
    DecodeError::InvalidTextComponent(reason)
}

/// 官方 `ComponentSerialization.createCodec`：字符串、非空列表，或者一个完整的复合标签。
fn validate(tag: &Tag, depth: u32) -> Result<(), DecodeError> {
    if depth >= MAX_DEPTH {
        return Err(bad("nested too deep"));
    }
    match tag {
        Tag::String(_) => Ok(()),
        Tag::List(items) if !items.is_empty() => {
            items.iter().try_for_each(|item| validate(item, depth + 1))
        }
        Tag::List(_) => Err(bad("empty component list")),
        Tag::Compound(entries) => validate_compound(entries, depth),
        _ => Err(bad("not a string, list or compound")),
    }
}

/// 完整形式：内容、`extra` 和样式。
fn validate_compound(entries: &[(String, Tag)], depth: u32) -> Result<(), DecodeError> {
    validate_contents(entries, depth)?;

    // 官方的 `extra` 是严格的可选字段加 `nonEmptyList`：有这个键就必须是非空的组件列表。
    if let Some(extra) = field(entries, "extra") {
        match extra {
            Tag::List(items) if !items.is_empty() => {
                items
                    .iter()
                    .try_for_each(|item| validate(item, depth + 1))?;
            }
            _ => return Err(bad("extra must be a non-empty component list")),
        }
    }

    validate_style(entries, depth)
}

/// 官方 `StrictEither`：有 `type` 就按它分派，否则按注册顺序试每一种内容。
fn validate_contents(entries: &[(String, Tag)], depth: u32) -> Result<(), DecodeError> {
    const KINDS: [&str; 7] = [
        "text",
        "translatable",
        "keybind",
        "score",
        "selector",
        "nbt",
        "object",
    ];

    if let Some(kind) = field(entries, "type") {
        let Tag::String(kind) = kind else {
            return Err(bad("type must be a string"));
        };
        if !KINDS.contains(&kind.as_str()) {
            return Err(bad("unknown content type"));
        }
        return validate_content(kind, entries, depth);
    }

    if KINDS
        .iter()
        .any(|kind| validate_content(kind, entries, depth).is_ok())
    {
        return Ok(());
    }
    Err(bad("no matching content type"))
}

fn validate_content(kind: &str, entries: &[(String, Tag)], depth: u32) -> Result<(), DecodeError> {
    match kind {
        "text" => string_field(entries, "text").map(|_| ()),
        "translatable" => {
            string_field(entries, "translate")?;
            // `fallback` 是宽松的可选字段，坏值被官方忽略，不算错。
            if let Some(with) = field(entries, "with") {
                validate_translate_args(with, depth)?;
            }
            Ok(())
        }
        "keybind" => string_field(entries, "keybind").map(|_| ()),
        "score" => {
            let Some(Tag::Compound(score)) = field(entries, "score") else {
                return Err(bad("score must be a compound"));
            };
            // `name` 是「选择器或者普通字符串」，普通字符串总能通过，所以只看类型。
            string_field(score, "name")?;
            string_field(score, "objective").map(|_| ())
        }
        "selector" => {
            string_field(entries, "selector")?;
            optional_component(entries, "separator", depth)
        }
        "nbt" => {
            string_field(entries, "nbt")?;
            // `interpret`、`plain` 和 `separator` 都是宽松的可选字段。
            validate_data_source(entries)
        }
        "object" => {
            validate_object_info(entries)?;
            optional_component(entries, "fallback", depth)
        }
        _ => Err(bad("unknown content type")),
    }
}

/// 官方 `TranslatableContents.ARG_CODEC` 的列表：每一项是数字、布尔、字符串或者组件。
fn validate_translate_args(with: &Tag, depth: u32) -> Result<(), DecodeError> {
    match with {
        // 数组里的元素都是数字，官方当成基本类型的参数。
        Tag::ByteArray(_) | Tag::IntArray(_) | Tag::LongArray(_) => Ok(()),
        Tag::List(items) => items.iter().try_for_each(|item| match item {
            Tag::Byte(_)
            | Tag::Short(_)
            | Tag::Int(_)
            | Tag::Long(_)
            | Tag::Float(_)
            | Tag::Double(_)
            | Tag::String(_) => Ok(()),
            other => validate(other, depth + 1),
        }),
        _ => Err(bad("with must be a list")),
    }
}

/// 官方 `DataSources.CODEC`，字段名是 `source`。
fn validate_data_source(entries: &[(String, Tag)]) -> Result<(), DecodeError> {
    if let Some(source) = field(entries, "source") {
        let Tag::String(source) = source else {
            return Err(bad("source must be a string"));
        };
        return match source.as_str() {
            "entity" => string_field(entries, "entity").map(|_| ()),
            "block" => string_field(entries, "block").map(|_| ()),
            "storage" => identifier_field(entries, "storage"),
            _ => Err(bad("unknown data source")),
        };
    }

    let any = string_field(entries, "entity").is_ok()
        || string_field(entries, "block").is_ok()
        || identifier_field(entries, "storage").is_ok();
    if any {
        Ok(())
    } else {
        Err(bad("nbt content needs a data source"))
    }
}

/// 官方 `ObjectInfos.CODEC`，字段名是 `object`。
fn validate_object_info(entries: &[(String, Tag)]) -> Result<(), DecodeError> {
    if let Some(object) = field(entries, "object") {
        let Tag::String(object) = object else {
            return Err(bad("object must be a string"));
        };
        return match object.as_str() {
            "atlas" => atlas_sprite(entries),
            "player" => player_sprite(entries),
            _ => Err(bad("unknown object info")),
        };
    }
    if atlas_sprite(entries).is_ok() || player_sprite(entries).is_ok() {
        Ok(())
    } else {
        Err(bad("object content needs an object info"))
    }
}

fn atlas_sprite(entries: &[(String, Tag)]) -> Result<(), DecodeError> {
    identifier_field(entries, "sprite")?;
    match field(entries, "atlas") {
        None => Ok(()),
        Some(_) => identifier_field(entries, "atlas"),
    }
}

fn player_sprite(entries: &[(String, Tag)]) -> Result<(), DecodeError> {
    // `ResolvableProfile.CODEC` 要么是玩家名，要么是一张档案表，这里只看类型。
    match field(entries, "player") {
        Some(Tag::String(_) | Tag::Compound(_)) => Ok(()),
        _ => Err(bad("player must be a name or a profile")),
    }
}

/// 官方 `Style.Serializer.MAP_CODEC`，所有字段都是严格的可选字段。
fn validate_style(entries: &[(String, Tag)], depth: u32) -> Result<(), DecodeError> {
    if let Some(color) = field(entries, "color") {
        let Tag::String(color) = color else {
            return Err(bad("color must be a string"));
        };
        validate_color(color)?;
    }
    if let Some(shadow) = field(entries, "shadow_color") {
        validate_argb(shadow)?;
    }
    for name in [
        "bold",
        "italic",
        "underlined",
        "strikethrough",
        "obfuscated",
    ] {
        if let Some(flag) = field(entries, name)
            && !is_number(flag)
        {
            return Err(bad("style flag must be a number"));
        }
    }
    if let Some(insertion) = field(entries, "insertion")
        && !matches!(insertion, Tag::String(_))
    {
        return Err(bad("insertion must be a string"));
    }
    if field(entries, "font").is_some() {
        identifier_field(entries, "font")?;
    }
    if let Some(click) = field(entries, "click_event") {
        validate_click_event(click)?;
    }
    if let Some(hover) = field(entries, "hover_event") {
        validate_hover_event(hover, depth)?;
    }
    Ok(())
}

/// 官方 `TextColor.parseColor`：十六个颜色名，或者 `#RRGGBB`。
fn validate_color(color: &str) -> Result<(), DecodeError> {
    const NAMES: [&str; 16] = [
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

    if let Some(hex) = color.strip_prefix('#') {
        // 官方用 `Integer.parseInt(hex, 16)`，再要求值在 0..=0xffffff。
        let value = i64::from_str_radix(hex, 16).map_err(|_| bad("invalid color value"))?;
        if !(0..=0xff_ffff).contains(&value) {
            return Err(bad("color value out of range"));
        }
        return Ok(());
    }
    if NAMES.contains(&color) {
        Ok(())
    } else {
        Err(bad("invalid color name"))
    }
}

/// 官方 `ExtraCodecs.ARGB_COLOR_CODEC`：一个整数，或者四个浮点数。
fn validate_argb(tag: &Tag) -> Result<(), DecodeError> {
    if is_number(tag) {
        return Ok(());
    }
    match tag {
        Tag::List(items) if items.len() == 4 && items.iter().all(is_number) => Ok(()),
        Tag::ByteArray(values) if values.len() == 4 => Ok(()),
        Tag::IntArray(values) if values.len() == 4 => Ok(()),
        Tag::LongArray(values) if values.len() == 4 => Ok(()),
        _ => Err(bad("shadow_color must be a number or four floats")),
    }
}

/// 官方 `ClickEvent.CODEC`。`open_file` 不许从服务端发，解码时也过不了校验。
fn validate_click_event(tag: &Tag) -> Result<(), DecodeError> {
    let Tag::Compound(entries) = tag else {
        return Err(bad("click_event must be a compound"));
    };
    let action = string_field(entries, "action")?;
    match action {
        "open_url" => string_field(entries, "url").map(|_| ()),
        "run_command" | "suggest_command" => chat_string_field(entries, "command"),
        "show_dialog" => match field(entries, "dialog") {
            // `Dialog.CODEC` 是注册表名或者一份内联定义。
            Some(Tag::String(_) | Tag::Compound(_)) => Ok(()),
            _ => Err(bad("dialog must be a name or a definition")),
        },
        "change_page" => match field(entries, "page") {
            Some(page) if is_number(page) => Ok(()),
            _ => Err(bad("page must be a number")),
        },
        "copy_to_clipboard" => string_field(entries, "value").map(|_| ()),
        "custom" => identifier_field(entries, "id"),
        _ => Err(bad("click event action not allowed")),
    }
}

/// 官方 `HoverEvent.CODEC`。
fn validate_hover_event(tag: &Tag, depth: u32) -> Result<(), DecodeError> {
    let Tag::Compound(entries) = tag else {
        return Err(bad("hover_event must be a compound"));
    };
    let action = string_field(entries, "action")?;
    match action {
        "show_text" => match field(entries, "value") {
            Some(value) => validate(value, depth + 1),
            None => Err(bad("show_text needs a value")),
        },
        // `ItemStackTemplate.MAP_CODEC` 里的 `components` 是 NBT 形式的组件表，不往下看。
        "show_item" => identifier_field(entries, "id"),
        "show_entity" => {
            identifier_field(entries, "id")?;
            match field(entries, "uuid") {
                Some(Tag::String(_)) => {}
                Some(Tag::IntArray(values)) if values.len() == 4 => {}
                _ => return Err(bad("uuid must be a string or four ints")),
            }
            optional_component(entries, "name", depth)
        }
        _ => Err(bad("unknown hover event action")),
    }
}

fn field<'a>(entries: &'a [(String, Tag)], name: &str) -> Option<&'a Tag> {
    entries
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

fn string_field<'a>(entries: &'a [(String, Tag)], name: &str) -> Result<&'a str, DecodeError> {
    match field(entries, name) {
        Some(Tag::String(value)) => Ok(value),
        _ => Err(bad("missing string field")),
    }
}

fn chat_string_field(entries: &[(String, Tag)], name: &str) -> Result<(), DecodeError> {
    let value = string_field(entries, name)?;
    if value.chars().all(crate::text::is_allowed_chat_character) {
        Ok(())
    } else {
        Err(bad("disallowed chat character"))
    }
}

fn identifier_field(entries: &[(String, Tag)], name: &str) -> Result<(), DecodeError> {
    let value = string_field(entries, name)?;
    parse_identifier(value)
        .map(|_| ())
        .map_err(|_| bad("field is not a resource location"))
}

fn optional_component(
    entries: &[(String, Tag)],
    name: &str,
    depth: u32,
) -> Result<(), DecodeError> {
    match field(entries, name) {
        None => Ok(()),
        Some(value) => validate(value, depth + 1),
    }
}

const fn is_number(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Byte(_) | Tag::Short(_) | Tag::Int(_) | Tag::Long(_) | Tag::Float(_) | Tag::Double(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        components::cursor::{Cursor, write_nbt_tag},
        text::Color,
    };

    fn accepts(tag: &Tag) -> bool {
        validate(tag, 0).is_ok()
    }

    fn compound(entries: &[(&str, Tag)]) -> Tag {
        Tag::Compound(
            entries
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone()))
                .collect(),
        )
    }

    fn text(value: &str) -> Tag {
        Tag::String(value.to_owned())
    }

    #[test]
    fn plain_strings_and_lists_are_components() {
        assert!(accepts(&text("hi")));
        assert!(accepts(&Tag::List(vec![text("a"), text("b")])));
        assert!(!accepts(&Tag::List(Vec::new())));
        assert!(!accepts(&Tag::Int(1)));
        assert!(!accepts(&Tag::IntArray(vec![1])));
    }

    #[test]
    fn the_components_this_crate_writes_are_accepted() {
        for component in [
            Component::text("hi"),
            Component::text("x").color(Color::Red).italic().underlined(),
            Component::translate("multiplayer.player.joined", ["Kitten".into()]),
            Component::translate("a", [1.into(), "b".into()]),
            Component::text("").suggest_command("/foo"),
            Component::text("a")
                .extra(Component::text("b"))
                .extra(Component::translate("c", [])),
        ] {
            assert!(accepts(component.tag()), "{component:?}");
        }
    }

    #[test]
    fn every_content_type_is_recognised() {
        assert!(accepts(&compound(&[("text", text("a"))])));
        assert!(accepts(&compound(&[("translate", text("a"))])));
        assert!(accepts(&compound(&[("keybind", text("key.jump"))])));
        assert!(accepts(&compound(&[(
            "score",
            compound(&[("name", text("@s")), ("objective", text("o"))])
        )])));
        assert!(accepts(&compound(&[("selector", text("@a"))])));
        assert!(accepts(&compound(&[
            ("nbt", text("Items")),
            ("block", text("0 0 0")),
        ])));
        assert!(accepts(&compound(&[
            ("object", text("atlas")),
            ("sprite", text("minecraft:x")),
        ])));
    }

    #[test]
    fn an_explicit_type_must_match_its_fields() {
        assert!(accepts(&compound(&[
            ("type", text("text")),
            ("text", text("a")),
        ])));
        assert!(!accepts(&compound(&[
            ("type", text("text")),
            ("translate", text("a")),
        ])));
        assert!(!accepts(&compound(&[("type", text("nope"))])));
        assert!(!accepts(&compound(&[("type", Tag::Int(1))])));
    }

    #[test]
    fn a_compound_without_any_content_is_rejected() {
        assert!(!accepts(&compound(&[])));
        assert!(!accepts(&compound(&[("color", text("red"))])));
        assert!(!accepts(&compound(&[("text", Tag::Int(1))])));
    }

    #[test]
    fn extra_must_be_a_non_empty_list_of_components() {
        assert!(accepts(&compound(&[
            ("text", text("a")),
            ("extra", Tag::List(vec![text("b")])),
        ])));
        assert!(!accepts(&compound(&[
            ("text", text("a")),
            ("extra", Tag::List(Vec::new())),
        ])));
        assert!(!accepts(&compound(&[
            ("text", text("a")),
            ("extra", text("b")),
        ])));
        assert!(!accepts(&compound(&[
            ("text", text("a")),
            ("extra", Tag::List(vec![Tag::Int(1)])),
        ])));
    }

    #[test]
    fn translate_arguments_can_be_primitives_or_components() {
        let args = Tag::List(vec![text("a"), text("b")]);
        assert!(accepts(&compound(&[
            ("translate", text("k")),
            ("with", args),
        ])));
        let args = Tag::IntArray(vec![1, 2]);
        assert!(accepts(&compound(&[
            ("translate", text("k")),
            ("with", args),
        ])));
        let args = Tag::List(vec![compound(&[("text", text("a"))])]);
        assert!(accepts(&compound(&[
            ("translate", text("k")),
            ("with", args),
        ])));
        let args = Tag::List(vec![compound(&[("color", text("red"))])]);
        assert!(!accepts(&compound(&[
            ("translate", text("k")),
            ("with", args),
        ])));
        assert!(!accepts(&compound(&[
            ("translate", text("k")),
            ("with", text("a")),
        ])));
    }

    #[test]
    fn colors_follow_the_official_parser() {
        for color in ["red", "#000000", "#FFFFFF", "#ffffff"] {
            assert!(
                accepts(&compound(&[("text", text("a")), ("color", text(color)),])),
                "{color}"
            );
        }
        for color in ["Red", "#1000000", "#zzzzzz", "#-1", ""] {
            assert!(
                !accepts(&compound(&[("text", text("a")), ("color", text(color)),])),
                "{color}"
            );
        }
    }

    #[test]
    fn click_events_follow_the_official_actions() {
        let event = |entries: &[(&str, Tag)]| {
            compound(&[("text", text("")), ("click_event", compound(entries))])
        };
        assert!(accepts(&event(&[
            ("action", text("run_command")),
            ("command", text("/say hi")),
        ])));
        assert!(accepts(&event(&[
            ("action", text("change_page")),
            ("page", Tag::Int(2)),
        ])));
        assert!(accepts(&event(&[
            ("action", text("custom")),
            ("id", text("foo:bar")),
        ])));
        // open_file 不许从服务端发，官方解码时就拒绝。
        assert!(!accepts(&event(&[
            ("action", text("open_file")),
            ("path", text("/etc/passwd")),
        ])));
        assert!(!accepts(&event(&[("action", text("nope"))])));
        assert!(!accepts(&event(&[
            ("action", text("run_command")),
            ("command", text("/say \u{7f}")),
        ])));
    }

    #[test]
    fn hover_events_follow_the_official_actions() {
        let event = |entries: &[(&str, Tag)]| {
            compound(&[("text", text("")), ("hover_event", compound(entries))])
        };
        assert!(accepts(&event(&[
            ("action", text("show_text")),
            ("value", text("hi")),
        ])));
        assert!(accepts(&event(&[
            ("action", text("show_item")),
            ("id", text("minecraft:stone")),
        ])));
        assert!(accepts(&event(&[
            ("action", text("show_entity")),
            ("id", text("minecraft:pig")),
            ("uuid", Tag::IntArray(vec![1, 2, 3, 4])),
        ])));
        assert!(!accepts(&event(&[
            ("action", text("show_entity")),
            ("id", text("minecraft:pig")),
            ("uuid", Tag::IntArray(vec![1, 2, 3])),
        ])));
        assert!(!accepts(&event(&[("action", text("show_text"))])));
    }

    #[test]
    fn style_flags_must_be_numbers() {
        assert!(accepts(&compound(&[
            ("text", text("a")),
            ("bold", Tag::Byte(1)),
        ])));
        assert!(!accepts(&compound(&[
            ("text", text("a")),
            ("bold", text("true")),
        ])));
    }

    #[test]
    fn round_trips_keep_the_bytes() {
        let component = Component::translate("multiplayer.player.joined", ["Kitten".into()])
            .color(Color::Yellow);
        let mut bytes = Vec::new();
        write_nbt_tag(&mut bytes, component.tag());

        let mut cursor = Cursor::new(&bytes);
        let decoded = read(&mut cursor).expect("官方能解的组件");
        assert_eq!(decoded, component);

        let mut again = Vec::new();
        write(&mut again, &decoded);
        assert_eq!(again, bytes);
    }

    #[test]
    fn rejects_a_malformed_component() {
        let mut bytes = Vec::new();
        write_nbt_tag(&mut bytes, &Tag::Int(1));
        let mut cursor = Cursor::new(&bytes);
        assert!(matches!(
            read(&mut cursor),
            Err(DecodeError::InvalidTextComponent(_))
        ));
    }
}
