//! Login 状态：确认玩家身份。默认是离线模式，也可以信任代理转发的 UUID、名字和皮肤。

use std::sync::Arc;

use mc_bamboo_protocol::{
    frame::append_frame,
    packet::{
        login::{LoginCompression, LoginFinished, LoginRequest, MAX_PLAYER_NAME_CHARS},
        profile::ProfileProperty,
    },
};
use md5::{Digest, Md5};
use uuid::{Builder, Uuid};

use super::{Connection, ConnectionError, forwarding};
use crate::{players::PlayerSession, server::Shared};

/// 这次登录最终采用的身份。
struct Profile {
    uuid: Uuid,
    name: String,
    properties: Vec<ProfileProperty>,
}

/// 读取玩家名并把玩家加入玩家表。同名玩家在线时要等它离开。玩家被拒绝或中途离开时返回 `None`。
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

    let profile = match identity {
        forwarding::Identity::Offline => Profile {
            uuid: offline_uuid(name),
            name: name.to_owned(),
            properties: Vec::new(),
        },
        // legacy 只转发 UUID 和属性，名字还是握手之后客户端自己报的。
        forwarding::Identity::Legacy(legacy) => Profile {
            uuid: legacy.uuid,
            name: name.to_owned(),
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

    match PlayerSession::join(shared, &profile.name, profile.uuid, profile.properties).await {
        Ok(session) => Ok(Some(session)),
        Err(refusal) => {
            connection.refuse_login(refusal.message()).await?;
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
        LoginRequest::Hello { .. } | LoginRequest::PluginResponse { .. } => {
            Err(ConnectionError::UnexpectedPacket)
        }
    }
}

/// 和官方相同的规则：不超过 16 个字符，每个字符都是可见的 ASCII 字符。
fn is_valid_player_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_PLAYER_NAME_CHARS
        && name.bytes().all(|byte| (33..127).contains(&byte))
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
