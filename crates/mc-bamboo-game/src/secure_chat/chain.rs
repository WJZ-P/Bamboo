//! `SignedMessageChain`、`LastSeenMessagesValidator` 和 `MessageSignatureCache`。
//!
//! `SignedMessageChain`, `LastSeenMessagesValidator`, and `MessageSignatureCache`.

use std::collections::{HashSet, VecDeque};

use rsa::RsaPublicKey;
use uuid::Uuid;

use mc_bamboo_protocol::packet::play::LastSeenUpdate;
pub(crate) use mc_bamboo_protocol::packet::play::MessageSignatureBytes;

use super::verify_sha256;

/// 官方 `LastSeenMessages.LAST_SEEN_MESSAGES_MAX_LENGTH`。
///
/// Official `LastSeenMessages.LAST_SEEN_MESSAGES_MAX_LENGTH`.
const WINDOW: usize = 20;
/// 官方 `MessageSignatureCache` 的默认容量。
///
/// Default capacity of `MessageSignatureCache`.
const CACHE: usize = 128;
/// 待确认的签名超过这个数就踢人。官方 `sendPlayerChatMessage` 用 4096。
///
/// Kick once pending signatures pass this count. Official `sendPlayerChatMessage` uses 4096.
pub(crate) const MAX_TRACKED: usize = 4096;
/// 签名正文开头的版本号。官方 `PlayerChatMessage.updateSignature` 写的是 1。
///
/// Version prefix of a signature payload. Official `PlayerChatMessage.updateSignature` writes 1.
const SIGNATURE_VERSION: i32 = 1;
/// 服务端认为消息过期的时间。官方 `PlayerChatMessage.MESSAGE_EXPIRES_AFTER_SERVER`，5 分钟。
///
/// When the server treats a message as expired. Official `PlayerChatMessage.MESSAGE_EXPIRES_AFTER_SERVER`, 5 minutes.
const MESSAGE_EXPIRES_AFTER_MILLIS: i64 = 5 * 60 * 1000;

const MISSING_PROFILE_KEY: &str = "chat.disabled.missingProfileKey";
const CHAIN_BROKEN: &str = "chat.disabled.chain_broken";
const EXPIRED_PROFILE_KEY: &str = "chat.disabled.expiredProfileKey";
const INVALID_SIGNATURE: &str = "chat.disabled.invalid_signature";
const OUT_OF_ORDER: &str = "chat.disabled.out_of_order_chat";
pub(crate) const INVALID_COMMAND_SIGNATURE: &str = "chat.disabled.invalid_command_signature";

/// 验签通过、可以转发给别人的一条消息。
///
/// One message that verified and can be forwarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SignedMessage {
    pub(crate) sender: Uuid,
    pub(crate) index: i32,
    pub(crate) signature: Option<MessageSignatureBytes>,
    pub(crate) content: String,
    pub(crate) time_millis: i64,
    pub(crate) salt: i64,
    pub(crate) last_seen: Vec<MessageSignatureBytes>,
}

impl SignedMessage {
    /// 官方 `PlayerList.verifyChatTrusted`：有签名，而且服务端还没把它当成过期。
    ///
    /// Official `PlayerList.verifyChatTrusted`: the message has a signature and the server
    /// does not consider it expired.
    pub(crate) fn is_trusted(&self, now_millis: i64) -> bool {
        self.signature.is_some()
            && now_millis
                <= self
                    .time_millis
                    .saturating_add(MESSAGE_EXPIRES_AFTER_MILLIS)
    }
}

/// 链上的失败。翻译键交给系统消息，再染成红色。
///
/// A chain failure. The translation key becomes a system message, then turns red.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChainFailure {
    pub(crate) key: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct Tracked {
    signature: MessageSignatureBytes,
    pending: bool,
}

/// 官方 `LastSeenMessagesValidator`。一开始放 20 个空位。
///
/// Official `LastSeenMessagesValidator`. It starts with 20 empty slots.
#[derive(Debug, Clone)]
pub(crate) struct LastSeenValidator {
    entries: VecDeque<Option<Tracked>>,
    last_pending: Option<MessageSignatureBytes>,
}

impl Default for LastSeenValidator {
    fn default() -> Self {
        Self {
            entries: VecDeque::from(vec![None; WINDOW]),
            last_pending: None,
        }
    }
}

