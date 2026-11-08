//! 客户端在 Play 状态发送的数据包。

use std::collections::BTreeMap;

use mc_bamboo_core::{
    BlockPos, Direction, Hand, Vec3,
    sign::{LINES, MAX_LINE_CHARS, SignTextSlot},
};
use mc_bamboo_data::{items::ITEMS, packets::play::serverbound, registries::data_component_type};

use crate::{
    codec::{DecodeError, Reader},
    components::book::{MAX_PAGES, PAGE_EDIT_LENGTH, TITLE_MAX_LENGTH},
    item::ItemStack,
};

const MAX_CHAT_CHARS: usize = 256;
const MAX_COMMAND_CHARS: usize = 32767;
const MAX_LANGUAGE_CHARS: usize = 16;
/// `ServerboundContainerClickPacket` 的 `changedSlots` 最多有多少项。
const MAX_CHANGED_SLOTS: usize = 128;
/// `HashedPatchMap` 里增加项和移除项各自的上限。
const MAX_HASHED_COMPONENTS: usize = 256;
/// 聊天签名的长度，线上没有长度前缀。
const SIGNATURE_BYTES: usize = 256;
/// `LastSeenMessages.Update` 里 20 位定长位集占的字节数。
const LAST_SEEN_ACKNOWLEDGED_BYTES: usize = 3;

/// 移动包 `flags` 里表示站在地上的位。
const MOVE_FLAG_ON_GROUND: u8 = 0x01;

/// [`PlayRequest::PlayerInput`] 的 `flags` 里表示按住潜行键的位。
pub const INPUT_FLAG_SHIFT: u8 = 0x20;
/// [`PlayRequest::PlayerAbilities`] 的 `flags` 里表示正在飞行的位。
pub const ABILITY_FLAG_FLYING: u8 = 0x02;
/// [`ClientInformation::model_customisation`] 里表示帽子的位。
pub const MODEL_PART_HAT: u8 = 0x40;

/// 客户端在 Play 状态发送的数据包。
///
/// 这里只解码服务端关心的包，其他的都归入 [`Other`](Self::Other)。
/// 不解码的包内容是什么都不算错误，调用方直接丢弃。
#[derive(Debug, Clone, PartialEq)]
pub enum PlayRequest {
    /// 对 [`KeepAlive`](super::KeepAlive) 的回应。
    KeepAlive(i64),
    /// 要求用 [`PongResponse`](super::PongResponse) 原样返回这个时间。
    PingRequest(i64),
    /// 客户端确认了 [`PlayerPosition`](super::PlayerPosition) 这次传送，
    /// 后面是它应用传送之后的位置和朝向。
    AcceptTeleportation {
        id: i32,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
    },
    /// 四种移动包合并而成，线上没有的部分是 `None`。
    MovePlayer {
        position: Option<Vec3>,
        /// `(yaw, pitch)`。
        rotation: Option<(f32, f32)>,
        on_ground: bool,
    },
    /// 收到了一批区块，并告诉服务端之后每 tick 发几个。
    ChunkBatchReceived {
        desired_chunks_per_tick: f32,
    },
    /// 聊天消息。时间戳、盐、签名和已读窗口都解码后丢弃了，离线模式用不上。
    Chat {
        message: String,
    },
    /// 没有开头 `/` 的命令。
    ChatCommand {
        command: String,
    },
    ClientInformation(ClientInformation),
    ClientCommand {
        action: ClientCommandKind,
    },
    /// 按键状态。潜行看 [`INPUT_FLAG_SHIFT`]。
    PlayerInput {
        flags: u8,
    },
    PlayerCommand {
        action: PlayerCommandKind,
    },
    /// 飞行状态变了。看 [`ABILITY_FLAG_FLYING`]。
    PlayerAbilities {
        flags: u8,
    },
    /// 左键挥手。
    Punch,
    /// 选中了快捷栏的另一格。官方接受范围外的值，只是不生效，所以这里也不算解码错误。
    SetCarriedItem {
        slot: i16,
    },
    /// 创造模式物品栏改了一个槽。`slot` 是 `InventoryMenu` 的编号，负数表示丢弃。
    /// `stack.item` 一定是已注册的物品，组件的内容没有校验，见 [`ItemStack`]。
    SetCreativeModeSlot {
        slot: i16,
        stack: ItemStack,
    },
    PlayerAction {
        action: PlayerActionKind,
        pos: BlockPos,
        face: Direction,
        sequence: i32,
    },
    /// 右键点击方块。`cursor` 是点击点减去 `pos`。
    UseItemOn {
        hand: Hand,
        pos: BlockPos,
        face: Direction,
        cursor: (f32, f32, f32),
        inside: bool,
        world_border_hit: bool,
        sequence: i32,
    },
    UseItem {
        hand: Hand,
        sequence: i32,
        yaw: f32,
        pitch: f32,
    },
    /// 中键选取方块。
    PickItemFromBlock {
        pos: BlockPos,
        include_data: bool,
    },
    /// 在菜单里点击。`changed_slots` 是客户端预测出来变了的槽，值只有物品的哈希。
    ContainerClick {
        container_id: i32,
        state_id: i32,
        slot: i16,
        button: i8,
        input: ContainerInput,
        changed_slots: BTreeMap<i16, HashedStack>,
        carried: HashedStack,
    },
    /// 客户端关掉了菜单。
    ContainerClose {
        container_id: i32,
    },
    /// 讲台翻页、跳页或取书。`button_id` 是 varint，100 起是跳到的页码加 100。
    ///
    /// Lectern page controls. `button_id` is a varint; values from 100 jump to
    /// `button_id - 100`.
    ContainerButtonClick {
        container_id: i32,
        button_id: i32,
    },
    /// 告示牌编辑器提交的文字，每一行最多 [`MAX_LINE_CHARS`] 个字符。
    SignUpdate {
        pos: BlockPos,
        lines: [String; LINES],
        slot: SignTextSlot,
    },
    /// 书与笔保存页面，或者带标题时署名。
    ///
    /// `slot` 是官方 `Inventory` 的下标，不是菜单槽：快捷栏 0 到 8，副手 40。
    /// 页数、每页长度和标题长度是 `ServerboundEditBookPacket` 的解码上限。
    EditBook {
        slot: i32,
        pages: Vec<String>,
        /// 有标题就是署名，没有就是只保存页面。
        title: Option<String>,
    },
    /// 服务端不解码的包，内容是包 ID。
    Other(i32),
}

