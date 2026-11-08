//! 测试自己的线上格式。
//!
//! 包的格式照规格手写，不调用 `mc-bamboo-game`，这样断言才是独立的。
//! 解码器读完之后一定检查没有剩余字节：多一个字节，真实客户端就会断线。

use std::mem;

use mc_bamboo_data::packets::play::{clientbound, serverbound};
use mc_bamboo_protocol::{
    codec::{Reader, write_string, write_varint},
    components::{ComponentPatch, cursor::Cursor},
};
use uuid::Uuid;

use super::frame;

pub const MIN_Y: i32 = -64;
pub const MAX_Y: i32 = 319;
const SECTIONS: usize = 24;
const BLOCKS_PER_SECTION: usize = 4096;
const BIOMES_PER_SECTION: usize = 64;
/// 大于这个位数时调色板是全局的，线上不写内容。
const MAX_INDIRECT_BLOCK_BITS: u8 = 8;
const MAX_INDIRECT_BIOME_BITS: u8 = 3;

pub const MAIN_HAND: i32 = 0;
pub const OFF_HAND: i32 = 1;

/// `Direction` 的线上 ID。
pub const DOWN: u8 = 0;
pub const UP: u8 = 1;
pub const NORTH: u8 = 2;
pub const SOUTH: u8 = 3;
pub const WEST: u8 = 4;
pub const EAST: u8 = 5;

/// `player_action` 的动作。
pub const START_DESTROY_BLOCK: i32 = 0;
pub const DROP_ALL_ITEMS: i32 = 4;
pub const DROP_ITEM: i32 = 5;
pub const RELEASE_USE_ITEM: i32 = 6;
pub const SWAP_ITEM_WITH_OFFHAND: i32 = 7;

/// `player_command` 的动作。
pub const START_SPRINTING: i32 = 1;
pub const STOP_SPRINTING: i32 = 2;
pub const START_FALL_FLYING: i32 = 6;

/// `player_input` 里的潜行键。
pub const INPUT_SHIFT: u8 = 0x20;
/// `player_abilities` 里的飞行位。
pub const ABILITY_FLYING: u8 = 0x02;

/// 元数据 index 0 的位。
pub const FLAG_SHIFT: u8 = 0x02;
pub const FLAG_SPRINTING: u8 = 0x08;
pub const POSE_STANDING: i32 = 0;
pub const POSE_CROUCHING: i32 = 5;
pub const ARM_LEFT: i32 = 0;
pub const ARM_RIGHT: i32 = 1;

/// `EquipmentSlot` 的 ordinal，不是 `EquipmentSlot.id`。
pub const SLOT_MAINHAND: u8 = 0;
pub const SLOT_OFFHAND: u8 = 1;
pub const SLOT_FEET: u8 = 2;
pub const SLOT_LEGS: u8 = 3;
pub const SLOT_CHEST: u8 = 4;
pub const SLOT_HEAD: u8 = 5;

/// `level_event` 的破坏方块事件。
pub const LEVEL_EVENT_BREAK: i32 = 2001;
pub const SWING_WHACK: i32 = 1;
pub const SWING_DURATION: i32 = 6;

/// 方块坐标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Pos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// `x(26) | z(26) | y(12)`。
    pub fn pack(self) -> i64 {
        ((i64::from(self.x) & 0x3ff_ffff) << 38)
            | ((i64::from(self.z) & 0x3ff_ffff) << 12)
            | (i64::from(self.y) & 0xfff)
    }

    pub fn unpack(value: i64) -> Self {
        Self {
            x: (value >> 38) as i32,
            y: (value << 52 >> 52) as i32,
            z: (value << 26 >> 38) as i32,
        }
    }

    pub const fn offset(self, x: i32, y: i32, z: i32) -> Self {
        Self::new(self.x + x, self.y + y, self.z + z)
    }

    pub const fn above(self) -> Self {
        self.offset(0, 1, 0)
    }

    /// 这一格往 `face` 方向的邻居。
    pub fn neighbor(self, face: u8) -> Self {
        match face {
            DOWN => self.offset(0, -1, 0),
            UP => self.offset(0, 1, 0),
            NORTH => self.offset(0, 0, -1),
            SOUTH => self.offset(0, 0, 1),
            WEST => self.offset(-1, 0, 0),
            EAST => self.offset(1, 0, 0),
            other => panic!("not a direction: {other}"),
        }
    }

    pub const fn chunk(self) -> (i32, i32) {
        (self.x >> 4, self.z >> 4)
    }
}

/// `T(c, vd)`：`ChunkTrackingView.contains` 的集合，也就是服务端要发的全部区块。
pub fn tracked_chunks(center: (i32, i32), view_distance: i32) -> Vec<(i32, i32)> {
    let reach = view_distance + 2;
    let mut chunks = Vec::new();
    for x in center.0 - reach..=center.0 + reach {
        for z in center.1 - reach..=center.1 + reach {
            let dx = i64::from(((x - center.0).abs() - 2).max(0));
            let dz = i64::from(((z - center.1).abs() - 2).max(0));
            if dx * dx + dz * dz < i64::from(view_distance) * i64::from(view_distance) {
                chunks.push((x, z));
            }
        }
    }
    chunks
}

/// `Mth.packDegrees`。
pub fn pack_degrees(angle: f32) -> u8 {
    (angle * 256.0 / 360.0).floor() as i32 as u8
}

/// `Math.round`：`.5` 向正无穷取整。
fn java_round(value: f64) -> i64 {
    let floor = value.floor();
    (if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }) as i64
}

/// `VecDeltaCodec.encode`。
pub fn fixed(value: f64) -> i64 {
    java_round(value * 4096.0)
}

// ---- 发给服务端的包 ----

fn packet(id: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    write_varint(&mut payload, id);
    payload
}

fn push_position(payload: &mut Vec<u8>, x: f64, y: f64, z: f64) {
    for coordinate in [x, y, z] {
        payload.extend_from_slice(&coordinate.to_be_bytes());
    }
}

pub fn accept_teleportation_with_id(id: i32, x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut payload = packet(serverbound::ACCEPT_TELEPORTATION);
    write_varint(&mut payload, id);
    push_position(&mut payload, x, y, z);
    payload.extend_from_slice(&[0; 8]);
    frame(&payload)
}

pub fn move_position(x: f64, y: f64, z: f64, on_ground: bool) -> Vec<u8> {
    let mut payload = packet(serverbound::MOVE_PLAYER_POS);
    push_position(&mut payload, x, y, z);
    payload.push(u8::from(on_ground));
    frame(&payload)
}

