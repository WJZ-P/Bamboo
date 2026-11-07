//! 文本组件：Configuration 和 Play 状态里给玩家看的文字，线上是 NBT。
//!
//! 客户端解码失败时会断线，比如颜色名不存在、`extra` 是空列表。
//! 所以这里只提供写不出这些内容的构造方法。

use std::mem::{discriminant, take};

use mc_bamboo_nbt::Tag;

/// 一段文本组件。内部的标签只可能是 `Tag::String` 或 `Tag::Compound`。
#[derive(Debug, Clone, PartialEq)]
pub struct Component(Tag);

impl Component {
    /// 一段字面文本。没有样式和子组件时写成 `TAG_String`。
    pub fn text(text: impl Into<String>) -> Self {
        Self(Tag::String(text.into()))
    }

    /// 一段翻译文本，客户端按自己的语言显示 `key`，再把 `args` 依次填进占位符。
    pub fn translate(key: impl Into<String>, args: impl IntoIterator<Item = TranslateArg>) -> Self {
        let mut entries = vec![("translate".to_owned(), Tag::String(key.into()))];
        let mut with: Vec<Tag> = args.into_iter().map(|arg| arg.0).collect();
        if !with.is_empty() {
            unify_elements(&mut with);
            entries.push(("with".to_owned(), Tag::List(with)));
        }
        Self(Tag::Compound(entries))
    }

    /// 设置颜色。子组件和翻译参数会继承它。
    pub fn color(self, color: Color) -> Self {
        self.with_entry("color", Tag::String(color.name().to_owned()))
    }

    /// 设置斜体。子组件和翻译参数会继承它。
    pub fn italic(self) -> Self {
        self.with_entry("italic", Tag::Byte(1))
    }

    /// 设置下划线。子组件和翻译参数会继承它。
    pub fn underlined(self) -> Self {
        self.with_entry("underlined", Tag::Byte(1))
    }

    /// 点击时把 `command` 填进聊天框。
    ///
    /// 客户端解码这个字段时检查 [`is_allowed_chat_character`]，所以不合法的字符不写。
    pub fn suggest_command(self, command: &str) -> Self {
        let command: String = command
            .chars()
            .filter(|&character| is_allowed_chat_character(character))
            .collect();
        let event = Tag::Compound(vec![
            (
                "action".to_owned(),
                Tag::String("suggest_command".to_owned()),
            ),
            ("command".to_owned(), Tag::String(command)),
        ]);
        self.with_entry("click_event", event)
    }

    /// 在末尾追加一个子组件。
    pub fn extra(self, child: Self) -> Self {
        let mut entries = self.into_entries();
        let children = entries.iter_mut().find_map(|(key, value)| match value {
            Tag::List(children) if key == "extra" => Some(children),
            _ => None,
        });
        match children {
            Some(children) => {
                children.push(child.0);
                unify_elements(children);
            }
            None => entries.push(("extra".to_owned(), Tag::List(vec![child.0]))),
        }
        Self(Tag::Compound(entries))
    }

    /// 按网络格式写入。
    pub fn write(&self, out: &mut Vec<u8>) {
        self.0.write_network(out);
    }

    fn into_entries(self) -> Vec<(String, Tag)> {
        match self.0 {
            Tag::Compound(entries) => entries,
            text => vec![("text".to_owned(), text)],
        }
    }

    /// 设置一个键，已经有这个键时替换。
    fn with_entry(self, key: &str, value: Tag) -> Self {
        let mut entries = self.into_entries();
        match entries.iter_mut().find(|(existing, _)| existing == key) {
            Some(entry) => entry.1 = value,
            None => entries.push((key.to_owned(), value)),
        }
        Self(Tag::Compound(entries))
    }
}

/// 聊天里允许出现的字符：小于 32、127 和 `§` 以外的都可以。
pub const fn is_allowed_chat_character(character: char) -> bool {
    character >= ' ' && character != '\u{7f}' && character != '§'
}

/// 翻译文本的一个参数：组件、字符串或整数。
#[derive(Debug, Clone, PartialEq)]
pub struct TranslateArg(Tag);

impl From<Component> for TranslateArg {
    fn from(component: Component) -> Self {
        Self(component.0)
    }
}

impl From<&str> for TranslateArg {
    fn from(text: &str) -> Self {
        Self(Tag::String(text.to_owned()))
    }
}

impl From<String> for TranslateArg {
    fn from(text: String) -> Self {
        Self(Tag::String(text))
    }
}

impl From<i32> for TranslateArg {
    fn from(value: i32) -> Self {
        Self(Tag::Int(value))
    }
}

/// 文本的 16 种命名颜色。写错颜色名客户端会断线，所以不接受任意字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Black,
    DarkBlue,
    DarkGreen,
    DarkAqua,
    DarkRed,
    DarkPurple,
    Gold,
    Gray,
    DarkGray,
    Blue,
    Green,
    Aqua,
    Red,
    LightPurple,
    Yellow,
    White,
}

