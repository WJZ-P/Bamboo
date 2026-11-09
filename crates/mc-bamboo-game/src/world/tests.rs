//! 用假的 [`PlayerSink`] 直接驱动 [`World::tick`]，不开 socket。

use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Sender},
    },
};

use mc_bamboo_blocks::LevelRandom;
use mc_bamboo_core::{BlockPos, ChunkPos, Direction, Hand};
use mc_bamboo_data::{blocks, packets::play::clientbound};
use mc_bamboo_protocol::{
    codec::Reader,
    frame::{MAX_FRAME_LEN, find_frame},
    item::ItemStack,
    packet::play::{
        ClientCommandKind, Disconnect, HumanoidArm, PlayerActionKind, PlayerCommandKind,
    },
    text::Color,
};
use uuid::Uuid;

use super::*;
use crate::{InFlight, PlayerSink, chunk_encoding::encode_chunk, links::encode_frames};

mod fuzz;

pub(crate) const QUOTA: usize = 4096;
pub(crate) const SPAWN_X: f64 = 8.5;
pub(crate) const SPAWN_Y: f64 = -60.0;
pub(crate) const SPAWN_Z: f64 = 8.5;

#[derive(Default)]
pub(crate) struct SinkState {
    bytes: Vec<u8>,
    closed: bool,
    refusing: bool,
}

#[derive(Clone, Default)]
pub(crate) struct FakeSink(Arc<Mutex<SinkState>>);

impl PlayerSink for FakeSink {
    fn send(&mut self, frames: Vec<u8>) -> bool {
        let mut state = self.0.lock().unwrap();
        if state.refusing {
            return false;
        }
        state.bytes.extend(frames);
        true
    }

    fn close(&mut self) {
        self.0.lock().unwrap().closed = true;
    }
}

/// 一个包：ID 和后面的内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) id: i32,
    pub(crate) body: Vec<u8>,
}

/// 假客户端。收到的字节按帧拆开，不解释内容。
pub(crate) struct Client {
    pub(crate) id: PlayerId,
    pub(crate) uuid: Uuid,
    sink: FakeSink,
}

impl Client {
    pub(crate) fn take_frames(&self) -> Vec<Frame> {
        let bytes = std::mem::take(&mut self.sink.0.lock().unwrap().bytes);
        let mut rest = bytes.as_slice();
        let mut frames = Vec::new();
        while !rest.is_empty() {
            let bounds = find_frame(rest, MAX_FRAME_LEN)
                .expect("服务端只发合法的帧")
                .expect("flush 的都是完整的帧");
            let end = bounds.header_len + bounds.payload_len;
            let mut reader = Reader::new(&rest[bounds.header_len..end]);
            let id = reader.varint().unwrap();
            frames.push(Frame {
                id,
                body: reader.rest().to_vec(),
            });
            rest = &rest[end..];
        }
        frames
    }

    pub(crate) fn take_ids(&self) -> Vec<i32> {
        self.take_frames().iter().map(|frame| frame.id).collect()
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.sink.0.lock().unwrap().closed
    }

    pub(crate) fn refuse_frames(&self) {
        self.sink.0.lock().unwrap().refusing = true;
    }
}

pub(crate) fn frames_with_id(frames: &[Frame], id: i32) -> Vec<&Frame> {
    frames.iter().filter(|frame| frame.id == id).collect()
}

/// `section_blocks_update` 里的格子：世界坐标和方块状态，按包里的顺序。
pub(crate) fn section_blocks(frame: &Frame) -> Vec<(BlockPos, u32)> {
    let mut reader = Reader::new(&frame.body);
    let section = reader.i64().unwrap();
    // 官方的 `SectionPos.of(long)`：x 在高 22 位，z 在中间 22 位，y 在低 20 位。
    let origin = BlockPos::new(
        (section >> 42) as i32 * 16,
        (section << 44 >> 44) as i32 * 16,
        (section << 22 >> 42) as i32 * 16,
    );
    let count = reader.varint().unwrap();
    let blocks = (0..count)
        .map(|_| {
            let entry = reader.varlong().unwrap();
            let local = (entry & 0xfff) as i32;
            let pos = origin.offset(local >> 8, local & 15, (local >> 4) & 15);
            (pos, (entry >> 12) as u32)
        })
        .collect();
    reader.finish().unwrap();
    blocks
}

/// 客户端按收到的顺序写进世界的方块，`block_update` 和 `section_blocks_update` 都算。
pub(crate) fn block_changes(frames: &[Frame]) -> Vec<(BlockPos, u32)> {
    frames
        .iter()
        .flat_map(|frame| match frame.id {
            clientbound::BLOCK_UPDATE => {
                let mut reader = Reader::new(&frame.body);
                let pos = reader.block_pos().unwrap();
                vec![(pos, reader.varint().unwrap() as u32)]
            }
            clientbound::SECTION_BLOCKS_UPDATE => section_blocks(frame),
            _ => Vec::new(),
        })
        .collect()
}

/// 区块包开头的坐标：两个 Int。
pub(crate) fn chunk_position(frame: &Frame) -> ChunkPos {
    let mut reader = Reader::new(&frame.body);
    ChunkPos::new(reader.i32().unwrap(), reader.i32().unwrap())
}

/// 区块包里坐标之后的内容。
pub(crate) fn chunk_body(frame: &Frame) -> &[u8] {
    &frame.body[8..]
}

