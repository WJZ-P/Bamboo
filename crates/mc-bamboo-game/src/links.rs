//! 每名玩家的出站链路。
//!
//! 一 tick 里写给同一名玩家的包都攒在它的缓冲区里，tick 末尾交给连接层一次。
//! 世界线程只和缓冲区打交道，不会等 socket。

use std::{collections::BTreeMap, mem, sync::Arc};

use mc_bamboo_protocol::{
    frame::append_frame,
    packet::{ClientboundPacket, play::Disconnect},
    text::Component,
};
use tracing::warn;

use crate::{
    inbound::{PlayerId, PlayerSink},
    outgoing::{Outgoing, SharedFrames},
};

/// 包太大编码不出来时踢人用的原因。客户端可以靠超长的物品组件走到这里。
pub(crate) const PACKET_ERROR: &str = "disconnect.packetError";

/// 把一个要发给多人的包编码成帧。这种包的内容都不是客户端能撑大的，
/// 真的放不下时只丢掉这个包，不影响任何人的连接。
pub(crate) fn encode_frames<P: ClientboundPacket>(packet: &P) -> Vec<u8> {
    let mut frames = Vec::new();
    if let Err(error) = append_frame(&mut frames, packet) {
        warn!(%error, "dropping a packet that cannot be encoded");
    }
    frames
}

struct Link {
    sink: Box<dyn PlayerSink>,
    buffer: Outgoing,
    /// 关闭之后所有写入都丢弃。
    open: bool,
}

impl Link {
    /// 返回交给连接层的未压缩字节数；拒收时不算进去。
    fn flush(&mut self) -> u64 {
        if self.buffer.is_empty() {
            return 0;
        }
        let bytes = self.buffer.len_bytes() as u64;
        if !self.sink.send(mem::take(&mut self.buffer)) {
            self.close();
            return 0;
        }
        bytes
    }

    fn close(&mut self) {
        self.open = false;
        self.buffer = Outgoing::default();
        self.sink.close();
    }
}

/// 主控投给这个岛的一个出站包（岛的实施计划第 2 节第 28 行：只有玩家所在的岛写他的
/// 缓冲，主控产生的帧经岛的信箱）。
///
/// One outbound packet the master posts to this island (island plan, section 2,
/// row 28: only a player's own island writes their buffer; master frames go
/// through the island's mailbox).
enum FrameMail {
    To(PlayerId, Vec<u8>),
    Shared(PlayerId, Arc<SharedFrames>),
    All(Vec<u8>),
    Kick(PlayerId, Component),
}

/// 在一个岛上执行的一小段投递（系统消息逐个看接收者的聊天可见性、命令反馈逐个看是不是
/// OP、签名聊天逐个接收者打包）。主控当场做一件事时，不在主控手里的岛的那一份记成它，
/// 岛落地时在自己身上执行（计划 5.5.3）。
///
/// A small delivery run on one island (a system message checking each
/// receiver's chat visibility, command feedback checking for ops, signed chat
/// packed per receiver). When the master does something at once, the share of
/// an island that is away is recorded as one of these and runs on that island
/// when it lands (plan 5.5.3).
pub(crate) type IslandTask = Arc<dyn Fn(&mut crate::region::Region) + Send + Sync>;

/// 主控延后投给一个不在手里的岛的一条出站投递（计划 5.5.3），岛落地时按顺序执行。
///
/// One outbound delivery the master defers for an island that is away (plan
/// 5.5.3); the island applies them in order when it lands.
#[derive(Clone)]
pub(crate) enum FanOut {
    /// 给这个岛上所有人的帧。
    All(Arc<[u8]>),
    /// 给这个岛上某一名玩家的帧（不在这个岛上就丢掉）。
    To(PlayerId, Arc<[u8]>),
    /// 踢掉这个岛上的某一名玩家。
    Kick(PlayerId, Component),
    /// 在这个岛上执行的一小段投递。
    Run(IslandTask),
}