impl Color {
    /// 线上的颜色名。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Black => "black",
            Self::DarkBlue => "dark_blue",
            Self::DarkGreen => "dark_green",
            Self::DarkAqua => "dark_aqua",
            Self::DarkRed => "dark_red",
            Self::DarkPurple => "dark_purple",
            Self::Gold => "gold",
            Self::Gray => "gray",
            Self::DarkGray => "dark_gray",
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Aqua => "aqua",
            Self::Red => "red",
            Self::LightPurple => "light_purple",
            Self::Yellow => "yellow",
            Self::White => "white",
        }
    }
}

/// NBT 列表只能有一种元素类型。类型混杂时把元素都写成 `{text:"..."}`，显示的内容不变。
fn unify_elements(elements: &mut [Tag]) {
    let uniform = elements
        .windows(2)
        .all(|pair| discriminant(&pair[0]) == discriminant(&pair[1]));
    if uniform {
        return;
    }
    for element in elements {
        let text = match element {
            Tag::String(text) => take(text),
            Tag::Int(value) => value.to_string(),
            _ => continue,
        };
        *element = Tag::Compound(vec![("text".to_owned(), Tag::String(text))]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{hex, string_entry};

    fn bytes(component: &Component) -> Vec<u8> {
        let mut out = Vec::new();
        component.write(&mut out);
        out
    }

    #[test]
    fn plain_text_stays_a_string_tag() {
        assert_eq!(bytes(&Component::text("hi")), hex("08 00 02 68 69"));
        assert_eq!(
            bytes(&Component::text("竹子")),
            hex("08 00 06 e7 ab b9 e5 ad 90")
        );
    }

    #[test]
    fn join_message_matches_the_official_bytes() {
        let component = Component::translate("multiplayer.player.joined", ["Kitten".into()])
            .color(Color::Yellow);

        let expected = hex("0a
             08 00 09 74 72 61 6e 73 6c 61 74 65
                00 19 6d 75 6c 74 69 70 6c 61 79 65 72 2e 70 6c 61 79 65 72 2e 6a 6f 69 6e 65 64
             09 00 04 77 69 74 68
                08 00 00 00 01
                00 06 4b 69 74 74 65 6e
             08 00 05 63 6f 6c 6f 72
                00 06 79 65 6c 6c 6f 77
             00");
        assert_eq!(expected.len(), 77);
        assert_eq!(bytes(&component), expected);
    }

    #[test]
    fn mixed_arguments_all_become_compounds() {
        let component = Component::translate(
            "chat.type.text",
            [
                Component::text("Kitten").color(Color::Gold).into(),
                "hello".into(),
            ],
        );

        let mut expected = vec![0x0a];
        expected.extend(string_entry("translate", "chat.type.text"));
        expected.extend(hex("09 00 04"));
        expected.extend_from_slice(b"with");
        expected.extend(hex("0a 00 00 00 02"));
        expected.extend(string_entry("text", "Kitten"));
        expected.extend(string_entry("color", "gold"));
        expected.push(0);
        expected.extend(string_entry("text", "hello"));
        expected.push(0);
        expected.push(0);
        assert_eq!(bytes(&component), expected);
    }

    #[test]
    fn integer_arguments_stay_integers_when_alone() {
        // 官方的字节，只是这里的键序不同，键序不影响解码。
        let component = Component::translate("build.tooHigh", [319.into()]).color(Color::Red);

        let mut expected = vec![0x0a];
        expected.extend(string_entry("translate", "build.tooHigh"));
        expected.extend(hex("09 00 04"));
        expected.extend_from_slice(b"with");
        expected.extend(hex("03 00 00 00 01 00 00 01 3f"));
        expected.extend(string_entry("color", "red"));
        expected.push(0);
        assert_eq!(bytes(&component), expected);
    }

    #[test]
    fn mixed_integer_arguments_turn_into_text() {
        let component = Component::translate("a", [1.into(), "b".into()]);
        let expected = Component(Tag::Compound(vec![
            ("translate".to_owned(), Tag::String("a".to_owned())),
            (
                "with".to_owned(),
                Tag::List(vec![
                    Tag::Compound(vec![("text".to_owned(), Tag::String("1".to_owned()))]),
                    Tag::Compound(vec![("text".to_owned(), Tag::String("b".to_owned()))]),
                ]),
            ),
        ]));
        assert_eq!(component, expected);
    }

    #[test]
    fn no_arguments_means_no_with_key() {
        let mut expected = vec![0x0a];
        expected.extend(string_entry("translate", "disconnect.timeout"));
        expected.push(0);
        assert_eq!(
            bytes(&Component::translate("disconnect.timeout", [])),
            expected
        );
    }

    #[test]
    fn color_turns_text_into_a_compound_and_can_be_replaced() {
        let component = Component::text("x").color(Color::Red).color(Color::Aqua);

        let mut expected = vec![0x0a];
        expected.extend(string_entry("text", "x"));
        expected.extend(string_entry("color", "aqua"));
        expected.push(0);
        assert_eq!(bytes(&component), expected);
    }

    #[test]
    fn extra_children_are_nested_in_order() {
        let component = Component::text("")
            .color(Color::Red)
            .extra(Component::translate("command.unknown.command", []));

        let mut expected = vec![0x0a];
        expected.extend(string_entry("text", ""));
        expected.extend(string_entry("color", "red"));
        expected.extend(hex("09 00 05"));
        expected.extend_from_slice(b"extra");
        expected.extend(hex("0a 00 00 00 01"));
        expected.extend(string_entry("translate", "command.unknown.command"));
        expected.push(0);
        expected.push(0);
        assert_eq!(bytes(&component), expected);
    }

    #[test]
    fn extra_keeps_plain_strings_while_they_are_uniform() {
        let component = Component::text("a")
            .extra(Component::text("b"))
            .extra(Component::text("c"));
        let expected = Component(Tag::Compound(vec![
            ("text".to_owned(), Tag::String("a".to_owned())),
            (
                "extra".to_owned(),
                Tag::List(vec![
                    Tag::String("b".to_owned()),
                    Tag::String("c".to_owned()),
                ]),
            ),
        ]));
        assert_eq!(component, expected);
    }

    #[test]
    fn a_styled_child_turns_earlier_plain_children_into_compounds() {
        let component = Component::text("a")
            .extra(Component::text("b"))
            .extra(Component::text("c").color(Color::Red));
        let expected = Component(Tag::Compound(vec![
            ("text".to_owned(), Tag::String("a".to_owned())),
            (
                "extra".to_owned(),
                Tag::List(vec![
                    Tag::Compound(vec![("text".to_owned(), Tag::String("b".to_owned()))]),
                    Tag::Compound(vec![
                        ("text".to_owned(), Tag::String("c".to_owned())),
                        ("color".to_owned(), Tag::String("red".to_owned())),
                    ]),
                ]),
            ),
        ]));
        assert_eq!(component, expected);
    }

    #[test]
    fn italic_and_underlined_are_written_once_as_bytes() {
        let component = Component::text("x")
            .italic()
            .underlined()
            .italic()
            .underlined();
        let expected = Component(Tag::Compound(vec![
            ("text".to_owned(), Tag::String("x".to_owned())),
            ("italic".to_owned(), Tag::Byte(1)),
            ("underlined".to_owned(), Tag::Byte(1)),
        ]));
        assert_eq!(component, expected);
    }

    #[test]
    fn suggest_command_writes_a_click_event() {
        let component = Component::text("").suggest_command("/foo bar");

        let mut expected = vec![0x0a];
        expected.extend(string_entry("text", ""));
        expected.extend(hex("0a 00 0b"));
        expected.extend_from_slice(b"click_event");
        expected.extend(string_entry("action", "suggest_command"));
        expected.extend(string_entry("command", "/foo bar"));
        expected.push(0);
        expected.push(0);
        assert_eq!(bytes(&component), expected);
    }

    #[test]
    fn suggest_command_leaves_out_what_the_client_would_refuse() {
        assert_eq!(
            Component::text("").suggest_command("/a§b\u{7f}c\nd"),
            Component::text("").suggest_command("/abcd")
        );
    }

    #[test]
    fn allowed_chat_characters_match_the_game() {
        let illegal = (0..32).chain([127, 167]);
        for code in illegal {
            let character = char::from_u32(code).unwrap();
            assert!(!is_allowed_chat_character(character), "{code}");
        }
        for character in [' ', '~', '\u{80}', '¦', '¨', '竹', '🎋'] {
            assert!(is_allowed_chat_character(character), "{character}");
        }
    }

    #[test]
    fn color_names_match_the_game() {
        let names = [
            (Color::Black, "black"),
            (Color::DarkBlue, "dark_blue"),
            (Color::DarkGreen, "dark_green"),
            (Color::DarkAqua, "dark_aqua"),
            (Color::DarkRed, "dark_red"),
            (Color::DarkPurple, "dark_purple"),
            (Color::Gold, "gold"),
            (Color::Gray, "gray"),
            (Color::DarkGray, "dark_gray"),
            (Color::Blue, "blue"),
            (Color::Green, "green"),
            (Color::Aqua, "aqua"),
            (Color::Red, "red"),
            (Color::LightPurple, "light_purple"),
            (Color::Yellow, "yellow"),
            (Color::White, "white"),
        ];
        for (color, name) in names {
            assert_eq!(color.name(), name);
        }
    }
}
