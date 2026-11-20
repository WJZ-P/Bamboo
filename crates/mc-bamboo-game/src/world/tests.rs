//! 用假的 [`PlayerSink`] 直接驱动 [`World::tick`]，不开 socket。

use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
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
use crate::{
    InFlight, Outgoing, PlayerSink,
    chunk_encoding::encode_chunk,
    links::encode_frames,
    profiler::{Metric, Phase, Series},
};

mod fuzz;
mod inbound_order;

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
    fn send(&mut self, frames: Outgoing) -> bool {
        let mut state = self.0.lock().unwrap();
        if state.refusing {
            return false;
        }
        state.bytes.extend(frames.to_raw_bytes());
        true
    }

    fn close(&mut self) {
        self.0.lock().unwrap().closed = true;
    }

    fn second_handle(&self) -> Option<Box<dyn PlayerSink>> {
        Some(Box::new(self.clone()))
    }
}

/// 一个包：ID 和后面的内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) id: i32,
    pub(crate) body: Vec<u8>,
}

/// 假客户端。收到的字节按帧拆开，不解释内容。克隆出来的是同一个连接的另一个把手。
#[derive(Clone)]
pub(crate) struct Client {
    pub(crate) id: PlayerId,
    pub(crate) uuid: Uuid,
    sink: FakeSink,
    /// 这名玩家自己的入站队列的发端，和连接层一样。
    inbox: crate::inbound::PlayerSender,
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

/// 显式选的驱动对应的 `island_threads`：齐步 0，分离 1（只有一个岛，多了用不上）。
fn threads_for(drive: crate::island::IslandDrive) -> usize {
    match drive {
        crate::island::IslandDrive::Lockstep => 0,
        crate::island::IslandDrive::Detached => 1,
    }
}

/// 测试里的“连接层”：用内存队列给世界线程发消息。
pub(crate) struct Harness {
    pub(crate) world: World,
    inbox: crate::inbound::InboundSender,
    in_flight: Arc<AtomicUsize>,
    next_id: i32,
    /// 玩家的请求和离开也走主控队列（第 4 步之前「所有消息一条通道」的样子），
    /// 对照测试用。
    single_channel: bool,
}

impl Harness {
    /// 默认走「无限超平坦」测试世界：现有单元测试/oracle 都假设哪里都有地。
    /// `BAMBOO_TEST_LOADED=1` 让原本走无限模式的整套测试改走已加载模式，
    /// 只用于「有多少测试依赖隐式存在」的报告，不改变默认行为。
    pub(crate) fn new(view_distance: i32) -> Self {
        let loaded = std::env::var("BAMBOO_TEST_LOADED").is_ok_and(|value| value == "1");
        Self::build(view_distance, loaded)
    }

    /// 同 [`Self::new`]，但显式只要一个岛（`islands = false`）；线程数照缺省（环境变量
    /// 优先）。给断言一个岛时的内部布局（例如打报告的剖析器跟着家岛）的测试用。
    ///
    /// Like [`Self::new`] but with one island explicitly (`islands = false`);
    /// the thread count follows the default (environment first). For tests
    /// asserting the one-island layout, e.g. the reporting profiler living
    /// on the home island.
    pub(crate) fn new_one_island(view_distance: i32) -> Self {
        let loaded = std::env::var("BAMBOO_TEST_LOADED").is_ok_and(|value| value == "1");
        Self::build_with(
            view_distance,
            loaded,
            crate::island::default_island_threads(),
            false,
        )
    }

    /// 显式指定岛的 tick 线程数（0 = 齐步），不看环境变量。
    /// An explicit island thread count (0 = lockstep), regardless of the environment.
    pub(crate) fn new_with_island_threads(
        view_distance: i32,
        loaded: bool,
        threads: usize,
    ) -> Self {
        Self::build_with(view_distance, loaded, threads, false)
    }

    /// 多岛（第 10 步），已加载模式，显式给岛的 tick 线程数（0 = 齐步），不看环境变量。
    /// Multi-island, loaded mode, with an explicit tick-thread count (0 = lockstep).
    pub(crate) fn new_islands(view_distance: i32, threads: usize) -> Self {
        Self::build_with(view_distance, true, threads, true)
    }

    /// 主控队列的发端，给要在别的线程上发关服的测试用。
    pub(crate) fn inbox_sender(&self) -> crate::inbound::InboundSender {
        self.inbox.clone()
    }

    /// 正式入口（已加载模式）：起点没有加载任何区块，测试自己交付。
    pub(crate) fn new_loaded(view_distance: i32) -> Self {
        Self::build(view_distance, true)
    }

    /// 已加载模式，显式指定驱动（齐步 / 分离），不看环境变量。
    /// Loaded mode with an explicit drive, regardless of the environment.
    pub(crate) fn new_loaded_with_drive(
        view_distance: i32,
        island_drive: crate::island::IslandDrive,
    ) -> Self {
        Self::build_with(view_distance, true, threads_for(island_drive), false)
    }

    /// 已加载模式 + 区块计算池（不接存档）。
    pub(crate) fn new_loaded_with_threads(view_distance: i32, chunk_worker_threads: usize) -> Self {
        let mut harness = Self::build(view_distance, true);
        harness
            .world
            .master
            .host
            .set_compute_threads(chunk_worker_threads);
        harness
    }

    /// 发一条关服消息并跑完这一刻，世界循环必须结束。
    pub(crate) fn shut_down(&mut self) {
        self.inbox.send(Inbound::Shutdown).unwrap();
        assert!(self.world.tick().is_break(), "关服要结束世界循环");
    }

    /// 正式入口 + 真实世界目录：`SaveSession` 和 IO 线程都接上，
    /// 供 `/save-off` / `save-all` / 关服这类生命周期测试用。
    /// Real world directory: both `SaveSession` and the IO thread are attached,
    /// for `/save-off` / `save-all` / shutdown lifecycle tests.
    pub(crate) fn new_loaded_with_save(view_distance: i32, root: &std::path::Path) -> Self {
        Self::new_loaded_with_save_and_threads(view_distance, root, 0)
    }

    /// 同上，另外指定区块计算池的线程数（0 = 内联执行器）。
    pub(crate) fn new_loaded_with_save_and_threads(
        view_distance: i32,
        root: &std::path::Path,
        chunk_worker_threads: usize,
    ) -> Self {
        Self::with_save(
            view_distance,
            root,
            chunk_worker_threads,
            crate::island::default_island_threads(),
            crate::island::default_islands(),
        )
    }

    /// 带存档的多岛世界，显式给岛的 tick 线程数（0 = 齐步）。
    /// A multi-island world with saving and an explicit tick-thread count.
    pub(crate) fn new_islands_with_save(
        view_distance: i32,
        root: &std::path::Path,
        threads: usize,
    ) -> Self {
        Self::with_save(view_distance, root, 0, threads, true)
    }

    /// 同上，另外显式指定驱动（齐步 / 分离），不看环境变量。
    /// The same with an explicit drive, regardless of the environment.
    pub(crate) fn new_loaded_with_save_threads_and_drive(
        view_distance: i32,
        root: &std::path::Path,
        chunk_worker_threads: usize,
        island_drive: crate::island::IslandDrive,
    ) -> Self {
        Self::with_save(
            view_distance,
            root,
            chunk_worker_threads,
            threads_for(island_drive),
            false,
        )
    }

    fn with_save(
        view_distance: i32,
        root: &std::path::Path,
        chunk_worker_threads: usize,
        island_threads: usize,
        islands: bool,
    ) -> Self {
        let (inbox, receiver) = crate::inbound::channel();
        let config = WorldConfig {
            view_distance,
            world_dir: Some(root.to_path_buf()),
            chunk_worker_threads,
            island_threads,
            islands,
            ..Default::default()
        };
        let mut world = World::new(config, receiver);
        // 和别的 Harness 一样：这些测试断言固定出生位置，按官方规则设半径 0。
        // Like the other Harnesses: these tests assert a fixed spawn position,
        // so set the official rule to radius 0.
        let _ = world
            .master
            .global
            .rules_mut()
            .assign(mc_bamboo_data::gamerules::RESPAWN_RADIUS, "0");
        world.publish_global();
        world.islands.region.random = LevelRandom::new(0);
        world.islands.region.sound_seed_generator = LevelRandom::new(0);
        Self {
            world,
            inbox,
            in_flight: Arc::default(),
            next_id: 1,
            single_channel: false,
        }
    }

    fn build(view_distance: i32, loaded: bool) -> Self {
        Self::build_with(
            view_distance,
            loaded,
            crate::island::default_island_threads(),
            crate::island::default_islands(),
        )
    }

    fn build_with(view_distance: i32, loaded: bool, island_threads: usize, islands: bool) -> Self {
        let (inbox, receiver) = crate::inbound::channel();
        let config = WorldConfig {
            view_distance,
            max_players: 20,
            online_mode: false,
            enforce_secure_profile: false,
            admin: Default::default(),
            island_threads,
            islands,
            ..Default::default()
        };
        let mut world = if loaded {
            World::new(config, receiver)
        } else {
            World::new_infinite_flat_for_tests(config, receiver)
        };
        // 已加载模式的测试自己搭场景：把出生点所在区块先交付，和官方
        // `PrepareSpawnTask` 一样在进服前就有落脚点可读。半径已被设成 0，
        // 这里只需一个区块。
        // Loaded-mode tests build their own scene: deliver the spawn chunk
        // before joining, like vanilla `PrepareSpawnTask`. Radius is 0, so
        // one chunk suffices.
        // 官方缺省 `respawn_radius = 10` 会让新玩家随机落在出生点附近；这些
        // 测试断言的是固定出生位置，所以按官方规则显式把半径设成 0（不是
        // 在生产代码里给测试开后门）。散布本身另有专门的单测 / 对拍。
        // Vanilla defaults `respawn_radius` to 10 and scatters new players;
        // these tests assert a fixed position, so set the official rule to 0.
        // Scatter itself has dedicated unit tests / oracle cases.
        let _ = world
            .master
            .global
            .rules_mut()
            .assign(mc_bamboo_data::gamerules::RESPAWN_RADIUS, "0");
        world.publish_global();
        // 正式启动的随机种子来自时钟，测试要可重复，固定成 0。音效种子会进包，也一样固定。
        world.islands.region.random = LevelRandom::new(0);
        world.islands.region.sound_seed_generator = LevelRandom::new(0);
        Self {
            world,
            inbox,
            in_flight: Arc::default(),
            next_id: 1,
            single_channel: false,
        }
    }

    /// 发出 `Join`，这一 tick 才处理。
    pub(crate) fn join_with(&mut self, name: &str, client: ClientInformation) -> Client {
        let id = PlayerId(self.next_id);
        self.next_id += 1;
        let sink = FakeSink::default();
        let uuid = Uuid::from_u128(id.0 as u128);
        let (player_sender, player_inbox) = self.inbox.player_channel();
        let request = JoinRequest {
            id,
            uuid,
            name: name.to_owned(),
            properties: Vec::new(),
            client,
            permission_level: 0,
            is_op: false,
            peer_ip: "127.0.0.1".to_owned(),
            online_at_login: 0,
            player_data: None,
            sink: Box::new(sink.clone()),
            inbox: player_inbox,
        };
        self.inbox.send(Inbound::Join(Box::new(request))).unwrap();
        Client {
            id,
            uuid,
            sink,
            inbox: player_sender,
        }
    }

    pub(crate) fn join(&mut self, name: &str) -> Client {
        self.join_with(name, client_requesting(2))
    }

    /// 用指定 UUID 进服，测试重连时用它。
    ///
    /// Join with a given UUID, used by reconnect tests.
    pub(crate) fn join_uuid(&mut self, name: &str, uuid: Uuid) -> Client {
        self.join_uuid_with_data(name, uuid, None)
    }

    /// 用指定 UUID 和存档字节进服，测试老玩家恢复位置时用。
    /// Join with a given UUID and save bytes, for restoring an old player's position.
    pub(crate) fn join_uuid_with_data(
        &mut self,
        name: &str,
        uuid: Uuid,
        player_data: Option<Vec<u8>>,
    ) -> Client {
        let id = PlayerId(self.next_id);
        self.next_id += 1;
        let sink = FakeSink::default();
        let (player_sender, player_inbox) = self.inbox.player_channel();
        let request = JoinRequest {
            id,
            uuid,
            name: name.to_owned(),
            properties: Vec::new(),
            client: client_requesting(2),
            permission_level: 0,
            is_op: false,
            peer_ip: "127.0.0.1".to_owned(),
            online_at_login: 0,
            player_data,
            sink: Box::new(sink.clone()),
            inbox: player_inbox,
        };
        self.inbox.send(Inbound::Join(Box::new(request))).unwrap();
        Client {
            id,
            uuid,
            sink,
            inbox: player_sender,
        }
    }

    /// 直接改世界出生点（`LevelData` 里的那份），测试出生点包用。
    /// Set the world spawn (the one in `LevelData`), for spawn-packet tests.
    pub(crate) fn set_spawn(&mut self, spawn: crate::world_state::RespawnData) {
        let _ = self.world.master.global.set_spawn(spawn);
    }

    pub(crate) fn request(&self, client: &Client, request: PlayRequest) {
        let in_flight = InFlight::acquire(&self.in_flight, QUOTA).expect("测试不会用完配额");
        self.send_player_message(
            client,
            Inbound::Request {
                player: client.id,
                request,
                in_flight,
            },
        );
    }

    pub(crate) fn leave(&self, client: &Client) {
        self.send_player_message(client, Inbound::Leave { player: client.id });
    }

    /// 一名玩家的请求和离开：照连接层走他自己的队列；对照模式下走主控队列。
    /// 世界已经扔掉这名玩家的队列（离开之后）时，和连接层一样不算错。
    ///
    /// A player's request or leave: through their own queue like the connection
    /// layer, or through the master queue in comparison mode. Like the
    /// connection layer, a queue the world has already dropped is not an error.
    fn send_player_message(&self, client: &Client, message: Inbound) {
        if self.single_channel {
            self.inbox.send(message).unwrap();
        } else {
            let _ = client.inbox.send(message);
        }
    }

    /// 对照模式：之后玩家的请求和离开都走主控队列（第 4 步之前的单通道）。
    pub(crate) fn use_single_channel(&mut self) {
        self.single_channel = true;
    }

