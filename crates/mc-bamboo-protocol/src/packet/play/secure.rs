//! 安全聊天用的字段和 `player_chat`、`delete_chat`。
//!
//! 布局对照官方 `LastSeenMessages.Update`、`ArgumentSignatures`、`RemoteChatSession.Data`、
//! `ClientboundPlayerChatPacket` 和 `ClientboundDeleteChatPacket`。
//!
//! Fields for secure chat, plus `player_chat` and `delete_chat`.
//!
//! Layout follows official `LastSeenMessages.Update`, `ArgumentSignatures`,
//! `RemoteChatSession.Data`, `ClientboundPlayerChatPacket`, and `ClientboundDeleteChatPacket`.

use uuid::Uuid;

use crate::packet::ClientboundPacket;
use crate::{
    codec::{write_bool, write_byte_array, write_i64, write_string, write_uuid, write_varint},
    text::Component,
};

/// 一条聊天签名。官方 `MessageSignature.BYTES`，没有长度前缀。
///
/// One chat signature. Official `MessageSignature.BYTES`, with no length prefix.
pub const MESSAGE_SIGNATURE_BYTES: usize = 256;
/// 玩家公钥的 DER 上限。官方 `ByteBufCodecs.MAX_PUBLIC_KEY_LENGTH`。
///
/// Player public key DER limit. Official `ByteBufCodecs.MAX_PUBLIC_KEY_LENGTH`.
pub const MAX_PUBLIC_KEY_BYTES: usize = 512;
/// Mojang 给公钥的签名上限。官方 `ProfilePublicKey.Data.MAX_KEY_SIGNATURE_SIZE`。
///
/// Mojang's signature over the player public key. Official `ProfilePublicKey.Data.MAX_KEY_SIGNATURE_SIZE`.
pub const MAX_KEY_SIGNATURE_BYTES: usize = 4096;
/// `LastSeenMessages.Update` 的确认窗口。20 位，固定 3 字节。
///
/// Acknowledgement window of `LastSeenMessages.Update`. 20 bits, fixed at 3 bytes.
pub const LAST_SEEN_BYTES: usize = 3;

pub type MessageSignatureBytes = [u8; MESSAGE_SIGNATURE_BYTES];

/// 官方 `LastSeenMessages.Update`。校验和 0 表示不核对。
///
/// Official `LastSeenMessages.Update`. A checksum of 0 skips the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastSeenUpdate {
    pub offset: i32,
    pub acknowledged: [u8; LAST_SEEN_BYTES],
    pub checksum: i8,
}

impl LastSeenUpdate {
    /// 空窗口：偏移 0、没有确认位、校验和 1。离线客户端发的就是这个。
    ///
    /// Empty window: offset 0, no acknowledged bits, checksum 1. Offline clients send this.
    pub const EMPTY: Self = Self {
        offset: 0,
        acknowledged: [0; LAST_SEEN_BYTES],
        checksum: 1,
    };
}

/// `chat_session_update` 里的 `RemoteChatSession.Data`。
///
/// `RemoteChatSession.Data` from `chat_session_update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteChatSession {
    pub session_id: Uuid,
    pub expires_at_millis: i64,
    pub public_key: Vec<u8>,
    pub key_signature: Vec<u8>,
}

/// 一个参数的签名。官方 `ArgumentSignatures.Entry`。
///
/// One argument signature. Official `ArgumentSignatures.Entry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgumentSignature {
    pub name: String,
    pub signature: MessageSignatureBytes,
}

/// `chat_command_signed` 里签名相关的字段。命令文本本身在外面。
///
/// Signature fields of `chat_command_signed`. The command text lives outside this struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedCommand {
    pub time_millis: i64,
    pub salt: i64,
    pub arguments: Vec<ArgumentSignature>,
    pub last_seen: LastSeenUpdate,
}

/// `LastSeenMessages.Packed` 里的一条。缓存命中写 `id + 1`，否则写 0 再跟 256 字节。
///
/// One entry of `LastSeenMessages.Packed`. A cache hit writes `id + 1`; a miss writes 0 and then 256 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackedSignature<'a> {
    Cached(i32),
    Full(&'a MessageSignatureBytes),
}

