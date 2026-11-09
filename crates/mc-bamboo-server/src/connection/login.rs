//! Login 状态：确认玩家身份。
//!
//! 三条路：离线模式只读一个玩家名，UUID 由玩家名算出来；代理转发信任代理给的 UUID、
//! 名字和皮肤；打开正版验证时走官方 `ServerLoginPacketListenerImpl.handleHello` /
//! `handleKey` 的全套：公布公钥、校验 challenge、启用加密，再去问 session server。
//!
//! 正版验证和代理转发互斥，[`ServerConfig::normalized`](crate::ServerConfig) 保证了这一点。

use std::sync::Arc;

use mc_bamboo_auth::{
    AuthError, Profile, ServerKeyPair, Timeouts, has_joined, random_challenge, server_id_hash,
};
use mc_bamboo_protocol::{
    frame::append_frame,
    packet::login::{
        LoginCompression, LoginFinished, LoginHello, LoginRequest, MAX_PLAYER_NAME_CHARS,
    },
};
use md5::{Digest, Md5};
use tracing::{error, info, warn};
use uuid::{Builder, Uuid};

use super::{Connection, ConnectionError, forwarding};
use crate::{players::PlayerSession, server::Shared};

/// 官方 `ServerLoginPacketListenerImpl.serverId` 一直是空串，哈希里也用它。
const SERVER_ID: &str = "";
/// session server 不认这次登录时客户端看到的话。
const UNVERIFIED_USERNAME: &str = "multiplayer.disconnect.unverified_username";
/// 问不到 session server 时客户端看到的话。
const AUTHSERVERS_DOWN: &str = "multiplayer.disconnect.authservers_down";
/// 官方 `ServerLoginPacketListenerImpl.tick` 在 600 刻（30 秒）时说的那句话。
pub(super) const SLOW_LOGIN: &str = "multiplayer.disconnect.slow_login";

/// 确认玩家身份并把玩家加入玩家表。同名玩家在线时要等它离开。
/// 玩家被拒绝或中途离开时返回 `None`。
pub(super) async fn admit(
    connection: &mut Connection,
    shared: &Arc<Shared>,
    identity: forwarding::Identity,
) -> Result<Option<PlayerSession>, ConnectionError> {
    let Some(payload) = connection.next_frame().await? else {
        return Ok(None);
    };
    let LoginRequest::Hello { name, .. } = LoginRequest::decode(&payload)? else {
        return Err(ConnectionError::UnexpectedPacket);
    };
    if !is_valid_player_name(name) {
        return Err(ConnectionError::InvalidPlayerName);
    }
    // 后面还要再读包，名字不能继续借着这一帧。
    let name = name.to_owned();

    let profile = match identity {
        // 没有转发时才可能开着正版验证，这时服务端手里有密钥对。
        forwarding::Identity::Offline => match &shared.keypair {
            Some(keypair) => match authenticate(connection, shared, keypair, name).await? {
                Some(profile) => profile,
                None => return Ok(None),
            },
            None => offline_profile(name),
        },
        // legacy 只转发 UUID 和属性，名字还是握手之后客户端自己报的。
        forwarding::Identity::Legacy(legacy) => Profile {
            uuid: legacy.uuid,
            name,
            properties: legacy.properties,
        },
        forwarding::Identity::Modern => {
            let Some(profile) = forwarding::authenticate(connection, shared).await? else {
                return Ok(None);
            };
            // 和官方一样，代理给的名字直接采用，不再按本地玩家名规则检查。
            Profile {
                uuid: profile.uuid,
                name: profile.name,
                properties: profile.properties,
            }
        }
    };

    // 官方 `ServerLoginPacketListenerImpl.verifyLoginAndFinishConnectionSetup`
    // 在身份确认之后、进服之前查封禁和白名单。
    let ip = connection.peer_ip.clone();
    let refusal = {
        let snapshot = shared.admin.read().expect("管理名单的锁不应该中毒");
        snapshot.login_refusal(profile.uuid, &ip, mc_bamboo_game::admin::now_millis())
    };
    if let Some(refusal) = refusal {
        connection.refuse_login_component(&refusal.text()).await?;
        return Ok(None);
    }

    match PlayerSession::join(shared, profile).await {
        Ok(session) => Ok(Some(session)),
        Err(refusal) => {
            connection.refuse_login(refusal.message()).await?;
            Ok(None)
        }
    }
}

