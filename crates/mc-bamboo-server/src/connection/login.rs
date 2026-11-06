//! Login 状态：确认玩家身份。目前只支持离线模式，不做正版验证和加密。

use std::sync::Arc;

use mc_bamboo_protocol::{
    frame::append_frame,
    packet::login::{LoginFinished, LoginRequest, MAX_PLAYER_NAME_CHARS},
};
use md5::{Digest, Md5};
use uuid::{Builder, Uuid};

use super::{Connection, ConnectionError};
use crate::{players::PlayerSession, server::Shared};

/// 完成登录。玩家被拒绝或中途离开时返回 `None`。
pub(super) async fn serve(
    connection: &mut Connection,
    shared: &Arc<Shared>,
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

    let uuid = offline_uuid(name);
    let session = match PlayerSession::join(shared, name, uuid) {
        Ok(session) => session,
        Err(refusal) => {
            connection.refuse_login(refusal.message()).await?;
            return Ok(None);
        }
    };

    let mut frame = Vec::new();
    append_frame(
        &mut frame,
        &LoginFinished {
            uuid,
            name,
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
            Ok(Some(session))
        }
        LoginRequest::Hello { .. } => Err(ConnectionError::UnexpectedPacket),
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
