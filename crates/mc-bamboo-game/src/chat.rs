//! 聊天和命令。
//!
//! 聊天是全服的事情：这里只拼出要广播的帧，发给谁由 `World` 决定。
//! 离线模式的消息没有签名，每名玩家要记住的只有 [`SpamThrottlers`] 里的刷屏计数。
//!
//! Bamboo 还没有命令，所以不发 `commands` 包：客户端没收到时命令树是空的根节点，
//! 和收到最小命令树完全一样，输入的命令照样走 `chat_command`（`chat-join.md` 7.3）。

use mc_bamboo_data::registries::chat_type;
use mc_bamboo_protocol::{
    packet::play::{AwardStats, ChatVisibility, ClientCommandKind, DisguisedChat, SystemChat},
    text::{Color, Component, is_allowed_chat_character},
};
use tracing::info;

use crate::{
    inbound::PlayerId,
    links::{Links, encode_frames},
    region::Region,
};

const ILLEGAL_CHARACTERS: &str = "multiplayer.disconnect.illegal_characters";
const SPAM: &str = "disconnect.spam";
const CHAT_DISABLED: &str = "chat.disabled.options";
const UNKNOWN_COMMAND: &str = "command.unknown.command";
const CONTEXT_HERE: &str = "command.context.here";

/// 回显命令时最多写多少个字符。协议允许 32767 个，但客户端的输入框只放得下 256 个，
/// 所以只有改过的客户端才会被截断，回应的大小也不会跟着发来的内容增长。
const MAX_ECHOED_COMMAND_CHARS: usize = 256;

/// 每条消息或者命令给 [`TickThrottler`] 加多少。
const SPAM_INCREMENT_STEP: i32 = 20;
/// `chat-spam-threshold-seconds` 和 `command-spam-threshold-seconds` 默认都是 10，阈值是它们的 20 倍。
const SPAM_THRESHOLD: i32 = 200;

/// `TickThrottler(20, 200)`：动作一次加 20，每 tick 减 1，到阈值就算刷屏。
#[derive(Debug, Default)]
struct TickThrottler {
    count: i32,
}

impl TickThrottler {
    fn increment(&mut self) {
        self.count += SPAM_INCREMENT_STEP;
    }

    fn tick(&mut self) {
        if self.count > 0 {
            self.count -= 1;
        }
    }

    fn is_under_threshold(&self) -> bool {
        self.count < SPAM_THRESHOLD
    }
}

/// 一名玩家的刷屏计数。官方的聊天和命令各有一个，互不影响。
#[derive(Debug, Default)]
pub(crate) struct SpamThrottlers {
    chat: TickThrottler,
    command: TickThrottler,
}

/// `ServerGamePacketListenerImpl.tick` 里的 `chatSpamThrottler.tick()` 和 `commandSpamThrottler.tick()`。
pub(crate) fn tick_spam_throttlers(region: &mut Region) {
    for player in region.players.values_mut() {
        player.spam.chat.tick();
        player.spam.command.tick();
    }
}

/// `detectChatRateSpam`：`World` 广播之后才调用，压线的那条消息所有人都收得到。
pub(crate) fn detect_chat_rate_spam(region: &mut Region, id: PlayerId) {
    if let Some(player) = region.players.get_mut(&id) {
        detect_rate_spam(&mut region.links, id, &mut player.spam.chat);
    }
}

/// `detectCommandRateSpam`。
fn detect_command_rate_spam(region: &mut Region, id: PlayerId) {
    if let Some(player) = region.players.get_mut(&id) {
        detect_rate_spam(&mut region.links, id, &mut player.spam.command);
    }
}

/// `detectRateSpam`。官方不踢 OP 和单人游戏的房主，Bamboo 没有这两种玩家。
fn detect_rate_spam(links: &mut Links, id: PlayerId, throttler: &mut TickThrottler) {
    throttler.increment();
    if !throttler.is_under_threshold() {
        links.kick(id, &Component::translate(SPAM, []));
    }
}

/// `chat`：编码一次，返回发给所有已进入 Play 的玩家的帧。发送者也要收到，客户端没有本地回显。
/// 广播之后要调用 [`detect_chat_rate_spam`]，没有返回帧的消息不计数。`chat-join.md` 第 4、5 节。
pub(crate) fn on_chat(region: &mut Region, id: PlayerId, message: String) -> Option<Vec<u8>> {
    if !is_legal(&message) {
        kick_for_illegal_characters(region, id);
        return None;
    }
    let player = region.players.get(&id)?;
    if player.client.chat_visibility == ChatVisibility::Hidden {
        let notice = Component::translate(CHAT_DISABLED, []).color(Color::Red);
        send_system_chat(region, id, &notice);
        return None;
    }
    // 官方这里多一个 `[Not Secure]` 前缀。Bamboo 的聊天全都没有签名，这个前缀不提供信息。
    info!("<{}> {message}", player.name);
    Some(chat_frames(&player.name, message))
}

/// `chat_command`：Bamboo 没有命令，回官方的两行红字之后才计数。`chat-join.md` 7.4。
pub(crate) fn on_chat_command(region: &mut Region, id: PlayerId, command: String) {
    if !is_legal(&command) {
        kick_for_illegal_characters(region, id);
        return;
    }
    let unknown = failure(Component::translate(UNKNOWN_COMMAND, []));
    send_system_chat(region, id, &unknown);
    send_system_chat(region, id, &failure(parse_context(&command)));
    detect_command_rate_spam(region, id);
}

/// `client_command`：`RequestStats` 回 `award_stats`，其他动作忽略。`chat-join.md` 第 4 节。
pub(crate) fn on_client_command(region: &mut Region, id: PlayerId, action: ClientCommandKind) {
    if action == ClientCommandKind::RequestStats {
        region.links.send(id, &AwardStats);
    }
}

fn is_legal(text: &str) -> bool {
    text.chars().all(is_allowed_chat_character)
}

fn kick_for_illegal_characters(region: &mut Region, id: PlayerId) {
    region
        .links
        .kick(id, &Component::translate(ILLEGAL_CHARACTERS, []));
}

fn send_system_chat(region: &mut Region, id: PlayerId, content: &Component) {
    let packet = SystemChat {
        content,
        overlay: false,
    };
    region.links.send(id, &packet);
}

/// 所有接收者共用的帧。客户端按 `minecraft:chat` 的模板拼出 `<name> message`。
fn chat_frames(name: &str, message: String) -> Vec<u8> {
    encode_frames(&DisguisedChat {
        message: &Component::text(message),
        chat_type: chat_type::CHAT,
        name: &Component::text(name),
        target_name: None,
    })
}

/// 官方的 `sendFailure` 在消息外面包的那层红色。
fn failure(message: Component) -> Component {
    Component::text("").color(Color::Red).extra(message)
}

/// 出错的位置，显示成 `foo bar<--[HERE]`，点击时把命令填回聊天框。
///
/// 命令树是空的，解析停在开头，所以整段输入都是解析不了的部分。
fn parse_context(command: &str) -> Component {
    let command = echoed_part(command);
    let mut context = Component::text("")
        .color(Color::Gray)
        .suggest_command(&format!("/{command}"));
    if !command.is_empty() {
        context = context.extra(Component::text(command).color(Color::Red).underlined());
    }
    context.extra(
        Component::translate(CONTEXT_HERE, [])
            .color(Color::Red)
            .italic(),
    )
}

fn echoed_part(command: &str) -> &str {
    match command.char_indices().nth(MAX_ECHOED_COMMAND_CHARS) {
        Some((end, _)) => &command[..end],
        None => command,
    }
}

#[cfg(test)]
mod tests;