/// 官方 `handleKey` 加上它起的那条验证线程：加密连接，然后问 session server。
///
/// 这个请求跑在连接任务里，世界线程碰不到它，所以一个玩家等 Mojang 回话不会卡住别人。
async fn authenticate(
    connection: &mut Connection,
    shared: &Arc<Shared>,
    keypair: &ServerKeyPair,
    name: String,
) -> Result<Option<Profile>, ConnectionError> {
    // 公钥和这次登录无关，但要在玩家进入世界之前取到，`login` 包才能写对
    // `enforcesSecureChat`。世界线程不去访问网络。
    //
    // The profile keys are not part of this login, but they have to be ready before the player
    // enters the world so the `login` packet can set `enforcesSecureChat`. The world thread does
    // not touch the network.
    crate::server::ensure_profile_keys(shared).await;
    let challenge = random_challenge();
    let mut frame = Vec::new();
    append_frame(
        &mut frame,
        &LoginHello {
            server_id: SERVER_ID,
            public_key: keypair.public_key_der(),
            challenge: &challenge,
            // 走到这里就说明服务端要去问 Mojang，官方这一位也是写死的 true。
            should_authenticate: true,
        },
    )?;
    connection.write(&frame).await?;

    let Some(payload) = connection.next_frame().await? else {
        return Err(ConnectionError::ClosedBeforeJoining);
    };
    let LoginRequest::Key {
        shared_secret,
        challenge: answer,
    } = LoginRequest::decode(&payload)?
    else {
        return Err(ConnectionError::UnexpectedPacket);
    };

    // 官方的顺序：先验 challenge，再解出密钥。
    if keypair.decrypt(answer).ok().as_deref() != Some(&challenge[..]) {
        return Err(ConnectionError::ChallengeMismatch);
    }
    let secret = keypair.decrypt_shared_secret(shared_secret)?;
    let digest = server_id_hash(SERVER_ID, &secret, keypair.public_key_der());
    // 官方在收到 key 包时就切换加密，所以后面的包，包括下面的断开原因，都是加密的。
    connection.enable_encryption(&secret);

    // 开了 prevent-proxy-connections 才把客户端的地址交给 Mojang 比对。
    let ip = shared
        .config
        .prevent_proxy_connections
        .then(|| connection.peer_ip())
        .flatten();
    let url = &shared.config.session_server_url;
    let timeouts = Timeouts {
        connect: shared.config.auth_connect_timeout,
        read: shared.config.auth_read_timeout,
    };
    match has_joined(url, &name, &digest, ip, timeouts).await {
        Ok(profile) => {
            info!("UUID of player {} is {}", profile.name, profile.uuid);
            Ok(Some(profile))
        }
        Err(AuthError::Unverified(_)) => {
            error!("Username '{name}' tried to join with an invalid session");
            connection
                .refuse_login_translated(UNVERIFIED_USERNAME)
                .await?;
            Ok(None)
        }
        Err(AuthError::Unavailable(error)) => {
            warn!(%error, "couldn't verify username because servers are unavailable");
            connection.refuse_login_translated(AUTHSERVERS_DOWN).await?;
            Ok(None)
        }
    }
}

/// 先告诉客户端压缩阈值，再告诉它登录成功并等它确认。
///
/// 和官方一样，压缩包写出去之后才切换编码，所以压缩包本身是未压缩的；
/// 之后的登录成功包开始就带压缩层。
pub(super) async fn finish(
    connection: &mut Connection,
    session: &PlayerSession,
    shared: &Shared,
) -> Result<(), ConnectionError> {
    let threshold = shared.config.compression_threshold;
    if threshold >= 0 {
        let mut frame = Vec::new();
        append_frame(&mut frame, &LoginCompression { threshold })?;
        connection.write(&frame).await?;
        connection.enable_compression(threshold);
    }

    let mut frame = Vec::new();
    append_frame(
        &mut frame,
        &LoginFinished {
            uuid: session.uuid,
            name: &session.name,
            properties: &session.properties,
            session_id: shared.session_id,
        },
    )?;
    connection.write(&frame).await?;

    let Some(payload) = connection.next_frame().await? else {
        return Err(ConnectionError::ClosedBeforeJoining);
    };
    match LoginRequest::decode(&payload)? {
        LoginRequest::Acknowledged => {
            connection.allow_full_size_frames();
            Ok(())
        }
        LoginRequest::Hello { .. }
        | LoginRequest::Key { .. }
        | LoginRequest::PluginResponse { .. } => Err(ConnectionError::UnexpectedPacket),
    }
}

/// 和官方相同的规则：不超过 16 个字符，每个字符都是可见的 ASCII 字符。
fn is_valid_player_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_PLAYER_NAME_CHARS
        && name.bytes().all(|byte| (33..127).contains(&byte))
}

/// 离线模式的身份，对应官方 `UUIDUtil.createOfflineProfile`：UUID 由玩家名算出，没有皮肤。
fn offline_profile(name: String) -> Profile {
    Profile {
        uuid: offline_uuid(&name),
        name,
        properties: Vec::new(),
    }
}

/// 离线模式的 UUID，由玩家名算出，和官方的算法相同。
fn offline_uuid(name: &str) -> Uuid {
    let digest = Md5::digest(format!("OfflinePlayer:{name}"));
    Builder::from_md5_bytes(digest.into()).into_uuid()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_the_game() {
        assert_eq!(
            offline_uuid("Notch").to_string(),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
    }

    #[test]
    fn checks_player_names_like_the_game() {
        assert!(is_valid_player_name("Kitten_01"));
        assert!(is_valid_player_name("a.b-c!"));

        assert!(!is_valid_player_name(""));
        assert!(!is_valid_player_name("has space"));
        assert!(!is_valid_player_name("竹子"));
        assert!(!is_valid_player_name("seventeen_chars_x"));
    }
}
