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
use crate::{
    InFlight, Outgoing, PlayerSink,
    chunk_encoding::encode_chunk,
    links::encode_frames,
    profiler::{Metric, Phase, Series},
};

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
    pub(crate) world: World,
    inbox: Sender<Inbound>,
    in_flight: Arc<AtomicUsize>,
    next_id: i32,
}

impl Harness {
    /// 默认走「无限超平坦」测试世界：现有单元测试/oracle 都假设哪里都有地。
    /// `BAMBOO_TEST_LOADED=1` 让原本走无限模式的整套测试改走已加载模式，
    /// 只用于「有多少测试依赖隐式存在」的报告，不改变默认行为。
    pub(crate) fn new(view_distance: i32) -> Self {
        let loaded = std::env::var("BAMBOO_TEST_LOADED").is_ok_and(|value| value == "1");
        Self::build(view_distance, loaded)
    }

    /// 正式入口（已加载模式）：起点没有加载任何区块，测试自己交付。
    pub(crate) fn new_loaded(view_distance: i32) -> Self {
        Self::build(view_distance, true)
    }

    /// 正式入口 + 真实世界目录：`SaveSession` 和 IO 线程都接上，
    /// 供 `/save-off` / `save-all` / 关服这类生命周期测试用。
    /// Real world directory: both `SaveSession` and the IO thread are attached,
    /// for `/save-off` / `save-all` / shutdown lifecycle tests.
    pub(crate) fn new_loaded_with_save(view_distance: i32, root: &std::path::Path) -> Self {
        let (inbox, receiver) = mpsc::channel();
        let config = WorldConfig {
            view_distance,
            world_dir: Some(root.to_path_buf()),
            ..Default::default()
        };
        let mut world = World::new(config, receiver);
        world.region.random = LevelRandom::new(0);
        world.region.sound_seed_generator = LevelRandom::new(0);
        Self {
            world,
            inbox,
            in_flight: Arc::default(),
            next_id: 1,
        }
    }