    pub(crate) fn tick(&mut self) {
        // 正式运行的节拍器在两刻之间留 50 ms，足够 IO 线程把上一刻的读回执
        // 全部送回信箱；测试的 tick 是背靠背的，这里显式做一次同步点，
        // 让「下一刻开头 poll_io 收齐整批读」的时序不依赖机器负载。
        // The real pacer leaves 50 ms between ticks, enough for the IO thread
        // to deliver all read replies. Test ticks are back-to-back, so add an
        // explicit sync point to keep the ordering deterministic.
        self.world.master.host.flush_io();
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
        // 多岛时玩家不一定在家岛上：在他所在的岛上放置。
        // With several islands the player may be off the home island: place
        // them on theirs.
        let island = self
            .world
            .islands
            .island_of_player(client.id)
            .unwrap_or(mc_bamboo_world::director::HOME_REGION);
        let here = self
            .world
            .islands
            .get(island)
            .and_then(|island| island.region.players.get(&client.id))
            .map(|player| player.position());
        if let Some(here) = here
            && !client.is_closed()
            && position.subtract(here).length_sqr() > 100.0
        {
            let region = &mut self.world.islands.get_mut(island).expect("停放着").region;
            crate::player_move::place_player(region, client.id, position);
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

    /// 一直回确认，直到没有新区块、也没有还等着岛收取的区块信件，返回收到的全部区块。
    /// 齐步驱动下每刻结束时邮筒总是空的，和只看「这一刻有没有新批次」相同；分离驱动下
    /// 交付晚一刻，进服那一刻还没有区块可发，要等岛下一刻开头把信收走。
    ///
    /// Keep acknowledging until a tick brings no batch and no chunk mail is
    /// still waiting for the island. In lockstep the postbox is always empty at
    /// the end of a tick, so this is the old condition; under the detached drive
    /// delivery is one tick late and the join tick has nothing to send yet.
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
            if finished == 0 && self.world.master.host.posted_len() == 0 {
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

    /// 让 `positions` 里的区块真的加载：走正式的加票 → 生成流水线 → 交付路径。
    /// 无限超平坦模式下每个位置本来就存在，直接返回；已加载模式下用 `Forced`
    /// 票（不过期）保持区块，再按需要推进几刻等交付预算把它们发到 region。
    pub(crate) fn ensure_loaded(&mut self, positions: &[ChunkPos]) {
        if !self.world.islands.region.chunks.is_loaded_mode() {
            return;
        }
        for &pos in positions {
            self.apply_ticket_for_test(mc_bamboo_world::director::TicketOp::Forced {
                pos,
                forced: true,
            });
        }
        // 交付每刻有个数上限，脚下 3×3 豁免；这里循环到全部到位为止。多岛时远处的区块交付
        // 给它自己的岛，不一定是家岛，所以按任意一个岛已加载算。
        // Deliveries are capped per tick (the 3×3 footprint is exempt), so
        // loop until all are in. With several islands a far chunk goes to its
        // own island, not necessarily home, so any island counts.
        for _ in 0..64 {
            if positions
                .iter()
                .all(|&pos| self.world.islands.island_of_chunk(pos).is_some())
            {
                return;
            }
            self.tick();
        }
        panic!("测试要求交付的区块没能加载：{positions:?}");
    }

    /// 测试专用：只读票表。
    pub(crate) fn tickets(&self) -> &mc_bamboo_world::ticket::TicketTable {
        self.world.tickets()
    }

    /// 测试专用：直接改票表（视距、模拟距离、超时票等）。
    pub(crate) fn tickets_mut(&mut self) -> &mut mc_bamboo_world::ticket::TicketTable {
        self.world.tickets_mut()
    }

    /// 测试专用：走一批 `TicketOp`。
    pub(crate) fn apply_ticket_for_test(&mut self, op: mc_bamboo_world::director::TicketOp) {
        self.world.apply_ticket(op);
    }

    /// 测量用：清零剖析器样本（只重置样本，不动报告间隔）。
    #[cfg(test)]
    pub(crate) fn clear_profile_samples(&mut self) {
        self.world.islands.region.profiler.reset_samples();
    }

    /// 测量用：取剖析器某个阶段/指标的样本。
    #[cfg(test)]
    pub(crate) fn profile_samples(
        &self,
    ) -> std::collections::BTreeMap<&'static str, (u64, u128, u64, u64, u64)> {
        use crate::profiler::{Phase, Series};
        let mut out = std::collections::BTreeMap::new();
        for (name, series) in [
            ("tick_total", Series::Metric(Metric::TickTotal)),
            ("delivery", Series::Phase(Phase::Delivery)),
            ("chunk_sending", Series::Phase(Phase::ChunkSending)),
            ("chunks_delivered", Series::Metric(Metric::ChunksDelivered)),
        ] {
            let h = self.world.islands.region.profiler.histogram(series);
            out.insert(name, (h.count(), h.sum(), h.p50(), h.p99(), h.max()));
        }
        out
    }

    /// 给其他模块的测试直接看全服状态。
    pub(crate) fn global(&self) -> &crate::global::Global {
        &self.world.master.global
    }

    /// 区块帧缓存里的共享帧，按坐标取。版本号变了就返回 `None`。
    pub(crate) fn shared_chunk_frame(&self, pos: ChunkPos) -> Option<Arc<crate::SharedFrames>> {
        self.world
            .islands
            .region
            .chunk_cache
            .frame(&self.world.islands.region.chunks, pos)
    }

    /// 缓存里的区块帧条目数。
    pub(crate) fn cached_frame_count(&self) -> usize {
        self.world.islands.region.chunk_cache.frame_count()
    }

    /// 缓存里有没有空帧。
    pub(crate) fn has_empty_cached_frame(&self) -> bool {
        self.world.islands.region.chunk_cache.has_empty_frame()
    }

    /// 给其他模块的测试直接看世界的状态。
    /// 只读 region（测试断言用）。
    pub(crate) fn world_region(&self) -> &Region {
        &self.world.islands.region
    }

    /// 玩家所在的岛的 `Region`（只有一个岛时就是家岛）。
    ///
    /// The `Region` of the island the player is on (home with one island).
    pub(crate) fn player_region(&mut self, client: &Client) -> &mut Region {
        let island = self
            .world
            .islands
            .island_of_player(client.id)
            .unwrap_or(mc_bamboo_world::director::HOME_REGION);
        &mut self.world.islands.get_mut(island).expect("停放着").region
    }

    /// 回到原处以后跑到 `pos` 的区块发来为止，收下这期间的全部帧。这一步不跨岛时（一个岛，
    /// 或者多岛但玩家还在新位置所在的岛上）只给一刻，当刻就要到；跨岛时（玩家现在的岛不是
    /// 新位置所在的岛）离境和到达晚一刻（总设计稿第 13 节第 11 条第 ② 项），给两刻。超过
    /// 就不再跑，调用方找不到这个区块就失败。
    ///
    /// After coming back, run until the chunk at `pos` arrives, collecting
    /// every frame. A step that stays on one island (one island, or several
    /// with the player already on the island of the new spot) gets one tick
    /// and must deliver that tick; a step that crosses islands (the player's
    /// island is not the new spot's) departs and arrives a tick late (design
    /// section 13, item 11 ②) and gets two. No more ticks are run, so the
    /// caller fails if the chunk is not there.
    pub(crate) fn tick_until_chunk(&mut self, client: &Client, pos: ChunkPos) -> Vec<Frame> {
        let here = self.player(client).chunk_pos();
        let island = self.world.islands.island_of_player(client.id);
        let target = self.world.master.host.island_of(here);
        let crossing = island.is_some() && target.is_some() && island != target;
        let ticks = if crossing { 2 } else { 1 };
        let mut frames = Vec::new();
        for _ in 0..ticks {
            self.acknowledge_batch(client, 64.0);
            self.tick();
            frames.extend(client.take_frames());
            if frames_with_id(&frames, clientbound::LEVEL_CHUNK_WITH_LIGHT)
                .iter()
                .any(|frame| chunk_position(frame) == pos)
            {
                break;
            }
        }
        frames
    }

    pub(crate) fn region(&mut self) -> &mut Region {
        &mut self.world.islands.region
    }

    pub(crate) fn island(&mut self) -> &mut crate::island::Island {
        &mut self.world.islands
    }

    /// 测试并行驱动时直接设线程数，不碰进程级环境变量（同一进程里测试并行跑）。
    pub(crate) fn set_entity_threads(&mut self, threads: usize) {
        self.world.islands.entity_threads = threads;
    }

    /// 已加载模式的测试专用：本刻交付/收回的个数、信箱里还欠着的个数（含邮筒里
    /// 还没被岛收取的；齐步下每刻结束时邮筒是空的）。
    pub(crate) fn director_mail(&self) -> (usize, usize, (usize, usize)) {
        let mailbox = &self.world.islands.mailbox;
        let (delivered, reclaimed) = mailbox.counters_this_tick();
        let (queued_deliveries, queued_reclaims) = mailbox.backlog();
        let (posted_deliveries, posted_reclaims) = self.world.master.host.posted_counts();
        (
            delivered,
            reclaimed,
            (
                queued_deliveries + posted_deliveries,
                queued_reclaims + posted_reclaims,
            ),
        )
    }

    /// 这个世界走分离驱动。
    pub(crate) fn detached(&self) -> bool {
        self.world.master.drive == crate::island::IslandDrive::Detached
    }

    /// 分离驱动下区块晚一刻交付：邮筒里还有信就再跑一刻，让岛在开头收走。
    /// 齐步下每刻结束时邮筒总是空的，这里什么都不做。只用在「交付晚一刻」要调整的断言前。
    ///
    /// Under the detached drive chunks arrive one tick late: if letters are
    /// still posted, run one more tick so the island takes them at its start.
    /// In lockstep the postbox is always empty at the end of a tick, so this
    /// does nothing. Used only before assertions adjusted for the late delivery.
    pub(crate) fn deliver_posted(&mut self) {
        if self.world.master.host.posted_len() > 0 {
            self.tick();
        }
    }

    /// 脚下 3×3 的一格「已经到位」：已交付；或者（分离驱动下）盒子已经投进邮筒，
    /// 或者岛这一刻开头收下时玩家还没走到这里、按预算排在了信箱里——两种都在岛下一刻
    /// 开头按玩家的新位置豁免预算交付。齐步下收信时玩家已经在这里，就是「已交付」。
    ///
    /// A footprint chunk is in place: delivered; or (detached) its box is posted,
    /// or was taken at the start of this tick before the player got here and
    /// waits in the mailbox under the budget. Either way the island delivers it
    /// at its next tick start, exempt from the budget for the player's new
    /// position. In lockstep the player is already here when mail is taken, so
    /// this is just "delivered".
    pub(crate) fn footprint_ready(&self, pos: ChunkPos) -> bool {
        self.world.islands.region.chunks.is_loaded(pos)
            || self.world.master.host.has_posted_delivery(pos)
            || (self.detached() && self.world.islands.mailbox.has_delivery(pos))
    }

    /// 这一刻从 region 取回的区块数（不含直接从信箱退回的盒子）。
    pub(crate) fn region_reclaims_this_tick(&self) -> usize {
        self.world.islands.mailbox.region_reclaims_this_tick()
    }

    /// 测试专用：把每名玩家的加载/生成在途上限改小，模拟慢生成时的积压。
    pub(crate) fn set_demand_caps(&mut self, loads: usize, generates: usize) {
        let demand = self.world.master.host.director().player_demand();
        let snapshot = mc_bamboo_world::director::DemandSnapshot::with_caps(
            demand.players().to_vec(),
            loads,
            generates,
        );
        self.world.master.host.set_player_demand(snapshot);
    }

    /// 测试专用：改这一刻之后的交付/收回上限（不碰进程级环境变量）。
    pub(crate) fn set_mail_budget(&mut self, budget: crate::director_host::MailBudget) {
        self.world.islands.mailbox.set_budget(budget);
    }

    /// 测试专用：Director 里已交付/在途的区块数。
    #[cfg(test)]
    pub(crate) fn director_loaded_count(&self) -> usize {
        self.world.master.host.director().loaded_count()
    }

    /// 测试专用：停放表里的盒子数。
    #[cfg(test)]
    pub(crate) fn parked_count(&self) -> usize {
        self.world.master.host.parked_count()
    }

    /// 测试专用：region 里已加载的区块坐标（按坐标排序）。
    #[cfg(test)]
    pub(crate) fn loaded_positions(&self) -> Vec<mc_bamboo_core::ChunkPos> {
        self.world
            .islands
            .region
            .chunks
            .edited_chunks()
            .map(|(pos, _)| pos)
            .filter(|&pos| self.world.islands.region.chunks.is_loaded(pos))
            .collect()
    }

    /// 测试专用：Director 里每条记录的归属名和代数。
    #[cfg(test)]
    pub(crate) fn director_records(
        &self,
    ) -> std::collections::BTreeMap<mc_bamboo_core::ChunkPos, (String, u64)> {
        self.world
            .master
            .host
            .director()
            .debug_state()
            .records()
            .iter()
            .map(|(&pos, record)| (pos, (format!("{:?}", record.owner), record.generation)))
            .collect()
    }

    /// 测试专用：把失去票之后的逗留改成 `delay` 刻，用来在短测试里看收回。
    pub(crate) fn set_reclaim_delay(&mut self, delay: u64) {
        self.world.master.host.set_reclaim_delay(delay);
    }

    /// 给世界里直接塞一个实体，绕过玩家动作。
    pub(crate) fn spawn_entity(&mut self, entity: mc_bamboo_blocks::entity::Entity) -> i32 {
        let (mut level, ticks) = self.world.islands.region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        world.spawn_entity(entity)
    }

    /// 给世界里直接写一个方块状态，绕过玩家动作。
    pub(crate) fn set_block(&mut self, pos: BlockPos, state: u32) {
        self.world.islands.region.chunks.set_block(pos, state);
    }

    /// 玩家本人。多岛时在他所在的岛上找（只有一个岛时就是家岛）。
    ///
    /// The player; with several islands, on whichever island they are.
    pub(crate) fn player(&self, client: &Client) -> &crate::player::Player {
        self.world
            .islands
            .iter()
            .find_map(|island| island.region.players.get(&client.id))
            .expect("玩家应该在世界里")
    }
}

#[test]
fn join_writes_the_ten_steps_in_order_in_one_tick() {
    let mut server = Harness::new(8);
    let client = server.join("Kitten");

    server.tick();
    // 分离驱动下第一批区块晚一刻：前面的进服序列不变，区块批次接在下一刻。
    // Detached: the join sequence is unchanged and the first batch follows next tick.
    server.deliver_posted();

    let mut expected = vec![
        clientbound::LOGIN,
        clientbound::CHANGE_DIFFICULTY,
        clientbound::PLAYER_ABILITIES,
        clientbound::ENTITY_EVENT,
        clientbound::COMMANDS,
        clientbound::PLAYER_POSITION,
        clientbound::PLAYER_INFO_UPDATE,
        clientbound::SET_ENTITY_DATA,
        clientbound::UPDATE_ATTRIBUTES,
        clientbound::INITIALIZE_BORDER,
        clientbound::SET_TIME,
        clientbound::SET_DEFAULT_SPAWN_POSITION,
        clientbound::GAME_EVENT,
        clientbound::SET_CHUNK_CACHE_CENTER,
        // 官方 `placeNewPlayer` 不发这两包；`ServerPlayer.doTick` 在进服这一刻
        // 发现 `lastSentHealth` 和 `lastSentExp` 还是构造时的哨兵值，
        // 先补 `set_health`，再补 `set_experience`。
        //
        // Official `placeNewPlayer` sends neither; `doTick` sends health first
        // then experience on the join tick because both snapshots are sentinels.
        clientbound::SET_HEALTH,
        clientbound::SET_EXPERIENCE,
        clientbound::CHUNK_BATCH_START,
    ];
    expected.extend([clientbound::LEVEL_CHUNK_WITH_LIGHT; 9]);
    expected.push(clientbound::CHUNK_BATCH_FINISHED);
    assert_eq!(client.take_ids(), expected);
}

/// 老玩家存档里的位置和世界出生点不同：官方 `set_default_spawn_position` 发
/// `LevelData` 里的世界出生点，玩家位置包（`teleport` / `player_position`）照存档，
/// 区块中心照玩家位置。三个包分别核对。
/// An old player's saved position differs from the world spawn: vanilla's
/// `set_default_spawn_position` sends the world spawn from `LevelData`, the
/// player-position packet follows the save, and the chunk center follows the
/// player position. Check each of the three packets.
#[test]
fn old_player_spawn_packet_uses_world_spawn_and_position_uses_save() {
    use mc_bamboo_storage::player_data::PlayerData;

    let mut server = Harness::new(8);
    // 世界出生点：一个和存档位置、朝向都不同的值。
    let world_spawn =
        crate::world_state::RespawnData::of(BlockPos::new(1000, 70, -1000), 90.0, 30.0);
    server.set_spawn(world_spawn);

    // 老玩家存档：位置、朝向都和世界出生点不同。
    let saved = PlayerData {
        position: [101.0, -60.0, 202.0],
        rotation: [45.0, 20.0],
        ..PlayerData::default()
    };
    let bytes = saved.to_gzip().unwrap();
    let client = server.join_uuid_with_data("OldTimer", Uuid::from_u128(0xabcd), Some(bytes));
    server.tick();
    let frames = client.take_frames();

    // 1) set_default_spawn_position = 世界出生点，不是玩家存档位置。
    let spawn = frames_with_id(&frames, clientbound::SET_DEFAULT_SPAWN_POSITION);
    assert_eq!(spawn.len(), 1);
    let mut r = Reader::new(&spawn[0].body);
    assert_eq!(r.string(32767).unwrap(), "minecraft:overworld");
    assert_eq!(r.i64().unwrap(), world_spawn.pos.pack());
    assert_eq!(r.f32().unwrap(), world_spawn.yaw);
    assert_eq!(r.f32().unwrap(), world_spawn.pitch);
    // 和存档位置确实不同，证明这里发的不是玩家自己的坐标。
    assert_ne!(world_spawn.pos.pack(), BlockPos::new(101, -60, 202).pack());

    // 2) player_position（teleport）= 存档位置（方块中心 +0.5）和存档朝向。
    let positions = frames_with_id(&frames, clientbound::PLAYER_POSITION);
    assert_eq!(positions.len(), 1);
    let mut r = Reader::new(&positions[0].body);
    let _teleport_id = r.varint().unwrap();
    let x = r.f64().unwrap();
    let _y = r.f64().unwrap();
    let z = r.f64().unwrap();
    let _velocity = r.bytes(24).unwrap();
    let yaw = r.f32().unwrap();
    let pitch = r.f32().unwrap();
    assert_eq!(x, 101.5);
    assert_eq!(z, 202.5);
    assert_eq!(yaw, 45.0);
    assert_eq!(pitch, 20.0);

    // 3) set_chunk_cache_center = 玩家所在区块（存档位置），不是世界出生点区块。
    let centers = frames_with_id(&frames, clientbound::SET_CHUNK_CACHE_CENTER);
    assert_eq!(centers.len(), 1);
    let mut r = Reader::new(&centers[0].body);
    let center = ChunkPos::new(r.varint().unwrap(), r.varint().unwrap());
    assert_eq!(center, ChunkPos::containing(101.5, 202.5));
    assert_ne!(center, ChunkPos::containing(1000.0, -1000.0));
}

#[test]
fn second_player_gets_the_roster_first_and_the_others_get_the_new_player() {
    let mut server = Harness::new(8);
    let first = server.join_and_settle("Alice");
    first.take_frames();

    let second = server.join("Bob");
    server.tick();

    let frames = second.take_frames();
    let ids: Vec<i32> = frames.iter().take(9).map(|frame| frame.id).collect();
    assert_eq!(
        ids,
        [
            clientbound::LOGIN,
            clientbound::CHANGE_DIFFICULTY,
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
    // 难度包和权限事件/命令树插在能力包和传送前后，所以名单从第 7 个包开始。
    //
    // The difficulty packet, permission event and command tree come before the roster,
    // so it starts at the seventh packet.
    for (frame, uuid) in [(&frames[6], first.uuid), (&frames[7], second.uuid)] {
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

/// 官方 `PlayerList.placeNewPlayer`：进服的 login 包里 `showDeathScreen` 是
/// `!IMMEDIATE_RESPAWN`；规则只通知客户端，不让服务端自己重生。
///
/// Vanilla `PlayerList.placeNewPlayer`: the login packet's `showDeathScreen`
/// is `!IMMEDIATE_RESPAWN`. The rule only notifies the client; the server
/// never respawns on its own.
#[test]
fn login_reports_the_immediate_respawn_rule_as_show_death_screen() {
    for (immediate, show_death_screen) in [(false, true), (true, false)] {
        let mut server = Harness::new(8);
        let _ = server.world.master.global.rules_mut().assign(
            mc_bamboo_data::gamerules::IMMEDIATE_RESPAWN,
            if immediate { "true" } else { "false" },
        );
        server.world.publish_global();
        let client = server.join("Kitten");
        server.tick();

        let frames = client.take_frames();
        let login = frames_with_id(&frames, clientbound::LOGIN);
        assert_eq!(login.len(), 1);
        let mut reader = Reader::new(&login[0].body);
        reader.i32().unwrap(); // 实体 ID
        reader.bool().unwrap(); // 极限模式
        reader.varint().unwrap(); // 维度个数
        reader.string(32767).unwrap();
        reader.varint().unwrap(); // 最大玩家数
        reader.varint().unwrap(); // 视距
        reader.varint().unwrap(); // 模拟距离
        reader.bool().unwrap(); // 隐藏调试信息
        assert_eq!(
            reader.bool(),
            Ok(show_death_screen),
            "immediate_respawn={immediate}"
        );
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
    // 分离驱动下区块晚一刻交付（岛的实施计划第 6 节第 6 步）。
    // Under the detached drive chunks arrive one tick late (island plan step 6).
    server.deliver_posted();

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

/// 把夜里睡觉的场景搭好：完整的床、站在床尾的玩家、午夜 + 雷暴。
fn sleep_scene() -> (Harness, Client, BlockPos, BlockPos) {
    use mc_bamboo_blocks::{BlockState, state::BED_PART, state::BedPart};
    use mc_bamboo_core::Vec3;

    let mut server = Harness::new_one_island(8);
    let client = server.join_and_settle("Kitten");
    let foot = BlockPos::new(8, -59, 8);
    let head = foot.south();
    let bed = BlockState(mc_bamboo_data::blocks::RED_BED)
        .with(mc_bamboo_blocks::state::HORIZONTAL_FACING, Direction::South);
    server.world.islands.region.chunks.set_block(foot, bed.0);
    server
        .world
        .islands
        .region
        .chunks
        .set_block(head, bed.with(BED_PART, BedPart::Head).0);
    let region = &mut server.world.islands.region;
    if let Some(player) = region.players.get_mut(&client.id) {
        player.set_position(&mut region.player_grid, Vec3::new(8.5, -59.0, 8.5));
    }
    {
        let global = &mut server.world.master.global;
        global
            .clock_named_mut("minecraft:overworld")
            .unwrap()
            .total_ticks = 18_000;
        let weather = &mut global.state_mut().weather;
        weather.raining = true;
        weather.thundering = true;
        weather.rain_level = 1.0;
        weather.o_rain_level = 1.0;
        weather.thunder_level = 1.0;
        weather.o_thunder_level = 1.0;
    }
    server.world.publish_global();
    assert!(
        server.world.master.global.sky_darken() >= 4,
        "夜里天色要够暗"
    );
    client.take_frames();
    crate::interaction::on_use_item_on(
        &mut server.world.islands.region,
        client.id,
        crate::interaction::UseItemOn {
            hand: Hand::Main,
            pos: foot,
            face: Direction::Up,
            cursor: (0.5, 0.5, 0.5),
            inside: false,
            sequence: 1,
        },
    );
    assert!(
        server.world.islands.region.players[&client.id]
            .sleep
            .is_some(),
        "玩家应该睡着了"
    );
    client.take_frames();
    (server, client, foot, head)
}

/// 午夜晴天、站在床尾，`prepare` 摆好怪物 / 改好模式之后右键床尾。
/// 返回玩家睡着没有，以及这次右键收到的包里有没有 `block.minecraft.bed.not_safe`。
fn try_sleep_with(prepare: impl FnOnce(&mut crate::region::Region, PlayerId)) -> (bool, bool) {
    use mc_bamboo_blocks::{BlockState, state::BED_PART, state::BedPart};
    use mc_bamboo_core::Vec3;

    let mut server = Harness::new_one_island(8);
    let client = server.join_and_settle("Kitten");
    let foot = BlockPos::new(8, -59, 8);
    let head = foot.south();
    let bed = BlockState(mc_bamboo_data::blocks::RED_BED)
        .with(mc_bamboo_blocks::state::HORIZONTAL_FACING, Direction::South);
    server.world.islands.region.chunks.set_block(foot, bed.0);
    server
        .world
        .islands
        .region
        .chunks
        .set_block(head, bed.with(BED_PART, BedPart::Head).0);
    let region = &mut server.world.islands.region;
    if let Some(player) = region.players.get_mut(&client.id) {
        player.set_position(&mut region.player_grid, Vec3::new(8.5, -59.0, 8.5));
    }
    server
        .world
        .master
        .global
        .clock_named_mut("minecraft:overworld")
        .unwrap()
        .total_ticks = 18_000;
    server.world.publish_global();
    if let Some(player) = server.world.islands.region.players.get_mut(&client.id) {
        player.game_mode = mc_bamboo_protocol::packet::play::GameMode::Survival;
    }
    prepare(&mut server.world.islands.region, client.id);
    client.take_frames();
    crate::interaction::on_use_item_on(
        &mut server.world.islands.region,
        client.id,
        crate::interaction::UseItemOn {
            hand: Hand::Main,
            pos: foot,
            face: Direction::Up,
            cursor: (0.5, 0.5, 0.5),
            inside: false,
            sequence: 1,
        },
    );
    server.world.islands.region.links.flush();
    let sleeping = server.world.islands.region.players[&client.id]
        .sleep
        .is_some();
    let needle = b"block.minecraft.bed.not_safe";
    let warned = client.take_frames().iter().any(|frame| {
        frame
            .body
            .windows(needle.len())
            .any(|window| window == needle)
    });
    (sleeping, warned)
}

/// 放一只僵尸；`dying` 时血量是 0（倒地动画里，还没被移除）。
fn place_zombie(region: &mut crate::region::Region, x: f64, y: f64, z: f64, dying: bool) -> i32 {
    let mut zombie = mc_bamboo_blocks::entity::Entity::zombie(mc_bamboo_core::Vec3::new(x, y, z));
    if dying && let Some(mob) = zombie.kind.mob_mut() {
        mob.health = 0.0;
    }
    region.entities.spawn(zombie)
}

/// `ServerPlayer.startSleepInBed`：床头底面中心 ±8 / ±5 / ±8 的盒里有怪物就睡不着，
/// 提示 `block.minecraft.bed.not_safe`。床头是 (8, -59, 9)，盒是 x、z 各 [0.5, 16.5]、
/// y [-64, -54]。
#[test]
fn a_zombie_near_the_bed_keeps_the_player_awake() {
    let (sleeping, warned) = try_sleep_with(|region, _| {
        place_zombie(region, 14.5, -60.0, 14.5, false);
    });
    assert!(!sleeping, "附近有僵尸不能睡");
    assert!(warned, "要提示 not_safe");
}

/// 倒地动画里的僵尸还在实体表里，官方的查询不看死活，照样挡着。
#[test]
fn a_dying_zombie_still_counts() {
    let (sleeping, warned) = try_sleep_with(|region, _| {
        place_zombie(region, 14.5, -60.0, 14.5, true);
    });
    assert!(!sleeping && warned, "倒地的僵尸也挡着");
}

/// 盒外的僵尸（碰撞箱离盒的 x 上界还差 0.2）、已经被移除的僵尸都不挡；
/// 创造模式不查怪物。
#[test]
fn zombies_outside_the_box_removed_or_ignored_in_creative_do_not_block_sleep() {
    // 僵尸宽 0.6：中心 17.1 时碰撞箱从 16.8 起，和盒的 16.5 不相交。
    let (sleeping, warned) = try_sleep_with(|region, _| {
        place_zombie(region, 17.1, -60.0, 9.5, false);
    });
    assert!(sleeping && !warned, "盒外的僵尸不挡");
    // 碰撞箱刚好压进盒里 0.1 就挡。
    let (sleeping, _) = try_sleep_with(|region, _| {
        place_zombie(region, 16.7, -60.0, 9.5, false);
    });
    assert!(!sleeping, "碰撞箱和盒相交就挡");
    let (sleeping, warned) = try_sleep_with(|region, _| {
        let uid = place_zombie(region, 14.5, -60.0, 14.5, false);
        region.entities.remove(uid);
    });
    assert!(sleeping && !warned, "移除之后不挡");
    let (sleeping, warned) = try_sleep_with(|region, id| {
        place_zombie(region, 14.5, -60.0, 14.5, false);
        if let Some(player) = region.players.get_mut(&id) {
            player.game_mode = mc_bamboo_protocol::packet::play::GameMode::Creative;
        }
    });
    assert!(sleeping && !warned, "创造模式不查怪物");
    // 牛不是怪物。
    let (sleeping, _) = try_sleep_with(|region, _| {
        region.entities.spawn(mc_bamboo_blocks::entity::Entity::cow(
            mc_bamboo_core::Vec3::new(14.5, -60.0, 14.5),
        ));
    });
    assert!(sleeping, "牛不挡");
}

/// 逐刻记录跳早的那一刻：时钟跳到多少、哪一刻发醒来包和时钟同步包。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct SleepTrace {
    /// 时钟从夜里跳到清晨的刻号。
    clock_jump_tick: Option<i64>,
    /// 跳到的主世界时钟刻。
    clock_jump_to: i64,
    /// 发出站姿 `set_entity_data` 的刻号。
    wake_pose_tick: Option<i64>,
    /// 发出带主世界时钟的 `set_time` 的刻号（周期同步是空表，不算）。
    clock_sync_tick: Option<i64>,
}

fn sleep_trace(server: &mut Harness, client: &Client, ticks: usize) -> SleepTrace {
    let mut trace = SleepTrace::default();
    let mut last_clock = server.world.master.global.clocks()[0].1.total_ticks;
    let mut was_sleeping = server
        .world
        .islands
        .region
        .players
        .get(&client.id)
        .is_some_and(|player| player.sleep.is_some());
    for _ in 0..ticks {
        let before = server.world.master.global.time().0;
        server.tick();
        let clock = server.world.master.global.clocks()[0].1.total_ticks;
        let expected = last_clock + 1;
        if clock != expected && trace.clock_jump_tick.is_none() {
            trace.clock_jump_tick = Some(before);
            trace.clock_jump_to = clock;
        }
        let sleeping = server
            .world
            .islands
            .region
            .players
            .get(&client.id)
            .is_some_and(|player| player.sleep.is_some());
        let woke = was_sleeping && !sleeping;
        was_sleeping = sleeping;
        let frames = client.take_frames();
        if woke && trace.wake_pose_tick.is_none() {
            trace.wake_pose_tick = Some(before);
            assert!(
                frames
                    .iter()
                    .any(|frame| frame.id == clientbound::SET_ENTITY_DATA),
                "醒来的这一刻要发站姿元数据: tick {before}"
            );
            assert!(
                frames
                    .iter()
                    .any(|frame| frame.id == clientbound::PLAYER_POSITION),
                "醒来的这一刻要发传送包: tick {before}"
            );
        }
        if trace.clock_sync_tick.is_none() {
            for frame in &frames {
                if frame.id == clientbound::SET_TIME {
                    let mut reader = Reader::new(&frame.body);
                    let _game_time = reader.i64().expect("set_time 的 gameTime");
                    if reader.varint().unwrap_or(0) > 0 {
                        trace.clock_sync_tick = Some(before);
                        break;
                    }
                }
            }
        }
        last_clock = clock;
    }
    trace
}

/// 夜里右键床：床被占、玩家睡姿、第二天跳早并清天气。
#[test]
fn sleeping_through_the_night_advances_the_clock_and_clears_weather() {
    use mc_bamboo_blocks::{BlockState, state::OCCUPIED};

    let (mut server, _client, _foot, head) = sleep_scene();
    let region = &server.world.islands.region;
    assert!(
        region
            .chunks
            .block(head)
            .is_some_and(|s| BlockState(s).get(OCCUPIED))
    );
    assert!(
        region.players[&PlayerId(1)].sleep.is_some(),
        "玩家应该睡着了"
    );
    assert!(
        region.players[&PlayerId(1)].sync.sleeping,
        "睡姿应该是 SLEEPING"
    );

    // 逐刻推进睡眠计数，够了人数就跳到早上、清天气、叫醒所有人。
    for _ in 0..101 {
        server.tick();
    }
    let global = &server.world.master.global;
    // 跳早之后时钟照官方继续走，所以这里只要求它落在刚过 0 刻的清晨。
    assert!(
        global.clocks()[0].1.total_ticks.rem_euclid(24_000) < 5,
        "跳到 0 刻（早上）之后照常推进: {}",
        global.clocks()[0].1.total_ticks
    );
    assert!(!global.state().weather.raining);
    assert!(!global.state().weather.thundering);
    let region = &server.world.islands.region;
    assert!(region.players[&PlayerId(1)].sleep.is_none(), "所有人醒来");
    assert!(!region.players[&PlayerId(1)].sync.sleeping);
    assert!(
        !region
            .chunks
            .block(head)
            .is_some_and(|s| BlockState(s).get(OCCUPIED)),
        "床的 occupied 清掉"
    );
}

/// 跳早在主控每刻开头（天气之后、岛开跑之前）决定：逐刻比跳的时刻、
/// 跳到的刻数、醒来包和时钟同步包都在同一刻。把决定推迟一刻的改坏必须被抓到。
#[test]
fn sleep_skip_happens_at_the_official_tick() {
    let (mut server, client, _foot, _head) = sleep_scene();
    let official = sleep_trace(&mut server, &client, 105);
    assert!(
        official.clock_jump_tick.is_some(),
        "熟睡人数够了应该跳早: {official:?}"
    );
    assert_eq!(
        official.clock_jump_tick, official.clock_sync_tick,
        "时钟同步包必须和跳时间同一刻: {official:?}"
    );
    assert_eq!(
        official.clock_jump_tick, official.wake_pose_tick,
        "醒来（站姿 / 传送包）必须和跳时间同一刻: {official:?}"
    );
    assert_eq!(
        official.clock_jump_to.rem_euclid(24_000),
        0,
        "跳到 wake_up_from_sleep 标记: {official:?}"
    );

    // 改坏：把第一次跳早决定推迟一刻，逐刻记录应该整体晚一刻。
    let (mut broken, broken_client, _foot, _head) = sleep_scene();
    broken.world.master.sleep_break_late = true;
    let late = sleep_trace(&mut broken, &broken_client, 106);
    assert_eq!(
        late.clock_jump_tick,
        official.clock_jump_tick.map(|tick| tick + 1),
        "改坏后跳早必须晚一刻: official {official:?}, broken {late:?}"
    );
    assert_eq!(
        late.clock_sync_tick,
        official.clock_sync_tick.map(|tick| tick + 1),
        "改坏后时钟同步包也要晚一刻: {late:?}"
    );
    assert_eq!(
        late.wake_pose_tick,
        official.wake_pose_tick.map(|tick| tick + 1),
        "改坏后醒来包也要晚一刻: {late:?}"
    );
}

/// 官方 `ServerLevel.tick` 的两个独立规则：`advance_time` 为假时不跳时间，
/// `advance_weather` 为假时不清天气，但 `wakeUpAllPlayers` 无条件发生。
#[test]
fn sleep_skip_rules_are_independent() {
    let (mut server, _client, _foot, _head) = sleep_scene();
    let _ = server
        .world
        .master
        .global
        .rules_mut()
        .assign(mc_bamboo_data::gamerules::ADVANCE_TIME, "false");
    let _ = server
        .world
        .master
        .global
        .rules_mut()
        .assign(mc_bamboo_data::gamerules::ADVANCE_WEATHER, "false");
    server.world.publish_global();
    for _ in 0..105 {
        server.tick();
    }
    let global = &server.world.master.global;
    assert_eq!(
        global.clocks()[0].1.total_ticks,
        18_000,
        "advance_time 为假时时钟不动"
    );
    assert!(
        global.state().weather.raining,
        "advance_weather 为假时天气不清"
    );
    assert!(
        server.world.islands.region.players[&PlayerId(1)]
            .sleep
            .is_none(),
        "wakeUpAllPlayers 无条件发生"
    );
}

/// 官方周期同步：每 20 刻一个空时钟表、只带 `gameTime` 的 `set_time`，
/// 包里的游戏时间是这一刻 `tickTime` 加一之前的值。
#[test]
fn periodic_time_sync_carries_the_game_time_before_this_tick() {
    use mc_bamboo_protocol::codec::Reader;

    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    let start = server.world.master.global.time().0;
    client.take_frames();
    // 跑到下一个 `tickCount % 20 == 0`。
    let due = start.div_euclid(20) * 20 + 20;
    while server.world.master.global.time().0 < due {
        server.tick();
    }
    let frames = client.take_frames();
    let syncs: Vec<_> = frames
        .iter()
        .filter(|frame| frame.id == clientbound::SET_TIME)
        .collect();
    assert_eq!(syncs.len(), 1, "每 20 刻一次周期同步: {frames:?}");
    let mut reader = Reader::new(&syncs[0].body);
    assert_eq!(reader.i64().unwrap(), due - 1, "包里的游戏时间");
    assert_eq!(reader.varint().unwrap(), 0, "时钟表是空的");
    assert!(reader.rest().is_empty());
}

/// 官方 `tickCount` 每次开服从 0 数起，和 `level.dat` 的 `Time` 无关：
/// 读档开服时第一个周期时间同步包必须出现在开服后第 20 刻，
/// 包里是当时的 `gameTime`。把判断改回按 `gameTime` 的旧做法会被改坏确认抓到。
#[test]
fn periodic_time_sync_uses_since_launch_ticks_not_saved_game_time() {
    use mc_bamboo_protocol::codec::Reader;
    use mc_bamboo_storage::{level_dat::LevelData, world_dir::WorldDir};

    const SAVED_TIME: i64 = 12_345;

    /// 收到第一个空时钟表的 `set_time` 时，返回 (开服后的刻数, 包里的 gameTime)。
    fn first_sync(break_by_game_time: bool) -> Option<(i32, i64)> {
        let tmp = D6TempDir::new("time-sync-phase");
        let dir = WorldDir::new(&tmp.0);
        dir.create_layout().unwrap();
        mc_bamboo_storage::world_dir::save_level_data(
            &dir,
            &LevelData {
                time: SAVED_TIME,
                ..LevelData::default()
            },
        )
        .unwrap();
        let mut server = Harness::new_loaded_with_save(2, &tmp.0);
        assert_eq!(
            server.world.master.global.time().0,
            SAVED_TIME,
            "gameTime 要从 level.dat 的 Time 接着走"
        );
        server.world.master.global.time_sync_break_by_game_time = break_by_game_time;
        let client = server.join("Kitten");
        server.tick();
        client.take_frames();
        for _ in 2..=40 {
            server.tick();
            for frame in client.take_frames() {
                if frame.id != clientbound::SET_TIME {
                    continue;
                }
                let mut reader = Reader::new(&frame.body);
                let game_time = reader.i64().expect("set_time 的 gameTime");
                if reader.varint().unwrap_or(-1) == 0 {
                    return Some((server.world.master.global.server_ticks(), game_time));
                }
            }
        }
        None
    }

    let official = first_sync(false).expect("开服后 20 刻内要有周期时间同步包");
    assert_eq!(official.0, 20, "第一个周期包在开服后第 20 刻: {official:?}");
    assert_eq!(
        official.1,
        SAVED_TIME + 19,
        "包里是这一刻 tickTime 加一之前的 gameTime: {official:?}"
    );

    // 改坏：按 `gameTime` 判断会让第一个周期包提前到第 15 刻。
    let broken = first_sync(true).expect("改坏后仍会发周期包");
    assert_ne!(broken.0, 20, "按 gameTime 的相位和开服刻数不同: {broken:?}");
    assert_eq!(broken.0, 15, "12345 + tick 是 20 的倍数时提前到 15 刻");
}

/// 多岛时每个岛读的都是同一份全局时间和天气算出的天色。
#[test]
fn every_island_reads_the_same_sky_darken() {
    use crate::island_partition::SectionPos;

    let mut server = Harness::new_loaded(8);
    let origin = SectionPos::new(64, 64);
    server.world.islands.ensure(1, origin);
    server
        .world
        .master
        .global
        .clock_named_mut("minecraft:overworld")
        .unwrap()
        .total_ticks = 18_000;
    let weather = &mut server.world.master.global.state_mut().weather;
    weather.o_rain_level = 1.0;
    weather.rain_level = 1.0;
    weather.o_thunder_level = 1.0;
    weather.thunder_level = 1.0;
    server.world.publish_global();
    let expected = server.world.master.global.sky_darken();
    assert!(expected > 0, "夜晚雷暴要变暗");
    for island in server.world.islands.iter_mut() {
        assert_eq!(island.region.global.sky_darken, expected);
        let (level, _) = island.region.block_level();
        assert_eq!(
            mc_bamboo_blocks::Level::sky_darken(&level),
            expected,
            "精确模式的方块世界读同一份值"
        );
    }
}

#[test]
fn no_more_chunks_without_an_acknowledgement() {
    let mut server = Harness::new(8);
    let client = server.join("Kitten");
    server.tick();
    // 分离驱动下区块晚一刻交付（岛的实施计划第 6 节第 6 步）。
    // Under the detached drive chunks arrive one tick late (island plan step 6).
    server.deliver_posted();
    client.take_frames();

    for _ in 0..50 {
        server.tick();
    }

    // 官方每 20 刻发一次空的 `set_time`，这里只该有周期性时间同步。
    // Vanilla sends an empty `set_time` every 20 ticks; only those periodic
    // time syncs may remain.
    let frames = client.take_frames();
    assert!(
        frames.iter().all(|frame| frame.id == clientbound::SET_TIME),
        "只有周期时间包: {frames:?}"
    );
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
    // 分离驱动下区块晚一刻交付（岛的实施计划第 6 节第 6 步）。
    // Under the detached drive chunks arrive one tick late (island plan step 6).
    server.deliver_posted();
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
    assert_eq!(server.world.islands.region.players.len(), 1);
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
    assert_eq!(server.world.islands.region.players.len(), 1);
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

    assert_eq!(server.world.islands.region.players.len(), 1);
    assert!(server.world.master.global.left_message(ghost.id).is_none());
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
        online_at_login: 0,
        player_data: None,
        sink: Box::new(sink),
        inbox: server.inbox.player_channel().1,
    };
    server.inbox.send(Inbound::Join(Box::new(request))).unwrap();
    server.tick();

    let ids: Vec<PlayerId> = server
        .world
        .islands
        .region
        .players
        .keys()
        .copied()
        .collect();
    assert_eq!(ids, [new_id]);
}

/// 离开 → 存档写还在 IO 线程排队 → 同 uuid 立刻重进：应拿到离开时的数据，
/// 而不是连接层从盘上读到的旧字节（官方 `PlayerDataStorage.save` 同步写，没有这个窗口）。
/// Leave -> the save write is still queued on the IO thread -> immediate rejoin
/// with the same uuid: the rejoining player must get the leaving data, not the
/// stale disk bytes read by the connection layer.
#[test]
fn rejoin_uses_in_flight_player_save_over_stale_disk() {
    use mc_bamboo_protocol::packet::play::GameMode;
    use mc_bamboo_storage::{chunk_io::ChunkIo, player_data::PlayerData, world_dir::WorldDir};

    // 一个只活到测试结束的临时世界目录。声明顺序：tmp 先、server 后，
    // drop 时 server 先关掉 IO 线程，tmp 再删目录。
    // A temp world dir that lives until the test ends. Declared before `server`
    // so the IO thread is closed before the directory is removed.
    struct TempWorldDir(std::path::PathBuf);
    impl TempWorldDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "bamboo-rejoin-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let _ = std::fs::remove_dir_all(&path);
            Self(path)
        }
    }
    impl Drop for TempWorldDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let tmp = TempWorldDir::new();
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new(8);
    // 给无限超平坦测试世界接上真实 IO 线程，让 `submit_player_write` 记录待写副本。
    // Attach a real IO thread so `submit_player_write` records the in-flight copy.
    server
        .world
        .master
        .host
        .set_io(ChunkIo::open(&dir).unwrap());

    let uuid = Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0);
    let old = server.join_uuid("Kitten", uuid);
    server.tick();
    // 把要离开的玩家改成旁观模式，作为"离开时的数据"的可区分标记。
    server.region().players.get_mut(&old.id).unwrap().game_mode = GameMode::Spectator;

    // 连接层读到的旧档：生存模式（和离开时的旁观模式不同）。
    let stale = PlayerData {
        game_type: 0,
        ..PlayerData::default()
    }
    .to_gzip()
    .unwrap();

    let new_id = PlayerId(50);
    let sink = FakeSink::default();
    // 同一个 tick 里先处理 Leave 再处理 Join：poll_io 在 tick 开头已经跑过，
    // 这次写的 PlayerWritten 回执要到下一刻才收，所以待写副本此刻一定还在。
    server.leave(&old);
    let request = JoinRequest {
        id: new_id,
        uuid,
        name: "Kitten".to_owned(),
        properties: Vec::new(),
        client: client_requesting(2),
        permission_level: 0,
        is_op: false,
        peer_ip: "127.0.0.1".to_owned(),
        online_at_login: 0,
        player_data: Some(stale),
        sink: Box::new(sink),
        inbox: server.inbox.player_channel().1,
    };
    server.inbox.send(Inbound::Join(Box::new(request))).unwrap();
    server.tick();

    // 重进拿到的是离开时的旁观模式（待写内存副本），不是盘上的生存模式旧档。
    let rejoined = server.world.islands.region.players.get(&new_id).unwrap();
    assert_eq!(
        rejoined.game_mode,
        GameMode::Spectator,
        "rejoin must use the in-flight leaving save, not the stale disk bytes"
    );
}

/// 临时世界目录，测试结束自动删。IO 线程要在删目录前关掉，所以 tmp 先声明、
/// server 后声明（drop 时 server 先关）。
pub(crate) struct D6TempDir(pub(crate) std::path::PathBuf);

impl D6TempDir {
    pub(crate) fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "bamboo-d6-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for D6TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 收回 IO 信箱里所有 `Written`，返回写过的区块坐标。
pub(crate) fn written_chunks(
    host: &crate::director_host::DirectorHost,
) -> Vec<mc_bamboo_core::ChunkPos> {
    use mc_bamboo_storage::chunk_io::Reply;
    let io = host.io().expect("测试接了 IO 线程");
    std::iter::from_fn(|| io.try_recv())
        .filter_map(|r| match r {
            Reply::Written { pos, .. } => Some(pos),
            _ => None,
        })
        .collect()
}

/// 帧里有没有 `commands.save.success` 这段翻译键（系统消息的 NBT 里是明文）。
fn has_save_success(client: &Client) -> bool {
    const KEY: &[u8] = b"commands.save.success";
    client
        .take_frames()
        .iter()
        .any(|f| f.body.windows(KEY.len()).any(|w| w == KEY))
}

/// 帧里有没有 `commands.save.failed`（F9）。
fn has_save_failure(client: &Client) -> bool {
    const KEY: &[u8] = b"commands.save.failed";
    client
        .take_frames()
        .iter()
        .any(|f| f.body.windows(KEY.len()).any(|w| w == KEY))
}

/// D6：匀速保存把仍加载的脏区块拍快照交给 IO 线程，区块不卸载；
/// 已经存过且没再改的区块下一刻不会重复存。
#[test]
fn d6_saves_a_loaded_dirty_chunk_and_keeps_it_loaded() {
    use mc_bamboo_data::blocks;
    use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

    let tmp = D6TempDir::new("paced");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded(2);
    let pos = mc_bamboo_core::ChunkPos::new(0, 0);
    server.ensure_loaded(&[pos]);
    server
        .world
        .master
        .host
        .set_io(ChunkIo::open(&dir).unwrap());
    server.set_block(mc_bamboo_core::BlockPos::new(3, -60, 3), blocks::GLOWSTONE);
    server.world.islands.region.touch(BlockPos::new(3, -60, 3));
    // 只读快照照复查「中 6」会推迟有待算光照的区块，先让光照算完。
    server.world.islands.region.chunks.finish_light_updates();
    // 和正式路径一样走写路径的改动标记；测试直接改 ChunkStore 时补一下。
    server
        .world
        .islands
        .region
        .touch(mc_bamboo_core::BlockPos::new(3, -60, 3));

    // 有待算光照时快照按复查「中 6」推迟；先跑几刻让光照算完再重试。
    for t in 1..8 {
        server
            .world
            .master
            .host
            .save_loaded_dirty(&mut server.world.islands.all_mut(), t, true);
    }
    server.world.master.host.flush_io();

    assert!(
        server.world.islands.region.chunks.is_loaded(pos),
        "snapshot must not unload the chunk"
    );
    assert!(
        written_chunks(&server.world.master.host).contains(&pos),
        "a dirty loaded chunk should be written"
    );

    // 版本号已经记为「已存」，且 10 秒冷却没到，pos 不该被重复写。
    // 发光方块的光照会顺带弄脏邻居，D6 之后几刻把那些摊平写盘，属于正常匀速行为，
    // 所以这里只针对 pos 本身，不断言整批为空。
    // Its revision is now recorded as saved and the cooldown has not elapsed, so
    // pos must not be written again. A light-emitting block also dirties nearby
    // chunks, which D6 paces over the following ticks; only assert about pos.
    for t in 2..12 {
        server
            .world
            .master
            .host
            .save_loaded_dirty(&mut server.world.islands.all_mut(), t, true);
        server.world.master.host.flush_io();
        assert!(
            !written_chunks(&server.world.master.host).contains(&pos),
            "a clean chunk must not be re-saved inside the cooldown"
        );
    }
}

/// `InhabitedTime` 的累加照官方不置脏（`incrementInhabitedTime` 不 `markUnsaved`）：
/// 只累加的区块不写盘；之后改了方块再存，写进去的是累加后的值。
#[test]
fn inhabited_time_is_saved_only_with_another_change() {
    use mc_bamboo_data::blocks;
    use mc_bamboo_storage::{
        chunk_io::ChunkIo,
        anvil::{RegionCompression, RegionFile},
        world_dir::WorldDir,
    };

    let tmp = D6TempDir::new("inhabited");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded(2);
    let pos = mc_bamboo_core::ChunkPos::new(0, 0);
    server.ensure_loaded(&[pos]);
    server
        .world
        .master
        .host
        .set_io(ChunkIo::open(&dir).unwrap());
    server.world.islands.region.chunks.finish_light_updates();
    // 先把加载时已经脏的区块存掉，从干净的状态开始。
    server
        .world
        .master
        .host
        .snapshot_all_dirty(&mut server.world.islands.all_mut(), 1);
    server.world.master.host.flush_io();
    let _ = written_chunks(&server.world.master.host);

    for _ in 0..5 {
        server.world.islands.region.chunks.increment_inhabited_time(pos);
    }
    server
        .world
        .master
        .host
        .snapshot_all_dirty(&mut server.world.islands.all_mut(), 2);
    server.world.master.host.flush_io();
    assert!(
        !written_chunks(&server.world.master.host).contains(&pos),
        "只累加 InhabitedTime 的区块不写盘"
    );

    server.set_block(mc_bamboo_core::BlockPos::new(3, -60, 3), blocks::STONE);
    server.world.islands.region.touch(BlockPos::new(3, -60, 3));
    server.world.islands.region.chunks.finish_light_updates();
    server
        .world
        .master
        .host
        .snapshot_all_dirty(&mut server.world.islands.all_mut(), 3);
    server.world.master.host.flush_io();
    assert!(written_chunks(&server.world.master.host).contains(&pos));
    let path = dir.region_dir().join("r.0.0.mca");
    let mut region =
        RegionFile::open(&path, dir.region_dir(), RegionCompression::Zlib).expect("区域文件能打开");
    let bytes = region.read(pos, 32 << 20).unwrap().expect("区块在盘上");
    let decoded = mc_bamboo_storage::chunk_nbt::decode_bytes(&bytes).expect("官方格式能解");
    assert_eq!(decoded.inhabited_time, 5, "存进去的是累加后的值");
}

/// `save-off` 期间匀速保存整个停掉。
#[test]
fn d6_stops_when_auto_save_is_off() {
    use mc_bamboo_data::blocks;
    use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

    let tmp = D6TempDir::new("off");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded(2);
    let pos = mc_bamboo_core::ChunkPos::new(0, 0);
    server.ensure_loaded(&[pos]);
    server
        .world
        .master
        .host
        .set_io(ChunkIo::open(&dir).unwrap());
    server.set_block(mc_bamboo_core::BlockPos::new(3, -60, 3), blocks::GLOWSTONE);
    server.world.islands.region.touch(BlockPos::new(3, -60, 3));

    server
        .world
        .master
        .host
        .save_loaded_dirty(&mut server.world.islands.all_mut(), 1, false);
    server.world.master.host.flush_io();
    assert!(
        written_chunks(&server.world.master.host).is_empty(),
        "save-off must stop the paced snapshots"
    );
}

/// `/save-all` 立刻给所有已加载脏区块拍快照，不受匀速和冷却限制。
#[test]
fn save_all_snapshots_every_dirty_loaded_chunk() {
    use mc_bamboo_data::blocks;
    use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

    let tmp = D6TempDir::new("all");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded(2);
    let a = mc_bamboo_core::ChunkPos::new(0, 0);
    let b = mc_bamboo_core::ChunkPos::new(1, 0);
    server.ensure_loaded(&[a, b]);
    server
        .world
        .master
        .host
        .set_io(ChunkIo::open(&dir).unwrap());
    server.set_block(mc_bamboo_core::BlockPos::new(3, -60, 3), blocks::GLOWSTONE);
    server.world.islands.region.touch(BlockPos::new(3, -60, 3));
    server.set_block(mc_bamboo_core::BlockPos::new(19, -60, 3), blocks::GLOWSTONE);
    server.world.islands.region.touch(BlockPos::new(19, -60, 3));
    server
        .world
        .islands
        .region
        .touch(mc_bamboo_core::BlockPos::new(3, -60, 3));
    server
        .world
        .islands
        .region
        .touch(mc_bamboo_core::BlockPos::new(19, -60, 3));
    server.world.islands.region.chunks.finish_light_updates();

    server
        .world
        .master
        .host
        .snapshot_all_dirty(&mut server.world.islands.all_mut(), 1);
    server.world.master.host.flush_io();
    let written = written_chunks(&server.world.master.host);
    assert!(
        written.contains(&a),
        "first dirty loaded chunk should be written"
    );
    assert!(
        written.contains(&b),
        "second dirty loaded chunk should be written"
    );
}

/// F7：`/save-off` 之后 `/save-all` 用 force 照样把脏区块写下去
/// （官方 `ServerLevel.save(..., noSave && !force)`）。
/// F7: `/save-all` forces the save even during `/save-off` (official
/// `ServerLevel.save(..., noSave && !force)`).
#[test]
fn save_all_is_a_force_even_during_save_off() {
    use mc_bamboo_data::blocks;
    use mc_bamboo_storage::world_dir::WorldDir;

    let tmp = D6TempDir::new("force-off");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    // 接真实 SaveSession：`/save-off` 才会把 noSave 落到会话上。
    let mut server = Harness::new_loaded_with_save(2, &tmp.0);
    let pos = mc_bamboo_core::ChunkPos::new(0, 0);
    server.ensure_loaded(&[pos]);
    server.set_block(mc_bamboo_core::BlockPos::new(3, -60, 3), blocks::GLOWSTONE);
    server.world.islands.region.touch(BlockPos::new(3, -60, 3));

    server
        .world
        .master
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveOff);
    server.tick();
    assert!(
        !server.world.master.save.as_ref().unwrap().auto_save(),
        "save-off must reach the SaveSession (noSave)"
    );
    // save-off 期间匀速不写：先确认没有 Written。
    server.world.master.host.flush_io();
    assert!(written_chunks(&server.world.master.host).is_empty());

    server
        .world
        .master
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveAll {
            flush: false,
            source: crate::inbound::PlayerId(1),
        });
    server.tick();
    server.world.master.host.flush_io();
    assert!(
        written_chunks(&server.world.master.host).contains(&pos),
        "save-all must force the dirty chunk out despite save-off"
    );
}

/// F7：官方 `PlayerList.remove` / `saveAll` 不看 `noSave`，
/// 所以 `/save-off` 期间玩家离开照样写存档。
/// F7: official `PlayerList.remove`/`saveAll` ignore `noSave`, so a leave
/// during `/save-off` still writes the player save.
#[test]
fn save_off_keeps_player_leave_saves_like_vanilla() {
    use mc_bamboo_storage::world_dir::WorldDir;

    let tmp = D6TempDir::new("leave-off");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded_with_save(2, &tmp.0);
    let client = server.join("Admin");
    server.tick();

    server
        .world
        .master
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveOff);
    server.tick();
    server.world.on_leave(client.id);
    server.world.master.host.flush_io();

    assert!(
        std::fs::read_dir(dir.players_data_dir())
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().ends_with(".dat")),
        "the leaving player's save must still be written during save-off"
    );
}

/// 补 9：只改容器内容不改方块（方块版本号不变），匀速保存也必须把它写下去，
/// 否则崩溃会丢箱子里的东西。
/// Item 9: a container-only edit (block revision unchanged) must still be
/// snapshotted by paced D6 saving, or a crash loses the chest contents.
#[test]
fn d6_saves_a_chunk_with_only_a_container_edit() {
    use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

    let tmp = D6TempDir::new("container-only");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded(2);
    let pos = mc_bamboo_core::ChunkPos::new(0, 0);
    server.ensure_loaded(&[pos]);
    server
        .world
        .master
        .host
        .set_io(ChunkIo::open(&dir).unwrap());

    // 只往容器表放一个条目，不改任何方块：ChunkStore 的版本号不会变，
    // 写路径的改动标记要让它照样被 D6 写下去。
    let chest = mc_bamboo_core::BlockPos::new(3, -60, 3);
    server.world.islands.region.touch(chest);
    server.world.islands.region.containers.insert(
        chest,
        crate::container::ContainerEntry::new(crate::container::ContainerKind::Chest),
    );

    server
        .world
        .master
        .host
        .save_loaded_dirty(&mut server.world.islands.all_mut(), 1, true);
    server.world.master.host.flush_io();
    assert!(
        written_chunks(&server.world.master.host).contains(&pos),
        "只改容器内容也要被 D6 写下去"
    );
}

/// 没 `flush`：`save-all` 成功消息在快照都交给 IO 之后这一刻就发。
#[test]
fn save_all_sends_success_after_handoff() {
    let mut server = Harness::new_loaded(2);
    server.world.islands.region.global.send_command_feedback = true;
    let client = server.join("Admin");
    server.tick();
    client.take_frames();

    server
        .world
        .master
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveAll {
            flush: false,
            source: client.id,
        });
    server.tick();
    if server.detached() {
        // 分离驱动下 `/save-all` 等岛这一刻交回、停放之后才过栅栏，成功消息排在
        // 这一刻刷出之后，下一刻才到客户端。
        // Detached: `/save-all` passes its barrier after the island hands back,
        // so the success message is flushed one tick later.
        server.tick();
    }
    assert!(
        has_save_success(&client),
        "non-flush success is sent once snapshots are handed off"
    );
    assert!(server.world.master.pending_save_success.is_empty());
}

