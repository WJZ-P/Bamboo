//! 一名玩家的聊天会话，以及 `chat_session_update`、`chat_ack`、签名消息和签名命令。
//!
//! One player's chat session, plus `chat_session_update`, `chat_ack`, signed messages, and signed commands.

use mc_bamboo_command::ParsedCommand;
use mc_bamboo_protocol::{
    packet::play::{LastSeenUpdate, RemoteChatSession, SignedCommand, SystemChat},
    text::{Color, Component},
};
use rsa::{RsaPublicKey, pkcs8::DecodePublicKey};
use tracing::warn;
use uuid::Uuid;

use crate::{inbound::PlayerId, region::Region};

use super::{
    chain::{self, ChainFailure, Decoder, LastSeenValidator, SignatureCache, SignedMessage},
    enforces,
};

const CHAT_VALIDATION_FAILED: &str = "multiplayer.disconnect.chat_validation_failed";
const EXPIRED_PUBLIC_KEY: &str = "multiplayer.disconnect.expired_public_key";
const INVALID_PUBLIC_KEY: &str = "multiplayer.disconnect.invalid_public_key_signature";

/// 验过的会话，写进 Tab 列表再广播出去。
///
/// A validated session, stored on the tab list and broadcast from there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionUpdate {
    pub(crate) session_id: Uuid,
    pub(crate) expires_at_millis: i64,
    pub(crate) public_key: Vec<u8>,
    pub(crate) key_signature: Vec<u8>,
}

#[derive(Debug, Clone)]
struct StoredKey {
    expires_at_millis: i64,
    public_key: Vec<u8>,
    key_signature: Vec<u8>,
}

/// 挂在 [`crate::player::Player`] 上的聊天状态。
///
/// Chat state stored on [`crate::player::Player`].
#[derive(Debug)]
pub(crate) struct PlayerChat {
    decoder: Decoder,
    pub(crate) seen: LastSeenValidator,
    pub(crate) cache: SignatureCache,
    pub(crate) next_index: i32,
    arguments: Vec<(String, SignedMessage)>,
    /// 这一条入站消息刚套用过的已读列表，紧跟着的验签要用同一份。
    ///
    /// Last-seen signatures just applied for the inbound message. Verification uses the same list.
    pending_seen: Vec<chain::MessageSignatureBytes>,
    stored_key: Option<StoredKey>,
}

type MessageSignatureBytes = chain::MessageSignatureBytes;

impl Default for PlayerChat {
    fn default() -> Self {
        Self {
            decoder: Decoder::Unsigned,
            seen: LastSeenValidator::default(),
            cache: SignatureCache::default(),
            next_index: 0,
            arguments: Vec::new(),
            pending_seen: Vec::new(),
            stored_key: None,
        }
    }
}

/// `handleChatSessionUpdate`。相同的公钥数据直接忽略，哪怕会话 ID 变了。
///
/// `handleChatSessionUpdate`. Identical key data is ignored, even when the session id changed.
pub(crate) fn on_session(
    region: &mut Region,
    id: PlayerId,
    session: RemoteChatSession,
) -> Option<SessionUpdate> {
    let player = region.players.get(&id)?;
    let player_uuid = player.uuid;
    let player_name = player.name.clone();
    let stored = player.chat.stored_key.as_ref().map(|key| StoredKey {
        expires_at_millis: key.expires_at_millis,
        public_key: key.public_key.clone(),
        key_signature: key.key_signature.clone(),
    });
    if let Some(stored) = &stored
        && stored.expires_at_millis == session.expires_at_millis
        && stored.public_key == session.public_key
        && stored.key_signature == session.key_signature
    {
        return None;
    }
    if let Some(stored) = &stored
        && session.expires_at_millis < stored.expires_at_millis
    {
        region
            .links
            .kick(id, &Component::translate(EXPIRED_PUBLIC_KEY, []));
        return None;
    }
    if !region.service_keys.can_validate() {
        warn!("ignoring a chat session from {player_name} because profile public keys are missing");
        return None;
    }
    let payload =
        chain::profile_key_payload(player_uuid, session.expires_at_millis, &session.public_key);
    if !region
        .service_keys
        .verify_profile_signature(&payload, &session.key_signature)
    {
        warn!(
            "rejected a chat session from {player_name} because the profile key signature is invalid"
        );
        region
            .links
            .kick(id, &Component::translate(INVALID_PUBLIC_KEY, []));
        return None;
    }
    let Ok(public_key) = RsaPublicKey::from_public_key_der(&session.public_key) else {
        warn!(
            "rejected a chat session from {player_name} because the profile key is not an RSA key"
        );
        region
            .links
            .kick(id, &Component::translate(INVALID_PUBLIC_KEY, []));
        return None;
    };
    let player = region.players.get_mut(&id)?;
    player.chat.decoder = Decoder::from_key(
        player_uuid,
        session.session_id,
        public_key,
        session.expires_at_millis,
    );
    player.chat.stored_key = Some(StoredKey {
        expires_at_millis: session.expires_at_millis,
        public_key: session.public_key.clone(),
        key_signature: session.key_signature.clone(),
    });
    Some(SessionUpdate {
        session_id: session.session_id,
        expires_at_millis: session.expires_at_millis,
        public_key: session.public_key,
        key_signature: session.key_signature,
    })
}

/// `chat_ack` 只前移窗口。失败就断开。
///
/// `chat_ack` only advances the window. Failure disconnects.
pub(crate) fn on_ack(region: &mut Region, id: PlayerId, offset: i32) {
    let failed = region
        .players
        .get_mut(&id)
        .is_some_and(|player| player.chat.seen.apply_offset(offset).is_err());
    if failed {
        warn!("rejected a chat acknowledgement from player {}", id.0);
        region
            .links
            .kick(id, &Component::translate(CHAT_VALIDATION_FAILED, []));
    }
}

