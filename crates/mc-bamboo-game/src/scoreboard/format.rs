//! 计分板、队伍和 Boss 栏用的文本。
//!
//! 方括号、队伍颜色和名字的拼法按 `ComponentUtils` 和 `PlayerTeam.formatNameForTeam`。
//!
//! Text used by the scoreboard, teams, and boss bars.
//!
//! Brackets, team colors, and name assembly follow `ComponentUtils` and
//! `PlayerTeam.formatNameForTeam`.

use mc_bamboo_protocol::text::{Color, Component};

/// 队伍颜色 id 对应的聊天颜色。
///
/// The chat color for a team-color id.
pub(crate) fn team_color(id: u8) -> Color {
    match id {
        0 => Color::Black,
        1 => Color::DarkBlue,
        2 => Color::DarkGreen,
        3 => Color::DarkAqua,
        4 => Color::DarkRed,
        5 => Color::DarkPurple,
        6 => Color::Gold,
        7 => Color::Gray,
        8 => Color::DarkGray,
        9 => Color::Blue,
        10 => Color::Green,
        11 => Color::Aqua,
        12 => Color::Red,
        13 => Color::LightPurple,
        14 => Color::Yellow,
        _ => Color::White,
    }
}

/// Boss 栏颜色在聊天里的颜色。粉对应红，紫对应深蓝。
///
/// The chat color of a boss-bar color. Pink is red, and purple is dark blue.
pub(crate) fn boss_chat_color(id: u8) -> Color {
    match id {
        0 => Color::Red,
        1 => Color::Blue,
        2 => Color::DarkRed,
        3 => Color::Green,
        4 => Color::Yellow,
        5 => Color::DarkBlue,
        _ => Color::White,
    }
}

pub(crate) fn brackets(inner: Component) -> Component {
    Component::translate("chat.square_brackets", [inner.into()])
}

/// 官方 `Objective.getFormattedDisplayName`。
///
/// Official `Objective.getFormattedDisplayName`.
pub(crate) fn objective_name(name: &str, display: &Component) -> Component {
    brackets(display.clone().hover_show_text(Component::text(name)))
}

/// 官方 `PlayerTeam.getFormattedDisplayName`。
///
/// Official `PlayerTeam.getFormattedDisplayName`.
pub(crate) fn team_name(name: &str, display: &Component, color: Option<u8>) -> Component {
    let inner = display
        .clone()
        .insertion(name)
        .hover_show_text(Component::text(name));
    let wrapped = brackets(inner);
    match color {
        Some(color) => wrapped.color(team_color(color)),
        None => wrapped,
    }
}

/// 官方 `CustomBossEvent.getDisplayName`。
///
/// Official `CustomBossEvent.getDisplayName`.
pub(crate) fn boss_name(id: &str, name: &Component, color: u8) -> Component {
    brackets(name.clone())
        .color(boss_chat_color(color))
        .hover_show_text(Component::text(id))
        .insertion(id)
}

/// 官方 `PlayerTeam.formatNameForTeam`。没有队伍时原样返回名字。
///
/// Official `PlayerTeam.formatNameForTeam`. Without a team the name is returned unchanged.
pub(crate) fn format_name(
    color: Option<u8>,
    prefix: &Component,
    suffix: &Component,
    name: &Component,
) -> Component {
    let mut root = Component::text("");
    if let Some(color) = color {
        root = root.color(team_color(color));
    }
    root.extra(prefix.clone())
        .extra(name.clone())
        .extra(suffix.clone())
}

/// 官方 `ComponentUtils.formatList`。一项时不加分隔符，多项之间是灰色的 `", "`。
///
/// Official `ComponentUtils.formatList`. One entry has no separator. Several entries
/// are separated by a gray `", "`.
pub(crate) fn format_list(entries: &[Component]) -> Component {
    match entries {
        [] => Component::text(""),
        [only] => only.clone(),
        many => {
            let mut result = Component::text("");
            for (index, entry) in many.iter().enumerate() {
                if index > 0 {
                    result = result.extra(Component::text(", ").color(Color::Gray));
                }
                result = result.extra(entry.clone());
            }
            result
        }
    }
}

/// 官方 `ComponentUtils.formatList(Collection<String>)`：排序，并且涂成绿色。
///
/// Official `ComponentUtils.formatList(Collection<String>)`: sorted and colored green.
pub(crate) fn format_green_names(names: &[String]) -> Component {
    let mut sorted = names.to_vec();
    sorted.sort();
    let entries: Vec<Component> = sorted
        .into_iter()
        .map(|name| Component::text(name).color(Color::Green))
        .collect();
    format_list(&entries)
}