/// 有 `flush`：成功消息要等 IO 栅栏回执（信箱），世界线程不等待；
/// 回执到达前不发，到达后补发。
#[test]
fn save_all_flush_success_waits_for_the_io_barrier() {
    use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

    let tmp = D6TempDir::new("flush-success");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded(2);
    server.world.islands.region.global.send_command_feedback = true;
    let client = server.join("Admin");
    server.tick();
    client.take_frames();
    server
        .world
        .master
        .host
        .set_io(ChunkIo::open(&dir).unwrap());

    server
        .world
        .master
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveAll {
            flush: true,
            source: client.id,
        });
    server.tick();
    assert!(
        !has_save_success(&client),
        "flush must defer success until the IO barrier acked"
    );
    assert_eq!(server.world.master.pending_save_success.len(), 1);

    // 让 IO 线程处理栅栏，再跑一刻让 poll_io 收回执、补发成功消息。
    server.world.master.host.flush_io();
    server.tick();
    assert!(
        has_save_success(&client),
        "success is sent after the IO barrier acked"
    );
    assert!(server.world.master.pending_save_success.is_empty());
}

/// F3：接了真实 IO 的 `save-all`（不带 flush）也不许世界线程阻塞等 IO；
/// 成功消息一律等信箱栅栏回执，回执到达后补发。
/// F3: with a real IO thread, even non-flush `save-all` must not block the world
/// thread on IO; success always waits for the mailbox barrier ack.
#[test]
fn non_flush_save_all_with_io_waits_for_the_mailbox_barrier() {
    use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

    let tmp = D6TempDir::new("nonflush-barrier");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded(2);
    server.world.islands.region.global.send_command_feedback = true;
    let client = server.join("Admin");
    server.tick();
    client.take_frames();
    server
        .world
        .master
        .host
        .set_io(ChunkIo::open(&dir).unwrap());

    server
        .world
        .master
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveAll {
            flush: false,
            source: client.id,
        });
    server.tick();
    assert!(
        !has_save_success(&client),
        "success must wait for the IO ack, not be sent inline"
    );
    assert_eq!(server.world.master.pending_save_success.len(), 1);

    server.world.master.host.flush_io();
    server.tick();
    assert!(has_save_success(&client), "success follows the barrier ack");
    assert!(server.world.master.pending_save_success.is_empty());
    let _ = dir; // 这一局没有 SaveSession，level.dat 的字节另由 save_session 测试覆盖
}

