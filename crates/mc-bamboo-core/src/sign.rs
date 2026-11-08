//! 告示牌的内容，对应官方的 `SignText`、`SignTextSlot` 和 `DyeColor`。
//!
//! 离线模式不做聊天过滤，所有文字都是明文，所以这里只存字符串，不存 `Component`。
//! 编辑器的默认样式是 `Style.EMPTY`，`sign_update` 传来的文字也只保留纯文本。

/// 一面告示牌的行数，官方 `SignText.LINES`。
pub const LINES: usize = 4;

/// 一行的最大字符数，官方 `SignText` 的 `MAX_STRING_LENGTH`。
pub const MAX_LINE_CHARS: usize = 384;

/// 一面告示牌，官方的 `SignText`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignText {
    pub messages: [String; LINES],
    pub color: DyeColor,
    pub glowing: bool,
}

impl SignText {
    /// 刚放下的告示牌，官方 `SignText.EMPTY`。
    pub const EMPTY: Self = Self {
        messages: [String::new(), String::new(), String::new(), String::new()],
        color: DyeColor::Black,
        glowing: false,
    };

    /// `SignText.hasMessage`。
    pub fn has_message(&self) -> bool {
        self.messages.iter().any(|line| !line.is_empty())
    }

    /// `SignText.hasEditableText`：每一行都是纯文本。
    ///
    /// 这里存的一直是纯文本，所以总是可以编辑。官方还要检查聊天过滤后的那一份。
    pub const fn has_editable_text(&self) -> bool {
        true
    }

    /// `SignText.withColor`：颜色没变时原样返回。
    pub fn with_color(self, color: DyeColor) -> Self {
        if self.color == color {
            self
        } else {
            Self { color, ..self }
        }
    }

    /// `SignText.withGlowingText`。
    pub fn with_glowing_text(self, glowing: bool) -> Self {
        if self.glowing == glowing {
            self
        } else {
            Self { glowing, ..self }
        }
    }
}

impl Default for SignText {
    fn default() -> Self {
        Self::EMPTY
    }
}

/// 告示牌的哪一面，枚举值就是网络上的 ID。官方 `SignTextSlot`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignTextSlot {
    Back = 0,
    Front = 1,
}

impl SignTextSlot {
    /// 从网络 ID 解码。官方用的 ID 映射越界时取默认值，也就是背面。
    pub const fn from_id(id: i32) -> Self {
        match id {
            1 => Self::Front,
            _ => Self::Back,
        }
    }

    /// 网络 ID。
    pub const fn id(self) -> i32 {
        self as i32
    }
}

/// 告示牌的一面加上两面共用的状态，对应官方 `SignBlockEntity` 存下来的字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignContent {
    pub front: SignText,
    pub back: SignText,
    pub waxed: bool,
    /// 正在编辑的玩家的 UUID，官方 `playerWhoMayEdit`。离线模式用不到账号，只比较字节。
    pub editor: Option<[u8; 16]>,
}

impl SignContent {
    /// 还没写过的告示牌。
    pub const BLANK: Self = Self {
        front: SignText::EMPTY,
        back: SignText::EMPTY,
        waxed: false,
        editor: None,
    };

    /// 这一面的文字。
    pub const fn text(&self, slot: SignTextSlot) -> &SignText {
        match slot {
            SignTextSlot::Front => &self.front,
            SignTextSlot::Back => &self.back,
        }
    }

    /// 改写这一面的文字。
    pub const fn text_mut(&mut self, slot: SignTextSlot) -> &mut SignText {
        match slot {
            SignTextSlot::Front => &mut self.front,
            SignTextSlot::Back => &mut self.back,
        }
    }
}

impl Default for SignContent {
    fn default() -> Self {
        Self::BLANK
    }
}

/// 告示牌的颜色，枚举值就是网络上的 ID。官方 `DyeColor`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DyeColor {
    White = 0,
    Orange = 1,
    Magenta = 2,
    LightBlue = 3,
    Yellow = 4,
    Lime = 5,
    Pink = 6,
    Gray = 7,
    LightGray = 8,
    Cyan = 9,
    Purple = 10,
    Blue = 11,
    Brown = 12,
    Green = 13,
    Red = 14,
    Black = 15,
}

impl DyeColor {
    /// 全部颜色，顺序就是官方的 `VALUES`。
    pub const ALL: [Self; 16] = [
        Self::White,
        Self::Orange,
        Self::Magenta,
        Self::LightBlue,
        Self::Yellow,
        Self::Lime,
        Self::Pink,
        Self::Gray,
        Self::LightGray,
        Self::Cyan,
        Self::Purple,
        Self::Blue,
        Self::Brown,
        Self::Green,
        Self::Red,
        Self::Black,
    ];

    /// 从网络 ID 解码。越界时官方取映射的默认值，也就是白色。
    pub const fn from_id(id: i32) -> Self {
        if id >= 0 && id < Self::ALL.len() as i32 {
            Self::ALL[id as usize]
        } else {
            Self::White
        }
    }