impl PlayRequest {
    /// 解码一帧的负载，负载以数据包 ID 开头。
    pub fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(payload);
        let request = match reader.varint()? {
            serverbound::KEEP_ALIVE => Self::KeepAlive(reader.i64()?),
            serverbound::PING_REQUEST => Self::PingRequest(reader.i64()?),
            serverbound::ACCEPT_TELEPORTATION => Self::AcceptTeleportation {
                id: reader.varint()?,
                x: reader.f64()?,
                y: reader.f64()?,
                z: reader.f64()?,
                yaw: reader.f32()?,
                pitch: reader.f32()?,
            },
            serverbound::MOVE_PLAYER_POS => {
                let position = read_position(&mut reader)?;
                Self::move_player(Some(position), None, reader.u8()?)
            }
            serverbound::MOVE_PLAYER_POS_ROT => {
                let position = read_position(&mut reader)?;
                let rotation = read_rotation(&mut reader)?;
                Self::move_player(Some(position), Some(rotation), reader.u8()?)
            }
            serverbound::MOVE_PLAYER_ROT => {
                let rotation = read_rotation(&mut reader)?;
                Self::move_player(None, Some(rotation), reader.u8()?)
            }
            serverbound::MOVE_PLAYER_STATUS_ONLY => Self::move_player(None, None, reader.u8()?),
            serverbound::CHUNK_BATCH_RECEIVED => Self::ChunkBatchReceived {
                desired_chunks_per_tick: reader.f32()?,
            },
            serverbound::CHAT => read_chat(&mut reader)?,
            serverbound::CHAT_COMMAND => Self::ChatCommand {
                command: reader.string(MAX_COMMAND_CHARS)?.to_owned(),
            },
            serverbound::CLIENT_INFORMATION => {
                Self::ClientInformation(ClientInformation::read(&mut reader)?)
            }
            serverbound::CLIENT_COMMAND => Self::ClientCommand {
                action: by_ordinal(&ClientCommandKind::ALL, reader.varint()?)?,
            },
            serverbound::PLAYER_INPUT => Self::PlayerInput {
                flags: reader.u8()?,
            },
            serverbound::PLAYER_COMMAND => read_player_command(&mut reader)?,
            serverbound::PLAYER_ABILITIES => Self::PlayerAbilities {
                flags: reader.u8()?,
            },
            serverbound::PUNCH => Self::Punch,
            serverbound::SET_CARRIED_ITEM => Self::SetCarriedItem {
                slot: reader.i16()?,
            },
            serverbound::SET_CREATIVE_MODE_SLOT => return read_creative_mode_slot(reader),
            serverbound::PLAYER_ACTION => Self::PlayerAction {
                action: by_ordinal(&PlayerActionKind::ALL, reader.varint()?)?,
                pos: reader.block_pos()?,
                face: Direction::from_id(i32::from(reader.u8()?)),
                sequence: reader.varint()?,
            },
            serverbound::USE_ITEM_ON => Self::UseItemOn {
                hand: Hand::from_id(reader.varint()?),
                pos: reader.block_pos()?,
                face: Direction::from_id(reader.varint()?),
                cursor: (reader.f32()?, reader.f32()?, reader.f32()?),
                inside: reader.bool()?,
                world_border_hit: reader.bool()?,
                sequence: reader.varint()?,
            },
            serverbound::USE_ITEM => Self::UseItem {
                hand: Hand::from_id(reader.varint()?),
                sequence: reader.varint()?,
                yaw: reader.f32()?,
                pitch: reader.f32()?,
            },
            serverbound::PICK_ITEM_FROM_BLOCK => Self::PickItemFromBlock {
                pos: reader.block_pos()?,
                include_data: reader.bool()?,
            },
            serverbound::CONTAINER_CLICK => read_container_click(&mut reader)?,
            serverbound::CONTAINER_CLOSE => Self::ContainerClose {
                container_id: reader.varint()?,
            },
            serverbound::CONTAINER_BUTTON_CLICK => Self::ContainerButtonClick {
                container_id: reader.varint()?,
                button_id: reader.varint()?,
            },
            serverbound::SIGN_UPDATE => read_sign_update(&mut reader)?,
            serverbound::EDIT_BOOK => read_edit_book(&mut reader)?,
            other => return Ok(Self::Other(other)),
        };
        reader.finish()?;
        Ok(request)
    }

    fn move_player(position: Option<Vec3>, rotation: Option<(f32, f32)>, flags: u8) -> Self {
        Self::MovePlayer {
            position,
            rotation,
            on_ground: flags & MOVE_FLAG_ON_GROUND != 0,
        }
    }
}

fn read_position(reader: &mut Reader<'_>) -> Result<Vec3, DecodeError> {
    Ok(Vec3::new(reader.f64()?, reader.f64()?, reader.f64()?))
}

/// 线上总是 yaw 在前。
fn read_rotation(reader: &mut Reader<'_>) -> Result<(f32, f32), DecodeError> {
    Ok((reader.f32()?, reader.f32()?))
}

fn read_chat(reader: &mut Reader<'_>) -> Result<PlayRequest, DecodeError> {
    let message = reader.string(MAX_CHAT_CHARS)?.to_owned();
    // 时间戳和盐。
    reader.i64()?;
    reader.i64()?;
    if reader.bool()? {
        reader.bytes(SIGNATURE_BYTES)?;
    }
    // LastSeenMessages.Update：偏移、确认窗口、校验和。
    reader.varint()?;
    reader.bytes(LAST_SEEN_ACKNOWLEDGED_BYTES)?;
    reader.i8()?;
    Ok(PlayRequest::Chat { message })
}

/// 解码 `sign_update`：方块坐标、四行文字、哪一面。官方按这个顺序写。
fn read_sign_update(reader: &mut Reader<'_>) -> Result<PlayRequest, DecodeError> {
    let pos = reader.block_pos()?;
    let mut lines: [String; LINES] = std::array::from_fn(|_| String::new());
    for line in &mut lines {
        *line = reader.string(MAX_LINE_CHARS)?.to_owned();
    }
    let slot = SignTextSlot::from_id(reader.varint()?);
    Ok(PlayRequest::SignUpdate { pos, lines, slot })
}

/// 解码 `edit_book`：槽位、页面、可选的标题。官方 `ServerboundEditBookPacket.STREAM_CODEC`。
fn read_edit_book(reader: &mut Reader<'_>) -> Result<PlayRequest, DecodeError> {
    let slot = reader.varint()?;
    let page_count = reader.count(MAX_PAGES)?;
    let mut pages = Vec::with_capacity(page_count);
    for _ in 0..page_count {
        pages.push(reader.string(PAGE_EDIT_LENGTH)?.to_owned());
    }
    let title = if reader.bool()? {
        Some(reader.string(TITLE_MAX_LENGTH)?.to_owned())
    } else {
        None
    };
    Ok(PlayRequest::EditBook { slot, pages, title })
}