pub(crate) fn received_chunks(frames: &[Frame]) -> Vec<ChunkPos> {
    frames_with_id(frames, clientbound::LEVEL_CHUNK_WITH_LIGHT)
        .into_iter()
        .map(chunk_position)
        .collect()
}

pub(crate) fn body_of(frames: Vec<u8>) -> Vec<u8> {
    let mut reader = Reader::new(&frames);
    reader.varint().unwrap();
    reader.varint().unwrap();
    reader.rest().to_vec()
}

pub(crate) fn client_requesting(view_distance: i8) -> ClientInformation {
    ClientInformation {
        view_distance,
        model_customisation: 0x7f,
        main_hand: HumanoidArm::Left,
        ..ClientInformation::default()
    }
}

/// 测试里的“连接层”：用内存队列给世界线程发消息。
pub(crate) struct Harness {
    world: World,
    inbox: Sender<Inbound>,
    in_flight: Arc<AtomicUsize>,
    next_id: i32,
}

impl Harness {
    pub(crate) fn new(view_distance: i32) -> Self {
        let (inbox, receiver) = mpsc::channel();
        let config = WorldConfig {
            view_distance,
            max_players: 20,
            online_mode: false,
            enforce_secure_profile: false,
            admin: Default::default(),
        };
        let mut world = World::new(config, receiver);
        // 正式启动的随机种子来自时钟，测试要可重复，固定成 0。音效种子会进包，也一样固定。
        world.region.random = LevelRandom::new(0);
        world.region.sound_seed_generator = LevelRandom::new(0);
        Self {
            world,
            inbox,
            in_flight: Arc::default(),
            next_id: 1,
        }
    }

    /// 发出 `Join`，这一 tick 才处理。
    pub(crate) fn join_with(&mut self, name: &str, client: ClientInformation) -> Client {
        let id = PlayerId(self.next_id);
        self.next_id += 1;
        let sink = FakeSink::default();
        let uuid = Uuid::from_u128(id.0 as u128);
        let request = JoinRequest {
            id,
            uuid,
            name: name.to_owned(),
            properties: Vec::new(),
            client,
            permission_level: 0,
            is_op: false,
            peer_ip: "127.0.0.1".to_owned(),
            sink: Box::new(sink.clone()),
        };
        self.inbox.send(Inbound::Join(Box::new(request))).unwrap();
        Client { id, uuid, sink }
    }

    pub(crate) fn join(&mut self, name: &str) -> Client {
        self.join_with(name, client_requesting(2))
    }

    /// 用指定 UUID 进服，测试重连时用它。
    ///
    /// Join with a given UUID, used by reconnect tests.
    pub(crate) fn join_uuid(&mut self, name: &str, uuid: Uuid) -> Client {
        let id = PlayerId(self.next_id);
        self.next_id += 1;
        let sink = FakeSink::default();
        let request = JoinRequest {
            id,
            uuid,
            name: name.to_owned(),
            properties: Vec::new(),
            client: client_requesting(2),
            permission_level: 0,
            is_op: false,
            peer_ip: "127.0.0.1".to_owned(),
            sink: Box::new(sink.clone()),
        };
        self.inbox.send(Inbound::Join(Box::new(request))).unwrap();
        Client { id, uuid, sink }
    }

    pub(crate) fn request(&self, client: &Client, request: PlayRequest) {
        let in_flight = InFlight::acquire(&self.in_flight, QUOTA).expect("测试不会用完配额");
        self.inbox
            .send(Inbound::Request {
                player: client.id,
                request,
                in_flight,
            })
            .unwrap();
    }

    pub(crate) fn leave(&self, client: &Client) {
        self.inbox
            .send(Inbound::Leave { player: client.id })
            .unwrap();
    }

    pub(crate) fn tick(&mut self) {
        assert!(self.world.tick().is_continue());
    }

    pub(crate) fn acknowledge_batch(&self, client: &Client, desired_chunks_per_tick: f32) {
        self.request(
            client,
            PlayRequest::ChunkBatchReceived {
                desired_chunks_per_tick,
            },
        );
    }

    pub(crate) fn confirm_teleport(&self, client: &Client) {
        self.request(
            client,
            PlayRequest::AcceptTeleportation {
                id: 1,
                x: SPAWN_X,
                y: SPAWN_Y,
                z: SPAWN_Z,
                yaw: 0.0,
                pitch: 0.0,
            },
        );
    }

    pub(crate) fn move_to(&mut self, client: &Client, x: f64, z: f64) {
        self.move_to_position(client, Vec3::new(x, SPAWN_Y, z));
    }

    /// 把玩家放到 `position`。离得远、连接还开着，就直接放过去：测试要的是人已经在那儿。
    /// 人还没进世界，或者已经被踢，就照常把移动包排进队列。
    ///
    /// Puts the player at `position`. A long move on an open connection is applied directly,
    /// because the test wants them already there. A player who has not joined or was kicked
    /// still gets a queued move packet.
    pub(crate) fn move_to_position(&mut self, client: &Client, position: Vec3) {
        let here = self
            .world
            .region
            .players
            .get(&client.id)
            .map(|player| player.position());
        if let Some(here) = here
            && !client.is_closed()
            && position.subtract(here).length_sqr() > 100.0
        {
            crate::player_move::place_player(&mut self.world.region, client.id, position);
            return;
        }
        self.load_player(client);
        self.request(
            client,
            PlayRequest::MovePlayer {
                position: Some(position),
                rotation: None,
                on_ground: true,
                horizontal_collision: false,
            },
        );
        self.client_tick_end(client);
    }