/// F9：写盘失败时 `save-all` 发 `commands.save.failed`，绝不报成功；
/// 消息在栅栏回执（保存真正完成）之后才发。
/// F9: a failed `save-all` sends `commands.save.failed`, never success, and
/// only after the barrier ack (the save really completed).
#[test]
fn save_all_reports_failed_when_the_io_write_fails() {
    use mc_bamboo_data::blocks;
    use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

    let tmp = D6TempDir::new("save-failed");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let io = ChunkIo::open(&dir).unwrap();
    // 区域目录换成普通文件，任何区块写都会失败。
    for directory in [dir.region_dir(), dir.entities_dir()] {
        std::fs::remove_dir(&directory).unwrap();
        std::fs::write(&directory, b"not a directory").unwrap();
    }

    let mut server = Harness::new_loaded(2);
    server.world.islands.region.global.send_command_feedback = true;
    let client = server.join("Admin");
    server.tick();
    client.take_frames();
    let pos = mc_bamboo_core::ChunkPos::new(0, 0);
    server.ensure_loaded(&[pos]);
    server.set_block(mc_bamboo_core::BlockPos::new(3, -60, 3), blocks::GLOWSTONE);
    server.world.islands.region.touch(BlockPos::new(3, -60, 3));
    server.world.master.host.set_io(io);

    server
        .world
        .master
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveAll {
            flush: false,
            source: client.id,
        });
    server.tick();
    assert!(!has_save_success(&client), "no success before the ack");
    assert!(!has_save_failure(&client), "failure waits for the ack too");

    // 让 IO 处理栅栏（写失败会把回执标成 ok=false），下一刻收回执。
    server.world.master.host.flush_io();
    server.tick();
    assert!(
        has_save_failure(&client),
        "commands.save.failed must be sent"
    );
}