fn read_player_command(reader: &mut Reader<'_>) -> Result<PlayRequest, DecodeError> {
    // 玩家自己的实体 ID，官方也不读。
    reader.varint()?;
    let action = by_ordinal(&PlayerCommandKind::ALL, reader.varint()?)?;
    // 只有 START_RIDING_JUMP 用的参数。
    reader.varint()?;
    Ok(PlayRequest::PlayerCommand { action })
}

/// 解码 `set_creative_mode_slot`。
///
/// 物品解码失败时官方断开连接，规格（`blocks-inventory.md` 2.5）允许 Bamboo 只忽略这个包。
/// 这里选择忽略，结果是 [`PlayRequest::Other`]，不会发到世界线程。不能用空物品表示：
/// 世界线程会把它当成清空槽位，而忽略的意思是槽位不变。
///
/// 这个包不调用 `finish`（`blocks-inventory.md` 2.5、`coverage.md` 1.0）。
/// 帧有长度前缀，物品后面多余的字节不会让字节流错位。
fn read_creative_mode_slot(mut reader: Reader<'_>) -> Result<PlayRequest, DecodeError> {
    let slot = reader.i16()?;
    Ok(match ItemStack::read_delimited(&mut reader) {
        Ok(stack) => PlayRequest::SetCreativeModeSlot { slot, stack },
        Err(_) => PlayRequest::Other(serverbound::SET_CREATIVE_MODE_SLOT),
    })
}

/// 解码 `container_click`。槽位和按钮的范围由调用方判断，官方也是这样。
fn read_container_click(reader: &mut Reader<'_>) -> Result<PlayRequest, DecodeError> {
    let container_id = reader.varint()?;
    let state_id = reader.varint()?;
    let slot = reader.i16()?;
    let button = reader.i8()?;
    let input = ContainerInput::from_id(reader.varint()?);

    let count = reader.count(MAX_CHANGED_SLOTS)?;
    let mut changed_slots = BTreeMap::new();
    for _ in 0..count {
        let slot = reader.i16()?;
        changed_slots.insert(slot, read_hashed_stack(reader)?);
    }

    Ok(PlayRequest::ContainerClick {
        container_id,
        state_id,
        slot,
        button,
        input,
        changed_slots,
        carried: read_hashed_stack(reader)?,
    })
}

/// `HashedStack.STREAM_CODEC`：没有物品时是一个 `false`，否则物品、数量和组件哈希。
fn read_hashed_stack(reader: &mut Reader<'_>) -> Result<HashedStack, DecodeError> {
    if !reader.bool()? {
        return Ok(HashedStack::Empty);
    }
    let item = reader.varint()?;
    if !usize::try_from(item).is_ok_and(|id| id < ITEMS.len()) {
        return Err(DecodeError::InvalidItem(item));
    }
    let count = reader.varint()?;

    let added_len = reader.count(MAX_HASHED_COMPONENTS)?;
    let mut added = Vec::with_capacity(added_len);
    for _ in 0..added_len {
        added.push((read_component_type(reader)?, reader.i32()?));
    }

    let removed_len = reader.count(MAX_HASHED_COMPONENTS)?;
    let mut removed = Vec::with_capacity(removed_len);
    for _ in 0..removed_len {
        removed.push(read_component_type(reader)?);
    }

    Ok(HashedStack::Item(HashedItem {
        item,
        count,
        added,
        removed,
    }))
}

fn read_component_type(reader: &mut Reader<'_>) -> Result<i32, DecodeError> {
    let kind = reader.varint()?;
    if u32::try_from(kind).is_ok_and(|kind| kind < data_component_type::TOTAL) {
        Ok(kind)
    } else {
        Err(DecodeError::InvalidComponentType(kind))
    }
}

/// 解码官方用 `readEnum` 读的字段。这种字段越界时官方解码失败，不是回绕。
fn by_ordinal<T: Copy>(values: &[T], id: i32) -> Result<T, DecodeError> {
    usize::try_from(id)
        .ok()
        .and_then(|index| values.get(index).copied())
        .ok_or(DecodeError::InvalidEnum(id))
}

/// 点击的类型，`ContainerInput`。数值就是网络上的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerInput {
    Pickup = 0,
    QuickMove = 1,
    Swap = 2,
    Clone = 3,
    Throw = 4,
    QuickCraft = 5,
    PickupAll = 6,
}

impl ContainerInput {
    /// 官方 `ByIdMap.continuous(..., OutOfBoundsStrategy.ZERO)`：越界的值都当成 `PICKUP`。
    pub const fn from_id(id: i32) -> Self {
        match id {
            1 => Self::QuickMove,
            2 => Self::Swap,
            3 => Self::Clone,
            4 => Self::Throw,
            5 => Self::QuickCraft,
            6 => Self::PickupAll,
            _ => Self::Pickup,
        }
    }
}

/// 客户端发来的物品摘要。组件只有哈希，服务端没法还原，见 [`HashedItem`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashedStack {
    /// 空物品。
    Empty,
    Item(HashedItem),
}

/// `HashedStack.ActualItem`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashedItem {
    pub item: i32,
    pub count: i32,
    /// 组件类型和官方算出来的 CRC32C 哈希。
    pub added: Vec<(i32, i32)>,
    pub removed: Vec<i32>,
}

/// `client_command` 的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientCommandKind {
    PerformRespawn,
    RequestStats,
    RequestGameRuleValues,
}

impl ClientCommandKind {
    const ALL: [Self; 3] = [
        Self::PerformRespawn,
        Self::RequestStats,
        Self::RequestGameRuleValues,
    ];
}

/// `player_command` 的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerCommandKind {
    StopSleeping,
    StartSprinting,
    StopSprinting,
    StartRidingJump,
    StopRidingJump,
    OpenInventory,
    StartFallFlying,
}

impl PlayerCommandKind {
    const ALL: [Self; 7] = [
        Self::StopSleeping,
        Self::StartSprinting,
        Self::StopSprinting,
        Self::StartRidingJump,
        Self::StopRidingJump,
        Self::OpenInventory,
        Self::StartFallFlying,
    ];
}

/// `player_action` 的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerActionKind {
    StartDestroyBlock,
    ChangeDestroyDirection,
    AbortDestroyBlock,
    StopDestroyBlock,
    DropAllItems,
    DropItem,
    ReleaseUseItem,
    SwapItemWithOffhand,
    Stab,
}

impl PlayerActionKind {
    const ALL: [Self; 9] = [
        Self::StartDestroyBlock,
        Self::ChangeDestroyDirection,
        Self::AbortDestroyBlock,
        Self::StopDestroyBlock,
        Self::DropAllItems,
        Self::DropItem,
        Self::ReleaseUseItem,
        Self::SwapItemWithOffhand,
        Self::Stab,
    ];
}

