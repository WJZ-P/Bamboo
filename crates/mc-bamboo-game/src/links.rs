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
const PACKET_ERROR: &str = "disconnect.packetError";

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
    fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        if !self.sink.send(mem::take(&mut self.buffer)) {
            self.close();
        }
    }

    fn close(&mut self) {
        self.open = false;
        self.buffer = Outgoing::default();
        self.sink.close();
    }
}

/// 所有玩家的出站链路。
#[derive(Default)]
pub(crate) struct Links {
    links: BTreeMap<PlayerId, Link>,
}

impl Links {
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
        if let Some(link) = self.open_link(to) {
            link.buffer.push_raw(frames);
        }
    }

    /// 发多共用的、已经编码好的帧（区块包）。
    pub(crate) fn send_shared(&mut self, to: PlayerId, frames: Arc<SharedFrames>) {
        if let Some(link) = self.open_link(to) {
            link.buffer.push_shared(frames);
        }
    }

    /// 发给所有人，帧只编码一次。
    pub(crate) fn broadcast(&mut self, frames: &[u8]) {
        for link in self.links.values_mut().filter(|link| link.open) {
            link.buffer.push_raw(frames);
        }
    }

    /// 发给除 `except` 以外的所有人。
    pub(crate) fn broadcast_except(&mut self, except: PlayerId, frames: &[u8]) {
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
        let Some(link) = self.open_link(to) else {
            return;
        };
        if let Err(error) = append_frame(link.buffer.raw_buffer(), &Disconnect { reason }) {
            warn!(%error, player = to.0, "cannot encode the disconnect reason");
        }
        link.flush();
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
    pub(crate) fn flush(&mut self) {
        for link in self.links.values_mut().filter(|link| link.open) {
            link.flush();
        }
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