impl LastSeenValidator {
    pub(crate) fn add_pending(&mut self, signature: MessageSignatureBytes) {
        if self.last_pending == Some(signature) {
            return;
        }
        self.entries.push_back(Some(Tracked {
            signature,
            pending: true,
        }));
        self.last_pending = Some(signature);
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// 官方 `applyOffset`。偏移必须落在 `0..=size-20`。
    ///
    /// Official `applyOffset`. The offset has to land in `0..=size-20`.
    pub(crate) fn apply_offset(&mut self, offset: i32) -> Result<(), ()> {
        let max = self.entries.len() - WINDOW;
        if offset < 0 || offset as usize > max {
            return Err(());
        }
        for _ in 0..offset {
            self.entries.pop_front();
        }
        Ok(())
    }

    /// 官方 `applyUpdate`。成功时返回这一条消息签名里要带上的已读签名。
    ///
    /// Official `applyUpdate`. On success, the last-seen signatures that belong in this message's signature.
    pub(crate) fn apply_update(
        &mut self,
        update: &LastSeenUpdate,
    ) -> Result<Vec<MessageSignatureBytes>, ()> {
        self.apply_offset(update.offset)?;
        if bitset_length(&update.acknowledged) > WINDOW {
            return Err(());
        }
        let mut seen = Vec::new();
        for index in 0..WINDOW {
            let acknowledged = bit(update.acknowledged, index);
            if acknowledged {
                let Some(entry) = self.entries.get(index).and_then(Option::as_ref) else {
                    return Err(());
                };
                let signature = entry.signature;
                if let Some(slot) = self.entries.get_mut(index).and_then(Option::as_mut) {
                    slot.pending = false;
                }
                seen.push(signature);
            } else if self
                .entries
                .get(index)
                .and_then(Option::as_ref)
                .is_some_and(|entry| !entry.pending)
            {
                return Err(());
            } else if let Some(slot) = self.entries.get_mut(index) {
                *slot = None;
            }
        }
        if !checksum_matches(update.checksum, &seen) {
            return Err(());
        }
        Ok(seen)
    }
}

/// 官方 `MessageSignatureCache`。`pack` 在 `push` 之前调用。
///
/// Official `MessageSignatureCache`. `pack` runs before `push`.
#[derive(Debug, Clone)]
pub(crate) struct SignatureCache {
    entries: [Option<MessageSignatureBytes>; CACHE],
}

impl Default for SignatureCache {
    fn default() -> Self {
        Self {
            entries: [None; CACHE],
        }
    }
}

impl SignatureCache {
    pub(crate) fn pack<'a>(
        &self,
        signatures: &'a [MessageSignatureBytes],
    ) -> Vec<mc_bamboo_protocol::packet::play::PackedSignature<'a>> {
        signatures
            .iter()
            .map(|signature| match self.find(signature) {
                Some(id) => mc_bamboo_protocol::packet::play::PackedSignature::Cached(id),
                None => mc_bamboo_protocol::packet::play::PackedSignature::Full(signature),
            })
            .collect()
    }

    fn find(&self, signature: &MessageSignatureBytes) -> Option<i32> {
        self.entries
            .iter()
            .position(|entry| entry.as_ref() == Some(signature))
            .map(|index| index as i32)
    }

    /// 官方 `push`：从队尾填进缓存，被挤出去、又不在新条目里的旧签名排回队首。
    ///
    /// Official `push`: fill the cache from the back of the queue. An evicted signature that is
    /// not part of the new batch goes back to the front.
    pub(crate) fn push(
        &mut self,
        last_seen: &[MessageSignatureBytes],
        signature: Option<MessageSignatureBytes>,
    ) {
        let mut queue: VecDeque<MessageSignatureBytes> = last_seen.iter().copied().collect();
        if let Some(signature) = signature {
            queue.push_back(signature);
        }
        let fresh: HashSet<MessageSignatureBytes> = queue.iter().copied().collect();
        let mut index = 0;
        while !queue.is_empty() && index < CACHE {
            let previous = self.entries[index].take();
            self.entries[index] = queue.pop_back();
            if let Some(previous) = previous
                && !fresh.contains(&previous)
            {
                queue.push_front(previous);
            }
            index += 1;
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Link {
    index: i32,
    sender: Uuid,
    session_id: Uuid,
}

/// 有公钥时的解码器。没有公钥时走 [`Decoder::Unsigned`]。
///
/// Decoder used once a public key is installed. Without one, [`Decoder::Unsigned`] is used.
#[derive(Debug)]
pub(crate) enum Decoder {
    Unsigned,
    Key {
        public_key: RsaPublicKey,
        expires_at_millis: i64,
        link: Option<Link>,
        last_timestamp_millis: i64,
    },
}

impl Decoder {
    pub(crate) fn from_key(
        sender: Uuid,
        session_id: Uuid,
        public_key: RsaPublicKey,
        expires_at_millis: i64,
    ) -> Self {
        Self::Key {
            public_key,
            expires_at_millis,
            link: Some(Link {
                index: 0,
                sender,
                session_id,
            }),
            last_timestamp_millis: 0,
        }
    }

    pub(crate) fn break_chain(&mut self) {
        if let Self::Key { link, .. } = self {
            *link = None;
        }
    }

    /// 官方 `SignedMessageChain.Decoder.unpack`。`enforces` 只在没有公钥时有用。
    ///
    /// Official `SignedMessageChain.Decoder.unpack`. `enforces` matters only while no key is installed.
    pub(crate) fn unpack(
        &mut self,
        enforces: bool,
        sender: Uuid,
        signature: Option<MessageSignatureBytes>,
        body: MessageParts<'_>,
    ) -> Result<SignedMessage, ChainFailure> {
        match self {
            Self::Unsigned => {
                if enforces {
                    Err(ChainFailure {
                        key: MISSING_PROFILE_KEY,
                    })
                } else {
                    // 官方 `PlayerChatMessage.unsigned`：时间是现在，盐是 0，已读列表是空的。
                    //
                    // Official `PlayerChatMessage.unsigned`: the time is now, the salt is 0,
                    // and the last-seen list is empty.
                    Ok(SignedMessage {
                        sender,
                        index: 0,
                        signature: None,
                        content: body.content.to_owned(),
                        time_millis: body.now_millis,
                        salt: 0,
                        last_seen: Vec::new(),
                    })
                }
            }
            Self::Key {
                public_key,
                expires_at_millis,
                link,
                last_timestamp_millis,
            } => unpack_with_key(
                public_key,
                *expires_at_millis,
                link,
                last_timestamp_millis,
                signature,
                body,
            ),
        }
    }
}

/// 一条待验消息的正文。时间和已读列表跟签名绑在一起。
///
/// Body of one message being checked. The time and last-seen list belong with the signature.
pub(crate) struct MessageParts<'a> {
    pub content: &'a str,
    pub time_millis: i64,
    pub salt: i64,
    pub last_seen: Vec<MessageSignatureBytes>,
    pub now_millis: i64,
}

fn unpack_with_key(
    public_key: &RsaPublicKey,
    expires_at_millis: i64,
    link: &mut Option<Link>,
    last_timestamp_millis: &mut i64,
    signature: Option<MessageSignatureBytes>,
    body: MessageParts<'_>,
) -> Result<SignedMessage, ChainFailure> {
    let MessageParts {
        content,
        time_millis,
        salt,
        last_seen,
        now_millis,
    } = body;
    let Some(signature) = signature else {
        return Err(ChainFailure {
            key: MISSING_PROFILE_KEY,
        });
    };
    // 过期是严格早于现在。相等还算有效。官方 `Instant.isBefore`。
    //
    // Expiry is strictly before now. An equal instant is still valid. Official `Instant.isBefore`.
    if expires_at_millis < now_millis {
        return Err(ChainFailure {
            key: EXPIRED_PROFILE_KEY,
        });
    }
    let Some(current) = link.clone() else {
        return Err(ChainFailure { key: CHAIN_BROKEN });
    };
    if time_millis < *last_timestamp_millis {
        *link = None;
        return Err(ChainFailure { key: OUT_OF_ORDER });
    }
    *last_timestamp_millis = time_millis;
    let payload = signature_payload(&current, content, time_millis, salt, &last_seen);
    if !verify_sha256(public_key, &payload, &signature) {
        *link = None;
        return Err(ChainFailure {
            key: INVALID_SIGNATURE,
        });
    }
    if now_millis > time_millis.saturating_add(MESSAGE_EXPIRES_AFTER_MILLIS) {
        tracing::warn!("received an expired chat message; the client and server clocks may differ");
    }
    *link = current.advance();
    Ok(SignedMessage {
        sender: current.sender,
        index: current.index,
        signature: Some(signature),
        content: content.to_owned(),
        time_millis,
        salt,
        last_seen,
    })
}

impl Link {
    fn advance(&self) -> Option<Self> {
        if self.index == i32::MAX {
            None
        } else {
            Some(Self {
                index: self.index + 1,
                sender: self.sender,
                session_id: self.session_id,
            })
        }
    }
}

/// 官方 `PlayerChatMessage.updateSignature` 喂进 SHA256withRSA 的字节。
///
/// Bytes official `PlayerChatMessage.updateSignature` feeds into SHA256withRSA.
pub(crate) fn signature_payload(
    link: &Link,
    content: &str,
    time_millis: i64,
    salt: i64,
    last_seen: &[MessageSignatureBytes],
) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&SIGNATURE_VERSION.to_be_bytes());
    payload.extend_from_slice(link.sender.as_bytes());
    payload.extend_from_slice(link.session_id.as_bytes());
    payload.extend_from_slice(&link.index.to_be_bytes());
    payload.extend_from_slice(&salt.to_be_bytes());
    payload.extend_from_slice(&time_millis.div_euclid(1000).to_be_bytes());
    let content_bytes = content.as_bytes();
    payload.extend_from_slice(&(content_bytes.len() as i32).to_be_bytes());
    payload.extend_from_slice(content_bytes);
    payload.extend_from_slice(&(last_seen.len() as i32).to_be_bytes());
    for signature in last_seen {
        payload.extend_from_slice(signature);
    }
    payload
}

