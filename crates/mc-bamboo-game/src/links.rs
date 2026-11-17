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

/// 所有玩家的出站链路。
#[derive(Default)]
pub(crate) struct Links {
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
