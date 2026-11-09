//! 把验过的消息按接收者分别装进 `player_chat`。
//!
//! Pack a verified message into `player_chat`, once per recipient.

use mc_bamboo_protocol::{
    packet::play::{ChatVisibility, PlayerChat},
    text::Component,
};

use crate::{inbound::PlayerId, region::Region};

use super::chain::{MAX_TRACKED, SignedMessage};

const TOO_MANY: &str = "multiplayer.disconnect.too_many_pending_chats";

/// 发给所有聊天设成“全部”的玩家，包括发送者自己。
///
/// Sends to every player whose chat visibility is full, including the sender.
pub(crate) fn broadcast_signed(
    region: &mut Region,
    message: &SignedMessage,
    chat_type: i32,
    name: &Component,
    target_name: Option<&Component>,
    unsigned: Option<&Component>,
) {
    let ids: Vec<PlayerId> = region
        .players
        .iter()
        .filter(|(_, player)| player.client.chat_visibility == ChatVisibility::Full)
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        send_one(region, id, message, chat_type, name, target_name, unsigned);
    }
}

/// 只发给一个人。对方把聊天关成“仅系统”或“隐藏”时官方也不发。
///
/// Sends to one player. Official servers also skip players whose chat is system-only or hidden.
pub(crate) fn send_signed_to(
    region: &mut Region,
    to: PlayerId,
    message: &SignedMessage,
    chat_type: i32,
    name: &Component,
    target_name: Option<&Component>,
    unsigned: Option<&Component>,
) {
    let visible = region
        .players
        .get(&to)
        .is_some_and(|player| player.client.chat_visibility == ChatVisibility::Full);
    if visible {
        send_one(region, to, message, chat_type, name, target_name, unsigned);
    }
}

fn send_one(
    region: &mut Region,
    to: PlayerId,
    message: &SignedMessage,
    chat_type: i32,
    name: &Component,
    target_name: Option<&Component>,
    unsigned: Option<&Component>,
) {
    let (global_index, packed) = {
        let Some(player) = region.players.get_mut(&to) else {
            return;
        };
        let global_index = player.chat.next_index;
        player.chat.next_index = global_index.wrapping_add(1);
        let packed = player.chat.cache.pack(&message.last_seen);
        (global_index, packed)
    };
    region.links.send(
        to,
        &PlayerChat {
            global_index,
            sender: message.sender,
            index: message.index,
            signature: message.signature.as_ref(),
            content: &message.content,
            time_millis: message.time_millis,
            salt: message.salt,
            last_seen: &packed,
            unsigned_content: unsigned,
            chat_type,
            name,
            target_name,
        },
    );
    // 没有签名时官方只增加 `nextChatIndex`，不推进已读窗口。
    //
    // Without a signature, official code only increments `nextChatIndex`. It does not
    // advance the seen window.
    let Some(signature) = message.signature else {
        return;
    };
    let Some(player) = region.players.get_mut(&to) else {
        return;
    };
    player.chat.cache.push(&message.last_seen, Some(signature));
    player.chat.seen.add_pending(signature);
    let too_many = player.chat.seen.len() > MAX_TRACKED;
    if too_many {
        region.links.kick(to, &Component::translate(TOO_MANY, []));
    }
}
