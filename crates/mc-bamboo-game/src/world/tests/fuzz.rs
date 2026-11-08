//! 用随机请求驱动世界，再用一个只看线上字节的客户端模型检查服务端有没有说错话。
//!
//! 请求里混着客户端能发出的任何值：NaN、无穷、世界外的坐标、乱序的确认。测试是 debug
//! 构建，整数溢出和数组越界都会 panic，所以这里同时证明了客户端的输入走不到 panic。
//! 随机数只来自种子，所有输出也都比较了一遍：两次运行不一样说明 tick 里有不确定的顺序。

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_core::{BlockPos, ChunkPos, Direction, Hand, Vec3};
use mc_bamboo_data::{
    blocks,
    items::{self, ITEMS},
    packets::play::clientbound,
    registries::data_component_type,
};
use mc_bamboo_protocol::{
    codec::{Reader, write_varint},
    components::{ComponentPatch, cursor::Cursor},
    item::ItemStack,
    packet::play::{
        ChatVisibility, ClientCommandKind, ClientInformation, EquipmentSlot, HumanoidArm,
        ParticleStatus, PlayRequest, PlayerActionKind, PlayerCommandKind, PlayerInfoUpdate, Pose,
    },
};
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT, chunk::Chunk};
use uuid::Uuid;

use super::{Client, Frame, Harness, chunk_position, section_blocks};
use crate::{
    chunk_tracking::ChunkView,
    inbound::{Inbound, PlayerId},
    player::Player,
};