/// 主控当场做一件事时对「不在主控手里的岛」的收集（计划 5.5.3）。
struct Absent {
    /// 那些岛上的玩家。
    ids: std::collections::BTreeSet<PlayerId>,
    /// 按发生的先后记下的投递。
    out: Vec<FanOut>,
}

/// 所有玩家的出站链路。
#[derive(Default)]
pub(crate) struct Links {
    /// 打开时，写不到的部分（给所有人的帧、给不在场玩家的帧）按先后记在这里。
    ///
    /// While set, what cannot be written here (frames for everyone, frames for
    /// players who are away) is recorded in order.
    absent: Option<Absent>,
    links: BTreeMap<PlayerId, Link>,
    /// 岛的出站信箱：主控执行时打开，写给玩家的包先按顺序投进这里，主控做完这件事
    /// 岛再按原顺序写进各名玩家的缓冲。`depth` 是嵌套层数，回到 0 才应用。
    ///
    /// The island's outbound mailbox: open while the master runs something,
    /// packets for players are posted here in order, and the island writes them
    /// into each player's buffer in the same order once the master is done.
    /// `depth` counts nesting; the mail is applied when it returns to 0.
    mail: Vec<FrameMail>,
    depth: usize,
}

impl Links {
    /// 主控开始在这个岛上执行一件事（全局类命令、控制台命令、交回的全局改动）：
    /// 之后写给玩家的包都先进岛的出站信箱。可以嵌套。
    ///
    /// The master starts running something on this island (a global or console
    /// command, a handed-back global change): packets go to the island's
    /// outbound mailbox from now on. May nest.
    pub(crate) fn open_mailbox(&mut self) {
        self.depth += 1;
    }

    /// 主控做完了：最外层时岛按投递顺序把信箱里的包写进各名玩家的缓冲。齐步和现在的
    /// 分离驱动都在这一刻应用，所以写进缓冲的顺序和时点和直接写一样。
    ///
    /// The master is done: at the outermost level the island writes the posted
    /// packets into each buffer in posting order. Both drives apply it right
    /// here for now, so order and timing match direct writes.
    pub(crate) fn deliver_mailbox(&mut self) {
        debug_assert!(self.depth > 0, "信箱没有打开");
        self.depth = self.depth.saturating_sub(1);
        if self.depth > 0 {
            return;
        }
        for mail in std::mem::take(&mut self.mail) {
            match mail {
                FrameMail::To(to, frames) => self.send_frames(to, &frames),
                FrameMail::Shared(to, frames) => self.send_shared(to, frames),
                FrameMail::All(frames) => self.broadcast(&frames),
                FrameMail::Kick(to, reason) => self.kick(to, &reason),
            }
        }
    }

    fn posting(&self) -> bool {
        self.depth > 0
    }

    /// 开始收集给不在主控手里的岛的投递；`ids` 是那些岛上的玩家（计划 5.5.3）。
    ///
    /// Start collecting deliveries for islands that are away; `ids` are the
    /// players on them (plan 5.5.3).
    pub(crate) fn begin_fan_out(&mut self, ids: std::collections::BTreeSet<PlayerId>) {
        debug_assert!(self.absent.is_none(), "收集已经开着");
        self.absent = Some(Absent {
            ids,
            out: Vec::new(),
        });
    }

    /// 结束收集，取走记下的投递（按发生的先后）。
    ///
    /// Stop collecting and take the recorded deliveries, in order.
    pub(crate) fn end_fan_out(&mut self) -> Vec<FanOut> {
        debug_assert!(!self.posting(), "出站信箱还开着就结束收集");
        self.absent
            .take()
            .map(|absent| absent.out)
            .unwrap_or_default()
    }

    /// 正在收集时记下一小段要在不在场的岛上执行的投递；没在收集时什么都不做。
    ///
    /// Record a delivery to run on the islands that are away, if collecting.
    pub(crate) fn fan_out(&mut self, task: impl FnOnce() -> IslandTask) {
        if let Some(absent) = &mut self.absent {
            absent.out.push(FanOut::Run(task()));
        }
    }

