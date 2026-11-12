//! 命令的成功和失败消息。措辞用官方的翻译键，颜色和广播对象也按官方来。
//!
//! Success and failure text. The wording uses the official translation keys, and the
//! colour and recipients follow the official rules.

use mc_bamboo_command::{ErrorArg, ParseError};
use mc_bamboo_protocol::text::{Color, Component, TranslateArg};
use tracing::info;

use crate::{inbound::PlayerId, region::Region};

/// 执行期的失败。解析错误另带游标，用来画第二行 `command.context.here`。
///
/// An execution failure. Parse errors also carry a cursor for the second
/// `command.context.here` line.
pub(crate) struct CommandFail {
    pub message: Component,
    pub cursor: Option<usize>,
}

pub(crate) fn fail(key: &'static str) -> CommandFail {
    CommandFail {
        message: Component::translate(key, []),
        cursor: None,
    }
}

pub(crate) fn fail_with(
    key: &'static str,
    args: impl IntoIterator<Item = TranslateArg>,
) -> CommandFail {
    CommandFail {
        message: Component::translate(key, args),
        cursor: None,
    }
}

pub(crate) fn fail_parse(input: &str, error: &ParseError, cursor: usize) -> CommandFail {
    let _ = input;
    CommandFail {
        message: Component::translate(error.key, error.args.iter().map(arg_to_translate)),
        cursor: Some(cursor),
    }
}

fn arg_to_translate(arg: &ErrorArg) -> TranslateArg {
    match arg {
        ErrorArg::Text(text) => text.clone().into(),
        ErrorArg::Int(value) => (*value).into(),
        ErrorArg::Float(value) => (*value).into(),
        ErrorArg::Bool(value) => (*value).into(),
    }
}

/// 官方 `sendFailure`：空组件套上红色，再附上原来的消息。
///
/// Official `sendFailure`: an empty component styled red, with the message appended.
fn redden(message: &Component) -> Component {
    Component::text("").color(Color::Red).extra(message.clone())
}

pub(crate) fn report(region: &mut Region, id: PlayerId, input: &str, failure: &CommandFail) {
    let message = redden(&failure.message);
    send_system(region, id, &message);
    // 官方只要 `getInput() != null` 就发第二行，空命令也画 `here`。
    //
    // The official code sends the second line whenever `getInput() != null`, including an empty command.
    if let Some(cursor) = failure.cursor {
        let context = redden(&context_line(input, cursor));
        send_system(region, id, &context);
    }
}

/// 官方 `Commands.finishParsing` 的第二行：灰色已输入部分可点击补全，剩下的红下划线。
///
/// The second line from official `Commands.finishParsing`: the grey consumed text suggests
/// the command, and the remainder is red and underlined.
fn context_line(input: &str, cursor: usize) -> Component {
    let chars: Vec<char> = input.chars().collect();
    let cursor = cursor.min(chars.len());
    let start = cursor.saturating_sub(10);
    let mut shown: String = chars[start..cursor].iter().collect();
    if cursor > 10 {
        shown.insert_str(0, "...");
    }
    let mut line = Component::text(shown)
        .color(Color::Gray)
        .suggest_command(&format!("/{input}"));
    if cursor < chars.len() {
        let rest: String = chars[cursor..].iter().collect();
        line = line.extra(Component::text(rest).color(Color::Red).underlined());
    }
    line.extra(
        Component::translate("command.context.here", [])
            .color(Color::Red)
            .italic(),
    )
}

pub(crate) fn send_system(region: &mut Region, id: PlayerId, content: &Component) {
    // 官方 `CommandSourceStack.sendSuccess` / `sendFailure` 最后都走
    // `ServerPlayer.sendSystemMessage`，隐藏聊天时不发非 overlay 系统消息。
    //
    // Official `CommandSourceStack.sendSuccess` / `sendFailure` both end at
    // `ServerPlayer.sendSystemMessage`, which suppresses non-overlay messages for hidden chat.
    crate::chat::send_system_message(region, id, content, false);
}

/// 官方 `sendSuccess`。`broadcast` 为真时，其他 OP 收到灰色斜体的 `chat.type.admin`。
///
/// Official `sendSuccess`. When `broadcast` is set, other ops receive a grey italic
/// `chat.type.admin` line.
pub(crate) fn success(region: &mut Region, source: PlayerId, message: Component, broadcast: bool) {
    // `CommandSourceStack.sendSuccess`：自己的系统消息受 `send_command_feedback` 限制。
    //
    // `CommandSourceStack.sendSuccess`: the caller's system message is gated by
    // `send_command_feedback`.
    if region.send_command_feedback {
        send_system(region, source, &message);
    }
    if !broadcast {
        return;
    }
    let Some(name) = region
        .players
        .get(&source)
        .map(|player| player.name.clone())
    else {
        return;
    };
    // `broadcastToAdmins`：其他 OP 的 `chat.type.admin` 也看 `send_command_feedback`。
    // 日志看 `log_admin_commands`，而且来源不是服务器本身。玩家执行的命令会记。
    //
    // `broadcastToAdmins`: other ops' `chat.type.admin` also checks `send_command_feedback`.
    // The log checks `log_admin_commands`, and the source is not the server. A player's
    // command is logged.
    if region.log_admin_commands {
        info!(player = %name, "broadcasting command feedback");
    }
    if !region.send_command_feedback {
        return;
    }
    let admin = Component::translate(
        "chat.type.admin",
        [Component::text(name).into(), message.into()],
    )
    .color(Color::Gray)
    .italic();
    // `broadcastToAdmins` 逐个 OP 调 `ServerPlayer.sendSystemMessage`，隐藏聊天的 OP 不发。
    //
    // `broadcastToAdmins` calls `ServerPlayer.sendSystemMessage` for each op; a hidden-chat op is skipped.
    let ops: Vec<PlayerId> = region
        .players
        .iter()
        .filter(|(id, player)| **id != source && player.is_op)
        .map(|(id, _)| *id)
        .collect();
    for id in ops {
        crate::chat::send_system_message(region, id, &admin, false);
    }
}
