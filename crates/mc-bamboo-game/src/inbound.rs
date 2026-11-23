//! 世界线程和连接任务之间的约定。

use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicU64, AtomicUsize, Ordering},
    mpsc::{self, Receiver, Sender, TryRecvError},
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

    /// 指向同一条连接的第二个出口。世界平时只经玩家所在的岛写他的出口；岛的 tick 线程
    /// 卡死、岛被判死时（`docs/island-implementation-plan.md` 5.6），主控够不着那个出口，
    /// 靠这一个发断开包并关闭连接。之后原来那个出口再写的帧必须被丢掉（连接已经结束）。
    /// 给不出第二个出口时返回 `None`：那样的玩家在他的岛被判死时不会被主动断开。
    ///
    /// A second exit to the same connection. The world normally writes a
    /// player's sink only through the island they are on; when that island's
    /// tick thread is stuck and the island is declared dead
    /// (`docs/island-implementation-plan.md` 5.6) the master cannot reach it
    /// and uses this one to send the disconnect and close. Frames the first
    /// sink writes afterwards must be dropped (the connection has ended).
    /// `None` when no second exit exists; such a player is not disconnected
    /// actively when their island dies.
    fn second_handle(&self) -> Option<Box<dyn PlayerSink>> {
        None
    }
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
    /// 连接层从 `players/advancements/<uuid>.json` 读到的原文。
    pub player_advancements: Option<String>,
    pub sink: Box<dyn PlayerSink>,
    /// 这名玩家自己的入站队列的收端，进服后挂在玩家上。发端由连接任务留着。
    ///
    /// The receiving end of this player's own inbound queue; it moves onto the
    /// player on join. The connection task keeps the sending end.
    pub inbox: PlayerInbox,
}

/// 连接任务发给世界线程的消息。
///
/// 每条消息发出时盖一个递增的序号（[`Sequenced`]），世界线程按序号处理。
/// 一名玩家的 [`Request`](Inbound::Request) 和 [`Leave`](Inbound::Leave) 走他自己的队列
/// （[`PlayerSender`] → [`PlayerInbox`]，收端挂在世界里的玩家上），其余消息走主控队列
/// （[`InboundSender`] → [`InboundReceiver`]）。同一连接的消息序号递增、不会乱序；
/// 同一账号的 `Leave` 发出之后下一次 [`Join`](Inbound::Join) 才会发出，所以序号也更小。
///
/// Messages from the connection tasks to the world thread. Each is stamped with
/// an increasing sequence number when sent ([`Sequenced`]) and the world handles
/// them in that order. A player's `Request` and `Leave` go through that player's
/// own queue (its receiving end lives on the player in the world); everything
/// else goes through the master queue. One connection's messages have increasing
/// numbers, and an account's `Leave` is sent before its next `Join`, so it also
/// has the smaller number.
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
    /// 控制台输入线程从 stdin 读到的一行命令，世界线程按服务器控制台来源执行。
    ///
    /// A console line read from stdin by the input thread, executed by the world thread
    /// with the server-console source.
    ConsoleCommand(String),
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

/// 盖了序号的一条入站消息。序号在一个世界的所有发端之间共享、严格递增，
/// 世界线程按它把主控队列和各玩家的队列归并成一条（岛的实施计划第 6 节第 4 步）。
///
/// One stamped inbound message. The number is shared by every sender of one
/// world and strictly increasing; the world thread merges the master queue and
/// every player's queue by it (island plan, section 6, step 4).
pub struct Sequenced {
    pub seq: u64,
    pub message: Inbound,
}

/// 新建主控队列。返回的发端可以克隆，也能给每名玩家开一条自己的队列
/// （[`InboundSender::player_channel`]），它们共用同一个序号计数器。
///
/// Create the master queue. The sender can be cloned and can open a queue per
/// player ([`InboundSender::player_channel`]); all of them share one counter.
pub fn channel() -> (InboundSender, InboundReceiver) {
    let (sender, receiver) = mpsc::channel();
    let sender = InboundSender {
        stamp: Stamp(Arc::new(AtomicU64::new(0))),
        sender,
    };
    (sender, InboundReceiver(receiver))
}

/// 收端已经不在了（世界线程结束，或者世界已经不收这名玩家的消息）。
///
/// The receiving end is gone (the world thread ended, or the world no longer
/// takes this player's messages).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed;

/// 序号计数器。一次原子加；同一发端连发的消息序号递增。
#[derive(Clone)]
struct Stamp(Arc<AtomicU64>);

impl Stamp {
    fn seal(&self, message: Inbound) -> Sequenced {
        Sequenced {
            seq: self.0.fetch_add(1, Ordering::Relaxed),
            message,
        }
    }
}