    /// 正在收集并且这名玩家在不在场的岛上。
    fn is_absent(&self, id: PlayerId) -> bool {
        self.absent
            .as_ref()
            .is_some_and(|absent| absent.ids.contains(&id))
    }

    /// 岛落地时照着做一条延后的投递（[`FanOut::Run`] 由调用方在整个岛上执行）。
    ///
    /// Apply one deferred delivery when the island lands ([`FanOut::Run`] is
    /// run by the caller on the whole island).
    pub(crate) fn apply_fan_out(&mut self, item: &FanOut) {
        match item {
            FanOut::All(frames) => self.broadcast(frames),
            FanOut::To(to, frames) => self.send_frames(*to, frames),
            FanOut::Kick(to, reason) => self.kick(*to, reason),
            FanOut::Run(_) => {}
        }
    }

    /// 岛拆分：`players` 的链路（连同还没刷出去的缓冲）搬进一份新的。只在岛停放时调用，
    /// 这时出站信箱是关着的、空的。
    ///
    /// Island split: move the links of `players`, buffers included. Only while
    /// the island is parked, when the outbound mailbox is closed and empty.
    pub(crate) fn split_off(&mut self, players: &std::collections::BTreeSet<PlayerId>) -> Self {
        debug_assert!(self.depth == 0 && self.mail.is_empty(), "出站信箱还开着");
        let mut out = Self::default();
        for id in players {
            if let Some(link) = self.links.remove(id) {
                out.links.insert(*id, link);
            }
        }
        out
    }

    /// 岛合并：另一份的链路并进来。
    ///
    /// Island merge: take the other side's links.
    pub(crate) fn absorb(&mut self, other: Self) {
        debug_assert!(self.depth == 0 && self.mail.is_empty(), "出站信箱还开着");
        debug_assert!(other.depth == 0 && other.mail.is_empty(), "出站信箱还开着");
        for (id, link) in other.links {
            let previous = self.links.insert(id, link);
            debug_assert!(previous.is_none(), "玩家在两个岛上");
        }
    }

    /// 有链路的玩家，按编号升序。测试核对拆合用。
    #[cfg(test)]
    pub(crate) fn players(&self) -> Vec<PlayerId> {
        self.links.keys().copied().collect()
    }

    pub(crate) fn add(&mut self, id: PlayerId, sink: Box<dyn PlayerSink>) {
        let link = Link {
            sink,
            buffer: Outgoing::default(),
            open: true,
        };
        self.links.insert(id, link);
    }

    /// 连接已经结束，丢掉这名玩家的链路和还没发出去的包。
    pub(crate) fn remove(&mut self, id: PlayerId) {
        self.links.remove(&id);
    }

    /// 这名玩家还能收包。没有这名玩家，或者它已经被踢时是 `false`。
    pub(crate) fn is_open(&self, id: PlayerId) -> bool {
        self.links.get(&id).is_some_and(|link| link.open)
    }

    fn open_link(&mut self, id: PlayerId) -> Option<&mut Link> {
        self.links.get_mut(&id).filter(|link| link.open)
    }