/// IO 线程 panic 是致命错误：世界线程要发现、记日志、走关服流程（tick 返回 Break），
/// 且 level.dat / data/ 的字节仍会尝试交给 IO，不挂死。
/// A dead IO thread is fatal: the world loop detects it and breaks into shutdown
/// without hanging (level/data bytes are still attempted).
#[test]
fn a_dead_io_thread_shuts_the_world_down_without_hanging() {
    use mc_bamboo_storage::{chunk_io::ChunkIo, world_dir::WorldDir};

    let tmp = D6TempDir::new("io-fatal");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded(2);
    server
        .world
        .master
        .host
        .set_io(ChunkIo::open(&dir).unwrap());

    // 故意留一个在途读请求：IO 线程死了也不能让 Director 永远等结果。
    // Leave a read in flight: a dead IO thread must not leave the Director waiting.
    server
        .world
        .master
        .host
        .io()
        .unwrap()
        .submit_read(mc_bamboo_core::ChunkPos::new(3, 3), 99, 0);
    server.world.master.host.io().unwrap().crash_for_test();
    for _ in 0..2000 {
        if server.world.master.host.io().unwrap().worker_stopped() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        server.world.master.host.io_failed(),
        "world must notice the dead IO thread"
    );
    // 关服路径不能挂死：这一 tick 直接走完 shut_down 并返回 Break。
    assert!(
        server.world.tick().is_break(),
        "a fatal IO error must end the world loop"
    );
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
    assert!(!server.world.islands.region.links.is_open(refusing.id));
    assert_eq!(server.world.islands.region.players.len(), 2);
    // Bob 走出了 Alice 的视距。
    assert_eq!(staying.take_ids(), [clientbound::REMOVE_ENTITIES]);

    server.leave(&refusing);
    server.tick();
    assert_eq!(server.world.islands.region.players.len(), 1);
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
    assert!(server.world.islands.region.players.is_empty());
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
    assert!(server.world.islands.region.players.is_empty());
}