/// 官方 `ProfilePublicKey.Data.signedPayload`：UUID、过期毫秒、公钥 DER，都是大端。
///
/// Official `ProfilePublicKey.Data.signedPayload`: UUID, expiry millis, then the public key DER, all big-endian.
pub(crate) fn profile_key_payload(
    player: Uuid,
    expires_at_millis: i64,
    public_key: &[u8],
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(24 + public_key.len());
    payload.extend_from_slice(player.as_bytes());
    payload.extend_from_slice(&expires_at_millis.to_be_bytes());
    payload.extend_from_slice(public_key);
    payload
}

fn bit(bytes: [u8; 3], index: usize) -> bool {
    bytes[index / 8] & (1 << (index % 8)) != 0
}

/// 官方 `BitSet.length()`。3 字节里第 20 位及更高的位也会算进去。
///
/// Official `BitSet.length()`. Bits 20 and above inside the 3 bytes count too.
fn bitset_length(bytes: &[u8; 3]) -> usize {
    let mut length = 0;
    for index in 0..24 {
        if bit(*bytes, index) {
            length = index + 1;
        }
    }
    length
}

fn checksum_matches(expected: i8, entries: &[MessageSignatureBytes]) -> bool {
    expected == 0 || expected == last_seen_checksum(entries)
}