/// xorshift64*。够乱，而且完全由种子决定。
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, limit: u64) -> u64 {
        (self.next() >> 11) % limit
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<T: Copy>(&mut self, values: &[T]) -> T {
        values[self.below(values.len() as u64) as usize]
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn around(&mut self, center: f64, radius: f64) -> f64 {
        center + (self.unit() * 2.0 - 1.0) * radius
    }

    fn int_around(&mut self, center: f64, radius: u64) -> i32 {
        // 位置可能是无穷，这里只是挑一个附近的方块。
        let center = (center.floor() as i32).clamp(-40_000_000, 40_000_000);
        center + self.below(radius * 2 + 1) as i32 - radius as i32
    }
}

const WORLD_TOP: i32 = MIN_Y + WORLD_HEIGHT;
/// 最后让世界安静下来的 tick 数。确认到了之后一 tick 能发 64 个区块，够发完 `T(8)`。
const SETTLE_TICKS: u32 = 40;
const HORIZONTAL_EXTREMES: [f64; 7] = [
    3.0e7,
    -3.0e7,
    3.1e7,
    -3.1e7,
    1.0e300,
    f64::INFINITY,
    f64::NEG_INFINITY,
];
const VERTICAL_SPOTS: [f64; 10] = [
    -60.0, -64.0, -65.5, 0.0, 317.0, 318.5, 319.5, 320.5, 2.1e7, -2.1e7,
];
const ANGLES: [f32; 8] = [0.0, 90.0, -90.0, 180.0, -180.0, 359.9, f32::MAX, f32::MIN];
const BAD_FLOATS: [f32; 3] = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
const COMMON_ITEMS: [i32; 17] = [
    items::STONE,
    items::DIRT,
    items::CHEST,
    items::OAK_SIGN,
    items::SHORT_GRASS,
    items::WATER_BUCKET,
    items::OAK_DOOR,
    items::COMMAND_BLOCK,
    items::DIAMOND_SWORD,
    items::TORCH,
    items::BELL,
    items::COMPOSTER,
    items::OAK_LOG,
    items::OAK_SLAB,
    items::IRON_CHAIN,
    items::LILY_PAD,
    items::SEA_PICKLE,
];

/// 客户端眼里的一个实体：元数据和装备。官方的默认值，血量的默认值是 1。
#[derive(Debug, Clone, PartialEq)]
struct Remote {
    flags: u8,
    pose: i32,
    health: f32,
    main_hand: i32,
    model_parts: u8,
    /// 下标是 `EquipmentSlot` 的 ordinal，值是物品和数量。
    equipment: [(i32, i32); 6],
}

impl Default for Remote {
    fn default() -> Self {
        Self {
            flags: 0,
            pose: Pose::Standing as i32,
            health: 1.0,
            main_hand: HumanoidArm::Right as i32,
            model_parts: 0,
            equipment: [(items::AIR, 0); 6],
        }
    }
}

impl Remote {
    fn read_metadata(&mut self, reader: &mut Reader<'_>) {
        loop {
            let index = reader.u8().unwrap();
            if index == 0xff {
                return;
            }
            reader.varint().unwrap();
            match index {
                0 => self.flags = reader.u8().unwrap(),
                6 => self.pose = reader.varint().unwrap(),
                9 => self.health = f32::from_bits(reader.i32().unwrap() as u32),
                15 => self.main_hand = reader.varint().unwrap(),
                16 => self.model_parts = reader.u8().unwrap(),
                other => panic!("metadata index {other}"),
            }
        }
    }

    fn read_equipment(&mut self, reader: &mut Reader<'_>) {
        loop {
            let slot = reader.u8().unwrap();
            let count = reader.varint().unwrap();
            self.equipment[usize::from(slot & 0x7f)] = if count > 0 {
                let item = reader.varint().unwrap();
                // 组件现在按官方原样发出去，客户端模型要把补丁读完才能接着读下一个槽。
                // Components are forwarded as vanilla writes them, so the model has to consume the patch.
                let consumed = {
                    let rest = reader.remaining();
                    let mut cursor = Cursor::new(rest);
                    ComponentPatch::read(&mut cursor).expect("equipment component patch");
                    rest.len() - cursor.remaining().len()
                };
                reader.bytes(consumed).unwrap();
                (item, count)
            } else {
                (items::AIR, 0)
            };
            if slot & 0x80 == 0 {
                return;
            }
        }
    }

    /// 服务端这名玩家现在的样子。
    fn of(player: &Player) -> Self {
        let pose = if player.sync.is_crouching() {
            Pose::Crouching
        } else {
            Pose::Standing
        };
        let mut equipment = [(items::AIR, 0); 6];
        for slot in [
            EquipmentSlot::MainHand,
            EquipmentSlot::OffHand,
            EquipmentSlot::Feet,
            EquipmentSlot::Legs,
            EquipmentSlot::Chest,
            EquipmentSlot::Head,
        ] {
            let stack = player.inventory.equipment(slot).expect("玩家有这个槽");
            if !stack.is_empty() {
                equipment[slot as usize] = (stack.item, stack.count);
            }
        }
        Self {
            flags: player.sync.shared_flags,
            pose: pose as i32,
            health: 20.0,
            main_hand: player.client.main_hand as i32,
            model_parts: player.client.model_customisation,
            equipment,
        }
    }
}

/// 客户端眼里的世界：只从收到的字节推出来。
struct Model {
    name: String,
    logged_in: bool,
    disconnected: bool,
    in_bundle: bool,
    /// 值是帽子显示没显示。
    tab: BTreeMap<Uuid, bool>,
    entities: BTreeMap<i32, Remote>,
    /// 自己的实体。只有元数据有意义，物品栏是客户端自己的。
    own: Remote,
    /// 值是收到时的区块内容，`None` 表示是模板。再加上之后的方块更新。
    chunks: BTreeMap<ChunkPos, Option<Chunk>>,
    updates: BTreeMap<ChunkPos, Vec<(BlockPos, u32)>>,
    center: Option<ChunkPos>,
    finished_batches: u32,
    /// 这一 tick 收到的 `block_changed_ack`。
    acknowledged: Option<i32>,
    /// 所有收到的字节的摘要，比较两次运行用。
    digest: u64,
}

impl Model {
    fn new(name: String) -> Self {
        Self {
            name,
            logged_in: false,
            disconnected: false,
            in_bundle: false,
            tab: BTreeMap::new(),
            entities: BTreeMap::new(),
            own: Remote::default(),
            chunks: BTreeMap::new(),
            updates: BTreeMap::new(),
            center: None,
            finished_batches: 0,
            acknowledged: None,
            digest: 0xcbf2_9ce4_8422_2325,
        }
    }

    fn mix(&mut self, frame: &Frame) {
        for byte in i32::to_be_bytes(frame.id).iter().chain(&frame.body) {
            self.digest = (self.digest ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    /// 处理这一 tick 收到的全部帧。`world` 用来给收到的区块拍快照，再看被配对的玩家在哪。
    fn observe(&mut self, self_id: i32, frames: &[Frame], world: &super::World) {
        for frame in frames {
            self.mix(frame);
            assert!(!self.disconnected, "{}: frame after disconnect", self.name);
            if !self.logged_in {
                assert_eq!(frame.id, clientbound::LOGIN, "{}: first frame", self.name);
                self.logged_in = true;
            }
            self.check_bundle(frame);
            self.apply(self_id, frame, world);
        }
        assert!(!self.in_bundle, "{}: bundle left open", self.name);
    }

    fn check_bundle(&mut self, frame: &Frame) {
        if frame.id == clientbound::BUNDLE_DELIMITER {
            self.in_bundle = !self.in_bundle;
        } else if self.in_bundle {
            assert!(
                [
                    clientbound::ADD_ENTITY,
                    clientbound::SET_ENTITY_DATA,
                    clientbound::SET_EQUIPMENT
                ]
                .contains(&frame.id),
                "{}: packet {} inside a bundle",
                self.name,
                frame.id
            );
        }
    }

    fn apply(&mut self, self_id: i32, frame: &Frame, world: &super::World) {
        let mut reader = Reader::new(&frame.body);
        match frame.id {
            clientbound::DISCONNECT => self.disconnected = true,
            clientbound::PLAYER_INFO_UPDATE => {
                let actions = reader.u8().unwrap();
                for _ in 0..reader.varint().unwrap() {
                    let (uuid, hat) = read_info_entry(&mut reader, actions);
                    let known = self.tab.contains_key(&uuid);
                    let added = actions == PlayerInfoUpdate::ALL_ACTIONS;
                    assert_eq!(known, !added, "{}: tab entry {uuid}", self.name);
                    if let Some(hat) = hat {
                        self.tab.insert(uuid, hat);
                    }
                }
            }
            clientbound::PLAYER_INFO_REMOVE => {
                for _ in 0..reader.varint().unwrap() {
                    let uuid = reader.uuid().unwrap();
                    let entry = self.tab.remove(&uuid);
                    assert!(entry.is_some(), "{}: removed ghost", self.name);
                }
            }
            clientbound::ADD_ENTITY => {
                let id = reader.varint().unwrap();
                let uuid = reader.uuid().unwrap();
                assert_ne!(id, self_id, "{}: own entity", self.name);
                assert!(self.tab.contains_key(&uuid), "{}: no tab entry", self.name);
                assert!(self.in_bundle, "{}: entity outside a bundle", self.name);
                let previous = self.entities.insert(id, Remote::default());
                assert!(previous.is_none(), "{}: entity twice", self.name);
                let target = &world.region.players[&PlayerId(id)];
                assert!(
                    self.chunks.contains_key(&target.chunk_pos()),
                    "{}: entity in a chunk it does not have",
                    self.name
                );
            }
            clientbound::REMOVE_ENTITIES => {
                for _ in 0..reader.varint().unwrap() {
                    let id = reader.varint().unwrap();
                    let entity = self.entities.remove(&id);
                    assert!(entity.is_some(), "{}: removed ghost", self.name);
                }
            }
            clientbound::MOVE_ENTITY_POS
            | clientbound::MOVE_ENTITY_POS_ROT
            | clientbound::MOVE_ENTITY_ROT
            | clientbound::ENTITY_POSITION_SYNC
            | clientbound::ROTATE_HEAD => {
                let id = reader.varint().unwrap();
                assert!(
                    self.entities.contains_key(&id),
                    "{}: packet {} for unknown entity {id}",
                    self.name,
                    frame.id
                );
            }
            clientbound::SWING_ANIMATION => {
                let id = reader.varint().unwrap();
                // 客户端没有预测的挥手也发给本人，`SwingSource.SERVER_ONLY`。
                assert!(
                    id == self_id || self.entities.contains_key(&id),
                    "{}: swing of unknown entity {id}",
                    self.name
                );
            }
            clientbound::SET_EQUIPMENT => {
                let id = reader.varint().unwrap();
                self.entities
                    .get_mut(&id)
                    .unwrap_or_else(|| panic!("{}: equipment of unknown {id}", self.name))
                    .read_equipment(&mut reader);
            }
            clientbound::SET_ENTITY_DATA => {
                let id = reader.varint().unwrap();
                let entity = if id == self_id {
                    &mut self.own
                } else {
                    self.entities
                        .get_mut(&id)
                        .unwrap_or_else(|| panic!("{}: metadata of unknown {id}", self.name))
                };
                entity.read_metadata(&mut reader);
            }
            clientbound::SET_CHUNK_CACHE_CENTER => {
                let (x, z) = (reader.varint().unwrap(), reader.varint().unwrap());
                self.center = Some(ChunkPos::new(x, z));
            }
            clientbound::LEVEL_CHUNK_WITH_LIGHT => {
                let pos = chunk_position(frame);
                // 改过的区块走编码缓存。发完区块以后这一 tick 没再改世界，收到的字节要和现在重新编码的一样。
                let chunks = &world.region.chunks;
                if !chunks.is_template(pos) {
                    let fresh = crate::chunk_encoding::encode_chunk(chunks, pos);
                    assert!(
                        frame.body[8..] == fresh[..],
                        "{}: stale chunk {pos:?}",
                        self.name
                    );
                }
                let center = self.center.expect("中心先于区块");
                let distance = (pos.x - center.x).abs().max((pos.z - center.z).abs());
                assert!(distance <= 35, "{}: chunk out of range", self.name);
                let snapshot = world.region.chunks.edited(pos).cloned();
                let previous = self.chunks.insert(pos, snapshot);
                assert!(previous.is_none(), "{}: chunk {pos:?} twice", self.name);
                self.updates.remove(&pos);
            }
            clientbound::FORGET_LEVEL_CHUNK => {
                let (z, x) = (reader.i32().unwrap(), reader.i32().unwrap());
                let pos = ChunkPos::new(x, z);
                assert!(
                    self.chunks.remove(&pos).is_some(),
                    "{}: forgot {pos:?} that it never had",
                    self.name
                );
                self.updates.remove(&pos);
            }
            clientbound::BLOCK_UPDATE => {
                let pos = reader.block_pos().unwrap();
                let state = reader.varint().unwrap();
                self.record_update(pos, state as u32);
            }
            clientbound::SECTION_BLOCKS_UPDATE => {
                let blocks = section_blocks(frame);
                let distinct: BTreeSet<BlockPos> = blocks.iter().map(|&(pos, _)| pos).collect();
                // 只改了一格的段发 `block_update`，一段里的格子不重复。
                assert!(
                    blocks.len() > 1,
                    "{}: section update of one block",
                    self.name
                );
                assert_eq!(
                    distinct.len(),
                    blocks.len(),
                    "{}: repeated block",
                    self.name
                );
                for (pos, state) in blocks {
                    self.record_update(pos, state);
                }
            }
            clientbound::BLOCK_CHANGED_ACK => {
                let previous = self.acknowledged.replace(reader.varint().unwrap());
                assert_eq!(previous, None, "{}: two acks in one tick", self.name);
            }
            clientbound::CHUNK_BATCH_FINISHED => self.finished_batches += 1,
            _ => {}
        }
    }

    /// 一格方块更新。官方客户端在没加载的区块和世界外写方块时什么也不做。
    fn record_update(&mut self, pos: BlockPos, state: u32) {
        assert_eq!(self.acknowledged, None, "{}: update after ack", self.name);
        assert!(state < blocks::TOTAL_STATES, "{}: state {state}", self.name);
        if self.chunks.contains_key(&pos.chunk()) && (MIN_Y..WORLD_TOP).contains(&pos.y) {
            self.updates
                .entry(pos.chunk())
                .or_default()
                .push((pos, state));
        }
    }

    /// 这个区块在客户端手里的方块，等于收到时的内容加上之后的更新。
    fn chunk(&self, pos: ChunkPos, world: &super::World) -> Chunk {
        let received = self.chunks.get(&pos).expect("区块在客户端手里");
        let mut chunk = received
            .clone()
            .unwrap_or_else(|| world.region.chunks.template().clone());
        for (block, state) in self.updates.get(&pos).into_iter().flatten() {
            chunk.set_block(local(block.x), block.y, local(block.z), *state);
        }
        chunk
    }
}

const fn local(coordinate: i32) -> usize {
    (coordinate & 15) as usize
}

/// 返回这一项的 UUID 和帽子的状态。这一项不带帽子时是 `None`。
fn read_info_entry(reader: &mut Reader<'_>, actions: u8) -> (Uuid, Option<bool>) {
    let uuid = reader.uuid().unwrap();
    let hat = match actions {
        PlayerInfoUpdate::ALL_ACTIONS => {
            reader.string(16).unwrap();
            assert_eq!(reader.varint().unwrap(), 0);
            assert!(!reader.bool().unwrap());
            reader.varint().unwrap();
            reader.bool().unwrap();
            reader.varint().unwrap();
            assert!(!reader.bool().unwrap());
            reader.varint().unwrap();
            Some(reader.bool().unwrap())
        }
        PlayerInfoUpdate::UPDATE_LATENCY => {
            reader.varint().unwrap();
            None
        }
        PlayerInfoUpdate::UPDATE_HAT => Some(reader.bool().unwrap()),
        other => panic!("unexpected actions {other:#x}"),
    };
    (uuid, hat)
}

struct Actor {
    client: Client,
    model: Model,
    position: Vec3,
    /// 会发非法值的玩家。别的玩家只发正常客户端发得出来的东西，这样才能活到后面的状态。
    hostile: bool,
    /// 被踢或者决定走了之后，再过这么多 tick 发 `Leave`。
    leaving_in: Option<u32>,
    /// 这一 tick 发出去的请求里最大的 sequence。
    expected_ack: Option<i32>,
    /// 被踢时的位置。之后的请求世界都不理。
    kicked_at: Option<Vec3>,
}

impl Actor {
    fn is_alive(&self) -> bool {
        !self.client.is_closed() && self.leaving_in.is_none()
    }
}

fn random_block(rng: &mut Rng, around: Vec3) -> BlockPos {
    if rng.chance(3) {
        return BlockPos::unpack(rng.next() as i64);
    }
    let y = if rng.chance(10) {
        rng.pick(&[
            MIN_Y - 1,
            MIN_Y,
            MIN_Y + 1,
            WORLD_TOP - 1,
            WORLD_TOP,
            2047,
            -2048,
        ])
    } else {
        rng.int_around(around.y, 5)
    };
    BlockPos::new(rng.int_around(around.x, 6), y, rng.int_around(around.z, 6))
}

fn random_position(rng: &mut Rng, around: Vec3, hostile: bool) -> Vec3 {
    let roll = rng.below(100);
    if hostile && roll < 1 {
        return Vec3::new(f64::NAN, around.y, around.z);
    }
    if roll < 4 {
        let extreme = rng.pick(&HORIZONTAL_EXTREMES);
        return Vec3::new(extreme, around.y, rng.pick(&HORIZONTAL_EXTREMES));
    }
    if roll < 12 {
        return Vec3::new(
            rng.around(around.x, 400.0),
            rng.pick(&VERTICAL_SPOTS),
            rng.around(around.z, 400.0),
        );
    }
    if roll < 18 {
        return Vec3::new(8.5, -60.0, 8.5);
    }
    Vec3::new(
        rng.around(around.x, 2.0),
        around.y,
        rng.around(around.z, 2.0),
    )
}

fn random_angle(rng: &mut Rng, hostile: bool) -> f32 {
    if hostile && rng.chance(1) {
        rng.pick(&BAD_FLOATS)
    } else if rng.chance(20) {
        rng.pick(&ANGLES)
    } else {
        rng.around(0.0, 180.0) as f32
    }
}

fn random_rotation(rng: &mut Rng, hostile: bool) -> (f32, f32) {
    (random_angle(rng, hostile), random_angle(rng, hostile))
}

fn random_stack(rng: &mut Rng) -> ItemStack {
    if rng.chance(15) {
        return ItemStack::empty();
    }
    let item = if rng.chance(70) {
        rng.pick(&COMMON_ITEMS)
    } else {
        rng.below(ITEMS.len() as u64) as i32
    };
    let mut stack = ItemStack::new(item, rng.below(99) as i32 + 1);
    if rng.chance(10) {
        // 创造模式槽位的解码器会拒掉解不开的组件。这里只放一个合法的 `max_stack_size`，
        // 装备包才能按官方补丁格式读完。
        //
        // The creative-slot decoder rejects a component it cannot decode. Only a valid
        // `max_stack_size` is attached here, so the equipment packet stays readable.
        let mut value = Vec::new();
        write_varint(&mut value, rng.below(99) as i32 + 1);
        stack
            .added
            .push((data_component_type::MAX_STACK_SIZE, value));
    }
    if rng.chance(5) {
        stack.removed.push(rng.below(122) as i32);
    }
    stack
}

fn random_sequence(rng: &mut Rng, hostile: bool) -> i32 {
    if hostile && rng.chance(1) {
        rng.pick(&[-1, i32::MIN])
    } else if rng.chance(1) {
        i32::MAX
    } else {
        rng.below(1_000_000) as i32
    }
}

fn random_cursor(rng: &mut Rng) -> (f32, f32, f32) {
    if rng.chance(3) {
        return (rng.pick(&BAD_FLOATS), 0.5, rng.pick(&BAD_FLOATS));
    }
    (rng.unit() as f32, rng.unit() as f32, rng.unit() as f32)
}

fn random_information(rng: &mut Rng) -> ClientInformation {
    ClientInformation {
        view_distance: if rng.chance(50) {
            rng.next() as i8
        } else {
            rng.below(12) as i8
        },
        chat_visibility: rng.pick(&[
            ChatVisibility::Full,
            ChatVisibility::System,
            ChatVisibility::Hidden,
        ]),
        model_customisation: rng.next() as u8,
        main_hand: rng.pick(&[HumanoidArm::Left, HumanoidArm::Right]),
        particle_status: ParticleStatus::All,
        ..ClientInformation::default()
    }
}

fn random_text(rng: &mut Rng, limit: u64, hostile: bool) -> String {
    let mut text = String::new();
    for _ in 0..rng.below(limit) {
        text.push(rng.pick(&['a', '竹', '🐾', ' ', '/']));
    }
    if hostile && rng.chance(20) {
        text.push(rng.pick(&['§', '\u{7f}', '\n']));
    }
    text
}

/// 这个请求要确认的 sequence。
fn sequence_of(request: &PlayRequest) -> Option<i32> {
    match *request {
        PlayRequest::UseItemOn { sequence, .. } | PlayRequest::UseItem { sequence, .. } => {
            Some(sequence)
        }
        PlayRequest::PlayerAction {
            action:
                PlayerActionKind::StartDestroyBlock
                | PlayerActionKind::ChangeDestroyDirection
                | PlayerActionKind::AbortDestroyBlock
                | PlayerActionKind::StopDestroyBlock,
            sequence,
            ..
        } => Some(sequence),
        _ => None,
    }
}

fn random_request(rng: &mut Rng, actor: &mut Actor) -> PlayRequest {
    let around = actor.position;
    let hostile = actor.hostile;
    match rng.below(26) {
        0..=7 => {
            let position = random_position(rng, around, hostile);
            if !position.x.is_nan() {
                actor.position = position;
            }
            PlayRequest::MovePlayer {
                position: rng.chance(90).then_some(position),
                rotation: rng.chance(50).then(|| random_rotation(rng, hostile)),
                on_ground: rng.chance(50),
            }
        }
        8 => {
            let (yaw, pitch) = random_rotation(rng, hostile);
            PlayRequest::AcceptTeleportation {
                // 确认过的 id 再发一次会被踢。
                id: if hostile {
                    rng.pick(&[0, 1, 1, 2, i32::MAX, -1])
                } else {
                    rng.pick(&[0, 2, i32::MAX, -1])
                },
                x: around.x,
                y: around.y,
                z: around.z,
                yaw,
                pitch,
            }
        }
        9 => PlayRequest::Chat {
            message: random_text(rng, 12, hostile),
        },
        10 => PlayRequest::ChatCommand {
            command: random_text(rng, 300, hostile),
        },
        11 => PlayRequest::ClientInformation(random_information(rng)),
        12 => PlayRequest::ClientCommand {
            action: rng.pick(&[
                ClientCommandKind::PerformRespawn,
                ClientCommandKind::RequestStats,
                ClientCommandKind::RequestGameRuleValues,
            ]),
        },
        13 => PlayRequest::PlayerInput {
            flags: rng.next() as u8,
        },
        14 => PlayRequest::PlayerCommand {
            action: rng.pick(&[
                PlayerCommandKind::StopSleeping,
                PlayerCommandKind::StartSprinting,
                PlayerCommandKind::StopSprinting,
                PlayerCommandKind::StartRidingJump,
                PlayerCommandKind::StopRidingJump,
                PlayerCommandKind::OpenInventory,
                PlayerCommandKind::StartFallFlying,
            ]),
        },
        15 => PlayRequest::PlayerAbilities {
            flags: rng.next() as u8,
        },
        16 => PlayRequest::Punch,
        17 => PlayRequest::SetCarriedItem {
            slot: if rng.chance(80) {
                rng.below(9) as i16
            } else {
                rng.next() as i16
            },
        },
        18 | 19 => PlayRequest::SetCreativeModeSlot {
            slot: match rng.below(10) {
                0..=4 => 36 + rng.below(9) as i16,
                5 => 45,
                6 => rng.below(47) as i16,
                7 => -1,
                _ => rng.next() as i16,
            },
            stack: random_stack(rng),
        },
        20 => PlayRequest::PickItemFromBlock {
            pos: random_block(rng, around),
            include_data: rng.chance(50),
        },
        21 | 22 => PlayRequest::PlayerAction {
            action: rng.pick(&[
                PlayerActionKind::StartDestroyBlock,
                PlayerActionKind::StartDestroyBlock,
                PlayerActionKind::StartDestroyBlock,
                PlayerActionKind::ChangeDestroyDirection,
                PlayerActionKind::AbortDestroyBlock,
                PlayerActionKind::StopDestroyBlock,
                PlayerActionKind::DropAllItems,
                PlayerActionKind::DropItem,
                PlayerActionKind::ReleaseUseItem,
                PlayerActionKind::SwapItemWithOffhand,
                PlayerActionKind::Stab,
            ]),
            pos: random_block(rng, around),
            face: Direction::from_id(rng.below(6) as i32),
            sequence: random_sequence(rng, hostile),
        },
        23 | 24 => PlayRequest::UseItemOn {
            hand: rng.pick(&[Hand::Main, Hand::Off]),
            pos: random_block(rng, around),
            face: Direction::from_id(rng.below(6) as i32),
            cursor: random_cursor(rng),
            inside: rng.chance(50),
            world_border_hit: rng.chance(50),
            sequence: random_sequence(rng, hostile),
        },
        _ => PlayRequest::UseItem {
            hand: rng.pick(&[Hand::Main, Hand::Off]),
            sequence: random_sequence(rng, hostile),
            yaw: random_angle(rng, hostile),
            pitch: random_angle(rng, hostile),
        },
    }
}

/// 一次运行的结果：每个客户端收到的字节的摘要。
fn run(seed: u64, ticks: u32, view_distance: i32) -> Vec<u64> {
    let mut rng = Rng::new(seed);
    let mut server = Harness::new(view_distance);
    let mut actors: Vec<Actor> = Vec::new();
    let mut digests = Vec::new();
    let mut joined = 0;

    for _ in 0..ticks {
        if actors.len() < 6 && rng.chance(15) {
            joined += 1;
            let name = format!("Player{}", joined % 4);
            let client = server.join_with(&name, random_information(&mut rng));
            if rng.chance(90) {
                server.confirm_teleport(&client);
            }
            actors.push(Actor {
                client,
                model: Model::new(name),
                position: Vec3::new(8.5, -60.0, 8.5),
                hostile: rng.chance(20),
                leaving_in: None,
                expected_ack: None,
                kicked_at: None,
            });
        }

        for actor in &mut actors {
            if actor.is_alive() {
                for _ in 0..rng.below(6) {
                    let request = random_request(&mut rng, actor);
                    actor.expected_ack = actor.expected_ack.max(sequence_of(&request));
                    server.request(&actor.client, request);
                }
                if rng.chance(5) {
                    let millis = rng.pick(&[0, 40, i32::MAX, i32::MIN]);
                    let player = actor.client.id;
                    server
                        .inbox
                        .send(Inbound::Latency { player, millis })
                        .unwrap();
                }
                if rng.chance(1) {
                    actor.leaving_in = Some(rng.below(3) as u32);
                }
                if rng.chance(1) {
                    actor.client.refuse_frames();
                }
            }
            if actor.client.is_closed() {
                if actor.leaving_in.is_none() {
                    actor.leaving_in = Some(rng.below(40) as u32);
                }
                // 客户端还不知道自己被踢了，这时发的包世界要当作没收到。
                let request = random_request(&mut rng, actor);
                server.request(&actor.client, request);
            }
        }

        server.tick();
        observe_all(&mut server, &mut actors, &mut rng, 80);
        for actor in &mut actors {
            check_tick(&server, actor);
        }

        actors.retain_mut(|actor| match &mut actor.leaving_in {
            Some(0) => {
                server.leave(&actor.client);
                digests.push(actor.model.digest);
                false
            }
            Some(ticks) => {
                *ticks -= 1;
                true
            }
            None => true,
        });
    }

    settle(&mut server, &mut actors, &mut rng);
    check_convergence(&server, &actors, view_distance);
    shut_down(&mut server, &mut actors);
    digests.extend(actors.iter().map(|actor| actor.model.digest));
    digests
}

/// 关服时所有人都收到断开原因，而且它是最后一个包。
fn shut_down(server: &mut Harness, actors: &mut [Actor]) {
    server.inbox.send(Inbound::Shutdown).unwrap();
    assert!(server.world.tick().is_break());
    for actor in actors {
        let frames = actor.client.take_frames();
        actor
            .model
            .observe(actor.client.id.0, &frames, &server.world);
        assert!(actor.model.disconnected, "{}: no reason", actor.model.name);
        assert!(actor.client.is_closed(), "{}: still open", actor.model.name);
    }
}

/// 一 tick 结束时的检查：预测都确认了，被踢的玩家没有再动过。
fn check_tick(server: &Harness, actor: &mut Actor) {
    let acknowledged = actor.model.acknowledged.take();
    let expected = actor.expected_ack.take();
    if actor.client.is_closed() {
        let position = server.player(&actor.client).position();
        let kicked_at = *actor.kicked_at.get_or_insert(position);
        assert_eq!(
            position, kicked_at,
            "{}: moved after a kick",
            actor.model.name
        );
    } else {
        assert_eq!(acknowledged, expected, "{}: acknowledged", actor.model.name);
    }
}

/// 让每个客户端读完这一 tick 的字节，按 `ack_percent` 的概率确认收到的批次。
fn observe_all(server: &mut Harness, actors: &mut [Actor], rng: &mut Rng, ack_percent: u64) {
    for actor in actors {
        let frames = actor.client.take_frames();
        let id = actor.client.id.0;
        actor.model.observe(id, &frames, &server.world);
        while actor.model.finished_batches > 0 && rng.chance(ack_percent) {
            actor.model.finished_batches -= 1;
            if actor.is_alive() {
                let desired = if rng.chance(3) {
                    rng.pick(&BAD_FLOATS)
                } else {
                    (rng.unit() * 70.0) as f32
                };
                server.acknowledge_batch(&actor.client, desired);
            }
        }
    }
}

/// 不再乱动，把排队的区块和该走的人都处理完。
fn settle(server: &mut Harness, actors: &mut Vec<Actor>, rng: &mut Rng) {
    actors.retain(|actor| {
        let alive = actor.is_alive();
        if !alive {
            server.leave(&actor.client);
        }
        alive
    });
    for actor in actors.iter() {
        // 第一批的确认没到之前，后面的批次一直停着。
        server.acknowledge_batch(&actor.client, 64.0);
    }
    for _ in 0..SETTLE_TICKS {
        server.tick();
        observe_all(server, actors, rng, 100);
        for actor in actors.iter() {
            server.acknowledge_batch(&actor.client, 64.0);
        }
    }
}

/// 所有人的客户端都和世界一致：该有的区块、实体都在，方块也一样。
fn check_convergence(server: &Harness, actors: &[Actor], view_distance: i32) {
    let world = &server.world;
    let region = &world.region;
    let tab: BTreeMap<Uuid, bool> = actors
        .iter()
        .map(|actor| (actor.client.uuid, server.player(&actor.client).shows_hat()))
        .collect();
    for actor in actors {
        let name = &actor.model.name;
        let player = server.player(&actor.client);
        assert_eq!(actor.model.tab, tab, "{name}: tab list");
        let own = Remote {
            health: 1.0,
            equipment: [(items::AIR, 0); 6],
            ..Remote::of(player)
        };
        assert_eq!(actor.model.own, own, "{name}: own metadata");

        let view = ChunkView {
            center: player.chunk_pos(),
            view_distance: player.view_distance(view_distance),
        };
        assert_eq!(actor.model.center, Some(view.center), "{name}: center");
        let radius = view.view_distance + 1;
        let tracked: Vec<ChunkPos> = (view.center.x - radius..=view.center.x + radius)
            .flat_map(|x| {
                (view.center.z - radius..=view.center.z + radius).map(move |z| ChunkPos::new(x, z))
            })
            .filter(|&pos| view.contains(pos))
            .collect();
        let held: Vec<ChunkPos> = actor.model.chunks.keys().copied().collect();
        assert_eq!(held, tracked, "{name}: chunks");

        for pos in held {
            let Some(chunk) = region.chunks.edited(pos) else {
                continue;
            };
            let seen = actor.model.chunk(pos, world);
            for y in MIN_Y..WORLD_TOP {
                for z in 0..16 {
                    for x in 0..16 {
                        let (server_state, client_state) =
                            (chunk.block(x, y, z), seen.block(x, y, z));
                        assert_eq!(
                            client_state, server_state,
                            "{name}: ghost block in {pos:?} at ({x}, {y}, {z})"
                        );
                    }
                }
            }
        }

        let ids: BTreeSet<i32> = actors.iter().map(|actor| actor.client.id.0).collect();
        assert!(
            actor.model.entities.keys().all(|id| ids.contains(id)),
            "{name}: entity of a player that left"
        );

        for other in actors {
            let target = server.player(&other.client);
            let expected = should_see(player, target, view_distance);
            let seen = actor.model.entities.get(&other.client.id.0);
            assert_eq!(seen.is_some(), expected, "{name}: {}", other.model.name);
            if let Some(seen) = seen {
                assert_eq!(*seen, Remote::of(target), "{name}: {}", other.model.name);
            }
        }
    }
}

/// `viewer` 的客户端应该有 `target` 这个实体。`entities.md` 第 4 节。
fn should_see(viewer: &Player, target: &Player, view_distance: i32) -> bool {
    let range = f64::from((viewer.view_distance(view_distance) * 16).min(512));
    let dx = viewer.position().x - target.position().x;
    let dz = viewer.position().z - target.position().z;
    viewer.id != target.id
        && dx * dx + dz * dz <= range * range
        && viewer.chunks.is_chunk_tracked(target.chunk_pos())
}

#[test]
fn random_requests_never_panic_or_confuse_a_client() {
    for seed in 1..=4 {
        run(seed, 150, 2 + (seed % 3) as i32);
    }
}

#[test]
fn same_seed_gives_the_same_bytes() {
    assert_eq!(run(7, 100, 2), run(7, 100, 2));
}

/// 手动跑更多种子：`FUZZ_SEEDS=2000 cargo test -p mc-bamboo-game long_random_run -- --ignored`。
#[test]
#[ignore = "几分钟，改了世界线程的逻辑之后手动跑"]
fn long_random_run() {
    let seeds = std::env::var("FUZZ_SEEDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(100u64);
    for seed in 1000..1000 + seeds {
        run(seed, 400, 2 + (seed % 7) as i32);
    }
}
