//! 组件里的文本组件：线上是 NBT，按官方 `ComponentSerialization.CODEC` 解码再写回去。
//!
//! 选择器、NBT 路径、对话框、`show_item` 和玩家档案都按官方 `Codec` 校验。
//! 写回去的是 `Codec` 编出来的标签，不是客户端原来的字节。
//!
//! Chat components on the wire are NBT. They are decoded and written back through
//! `ComponentSerialization.CODEC`. Selectors, NBT paths, dialogs, `show_item` and
//! player profiles use the official codecs. The stored tag is what that codec
//! emits, not the bytes the client sent.

use mc_bamboo_nbt::Tag;

use crate::{codec::DecodeError, components::cursor::Cursor, text::Component};

/// 锁谓词里嵌着的文本页：按组件 `Codec` 解码，解不开就拒绝。
///
/// A text page nested in a lock predicate. Reject it when the component codec does.
pub(crate) fn accept_tag(tag: &Tag) -> Result<(), DecodeError> {
    super::text_wire::canonical(tag, 0).map(|_| ())
}

/// 官方 `ComponentSerialization.STREAM_CODEC`：读一个 NBT 标签，按 `Component` 解码再写回。
pub fn read(cursor: &mut Cursor<'_>) -> Result<Component, DecodeError> {
    let tag = cursor.nbt_tag()?;
    Ok(Component::from_tag(super::text_wire::canonical(&tag, 0)?))
}

/// 官方 `ComponentSerialization.OPTIONAL_STREAM_CODEC`。
pub fn read_optional(cursor: &mut Cursor<'_>) -> Result<Option<Component>, DecodeError> {
    cursor.optional(read)
}

/// 按网络格式写回去。
pub fn write(out: &mut Vec<u8>, component: &Component) {
    component.write(out);
}

/// 这段文本是不是已经按官方 `Codec` 校验过。
///
/// 转发给别人之前，客户端会用同一份 `Codec` 再解一次。解得开才转发。
///
/// Whether this component survives the official codec. It is forwarded only when
/// the client can decode it again with that same codec.
pub fn is_fully_checked(component: &Component) -> bool {
    super::text_wire::canonical(component.tag(), 0).is_ok()
}

/// 标签过得了官方文本 `Codec` 时，返回按 `Codec` 重写过的组件。
///
/// The component rewritten by the official text codec, when the tag is accepted.
pub fn checked_component(tag: &Tag) -> Option<crate::text::Component> {
    let rewritten = super::text_wire::canonical(tag, 0).ok()?;
    Some(crate::text::Component::from_tag(rewritten))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        components::cursor::{Cursor, write_nbt_tag},
        text::Color,
    };

    fn accepts(tag: &Tag) -> bool {
        checked_component(tag).is_some()
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
    fn the_codec_rewrites_keys_into_hash_map_order_and_stays_there() {
        let component = Component::translate("multiplayer.player.joined", ["Kitten".into()])
            .color(Color::Yellow);
        let mut bytes = Vec::new();
        write_nbt_tag(&mut bytes, component.tag());

        let mut cursor = Cursor::new(&bytes);
        let decoded = read(&mut cursor).expect("官方能解的组件");
        // 官方 `HashMap` 的桶序是 with、color、translate，不是构造时的插入顺序。
        // Java `HashMap` bucket order is with, color, translate, not insertion order.
        assert_eq!(
            decoded.tag(),
            &Tag::Compound(vec![
                (
                    "with".to_owned(),
                    Tag::List(vec![Tag::String("Kitten".to_owned())])
                ),
                ("color".to_owned(), Tag::String("yellow".to_owned())),
                (
                    "translate".to_owned(),
                    Tag::String("multiplayer.player.joined".to_owned())
                ),
            ])
        );

        let mut again = Vec::new();
        write(&mut again, &decoded);
        let mut cursor = Cursor::new(&again);
        let stable = read(&mut cursor).expect("重写后的字节还要解得开");
        assert_eq!(stable, decoded);
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