/// 客户端的设置，Configuration 和 Play 状态的格式相同。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInformation {
    pub language: String,
    /// 客户端的渲染距离。线上是有符号字节，使用前要 clamp。
    pub view_distance: i8,
    pub chat_visibility: ChatVisibility,
    pub chat_colors: bool,
    /// 皮肤外层的掩码，直接写进实体元数据 index 16。帽子是 [`MODEL_PART_HAT`]。
    pub model_customisation: u8,
    pub main_hand: HumanoidArm,
    pub text_filtering_enabled: bool,
    pub allows_listing: bool,
    pub particle_status: ParticleStatus,
}

impl ClientInformation {
    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            language: reader.string(MAX_LANGUAGE_CHARS)?.to_owned(),
            view_distance: reader.i8()?,
            chat_visibility: ChatVisibility::from_id(reader.varint()?),
            chat_colors: reader.bool()?,
            model_customisation: reader.u8()?,
            main_hand: HumanoidArm::from_id(reader.varint()?),
            text_filtering_enabled: reader.bool()?,
            allows_listing: reader.bool()?,
            particle_status: ParticleStatus::from_id(reader.varint()?),
        })
    }
}

impl Default for ClientInformation {
    /// 和官方的 `ClientInformation.createDefault()` 相同。
    fn default() -> Self {
        Self {
            language: "en_us".to_owned(),
            view_distance: 2,
            chat_visibility: ChatVisibility::Full,
            chat_colors: true,
            model_customisation: 0,
            main_hand: HumanoidArm::Right,
            text_filtering_enabled: false,
            allows_listing: false,
            particle_status: ParticleStatus::All,
        }
    }
}

/// 聊天可见性。枚举值就是网络上的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatVisibility {
    Full = 0,
    System = 1,
    Hidden = 2,
}

impl ChatVisibility {
    const ALL: [Self; 3] = [Self::Full, Self::System, Self::Hidden];

    /// 从网络 ID 解码。越界时和官方一样取模回绕，不算错误。
    pub const fn from_id(id: i32) -> Self {
        Self::ALL[id.rem_euclid(3) as usize]
    }
}

/// 粒子效果设置。枚举值就是网络上的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleStatus {
    All = 0,
    Decreased = 1,
    Minimal = 2,
}

impl ParticleStatus {
    const ALL: [Self; 3] = [Self::All, Self::Decreased, Self::Minimal];

    /// 从网络 ID 解码。越界时和官方一样取模回绕，不算错误。
    pub const fn from_id(id: i32) -> Self {
        Self::ALL[id.rem_euclid(3) as usize]
    }
}

/// 主手。枚举值就是网络上的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanoidArm {
    Left = 0,
    Right = 1,
}

