//! 玩家公钥、消息链和 `LastSeenMessages`。
//!
//! 对照官方 `ProfilePublicKey`、`SignedMessageChain`、`LastSeenMessagesValidator`、
//! `MessageSignatureCache` 和 `ServerGamePacketListenerImpl` 里处理聊天的那几段。
//! 公钥字节由连接任务事先取好，这里只做校验，不访问网络。
//!
//! Player public keys, the message chain, and `LastSeenMessages`.
//!
//! Follows official `ProfilePublicKey`, `SignedMessageChain`, `LastSeenMessagesValidator`,
//! `MessageSignatureCache`, and the chat handlers in `ServerGamePacketListenerImpl`.
//! The connection task fetches key bytes ahead of time. This module only verifies them.

mod chain;
mod deliver;
mod session;

use std::sync::{Arc, PoisonError, RwLock};

use rsa::{RsaPublicKey, pkcs8::DecodePublicKey};
use sha1::Sha1;
use sha2::Sha256;

pub(crate) use chain::SignedMessage;
pub(crate) use chain::now_millis;
pub(crate) use deliver::{broadcast_signed, send_signed_to};
pub(crate) use session::{
    PlayerChat, SessionUpdate, accept_chat, apply_last_seen, collect_arguments, collect_command,
    on_ack, on_session, take_argument,
};

use crate::region::Region;

/// 连接任务写、世界线程读的 Mojang 公钥。空的时候验不了玩家公钥。
///
/// Mojang public keys. Connection tasks write them and the world thread reads them.
/// An empty set cannot validate player keys.
///
/// 锁中毒时不 panic，继续用里面的值：这把锁只保护一整份 `Vec<RsaPublicKey>`，
/// 写入是把解析好的一批整体赋进去的单个赋值，公钥表没有「改了一半」的中间状态。
/// 就算持锁线程在别处 panic，留下的仍是完整的一批公钥；继续用这批公钥验签，
/// 比让世界线程跟着崩掉安全。
#[derive(Debug, Default)]
pub struct ServiceProfileKeys {
    keys: RwLock<Vec<RsaPublicKey>>,
}

impl ServiceProfileKeys {
    pub fn empty() -> Self {
        Self::default()
    }

    /// 换成这一批 DER。有一把解不出来就不改原来的那批，官方只在这次响应解析成功时才换掉缓存。
    ///
    /// Replaces the set with these DER bytes. One unreadable key leaves the previous set
    /// in place. Official code only swaps the cache when this response parses.
    pub fn replace(&self, ders: &[Vec<u8>]) -> bool {
        // 先解析完再拿锁：解析失败时原来的那批公钥不动。
        let mut parsed = Vec::with_capacity(ders.len());
        for der in ders {
            let Ok(key) = RsaPublicKey::from_public_key_der(der) else {
                return false;
            };
            parsed.push(key);
        }
        *self.keys.write().unwrap_or_else(PoisonError::into_inner) = parsed;
        true
    }

    pub fn can_validate(&self) -> bool {
        !self
            .keys
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    }

    pub(crate) fn verify_profile_signature(&self, payload: &[u8], signature: &[u8]) -> bool {
        self.keys
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|key| verify_pkcs1::<Sha1>(key, payload, signature))
    }
}

/// 世界线程上的一份共享公钥。测试用空的。
///
/// Shared keys for the world thread. Tests start with an empty set.
pub fn empty_keys() -> Arc<ServiceProfileKeys> {
    Arc::new(ServiceProfileKeys::empty())
}

/// `SHA256withRSA`，玩家给聊天签名用的算法。
///
/// `SHA256withRSA`, the algorithm a player uses to sign chat.
pub(crate) fn verify_sha256(key: &RsaPublicKey, payload: &[u8], signature: &[u8]) -> bool {
    verify_pkcs1::<Sha256>(key, payload, signature)
}

fn verify_pkcs1<D>(key: &RsaPublicKey, payload: &[u8], signature: &[u8]) -> bool
where
    D: sha2::digest::Digest + sha2::digest::const_oid::AssociatedOid,
{
    use rsa::pkcs1v15::Pkcs1v15Sign;

    let hashed = D::digest(payload);
    key.verify(Pkcs1v15Sign::new::<D>(), &hashed, signature)
        .is_ok()
}

/// `enforce-secure-profile` 真正生效的条件，官方 `DedicatedServer.enforceSecureProfile`。
///
/// When `enforce-secure-profile` actually applies. Official `DedicatedServer.enforceSecureProfile`.
pub(crate) fn enforces(region: &Region) -> bool {
    region.enforce_secure_profile && region.online_mode && region.service_keys.can_validate()
}

pub(crate) fn has_signable_argument(arguments: &[(String, mc_bamboo_command::Value)]) -> bool {
    arguments
        .iter()
        .any(|(_, value)| value.as_message().is_some())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rsa::{RsaPrivateKey, RsaPublicKey, pkcs1v15::Pkcs1v15Sign, pkcs8::EncodePublicKey};
    use sha1::Sha1;
    use sha1::digest::Digest;
    use uuid::Uuid;

    use super::ServiceProfileKeys;
    use crate::secure_chat::chain::profile_key_payload;

    /// 连接任务在持写锁时 panic 会让锁中毒；世界线程读公钥不能跟着 panic，
    /// 中毒前装好的那批公钥也仍然要能验签，换钥也要能继续成功。
    #[test]
    fn poisoned_keys_are_still_readable_and_replaceable() {
        let mut rng = rand_core::OsRng;
        let private_key = RsaPrivateKey::new(&mut rng, 2048).expect("rsa key");
        let der = RsaPublicKey::from(&private_key)
            .to_public_key_der()
            .expect("der")
            .as_bytes()
            .to_vec();
        let payload = profile_key_payload(Uuid::from_u128(3), 50, &der);
        let hashed = Sha1::digest(&payload);
        let signature = private_key
            .sign(Pkcs1v15Sign::new::<Sha1>(), &hashed)
            .expect("sign");

        let keys = Arc::new(ServiceProfileKeys::empty());
        assert!(keys.replace(std::slice::from_ref(&der)));
        let writer = Arc::clone(&keys);
        let join = std::thread::spawn(move || {
            let _guard = writer.keys.write().unwrap();
            panic!("连接任务在换公钥时崩了");
        });
        assert!(join.join().is_err());
        assert!(keys.can_validate());
        assert!(keys.verify_profile_signature(&payload, &signature));
        assert!(keys.replace(&[]));
        assert!(!keys.can_validate());
    }
}