    pub(crate) fn load_player(&self, client: &Client) {
        self.request(
            client,
            PlayRequest::Other(mc_bamboo_data::packets::play::serverbound::PLAYER_LOADED),
        );
    }

    pub(crate) fn client_tick_end(&self, client: &Client) {
        self.request(
            client,
            PlayRequest::Other(mc_bamboo_data::packets::play::serverbound::CLIENT_TICK_END),
        );
    }

    /// 一直回确认，直到没有新区块，返回收到的全部区块。
    pub(crate) fn receive_all_chunks(&mut self, client: &Client) -> Vec<ChunkPos> {
        let mut received = Vec::new();
        loop {
            self.tick();
            let frames = client.take_frames();
            let finished = frames_with_id(&frames, clientbound::CHUNK_BATCH_FINISHED).len();
            received.extend(received_chunks(&frames));
            for _ in 0..finished {
                self.acknowledge_batch(client, 64.0);
            }
            if finished == 0 {
                return received;
            }
        }
    }

    /// 进服、确认传送、收完区块。
    pub(crate) fn join_and_settle(&mut self, name: &str) -> Client {
        let client = self.join(name);
        self.confirm_teleport(&client);
        self.load_player(&client);
        self.receive_all_chunks(&client);
        client
    }

    /// 给其他模块的测试直接看全服状态。
    pub(crate) fn global(&self) -> &crate::global::Global {
        &self.world.global
    }

    /// 给其他模块的测试直接看世界的状态。
    pub(crate) fn region(&mut self) -> &mut Region {
        &mut self.world.region
    }

    pub(crate) fn player(&self, client: &Client) -> &crate::player::Player {
        self.world
            .region
            .players
            .get(&client.id)
            .expect("玩家应该在世界里")
    }
}

#[test]
fn join_writes_the_ten_steps_in_order_in_one_tick() {
    let mut server = Harness::new(8);
    let client = server.join("Kitten");

    server.tick();

    let mut expected = vec![
        clientbound::LOGIN,
        clientbound::PLAYER_ABILITIES,
        clientbound::ENTITY_EVENT,
        clientbound::COMMANDS,
        clientbound::PLAYER_POSITION,
        clientbound::PLAYER_INFO_UPDATE,
        clientbound::SET_ENTITY_DATA,
        clientbound::UPDATE_ATTRIBUTES,
        clientbound::SET_TIME,
        clientbound::GAME_EVENT,
        clientbound::SET_CHUNK_CACHE_CENTER,
        clientbound::CHUNK_BATCH_START,
    ];
    expected.extend([clientbound::LEVEL_CHUNK_WITH_LIGHT; 9]);
    expected.push(clientbound::CHUNK_BATCH_FINISHED);
    assert_eq!(client.take_ids(), expected);
}

#[test]
fn second_player_gets_the_roster_first_and_the_others_get_the_new_player() {
    let mut server = Harness::new(8);
    let first = server.join_and_settle("Alice");
    first.take_frames();

    let second = server.join("Bob");
    server.tick();

    let frames = second.take_frames();
    let ids: Vec<i32> = frames.iter().take(8).map(|frame| frame.id).collect();
    assert_eq!(
        ids,
        [
            clientbound::LOGIN,
            clientbound::PLAYER_ABILITIES,
            clientbound::ENTITY_EVENT,
            clientbound::COMMANDS,
            clientbound::PLAYER_POSITION,
            clientbound::PLAYER_INFO_UPDATE,
            clientbound::PLAYER_INFO_UPDATE,
            clientbound::SET_ENTITY_DATA,
        ]
    );
    // 已有的玩家先发给新玩家，然后才是它自己，两次都是全部 8 个动作。
    // 权限事件和命令树插在能力包和传送之间，所以名单从第 6 个包开始。
    //
    // The permission event and command tree sit between abilities and the teleport,
    // so the roster starts at the sixth packet.
    for (frame, uuid) in [(&frames[5], first.uuid), (&frames[6], second.uuid)] {
        assert_eq!(frame.body[..2], [0xff, 0x01]);
        assert_eq!(frame.body[2..18], *uuid.as_bytes());
    }

    let others = first.take_frames();
    // 后面是配对的 bundle，由 `entity_sync` 的测试检查。
    let ids: Vec<i32> = others.iter().take(2).map(|frame| frame.id).collect();
    assert_eq!(
        ids,
        [clientbound::PLAYER_INFO_UPDATE, clientbound::SYSTEM_CHAT]
    );
    assert_eq!(others[0].body[2..18], *second.uuid.as_bytes());

    let joined =
        Component::translate("multiplayer.player.joined", ["Bob".into()]).color(Color::Yellow);
    let expected = encode_frames(&mc_bamboo_protocol::packet::play::SystemChat {
        content: &joined,
        overlay: false,
    });
    assert_eq!(others[1].body, body_of(expected));
}

#[test]
fn everyone_online_arrives_in_one_tab_list_packet() {
    let mut server = Harness::new(8);
    let first = server.join_and_settle("Alice");
    let second = server.join_and_settle("Bob");

    let third = server.join("Carol");
    server.tick();

    let frames = third.take_frames();
    let updates = frames_with_id(&frames, clientbound::PLAYER_INFO_UPDATE);
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].body[..2], [0xff, 0x02]);
    assert_eq!(updates[0].body[2..18], *first.uuid.as_bytes());
    assert!(
        updates[0]
            .body
            .windows(16)
            .any(|part| part == second.uuid.as_bytes())
    );
    assert_eq!(updates[1].body[..2], [0xff, 0x01]);
    assert_eq!(updates[1].body[2..18], *third.uuid.as_bytes());
}