/// `unpackAndApplyLastSeen`。失败时已经把玩家踢了，返回 `false`。
///
/// `unpackAndApplyLastSeen`. Returns `false` after the player has been kicked.
pub(crate) fn apply_last_seen(region: &mut Region, id: PlayerId, update: &LastSeenUpdate) -> bool {
    let applied = region
        .players
        .get_mut(&id)
        .map(|player| player.chat.seen.apply_update(update));
    match applied {
        Some(Ok(seen)) => {
            if let Some(player) = region.players.get_mut(&id) {
                player.chat.pending_seen = seen;
            }
            true
        }
        Some(Err(())) => {
            warn!("rejected last-seen messages from player {}", id.0);
            region
                .links
                .kick(id, &Component::translate(CHAT_VALIDATION_FAILED, []));
            false
        }
        None => false,
    }
}

/// 聊天正文。`Ok(None)` 是没有签名、按伪装聊天转发的消息。
///
/// A chat body. `Ok(None)` is an unsigned message forwarded as disguised chat.
pub(crate) fn accept_chat(
    region: &mut Region,
    id: PlayerId,
    content: &str,
    signature: Option<MessageSignatureBytes>,
    time_millis: i64,
    salt: i64,
) -> Result<Option<SignedMessage>, ()> {
    let enforced = enforces(region);
    let unpacked = {
        let Some(player) = region.players.get_mut(&id) else {
            return Err(());
        };
        let sender = player.uuid;
        let last_seen = std::mem::take(&mut player.chat.pending_seen);
        player.chat.decoder.unpack(
            enforced,
            sender,
            signature,
            chain::MessageParts {
                content,
                time_millis,
                salt,
                last_seen,
                now_millis: chain::now_millis(),
            },
        )
    };
    match unpacked {
        Ok(message) if message.signature.is_some() => Ok(Some(message)),
        Ok(_) => Ok(None),
        Err(failure) => {
            tell_failure(region, id, failure);
            Err(())
        }
    }
}

/// 签名命令的参数。空的签名列表走无签名解码。失败时发系统消息，命令仍然计入刷屏。
///
/// Arguments of a signed command. An empty signature list uses the unsigned decoder.
/// Failure sends a system message; the command still counts toward spam.
pub(crate) fn collect_command(
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    signed: &SignedCommand,
) -> bool {
    let expected: Vec<(String, String)> = parsed
        .arguments
        .iter()
        .filter_map(|(name, value)| {
            value
                .as_message()
                .map(|message| (name.clone(), message.text.clone()))
        })
        .collect();
    let enforced = enforces(region);
    let now = chain::now_millis();
    let outcome = {
        let Some(player) = region.players.get_mut(&id) else {
            return false;
        };
        player.chat.arguments.clear();
        let sender = player.uuid;
        let last_seen = player.chat.pending_seen.clone();
        collect_into(player, enforced, sender, &expected, signed, &last_seen, now)
    };
    match outcome {
        Ok(arguments) => {
            if let Some(player) = region.players.get_mut(&id) {
                player.chat.arguments = arguments;
            }
            true
        }
        Err(failure) => {
            tell_failure(region, id, failure);
            false
        }
    }
}

fn collect_into(
    player: &mut crate::player::Player,
    enforced: bool,
    sender: Uuid,
    expected: &[(String, String)],
    signed: &SignedCommand,
    last_seen: &[MessageSignatureBytes],
    now: i64,
) -> Result<Vec<(String, SignedMessage)>, ChainFailure> {
    if signed.arguments.is_empty() {
        let mut stored = Vec::new();
        for (name, text) in expected {
            let message = player.chat.decoder.unpack(
                enforced,
                sender,
                None,
                chain::MessageParts {
                    content: text,
                    time_millis: now,
                    salt: 0,
                    last_seen: Vec::new(),
                    now_millis: now,
                },
            )?;
            stored.push((name.clone(), message));
        }
        return Ok(stored);
    }

    let mut accepted = Vec::new();
    for argument in &signed.arguments {
        let Some((_, text)) = expected.iter().find(|(name, _)| name == &argument.name) else {
            player.chat.decoder.break_chain();
            return Err(ChainFailure {
                key: chain::INVALID_COMMAND_SIGNATURE,
            });
        };
        let message = player.chat.decoder.unpack(
            enforced,
            sender,
            Some(argument.signature),
            chain::MessageParts {
                content: text,
                time_millis: signed.time_millis,
                salt: signed.salt,
                last_seen: last_seen.to_vec(),
                now_millis: now,
            },
        )?;
        accepted.push((argument.name.clone(), message));
    }
    for (name, _) in expected {
        if !accepted.iter().any(|(signed_name, _)| signed_name == name) {
            return Err(ChainFailure {
                key: chain::INVALID_COMMAND_SIGNATURE,
            });
        }
    }
    Ok(accepted)
}

pub(crate) fn take_argument(
    region: &mut Region,
    id: PlayerId,
    name: &str,
) -> Option<SignedMessage> {
    let player = region.players.get_mut(&id)?;
    let index = player
        .chat
        .arguments
        .iter()
        .position(|(argument, _)| argument == name)?;
    Some(player.chat.arguments.swap_remove(index).1)
}

fn tell_failure(region: &mut Region, id: PlayerId, failure: ChainFailure) {
    warn!(
        player = id.0,
        key = failure.key,
        "failed to update the secure chat state"
    );
    let content = Component::translate(failure.key, []).color(Color::Red);
    region.links.send(
        id,
        &SystemChat {
            content: &content,
            overlay: false,
        },
    );
}