    fn build(view_distance: i32, loaded: bool) -> Self {
        let (inbox, receiver) = mpsc::channel();
        let config = WorldConfig {
            view_distance,
            max_players: 20,
            online_mode: false,
            enforce_secure_profile: false,
            admin: Default::default(),
            ..Default::default()
        };
        let mut world = if loaded {
            World::new(config, receiver)
        } else {
            World::new_infinite_flat_for_tests(config, receiver)
        };
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
            online_at_login: 0,
            player_data: None,
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
        };
        self.inbox.send(Inbound::Join(Box::new(request))).unwrap();
        Client { id, uuid, sink }
    }

    /// 直接改世界出生点（`LevelData` 里的那份），测试出生点包用。
    /// Set the world spawn (the one in `LevelData`), for spawn-packet tests.
    pub(crate) fn set_spawn(&mut self, spawn: crate::world_state::RespawnData) {
        let _ = self.world.global.set_spawn(spawn);
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

    /// 让 `positions` 里的区块真的加载：走正式的加票 → 生成流水线 → 交付路径。
    /// 无限超平坦模式下每个位置本来就存在，直接返回；已加载模式下用 `Forced`
    /// 票（不过期）保持区块，再按需要推进几刻等交付预算把它们发到 region。
    pub(crate) fn ensure_loaded(&mut self, positions: &[ChunkPos]) {
        if !self.world.region.chunks.is_loaded_mode() {
            return;
        }
        for &pos in positions {
            self.apply_ticket_for_test(mc_bamboo_world::director::TicketOp::Forced {
                pos,
                forced: true,
            });
        }
        // 交付每刻有个数上限，脚下 3×3 豁免；这里循环到全部到位为止。
        for _ in 0..64 {
            if positions
                .iter()
                .all(|&pos| self.world.region.chunks.is_loaded(pos))
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
        self.world.region.profiler.reset_samples();
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
            let h = self.world.region.profiler.histogram(series);
            out.insert(name, (h.count(), h.sum(), h.p50(), h.p99(), h.max()));
        }
        out
    }

    /// 给其他模块的测试直接看全服状态。
    pub(crate) fn global(&self) -> &crate::global::Global {
        &self.world.global
    }

    /// 区块帧缓存里的共享帧，按坐标取。版本号变了就返回 `None`。
    pub(crate) fn shared_chunk_frame(&self, pos: ChunkPos) -> Option<Arc<crate::SharedFrames>> {
        self.world
            .region
            .chunk_cache
            .frame(&self.world.region.chunks, pos)
    }

    /// 缓存里的区块帧条目数。
    pub(crate) fn cached_frame_count(&self) -> usize {
        self.world.region.chunk_cache.frame_count()
    }

    /// 缓存里有没有空帧。
    pub(crate) fn has_empty_cached_frame(&self) -> bool {
        self.world.region.chunk_cache.has_empty_frame()
    }

    /// 给其他模块的测试直接看世界的状态。
    /// 只读 region（测试断言用）。
    pub(crate) fn world_region(&self) -> &Region {
        &self.world.region
    }

    pub(crate) fn region(&mut self) -> &mut Region {
        &mut self.world.region
    }

    /// 测试并行驱动时直接设线程数，不碰进程级环境变量（同一进程里测试并行跑）。
    pub(crate) fn set_entity_threads(&mut self, threads: usize) {
        self.world.entity_threads = threads;
    }

    /// 已加载模式的测试专用：本刻交付/收回的个数、信箱里还欠着的个数。
    pub(crate) fn director_mail(&self) -> (usize, usize, (usize, usize)) {
        let (delivered, reclaimed) = self.world.host.counters_this_tick();
        (delivered, reclaimed, self.world.host.mail_backlog())
    }

    /// 这一刻从 region 取回的区块数（不含直接从信箱退回的盒子）。
    pub(crate) fn region_reclaims_this_tick(&self) -> usize {
        self.world.host.region_reclaims_this_tick()
    }

    /// 测试专用：改这一刻之后的交付/收回上限（不碰进程级环境变量）。
    pub(crate) fn set_mail_budget(&mut self, budget: crate::director_host::MailBudget) {
        self.world.host.set_budget(budget);
    }

    /// 测试专用：Director 里已交付/在途的区块数。
    #[cfg(test)]
    pub(crate) fn director_loaded_count(&self) -> usize {
        self.world.host.director().loaded_count()
    }

    /// 测试专用：停放表里的盒子数。
    #[cfg(test)]
    pub(crate) fn parked_count(&self) -> usize {
        self.world.host.parked_count()
    }

    /// 测试专用：region 里已加载的区块坐标（按坐标排序）。
    #[cfg(test)]
    pub(crate) fn loaded_positions(&self) -> Vec<mc_bamboo_core::ChunkPos> {
        self.world
            .region
            .chunks
            .edited_chunks()
            .map(|(pos, _)| pos)
            .filter(|&pos| self.world.region.chunks.is_loaded(pos))
            .collect()
    }

    /// 测试专用：Director 里每条记录的归属名和代数。
    #[cfg(test)]
    pub(crate) fn director_records(
        &self,
    ) -> std::collections::BTreeMap<mc_bamboo_core::ChunkPos, (String, u64)> {
        self.world
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
        self.world.host.set_reclaim_delay(delay);
    }

    /// 给世界里直接塞一个实体，绕过玩家动作。
    pub(crate) fn spawn_entity(&mut self, entity: mc_bamboo_blocks::entity::Entity) -> i32 {
        let (mut level, ticks) = self.world.region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        world.spawn_entity(entity)
    }

    /// 给世界里直接写一个方块状态，绕过玩家动作。
    pub(crate) fn set_block(&mut self, pos: BlockPos, state: u32) {
        self.world.region.chunks.set_block(pos, state);
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
        online_at_login: 0,
        player_data: None,
        sink: Box::new(sink),
    };
    server.inbox.send(Inbound::Join(Box::new(request))).unwrap();
    server.tick();

    let ids: Vec<PlayerId> = server.world.region.players.keys().copied().collect();
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
    server.world.host.set_io(ChunkIo::open(&dir).unwrap());

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
    };
    server.inbox.send(Inbound::Join(Box::new(request))).unwrap();
    server.tick();

    // 重进拿到的是离开时的旁观模式（待写内存副本），不是盘上的生存模式旧档。
    let rejoined = server.world.region.players.get(&new_id).unwrap();
    assert_eq!(
        rejoined.game_mode,
        GameMode::Spectator,
        "rejoin must use the in-flight leaving save, not the stale disk bytes"
    );
}

/// 临时世界目录，测试结束自动删。IO 线程要在删目录前关掉，所以 tmp 先声明、
/// server 后声明（drop 时 server 先关）。
struct D6TempDir(std::path::PathBuf);