    /// 发一个包。包太大编码不出来时踢掉这名玩家，别的玩家不受影响。
    pub(crate) fn send<P: ClientboundPacket>(&mut self, to: PlayerId, packet: &P) {
        if self.posting() {
            if !self.is_open(to) {
                return;
            }
            let mut frames = Vec::new();
            match append_frame(&mut frames, packet) {
                Ok(()) => self.mail.push(FrameMail::To(to, frames)),
                Err(error) => {
                    warn!(%error, player = to.0, "kicking a player whose packet cannot be encoded");
                    self.kick(to, &Component::translate(PACKET_ERROR, []));
                }
            }
            return;
        }
        if self.is_absent(to) {
            let mut frames = Vec::new();
            let item = match append_frame(&mut frames, packet) {
                Ok(()) => FanOut::To(to, frames.into()),
                Err(error) => {
                    warn!(%error, player = to.0, "kicking a player whose packet cannot be encoded");
                    FanOut::Kick(to, Component::translate(PACKET_ERROR, []))
                }
            };
            if let Some(absent) = &mut self.absent {
                absent.out.push(item);
            }
            return;
        }
        let Some(link) = self.open_link(to) else {
            return;
        };
        if let Err(error) = append_frame(link.buffer.raw_buffer(), packet) {
            warn!(%error, player = to.0, "kicking a player whose packet cannot be encoded");
            self.kick(to, &Component::translate(PACKET_ERROR, []));
        }
    }

    /// 发已经编码好的帧。
    pub(crate) fn send_frames(&mut self, to: PlayerId, frames: &[u8]) {
        if self.posting() {
            self.mail.push(FrameMail::To(to, frames.to_vec()));
            return;
        }
        if self.is_absent(to) {
            if let Some(absent) = &mut self.absent {
                absent.out.push(FanOut::To(to, frames.into()));
            }
            return;
        }
        if let Some(link) = self.open_link(to) {
            link.buffer.push_raw(frames);
        }
    }

    /// 发多共用的、已经编码好的帧（区块包）。
    pub(crate) fn send_shared(&mut self, to: PlayerId, frames: Arc<SharedFrames>) {
        if self.posting() {
            self.mail.push(FrameMail::Shared(to, frames));
            return;
        }
        if let Some(link) = self.open_link(to) {
            link.buffer.push_shared(frames);
        }
    }

    /// 发给所有人，帧只编码一次。
    pub(crate) fn broadcast(&mut self, frames: &[u8]) {
        if self.posting() {
            self.mail.push(FrameMail::All(frames.to_vec()));
            return;
        }
        for link in self.links.values_mut().filter(|link| link.open) {
            link.buffer.push_raw(frames);
        }
        if let Some(absent) = &mut self.absent {
            absent.out.push(FanOut::All(frames.into()));
        }
    }

    /// 发给除 `except` 以外的所有人。进服消息改用 `broadcast_system_message` 后只剩测试用。
    ///
    /// Send to everyone except `except`. Test-only since the join message moved to
    /// `broadcast_system_message`.
    #[cfg(test)]
    pub(crate) fn broadcast_except(&mut self, except: PlayerId, frames: &[u8]) {
        if self.posting() {
            let ids: Vec<PlayerId> = self.links.keys().copied().collect();
            for id in ids.into_iter().filter(|&id| id != except) {
                self.send_frames(id, frames);
            }
            return;
        }
        for (_, link) in self
            .links
            .iter_mut()
            .filter(|(id, link)| **id != except && link.open)
        {
            link.buffer.push_raw(frames);
        }
    }

    /// 告诉客户端断开的原因，然后关闭链路，之后写给它的包都丢弃。
    ///
    /// 玩家留在世界里，直到连接层的 [`Inbound::Leave`](crate::Inbound::Leave) 到达。
    pub(crate) fn kick(&mut self, to: PlayerId, reason: &Component) {
        if self.posting() {
            self.mail.push(FrameMail::Kick(to, reason.clone()));
            return;
        }
        if self.is_absent(to) {
            if let Some(absent) = &mut self.absent {
                absent.out.push(FanOut::Kick(to, reason.clone()));
            }
            return;
        }
        let Some(link) = self.open_link(to) else {
            return;
        };
        if let Err(error) = append_frame(link.buffer.raw_buffer(), &Disconnect { reason }) {
            warn!(%error, player = to.0, "cannot encode the disconnect reason");
        }
        let _ = link.flush();
        if link.open {
            link.close();
        }
    }