#[test]
fn joining_player_does_not_get_its_own_join_message() {
    let mut server = Harness::new(8);
    let client = server.join("Kitten");

    server.tick();

    assert!(frames_with_id(&client.take_frames(), clientbound::SYSTEM_CHAT).is_empty());
}

#[test]
fn login_uses_the_clamped_server_view_distance() {
    for (configured, expected) in [(0, 2), (1, 2), (8, 8), (32, 32), (99, 32)] {
        let mut server = Harness::new(configured);
        let client = server.join_with("Kitten", client_requesting(127));
        server.tick();

        let frames = client.take_frames();
        let login = &frames[0];
        let mut reader = Reader::new(&login.body);
        reader.i32().unwrap(); // 实体 ID
        reader.bool().unwrap(); // 极限模式
        reader.varint().unwrap(); // 维度个数
        reader.string(32767).unwrap();
        reader.varint().unwrap(); // 最大玩家数
        assert_eq!(reader.varint(), Ok(expected), "configured {configured}");
    }
}

#[test]
fn own_entity_data_comes_from_the_client_information() {
    let mut server = Harness::new(8);
    server.next_id = 7;
    let client = server.join_with("Kitten", client_requesting(2));

    server.tick();

    let frames = client.take_frames();
    let data = frames_with_id(&frames, clientbound::SET_ENTITY_DATA)[0];
    // 实体 7，index 15 左手，index 16 全部外层。
    assert_eq!(data.body, [0x07, 0x0f, 0x2a, 0x00, 0x10, 0x00, 0x7f, 0xff]);
}

#[test]
fn first_batch_is_exactly_the_three_by_three_around_the_spawn() {
    let mut server = Harness::new(8);
    let client = server.join_with("Kitten", client_requesting(8));

    server.tick();

    let frames = client.take_frames();
    let chunks: BTreeSet<ChunkPos> = received_chunks(&frames).into_iter().collect();
    let expected: BTreeSet<ChunkPos> = (-1..=1)
        .flat_map(|x| (-1..=1).map(move |z| ChunkPos::new(x, z)))
        .collect();
    assert_eq!(chunks, expected);
    assert_eq!(received_chunks(&frames)[0], ChunkPos::new(0, 0));

    let finished = frames_with_id(&frames, clientbound::CHUNK_BATCH_FINISHED);
    assert_eq!(finished[0].body, [9]);
}

#[test]
fn no_more_chunks_without_an_acknowledgement() {
    let mut server = Harness::new(8);
    let client = server.join("Kitten");
    server.tick();
    client.take_frames();

    for _ in 0..50 {
        server.tick();
    }

    assert!(client.take_frames().is_empty());
}

#[test]
fn batches_continue_after_chunk_batch_received() {
    let mut server = Harness::new(8);
    let client = server.join_with("Kitten", client_requesting(8));
    server.tick();
    client.take_frames();

    server.acknowledge_batch(&client, 3.5);
    server.tick();
    assert_eq!(received_chunks(&client.take_frames()).len(), 3);
    server.tick();
    assert_eq!(received_chunks(&client.take_frames()).len(), 3);

    server.acknowledge_batch(&client, 64.0);
    server.acknowledge_batch(&client, 64.0);
    server.tick();
    assert_eq!(received_chunks(&client.take_frames()).len(), 64);
}

#[test]
fn every_chunk_of_the_tracking_set_arrives_once() {
    for (requested, expected) in [(2, 49), (8, 329), (12, 329)] {
        let mut server = Harness::new(8);
        let client = server.join_with("Kitten", client_requesting(requested));

        let chunks = server.receive_all_chunks(&client);

        let unique: BTreeSet<ChunkPos> = chunks.iter().copied().collect();
        assert_eq!(chunks.len(), expected, "requested {requested}");
        assert_eq!(unique.len(), expected, "requested {requested}");
    }
}

#[test]
fn crossing_a_chunk_border_sends_center_then_forgets_then_new_chunks() {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    client.take_frames();

    server.move_to(&client, 15.9, 8.0);
    server.tick();
    assert!(client.take_frames().is_empty());

    server.move_to(&client, 16.1, 8.0);
    server.tick();

    let frames = client.take_frames();
    let ids: Vec<i32> = frames.iter().map(|frame| frame.id).collect();
    let mut expected = vec![clientbound::SET_CHUNK_CACHE_CENTER];
    expected.extend([clientbound::FORGET_LEVEL_CHUNK; 7]);
    expected.push(clientbound::CHUNK_BATCH_START);
    expected.extend([clientbound::LEVEL_CHUNK_WITH_LIGHT; 7]);
    expected.push(clientbound::CHUNK_BATCH_FINISHED);
    assert_eq!(ids, expected);

    assert_eq!(frames[0].body, [0x01, 0x00]);
    // 线上 z 在前，x 在外层，z 升序。
    assert_eq!(
        frames[1].body,
        [0xff, 0xff, 0xff, 0xfd, 0xff, 0xff, 0xff, 0xfd]
    );
    assert_eq!(
        frames[7].body,
        [0x00, 0x00, 0x00, 0x03, 0xff, 0xff, 0xff, 0xfd]
    );
    // 新区块按到玩家的距离排序。
    let chunks = received_chunks(&frames);
    assert_eq!(chunks[0], ChunkPos::new(4, 0));
    let columns: BTreeSet<i32> = chunks.iter().map(|pos| pos.x).collect();
    assert_eq!(columns, BTreeSet::from([4]));
}

