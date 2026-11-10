//! 在线玩家表：登录的准入表，不是游戏状态。

use std::sync::{
    Arc,
    atomic::{AtomicI32, Ordering},
};

use mc_bamboo_auth::Profile;
use mc_bamboo_protocol::packet::profile::ProfileProperty;
use tokio_util::sync::{CancellationToken, WaitForCancellationFuture};
use tracing::info;
use uuid::Uuid;

use crate::{
    event::{PlayerInfo, ServerEvent},
    server::Shared,
};

/// 玩家进不了服务器的原因。Login 阶段的断开原因是 JSON 文本组件，
/// 这里和官方一样用翻译键，让客户端按自己的语言显示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JoinRefusal {
    ServerFull,
}

impl JoinRefusal {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::ServerFull => mc_bamboo_game::admin::LoginRefusal::ServerFull.text().key,
        }
    }
}

/// 下一个可用的实体 ID。
static NEXT_ENTITY_ID: AtomicI32 = AtomicI32::new(1);

/// 玩家表和这名玩家的连接之间的两个信号。
#[derive(Clone, Default)]
pub(crate) struct SessionSignals {
    /// 取消它，连接就会结束。
    kick: CancellationToken,
    /// 这名玩家离开玩家表时取消。
    gone: CancellationToken,
}

/// 玩家表里的一项。
#[derive(Clone)]
pub(crate) struct OnlinePlayer {
    pub(crate) info: PlayerInfo,
    signals: SessionSignals,
    /// 官方 `ServerOpList.canBypassPlayerLimit`：满员时只有这个位为真的 OP 能进。
    pub(crate) bypasses_player_limit: bool,
}

/// 玩家加不进玩家表的原因。
pub(crate) enum AddPlayerError {
    Full,
    /// 同名玩家还在线，这是它的信号。
    Duplicate(SessionSignals),
}

impl OnlinePlayer {
    pub(crate) fn signals(&self) -> SessionSignals {
        self.signals.clone()
    }
}

/// 一名玩家的在线状态。销毁时自动从玩家表移除，所以连接无论怎么结束都不会留下幽灵玩家。
pub(crate) struct PlayerSession {
    shared: Arc<Shared>,
    signals: SessionSignals,
    pub(crate) name: String,
    pub(crate) uuid: Uuid,
    /// session server 或者代理给出的皮肤等属性。离线模式没有属性。
    pub(crate) properties: Vec<ProfileProperty>,
    pub(crate) entity_id: i32,
    /// 登录时玩家表里的人数，世界线程按权威名单复查满员用。
    pub(crate) online_at_login: usize,
}

impl PlayerSession {
    /// 把玩家加入玩家表。`profile` 是正版验证或者代理转发的结果，离线模式下由玩家名现编。
    ///
    /// 同名玩家在线时和官方一样踢掉旧连接，等它离开之后再进入。断网时旧连接要等心跳超时才会结束，
    /// 这段时间拒绝重连的话玩家就进不来了。这里没有自己的时限，调用方的 `pre_play_timeout` 兜底。
    pub(crate) async fn join(shared: &Arc<Shared>, profile: Profile) -> Result<Self, JoinRefusal> {
        let Profile {
            uuid,
            name,
            properties,
        } = profile;
        // 名单每次变更都会发布新快照，`/op`、`/deop` 和文件重载都即时生效。
        let bypasses_player_limit = shared
            .admin
            .read()
            .expect("管理名单的锁不应该中毒")
            .bypasses_player_limit(uuid);
        let player = OnlinePlayer {
            info: PlayerInfo {
                name: name.clone(),
                uuid: uuid.to_string(),
            },
            signals: SessionSignals::default(),
            bypasses_player_limit,
        };
        loop {
            match shared.add_player(uuid, &player) {
                Ok(()) => break,
                Err(AddPlayerError::Full) => return Err(JoinRefusal::ServerFull),
                Err(AddPlayerError::Duplicate(old)) => {
                    old.kick.cancel();
                    old.gone.cancelled().await;
                }
            }
        }
        shared.publish(ServerEvent::PlayerJoined(player.info.clone()));
        info!("{name} joined the game");

        Ok(Self {
            shared: Arc::clone(shared),
            signals: player.signals(),
            name,
            uuid,
            properties,
            entity_id: NEXT_ENTITY_ID.fetch_add(1, Ordering::Relaxed),
            online_at_login: shared.online_count().saturating_sub(1),
        })
    }

    /// 同名玩家登录时完成，这个连接要让位。
    pub(crate) fn kicked(&self) -> WaitForCancellationFuture<'_> {
        self.signals.kick.cancelled()
    }
}

impl Drop for PlayerSession {
    fn drop(&mut self) {
        if let Some(player) = self.shared.remove_player(self.uuid) {
            self.shared.publish(ServerEvent::PlayerLeft(player.info));
            info!("{} left the game", self.name);
        }
        self.signals.gone.cancel();
    }
}
