//! 世界线程和连接任务之间的约定。

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use mc_bamboo_protocol::packet::{
    play::{ClientInformation, PlayRequest},
    profile::ProfileProperty,
};
use uuid::Uuid;

use crate::outgoing::Outgoing;

/// 一次登录对应的玩家 ID，同时也是这名玩家的实体 ID。
///
/// 每次登录都分配新值，不用 UUID：同一账号快速重连时，
/// 旧连接的 [`Inbound::Leave`] 不会误删新连接的玩家。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerId(pub i32);

/// 世界线程发包的出口，连接层负责实现。两个方法都不能阻塞。
pub trait PlayerSink: Send {
    /// 交出一批完整的帧。积压太多或者连接已经结束时返回 `false`。
    fn send(&mut self, frames: Outgoing) -> bool;

    /// 已经交出的帧都写完之后关闭连接。
    fn close(&mut self);
}

/// 一名玩家进入世界需要的全部信息。
pub struct JoinRequest {
    pub id: PlayerId,
    pub uuid: Uuid,
    pub name: String,
    /// session server 或者代理给出的皮肤等属性。离线模式没有属性。
    pub properties: Vec<ProfileProperty>,
    /// Configuration 阶段收到的客户端设置，没收到时是默认值。
    pub client: ClientInformation,
    /// `ops.json` 里的权限等级，0 到 4。不在表里就是 0。
    ///
    /// Permission level from `ops.json`, 0 through 4. Missing players are 0.
    pub permission_level: u8,
    /// 是否列在 `ops.json` 里。等级 0 的 OP 也算 OP，刷屏豁免按这个判断。
    /// 进服时世界线程会按自己的权威副本重算这两个值。
    ///
    /// Whether the player is listed in `ops.json`. A level-0 op still counts for the spam
    /// exemption. The world thread recomputes both fields from its authoritative copy on join.
    pub is_op: bool,
    /// 连接层的对端 IP 文本（不含端口），供世界线程复查 IP 封禁。
    pub peer_ip: String,
    /// 连接层判断满员时那一刻的在线人数，供世界线程按权威名单复查。
    pub online_at_login: usize,
    /// 连接层从 `players/data/<uuid>.dat` 读到的原始 gzip 字节。
    /// 没有存档或没启用存档时为 `None`，世界线程按新玩家处理。
    ///
    /// Raw gzip bytes the connection layer read from `players/data/<uuid>.dat`.
    /// `None` means a new player (no save, or saving disabled).
    pub player_data: Option<Vec<u8>>,
    pub sink: Box<dyn PlayerSink>,
}

/// 连接任务发给世界线程的消息。
///
/// 所有连接共用一条队列，所以同一连接的消息不会乱序，同一账号的
/// [`Leave`](Inbound::Leave) 也一定排在下一次 [`Join`](Inbound::Join) 前面。
pub enum Inbound {
    Join(Box<JoinRequest>),
    Request {
        player: PlayerId,
        request: PlayRequest,
        /// 世界线程处理完这条消息后归还的配额。
        in_flight: InFlight,
    },
    /// 心跳算出的新延迟，单位是毫秒。
    Latency {
        player: PlayerId,
        millis: i32,
    },
    /// 连接结束了。不占配额，所以连接无论怎么结束都能送达。
    Leave {
        player: PlayerId,
    },
    /// `/whitelist reload` 从连接层读回来的白名单。
    WhitelistReload(crate::admin::WhitelistReload),
    /// 踢出所有玩家并结束世界线程。
    Shutdown,
}

/// 一条排队中的请求占用的配额，销毁时自动归还。
///
/// 连接任务靠计数限制积压：世界线程卡住或者客户端发包太快时断开连接，
/// 而不是停止读 socket，否则连接任务就读不到心跳回应了。
pub struct InFlight(Arc<AtomicUsize>);

impl InFlight {
    /// 占用一个配额。计数已经达到 `limit` 时返回 `None`。
    pub fn acquire(counter: &Arc<AtomicUsize>, limit: usize) -> Option<Self> {
        if counter.fetch_add(1, Ordering::Relaxed) < limit {
            Some(Self(Arc::clone(counter)))
        } else {
            counter.fetch_sub(1, Ordering::Relaxed);
            None
        }
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_flight_quota_is_returned_on_drop() {
        let counter = Arc::new(AtomicUsize::new(0));
        let first = InFlight::acquire(&counter, 2);
        let second = InFlight::acquire(&counter, 2);

        assert!(first.is_some() && second.is_some());
        assert!(InFlight::acquire(&counter, 2).is_none());
        drop(first);
        assert!(InFlight::acquire(&counter, 2).is_some());
    }
}