pub fn move_position_rotation(
    (x, y, z): (f64, f64, f64),
    yaw: f32,
    pitch: f32,
    on_ground: bool,
) -> Vec<u8> {
    let mut payload = packet(serverbound::MOVE_PLAYER_POS_ROT);
    push_position(&mut payload, x, y, z);
    payload.extend_from_slice(&yaw.to_be_bytes());
    payload.extend_from_slice(&pitch.to_be_bytes());
    payload.push(u8::from(on_ground));
    frame(&payload)
}

pub fn move_rotation(yaw: f32, pitch: f32, on_ground: bool) -> Vec<u8> {
    let mut payload = packet(serverbound::MOVE_PLAYER_ROT);
    payload.extend_from_slice(&yaw.to_be_bytes());
    payload.extend_from_slice(&pitch.to_be_bytes());
    payload.push(u8::from(on_ground));
    frame(&payload)
}

pub fn player_input(flags: u8) -> Vec<u8> {
    let mut payload = packet(serverbound::PLAYER_INPUT);
    payload.push(flags);
    frame(&payload)
}

pub fn player_command(entity_id: i32, action: i32) -> Vec<u8> {
    let mut payload = packet(serverbound::PLAYER_COMMAND);
    write_varint(&mut payload, entity_id);
    write_varint(&mut payload, action);
    write_varint(&mut payload, 0);
    frame(&payload)
}

pub fn player_abilities(flags: u8) -> Vec<u8> {
    let mut payload = packet(serverbound::PLAYER_ABILITIES);
    payload.push(flags);
    frame(&payload)
}

pub fn punch() -> Vec<u8> {
    frame(&packet(serverbound::PUNCH))
}

pub fn set_carried_item(slot: i16) -> Vec<u8> {
    let mut payload = packet(serverbound::SET_CARRIED_ITEM);
    payload.extend_from_slice(&slot.to_be_bytes());
    frame(&payload)
}

/// 不带组件的物品。`count` 为 0 时是空物品。
pub fn set_creative_mode_slot(slot: i16, item: i32, count: i32) -> Vec<u8> {
    let mut stack = Vec::new();
    write_varint(&mut stack, count);
    if count > 0 {
        write_varint(&mut stack, item);
        stack.extend_from_slice(&[0, 0]);
    }
    set_creative_mode_slot_raw(slot, &stack)
}

/// `stack` 是 delimited 格式的物品。
pub fn set_creative_mode_slot_raw(slot: i16, stack: &[u8]) -> Vec<u8> {
    let mut payload = packet(serverbound::SET_CREATIVE_MODE_SLOT);
    payload.extend_from_slice(&slot.to_be_bytes());
    payload.extend_from_slice(stack);
    frame(&payload)
}

pub fn player_action(action: i32, pos: Pos, face: u8, sequence: i32) -> Vec<u8> {
    let mut payload = packet(serverbound::PLAYER_ACTION);
    write_varint(&mut payload, action);
    payload.extend_from_slice(&pos.pack().to_be_bytes());
    payload.push(face);
    write_varint(&mut payload, sequence);
    frame(&payload)
}

/// `use_item_on`，`cursor` 是点击点减去 `pos`。
pub fn use_item_on(
    hand: i32,
    pos: Pos,
    face: u8,
    cursor: (f32, f32, f32),
    sequence: i32,
) -> Vec<u8> {
    let mut payload = packet(serverbound::USE_ITEM_ON);
    write_varint(&mut payload, hand);
    payload.extend_from_slice(&pos.pack().to_be_bytes());
    write_varint(&mut payload, i32::from(face));
    for value in [cursor.0, cursor.1, cursor.2] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload.extend_from_slice(&[0, 0]);
    write_varint(&mut payload, sequence);
    frame(&payload)
}

/// 这个面中心的 `cursor`。
pub fn face_center(face: u8) -> (f32, f32, f32) {
    match face {
        DOWN => (0.5, 0.0, 0.5),
        UP => (0.5, 1.0, 0.5),
        NORTH => (0.5, 0.5, 0.0),
        SOUTH => (0.5, 0.5, 1.0),
        WEST => (0.0, 0.5, 0.5),
        EAST => (1.0, 0.5, 0.5),
        other => panic!("not a direction: {other}"),
    }
}

pub fn use_item(hand: i32, sequence: i32, yaw: f32, pitch: f32) -> Vec<u8> {
    let mut payload = packet(serverbound::USE_ITEM);
    write_varint(&mut payload, hand);
    write_varint(&mut payload, sequence);
    payload.extend_from_slice(&yaw.to_be_bytes());
    payload.extend_from_slice(&pitch.to_be_bytes());
    frame(&payload)
}

pub fn pick_item_from_block(pos: Pos) -> Vec<u8> {
    let mut payload = packet(serverbound::PICK_ITEM_FROM_BLOCK);
    payload.extend_from_slice(&pos.pack().to_be_bytes());
    payload.push(0);
    frame(&payload)
}

/// 离线客户端发的 `chat`：没有签名，`lastSeen` 是空的。
pub fn chat(message: &str) -> Vec<u8> {
    let mut payload = packet(serverbound::CHAT);
    write_string(&mut payload, message);
    payload.extend_from_slice(&0x0000_018b_cfe5_6800u64.to_be_bytes());
    payload.extend_from_slice(&0x1122_3344_5566_7788u64.to_be_bytes());
    payload.push(0);
    payload.extend_from_slice(&[0, 0, 0, 0, 1]);
    frame(&payload)
}

pub fn chat_command(command: &str) -> Vec<u8> {
    let mut payload = packet(serverbound::CHAT_COMMAND);
    write_string(&mut payload, command);
    frame(&payload)
}

pub fn client_command(action: i32) -> Vec<u8> {
    let mut payload = packet(serverbound::CLIENT_COMMAND);
    write_varint(&mut payload, action);
    frame(&payload)
}

/// Play 状态的 `client_information`。
pub fn play_client_information(view_distance: i8, model_parts: u8, main_hand: i32) -> Vec<u8> {
    let mut payload = packet(serverbound::CLIENT_INFORMATION);
    write_client_information(&mut payload, view_distance, model_parts, main_hand);
    frame(&payload)
}

/// `client_information` 的内容，两个状态相同。
pub fn write_client_information(
    payload: &mut Vec<u8>,
    view_distance: i8,
    model_parts: u8,
    main_hand: i32,
) {
    write_string(payload, "en_us");
    payload.push(view_distance as u8);
    write_varint(payload, 0);
    payload.push(1);
    payload.push(model_parts);
    write_varint(payload, main_hand);
    payload.extend_from_slice(&[0, 1]);
    write_varint(payload, 0);
}