#[test]
fn edited_chunk_is_resent_with_the_edit() {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    client.take_frames();
    let edited = ChunkPos::new(0, 0);
    let flat = server.world.islands.region.template_body.clone();

    let pos = BlockPos::new(1, -60, 1);
    assert!(
        server
            .world
            .islands
            .region
            .chunks
            .set_block(pos, blocks::CHEST)
    );

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
    let frames = server.tick_until_chunk(&client, edited);
    let chunks = frames_with_id(&frames, clientbound::LEVEL_CHUNK_WITH_LIGHT);
    let resent = chunks
        .iter()
        .find(|frame| chunk_position(frame) == edited)
        .expect("这个区块又进入了视图");
    let store = &server.world.islands.region.chunks;
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
    server.ensure_loaded(&[edited]);
    server
        .world
        .islands
        .region
        .chunks
        .set_block(pos, blocks::STONE);

    let client = server.join("Kitten");
    server.tick();

    let frames = client.take_frames();
    let chunks = frames_with_id(&frames, clientbound::LEVEL_CHUNK_WITH_LIGHT);
    let chunk = chunks
        .iter()
        .find(|frame| chunk_position(frame) == edited)
        .expect("3×3 里有这个区块");
    assert_ne!(chunk_body(chunk), server.world.islands.region.template_body);
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
    let ticks_so_far = server.world.master.global.time().0;
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
        // 每 20 刻的官方时间同步包不算延迟广播。
        // The official 20-tick time sync is not a latency broadcast.
        let frames = client.take_frames();
        assert!(
            frames.iter().all(|frame| frame.id == clientbound::SET_TIME),
            "只有周期时间包: {frames:?}"
        );
    }
    server.tick();

    let frames: Vec<_> = client
        .take_frames()
        .into_iter()
        .filter(|frame| frame.id != clientbound::SET_TIME)
        .collect();
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
fn profiler_charges_every_phase_once_per_tick() {
    // 断言的是一个岛时的布局：打报告的剖析器跟着家岛。多岛（混合驱动）时主控的各段记在
    // 主控自己的剖析器里，见 `island_hybrid_tests::profiling_covers_every_master_tick_and_every_island_tick`。
    // Asserts the one-island layout (the reporting profiler is the home
    // island's); the hybrid drive is covered by a test of its own.
    let mut server = Harness::new_one_island(2);
    server.tick();

    for phase in [
        Phase::Inbox,
        Phase::LightUpdates,
        Phase::WeatherAndTime,
        Phase::ScheduledTicks,
        Phase::RandomTicks,
        Phase::BlockEvents,
        Phase::Entities,
        Phase::BlockEntities,
        Phase::PlayerMisc,
        Phase::Views,
        Phase::BlockChangeBroadcast,
        Phase::EntitySyncAndTracking,
        Phase::ChunkSending,
        Phase::Flush,
        Phase::TickTail,
    ] {
        assert_eq!(
            server
                .world
                .islands
                .region
                .profiler
                .histogram(Series::Phase(phase))
                .count(),
            1,
            "{phase:?}"
        );
    }
    assert_eq!(
        server
            .world
            .islands
            .region
            .profiler
            .histogram(Series::Metric(Metric::TickTotal))
            .count(),
        1
    );
    for metric in [
        Metric::InboundMessages,
        Metric::OnlinePlayers,
        Metric::DueScheduledTicks,
        Metric::Entities,
        Metric::ChunksSent,
    ] {
        assert_eq!(
            server
                .world
                .islands
                .region
                .profiler
                .histogram(Series::Metric(metric))
                .count(),
            1,
            "{metric:?}"
        );
    }
    let report = server.world.islands.region.profiler.report();
    assert!(report.contains("phases (by mean):"));
    for phase in [
        "inbox",
        "light_updates",
        "weather_and_time",
        "scheduled_ticks",
        "random_ticks",
        "block_events",
        "entities",
        "block_entities",
        "player_misc",
        "views",
        "block_change_broadcast",
        "entity_sync_and_tracking",
        "chunk_sending",
        "flush",
        "tick_tail",
    ] {
        assert!(report.contains(phase), "{phase}");
    }
}

#[test]
fn early_return_from_inbox_still_finishes_the_tick_timing() {
    // 断言的是一个岛时的布局：打报告的剖析器跟着家岛。多岛（混合驱动）时主控的各段记在
    // 主控自己的剖析器里，见 `island_hybrid_tests::profiling_covers_every_master_tick_and_every_island_tick`。
    // Asserts the one-island layout (the reporting profiler is the home
    // island's); the hybrid drive is covered by a test of its own.
    let mut server = Harness::new_one_island(2);
    server.tick();
    server.world.islands.region.profiler.reset_samples();

    server.inbox.send(Inbound::Shutdown).unwrap();
    assert!(server.world.tick().is_break());

    assert_eq!(
        server
            .world
            .islands
            .region
            .profiler
            .histogram(Series::Phase(Phase::Inbox))
            .count(),
        0
    );
    assert_eq!(
        server
            .world
            .islands
            .region
            .profiler
            .histogram(Series::Metric(Metric::TickTotal))
            .count(),
        1
    );
    assert_eq!(
        server
            .world
            .islands
            .region
            .profiler
            .histogram(Series::Metric(Metric::InboundMessages))
            .count(),
        1
    );
}

