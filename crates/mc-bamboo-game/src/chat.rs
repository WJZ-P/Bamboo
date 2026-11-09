//! 聊天和命令。
//!
//! 聊天是全服的事情：这里只拼出要广播的帧，发给谁由 `World` 决定。
//! 没签名的玩家聊天仍是 `player_chat`，发送者是这名玩家。刷屏计数在 [`TickThrottler`] 里。
//!
//! 命令的解析和执行在 `command`。这里留下聊天，以及命令和聊天共用的刷屏计数。
//!
//! Command parsing and execution live in `command`. This module keeps chat and the
//! spam counters shared by chat and commands.

use mc_bamboo_data::registries::chat_type;
use mc_bamboo_protocol::{
    packet::play::{AwardStats, ChatVisibility, ClientCommandKind, LastSeenUpdate, SystemChat},
    text::{Color, Component, is_allowed_chat_character},
};
use tracing::info;

use crate::{inbound::PlayerId, links::Links, region::Region, secure_chat};

const ILLEGAL_CHARACTERS: &str = "multiplayer.disconnect.illegal_characters";
const SPAM: &str = "disconnect.spam";
const CHAT_DISABLED: &str = "chat.disabled.options";

/// 每条消息或者命令给 [`TickThrottler`] 加多少。
const SPAM_INCREMENT_STEP: i32 = 20;
/// `chat-spam-threshold-seconds` 和 `command-spam-threshold-seconds` 默认都是 10，阈值是它们的 20 倍。
///
/// 这是 `server.properties` 的阈值，不是游戏规则。官方 `ServerGamePacketListenerImpl.detectRateSpam`
/// 用的也是这个配置。
///
/// This is the `server.properties` threshold, not a game rule. Official
/// `ServerGamePacketListenerImpl.detectRateSpam` uses that setting too.
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
        let is_op = player.is_op;
        detect_rate_spam(&mut region.links, id, &mut player.spam.chat, is_op);
    }
}

/// `detectCommandRateSpam`。命令无论成败都计数。
///
/// `detectCommandRateSpam`. Counted whether the command succeeded or failed.
pub(crate) fn detect_command_rate_spam(region: &mut Region, id: PlayerId) {
    if let Some(player) = region.players.get_mut(&id) {
        let is_op = player.is_op;
        detect_rate_spam(&mut region.links, id, &mut player.spam.command, is_op);
    }
}

/// `detectRateSpam`。官方不踢 OP，也不踢单人游戏的房主。Bamboo 没有单人房主。
///
/// `detectRateSpam`. Official servers do not kick ops, or the owner of a singleplayer world.
/// Bamboo has no singleplayer owner.
fn detect_rate_spam(links: &mut Links, id: PlayerId, throttler: &mut TickThrottler, is_op: bool) {
    throttler.increment();
    if !throttler.is_under_threshold() && !is_op {
        links.kick(id, &Component::translate(SPAM, []));
    }
}

/// 命令文本里有官方不允许的字符时踢掉玩家，并返回 `true`。
///
/// Kicks the player when the command text contains a character the official server rejects,
/// and returns `true`.
pub(crate) fn reject_illegal_command(region: &mut Region, id: PlayerId, command: &str) -> bool {
    if is_legal(command) {
        return false;
    }
    kick_for_illegal_characters(region, id);
    true
}

/// `handleChat`。返回 `true` 时调用方要记一次聊天刷屏，和官方一样只有真的广播了才计数。
///
/// `handleChat`. The caller counts chat spam when this returns `true`. Official servers do that
/// only after a message was actually broadcast.
/// 聊天消息的内容。`&Global` 单独传，避免把整个世界都借给聊天。
///
/// The chat message. `&Global` is separate so chat does not borrow the whole world.
pub(crate) struct ChatMessage<'a> {
    pub global: &'a crate::global::Global,
    pub text: String,
    pub signature: Option<[u8; 256]>,
    pub time_millis: i64,
    pub salt: i64,
    pub last_seen: &'a LastSeenUpdate,
}

pub(crate) fn on_chat(region: &mut Region, id: PlayerId, message: ChatMessage<'_>) -> bool {
    let ChatMessage {
        global,
        text: message,
        signature,
        time_millis,
        salt,
        last_seen,
    } = message;
    // 已读窗口在非法字符和隐藏聊天之前套用，官方 `unpackAndApplyLastSeen` 也是这样。
    //
    // The seen window is applied before the illegal-character and hidden-chat checks.
    // Official `unpackAndApplyLastSeen` does the same.
    if !secure_chat::apply_last_seen(region, id, last_seen) {
        return false;
    }
    if !is_legal(&message) {
        kick_for_illegal_characters(region, id);
        return false;
    }
    let hidden = region
        .players
        .get(&id)
        .is_some_and(|player| player.client.chat_visibility == ChatVisibility::Hidden);
    if hidden {
        let notice = Component::translate(CHAT_DISABLED, []).color(Color::Red);
        send_system_chat(region, id, &notice);
        return false;
    }
    let name = match region.players.get(&id) {
        Some(player) => player.name.clone(),
        None => return false,
    };
    match secure_chat::accept_chat(region, id, &message, signature, time_millis, salt) {
        Ok(signed) => {
            let trusted = signed.is_trusted(region.now_millis());
            log_player_broadcast(trusted, chat_type::CHAT, &name, &message);
            let display = global.scoreboard().format_player_name(&name);
            secure_chat::broadcast_signed(region, &signed, chat_type::CHAT, &display, None, None);
            true
        }
        Err(()) => false,
    }
}

/// 官方 `PlayerList.logChatMessage`。不可信的消息前面加 `[Not Secure]`。
///
/// Official `PlayerList.logChatMessage`. Untrusted messages are prefixed with `[Not Secure]`.
pub(crate) fn log_player_broadcast(trusted: bool, chat_type_id: i32, name: &str, content: &str) {
    let line = match chat_type_id {
        chat_type::SAY_COMMAND => format!("[{name}] {content}"),
        chat_type::EMOTE_COMMAND => format!("* {name} {content}"),
        _ => format!("<{name}> {content}"),
    };
    if trusted {
        info!("{line}");
    } else {
        info!("[Not Secure] {line}");
    }
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

#[cfg(test)]
mod tests;