// ---- 服务端发来的包 ----

/// 收到的一帧：包 ID 和后面的内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub id: i32,
    pub body: Vec<u8>,
}

impl Frame {
    pub fn parse(payload: &[u8]) -> Self {
        let mut reader = Reader::new(payload);
        let id = reader.varint().expect("frame without a packet id");
        Self {
            id,
            body: reader.rest().to_vec(),
        }
    }

    pub fn is<P: Packet>(&self) -> bool {
        self.id == P::ID
    }

    pub fn decode<P: Packet>(&self) -> P {
        decode(&self.body)
    }
}

/// 一种能解码的包。解码失败是服务端的错，直接让测试失败。
pub trait Packet: Sized {
    const ID: i32;
    fn read(reader: &mut Reader<'_>) -> Self;
}

/// 解码包 ID 之后的内容。
pub fn decode<P: Packet>(body: &[u8]) -> P {
    let mut reader = Reader::new(body);
    let packet = P::read(&mut reader);
    if let Err(error) = reader.finish() {
        panic!("{}: {error}", packet_name(P::ID));
    }
    packet
}

/// 取走还没读的全部字节。
fn take_rest<'a>(reader: &mut Reader<'a>) -> &'a [u8] {
    mem::replace(reader, Reader::new(&[])).rest()
}

fn varint(reader: &mut Reader<'_>) -> i32 {
    reader.varint().expect("VarInt")
}

fn byte(reader: &mut Reader<'_>) -> u8 {
    reader.u8().expect("byte")
}

fn boolean(reader: &mut Reader<'_>) -> bool {
    let value = byte(reader);
    assert!(value <= 1, "boolean byte {value}");
    value == 1
}

fn double(reader: &mut Reader<'_>) -> f64 {
    reader.f64().expect("double")
}

fn float(reader: &mut Reader<'_>) -> f32 {
    reader.f32().expect("float")
}

fn short(reader: &mut Reader<'_>) -> i16 {
    reader.i16().expect("short")
}

fn int(reader: &mut Reader<'_>) -> i32 {
    reader.i32().expect("int")
}

fn uuid(reader: &mut Reader<'_>) -> Uuid {
    reader.uuid().expect("UUID")
}

fn position(reader: &mut Reader<'_>) -> Pos {
    Pos::unpack(reader.i64().expect("block position"))
}

fn list_len(reader: &mut Reader<'_>) -> usize {
    usize::try_from(varint(reader)).expect("negative list length")
}

pub struct Login {
    pub entity_id: i32,
}

impl Packet for Login {
    const ID: i32 = clientbound::LOGIN;