#[test]
fn chunk_latency_counts_wall_time_between_entering_pending_and_sending() {
    let mut server = Harness::new(8);
    let client = server.join_with("Kitten", client_requesting(8));
    // 第一刻：区块进入 pending，并发出最近的 9 个。
    server.tick();
    client.take_frames();
    server.world.islands.region.profiler.reset_samples();

    // 确认上一批，下一刻继续发，延迟按「进入 pending 到写出」的刻数记。
    server.acknowledge_batch(&client, 10.0);
    server.tick();

    let latency = server
        .world
        .islands
        .region
        .profiler
        .histogram(Series::Metric(Metric::ChunkLatencyUs));
    assert!(latency.count() > 0);
    let chunks_sent = server
        .world
        .islands
        .region
        .profiler
        .histogram(Series::Metric(Metric::ChunksSent));
    assert_eq!(chunks_sent.count(), 1);
    assert_eq!(chunks_sent.sum(), latency.count() as u128);
}

#[test]
fn world_time_advances_once_per_tick() {
    let mut server = Harness::new(2);

    for _ in 0..5 {
        server.tick();
    }

    assert_eq!(server.world.master.global.time().0, 5);
    assert_eq!(server.world.islands.region.block_ticks.now(), 5);
    assert_eq!(
        server.world.islands.region.tick.since(Default::default()),
        5
    );
}

/// 存盘 → 重开：官方 `PrimaryLevelData.parse` 从 `level.dat` 的 `Time` 接着走，
/// 世界时钟从 `world_clocks.dat` 接着走，岛的计划刻时钟也跟着世界时间对上。
/// 重启前后「还有多少刻到点」不变，漏斗等读盘后的相对时刻不会退货到 0。
///
/// Save → restart: vanilla's `PrimaryLevelData.parse` resumes `Time` from
/// `level.dat`, world clocks resume from `world_clocks.dat`, and the island's
/// scheduled-tick clock starts at the restored game time. Relative tick
/// deadlines are unchanged across the restart.
#[test]
fn save_and_reopen_resumes_game_time_day_time_and_scheduled_ticks() {
    use mc_bamboo_storage::world_dir::WorldDir;

    const TICKS_AFTER: usize = 5;
    // 交付区块最多可能走 64 刻，排刻延迟要远大于它，免得比较前刻就到期了。
    // Chunk delivery may take up to 64 ticks, so the scheduled delay must
    // exceed that or the tick would fire before we compare.
    const TICK_DELAY: i32 = 1000;
    let tmp = D6TempDir::new("restore-time");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let pos = BlockPos::new(3, -60, 3);

    let (saved_time, saved_day_time, before) = {
        let mut server = Harness::new_loaded_with_save(2, &tmp.0);
        server.ensure_loaded(&[ChunkPos::new(0, 0)]);
        // 让主世界时钟真的走起来并留一个非零的流速，重开后必须一起恢复。
        // Resume the overworld clock with a non-default rate so the restore
        // checks every field of ClockState.
        let clock = server
            .world
            .master
            .global
            .clock_named_mut("minecraft:overworld")
            .unwrap();
        clock.set_paused(false);
        clock.set_rate(2.0);
        for _ in 0..137 {
            server.tick();
        }
        // 排一个计划刻，记录到点时刻相对当前游戏时间的差。
        server.world.islands.region.block_ticks.schedule(
            pos,
            mc_bamboo_data::blocks::STONE as usize,
            TICK_DELAY,
            mc_bamboo_blocks::ticks::TickPriority::Normal,
        );
        let saved_now = server.world.islands.region.block_ticks.now();
        let before: Vec<i64> = server
            .world
            .islands
            .region
            .block_ticks
            .pending()
            .map(|tick| tick.trigger_tick - saved_now)
            .collect();
        assert!(!before.is_empty(), "存档里应该有一项到点的计划刻");
        let state = (
            server.world.master.global.time().0,
            server.world.master.global.clock().total_ticks,
            before,
        );
        // 走关服路径：写 level.dat / data/（含 Time 和 world_clocks），释放锁。
        server.shut_down();
        state
    };
    assert!(saved_time > 0, "第一局确实推进过游戏时间");
    assert!(saved_day_time > 0, "第一局确实推进过昼夜时间");

    let mut reopened = Harness::new_loaded_with_save(2, &tmp.0);
    assert_eq!(
        reopened.world.master.global.time().0,
        saved_time,
        "游戏时间要从 level.dat 的 Time 恢复"
    );
    assert_eq!(
        reopened.world.islands.region.block_ticks.now(),
        saved_time,
        "岛的时钟从恢复的游戏时间起算"
    );
    assert_eq!(
        reopened.world.islands.region.tick.game_time(),
        saved_time,
        "岛的模拟时钟也从恢复的游戏时间起算"
    );
    let clock = reopened.world.master.global.clock();
    assert_eq!(clock.total_ticks, saved_day_time, "昼夜时间接着走");
    assert_eq!(clock.rate, 2.0, "流速也要从 world_clocks.dat 恢复");
    assert!(!clock.paused, "暂停标志也要从 world_clocks.dat 恢复");
    // 全新世界里 world_clocks.dat 写的是默认（0 / rate 1 / 不暂停）的末地时钟。
    assert_eq!(
        reopened
            .world
            .master
            .global
            .clock_named("minecraft:the_end")
            .unwrap()
            .rate,
        1.0
    );

    // 计划刻读盘后的绝对到点时刻和重启前一致（`ensure_loaded` 为了交付区块
    // 可能又走了几刻，所以按存盘时的基准比较），再走几刻仍逐刻同步。
    reopened.ensure_loaded(&[ChunkPos::new(0, 0)]);
    // 区块不在岛上时它的计划刻不走，到点时刻从交付那一刻算起。分离驱动下交付晚一刻，
    // 所以基准也晚一刻。
    // A chunk's scheduled ticks do not advance while it is away; they count from
    // delivery. Under the detached drive delivery is one tick later, and so is the base.
    let delivered_at = saved_time + i64::from(reopened.detached());
    let after: Vec<i64> = reopened
        .world
        .islands
        .region
        .block_ticks
        .pending()
        .map(|tick| tick.trigger_tick - delivered_at)
        .collect();
    assert_eq!(after, before, "重启前后计划刻的相对到点时刻不变");
    let resume_at = reopened.world.master.global.time().0;
    for step in 1..=TICKS_AFTER as i64 {
        reopened.tick();
        assert_eq!(reopened.world.master.global.time().0, resume_at + step);
        assert_eq!(
            reopened.world.islands.region.block_ticks.now(),
            resume_at + step
        );
    }
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
        .islands
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
        assert!(schedule.catching_up(), "连续跑的刻标成追赶");
        now += Duration::from_millis(1);
    }
    assert!(!schedule.after_tick(now).is_zero());
    assert!(!schedule.catching_up(), "重新睡上之后不再是追赶");
}

#[test]
fn schedule_skips_ahead_when_it_is_too_far_behind() {
    let start = Instant::now();
    let mut schedule = Schedule::starting_at(start);

    let now = start + Duration::from_secs(10);
    assert_eq!(schedule.after_tick(now), Duration::ZERO);
    assert!(schedule.skipped_catch_up());

    // 不再补那 10 秒，下一 tick 从现在算起。
    assert_eq!(schedule.after_tick(now), Duration::from_millis(50));
    assert!(!schedule.skipped_catch_up());
}

#[test]
fn join_remembers_the_profile_for_admin_name_lookups() {
    let mut server = Harness::new(2);
    let client = server.join("Kitten");
    server.tick();
    assert_eq!(
        server
            .world
            .master
            .global
            .admin
            .known_player("kItTeN")
            .map(|player| player.uuid),
        Some(client.uuid)
    );
}

/// 并行驱动的场景：掉落物、下落方块、TNT 和漏斗各来一点，跑若干刻并收集世界线程
/// 发出的全部字节。线程数只影响怎么算，不影响结果，所以 1 和 4 必须逐位相同。
fn entity_bytes(threads: usize) -> Vec<u8> {
    use mc_bamboo_blocks::{BlockState, entity::Entity};
    use mc_bamboo_data::blocks;
    use mc_bamboo_protocol::item::ItemStack;

    let mut server = Harness::new(2);
    let client = server.join_and_settle("Kitten");
    client.take_frames();
    server.set_entity_threads(threads);

    // 一小片石头地面上的相邻物品（会互相合并）、孤立物品、下落沙块和点燃的 TNT。
    let floor = BlockPos::new(8, -61, 8);
    for dx in 0..4 {
        server.set_block(floor.offset(dx, 0, 0), blocks::STONE);
    }
    for dx in 0..3 {
        server.spawn_entity(Entity::ground_item(
            Vec3::new(8.5 + f64::from(dx) * 0.2, -59.5, 8.5),
            ItemStack::new(1, 1),
        ));
    }
    server.spawn_entity(Entity::ground_item(
        Vec3::new(24.5, -59.5, 8.5),
        ItemStack::new(2, 1),
    ));
    server.spawn_entity(Entity::falling_block(
        BlockPos::new(10, -55, 8),
        BlockState(blocks::SAND),
    ));
    server.spawn_entity(Entity::primed_tnt(Vec3::new(12.5, -58.5, 8.5), 0.25));
    // 漏斗：物品落在它里面时会走 `SuckIntoHopper` 命令，落地阶段写容器。
    server.set_block(BlockPos::new(9, -60, 8), blocks::HOPPER);
    server.spawn_entity(Entity::ground_item(
        Vec3::new(9.5, -59.7, 8.5),
        ItemStack::new(3, 1),
    ));

    let mut bytes = Vec::new();
    for _ in 0..12 {
        server.tick();
        for frame in client.take_frames() {
            mc_bamboo_protocol::codec::write_varint(&mut bytes, frame.id);
            bytes.extend_from_slice(&frame.body);
        }
    }
    bytes
}

#[test]
fn one_and_four_entity_threads_emit_identical_bytes() {
    let one = entity_bytes(1);
    let four = entity_bytes(4);
    assert!(!one.is_empty(), "场景要真的产生实体同步包");
    assert_eq!(one, four);
}

/// 没有实体互相作用时，并行驱动和精确驱动发出的字节也逐位相同。
/// 设计稿第 13 节第 5 条：差别只来自「看到的别的实体是这一刻开始时的样子」。
#[test]
fn isolated_entities_emit_identical_bytes_in_both_drivers() {
    fn isolated(threads: usize) -> Vec<u8> {
        use mc_bamboo_blocks::entity::Entity;
        use mc_bamboo_protocol::item::ItemStack;

        let mut server = Harness::new(2);
        let client = server.join_and_settle("Kitten");
        client.take_frames();
        server.set_entity_threads(threads);
        // 相距很远的几个孤立物品，物理、随机源、发包顺序都不涉及别的实体。
        for index in 0..3 {
            server.spawn_entity(Entity::ground_item(
                Vec3::new(8.5 + f64::from(index) * 120.0, -59.5, 8.5),
                ItemStack::new(1, 1),
            ));
        }
        let mut bytes = Vec::new();
        for _ in 0..8 {
            server.tick();
            for frame in client.take_frames() {
                bytes.extend_from_slice(&(frame.id as u64).to_le_bytes());
                bytes.extend_from_slice(&frame.body);
            }
        }
        bytes
    }
    let precise = isolated(0);
    let parallel = isolated(4);
    assert!(!precise.is_empty());
    assert_eq!(precise, parallel);
}

/// 移动中的活塞：`moving_piston` 的碰撞形状来自方块实体，只读视图在这一格要和
/// 精确驱动给出同一个 `isFaceSturdy` 答案。物品泡在下落的水里时，
/// `FlowingFluid.getFlow` 会查四周这一面结不结实，两种驱动因此必须逐刻一致。
fn moving_piston_entity_snapshots(threads: usize) -> Vec<(Vec3, Vec3, i32, bool)> {
    use mc_bamboo_blocks::{
        BlockState, archive::create_block_entity, entity::Entity, state::LEVEL,
    };
    use mc_bamboo_data::blocks;
    use mc_bamboo_nbt::Tag;
    use mc_bamboo_protocol::item::ItemStack;

    let mut server = Harness::new(2);
    let client = server.join_and_settle("Kitten");
    client.take_frames();
    server.set_entity_threads(threads);

    let piston_pos = BlockPos::new(9, -60, 8);
    server.set_block(piston_pos, blocks::MOVING_PISTON);
    let tag = Tag::Compound(vec![
        (
            "blockState".to_owned(),
            Tag::String("minecraft:stone".to_owned()),
        ),
        ("facing".to_owned(), Tag::Byte(1)),
        ("progress".to_owned(), Tag::Float(0.5)),
        ("extending".to_owned(), Tag::Byte(1)),
        ("source".to_owned(), Tag::Byte(0)),
    ]);
    let entity = create_block_entity(
        "minecraft:piston",
        piston_pos,
        BlockState(blocks::MOVING_PISTON),
        &tag,
    )
    .expect("活塞方块实体")
    .expect("类型已知");
    server.region().block_entities.insert(entity);

    // 下落的水在活塞旁边，物品泡在里面。流动的水会查四周的 `isFaceSturdy`。
    let water = BlockPos::new(8, -60, 8);
    server.set_block(water, BlockState::WATER.with(LEVEL, 8).0);
    server.set_block(water.above(), BlockState::WATER.0);
    let mut item = Entity::ground_item_with_delta(
        Vec3::new(8.5, -59.5, 8.5),
        ItemStack::new(1, 1),
        Vec3::new(0.0, 0.0, 0.0),
    );
    item.ground_mut().unwrap().set_pickup_delay(1000);
    server.spawn_entity(item);

    let _ = client.take_frames();
    let mut snapshots = Vec::new();
    for _ in 0..6 {
        server.tick();
        for uid in server.world_region().entities.ids() {
            let entity = server.world_region().entities.get(uid).expect("物品还在");
            snapshots.push((
                entity.position,
                entity.delta,
                entity.ground().map_or(-1, |item| item.age),
                entity.on_ground,
            ));
        }
    }
    snapshots
}

#[test]
fn moving_piston_entities_match_in_both_drivers() {
    let precise = moving_piston_entity_snapshots(0);
    let parallel = moving_piston_entity_snapshots(4);
    assert!(!precise.is_empty());
    assert_eq!(precise, parallel);
}