impl D6TempDir {
    fn new(tag: &str) -> Self {
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
fn written_chunks(host: &crate::director_host::DirectorHost) -> Vec<mc_bamboo_core::ChunkPos> {
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
    server.world.host.set_io(ChunkIo::open(&dir).unwrap());
    server.set_block(mc_bamboo_core::BlockPos::new(3, -60, 3), blocks::GLOWSTONE);
    server.world.region.touch(BlockPos::new(3, -60, 3));
    // 只读快照照复查「中 6」会推迟有待算光照的区块，先让光照算完。
    server.world.region.chunks.finish_light_updates();
    // 和正式路径一样走写路径的改动标记；测试直接改 ChunkStore 时补一下。
    server
        .world
        .region
        .touch(mc_bamboo_core::BlockPos::new(3, -60, 3));

    // 有待算光照时快照按复查「中 6」推迟；先跑几刻让光照算完再重试。
    for t in 1..8 {
        server
            .world
            .host
            .save_loaded_dirty(&mut server.world.region, t, true);
    }
    server.world.host.flush_io();

    assert!(
        server.world.region.chunks.is_loaded(pos),
        "snapshot must not unload the chunk"
    );
    assert!(
        written_chunks(&server.world.host).contains(&pos),
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
            .host
            .save_loaded_dirty(&mut server.world.region, t, true);
        server.world.host.flush_io();
        assert!(
            !written_chunks(&server.world.host).contains(&pos),
            "a clean chunk must not be re-saved inside the cooldown"
        );
    }
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
    server.world.host.set_io(ChunkIo::open(&dir).unwrap());
    server.set_block(mc_bamboo_core::BlockPos::new(3, -60, 3), blocks::GLOWSTONE);
    server.world.region.touch(BlockPos::new(3, -60, 3));

    server
        .world
        .host
        .save_loaded_dirty(&mut server.world.region, 1, false);
    server.world.host.flush_io();
    assert!(
        written_chunks(&server.world.host).is_empty(),
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
    server.world.host.set_io(ChunkIo::open(&dir).unwrap());
    server.set_block(mc_bamboo_core::BlockPos::new(3, -60, 3), blocks::GLOWSTONE);
    server.world.region.touch(BlockPos::new(3, -60, 3));
    server.set_block(mc_bamboo_core::BlockPos::new(19, -60, 3), blocks::GLOWSTONE);
    server.world.region.touch(BlockPos::new(19, -60, 3));
    server
        .world
        .region
        .touch(mc_bamboo_core::BlockPos::new(3, -60, 3));
    server
        .world
        .region
        .touch(mc_bamboo_core::BlockPos::new(19, -60, 3));
    server.world.region.chunks.finish_light_updates();

    server
        .world
        .host
        .snapshot_all_dirty(&mut server.world.region, 1);
    server.world.host.flush_io();
    let written = written_chunks(&server.world.host);
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
    server.world.region.touch(BlockPos::new(3, -60, 3));

    server
        .world
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveOff);
    server.tick();
    assert!(
        !server.world.save.as_ref().unwrap().auto_save(),
        "save-off must reach the SaveSession (noSave)"
    );
    // save-off 期间匀速不写：先确认没有 Written。
    server.world.host.flush_io();
    assert!(written_chunks(&server.world.host).is_empty());

    server
        .world
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveAll {
            flush: false,
            source: crate::inbound::PlayerId(1),
        });
    server.tick();
    server.world.host.flush_io();
    assert!(
        written_chunks(&server.world.host).contains(&pos),
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
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveOff);
    server.tick();
    server.world.on_leave(client.id);
    server.world.host.flush_io();

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
    server.world.host.set_io(ChunkIo::open(&dir).unwrap());

    // 只往容器表放一个条目，不改任何方块：ChunkStore 的版本号不会变，
    // 写路径的改动标记要让它照样被 D6 写下去。
    let chest = mc_bamboo_core::BlockPos::new(3, -60, 3);
    server.world.region.touch(chest);
    server.world.region.containers.insert(
        chest,
        crate::container::ContainerEntry::new(crate::container::ContainerKind::Chest),
    );

    server
        .world
        .host
        .save_loaded_dirty(&mut server.world.region, 1, true);
    server.world.host.flush_io();
    assert!(
        written_chunks(&server.world.host).contains(&pos),
        "只改容器内容也要被 D6 写下去"
    );
}

/// 没 `flush`：`save-all` 成功消息在快照都交给 IO 之后这一刻就发。
#[test]
fn save_all_sends_success_after_handoff() {
    let mut server = Harness::new_loaded(2);
    server.world.region.send_command_feedback = true;
    let client = server.join("Admin");
    server.tick();
    client.take_frames();

    server
        .world
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveAll {
            flush: false,
            source: client.id,
        });
    server.tick();
    assert!(
        has_save_success(&client),
        "non-flush success is sent once snapshots are handed off"
    );
    assert!(server.world.pending_save_success.is_empty());
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
    server.world.region.send_command_feedback = true;
    let client = server.join("Admin");
    server.tick();
    client.take_frames();
    server.world.host.set_io(ChunkIo::open(&dir).unwrap());

