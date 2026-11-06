//! 在线玩家表。

use std::sync::{
    Arc,
    atomic::{AtomicI32, Ordering},
};

use tracing::info;
use uuid::Uuid;

use crate::{
    event::{PlayerInfo, ServerEvent},
    server::Shared,
};

/// 玩家进不了服务器的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JoinRefusal {
    ServerFull,
    AlreadyOnline,
}

impl JoinRefusal {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::ServerFull => "服务器已满",
            Self::AlreadyOnline => "这个名字已经在服务器里了",
        }
    }
}

/// 下一个可用的实体 ID。
static NEXT_ENTITY_ID: AtomicI32 = AtomicI32::new(1);

/// 一名玩家的在线状态。销毁时自动从玩家表移除，所以连接无论怎么结束都不会留下幽灵玩家。
pub(crate) struct PlayerSession {
    shared: Arc<Shared>,
    pub(crate) name: String,
    pub(crate) uuid: Uuid,
    pub(crate) entity_id: i32,
}

impl PlayerSession {
    /// 把玩家加入玩家表。
    pub(crate) fn join(shared: &Arc<Shared>, name: &str, uuid: Uuid) -> Result<Self, JoinRefusal> {
        let info = PlayerInfo {
            name: name.to_owned(),
            uuid: uuid.to_string(),
        };
        shared.add_player(uuid, info.clone())?;
        shared.publish(ServerEvent::PlayerJoined(info));
        info!("{name} joined the game");

        Ok(Self {
            shared: Arc::clone(shared),
            name: name.to_owned(),
            uuid,
            entity_id: NEXT_ENTITY_ID.fetch_add(1, Ordering::Relaxed),
        })
    }
}

impl Drop for PlayerSession {
    fn drop(&mut self) {
        if let Some(info) = self.shared.remove_player(self.uuid) {
            self.shared.publish(ServerEvent::PlayerLeft(info));
            info!("{} left the game", self.name);
        }
    }
}