    fn read(reader: &mut Reader<'_>) -> Self {
        let entity_id = int(reader);
        take_rest(reader);
        Self { entity_id }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerPosition {
    pub id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Packet for PlayerPosition {
    const ID: i32 = clientbound::PLAYER_POSITION;

    fn read(reader: &mut Reader<'_>) -> Self {
        let id = varint(reader);
        let (x, y, z) = (double(reader), double(reader), double(reader));
        for _ in 0..3 {
            double(reader);
        }
        float(reader);
        float(reader);
        assert_eq!(int(reader), 0, "M2 only teleports to absolute positions");
        Self { id, x, y, z }
    }
}

/// Tab 列表里的一档案属性，比如签名过的皮肤。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabProperty {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

/// Tab 列表的一条记录。只有动作掩码里有的字段才是 `Some`。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TabEntry {
    pub uuid: Uuid,
    pub name: Option<String>,
    /// 跟着名字一起来的档案属性，离线模式是空列表。
    pub properties: Vec<TabProperty>,
    pub game_mode: Option<i32>,
    pub listed: Option<bool>,
    pub latency: Option<i32>,
    pub list_order: Option<i32>,
    pub show_hat: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerInfoUpdate {
    pub actions: u8,
    pub entries: Vec<TabEntry>,
}

impl PlayerInfoUpdate {
    /// 进服时官方把 8 个动作都写上。
    pub const ALL_ACTIONS: u8 = 0xff;

    pub fn entry(&self, uuid: Uuid) -> Option<&TabEntry> {
        self.entries.iter().find(|entry| entry.uuid == uuid)
    }

    pub fn adds(&self, uuid: Uuid) -> bool {
        self.actions & 0x01 != 0 && self.entry(uuid).is_some()
    }
}

impl Packet for PlayerInfoUpdate {
    const ID: i32 = clientbound::PLAYER_INFO_UPDATE;

    fn read(reader: &mut Reader<'_>) -> Self {
        let actions = byte(reader);
        let count = list_len(reader);
        let entries = (0..count)
            .map(|_| read_tab_entry(reader, actions))
            .collect();
        Self { actions, entries }
    }
}

fn read_tab_entry(reader: &mut Reader<'_>, actions: u8) -> TabEntry {
    let mut entry = TabEntry {
        uuid: uuid(reader),
        ..TabEntry::default()
    };
    if actions & 0x01 != 0 {
        entry.name = Some(reader.string(16).expect("name").to_owned());
        let count = list_len(reader);
        entry.properties = (0..count).map(|_| read_tab_property(reader)).collect();
    }
    if actions & 0x02 != 0 {
        assert!(!boolean(reader), "offline players have no chat session");
    }
    if actions & 0x04 != 0 {
        entry.game_mode = Some(varint(reader));
    }
    if actions & 0x08 != 0 {
        entry.listed = Some(boolean(reader));
    }
    if actions & 0x10 != 0 {
        entry.latency = Some(varint(reader));
    }
    if actions & 0x20 != 0 {
        assert!(!boolean(reader), "players have no display name");
    }
    if actions & 0x40 != 0 {
        entry.list_order = Some(varint(reader));
    }
    if actions & 0x80 != 0 {
        entry.show_hat = Some(boolean(reader));
    }
    entry
}

/// 档案上的一项属性，签名是可选的。
fn read_tab_property(reader: &mut Reader<'_>) -> TabProperty {
    let name = reader.string(64).expect("property name").to_owned();
    let value = reader.string(32767).expect("property value").to_owned();
    let signature =
        boolean(reader).then(|| reader.string(1024).expect("property signature").to_owned());
    TabProperty {
        name,
        value,
        signature,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerInfoRemove {
    pub uuids: Vec<Uuid>,
}

impl Packet for PlayerInfoRemove {
    const ID: i32 = clientbound::PLAYER_INFO_REMOVE;

    fn read(reader: &mut Reader<'_>) -> Self {
        let count = list_len(reader);
        Self {
            uuids: (0..count).map(|_| uuid(reader)).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AddEntity {
    pub id: i32,
    pub uuid: Uuid,
    pub kind: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub pitch: u8,
    pub yaw: u8,
    pub head_yaw: u8,
    pub data: i32,
}

impl Packet for AddEntity {
    const ID: i32 = clientbound::ADD_ENTITY;

    fn read(reader: &mut Reader<'_>) -> Self {
        let id = varint(reader);
        let uuid = uuid(reader);
        let kind = varint(reader);
        let (x, y, z) = (double(reader), double(reader), double(reader));
        assert_eq!(byte(reader), 0, "players have no movement in M2");
        Self {
            id,
            uuid,
            kind,
            x,
            y,
            z,
            pitch: byte(reader),
            yaw: byte(reader),
            head_yaw: byte(reader),
            data: varint(reader),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveEntities {
    pub ids: Vec<i32>,
}

impl Packet for RemoveEntities {
    const ID: i32 = clientbound::REMOVE_ENTITIES;

    fn read(reader: &mut Reader<'_>) -> Self {
        let count = list_len(reader);
        Self {
            ids: (0..count).map(|_| varint(reader)).collect(),
        }
    }
}

/// `VecDelta` 的一步。`ticks` 是 `None` 时是 Linear。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeltaStep {
    pub ticks: Option<i32>,
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

fn read_delta(reader: &mut Reader<'_>, step_count: i32) -> Vec<DeltaStep> {
    if step_count <= 0 {
        return vec![DeltaStep {
            ticks: None,
            x: short(reader),
            y: short(reader),
            z: short(reader),
        }];
    }
    (0..step_count)
        .map(|_| DeltaStep {
            ticks: Some(varint(reader)),
            x: short(reader),
            y: short(reader),
            z: short(reader),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveEntityPos {
    pub entity: i32,
    pub on_ground: bool,
    pub steps: Vec<DeltaStep>,
}

impl Packet for MoveEntityPos {
    const ID: i32 = clientbound::MOVE_ENTITY_POS;

    fn read(reader: &mut Reader<'_>) -> Self {
        let entity = varint(reader);
        let properties = varint(reader);
        Self {
            entity,
            on_ground: properties & 1 != 0,
            steps: read_delta(reader, properties >> 1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveEntityPosRot {
    pub entity: i32,
    pub on_ground: bool,
    pub steps: Vec<DeltaStep>,
    pub yaw: u8,
    pub pitch: u8,
}

impl Packet for MoveEntityPosRot {
    const ID: i32 = clientbound::MOVE_ENTITY_POS_ROT;

    fn read(reader: &mut Reader<'_>) -> Self {
        let entity = varint(reader);
        let properties = varint(reader);
        Self {
            entity,
            on_ground: properties & 1 != 0,
            steps: read_delta(reader, properties >> 1),
            yaw: byte(reader),
            pitch: byte(reader),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveEntityRot {
    pub entity: i32,
    pub on_ground: bool,
    pub yaw: u8,
    pub pitch: u8,
}

impl Packet for MoveEntityRot {
    const ID: i32 = clientbound::MOVE_ENTITY_ROT;

    fn read(reader: &mut Reader<'_>) -> Self {
        Self {
            entity: varint(reader),
            on_ground: boolean(reader),
            yaw: byte(reader),
            pitch: byte(reader),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PositionPath {
    Linear((f64, f64, f64)),
    /// 每步是坐标和 tick 数。
    Stepped(Vec<((f64, f64, f64), i32)>),
}

impl PositionPath {
    pub fn end(&self) -> (f64, f64, f64) {
        match self {
            Self::Linear(position) => *position,
            Self::Stepped(steps) => steps.last().expect("a stepped path is never empty").0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntityPositionSync {
    pub entity: i32,
    pub path: PositionPath,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

impl Packet for EntityPositionSync {
    const ID: i32 = clientbound::ENTITY_POSITION_SYNC;

    fn read(reader: &mut Reader<'_>) -> Self {
        let entity = varint(reader);
        let path = match varint(reader) {
            0 => PositionPath::Linear((double(reader), double(reader), double(reader))),
            1 => {
                let count = list_len(reader);
                assert!(count > 0, "empty stepped path");
                let steps = (0..count)
                    .map(|_| {
                        let position = (double(reader), double(reader), double(reader));
                        (position, varint(reader))
                    })
                    .collect();
                PositionPath::Stepped(steps)
            }
            other => panic!("unknown path type {other}"),
        };
        Self {
            entity,
            path,
            yaw: float(reader),
            pitch: float(reader),
            on_ground: boolean(reader),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotateHead {
    pub entity: i32,
    pub head_yaw: u8,
}

impl Packet for RotateHead {
    const ID: i32 = clientbound::ROTATE_HEAD;

    fn read(reader: &mut Reader<'_>) -> Self {
        Self {
            entity: varint(reader),
            head_yaw: byte(reader),
        }
    }
}

/// 元数据的值。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EntityValue {
    Byte(u8),
    Int(i32),
    Float(f32),
    Pose(i32),
    Arm(i32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetEntityData {
    pub entity: i32,
    pub items: Vec<(u8, EntityValue)>,
}

impl SetEntityData {
    pub fn value(&self, index: u8) -> Option<EntityValue> {
        self.items
            .iter()
            .find(|(item, _)| *item == index)
            .map(|(_, value)| *value)
    }
}

impl Packet for SetEntityData {
    const ID: i32 = clientbound::SET_ENTITY_DATA;

    fn read(reader: &mut Reader<'_>) -> Self {
        let entity = varint(reader);
        let mut items = Vec::new();
        loop {
            let index = byte(reader);
            if index == 0xff {
                return Self { entity, items };
            }
            let serializer = varint(reader);
            items.push((index, read_entity_value(reader, index, serializer)));
        }
    }
}

/// 序列化器和 index 对不上时，官方的 `SynchedEntityData.assignValue` 会抛异常。
fn read_entity_value(reader: &mut Reader<'_>, index: u8, serializer: i32) -> EntityValue {
    let expected = match index {
        0 | 8 | 16 => 0,
        1 | 7 | 12 | 13 | 18 => 1,
        9 | 17 => 3,
        6 => 20,
        15 => 42,
        other => panic!("metadata index {other} is not used in M2"),
    };
    assert_eq!(
        serializer, expected,
        "wrong serializer for metadata index {index}"
    );
    match serializer {
        0 => EntityValue::Byte(byte(reader)),
        1 => EntityValue::Int(varint(reader)),
        3 => EntityValue::Float(float(reader)),
        20 => EntityValue::Pose(varint(reader)),
        42 => EntityValue::Arm(varint(reader)),
        other => unreachable!("serializer {other}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwingAnimation {
    pub entity: i32,
    pub hand: i32,
    pub kind: i32,
    pub duration: i32,
}

impl Packet for SwingAnimation {
    const ID: i32 = clientbound::SWING_ANIMATION;

    fn read(reader: &mut Reader<'_>) -> Self {
        Self {
            entity: varint(reader),
            hand: varint(reader),
            kind: varint(reader),
            duration: varint(reader),
        }
    }
}

/// 线上的一个物品，不含组件。组件另外看，见 [`read_stack`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stack {
    pub item: i32,
    pub count: i32,
}

/// 读一个 clientbound 的物品，空物品是 `None`。第二个返回值是它带的组件。
///
/// 组件的值没有长度前缀，只能按每种组件自己的格式解析，所以这里借
/// `ComponentPatch::read`：那一层的字节由 `components` 对照和官方代码逐项比过。
fn read_stack(reader: &mut Reader<'_>) -> (Option<Stack>, ComponentPatch) {
    let count = varint(reader);
    if count <= 0 {
        return (None, ComponentPatch::new());
    }
    let item = varint(reader);
    let rest = reader.remaining();
    let mut cursor = Cursor::new(rest);
    let patch = ComponentPatch::read(&mut cursor).expect("component patch");
    let used = rest.len() - cursor.remaining().len();
    reader.bytes(used).expect("component patch bytes");
    (Some(Stack { item, count }), patch)
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetEquipment {
    pub entity: i32,
    /// 槽位的 ordinal 和物品，线上的顺序。
    pub slots: Vec<(u8, Option<Stack>)>,
    /// 每一项物品带的组件，和 [`Self::slots`] 一一对应。
    pub components: Vec<ComponentPatch>,
}

impl SetEquipment {
    pub fn slot(&self, ordinal: u8) -> Option<Option<Stack>> {
        self.slots
            .iter()
            .find(|(slot, _)| *slot == ordinal)
            .map(|(_, stack)| *stack)
    }

    /// 这个槽位里物品带的组件。
    pub fn patch(&self, ordinal: u8) -> Option<&ComponentPatch> {
        let index = self.slots.iter().position(|(slot, _)| *slot == ordinal)?;
        self.components.get(index)
    }
}

impl Packet for SetEquipment {
    const ID: i32 = clientbound::SET_EQUIPMENT;

    fn read(reader: &mut Reader<'_>) -> Self {
        let entity = varint(reader);
        let mut slots = Vec::new();
        let mut components = Vec::new();
        loop {
            let slot = byte(reader);
            let (stack, patch) = read_stack(reader);
            slots.push((slot & 0x7f, stack));
            components.push(patch);
            if slot & 0x80 == 0 {
                return Self {
                    entity,
                    slots,
                    components,
                };
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockUpdate {
    pub pos: Pos,
    pub state: u32,
}

impl Packet for BlockUpdate {
    const ID: i32 = clientbound::BLOCK_UPDATE;

    fn read(reader: &mut Reader<'_>) -> Self {
        Self {
            pos: position(reader),
            state: varint(reader) as u32,
        }
    }
}

/// 一段里同一 tick 改了几格。坐标已经换成世界坐标，按包里的顺序。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionBlocksUpdate {
    pub blocks: Vec<(Pos, u32)>,
}

impl Packet for SectionBlocksUpdate {
    const ID: i32 = clientbound::SECTION_BLOCKS_UPDATE;

    fn read(reader: &mut Reader<'_>) -> Self {
        // 段坐标是 `x(22) | z(22) | y(20)`。
        let section = reader.i64().expect("section position");
        let origin = Pos::new(
            (section >> 42) as i32 * 16,
            (section << 44 >> 44) as i32 * 16,
            (section << 22 >> 42) as i32 * 16,
        );
        let blocks = (0..list_len(reader))
            .map(|_| {
                // 状态 ID 在高位，低 12 位是段内的 `x << 8 | z << 4 | y`。
                let entry = reader.varlong().expect("VarLong");
                let local = (entry & 0xfff) as i32;
                let pos = origin.offset(local >> 8, local & 15, (local >> 4) & 15);
                (pos, (entry >> 12) as u32)
            })
            .collect();
        Self { blocks }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockChangedAck {
    pub sequence: i32,
}

impl Packet for BlockChangedAck {
    const ID: i32 = clientbound::BLOCK_CHANGED_ACK;

    fn read(reader: &mut Reader<'_>) -> Self {
        Self {
            sequence: varint(reader),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelEvent {
    pub kind: i32,
    pub pos: Pos,
    pub data: i32,
    pub global: bool,
}

impl Packet for LevelEvent {
    const ID: i32 = clientbound::LEVEL_EVENT;

    fn read(reader: &mut Reader<'_>) -> Self {
        Self {
            kind: int(reader),
            pos: position(reader),
            data: int(reader),
            global: boolean(reader),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetHeldSlot {
    pub slot: i32,
}

impl Packet for SetHeldSlot {
    const ID: i32 = clientbound::SET_HELD_SLOT;

    fn read(reader: &mut Reader<'_>) -> Self {
        Self {
            slot: varint(reader),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerSetSlot {
    pub container: i32,
    pub state_id: i32,
    pub slot: i16,
    /// trusted 格式的物品，可能带组件。
    pub stack: Vec<u8>,
}

impl Packet for ContainerSetSlot {
    const ID: i32 = clientbound::CONTAINER_SET_SLOT;

    fn read(reader: &mut Reader<'_>) -> Self {
        Self {
            container: varint(reader),
            state_id: varint(reader),
            slot: short(reader),
            stack: take_rest(reader).to_vec(),
        }
    }
}

/// `item`、`count` 这个物品没有组件时的 trusted 字节。`count` 为 0 时是空物品。
pub fn plain_stack_bytes(item: i32, count: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_varint(&mut bytes, count);
    if count > 0 {
        write_varint(&mut bytes, item);
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes
}

#[derive(Debug, Clone, PartialEq)]
pub struct SystemChat {
    pub content: Nbt,
    pub overlay: bool,
}

impl Packet for SystemChat {
    const ID: i32 = clientbound::SYSTEM_CHAT;

    fn read(reader: &mut Reader<'_>) -> Self {
        Self {
            content: Nbt::read(reader),
            overlay: boolean(reader),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisguisedChat {
    pub message: Nbt,
    /// 线上的值，也就是注册表 ID 加一。
    pub chat_type: i32,
    pub name: Nbt,
    pub target: Option<Nbt>,
}

impl Packet for DisguisedChat {
    const ID: i32 = clientbound::DISGUISED_CHAT;

    fn read(reader: &mut Reader<'_>) -> Self {
        Self {
            message: Nbt::read(reader),
            chat_type: varint(reader),
            name: Nbt::read(reader),
            target: boolean(reader).then(|| Nbt::read(reader)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Disconnect {
    pub reason: Nbt,
}

impl Packet for Disconnect {
    const ID: i32 = clientbound::DISCONNECT;

    fn read(reader: &mut Reader<'_>) -> Self {
        Self {
            reason: Nbt::read(reader),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AwardStats {
    pub count: usize,
}

impl Packet for AwardStats {
    const ID: i32 = clientbound::AWARD_STATS;

    fn read(reader: &mut Reader<'_>) -> Self {
        Self {
            count: list_len(reader),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleDelimiter;

impl Packet for BundleDelimiter {
    const ID: i32 = clientbound::BUNDLE_DELIMITER;

    fn read(_reader: &mut Reader<'_>) -> Self {
        Self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetChunkCacheCenter {
    pub x: i32,
    pub z: i32,
}

impl Packet for SetChunkCacheCenter {
    const ID: i32 = clientbound::SET_CHUNK_CACHE_CENTER;

    fn read(reader: &mut Reader<'_>) -> Self {
        Self {
            x: varint(reader),
            z: varint(reader),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForgetLevelChunk {
    pub x: i32,
    pub z: i32,
}

impl Packet for ForgetLevelChunk {
    const ID: i32 = clientbound::FORGET_LEVEL_CHUNK;

    fn read(reader: &mut Reader<'_>) -> Self {
        let z = int(reader);
        let x = int(reader);
        Self { x, z }
    }
}

/// 区块包里的一个方块实体。
#[derive(Debug, Clone, PartialEq)]
pub struct BlockEntityEntry {
    pub packed_xz: u8,
    pub y: i16,
    pub kind: i32,
    /// 官方 `getUpdateTag` 的内容。空标签写成一个 `TAG_End`，这时是 `None`。
    pub data: Option<Nbt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkSection {
    pub block_count: i16,
    pub fluid_count: i16,
    pub blocks: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LevelChunk {
    pub x: i32,
    pub z: i32,
    pub sections: Vec<ChunkSection>,
    pub block_entities: Vec<BlockEntityEntry>,
}

impl LevelChunk {
    /// 这个区块里 `pos` 的方块状态。
    pub fn state_at(&self, pos: Pos) -> u32 {
        assert_eq!(
            pos.chunk(),
            (self.x, self.z),
            "position is in another chunk"
        );
        let section = &self.sections[usize::try_from((pos.y - MIN_Y) >> 4).expect("Y below world")];
        let index = ((pos.y & 15) << 8) | ((pos.z & 15) << 4) | (pos.x & 15);
        section.blocks[index as usize]
    }

    pub fn block_entity_at(&self, pos: Pos) -> Option<&BlockEntityEntry> {
        let packed_xz = (((pos.x & 15) << 4) | (pos.z & 15)) as u8;
        self.block_entities
            .iter()
            .find(|entry| entry.packed_xz == packed_xz && i32::from(entry.y) == pos.y)
    }
}

impl Packet for LevelChunk {
    const ID: i32 = clientbound::LEVEL_CHUNK_WITH_LIGHT;

    fn read(reader: &mut Reader<'_>) -> Self {
        let x = int(reader);
        let z = int(reader);
        skip_heightmaps(reader);

        let data_len = list_len(reader);
        let data = reader.bytes(data_len).expect("chunk data");
        let mut data = Reader::new(data);
        let sections = (0..SECTIONS).map(|_| read_section(&mut data)).collect();
        data.finish().expect("sections must fill the chunk data");

        let count = list_len(reader);
        let block_entities = (0..count).map(|_| read_block_entity(reader)).collect();
        skip_light(reader);
        Self {
            x,
            z,
            sections,
            block_entities,
        }
    }
}

fn skip_heightmaps(reader: &mut Reader<'_>) {
    for _ in 0..list_len(reader) {
        varint(reader);
        let longs = list_len(reader);
        reader.bytes(longs * 8).expect("heightmap");
    }
}

fn read_section(reader: &mut Reader<'_>) -> ChunkSection {
    let block_count = short(reader);
    let fluid_count = short(reader);
    let blocks = read_container(reader, BLOCKS_PER_SECTION, MAX_INDIRECT_BLOCK_BITS);
    read_container(reader, BIOMES_PER_SECTION, MAX_INDIRECT_BIOME_BITS);
    ChunkSection {
        block_count,
        fluid_count,
        blocks,
    }
}

/// 调色板容器：位数、调色板、定长的打包数据。条目不跨 Long。
fn read_container(reader: &mut Reader<'_>, entries: usize, max_indirect_bits: u8) -> Vec<u32> {
    let bits = byte(reader);
    if bits == 0 {
        return vec![varint(reader) as u32; entries];
    }
    let palette: Option<Vec<u32>> = (bits <= max_indirect_bits).then(|| {
        let count = list_len(reader);
        (0..count).map(|_| varint(reader) as u32).collect()
    });

    let per_long = 64 / usize::from(bits);
    let longs = entries.div_ceil(per_long);
    let packed = reader.bytes(longs * 8).expect("paletted data");
    let mask = (1u64 << bits) - 1;
    (0..entries)
        .map(|index| {
            let word = index / per_long;
            let bytes: [u8; 8] = packed[word * 8..word * 8 + 8]
                .try_into()
                .expect("eight bytes");
            let shift = (index % per_long) * usize::from(bits);
            let value = (u64::from_be_bytes(bytes) >> shift & mask) as u32;
            match &palette {
                Some(palette) => palette[value as usize],
                None => value,
            }
        })
        .collect()
}

fn read_block_entity(reader: &mut Reader<'_>) -> BlockEntityEntry {
    let packed_xz = byte(reader);
    let y = short(reader);
    let kind = varint(reader);
    let tag = byte(reader);
    let data = (tag != 0).then(|| Nbt::read_payload(reader, tag));
    BlockEntityEntry {
        packed_xz,
        y,
        kind,
        data,
    }
}

fn skip_light(reader: &mut Reader<'_>) {
    for _ in 0..4 {
        let bytes = list_len(reader);
        reader.bytes(bytes).expect("light mask");
    }
    for _ in 0..2 {
        for _ in 0..list_len(reader) {
            let bytes = list_len(reader);
            reader.bytes(bytes).expect("light layer");
        }
    }
}

/// 网络 NBT。根节点没有名字。
#[derive(Debug, Clone, PartialEq)]
pub enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(Vec<Nbt>),
    Compound(Vec<(String, Nbt)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Nbt {
    pub fn read(reader: &mut Reader<'_>) -> Self {
        let tag = byte(reader);
        Self::read_payload(reader, tag)
    }

    fn read_payload(reader: &mut Reader<'_>, tag: u8) -> Self {
        match tag {
            1 => Self::Byte(reader.i8().expect("NBT byte")),
            2 => Self::Short(short(reader)),
            3 => Self::Int(int(reader)),
            4 => Self::Long(reader.i64().expect("NBT long")),
            5 => Self::Float(float(reader)),
            6 => Self::Double(double(reader)),
            7 => {
                let len = nbt_len(reader);
                Self::ByteArray(reader.bytes(len).expect("NBT byte array").to_vec())
            }
            8 => Self::String(read_nbt_string(reader)),
            9 => {
                let element = byte(reader);
                let len = nbt_len(reader);
                Self::List(
                    (0..len)
                        .map(|_| Self::read_payload(reader, element))
                        .collect(),
                )
            }
            10 => {
                let mut entries = Vec::new();
                loop {
                    let tag = byte(reader);
                    if tag == 0 {
                        return Self::Compound(entries);
                    }
                    let key = read_nbt_string(reader);
                    entries.push((key, Self::read_payload(reader, tag)));
                }
            }
            11 => {
                let len = nbt_len(reader);
                Self::IntArray((0..len).map(|_| int(reader)).collect())
            }
            12 => {
                let len = nbt_len(reader);
                Self::LongArray((0..len).map(|_| reader.i64().expect("NBT long")).collect())
            }
            other => panic!("unknown NBT tag {other}"),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Self> {
        let Self::Compound(entries) = self else {
            return None;
        };
        entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Self]> {
        match self {
            Self::List(items) => Some(items),
            _ => None,
        }
    }

    /// 组件的字面文本：`TAG_String` 或者 `{text:...}`。
    pub fn text(&self) -> Option<&str> {
        self.as_str()
            .or_else(|| self.get("text").and_then(Self::as_str))
    }

    /// 这个组件和它的子组件里所有的翻译键，深度优先。
    pub fn translate_keys(&self) -> Vec<&str> {
        let mut keys = Vec::new();
        self.collect_keys(&mut keys);
        keys
    }

    fn collect_keys<'a>(&'a self, keys: &mut Vec<&'a str>) {
        keys.extend(self.get("translate").and_then(Self::as_str));
        for child in self
            .get("extra")
            .and_then(Self::as_list)
            .unwrap_or_default()
        {
            child.collect_keys(keys);
        }
    }

    /// 写着 `key` 的组件实际显示的颜色：样式从父组件往下继承。
    pub fn color_of(&self, key: &str) -> Option<&str> {
        self.color_inherited(key, None)
    }

    fn color_inherited<'a>(&'a self, key: &str, parent: Option<&'a str>) -> Option<&'a str> {
        let color = self.get("color").and_then(Self::as_str).or(parent);
        if self.get("translate").and_then(Self::as_str) == Some(key) {
            return color;
        }
        self.get("extra")
            .and_then(Self::as_list)
            .unwrap_or_default()
            .iter()
            .find_map(|child| child.color_inherited(key, color))
    }
}

fn nbt_len(reader: &mut Reader<'_>) -> usize {
    usize::try_from(int(reader)).expect("negative NBT length")
}

/// Java Modified UTF-8：两字节长度，补充平面的字符写成两个代理项，各占 3 字节。
fn read_nbt_string(reader: &mut Reader<'_>) -> String {
    let len = usize::from(reader.u16().expect("NBT string length"));
    let bytes = reader.bytes(len).expect("NBT string");
    let mut units = Vec::with_capacity(len);
    let mut rest = bytes;
    while let Some(&first) = rest.first() {
        let (unit, used) = match first {
            0x00..=0x7f => (u16::from(first), 1),
            0xc0..=0xdf => (
                (u16::from(first) & 0x1f) << 6 | (u16::from(rest[1]) & 0x3f),
                2,
            ),
            0xe0..=0xef => (
                (u16::from(first) & 0x0f) << 12
                    | (u16::from(rest[1]) & 0x3f) << 6
                    | (u16::from(rest[2]) & 0x3f),
                3,
            ),
            other => panic!("byte {other:#x} cannot start a Modified UTF-8 character"),
        };
        units.push(unit);
        rest = &rest[used..];
    }
    String::from_utf16(&units).expect("NBT string has a lone surrogate")
}

// ---- 客户端看到的世界 ----

/// 客户端眼里的另一名玩家。位置和 `VecDeltaCodec` 一样用定点数累加。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tracked {
    pub x: i64,
    pub y: i64,
    pub z: i64,
    pub on_ground: Option<bool>,
    pub yaw: u8,
    pub pitch: u8,
    pub head_yaw: u8,
}

impl Tracked {
    fn apply_steps(&mut self, steps: &[DeltaStep]) {
        for step in steps {
            self.x += i64::from(step.x);
            self.y += i64::from(step.y);
            self.z += i64::from(step.z);
        }
    }

    pub fn is_at(&self, x: f64, y: f64, z: f64) -> bool {
        (self.x, self.y, self.z) == (fixed(x), fixed(y), fixed(z))
    }
}

fn known<'a>(tracked: &'a mut Option<Tracked>, kind: &str) -> &'a mut Tracked {
    tracked
        .as_mut()
        .unwrap_or_else(|| panic!("{kind} for an entity the client does not know"))
}

/// 按客户端的做法重放收到的包，算出 `entity` 现在的样子。客户端不知道它时返回 `None`。
pub fn replay_entity(frames: &[Frame], entity: i32) -> Option<Tracked> {
    let mut tracked = None;
    for frame in frames {
        match frame.id {
            clientbound::ADD_ENTITY => {
                let packet: AddEntity = frame.decode();
                if packet.id == entity {
                    tracked = Some(Tracked {
                        x: fixed(packet.x),
                        y: fixed(packet.y),
                        z: fixed(packet.z),
                        on_ground: None,
                        yaw: packet.yaw,
                        pitch: packet.pitch,
                        head_yaw: packet.head_yaw,
                    });
                }
            }
            clientbound::REMOVE_ENTITIES => {
                let packet: RemoveEntities = frame.decode();
                if packet.ids.contains(&entity) {
                    tracked = None;
                }
            }
            clientbound::MOVE_ENTITY_POS => {
                let packet: MoveEntityPos = frame.decode();
                if packet.entity == entity {
                    let state = known(&mut tracked, "move_entity_pos");
                    state.apply_steps(&packet.steps);
                    state.on_ground = Some(packet.on_ground);
                }
            }
            clientbound::MOVE_ENTITY_POS_ROT => {
                let packet: MoveEntityPosRot = frame.decode();
                if packet.entity == entity {
                    let state = known(&mut tracked, "move_entity_pos_rot");
                    state.apply_steps(&packet.steps);
                    state.on_ground = Some(packet.on_ground);
                    state.yaw = packet.yaw;
                    state.pitch = packet.pitch;
                }
            }
            clientbound::MOVE_ENTITY_ROT => {
                let packet: MoveEntityRot = frame.decode();
                if packet.entity == entity {
                    let state = known(&mut tracked, "move_entity_rot");
                    state.on_ground = Some(packet.on_ground);
                    state.yaw = packet.yaw;
                    state.pitch = packet.pitch;
                }
            }
            clientbound::ENTITY_POSITION_SYNC => {
                let packet: EntityPositionSync = frame.decode();
                if packet.entity == entity {
                    let state = known(&mut tracked, "entity_position_sync");
                    let (x, y, z) = packet.path.end();
                    (state.x, state.y, state.z) = (fixed(x), fixed(y), fixed(z));
                    state.on_ground = Some(packet.on_ground);
                    state.yaw = pack_degrees(packet.yaw);
                    state.pitch = pack_degrees(packet.pitch);
                }
            }
            clientbound::ROTATE_HEAD => {
                let packet: RotateHead = frame.decode();
                if packet.entity == entity {
                    known(&mut tracked, "rotate_head").head_yaw = packet.head_yaw;
                }
            }
            _ => {}
        }
    }
    tracked
}

/// 客户端现在看到的这一格：最新的区块或者 `block_update`。区块没收到时是 `None`。
pub fn replay_block(frames: &[Frame], pos: Pos) -> Option<u32> {
    let mut state = None;
    for frame in frames {
        match frame.id {
            clientbound::LEVEL_CHUNK_WITH_LIGHT => {
                let mut header = Reader::new(&frame.body);
                if (int(&mut header), int(&mut header)) == pos.chunk() {
                    state = Some(frame.decode::<LevelChunk>().state_at(pos));
                }
            }
            clientbound::FORGET_LEVEL_CHUNK => {
                let packet: ForgetLevelChunk = frame.decode();
                if (packet.x, packet.z) == pos.chunk() {
                    state = None;
                }
            }
            clientbound::BLOCK_UPDATE => {
                let packet: BlockUpdate = frame.decode();
                if packet.pos == pos && state.is_some() {
                    state = Some(packet.state);
                }
            }
            _ => {}
        }
    }
    state
}

/// 断言分隔符成对：`add_entity` 都在 bundle 里，断开包不在 bundle 里，结尾没有开着的 bundle。
pub fn assert_bundles_paired(name: &str, frames: &[Frame]) {
    let mut inside = false;
    for frame in frames {
        match frame.id {
            clientbound::BUNDLE_DELIMITER => inside = !inside,
            clientbound::ADD_ENTITY => {
                assert!(inside, "{name}: add_entity outside of a bundle");
            }
            clientbound::DISCONNECT => {
                assert!(!inside, "{name}: disconnect inside a bundle");
            }
            _ => {}
        }
    }
    assert!(!inside, "{name}: the last bundle was never closed");
}

/// 失败信息里用的包名。
pub fn packet_name(id: i32) -> String {
    let name = match id {
        clientbound::ADD_ENTITY => "add_entity",
        clientbound::AWARD_STATS => "award_stats",
        clientbound::BLOCK_CHANGED_ACK => "block_changed_ack",
        clientbound::BLOCK_UPDATE => "block_update",
        clientbound::BUNDLE_DELIMITER => "bundle_delimiter",
        clientbound::CHUNK_BATCH_FINISHED => "chunk_batch_finished",
        clientbound::CHUNK_BATCH_START => "chunk_batch_start",
        clientbound::CONTAINER_SET_SLOT => "container_set_slot",
        clientbound::DISCONNECT => "disconnect",
        clientbound::DISGUISED_CHAT => "disguised_chat",
        clientbound::ENTITY_POSITION_SYNC => "entity_position_sync",
        clientbound::FORGET_LEVEL_CHUNK => "forget_level_chunk",
        clientbound::GAME_EVENT => "game_event",
        clientbound::KEEP_ALIVE => "keep_alive",
        clientbound::LEVEL_CHUNK_WITH_LIGHT => "level_chunk_with_light",
        clientbound::LEVEL_EVENT => "level_event",
        clientbound::LOGIN => "login",
        clientbound::MOVE_ENTITY_POS => "move_entity_pos",
        clientbound::MOVE_ENTITY_POS_ROT => "move_entity_pos_rot",
        clientbound::MOVE_ENTITY_ROT => "move_entity_rot",
        clientbound::PLAYER_ABILITIES => "player_abilities",
        clientbound::PLAYER_INFO_REMOVE => "player_info_remove",
        clientbound::PLAYER_INFO_UPDATE => "player_info_update",
        clientbound::PLAYER_POSITION => "player_position",
        clientbound::PONG_RESPONSE => "pong_response",
        clientbound::REMOVE_ENTITIES => "remove_entities",
        clientbound::ROTATE_HEAD => "rotate_head",
        clientbound::SECTION_BLOCKS_UPDATE => "section_blocks_update",
        clientbound::SET_CHUNK_CACHE_CENTER => "set_chunk_cache_center",
        clientbound::SET_ENTITY_DATA => "set_entity_data",
        clientbound::SET_EQUIPMENT => "set_equipment",
        clientbound::SET_HELD_SLOT => "set_held_slot",
        clientbound::SET_TIME => "set_time",
        clientbound::SWING_ANIMATION => "swing_animation",
        clientbound::SYSTEM_CHAT => "system_chat",
        clientbound::UPDATE_ATTRIBUTES => "update_attributes",
        other => return format!("#{other}"),
    };
    name.to_owned()
}