    /// 踢掉所有人。
    pub(crate) fn kick_all(&mut self, reason: &Component) {
        let ids: Vec<PlayerId> = self.links.keys().copied().collect();
        for id in ids {
            self.kick(id, reason);
        }
    }

    /// 把攒下的包交给连接层。连接层拒收时关闭这条链路。
    /// 返回这一刻所有玩家实际交出去的未压缩字节数，给剖析器记账。
    pub(crate) fn flush(&mut self) -> u64 {
        debug_assert!(!self.posting(), "出站信箱还开着就刷出");
        self.links
            .values_mut()
            .filter(|link| link.open)
            .map(|link| link.flush())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use mc_bamboo_protocol::{
        frame::MAX_FRAME_LEN,
        packet::play::{ChunkBatchFinished, KeepAlive, LevelChunkWithLight},
    };

    use super::*;

    const ALICE: PlayerId = PlayerId(1);
    const BOB: PlayerId = PlayerId(2);

    #[derive(Default)]
    struct SinkState {
        batches: Vec<Vec<u8>>,
        closes: usize,
        refuses: bool,
    }

    #[derive(Clone, Default)]
    struct TestSink(Arc<Mutex<SinkState>>);

    impl TestSink {
        fn bytes(&self) -> Vec<u8> {
            self.0.lock().unwrap().batches.concat()
        }

        fn batches(&self) -> usize {
            self.0.lock().unwrap().batches.len()
        }

        fn closes(&self) -> usize {
            self.0.lock().unwrap().closes
        }

        fn refuse(&self) {
            self.0.lock().unwrap().refuses = true;
        }
    }

    impl PlayerSink for TestSink {
        fn send(&mut self, frames: Outgoing) -> bool {
            let mut state = self.0.lock().unwrap();
            if state.refuses {
                return false;
            }
            state.batches.push(frames.to_raw_bytes());
            true
        }

        fn close(&mut self) {
            self.0.lock().unwrap().closes += 1;
        }
    }

    /// 内容比一帧的上限还大的包。
    struct Oversized;

    impl ClientboundPacket for Oversized {
        const ID: i32 = 1;

        fn write_body(&self, out: &mut Vec<u8>) {
            out.resize(out.len() + MAX_FRAME_LEN + 1, 0);
        }
    }

    fn links_with(ids: &[PlayerId]) -> (Links, Vec<TestSink>) {
        let mut links = Links::default();
        let sinks: Vec<TestSink> = ids.iter().map(|_| TestSink::default()).collect();
        for (&id, sink) in ids.iter().zip(&sinks) {
            links.add(id, Box::new(sink.clone()));
        }
        (links, sinks)
    }

    fn frame<P: ClientboundPacket>(packet: &P) -> Vec<u8> {
        encode_frames(packet)
    }

    #[test]
    fn one_flush_hands_over_everything_written_in_the_tick() {
        let (mut links, sinks) = links_with(&[ALICE]);
        let first = KeepAlive { id: 1 };
        let second = ChunkBatchFinished { batch_size: 9 };

        links.send(ALICE, &first);
        links.send(ALICE, &second);
        assert_eq!(sinks[0].batches(), 0);
        links.flush();
        links.flush();

        assert_eq!(sinks[0].batches(), 1);
        assert_eq!(sinks[0].bytes(), [frame(&first), frame(&second)].concat());
    }

    #[test]
    fn shared_frames_keep_their_place_between_raw_frames() {
        let (mut links, sinks) = links_with(&[ALICE]);
        let packet = LevelChunkWithLight {
            x: 3,
            z: -2,
            body: &[1; 300],
        };
        let mut shared_bytes = Vec::new();
        append_frame(&mut shared_bytes, &packet).unwrap();
        let shared = Arc::new(SharedFrames::new(shared_bytes.clone()));
        let first = KeepAlive { id: 1 };
        let last = ChunkBatchFinished { batch_size: 1 };

        links.send(ALICE, &first);
        links.send_shared(ALICE, Arc::clone(&shared));
        links.send(ALICE, &last);
        links.flush();

        assert_eq!(
            sinks[0].bytes(),
            [frame(&first), shared_bytes, frame(&last)].concat()
        );
        assert_eq!(sinks[0].batches(), 1);
    }

    #[test]
    fn flush_reports_the_uncompressed_bytes_it_handed_over() {
        let (mut links, sinks) = links_with(&[ALICE, BOB]);
        let first = KeepAlive { id: 1 };
        let second = ChunkBatchFinished { batch_size: 9 };
        let shared = Arc::new(SharedFrames::new(vec![1, 2, 3, 4, 5]));

        links.send(ALICE, &first);
        links.send_shared(ALICE, Arc::clone(&shared));
        links.send(BOB, &second);
        let expected = (frame(&first).len() + shared.raw().len() + frame(&second).len()) as u64;

        assert_eq!(links.flush(), expected);
        assert_eq!(links.flush(), 0, "空缓冲不再记账");
        assert_eq!(
            sinks[0].bytes().len() + sinks[1].bytes().len(),
            expected as usize
        );
    }

    #[test]
    fn refused_bytes_are_not_counted_as_flushed() {
        let (mut links, sinks) = links_with(&[ALICE, BOB]);
        sinks[0].refuse();

        links.broadcast(&[1, 2, 3]);
        assert_eq!(links.flush(), 3, "拒收的那名玩家不算");
        assert_eq!(sinks[1].bytes(), [1, 2, 3]);
    }

    #[test]
    fn one_shared_frame_can_be_handed_to_many_players() {
        let (mut links, sinks) = links_with(&[ALICE, BOB]);
        let shared = Arc::new(SharedFrames::new(vec![1, 2, 3]));

        links.send_shared(ALICE, Arc::clone(&shared));
        links.send_shared(BOB, Arc::clone(&shared));
        links.flush();

        assert_eq!(sinks[0].bytes(), [1, 2, 3]);
        assert_eq!(sinks[1].bytes(), [1, 2, 3]);
    }

    #[test]
    fn broadcast_reaches_everyone_and_except_skips_one() {
        let (mut links, sinks) = links_with(&[ALICE, BOB]);

        links.broadcast(&[1]);
        links.broadcast_except(ALICE, &[2]);
        links.send_frames(ALICE, &[3]);
        links.flush();

        assert_eq!(sinks[0].bytes(), [1, 3]);
        assert_eq!(sinks[1].bytes(), [1, 2]);
    }

    #[test]
    fn writing_to_an_unknown_player_is_harmless() {
        let (mut links, sinks) = links_with(&[ALICE]);

        links.send(BOB, &KeepAlive { id: 1 });
        links.send_frames(BOB, &[1]);
        links.kick(BOB, &Component::text("bye"));
        links.remove(BOB);
        links.flush();

        assert!(!links.is_open(BOB));
        assert_eq!(sinks[0].batches(), 0);
    }

    #[test]
    fn kick_flushes_the_reason_closes_and_drops_later_writes() {
        let (mut links, sinks) = links_with(&[ALICE]);
        let reason = Component::text("bye");
        let queued = KeepAlive { id: 1 };

        links.send(ALICE, &queued);
        links.kick(ALICE, &reason);
        let expected = [frame(&queued), frame(&Disconnect { reason: &reason })].concat();
        assert_eq!(sinks[0].bytes(), expected);
        assert_eq!(sinks[0].closes(), 1);
        assert!(!links.is_open(ALICE));

        links.send(ALICE, &queued);
        links.broadcast(&[1]);
        links.kick(ALICE, &reason);
        links.flush();
        assert_eq!(sinks[0].bytes(), expected);
        assert_eq!(sinks[0].closes(), 1);
    }

    #[test]
    fn refused_frames_close_the_link_and_later_writes_are_dropped() {
        let (mut links, sinks) = links_with(&[ALICE, BOB]);
        sinks[0].refuse();

        links.broadcast(&[1]);
        links.flush();

        assert!(!links.is_open(ALICE));
        assert_eq!(sinks[0].closes(), 1);
        assert!(links.is_open(BOB));
        assert_eq!(sinks[1].bytes(), [1]);

        links.broadcast(&[2]);
        links.flush();
        assert_eq!(sinks[0].closes(), 1);
        assert_eq!(sinks[0].batches(), 0);
        assert_eq!(sinks[1].bytes(), [1, 2]);
    }

    #[test]
    fn kicking_a_refusing_sink_closes_it_once() {
        let (mut links, sinks) = links_with(&[ALICE]);
        sinks[0].refuse();

        links.kick(ALICE, &Component::text("bye"));

        assert_eq!(sinks[0].closes(), 1);
    }

    #[test]
    fn oversized_packet_kicks_only_its_receiver() {
        let (mut links, sinks) = links_with(&[ALICE, BOB]);
        let queued = KeepAlive { id: 1 };

        links.send(ALICE, &queued);
        links.send(ALICE, &Oversized);
        links.send(BOB, &queued);
        links.flush();

        let reason = Component::translate(PACKET_ERROR, []);
        assert_eq!(
            sinks[0].bytes(),
            [frame(&queued), frame(&Disconnect { reason: &reason })].concat()
        );
        assert_eq!(sinks[0].closes(), 1);
        assert!(!links.is_open(ALICE));
        assert_eq!(sinks[1].bytes(), frame(&queued));
        assert!(links.is_open(BOB));
    }

    /// 主控执行期间写的包先进岛的出站信箱，应用之前一个字节都不进缓冲；应用后和直接
    /// 写的字节、顺序、踢人效果逐位相同（含编码失败转成的踢人、踢人之后被丢掉的包）。
    ///
    /// Packets written while the master runs stay in the island's outbound
    /// mailbox until applied; afterwards the bytes, order and kicks match direct
    /// writes exactly (including an encode failure turned into a kick and the
    /// packets dropped after a kick).
    #[test]
    fn mailbox_writes_match_direct_writes_and_wait_for_delivery() {
        fn script(links: &mut Links) {
            let shared = Arc::new(SharedFrames::new(vec![9, 9]));
            links.send(ALICE, &KeepAlive { id: 1 });
            links.broadcast(&[1, 2]);
            links.send_shared(BOB, Arc::clone(&shared));
            links.send(BOB, &Oversized);
            links.send_frames(BOB, &[7]);
            links.broadcast_except(ALICE, &[3]);
            links.kick(ALICE, &Component::text("bye"));
            links.send(ALICE, &ChunkBatchFinished { batch_size: 2 });
        }
        let (mut direct, direct_sinks) = links_with(&[ALICE, BOB]);
        script(&mut direct);
        direct.flush();

        let (mut posted, posted_sinks) = links_with(&[ALICE, BOB]);
        posted.open_mailbox();
        posted.open_mailbox();
        script(&mut posted);
        posted.deliver_mailbox();
        assert!(posted.is_open(ALICE), "内层关上时还没应用");
        assert_eq!(posted_sinks[0].batches(), 0, "应用之前不碰缓冲");
        posted.deliver_mailbox();
        posted.flush();

        for (direct, posted) in direct_sinks.iter().zip(&posted_sinks) {
            assert_eq!(direct.bytes(), posted.bytes());
            assert_eq!(direct.closes(), posted.closes());
        }
        assert!(!posted.is_open(ALICE));
        assert!(!posted.is_open(BOB), "编码失败照样踢掉");
    }

    #[test]
    fn kick_all_reaches_every_open_link() {
        let (mut links, sinks) = links_with(&[ALICE, BOB]);
        let reason = Component::text("bye");
        links.kick(BOB, &reason);

        links.kick_all(&reason);

        assert_eq!(sinks[0].closes(), 1);
        assert_eq!(sinks[1].closes(), 1);
    }
}