impl HumanoidArm {
    /// 从网络 ID 解码。官方把越界的值当作左手。
    pub const fn from_id(id: i32) -> Self {
        match id {
            1 => Self::Right,
            _ => Self::Left,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        codec::{write_string, write_varint},
        frame::{MAX_FRAME_LEN, find_frame},
        packet::test_util::hex,
    };

    /// 解码规格里的完整帧，确认帧头的长度也是对的。
    fn decode_frame(text: &str) -> PlayRequest {
        let frame = hex(text);
        let bounds = find_frame(&frame, MAX_FRAME_LEN).unwrap().unwrap();
        assert_eq!(frame.len(), bounds.header_len + bounds.payload_len);
        PlayRequest::decode(&frame[bounds.header_len..]).unwrap()
    }

    /// 解码规格里只有包体的例子。
    fn decode_body(id: i32, body: &str) -> Result<PlayRequest, DecodeError> {
        let mut payload = Vec::new();
        write_varint(&mut payload, id);
        payload.extend(hex(body));
        PlayRequest::decode(&payload)
    }

    #[test]
    fn decoded_requests_can_cross_threads() {
        fn assert_owned<T: Send + 'static>() {}
        assert_owned::<PlayRequest>();
    }

    #[test]
    fn decodes_keep_alive() {
        assert_eq!(
            decode_body(serverbound::KEEP_ALIVE, "00 00 00 00 00 00 00 01"),
            Ok(PlayRequest::KeepAlive(1))
        );
    }

    #[test]
    fn decodes_ping_request() {
        assert_eq!(
            decode_body(serverbound::PING_REQUEST, "00 00 01 8b cf e5 68 00"),
            Ok(PlayRequest::PingRequest(0x0000_018b_cfe5_6800))
        );
    }

    #[test]
    fn decodes_accept_teleportation_with_its_position() {
        assert_eq!(
            decode_frame(
                "22 00  01
                 40 21 00 00 00 00 00 00  c0 4e 00 00 00 00 00 00  40 21 00 00 00 00 00 00
                 00 00 00 00  00 00 00 00"
            ),
            PlayRequest::AcceptTeleportation {
                id: 1,
                x: 8.5,
                y: -60.0,
                z: 8.5,
                yaw: 0.0,
                pitch: 0.0,
            }
        );
    }

    #[test]
    fn merges_the_four_move_packets() {
        let position = Vec3::new(8.5, -60.0, 8.5);

        assert_eq!(
            decode_frame(
                "1a 1e  40 21 00 00 00 00 00 00  c0 4e 00 00 00 00 00 00
                        40 21 00 00 00 00 00 00  01"
            ),
            PlayRequest::MovePlayer {
                position: Some(position),
                rotation: None,
                on_ground: true,
            }
        );
        assert_eq!(
            decode_frame(
                "22 1f  40 21 00 00 00 00 00 00  c0 4e 00 00 00 00 00 00
                        40 21 00 00 00 00 00 00  42 b4 00 00  c2 34 00 00  03"
            ),
            PlayRequest::MovePlayer {
                position: Some(position),
                rotation: Some((90.0, -45.0)),
                on_ground: true,
            }
        );
        assert_eq!(
            decode_frame("0a 20  42 b4 00 00  c2 34 00 00  00"),
            PlayRequest::MovePlayer {
                position: None,
                rotation: Some((90.0, -45.0)),
                on_ground: false,
            }
        );
        assert_eq!(
            decode_frame("02 21 01"),
            PlayRequest::MovePlayer {
                position: None,
                rotation: None,
                on_ground: true,
            }
        );
    }

    #[test]
    fn horizontal_collision_alone_is_not_on_ground() {
        assert_eq!(
            decode_body(serverbound::MOVE_PLAYER_STATUS_ONLY, "02"),
            Ok(PlayRequest::MovePlayer {
                position: None,
                rotation: None,
                on_ground: false,
            })
        );
    }

    #[test]
    fn decodes_chunk_batch_received() {
        assert_eq!(
            decode_frame("05 0b 40 60 00 00"),
            PlayRequest::ChunkBatchReceived {
                desired_chunks_per_tick: 3.5
            }
        );
    }

    const CHAT_BODY: &str = "02 68 69
                             00 00 01 8b cf e5 68 00
                             11 22 33 44 55 66 77 88
                             00
                             00
                             00 00 00
                             01";

    #[test]
    fn decodes_chat_and_discards_the_signing_fields() {
        assert_eq!(
            decode_body(serverbound::CHAT, CHAT_BODY),
            Ok(PlayRequest::Chat {
                message: "hi".to_owned()
            })
        );
    }

    #[test]
    fn chat_discards_a_non_empty_last_seen_update() {
        assert_eq!(
            decode_body(
                serverbound::CHAT,
                "02 68 69
                 00 00 01 8b cf e5 68 00
                 11 22 33 44 55 66 77 88
                 00
                 03 01 02 08 5a"
            ),
            Ok(PlayRequest::Chat {
                message: "hi".to_owned()
            })
        );
    }

    #[test]
    fn chat_skips_a_signature_when_present() {
        let mut payload = vec![serverbound::CHAT as u8];
        payload.extend(hex("02 68 69"));
        payload.extend([0; 16]);
        payload.push(1);
        payload.extend([0xab; SIGNATURE_BYTES]);
        payload.extend(hex("00 00 00 00 01"));

        assert_eq!(
            PlayRequest::decode(&payload),
            Ok(PlayRequest::Chat {
                message: "hi".to_owned()
            })
        );
    }

    #[test]
    fn chat_length_counts_utf16_units() {
        let mut payload = vec![serverbound::CHAT as u8];
        write_string(&mut payload, &"竹".repeat(MAX_CHAT_CHARS));
        payload.extend([0; 16]);
        payload.extend(hex("00 00 00 00 00 01"));
        assert_eq!(
            PlayRequest::decode(&payload),
            Ok(PlayRequest::Chat {
                message: "竹".repeat(MAX_CHAT_CHARS)
            })
        );

        let mut payload = vec![serverbound::CHAT as u8];
        write_string(&mut payload, &"竹".repeat(MAX_CHAT_CHARS + 1));
        assert_eq!(
            PlayRequest::decode(&payload),
            Err(DecodeError::StringTooLong {
                max_chars: MAX_CHAT_CHARS
            })
        );
    }

    #[test]
    fn chat_rejects_truncated_and_trailing_bytes() {
        assert_eq!(
            decode_body(serverbound::CHAT, "02 68 69 00 00"),
            Err(DecodeError::UnexpectedEnd)
        );
        assert_eq!(
            decode_body(serverbound::CHAT, &format!("{CHAT_BODY} ff")),
            Err(DecodeError::TrailingBytes(1))
        );
    }

    #[test]
    fn decodes_chat_command() {
        assert_eq!(
            decode_body(serverbound::CHAT_COMMAND, "07 66 6f 6f 20 62 61 72"),
            Ok(PlayRequest::ChatCommand {
                command: "foo bar".to_owned()
            })
        );
    }

    #[test]
    fn decodes_client_information() {
        assert_eq!(
            decode_frame("0f 0e  05 7a 68 5f 63 6e  0c  00  01  7f  01  00  01  00"),
            PlayRequest::ClientInformation(ClientInformation {
                language: "zh_cn".to_owned(),
                view_distance: 12,
                chat_visibility: ChatVisibility::Full,
                chat_colors: true,
                model_customisation: 0x7f,
                main_hand: HumanoidArm::Right,
                text_filtering_enabled: false,
                allows_listing: true,
                particle_status: ParticleStatus::All,
            })
        );
    }

    #[test]
    fn client_information_enums_wrap_or_default_like_the_game() {
        let decoded = decode_body(
            serverbound::CLIENT_INFORMATION,
            "05 7a 68 5f 63 6e  fe  ff ff ff ff 0f  00  ff  02  01  00  05",
        );
        assert_eq!(
            decoded,
            Ok(PlayRequest::ClientInformation(ClientInformation {
                language: "zh_cn".to_owned(),
                view_distance: -2,
                chat_visibility: ChatVisibility::Hidden,
                chat_colors: false,
                model_customisation: 0xff,
                main_hand: HumanoidArm::Left,
                text_filtering_enabled: true,
                allows_listing: false,
                particle_status: ParticleStatus::Minimal,
            }))
        );
    }

    #[test]
    fn default_client_information_matches_the_game() {
        assert_eq!(
            ClientInformation::default(),
            ClientInformation {
                language: "en_us".to_owned(),
                view_distance: 2,
                chat_visibility: ChatVisibility::Full,
                chat_colors: true,
                model_customisation: 0,
                main_hand: HumanoidArm::Right,
                text_filtering_enabled: false,
                allows_listing: false,
                particle_status: ParticleStatus::All,
            }
        );
    }

    #[test]
    fn decodes_client_command() {
        assert_eq!(
            decode_body(serverbound::CLIENT_COMMAND, "01"),
            Ok(PlayRequest::ClientCommand {
                action: ClientCommandKind::RequestStats
            })
        );
        assert_eq!(
            decode_body(serverbound::CLIENT_COMMAND, "03"),
            Err(DecodeError::InvalidEnum(3))
        );
    }

    #[test]
    fn decodes_player_input() {
        assert_eq!(
            decode_frame("02 2b 41"),
            PlayRequest::PlayerInput { flags: 0x41 }
        );
        assert_eq!(
            decode_frame("02 2b 21"),
            PlayRequest::PlayerInput { flags: 0x21 }
        );
        assert_eq!(
            decode_frame("02 2b 61"),
            PlayRequest::PlayerInput { flags: 0x61 }
        );
    }

    #[test]
    fn decodes_player_command() {
        assert_eq!(
            decode_body(serverbound::PLAYER_COMMAND, "07 01 00"),
            Ok(PlayRequest::PlayerCommand {
                action: PlayerCommandKind::StartSprinting
            })
        );
        assert_eq!(
            decode_body(serverbound::PLAYER_COMMAND, "07 06 00"),
            Ok(PlayRequest::PlayerCommand {
                action: PlayerCommandKind::StartFallFlying
            })
        );
        assert_eq!(
            decode_body(serverbound::PLAYER_COMMAND, "07 07 00"),
            Err(DecodeError::InvalidEnum(7))
        );
    }

    #[test]
    fn player_command_ordinals_match_the_game() {
        let kinds = [
            PlayerCommandKind::StopSleeping,
            PlayerCommandKind::StartSprinting,
            PlayerCommandKind::StopSprinting,
            PlayerCommandKind::StartRidingJump,
            PlayerCommandKind::StopRidingJump,
            PlayerCommandKind::OpenInventory,
            PlayerCommandKind::StartFallFlying,
        ];
        for (id, action) in kinds.into_iter().enumerate() {
            assert_eq!(by_ordinal(&PlayerCommandKind::ALL, id as i32), Ok(action));
        }
    }

    #[test]
    fn decodes_player_abilities() {
        assert_eq!(
            decode_body(serverbound::PLAYER_ABILITIES, "02"),
            Ok(PlayRequest::PlayerAbilities {
                flags: ABILITY_FLAG_FLYING
            })
        );
    }

    #[test]
    fn decodes_punch_and_rejects_a_body() {
        assert_eq!(decode_body(serverbound::PUNCH, ""), Ok(PlayRequest::Punch));
        assert_eq!(
            decode_body(serverbound::PUNCH, "00"),
            Err(DecodeError::TrailingBytes(1))
        );
    }

    #[test]
    fn decodes_set_carried_item() {
        assert_eq!(
            decode_body(serverbound::SET_CARRIED_ITEM, "00 04"),
            Ok(PlayRequest::SetCarriedItem { slot: 4 })
        );
        assert_eq!(
            decode_body(serverbound::SET_CARRIED_ITEM, "00 03"),
            Ok(PlayRequest::SetCarriedItem { slot: 3 })
        );
        assert_eq!(
            decode_body(serverbound::SET_CARRIED_ITEM, "ff ff"),
            Ok(PlayRequest::SetCarriedItem { slot: -1 })
        );
    }

    #[test]
    fn decodes_creative_mode_slot_examples() {
        let id = serverbound::SET_CREATIVE_MODE_SLOT;

        assert_eq!(
            decode_body(id, "00 24 40 01 00 00"),
            Ok(PlayRequest::SetCreativeModeSlot {
                slot: 36,
                stack: ItemStack::new(1, 64),
            })
        );
        assert_eq!(
            decode_body(id, "00 24 01 01 00 00"),
            Ok(PlayRequest::SetCreativeModeSlot {
                slot: 36,
                stack: ItemStack::new(1, 1),
            })
        );
        assert_eq!(
            decode_body(id, "00 24 00"),
            Ok(PlayRequest::SetCreativeModeSlot {
                slot: 36,
                stack: ItemStack::empty(),
            })
        );
        assert_eq!(
            decode_body(id, "ff ff 01 01 00 00"),
            Ok(PlayRequest::SetCreativeModeSlot {
                slot: -1,
                stack: ItemStack::new(1, 1),
            })
        );
        assert_eq!(
            decode_body(id, "00 25 02 01 01 01 01 01 10 0c"),
            Ok(PlayRequest::SetCreativeModeSlot {
                slot: 37,
                stack: ItemStack {
                    item: 1,
                    count: 2,
                    added: vec![(1, vec![0x10])],
                    removed: vec![12],
                },
            })
        );
    }

    #[test]
    fn a_malformed_creative_item_is_ignored_not_fatal() {
        let id = serverbound::SET_CREATIVE_MODE_SLOT;
        let ignored = Ok(PlayRequest::Other(id));

        // 数量超过 99。
        assert_eq!(decode_body(id, "00 24 64 01 00 00"), ignored);
        // 负数物品 ID。
        assert_eq!(decode_body(id, "00 24 01 ff ff ff ff 0f 00 00"), ignored);
        // 没有注册的物品 ID。
        assert_eq!(decode_body(id, "00 24 01 c0 84 3d 00 00"), ignored);
        // 组件的值比帧还长。
        assert_eq!(decode_body(id, "00 24 01 01 01 00 01 05 00"), ignored);
        // 物品读到一半就没了。
        assert_eq!(decode_body(id, "00 24 01"), ignored);
    }

    #[test]
    fn a_creative_slot_without_its_slot_number_is_still_malformed() {
        assert_eq!(
            decode_body(serverbound::SET_CREATIVE_MODE_SLOT, "00"),
            Err(DecodeError::UnexpectedEnd)
        );
    }

    #[test]
    fn creative_slot_does_not_call_finish() {
        assert_eq!(
            decode_body(serverbound::SET_CREATIVE_MODE_SLOT, "00 24 00 ff ff"),
            Ok(PlayRequest::SetCreativeModeSlot {
                slot: 36,
                stack: ItemStack::empty(),
            })
        );
    }

    #[test]
    fn decodes_container_click() {
        assert_eq!(
            decode_body(serverbound::CONTAINER_CLICK, "01 02 00 24 00 00 00 00"),
            Ok(PlayRequest::ContainerClick {
                container_id: 1,
                state_id: 2,
                slot: 36,
                button: 0,
                input: ContainerInput::Pickup,
                changed_slots: BTreeMap::new(),
                carried: HashedStack::Empty,
            })
        );
    }

    #[test]
    fn container_click_keeps_every_hash_and_wraps_an_unknown_input() {
        assert_eq!(
            decode_body(
                serverbound::CONTAINER_CLICK,
                "01 02 ff ff 01 05
                 01 00 24 01 01 02 01 01 11 22 33 44 01 0c
                 01 03 04 00 00"
            ),
            Ok(PlayRequest::ContainerClick {
                container_id: 1,
                state_id: 2,
                slot: -1,
                button: 1,
                input: ContainerInput::QuickCraft,
                changed_slots: BTreeMap::from([(
                    36,
                    HashedStack::Item(HashedItem {
                        item: 1,
                        count: 2,
                        added: vec![(1, 0x1122_3344)],
                        removed: vec![12],
                    })
                )]),
                carried: HashedStack::Item(HashedItem {
                    item: 3,
                    count: 4,
                    added: Vec::new(),
                    removed: Vec::new(),
                }),
            })
        );

        assert_eq!(
            decode_body(serverbound::CONTAINER_CLICK, "01 02 00 24 00 07 00 00"),
            Ok(PlayRequest::ContainerClick {
                container_id: 1,
                state_id: 2,
                slot: 36,
                button: 0,
                input: ContainerInput::Pickup,
                changed_slots: BTreeMap::new(),
                carried: HashedStack::Empty,
            })
        );
    }

    #[test]
    fn container_click_rejects_unknown_registry_ids_and_oversized_maps() {
        // 没有注册的物品。
        assert_eq!(
            decode_body(
                serverbound::CONTAINER_CLICK,
                "01 02 00 24 00 00 00 01 c0 84 3d 01 00 00"
            ),
            Err(DecodeError::InvalidItem(1_000_000))
        );
        // 没有注册的组件类型。
        assert_eq!(
            decode_body(
                serverbound::CONTAINER_CLICK,
                "01 02 00 24 00 00 01 00 24 01 01 01 01 ff ff ff ff 0f 00 00"
            ),
            Err(DecodeError::InvalidComponentType(-1))
        );
        // changedSlots 超过 128 项。
        assert_eq!(
            decode_body(serverbound::CONTAINER_CLICK, "01 02 00 24 00 00 81 01"),
            Err(DecodeError::InvalidCount(129))
        );
    }

    #[test]
    fn decodes_container_close() {
        assert_eq!(
            decode_body(serverbound::CONTAINER_CLOSE, "05"),
            Ok(PlayRequest::ContainerClose { container_id: 5 })
        );
        assert_eq!(
            decode_body(serverbound::CONTAINER_BUTTON_CLICK, "01 64"),
            Ok(PlayRequest::ContainerButtonClick {
                container_id: 1,
                button_id: 100,
            })
        );
    }

    #[test]
    fn decodes_player_action() {
        assert_eq!(
            decode_body(
                serverbound::PLAYER_ACTION,
                "00  00 00 02 00 00 00 8f c3  01  01"
            ),
            Ok(PlayRequest::PlayerAction {
                action: PlayerActionKind::StartDestroyBlock,
                pos: BlockPos::new(8, -61, 8),
                face: Direction::Up,
                sequence: 1,
            })
        );
    }

    #[test]
    fn player_action_ordinals_match_the_game() {
        let kinds = [
            PlayerActionKind::StartDestroyBlock,
            PlayerActionKind::ChangeDestroyDirection,
            PlayerActionKind::AbortDestroyBlock,
            PlayerActionKind::StopDestroyBlock,
            PlayerActionKind::DropAllItems,
            PlayerActionKind::DropItem,
            PlayerActionKind::ReleaseUseItem,
            PlayerActionKind::SwapItemWithOffhand,
            PlayerActionKind::Stab,
        ];
        for (id, action) in kinds.into_iter().enumerate() {
            assert_eq!(by_ordinal(&PlayerActionKind::ALL, id as i32), Ok(action));
        }
    }

    #[test]
    fn negative_ordinals_are_rejected() {
        assert_eq!(
            by_ordinal(&PlayerActionKind::ALL, -1),
            Err(DecodeError::InvalidEnum(-1))
        );
    }

    #[test]
    fn player_action_rejects_an_unknown_action_but_wraps_the_face() {
        assert_eq!(
            decode_body(
                serverbound::PLAYER_ACTION,
                "09  00 00 02 00 00 00 8f c3  01  01"
            ),
            Err(DecodeError::InvalidEnum(9))
        );
        assert_eq!(
            decode_body(
                serverbound::PLAYER_ACTION,
                "07  00 00 00 00 00 00 00 00  07  00"
            ),
            Ok(PlayRequest::PlayerAction {
                action: PlayerActionKind::SwapItemWithOffhand,
                pos: BlockPos::new(0, 0, 0),
                face: Direction::Up,
                sequence: 0,
            })
        );
    }

    #[test]
    fn player_action_reads_the_face_as_an_unsigned_byte() {
        // 按 VarInt 读的话 `ff` 是续位，sequence 也会读错。
        assert_eq!(
            decode_body(
                serverbound::PLAYER_ACTION,
                "00  00 00 00 00 00 00 00 00  ff  05"
            ),
            Ok(PlayRequest::PlayerAction {
                action: PlayerActionKind::StartDestroyBlock,
                pos: BlockPos::new(0, 0, 0),
                face: Direction::South,
                sequence: 5,
            })
        );
    }

    #[test]
    fn decodes_use_item_on() {
        assert_eq!(
            decode_body(
                serverbound::USE_ITEM_ON,
                "00
                 00 00 02 00 00 00 8f c3
                 01
                 3f 00 00 00
                 3f 80 00 00
                 3f 00 00 00
                 00 00
                 02"
            ),
            Ok(PlayRequest::UseItemOn {
                hand: Hand::Main,
                pos: BlockPos::new(8, -61, 8),
                face: Direction::Up,
                cursor: (0.5, 1.0, 0.5),
                inside: false,
                world_border_hit: false,
                sequence: 2,
            })
        );
    }

    #[test]
    fn use_item_on_wraps_the_face_and_defaults_the_hand() {
        let decoded = decode_body(
            serverbound::USE_ITEM_ON,
            "07
             00 00 02 00 00 00 8f c3
             07
             3f 00 00 00
             3f 80 00 00
             3f 00 00 00
             01 01
             02",
        );
        assert_eq!(
            decoded,
            Ok(PlayRequest::UseItemOn {
                hand: Hand::Main,
                pos: BlockPos::new(8, -61, 8),
                face: Direction::Up,
                cursor: (0.5, 1.0, 0.5),
                inside: true,
                world_border_hit: true,
                sequence: 2,
            })
        );
    }

    #[test]
    fn decodes_use_item() {
        assert_eq!(
            decode_body(serverbound::USE_ITEM, "01 03 42 b4 00 00 c1 78 00 00"),
            Ok(PlayRequest::UseItem {
                hand: Hand::Off,
                sequence: 3,
                yaw: 90.0,
                pitch: -15.5,
            })
        );
    }

    #[test]
    fn decodes_pick_item_from_block() {
        assert_eq!(
            decode_body(
                serverbound::PICK_ITEM_FROM_BLOCK,
                "00 00 02 00 00 00 8f c3 00"
            ),
            Ok(PlayRequest::PickItemFromBlock {
                pos: BlockPos::new(8, -61, 8),
                include_data: false,
            })
        );
    }

    #[test]
    fn decodes_sign_update_with_four_lines_then_the_slot() {
        assert_eq!(
            decode_body(
                serverbound::SIGN_UPDATE,
                "00 00 02 00 00 00 8f c3
                 02 68 69
                 00
                 01 61
                 00
                 01"
            ),
            Ok(PlayRequest::SignUpdate {
                pos: BlockPos::new(8, -61, 8),
                lines: [
                    "hi".to_owned(),
                    String::new(),
                    "a".to_owned(),
                    String::new()
                ],
                slot: SignTextSlot::Front,
            })
        );
    }

    #[test]
    fn sign_update_defaults_an_unknown_slot_to_the_back() {
        let decoded = decode_body(
            serverbound::SIGN_UPDATE,
            "00 00 02 00 00 00 8f c3
             00
             00
             00
             00
             07",
        );
        assert_eq!(
            decoded,
            Ok(PlayRequest::SignUpdate {
                pos: BlockPos::new(8, -61, 8),
                lines: [String::new(), String::new(), String::new(), String::new()],
                slot: SignTextSlot::Back,
            })
        );
    }

    #[test]
    fn sign_update_rejects_a_line_longer_than_the_limit_and_short_bodies() {
        let mut payload = vec![serverbound::SIGN_UPDATE as u8];
        payload.extend(hex("00 00 02 00 00 00 8f c3"));
        write_string(&mut payload, &"竹".repeat(MAX_LINE_CHARS));
        payload.extend(hex("00 00 00 00"));
        assert!(matches!(
            PlayRequest::decode(&payload),
            Ok(PlayRequest::SignUpdate { .. })
        ));

        let mut payload = vec![serverbound::SIGN_UPDATE as u8];
        payload.extend(hex("00 00 02 00 00 00 8f c3"));
        write_string(&mut payload, &"竹".repeat(MAX_LINE_CHARS + 1));
        payload.extend(hex("00 00 00 00"));
        assert_eq!(
            PlayRequest::decode(&payload),
            Err(DecodeError::StringTooLong {
                max_chars: MAX_LINE_CHARS
            })
        );

        // 少了第四行和那一面。
        assert_eq!(
            decode_body(
                serverbound::SIGN_UPDATE,
                "00 00 02 00 00 00 8f c3  00  00  00"
            ),
            Err(DecodeError::UnexpectedEnd)
        );
    }

    fn edit_book_bytes(slot: i32, pages: &[String], title: Option<&str>) -> Vec<u8> {
        let mut payload = Vec::new();
        write_varint(&mut payload, serverbound::EDIT_BOOK);
        write_varint(&mut payload, slot);
        write_varint(&mut payload, pages.len() as i32);
        for page in pages {
            write_string(&mut payload, page);
        }
        match title {
            Some(title) => {
                payload.push(1);
                write_string(&mut payload, title);
            }
            None => payload.push(0),
        }
        payload
    }

    #[test]
    fn decodes_edit_book_pages_and_an_optional_title() {
        let pages = ["一".to_owned(), String::new(), "c".to_owned()];
        assert_eq!(
            PlayRequest::decode(&edit_book_bytes(4, &pages, None)).unwrap(),
            PlayRequest::EditBook {
                slot: 4,
                pages: pages.to_vec(),
                title: None,
            }
        );
        assert_eq!(
            PlayRequest::decode(&edit_book_bytes(40, &pages, Some(" 标题 "))).unwrap(),
            PlayRequest::EditBook {
                slot: 40,
                pages: pages.to_vec(),
                title: Some(" 标题 ".to_owned()),
            }
        );
        // 槽位合不合法是处理时的事，解码照单全收，和官方一样。
        assert_eq!(
            PlayRequest::decode(&edit_book_bytes(-1, &[], None)).unwrap(),
            PlayRequest::EditBook {
                slot: -1,
                pages: Vec::new(),
                title: None,
            }
        );
    }

    #[test]
    fn edit_book_rejects_too_many_pages_or_too_long_text() {
        let hundred = vec![String::new(); MAX_PAGES];
        assert!(PlayRequest::decode(&edit_book_bytes(0, &hundred, None)).is_ok());

        let mut too_many = Vec::new();
        write_varint(&mut too_many, serverbound::EDIT_BOOK);
        write_varint(&mut too_many, 0);
        write_varint(&mut too_many, (MAX_PAGES + 1) as i32);
        assert_eq!(
            PlayRequest::decode(&too_many),
            Err(DecodeError::InvalidCount((MAX_PAGES + 1) as i32))
        );

        let page = "竹".repeat(PAGE_EDIT_LENGTH);
        assert!(PlayRequest::decode(&edit_book_bytes(0, &[page], None)).is_ok());
        let too_long = "竹".repeat(PAGE_EDIT_LENGTH + 1);
        assert_eq!(
            PlayRequest::decode(&edit_book_bytes(0, std::slice::from_ref(&too_long), None)),
            Err(DecodeError::StringTooLong {
                max_chars: PAGE_EDIT_LENGTH
            })
        );

        let title = "a".repeat(TITLE_MAX_LENGTH);
        assert!(PlayRequest::decode(&edit_book_bytes(0, &[], Some(&title))).is_ok());
        let title = "a".repeat(TITLE_MAX_LENGTH + 1);
        assert_eq!(
            PlayRequest::decode(&edit_book_bytes(0, &[], Some(&title))),
            Err(DecodeError::StringTooLong {
                max_chars: TITLE_MAX_LENGTH
            })
        );
    }

    #[test]
    fn edit_book_rejects_trailing_bytes() {
        let mut payload = edit_book_bytes(0, &["hi".to_owned()], None);
        payload.push(0);
        assert_eq!(
            PlayRequest::decode(&payload),
            Err(DecodeError::TrailingBytes(1))
        );
    }

    #[test]
    fn decoded_packets_reject_trailing_bytes() {
        assert_eq!(
            decode_body(serverbound::KEEP_ALIVE, "00 00 00 00 00 00 00 01 00"),
            Err(DecodeError::TrailingBytes(1))
        );
        assert_eq!(
            decode_body(serverbound::MOVE_PLAYER_STATUS_ONLY, "01 00"),
            Err(DecodeError::TrailingBytes(1))
        );
    }

    #[test]
    fn unknown_and_undecoded_packets_are_not_errors() {
        assert_eq!(
            decode_body(serverbound::CLIENT_TICK_END, ""),
            Ok(PlayRequest::Other(serverbound::CLIENT_TICK_END))
        );
        // 不调用 finish，所以内容是什么都行。
        assert_eq!(
            decode_body(serverbound::CHAT_ACK, "ff ff ff"),
            Ok(PlayRequest::Other(serverbound::CHAT_ACK))
        );
        assert_eq!(decode_body(0x7f, "01 02 03"), Ok(PlayRequest::Other(0x7f)));
    }

    #[test]
    fn spec_frames_of_undecoded_packets_are_not_errors() {
        assert_eq!(
            decode_frame("01 2c"),
            PlayRequest::Other(serverbound::PLAYER_LOADED)
        );
        assert_eq!(
            decode_frame("01 0d"),
            PlayRequest::Other(serverbound::CLIENT_TICK_END)
        );
        assert_eq!(
            decode_body(
                serverbound::CHAT_COMMAND_SIGNED,
                "06 73 61 79 20 68 69
                 00 00 01 8b cf e5 68 00
                 11 22 33 44 55 66 77 88
                 00
                 00 00 00 00 01"
            ),
            Ok(PlayRequest::Other(serverbound::CHAT_COMMAND_SIGNED))
        );
    }
}