    /// 网络 ID。
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// 官方的序列化名，比如 `light_blue`。NBT 和聊天格式里都用它。
    pub const fn name(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Orange => "orange",
            Self::Magenta => "magenta",
            Self::LightBlue => "light_blue",
            Self::Yellow => "yellow",
            Self::Lime => "lime",
            Self::Pink => "pink",
            Self::Gray => "gray",
            Self::LightGray => "light_gray",
            Self::Cyan => "cyan",
            Self::Purple => "purple",
            Self::Blue => "blue",
            Self::Brown => "brown",
            Self::Green => "green",
            Self::Red => "red",
            Self::Black => "black",
        }
    }
}

/// `ChatFormatting.stripFormatting`：去掉 `§` 加一个格式码。
///
/// 官方的正则 `(?i)§[0-9A-FK-OR]` 从左往右不重叠地替换，所以 `§§a` 只去掉后面一对，
/// 留下前面那个 `§`。这里按同样的顺序一个字符一个字符地看。
pub fn strip_formatting(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '§' && characters.peek().copied().is_some_and(is_format_code) {
            characters.next();
        } else {
            stripped.push(character);
        }
    }
    stripped
}

/// 格式码：颜色 `0-9A-F`、样式 `K-O`、重置 `R`，不分大小写。
fn is_format_code(character: char) -> bool {
    matches!(
        character.to_ascii_lowercase(),
        '0'..='9' | 'a'..='f' | 'k'..='o' | 'r'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_text_has_no_message_and_is_editable() {
        let empty = SignText::EMPTY;
        assert!(!empty.has_message());
        assert!(empty.has_editable_text());
        assert_eq!(empty.color, DyeColor::Black);
        assert!(!empty.glowing);
        assert_eq!(
            SignContent::BLANK.text(SignTextSlot::Front),
            &SignText::EMPTY
        );
    }

    #[test]
    fn any_line_makes_the_sign_written() {
        let mut text = SignText::EMPTY;
        text.messages[2] = "hi".to_owned();
        assert!(text.has_message());
    }

    #[test]
    fn changing_a_style_returns_a_new_text_only_when_it_changes() {
        let text = SignText::EMPTY;
        assert_eq!(text.clone().with_color(DyeColor::Black), text);
        assert_eq!(text.clone().with_glowing_text(false), text);
        assert_ne!(text.clone().with_color(DyeColor::Red), text);
        assert_ne!(text.clone().with_glowing_text(true), text);
    }

    #[test]
    fn slots_read_back_the_side_they_name() {
        let mut content = SignContent::BLANK;
        content.text_mut(SignTextSlot::Front).messages[0] = "front".to_owned();
        content.text_mut(SignTextSlot::Back).messages[0] = "back".to_owned();
        assert_eq!(content.front.messages[0], "front");
        assert_eq!(content.back.messages[0], "back");
    }

    #[test]
    fn slot_ids_are_back_zero_and_front_one() {
        assert_eq!(SignTextSlot::from_id(0), SignTextSlot::Back);
        assert_eq!(SignTextSlot::from_id(1), SignTextSlot::Front);
        // 官方 ID 映射越界时取默认值。
        assert_eq!(SignTextSlot::from_id(2), SignTextSlot::Back);
        assert_eq!(SignTextSlot::from_id(-1), SignTextSlot::Back);
        assert_eq!(SignTextSlot::Front.id(), 1);
    }

    #[test]
    fn dye_ids_and_names_follow_the_registry() {
        assert_eq!(DyeColor::ALL.len(), 16);
        for (id, color) in DyeColor::ALL.into_iter().enumerate() {
            assert_eq!(color.id(), id as i32);
            assert_eq!(DyeColor::from_id(id as i32), color);
        }
        assert_eq!(DyeColor::LightBlue.name(), "light_blue");
        assert_eq!(DyeColor::LightGray.name(), "light_gray");
        assert_eq!(DyeColor::Black.name(), "black");
        // 越界时官方取映射的默认值。
        assert_eq!(DyeColor::from_id(16), DyeColor::White);
        assert_eq!(DyeColor::from_id(-1), DyeColor::White);
    }

    #[test]
    fn formatting_codes_are_removed_and_nothing_else_is() {
        let cases = [
            ("plain", "plain"),
            ("§a绿色", "绿色"),
            ("§L§NBold§r", "Bold"),
            ("§1§2§3§4§5§6§7§8§9§0", ""),
            // 不认识的格式码保留。
            ("§z§", "§z§"),
            // 正则不重叠：第一对里第二个 `§` 不是格式码，留下它。
            ("§§a", "§"),
            // 中文和代理对不受影响。
            ("竹🎋§x", "竹🎋§x"),
        ];
        for (input, expected) in cases {
            assert_eq!(strip_formatting(input), expected, "input {input:?}");
        }
    }
}