/// 官方 `LastSeenMessages.computeChecksum`，里面的 `MessageSignature.checksum` 是 `Arrays.hashCode`。
///
/// Official `LastSeenMessages.computeChecksum`. `MessageSignature.checksum` is `Arrays.hashCode`.
pub(crate) fn last_seen_checksum(entries: &[MessageSignatureBytes]) -> i8 {
    let mut checksum: i32 = 1;
    for entry in entries {
        checksum = checksum.wrapping_mul(31).wrapping_add(java_hash(entry));
    }
    let byte = checksum as i8;
    if byte == 0 { 1 } else { byte }
}

fn java_hash(bytes: &[u8]) -> i32 {
    let mut result: i32 = 1;
    for &byte in bytes {
        result = result.wrapping_mul(31).wrapping_add(i32::from(byte as i8));
    }
    result
}

pub(crate) fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signature(byte: u8) -> MessageSignatureBytes {
        [byte; 256]
    }

    fn parts<'a>(
        content: &'a str,
        time_millis: i64,
        salt: i64,
        now_millis: i64,
    ) -> MessageParts<'a> {
        MessageParts {
            content,
            time_millis,
            salt,
            last_seen: Vec::new(),
            now_millis,
        }
    }

    #[test]
    fn an_empty_update_leaves_the_initial_window() {
        let mut seen = LastSeenValidator::default();
        let listed = seen.apply_update(&LastSeenUpdate::EMPTY).unwrap();
        assert!(listed.is_empty());
        assert_eq!(seen.len(), WINDOW);
    }

    #[test]
    fn offset_cannot_pass_the_unfilled_tail() {
        let mut seen = LastSeenValidator::default();
        assert!(seen.apply_offset(1).is_err());
        seen.add_pending(signature(1));
        assert!(seen.apply_offset(1).is_ok());
        assert!(seen.apply_offset(1).is_err());
        assert_eq!(seen.len(), WINDOW);
    }

    #[test]
    fn a_negative_offset_is_rejected() {
        let mut seen = LastSeenValidator::default();
        assert!(seen.apply_offset(-1).is_err());
    }

    #[test]
    fn acknowledging_an_empty_slot_is_rejected() {
        let mut seen = LastSeenValidator::default();
        let update = LastSeenUpdate {
            offset: 0,
            acknowledged: [0x01, 0, 0],
            checksum: 0,
        };
        assert!(seen.apply_update(&update).is_err());
    }

    #[test]
    fn a_pending_message_can_be_ignored_and_then_must_stay_acknowledged() {
        let mut seen = LastSeenValidator::default();
        seen.add_pending(signature(0xab));
        assert!(seen.apply_offset(1).is_ok());
        let ignored = seen.apply_update(&LastSeenUpdate::EMPTY).unwrap();
        assert!(ignored.is_empty());

        seen.add_pending(signature(0xcd));
        assert!(seen.apply_offset(1).is_ok());
        // 偏移 1 只拿掉开头的一个空位，这条签名落在窗口的最后一位（下标 19）。
        //
        // An offset of 1 only drops the first empty slot. The signature lands on the last bit of
        // the window (index 19).
        let checksum = last_seen_checksum(&[signature(0xcd)]);
        let acknowledged = seen
            .apply_update(&LastSeenUpdate {
                offset: 0,
                acknowledged: [0, 0, 0x08],
                checksum,
            })
            .unwrap();
        assert_eq!(acknowledged, vec![signature(0xcd)]);

        let forgotten = LastSeenUpdate {
            offset: 0,
            acknowledged: [0, 0, 0],
            checksum: 1,
        };
        assert!(seen.apply_update(&forgotten).is_err());
    }

    #[test]
    fn checksum_zero_skips_the_check_and_a_wrong_checksum_does_not() {
        let mut seen = LastSeenValidator::default();
        seen.add_pending(signature(0xab));
        seen.apply_offset(1).unwrap();
        let update = LastSeenUpdate {
            offset: 0,
            acknowledged: [0, 0, 0x08],
            checksum: 0,
        };
        assert!(seen.apply_update(&update).is_ok());

        let mut seen = LastSeenValidator::default();
        seen.add_pending(signature(0xab));
        seen.apply_offset(1).unwrap();
        let update = LastSeenUpdate {
            offset: 0,
            acknowledged: [0, 0, 0x08],
            checksum: 2,
        };
        assert!(seen.apply_update(&update).is_err());
    }

    #[test]
    fn bits_past_the_twenty_bit_window_are_rejected() {
        let mut seen = LastSeenValidator::default();
        let update = LastSeenUpdate {
            offset: 0,
            acknowledged: [0, 0, 0x10],
            checksum: 0,
        };
        assert!(seen.apply_update(&update).is_err());
    }

    #[test]
    fn checksum_of_one_ab_signature_matches_arrays_hash_code() {
        assert_eq!(last_seen_checksum(&[signature(0xab)]), 32);
        assert_eq!(last_seen_checksum(&[]), 1);
    }

    #[test]
    fn the_same_pending_signature_is_not_stored_twice() {
        let mut seen = LastSeenValidator::default();
        seen.add_pending(signature(1));
        seen.add_pending(signature(1));
        assert_eq!(seen.len(), WINDOW + 1);
        seen.add_pending(signature(2));
        seen.add_pending(signature(1));
        assert_eq!(seen.len(), WINDOW + 3);
    }

    #[test]
    fn the_cache_reuses_an_id_and_pushes_from_the_back() {
        let mut cache = SignatureCache::default();
        let first = signature(1);
        let second = signature(2);
        cache.push(&[], Some(first));
        cache.push(&[first], Some(second));
        let signatures = [first, second];
        let packed = cache.pack(&signatures);
        assert_eq!(
            packed,
            vec![
                mc_bamboo_protocol::packet::play::PackedSignature::Cached(1),
                mc_bamboo_protocol::packet::play::PackedSignature::Cached(0),
            ]
        );
    }

    #[test]
    fn signature_payload_uses_epoch_seconds_and_version_one() {
        let link = Link {
            index: 3,
            sender: Uuid::from_u128(0x11),
            session_id: Uuid::from_u128(0x22),
        };
        let payload = signature_payload(&link, "hi", 1_500, 9, &[]);
        assert_eq!(&payload[..4], &1_i32.to_be_bytes());
        assert_eq!(&payload[4..20], Uuid::from_u128(0x11).as_bytes());
        assert_eq!(&payload[20..36], Uuid::from_u128(0x22).as_bytes());
        assert_eq!(&payload[36..40], &3_i32.to_be_bytes());
        assert_eq!(&payload[40..48], &9_i64.to_be_bytes());
        assert_eq!(&payload[48..56], &1_i64.to_be_bytes());
        assert_eq!(&payload[56..60], &2_i32.to_be_bytes());
        assert_eq!(&payload[60..62], b"hi");
        assert_eq!(&payload[62..66], &0_i32.to_be_bytes());
    }

    #[test]
    fn a_negative_timestamp_floors_toward_the_previous_second() {
        let link = Link {
            index: 0,
            sender: Uuid::nil(),
            session_id: Uuid::nil(),
        };
        let payload = signature_payload(&link, "", -1, 0, &[]);
        assert_eq!(&payload[48..56], &(-1_i64).to_be_bytes());
    }

    #[test]
    fn chain_rules_follow_the_official_order() {
        let (private_key, public_key) = test_key();
        let sender = Uuid::from_u128(7);
        let session = Uuid::from_u128(8);
        let mut decoder = Decoder::from_key(sender, session, public_key, 10_000);
        let sign = |index: i32, millis: i64, content: &str| {
            let link = Link {
                index,
                sender,
                session_id: session,
            };
            sign_message(&private_key, &link, content, millis, 4, &[])
        };

        let missing = decoder.unpack(true, sender, None, parts("a", 1_000, 4, 1_000));
        assert_eq!(missing.unwrap_err().key, MISSING_PROFILE_KEY);

        let expired = decoder.unpack(
            false,
            sender,
            Some(sign(0, 1_000, "a")),
            parts("a", 1_000, 4, 10_001),
        );
        assert_eq!(expired.unwrap_err().key, EXPIRED_PROFILE_KEY);

        let first = decoder
            .unpack(
                false,
                sender,
                Some(sign(0, 1_000, "a")),
                parts("a", 1_000, 4, 2_000),
            )
            .unwrap();
        assert_eq!(first.index, 0);
        assert!(first.signature.is_some());

        let backwards = decoder.unpack(
            false,
            sender,
            Some(sign(1, 999, "b")),
            parts("b", 999, 4, 2_000),
        );
        assert_eq!(backwards.unwrap_err().key, OUT_OF_ORDER);

        let broken = decoder.unpack(
            false,
            sender,
            Some(sign(1, 1_000, "b")),
            parts("b", 1_000, 4, 2_000),
        );
        assert_eq!(broken.unwrap_err().key, CHAIN_BROKEN);
    }

    #[test]
    fn a_bad_signature_updates_the_timestamp_and_breaks_the_chain() {
        let (private_key, public_key) = test_key();
        let sender = Uuid::from_u128(1);
        let session = Uuid::from_u128(2);
        let mut decoder = Decoder::from_key(sender, session, public_key, 50_000);
        let mut bad = sign_message(
            &private_key,
            &Link {
                index: 0,
                sender,
                session_id: session,
            },
            "a",
            1_000,
            4,
            &[],
        );
        bad[0] ^= 0xff;
        let failed = decoder.unpack(false, sender, Some(bad), parts("a", 1_000, 4, 1_000));
        assert_eq!(failed.unwrap_err().key, INVALID_SIGNATURE);

        let later = sign_message(
            &private_key,
            &Link {
                index: 0,
                sender,
                session_id: session,
            },
            "b",
            2_000,
            4,
            &[],
        );
        let still_broken = decoder.unpack(false, sender, Some(later), parts("b", 2_000, 4, 2_000));
        assert_eq!(still_broken.unwrap_err().key, CHAIN_BROKEN);
    }

    #[test]
    fn the_index_stops_at_integer_max() {
        let (private_key, public_key) = test_key();
        let sender = Uuid::from_u128(3);
        let session = Uuid::from_u128(4);
        let mut decoder = Decoder::Key {
            public_key,
            expires_at_millis: 50_000,
            link: Some(Link {
                index: i32::MAX,
                sender,
                session_id: session,
            }),
            last_timestamp_millis: 0,
        };
        let signature = sign_message(
            &private_key,
            &Link {
                index: i32::MAX,
                sender,
                session_id: session,
            },
            "z",
            5,
            1,
            &[],
        );
        assert!(
            decoder
                .unpack(false, sender, Some(signature), parts("z", 5, 1, 5))
                .is_ok()
        );
        let next = sign_message(
            &private_key,
            &Link {
                index: 0,
                sender,
                session_id: session,
            },
            "z",
            6,
            1,
            &[],
        );
        assert_eq!(
            decoder
                .unpack(false, sender, Some(next), parts("z", 6, 1, 6))
                .unwrap_err()
                .key,
            CHAIN_BROKEN
        );
    }

    #[test]
    fn unsigned_decoder_rejects_only_when_secure_chat_is_enforced() {
        let mut decoder = Decoder::Unsigned;
        assert_eq!(
            decoder
                .unpack(true, Uuid::nil(), None, parts("hi", 0, 0, 0))
                .unwrap_err()
                .key,
            MISSING_PROFILE_KEY
        );
        let accepted = decoder
            .unpack(false, Uuid::from_u128(9), None, parts("hi", 5, 9, 42))
            .unwrap();
        assert!(accepted.signature.is_none());
        assert_eq!(accepted.sender, Uuid::from_u128(9));
        assert_eq!(accepted.index, 0);
        assert_eq!(accepted.time_millis, 42);
        assert_eq!(accepted.salt, 0);
        assert!(accepted.last_seen.is_empty());
    }

    #[test]
    fn a_profile_key_signature_is_sha1_with_rsa() {
        use rsa::pkcs1v15::Pkcs1v15Sign;
        use rsa::pkcs8::EncodePublicKey;
        use sha1::Sha1;
        use sha1::digest::Digest;

        let (private_key, public_key) = test_key();
        let der = public_key
            .to_public_key_der()
            .expect("spki")
            .as_bytes()
            .to_vec();
        let payload = profile_key_payload(Uuid::from_u128(3), 50, &der);
        let hashed = Sha1::digest(&payload);
        let signature = private_key
            .sign(Pkcs1v15Sign::new::<Sha1>(), &hashed)
            .expect("sign profile key");
        let keys = super::super::ServiceProfileKeys::empty();
        assert!(keys.replace(std::slice::from_ref(&der)));
        assert!(keys.verify_profile_signature(&payload, &signature));
        let mut broken = signature;
        broken[0] ^= 1;
        assert!(!keys.verify_profile_signature(&payload, &broken));
    }

    fn test_key() -> (rsa::RsaPrivateKey, RsaPublicKey) {
        use rsa::RsaPrivateKey;
        let mut rng = rand_core::OsRng;
        let private_key = RsaPrivateKey::new(&mut rng, 2048).expect("rsa key");
        let public_key = rsa::RsaPublicKey::from(&private_key);
        (private_key, public_key)
    }

    fn sign_message(
        private_key: &rsa::RsaPrivateKey,
        link: &Link,
        content: &str,
        time_millis: i64,
        salt: i64,
        last_seen: &[MessageSignatureBytes],
    ) -> MessageSignatureBytes {
        use rsa::pkcs1v15::Pkcs1v15Sign;
        use sha2::Sha256;
        use sha2::digest::Digest;

        let payload = signature_payload(link, content, time_millis, salt, last_seen);
        let hashed = Sha256::digest(&payload);
        let signed = private_key
            .sign(Pkcs1v15Sign::new::<Sha256>(), &hashed)
            .expect("sign");
        let mut bytes = [0; 256];
        bytes.copy_from_slice(&signed);
        bytes
    }
}