    server
        .world
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
    assert_eq!(server.world.pending_save_success.len(), 1);

    // 让 IO 线程处理栅栏，再跑一刻让 poll_io 收回执、补发成功消息。
    server.world.host.flush_io();
    server.tick();
    assert!(
        has_save_success(&client),
        "success is sent after the IO barrier acked"
    );
    assert!(server.world.pending_save_success.is_empty());
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
    server.world.region.send_command_feedback = true;
    let client = server.join("Admin");
    server.tick();
    client.take_frames();
    server.world.host.set_io(ChunkIo::open(&dir).unwrap());

    server
        .world
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
    assert_eq!(server.world.pending_save_success.len(), 1);

    server.world.host.flush_io();
    server.tick();
    assert!(has_save_success(&client), "success follows the barrier ack");
    assert!(server.world.pending_save_success.is_empty());
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
    server.world.region.send_command_feedback = true;
    let client = server.join("Admin");
    server.tick();
    client.take_frames();
    let pos = mc_bamboo_core::ChunkPos::new(0, 0);
    server.ensure_loaded(&[pos]);
    server.set_block(mc_bamboo_core::BlockPos::new(3, -60, 3), blocks::GLOWSTONE);
    server.world.region.touch(BlockPos::new(3, -60, 3));
    server.world.host.set_io(io);

    server
        .world
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
    server.world.host.flush_io();
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
    server.world.host.set_io(ChunkIo::open(&dir).unwrap());

    // 故意留一个在途读请求：IO 线程死了也不能让 Director 永远等结果。
    // Leave a read in flight: a dead IO thread must not leave the Director waiting.
    server
        .world
        .host
        .io()
        .unwrap()
        .submit_read(mc_bamboo_core::ChunkPos::new(3, 3), 99, 0);
    server.world.host.io().unwrap().crash_for_test();
    for _ in 0..2000 {
        if server.world.host.io().unwrap().worker_stopped() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        server.world.host.io_failed(),
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
    server.ensure_loaded(&[edited]);
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
fn profiler_charges_every_phase_once_per_tick() {
    let mut server = Harness::new(2);
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
                .region
                .profiler
                .histogram(Series::Metric(metric))
                .count(),
            1,
            "{metric:?}"
        );
    }
    let report = server.world.region.profiler.report();
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
    let mut server = Harness::new(2);
    server.tick();
    server.world.region.profiler.reset_samples();

    server.inbox.send(Inbound::Shutdown).unwrap();
    assert!(server.world.tick().is_break());

    assert_eq!(
        server
            .world
            .region
            .profiler
            .histogram(Series::Phase(Phase::Inbox))
            .count(),
        0
    );
    assert_eq!(
        server
            .world
            .region
            .profiler
            .histogram(Series::Metric(Metric::TickTotal))
            .count(),
        1
    );
    assert_eq!(
        server
            .world
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
    server.world.region.profiler.reset_samples();

    // 确认上一批，下一刻继续发，延迟按「进入 pending 到写出」的刻数记。
    server.acknowledge_batch(&client, 10.0);
    server.tick();

    let latency = server
        .world
        .region
        .profiler
        .histogram(Series::Metric(Metric::ChunkLatencyUs));
    assert!(latency.count() > 0);
    let chunks_sent = server
        .world
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
        server.world.region.chunks.block(pos),
        Some(blocks::DIAMOND_BLOCK)
    );
    server
        .world
        .global
        .save_requests
        .push(crate::save_session::SaveCommand::SaveAll {
            flush: true,
            source: client.id,
        });
    for _ in 0..6 {
        server.tick();
    }
    server.world.host.flush_io();
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
