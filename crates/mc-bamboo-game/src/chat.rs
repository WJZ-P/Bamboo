//! 聊天和命令。
//!
//! 聊天是全服的事情：这里只拼出要广播的帧，发给谁由 `World` 决定。
//! 离线模式的消息没有签名，每名玩家要记住的只有 [`SpamThrottlers`] 里的刷屏计数。
//!
//! 命令的解析和执行在 `command`。这里留下聊天，以及命令和聊天共用的刷屏计数。
//!
//! Command parsing and execution live in `command`. This module keeps chat and the
//! spam counters shared by chat and commands.

use mc_bamboo_data::registries::chat_type;
use mc_bamboo_protocol::{
    packet::play::{
        AwardStats, ChatVisibility, ClientCommandKind, DisguisedChat, LastSeenUpdate, SystemChat,
    },
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
pub(crate) fn on_chat(
    region: &mut Region,
    id: PlayerId,
    message: String,
    signature: Option<[u8; 256]>,
    time_millis: i64,
    salt: i64,
    last_seen: &LastSeenUpdate,
) -> bool {
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
        Ok(Some(signed)) => {
            info!("<{name}> {message}");
            let display = Component::text(name);
            secure_chat::broadcast_signed(region, &signed, chat_type::CHAT, &display, None, None);
            true
        }
        Ok(None) => {
            // 没有公钥、也没强制签名时，官方会发不带签名的 `player_chat`。
            // 这里仍发 `disguised_chat`，离线探针和现有测试按这个包核对。
            //
            // With no key and secure chat not enforced, official servers send unsigned `player_chat`.
            // This still sends `disguised_chat`, which the offline probe and the existing tests check.
            info!("<{name}> {message}");
            deliver_disguised(
                region,
                &name,
                &Component::text(message),
                chat_type::CHAT,
                None,
            );
            true
        }
        Err(()) => false,
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

/// 只发给聊天可见性是“全部”的玩家。官方 `ServerPlayer.acceptsChatMessages`。
///
/// Sends only to players whose chat visibility is full. Official `ServerPlayer.acceptsChatMessages`.
fn deliver_disguised(
    region: &mut Region,
    name: &str,
    message: &Component,
    chat_type: i32,
    target_name: Option<&Component>,
) {
    let display = Component::text(name);
    let ids: Vec<PlayerId> = region
        .players
        .iter()
        .filter(|(_, player)| player.client.chat_visibility == ChatVisibility::Full)
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        region.links.send(
            id,
            &DisguisedChat {
                message,
                chat_type,
                name: &display,
                target_name,
            },
        );
    }
}

#[cfg(test)]
mod tests;