/// 带签名的玩家聊天。每个接收者的 `global_index` 和打包后的已读列表都不一样，所以不能只编码一次。
///
/// Signed player chat. `global_index` and the packed last-seen list differ per recipient, so this is not encoded once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerChat<'a> {
    pub global_index: i32,
    pub sender: Uuid,
    pub index: i32,
    pub signature: Option<&'a MessageSignatureBytes>,
    pub content: &'a str,
    pub time_millis: i64,
    pub salt: i64,
    pub last_seen: &'a [PackedSignature<'a>],
    pub unsigned_content: Option<&'a Component>,
    /// `minecraft:chat_type` 的注册表 ID。线上的值比它大一。
    ///
    /// Registry id of `minecraft:chat_type`. The value on the wire is one larger.
    pub chat_type: i32,
    pub name: &'a Component,
    pub target_name: Option<&'a Component>,
}

impl ClientboundPacket for PlayerChat<'_> {
    const ID: i32 = mc_bamboo_data::packets::play::clientbound::PLAYER_CHAT;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.global_index);
        write_uuid(out, self.sender);
        write_varint(out, self.index);
        write_bool(out, self.signature.is_some());
        if let Some(signature) = self.signature {
            out.extend_from_slice(signature);
        }
        write_string(out, self.content);
        write_i64(out, self.time_millis);
        write_i64(out, self.salt);
        write_varint(out, self.last_seen.len() as i32);
        for entry in self.last_seen {
            match entry {
                PackedSignature::Cached(id) => write_varint(out, id.wrapping_add(1)),
                PackedSignature::Full(signature) => {
                    write_varint(out, 0);
                    out.extend_from_slice(signature.as_slice());
                }
            }
        }
        write_bool(out, self.unsigned_content.is_some());
        if let Some(content) = self.unsigned_content {
            content.write(out);
        }
        // `FilterMask.PASS_THROUGH`。没有文本过滤。
        //
        // `FilterMask.PASS_THROUGH`. There is no text filter.
        write_varint(out, 0);
        write_varint(out, self.chat_type + 1);
        self.name.write(out);
        write_bool(out, self.target_name.is_some());
        if let Some(target) = self.target_name {
            target.write(out);
        }
    }
}

/// 官方 `ClientboundDeleteChatPacket`。原版专用服务器不会发这个包，编码留在这里，调用点不发。
///
/// Official `ClientboundDeleteChatPacket`. The vanilla dedicated server never sends it.
/// The encoder is here; nothing in the server emits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteChat<'a> {
    pub signature: &'a MessageSignatureBytes,
}

impl ClientboundPacket for DeleteChat<'_> {
    const ID: i32 = mc_bamboo_data::packets::play::clientbound::DELETE_CHAT;

    fn write_body(&self, out: &mut Vec<u8>) {
        // 完整签名：id 写成 0，后面是 256 字节。
        //
        // A full signature: the id is written as 0, then 256 bytes.
        write_varint(out, 0);
        out.extend_from_slice(self.signature);
    }
}

/// 给 Tab 列表用的聊天会话。没有会话时写一个 `false`。
///
/// Chat session shown in the tab list. Absence is a single `false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatSessionView<'a> {
    pub session_id: Uuid,
    pub expires_at_millis: i64,
    pub public_key: &'a [u8],
    pub key_signature: &'a [u8],
}

pub(crate) fn write_chat_session(out: &mut Vec<u8>, session: Option<ChatSessionView<'_>>) {
    let Some(session) = session else {
        write_bool(out, false);
        return;
    };
    write_bool(out, true);
    write_uuid(out, session.session_id);
    write_i64(out, session.expires_at_millis);
    write_byte_array(out, session.public_key);
    write_byte_array(out, session.key_signature);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::test_util::{body, hex};

    #[test]
    fn delete_chat_writes_the_full_signature() {
        let signature = [0xab; MESSAGE_SIGNATURE_BYTES];
        let mut expected = vec![0x00];
        expected.extend(signature);
        assert_eq!(
            body(&DeleteChat {
                signature: &signature
            }),
            expected
        );
    }

    #[test]
    fn player_chat_packs_a_cached_signature_as_id_plus_one() {
        let signature = [0x11; MESSAGE_SIGNATURE_BYTES];
        let name = Component::text("A");
        let last_seen = [
            PackedSignature::Cached(0),
            PackedSignature::Full(&signature),
        ];
        let packet = PlayerChat {
            global_index: 0,
            sender: Uuid::nil(),
            index: 0,
            signature: Some(&signature),
            content: "hi",
            time_millis: 0,
            salt: 0,
            last_seen: &last_seen,
            unsigned_content: None,
            chat_type: 0,
            name: &name,
            target_name: None,
        };
        let encoded = body(&packet);
        assert_eq!(&encoded[..1], &hex("00"));
        assert_eq!(encoded[1 + 16], 0);
        assert_eq!(encoded[1 + 16 + 1], 1);
        assert_eq!(&encoded[1 + 16 + 2..1 + 16 + 2 + 256], &signature);
    }
}