/// 掉落物和下落方块落在一台正在伸缩的活塞上。形状来自方块实体，两种驱动都必须
/// 用同一份按位置拷出的形状，逐刻位置、速度、落地标记要一样。
///
/// Items and falling blocks land on a piston mid-animation. The shape comes from the
/// block entity, so both drivers must use the same per-position snapshot and agree
/// on every tick's position, velocity and grounding.
fn landing_on_moving_piston(threads: usize) -> Vec<(i32, Vec3, Vec3, bool)> {
    use mc_bamboo_blocks::{BlockState, archive::create_block_entity, entity::Entity};
    use mc_bamboo_data::blocks;
    use mc_bamboo_nbt::Tag;
    use mc_bamboo_protocol::item::ItemStack;

    let mut server = Harness::new(2);
    let client = server.join_and_settle("Kitten");
    client.take_frames();
    server.set_entity_threads(threads);

    // 这一格是 moving_piston，形状是「被推的石头」朝上平移的半格；再放一台正在
    // 缩回的源活塞，两种方向的形状都要被实体碰到。
    let extending = BlockPos::new(9, -60, 8);
    let retracting = BlockPos::new(11, -60, 8);
    for pos in [extending, retracting] {
        server.set_block(pos, blocks::MOVING_PISTON);
    }
    let piston = |pos: BlockPos, tag: Vec<(String, Tag)>| {
        create_block_entity(
            "minecraft:piston",
            pos,
            BlockState(blocks::MOVING_PISTON),
            &Tag::Compound(tag),
        )
        .expect("活塞方块实体")
        .expect("类型已知")
    };
    server.region().block_entities.insert(piston(
        extending,
        vec![
            (
                "blockState".to_owned(),
                Tag::String("minecraft:stone".to_owned()),
            ),
            ("facing".to_owned(), Tag::Byte(1)),
            ("progress".to_owned(), Tag::Float(0.5)),
            ("extending".to_owned(), Tag::Byte(1)),
            ("source".to_owned(), Tag::Byte(0)),
        ],
    ));
    server.region().block_entities.insert(piston(
        retracting,
        vec![
            (
                "blockState".to_owned(),
                Tag::String("minecraft:piston".to_owned()),
            ),
            ("facing".to_owned(), Tag::Byte(1)),
            ("progress".to_owned(), Tag::Float(0.5)),
            ("extending".to_owned(), Tag::Byte(0)),
            ("source".to_owned(), Tag::Byte(1)),
        ],
    ));

    let mut item = Entity::ground_item_with_delta(
        Vec3::new(9.5, -57.5, 8.5),
        ItemStack::new(1, 1),
        Vec3::default(),
    );
    item.ground_mut().unwrap().set_pickup_delay(1000);
    server.spawn_entity(item);
    server.spawn_entity(Entity::falling_block(
        BlockPos::new(11, -56, 8),
        BlockState(blocks::SAND),
    ));

    let mut snapshots = Vec::new();
    for _ in 0..16 {
        server.tick();
        for uid in server.world_region().entities.ids() {
            let entity = server.world_region().entities.get(uid).expect("实体还在");
            snapshots.push((entity.id, entity.position, entity.delta, entity.on_ground));
        }
    }
    snapshots
}

#[test]
fn falling_entities_land_on_moving_pistons_in_both_drivers() {
    let precise = landing_on_moving_piston(0);
    let parallel = landing_on_moving_piston(4);
    // 有方块实体时物品会停在移动活塞平移后的那一层，不会一直往下掉。
    assert!(
        precise
            .iter()
            .any(|(_, position, _, on_ground)| *on_ground && (position.y - -59.0).abs() < 1.0e-6),
        "有方块实体时实体要落在移动活塞上"
    );
    assert!(!precise.is_empty());
    assert_eq!(precise, parallel);
}

/// 没有方块实体时官方给 `Shapes.empty()`，实体要穿过去。两种驱动也一致。
#[test]
fn moving_piston_without_block_entity_is_empty_in_both_drivers() {
    fn fall(threads: usize) -> Vec<(Vec3, bool)> {
        use mc_bamboo_blocks::{BlockState, entity::Entity};
        use mc_bamboo_data::blocks;

        let mut server = Harness::new(2);
        let client = server.join_and_settle("Kitten");
        client.take_frames();
        server.set_entity_threads(threads);
        server.set_block(BlockPos::new(9, -60, 8), blocks::MOVING_PISTON);
        server.spawn_entity(Entity::falling_block(
            BlockPos::new(9, -56, 8),
            BlockState(blocks::SAND),
        ));
        let mut snapshots = Vec::new();
        for _ in 0..6 {
            server.tick();
            let region = server.world_region();
            let uid = *region.entities.ids().first().expect("下落方块还在");
            let entity = region.entities.get(uid).expect("下落方块还在");
            snapshots.push((entity.position, entity.on_ground));
        }
        snapshots
    }
    let precise = fall(0);
    let parallel = fall(4);
    // 没有方块实体时形状是空的，这几刻里不会落地。
    assert!(
        precise.iter().all(|(_, on_ground)| !on_ground),
        "没有方块实体时 moving_piston 是空形状"
    );
    assert!(!precise.is_empty());
    assert_eq!(precise, parallel);
}

/// 开着盖子的潜影盒、随机偏移的竹子、细雪和脚手架：这些 `dynamicShape` 方块的
/// 碰撞形状按状态或方块实体变化。两种驱动逐刻都要读到同一份形状。
fn dynamic_shape_entities(threads: usize) -> Vec<(i32, Vec3, Vec3, bool)> {
    use mc_bamboo_blocks::{BlockState, ShulkerBoxEntity, entity::Entity};
    use mc_bamboo_data::blocks;
    use mc_bamboo_protocol::item::ItemStack;

    let mut server = Harness::new(2);
    let client = server.join_and_settle("Kitten");
    client.take_frames();
    server.set_entity_threads(threads);

    let base_y = -60;
    let cells = [
        (8, blocks::SHULKER_BOX),
        (10, blocks::BAMBOO),
        (12, blocks::POWDER_SNOW),
        (14, blocks::SCAFFOLDING),
        (16, blocks::POINTED_DRIPSTONE),
    ];
    for (x, state) in cells {
        server.set_block(BlockPos::new(x, base_y, 8), state);
    }
    let shulker_pos = BlockPos::new(8, base_y, 8);
    let mut shulker = ShulkerBoxEntity::new(shulker_pos);
    shulker.set_progress(1.0);
    server.region().block_entities.insert(Box::new(shulker));

    // 落上、横穿（贴着一侧）、从下面竖直穿过，三种撞法都要覆盖。
    for (x, _) in cells {
        let mut item = Entity::ground_item_with_delta(
            Vec3::new(f64::from(x) + 0.5, f64::from(base_y) + 4.0, 8.5),
            ItemStack::new(1, 1),
            Vec3::default(),
        );
        item.ground_mut().unwrap().set_pickup_delay(1000);
        server.spawn_entity(item);
        let mut side = Entity::ground_item_with_delta(
            Vec3::new(f64::from(x) + 0.5, f64::from(base_y) + 0.5, 8.5),
            ItemStack::new(1, 1),
            Vec3::new(0.6, 0.0, 0.0),
        );
        side.ground_mut().unwrap().set_pickup_delay(1000);
        server.spawn_entity(side);
    }
    server.spawn_entity(Entity::falling_block(
        BlockPos::new(8, base_y + 4, 8),
        BlockState(blocks::SAND),
    ));

    let mut snapshots = Vec::new();
    for _ in 0..20 {
        server.tick();
        for uid in server.world_region().entities.ids() {
            let entity = server.world_region().entities.get(uid).expect("实体还在");
            snapshots.push((entity.id, entity.position, entity.delta, entity.on_ground));
        }
    }
    snapshots
}

#[test]
fn dynamic_shape_entities_match_in_both_drivers() {
    let precise = dynamic_shape_entities(0);
    let parallel = dynamic_shape_entities(4);
    assert!(precise.len() > 100, "要有逐刻的实体快照");
    assert_eq!(precise, parallel);
}

/// 崩溃测试的具体方块路径：`/setblock` 之后 `save-all flush`，
/// 盘上要真的能读出那一格（用同一个 `world-block` 的解码路径）。
#[test]
fn setblock_then_save_all_flush_persists_the_block() {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_storage::{
        anvil::{RegionCompression, RegionFile},
        world_dir::WorldDir,
    };

    let tmp = D6TempDir::new("setblock-persist");
    let dir = WorldDir::new(&tmp.0);
    dir.create_layout().unwrap();
    let mut server = Harness::new_loaded_with_save(2, &tmp.0);
    let client = server.join("Admin");
    server.tick();
    client.take_frames();
    let pos = BlockPos::new(5, -59, 5);
    let chunk = ChunkPos::new(0, 0);
    server.ensure_loaded(&[chunk]);
    server
        .world
        .islands
        .region
        .players
        .get_mut(&client.id)
        .unwrap()
        .permission_level = 4;

    server.request(
        &client,
        PlayRequest::unsigned_command("setblock 5 -59 5 minecraft:diamond_block"),
    );
    server.tick();
    assert_eq!(
        server.world.islands.region.chunks.block(pos),
        Some(blocks::DIAMOND_BLOCK)
    );
    server
        .world
        .master
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveAll {
            flush: true,
            source: client.id,
        });
    for _ in 0..6 {
        server.tick();
    }
    server.world.master.host.flush_io();
    server.tick();
    let path = dir.region_dir().join("r.0.0.mca");
    assert!(path.exists(), "区块区域文件已经写出");
    let mut region =
        RegionFile::open(&path, dir.region_dir(), RegionCompression::Zlib).expect("区域文件能打开");
    let bytes = region.read(chunk, 32 << 20).unwrap().expect("区块在盘上");
    let decoded = mc_bamboo_storage::chunk_nbt::decode_bytes(&bytes).expect("官方格式能解");
    assert_eq!(
        decoded.column.chunk().block(5, -59, 5),
        blocks::DIAMOND_BLOCK,
        "kill -9 之前改的那一格要在盘上"
    );
}

/// 每个世界一个实体 ID 分配器，从 `FIRST_ENTITY_ID` 起；两个世界互不影响
/// （不是进程级静态量），岛的实体表拿的就是这个世界的句柄。
///
/// One entity-id allocator per world, starting at `FIRST_ENTITY_ID`; two worlds
/// stay independent (no process-wide static), and the island's store holds the
/// world's handle.
#[test]
fn each_world_has_its_own_entity_id_allocator() {
    use mc_bamboo_blocks::entity::{Entity, FIRST_ENTITY_ID};
    let mut first = Harness::new(8);
    let second = Harness::new(8);
    let item = || {
        Entity::ground_item(
            mc_bamboo_core::Vec3::new(0.5, -60.0, 0.5),
            mc_bamboo_protocol::item::ItemStack::new(1, 1),
        )
    };
    let entities = &mut first.world.islands.region.entities;
    assert_eq!(entities.spawn(item()), FIRST_ENTITY_ID);
    assert_eq!(entities.spawn(item()), FIRST_ENTITY_ID + 1);
    let other = &second.world.islands.region.entities;
    assert_eq!(other.ids_handle().peek(), FIRST_ENTITY_ID);
    assert!(!other.ids_handle().same_counter(entities.ids_handle()));
}

/// 岛的时钟从世界时间起算、每刻和它一起加一；计划刻队列的时钟跟的是岛的时钟，
/// 不是世界时间：把岛的时钟平移之后，队列的 `now` 跟着走，世界时间不动。
///
/// The island clock starts at the world time and advances with it; the
/// scheduled-tick clock follows the island clock, not the world time.
#[test]
fn the_island_clock_drives_scheduled_ticks() {
    let mut server = Harness::new(8);
    for _ in 0..5 {
        server.tick();
        let region = &server.world.islands.region;
        let time = server.world.master.global.time().0;
        assert_eq!(region.tick.game_time(), time);
        assert_eq!(region.block_ticks.now(), time);
    }
    server.world.islands.region.rebase_clock(1_000);
    server.tick();
    let time = server.world.master.global.time().0;
    let region = &server.world.islands.region;
    assert_eq!(time, 6);
    assert_eq!(region.tick.game_time(), 1_006);
    assert_eq!(region.block_ticks.now(), 1_006);
}

/// `/damage` 对玩家：难度缩放、护甲、吸收和出站包。这一期血量归零不触发死亡。
#[test]
fn damage_command_hurts_a_player_and_sends_health_packets() {
    let mut server = Harness::new(8);
    let client = server.join("Kitten");
    server.tick();
    client.take_frames();
    server
        .world
        .islands
        .region
        .players
        .get_mut(&client.id)
        .unwrap()
        .permission_level = 4;

    // 缺省是创造模式（能力无敌），先切到生存。
    server.request(
        &client,
        PlayRequest::unsigned_command("gamemode survival @s"),
    );
    server.tick();
    client.take_frames();
    server.request(&client, PlayRequest::unsigned_command("damage @s 5"));
    server.tick();
    let player = server.world.islands.region.players.get(&client.id).unwrap();
    assert_eq!(player.health.health, 15.0);
    assert_eq!(player.health.damage_cooldown, 19);
    assert_eq!(player.health.hurt_time, 9);
    let ids: Vec<i32> = client.take_frames().iter().map(|frame| frame.id).collect();
    assert!(ids.contains(&clientbound::SET_HEALTH), "{ids:?}");
    assert!(ids.contains(&clientbound::DAMAGE_EVENT), "{ids:?}");

    // 冷却期内同样大小的伤害打不进来；`/damage` 明确报错。
    server.request(&client, PlayRequest::unsigned_command("damage @s 5"));
    server.tick();
    assert_eq!(
        server
            .world
            .islands
            .region
            .players
            .get(&client.id)
            .unwrap()
            .health
            .health,
        15.0
    );
}

/// 左键打牛：一次 `player_attack` 伤害，空手伤害是 2。
#[test]
fn left_click_attack_hurts_a_cow() {
    use mc_bamboo_blocks::entity::Entity;

    let mut server = Harness::new(8);
    let client = server.join("Kitten");
    server.tick();
    client.take_frames();
    let region = &mut server.world.islands.region;
    // 牛放在玩家身边，够距判定才会通过。
    let position = region.players.get(&client.id).unwrap().position();
    let cow = region.entities.spawn(Entity::cow(position));
    server.request(&client, PlayRequest::Attack { entity_id: cow });
    server.tick();
    let health = server
        .world
        .islands
        .region
        .entities
        .public_at(cow)
        .and_then(|public| match &public.kind {
            mc_bamboo_blocks::entity::KindPublic::Mob(mob) => Some(mob.health),
            _ => None,
        });
    assert_eq!(health, Some(8.0), "空手一次攻击扣 2 点");
}