#[test]
fn moving_up_and_down_sends_nothing() {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    client.take_frames();

    server.request(
        &client,
        PlayRequest::MovePlayer {
            position: Some(Vec3::new(SPAWN_X, -50.0, SPAWN_Z)),
            rotation: None,
            on_ground: false,
            horizontal_collision: false,
        },
    );
    server.tick();

    assert!(client.take_frames().is_empty());
}

#[test]
fn walking_before_the_chunks_arrived_drops_them_without_forgetting() {
    let mut server = Harness::new(8);
    let client = server.join("Kitten");
    server.confirm_teleport(&client);
    server.tick();
    client.take_frames();

    // 第一批确认之前就走远了：新视图和旧视图没有交集。
    server.move_to(&client, 1000.0, 1000.0);
    server.tick();

    let frames = client.take_frames();
    let forgotten = frames_with_id(&frames, clientbound::FORGET_LEVEL_CHUNK);
    assert_eq!(forgotten.len(), 9);
    assert_eq!(frames[0].id, clientbound::SET_CHUNK_CACHE_CENTER);
}

#[test]
fn stale_leave_is_ignored() {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    client.take_frames();

    server
        .inbox
        .send(Inbound::Leave {
            player: PlayerId(999),
        })
        .unwrap();
    server.tick();

    assert!(client.take_frames().is_empty());
    assert_eq!(server.world.region.players.len(), 1);
    assert!(!client.is_closed());
}

#[test]
fn second_leave_of_the_same_player_changes_nothing() {
    let mut server = Harness::new(8);
    let staying = server.join_and_settle("Alice");
    let leaving = server.join_and_settle("Bob");
    staying.take_frames();

    server.leave(&leaving);
    server.leave(&leaving);
    server.tick();

    let frames = staying.take_frames();
    assert_eq!(frames_with_id(&frames, clientbound::SYSTEM_CHAT).len(), 1);
    assert_eq!(
        frames_with_id(&frames, clientbound::PLAYER_INFO_REMOVE).len(),
        1
    );
}

#[test]
fn leave_tells_the_others_in_order() {
    let mut server = Harness::new(8);
    let staying = server.join_and_settle("Alice");
    let leaving = server.join_and_settle("Bob");
    staying.take_frames();

    server.leave(&leaving);
    server.tick();

    let frames = staying.take_frames();
    let ids: Vec<i32> = frames.iter().map(|frame| frame.id).collect();
    assert_eq!(
        ids,
        [
            clientbound::SYSTEM_CHAT,
            clientbound::REMOVE_ENTITIES,
            clientbound::PLAYER_INFO_REMOVE,
        ]
    );
    let left = Component::translate("multiplayer.player.left", ["Bob".into()]).color(Color::Yellow);
    let expected = encode_frames(&mc_bamboo_protocol::packet::play::SystemChat {
        content: &left,
        overlay: false,
    });
    assert_eq!(frames[0].body, body_of(expected));
    assert_eq!(frames[2].body[0], 1);
    assert_eq!(frames[2].body[1..], *leaving.uuid.as_bytes());
    assert_eq!(server.world.region.players.len(), 1);
}

#[test]
fn leave_before_reaching_the_world_sends_nothing() {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    client.take_frames();

    // 在 Configuration 里断开的玩家有 ID，但是世界线程从没见过它。
    server
        .inbox
        .send(Inbound::Leave {
            player: PlayerId(server.next_id),
        })
        .unwrap();
    server.tick();

    assert!(client.take_frames().is_empty());
}

#[test]
fn join_and_leave_in_the_same_tick_leaves_no_ghost() {
    let mut server = Harness::new(8);
    let staying = server.join_and_settle("Alice");
    staying.take_frames();

    let ghost = server.join("Bob");
    server.leave(&ghost);
    server.tick();

    assert_eq!(server.world.region.players.len(), 1);
    assert!(server.world.global.left_message(ghost.id).is_none());
    let frames = staying.take_frames();
    let ids: Vec<i32> = frames.iter().map(|frame| frame.id).collect();
    assert_eq!(
        ids,
        [
            clientbound::PLAYER_INFO_UPDATE,
            clientbound::SYSTEM_CHAT,
            clientbound::SYSTEM_CHAT,
            clientbound::PLAYER_INFO_REMOVE,
        ]
    );
}

#[test]
fn reconnecting_with_the_same_uuid_in_one_tick_keeps_the_new_player() {
    let mut server = Harness::new(8);
    let old = server.join_and_settle("Kitten");
    let new_id = PlayerId(50);
    let sink = FakeSink::default();

    server.leave(&old);
    let request = JoinRequest {
        id: new_id,
        uuid: old.uuid,
        name: "Kitten".to_owned(),
        properties: Vec::new(),
        client: client_requesting(2),
        permission_level: 0,
        is_op: false,
        peer_ip: "127.0.0.1".to_owned(),
        sink: Box::new(sink),
    };
    server.inbox.send(Inbound::Join(Box::new(request))).unwrap();
    server.tick();

    let ids: Vec<PlayerId> = server.world.region.players.keys().copied().collect();
    assert_eq!(ids, [new_id]);
}