/// 主控队列的发端：进服、延迟、名单重载、控制台命令、关服都走这里。
/// 玩家的请求和离开也可以走这里（测试和没有玩家队列的场合），世界线程一样按序号处理。
///
/// The master queue's sender: join, latency, list reloads, console commands and
/// shutdown. A player's requests and leave may also come this way (tests, or
/// places without a player queue); the world handles them by number all the same.
#[derive(Clone)]
pub struct InboundSender {
    stamp: Stamp,
    sender: Sender<Sequenced>,
}

impl InboundSender {
    /// 盖上序号发出。世界线程已经结束时失败。
    pub fn send(&self, message: Inbound) -> Result<(), Closed> {
        self.sender
            .send(self.stamp.seal(message))
            .map_err(|_| Closed)
    }

    /// 给一名玩家开一条自己的队列：发端留给连接任务，收端放进 [`JoinRequest::inbox`]。
    ///
    /// Open a queue for one player: the connection task keeps the sender and the
    /// receiver goes into [`JoinRequest::inbox`].
    pub fn player_channel(&self) -> (PlayerSender, PlayerInbox) {
        let (sender, receiver) = mpsc::channel();
        (
            PlayerSender {
                stamp: self.stamp.clone(),
                sender,
            },
            PlayerInbox(Mutex::new(receiver)),
        )
    }
}

/// 主控队列的收端，世界线程持有。
///
/// The master queue's receiver, held by the world thread.
pub struct InboundReceiver(Receiver<Sequenced>);

/// 按序号阻塞地逐条取，所有发端都没了时结束。给替换世界线程的测试用。
///
/// Blocking iteration in order until every sender is gone; for tests that
/// replace the world thread.
impl IntoIterator for InboundReceiver {
    type Item = Inbound;
    type IntoIter = std::iter::Map<mpsc::IntoIter<Sequenced>, fn(Sequenced) -> Inbound>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter().map(|sealed| sealed.message)
    }
}

impl InboundReceiver {
    /// 阻塞等下一条。所有发端都没了时返回 `None`。
    pub fn recv(&self) -> Option<Sequenced> {
        self.0.recv().ok()
    }

    /// 不等待地取一条，没有就返回 `None`。
    pub fn try_recv(&self) -> Option<Sequenced> {
        self.0.try_recv().ok()
    }

    /// 取出已经到达的全部消息，追加到 `out`。返回发端是不是已经全没了。
    pub(crate) fn drain_into(&self, out: &mut Vec<Sequenced>) -> bool {
        loop {
            match self.0.try_recv() {
                Ok(message) => out.push(message),
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => return true,
            }
        }
    }
}

/// 一名玩家自己的队列的发端，连接任务持有，只发这名玩家的
/// [`Request`](Inbound::Request) 和 [`Leave`](Inbound::Leave)。
///
/// The sender of one player's own queue, held by the connection task. It only
/// carries that player's `Request` and `Leave`. Cloning gives another handle
/// to the same queue; the shared stamp keeps their sequence numbers ordered.
#[derive(Clone)]
pub struct PlayerSender {
    stamp: Stamp,
    sender: Sender<Sequenced>,
}

impl PlayerSender {
    /// 盖上序号发出。世界已经不收这名玩家的消息时失败。
    pub fn send(&self, message: Inbound) -> Result<(), Closed> {
        debug_assert!(matches!(
            message,
            Inbound::Request { .. } | Inbound::Leave { .. }
        ));
        self.sender
            .send(self.stamp.seal(message))
            .map_err(|_| Closed)
    }
}

/// 一名玩家自己的队列的收端。进服后挂在世界里的玩家上，跟着玩家走。
/// 包一层锁只是为了让持有它的类型能在线程之间共享引用（`mpsc::Receiver` 不是
/// `Sync`）；只有世界线程取，锁永远没有争用。
///
/// The receiver of one player's own queue. After join it lives on the player
/// in the world and moves with them. The lock only makes types holding it
/// shareable across threads (`mpsc::Receiver` is not `Sync`); only the world
/// thread drains it, so the lock is never contended.
pub struct PlayerInbox(Mutex<Receiver<Sequenced>>);

impl PlayerInbox {
    /// 取出已经到达的全部消息，追加到 `out`。
    pub(crate) fn drain_into(&self, out: &mut Vec<Sequenced>) {
        let receiver = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        while let Ok(message) = receiver.try_recv() {
            out.push(message);
        }
    }
}

impl Default for PlayerInbox {
    /// 一条没有发端的空队列，给不经过连接层建出来的玩家用。
    fn default() -> Self {
        Self(Mutex::new(mpsc::channel().1))
    }
}

/// 按序号把这一刻取到的消息归并成一条。每条队列本身已经按序号递增，
/// 序号全局唯一，所以结果和「所有消息走一条通道」的到达顺序相同。
///
/// Merge this tick's messages by number. Each queue is already increasing and
/// numbers are unique, so the result is the arrival order of a single channel.
pub(crate) fn merge(batch: &mut [Sequenced]) {
    batch.sort_unstable_by_key(|message| message.seq);
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