#[test]
fn refused_sink_is_closed_and_the_player_waits_for_its_leave() {
    let mut server = Harness::new(8);
    let staying = server.join_and_settle("Alice");
    let refusing = server.join_and_settle("Bob");
    staying.take_frames();

    refusing.refuse_frames();
    server.move_to(&refusing, 100.0, 100.0);
    server.tick();

    assert!(refusing.is_closed());
    assert!(!server.world.region.links.is_open(refusing.id));
    assert_eq!(server.world.region.players.len(), 2);
    // Bob 走出了 Alice 的视距。
    assert_eq!(staying.take_ids(), [clientbound::REMOVE_ENTITIES]);

    server.leave(&refusing);
    server.tick();
    assert_eq!(server.world.region.players.len(), 1);
    assert!(!staying.is_closed());
    assert_eq!(
        frames_with_id(&staying.take_frames(), clientbound::SYSTEM_CHAT).len(),
        1
    );
}

#[test]
fn nan_position_kicks_and_later_requests_are_ignored() {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    client.take_frames();

    server.request(
        &client,
        PlayRequest::MovePlayer {
            position: Some(Vec3::new(f64::NAN, 0.0, 0.0)),
            rotation: None,
            on_ground: false,
            horizontal_collision: false,
        },
    );
    server.request(
        &client,
        PlayRequest::MovePlayer {
            position: Some(Vec3::new(100.0, SPAWN_Y, 100.0)),
            rotation: None,
            on_ground: true,
            horizontal_collision: false,
        },
    );
    server.tick();

    let reason = Component::translate("multiplayer.disconnect.invalid_player_movement", []);
    let expected = Frame {
        id: clientbound::DISCONNECT,
        body: body_of(encode_frames(&Disconnect { reason: &reason })),
    };
    assert_eq!(client.take_frames(), [expected]);
    assert!(client.is_closed());
    // 被踢的玩家留到 `Leave`，请求和写入都不再有效果。
    assert_eq!(server.player(&client).position().x, SPAWN_X);

    server.leave(&client);
    server.tick();
    assert!(server.world.region.players.is_empty());
}

#[test]
fn quota_is_returned_after_each_request() {
    let mut server = Harness::new(8);
    let client = server.join("Kitten");
    server.tick();

    server.acknowledge_batch(&client, 1.0);
    // 一步之内，请求才会进队列。再远就直接放过去，不占名额。
    //
    // A step this short is queued. Anything past the speed limit is placed and does not
    // take a quota slot.
    server.move_to(&client, SPAWN_X + 1.0, SPAWN_Z);
    // 确认、player_loaded、移动、client_tick_end。
    //
    // The acknowledgement, player_loaded, the move, and client_tick_end.
    assert_eq!(server.in_flight.load(Ordering::Relaxed), 4);
    server.tick();

    assert_eq!(server.in_flight.load(Ordering::Relaxed), 0);
}

#[test]
fn requests_from_players_that_left_are_dropped_with_their_quota() {
    let mut server = Harness::new(8);
    let client = server.join("Kitten");
    server.leave(&client);
    server.move_to(&client, 1.0, 1.0);

    server.tick();

    assert_eq!(server.in_flight.load(Ordering::Relaxed), 0);
    assert!(server.world.region.players.is_empty());
}

#[test]
fn edited_chunk_is_resent_with_the_edit() {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    client.take_frames();
    let edited = ChunkPos::new(0, 0);
    let flat = server.world.region.template_body.clone();

    let pos = BlockPos::new(1, -60, 1);
    assert!(server.world.region.chunks.set_block(pos, blocks::CHEST));

    // 走到视距外，再走回来，这个区块要重新发。
    server.move_to(&client, 1000.0, 1000.0);
    server.tick();
    let frames = client.take_frames();
    assert!(
        frames_with_id(&frames, clientbound::FORGET_LEVEL_CHUNK)
            .iter()
            .any(|frame| frame.body == [0, 0, 0, 0, 0, 0, 0, 0])
    );
    server.acknowledge_batch(&client, 64.0);
    server.tick();
    client.take_frames();

    server.move_to(&client, SPAWN_X, SPAWN_Z);
    server.acknowledge_batch(&client, 64.0);
    server.tick();

    let frames = client.take_frames();
    let chunks = frames_with_id(&frames, clientbound::LEVEL_CHUNK_WITH_LIGHT);
    let resent = chunks
        .iter()
        .find(|frame| chunk_position(frame) == edited)
        .expect("这个区块又进入了视图");
    let store = &server.world.region.chunks;
    assert!(store.edited(edited).is_some(), "这个区块改过了");
    let current = encode_chunk(store, edited);
    assert_eq!(chunk_body(resent), current);
    assert_ne!(chunk_body(resent), flat);

    let neighbour = chunks
        .iter()
        .find(|frame| chunk_position(frame) != edited)
        .expect("这一批不只一个区块");
    assert_eq!(chunk_body(neighbour), flat);
}

#[test]
fn new_player_receives_chunks_edited_before_it_joined() {
    let mut server = Harness::new(8);
    let edited = ChunkPos::new(1, 1);
    let pos = BlockPos::new(17, -60, 17);
    server.world.region.chunks.set_block(pos, blocks::STONE);

    let client = server.join("Kitten");
    server.tick();

    let frames = client.take_frames();
    let chunks = frames_with_id(&frames, clientbound::LEVEL_CHUNK_WITH_LIGHT);
    let chunk = chunks
        .iter()
        .find(|frame| chunk_position(frame) == edited)
        .expect("3×3 里有这个区块");
    assert_ne!(chunk_body(chunk), server.world.region.template_body);
}

#[test]
fn client_information_changes_the_view_distance_on_the_next_tick() {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    client.take_frames();

    server.request(
        &client,
        PlayRequest::ClientInformation(client_requesting(3)),
    );
    let chunks = server.receive_all_chunks(&client);
    // 第一轮没有未确认的批次，配额从确认之后的值开始累积，所以这一批就能发出去。
    assert_eq!(chunks.len(), 81 - 49);

    server.request(
        &client,
        PlayRequest::ClientInformation(client_requesting(2)),
    );
    server.tick();
    let frames = client.take_frames();
    assert_eq!(
        frames_with_id(&frames, clientbound::FORGET_LEVEL_CHUNK).len(),
        81 - 49
    );
    assert!(frames_with_id(&frames, clientbound::SET_CHUNK_CACHE_CENTER).is_empty());
}

#[test]
fn hat_change_broadcasts_update_hat_to_everyone() {
    let mut server = Harness::new(8);
    let watcher = server.join_and_settle("Alice");
    let client = server.join_and_settle("Bob");
    watcher.take_frames();
    client.take_frames();

    let without_hat = ClientInformation {
        model_customisation: 0x3f,
        ..client_requesting(2)
    };
    server.request(&client, PlayRequest::ClientInformation(without_hat.clone()));
    server.tick();

    for receiver in [&watcher, &client] {
        let frames = receiver.take_frames();
        // 皮肤外层也变了，所以还有一个 `set_entity_data`。
        let updates = frames_with_id(&frames, clientbound::PLAYER_INFO_UPDATE);
        assert_eq!(updates.len(), 1);
        let mut expected = vec![0x80, 0x01];
        expected.extend_from_slice(client.uuid.as_bytes());
        expected.push(0x00);
        assert_eq!(updates[0].body, expected);
    }

    // 内容没变的 `client_information` 不用再广播。
    server.request(&client, PlayRequest::ClientInformation(without_hat));
    server.tick();
    let frames = watcher.take_frames();
    assert!(frames_with_id(&frames, clientbound::PLAYER_INFO_UPDATE).is_empty());
}

#[test]
fn skin_parts_and_main_hand_are_stored_and_sent_in_the_same_tick() {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");

    server.request(
        &client,
        PlayRequest::ClientInformation(client_requesting(2)),
    );
    server.tick();
    assert!(!server.player(&client).sync.metadata_dirty);

    server.request(
        &client,
        PlayRequest::ClientInformation(ClientInformation {
            main_hand: HumanoidArm::Right,
            model_customisation: 0x01,
            ..client_requesting(2)
        }),
    );
    server.tick();

    let player = server.player(&client);
    assert!(!player.sync.metadata_dirty);
    assert_eq!(player.client.main_hand, HumanoidArm::Right);
    assert_eq!(player.client.model_customisation, 0x01);
}

#[test]
fn latency_reaches_the_tab_list_on_the_periodic_broadcast() {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    let ticks_so_far = server.world.global.time().0;
    client.take_frames();

    server
        .inbox
        .send(Inbound::Latency {
            player: client.id,
            millis: 42,
        })
        .unwrap();
    server
        .inbox
        .send(Inbound::Latency {
            player: PlayerId(999),
            millis: 1,
        })
        .unwrap();
    for _ in ticks_so_far..600 {
        server.tick();
        assert!(client.take_frames().is_empty());
    }
    server.tick();

    let frames = client.take_frames();
    assert_eq!(frames.len(), 1);
    let mut expected = vec![0x10, 0x01];
    expected.extend_from_slice(client.uuid.as_bytes());
    expected.push(42);
    assert_eq!(frames[0].body, expected);
}

#[test]
fn shutdown_disconnects_everyone_and_stops() {
    let mut server = Harness::new(8);
    let first = server.join_and_settle("Alice");
    let second = server.join_and_settle("Bob");
    first.take_frames();
    second.take_frames();

    server.inbox.send(Inbound::Shutdown).unwrap();

    assert!(server.world.tick().is_break());
    let reason = Component::translate("multiplayer.disconnect.server_shutdown", []);
    let expected = Frame {
        id: clientbound::DISCONNECT,
        body: body_of(encode_frames(&Disconnect { reason: &reason })),
    };
    for client in [&first, &second] {
        assert_eq!(client.take_frames(), std::slice::from_ref(&expected));
        assert!(client.is_closed());
    }
}

#[test]
fn messages_before_shutdown_still_run_in_order() {
    let mut server = Harness::new(8);
    let client = server.join("Kitten");
    server.confirm_teleport(&client);
    server.inbox.send(Inbound::Shutdown).unwrap();

    assert!(server.world.tick().is_break());

    let ids = client.take_ids();
    assert_eq!(ids.first(), Some(&clientbound::LOGIN));
    assert_eq!(ids.last(), Some(&clientbound::DISCONNECT));
    assert!(client.is_closed());
}

#[test]
fn losing_every_sender_stops_the_world_like_a_shutdown() {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    client.take_frames();

    let Harness {
        mut world, inbox, ..
    } = server;
    drop(inbox);

    assert!(world.tick().is_break());
    assert_eq!(client.take_ids(), [clientbound::DISCONNECT]);
    assert!(client.is_closed());
}

#[test]
fn run_returns_after_shutdown() {
    let mut server = Harness::new(2);
    let client = server.join("Kitten");
    server.inbox.send(Inbound::Shutdown).unwrap();

    server.world.run();

    assert_eq!(client.take_ids().last(), Some(&clientbound::DISCONNECT));
}

#[test]
fn world_time_advances_once_per_tick() {
    let mut server = Harness::new(2);

    for _ in 0..5 {
        server.tick();
    }

    assert_eq!(server.world.global.time().0, 5);
    assert_eq!(server.world.region.block_ticks.now(), 5);
    assert_eq!(server.world.region.tick.since(Default::default()), 5);
}

#[test]
fn every_decoded_request_reaches_a_module_without_disconnecting() {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    client.take_frames();
    let pos = BlockPos::new(1, -61, 1);

    let requests = [
        PlayRequest::unsigned_chat("hi"),
        PlayRequest::unsigned_command("foo"),
        PlayRequest::ClientCommand {
            action: ClientCommandKind::RequestStats,
        },
        PlayRequest::PlayerInput { flags: 0x21 },
        PlayRequest::PlayerCommand {
            action: PlayerCommandKind::StartSprinting,
        },
        PlayRequest::PlayerAbilities { flags: 0x02 },
        PlayRequest::Punch,
        PlayRequest::SetCarriedItem { slot: 3 },
        PlayRequest::SetCreativeModeSlot {
            slot: 36,
            stack: ItemStack::new(1, 64),
        },
        PlayRequest::PickItemFromBlock {
            pos,
            include_data: false,
        },
        PlayRequest::PlayerAction {
            action: PlayerActionKind::StartDestroyBlock,
            pos,
            face: Direction::Up,
            sequence: 1,
        },
        PlayRequest::UseItemOn {
            hand: Hand::Main,
            pos,
            face: Direction::Up,
            cursor: (0.5, 1.0, 0.5),
            inside: false,
            world_border_hit: false,
            sequence: 2,
        },
        PlayRequest::UseItem {
            hand: Hand::Off,
            sequence: 3,
            yaw: 0.0,
            pitch: 0.0,
        },
        PlayRequest::EditBook {
            slot: -1,
            pages: vec!["hi".to_owned()],
            title: None,
        },
        // 这三种本来不会发到世界线程。
        PlayRequest::KeepAlive(1),
        PlayRequest::PingRequest(1),
        PlayRequest::Other(13),
    ];
    for request in requests {
        server.request(&client, request);
    }
    server.tick();

    assert!(!client.is_closed());
}

#[test]
fn start_fall_flying_is_refused_with_the_current_flags() {
    let mut server = Harness::new(8);
    server.next_id = 7;
    let client = server.join_and_settle("Kitten");
    client.take_frames();

    server.request(
        &client,
        PlayRequest::PlayerCommand {
            action: PlayerCommandKind::StartFallFlying,
        },
    );
    server.tick();

    let frames = client.take_frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].id, clientbound::SET_ENTITY_DATA);
    assert_eq!(frames[0].body, [0x07, 0x00, 0x00, 0x00, 0xff]);

    server
        .world
        .region
        .players
        .get_mut(&client.id)
        .unwrap()
        .sync
        .shared_flags = 0x88;
    server.request(
        &client,
        PlayRequest::PlayerCommand {
            action: PlayerCommandKind::StartFallFlying,
        },
    );
    server.tick();
    let frames = client.take_frames();
    assert_eq!(frames[0].body, [0x07, 0x00, 0x00, 0x08, 0xff]);
}

#[test]
fn schedule_waits_for_the_rest_of_the_tick() {
    let start = Instant::now();
    let mut schedule = Schedule::starting_at(start);

    assert_eq!(
        schedule.after_tick(start + Duration::from_millis(10)),
        Duration::from_millis(40)
    );
    // 上一次 sleep 多睡了 5 ms，下一次就少等 5 ms，误差不累积。
    assert_eq!(
        schedule.after_tick(start + Duration::from_millis(55)),
        Duration::from_millis(45)
    );
}

#[test]
fn schedule_catches_up_after_a_small_delay() {
    let start = Instant::now();
    let mut schedule = Schedule::starting_at(start);

    // 这一 tick 本该在 0 ms 结束，所以 50、100、150、200 ms 这 4 个 tick 都不用等。
    let mut now = start + Duration::from_millis(200);
    for _ in 0..4 {
        assert_eq!(schedule.after_tick(now), Duration::ZERO);
        now += Duration::from_millis(1);
    }
    assert!(!schedule.after_tick(now).is_zero());
}

#[test]
fn schedule_skips_ahead_when_it_is_too_far_behind() {
    let start = Instant::now();
    let mut schedule = Schedule::starting_at(start);

    let now = start + Duration::from_secs(10);
    assert_eq!(schedule.after_tick(now), Duration::ZERO);

    // 不再补那 10 秒，下一 tick 从现在算起。
    assert_eq!(schedule.after_tick(now), Duration::from_millis(50));
}

#[test]
fn join_remembers_the_profile_for_admin_name_lookups() {
    let mut server = Harness::new(2);
    let client = server.join("Kitten");
    server.tick();
    assert_eq!(
        server
            .world
            .global
            .admin
            .known_player("kItTeN")
            .map(|player| player.uuid),
        Some(client.uuid)
    );
}
