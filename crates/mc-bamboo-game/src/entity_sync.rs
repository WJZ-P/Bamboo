//! 玩家互相可见：配对、持续同步、元数据和装备。
//!
//! 同步状态每名玩家一份，所有观察者共用。新观察者从上次发出去的状态开始，
//! 下一个位移才接得上，同一个包也只要编码一次。

use std::{collections::BTreeSet, mem};

use mc_bamboo_core::{Hand, Vec3};
use mc_bamboo_data::registries::entity_type;
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{
        ABILITY_FLAG_FLYING, AddEntity, BundleDelimiter, ClientInformation,
        ENTITY_FLAG_FALL_FLYING, ENTITY_FLAG_SHIFT_KEY_DOWN, ENTITY_FLAG_SPRINTING, EntityData,
        EntityPositionSync, EquipmentSlot, GameMode, HumanoidArm, INPUT_FLAG_SHIFT, MoveEntityPos,
        MoveEntityPosRot, MoveEntityRot, PlayerCommandKind, Pose, PositionDelta, PositionPath,
        RemoveEntities, RotateHead, SetEntityData, SetEquipment, SwingAnimation, VecDelta,
        pack_degrees,
    },
};

use crate::{
    inbound::PlayerId,
    links::{Links, encode_frames},
    player::Player,
    region::Region,
};

/// 玩家的 `updateInterval`：每 2 tick 一个周期。
const UPDATE_INTERVAL: u64 = 2;
/// 静止的玩家也每隔这么多 tick 发一次位置。
const FORCED_POSITION_PERIOD: u64 = 60;
/// 超过这么多个周期没有全量同步时发一次。
const FORCED_SYNC_CYCLES: u32 = 400;
/// 2⁻¹⁷。和位移的平方比较，所以实际阈值约 0.00276 格。
const POSITION_TOLERANCE: f64 = 1.0 / 131_072.0;
/// 玩家的 `clientTrackingRange` 是 32 区块。
const MAX_TRACKING_RANGE: i32 = 512;
const BLOCKS_PER_CHUNK: i32 = 16;
/// M2 没有伤害，血量一直是满的。
const PLAYER_HEALTH: f32 = 20.0;

/// 玩家有的装备槽，按 ordinal 排序。
const EQUIPMENT_SLOTS: [EquipmentSlot; 6] = [
    EquipmentSlot::MainHand,
    EquipmentSlot::OffHand,
    EquipmentSlot::Feet,
    EquipmentSlot::Legs,
    EquipmentSlot::Chest,
    EquipmentSlot::Head,
];

/// 上次发出去的装备。官方 `LivingEntity.equipmentHasChanged` 用 `ItemStack.matches` 比，
/// 所以物品、数量和组件任何一样变了都要重发。
const NO_EQUIPMENT: ItemStack = ItemStack::empty();

/// 玩家元数据里服务端维护的部分。其余 index 一直是默认值，不用发。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Metadata {
    shared_flags: u8,
    pose: Pose,
    health: f32,
    main_hand: HumanoidArm,
    model_customisation: u8,
}

impl Metadata {
    /// 官方定义的默认值。血量的默认值是 1，玩家的构造函数才设成 20，所以配对时总要发。
    const DEFAULT: Self = Self {
        shared_flags: 0,
        pose: Pose::Standing,
        health: 1.0,
        main_hand: HumanoidArm::Right,
        model_customisation: 0,
    };

    fn of(player: &Player) -> Self {
        Self {
            shared_flags: player.sync.shared_flags,
            pose: player.sync.pose(),
            health: PLAYER_HEALTH,
            main_hand: player.client.main_hand,
            model_customisation: player.client.model_customisation,
        }
    }

    /// 和 `previous` 不同的项，index 升序。
    fn changes_since(self, previous: Self) -> Vec<EntityData> {
        let flags = self.shared_flags != previous.shared_flags;
        let pose = self.pose != previous.pose;
        let health = self.health != previous.health;
        let main_hand = self.main_hand != previous.main_hand;
        let model = self.model_customisation != previous.model_customisation;
        [
            flags.then_some(EntityData::SharedFlags(self.shared_flags)),
            pose.then_some(EntityData::Pose(self.pose)),
            health.then_some(EntityData::Health(self.health)),
            main_hand.then_some(EntityData::MainHand(self.main_hand)),
            model.then_some(EntityData::ModelCustomisation(self.model_customisation)),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// 一名玩家的同步状态。`entities.md` 7.1。
#[derive(Debug)]
pub(crate) struct EntitySync {
    /// 元数据 index 0 的标志字节。`entities.md` 第 10 节。
    pub(crate) shared_flags: u8,
    /// 元数据变了：这一 tick 进入周期，把变化发给观察者和玩家自己。
    pub(crate) metadata_dirty: bool,
    /// 按着潜行键。来自 `player_input`。
    pub(crate) shift_key_down: bool,
    /// 正在飞行。来自 `player_abilities`。
    pub(crate) flying: bool,
    /// 上次发出去的位置，客户端按它解码位移。
    base: Vec3,
    /// 上次发出去的角度，都是 [`pack_degrees`] 的结果。
    last_yaw: u8,
    last_pitch: u8,
    last_head: u8,
    was_on_ground: bool,
    tick_count: u64,
    /// 距上次全量同步的周期数。
    teleport_delay: u32,
    /// 距上个周期的 tick 数。从 -1 开始，所以第一个周期是 0。
    ticks_since_cycle: i32,
    /// 上次发出去的元数据。
    sent_metadata: Metadata,
    /// 上次比较时的装备，和 [`EQUIPMENT_SLOTS`] 一一对应。
    sent_equipment: [ItemStack; EQUIPMENT_SLOTS.len()],
    /// 看得见这名玩家的人。它们的客户端里有这个实体。
    seen_by: BTreeSet<PlayerId>,
}

impl EntitySync {
    /// 玩家进入世界时调用，这时朝向是 0，人不在地上。
    ///
    /// 进服第 6 步把 `client` 里的皮肤外层和主手发给了玩家自己，所以这两项算已经发过。
    pub(crate) fn new(position: Vec3, client: &ClientInformation) -> Self {
        Self {
            shared_flags: 0,
            metadata_dirty: false,
            shift_key_down: false,
            flying: false,
            base: position,
            last_yaw: 0,
            last_pitch: 0,
            last_head: 0,
            was_on_ground: false,
            tick_count: 0,
            teleport_delay: 0,
            ticks_since_cycle: -1,
            sent_metadata: Metadata {
                health: PLAYER_HEALTH,
                main_hand: client.main_hand,
                model_customisation: client.model_customisation,
                ..Metadata::DEFAULT
            },
            sent_equipment: [const { NO_EQUIPMENT }; EQUIPMENT_SLOTS.len()],
            seen_by: BTreeSet::new(),
        }
    }

    /// 蹲着：按着潜行键并且没在飞。创造模式飞行时按潜行键是下降，姿势不变。
    /// 放置方块时用它决定眼高和碰撞盒。
    pub(crate) fn is_crouching(&self) -> bool {
        self.shift_key_down && !self.flying
    }

    /// 其他玩家的姿势完全由 index 6 决定，客户端不会根据潜行位自己推算。
    fn pose(&self) -> Pose {
        if self.is_crouching() {
            Pose::Crouching
        } else {
            Pose::Standing
        }
    }

    fn set_shared_flag(&mut self, flag: u8, enabled: bool) {
        if enabled {
            self.shared_flags |= flag;
        } else {
            self.shared_flags &= !flag;
        }
    }

    /// 把同一份字节发给每个观察者。
    fn send_to_viewers(&self, links: &mut Links, frames: &[u8]) {
        for &viewer in &self.seen_by {
            links.send_frames(viewer, frames);
        }
    }
}

/// tick 第 3 阶段：`head_yaw`、姿势、元数据脏标记。`entities.md` 第 13 节。
pub(crate) fn update_derived_state(region: &mut Region) {
    for player in region.players.values_mut() {
        player.head_yaw = player.yaw;
        player.sync.metadata_dirty |= Metadata::of(player) != player.sync.sent_metadata;
    }
}

/// tick 第 6 阶段：`visible` 配对和解除，然后每名玩家 `send_changes`。
/// `entities.md` 第 4 到 7 节。
pub(crate) fn sync_entities(region: &mut Region) {
    update_pairings(region);
    for player in region.players.values_mut() {
        send_changes(player, &mut region.links);
    }
}

/// 一名玩家这一 tick 的观察者变化。
struct ViewerChanges {
    target: PlayerId,
    gained: Vec<PlayerId>,
    lost: Vec<PlayerId>,
}

fn update_pairings(region: &mut Region) {
    let changes: Vec<ViewerChanges> = region
        .players
        .values()
        .filter_map(|target| viewer_changes(region, target))
        .collect();
    for change in changes {
        apply_viewer_changes(region, change);
    }
}

fn viewer_changes(region: &Region, target: &Player) -> Option<ViewerChanges> {
    let mut gained = Vec::new();
    let mut lost = Vec::new();
    for viewer in region.players.values() {
        let visible = is_visible(viewer, target, region.view_distance);
        match (visible, target.sync.seen_by.contains(&viewer.id)) {
            (true, false) => gained.push(viewer.id),
            (false, true) => lost.push(viewer.id),
            _ => {}
        }
    }
    (!gained.is_empty() || !lost.is_empty()).then_some(ViewerChanges {
        target: target.id,
        gained,
        lost,
    })
}

/// `viewer` 的客户端现在应该有 `target` 这个实体。没有滞后区间，条件一不成立就移除。
/// 范围看观察者的视距，所以不对称：A 看得到 B 时 B 不一定看得到 A。
///
/// 官方还要看旁观模式。M2 全员创造模式，这一项总是成立。
fn is_visible(viewer: &Player, target: &Player, server_view_distance: i32) -> bool {
    if viewer.id == target.id {
        return false;
    }
    let range = visible_range(viewer.view_distance(server_view_distance));
    let dx = viewer.position().x - target.position().x;
    let dz = viewer.position().z - target.position().z;
    // 区块还没发到时不配对：`add_entity` 到达时区块要已经加载了。
    dx * dx + dz * dz <= range * range && viewer.chunks.is_chunk_tracked(target.chunk_pos())
}

/// 观察者看得见玩家的水平距离，单位是格。
fn visible_range(view_distance: i32) -> f64 {
    f64::from((view_distance * BLOCKS_PER_CHUNK).min(MAX_TRACKING_RANGE))
}

fn apply_viewer_changes(region: &mut Region, change: ViewerChanges) {
    let Some(target) = region.players.get_mut(&change.target) else {
        return;
    };
    if !change.lost.is_empty() {
        let frames = encode_frames(&RemoveEntities {
            entity_ids: &[target.id.0],
        });
        for viewer in change.lost {
            target.sync.seen_by.remove(&viewer);
            region.links.send_frames(viewer, &frames);
        }
    }
    if !change.gained.is_empty() {
        let frames = pairing_frames(target);
        for viewer in change.gained {
            target.sync.seen_by.insert(viewer);
            region.links.send_frames(viewer, &frames);
        }
    }
}

/// 把一名玩家介绍给新观察者的 bundle。`entities.md` 第 5 节。
///
/// 位置、角度和元数据都取上次发出去的状态，还没发的变化下一个周期会发给包括它在内的所有观察者。
/// `update_attributes` 配对时不发（D5）。两个分隔符要和内容一起写进同一条队列，
/// 漏掉第二个的话后面的包都会被客户端吞掉，所以这里返回一整段字节。
fn pairing_frames(player: &Player) -> Vec<u8> {
    let sync = &player.sync;
    let mut frames = encode_frames(&BundleDelimiter);
    frames.extend(encode_frames(&AddEntity {
        entity_id: player.id.0,
        uuid: player.uuid,
        entity_type: entity_type::PLAYER,
        position: sync.base,
        movement: Vec3::default(),
        pitch: sync.last_pitch,
        yaw: sync.last_yaw,
        head_yaw: sync.last_head,
        data: 0,
    }));
    frames.extend(encode_frames(&SetEntityData {
        entity_id: player.id.0,
        items: &sync.sent_metadata.changes_since(Metadata::DEFAULT),
    }));
    let equipment = worn_equipment(player);
    if let Some(packet) = SetEquipment::new(player.id.0, &equipment) {
        frames.extend(encode_frames(&packet));
    }
    frames.extend(encode_frames(&BundleDelimiter));
    frames
}

/// 全部非空的装备槽。
fn worn_equipment(player: &Player) -> Vec<(EquipmentSlot, &ItemStack)> {
    EQUIPMENT_SLOTS
        .into_iter()
        .filter_map(|slot| Some((slot, player.inventory.equipment(slot)?)))
        .filter(|(_, stack)| !stack.is_empty())
        .collect()
}

/// 每名玩家每 tick 调用一次。`entities.md` 7.2。
fn send_changes(player: &mut Player, links: &mut Links) {
    player.sync.ticks_since_cycle += 1;
    if player.sync.tick_count.is_multiple_of(UPDATE_INTERVAL) || player.sync.metadata_dirty {
        run_cycle(player, links);
    }
    player.sync.tick_count += 1;
}

/// 一个周期里包的顺序是固定的：移动包、`set_entity_data`、`rotate_head`。
fn run_cycle(player: &mut Player, links: &mut Links) {
    let yaw = pack_degrees(player.yaw);
    let pitch = pack_degrees(player.pitch);
    let ticks = mem::take(&mut player.sync.ticks_since_cycle);
    player.sync.teleport_delay += 1;

    let movement = choose_movement(player, yaw, pitch);
    let mut frames = movement
        .map(|movement| movement.frames(player, ticks, yaw, pitch))
        .unwrap_or_default();
    let metadata = take_metadata_frames(player);
    frames.extend_from_slice(&metadata);

    if let Some(movement) = movement {
        if movement.has_position() {
            player.sync.base = player.position();
        }
        if movement.has_rotation() {
            player.sync.last_yaw = yaw;
            player.sync.last_pitch = pitch;
        }
    }

    let head_yaw = pack_degrees(player.head_yaw);
    if head_yaw != player.sync.last_head {
        player.sync.last_head = head_yaw;
        frames.extend(encode_frames(&RotateHead {
            entity_id: player.id.0,
            head_yaw,
        }));
    }

    player.sync.send_to_viewers(links, &frames);
    // 皮肤外层和主手在客户端只由这个包写入，所以玩家自己也要收到。
    links.send_frames(player.id, &metadata);
}

/// 这个周期要发的移动包。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Movement {
    /// `entity_position_sync`。
    Sync,
    /// `move_entity_pos`。
    Position(PositionDelta),
    /// `move_entity_pos_rot`。
    PositionRotation(PositionDelta),
    /// `move_entity_rot`。
    Rotation,
}

impl Movement {
    fn has_position(self) -> bool {
        !matches!(self, Self::Rotation)
    }

    fn has_rotation(self) -> bool {
        !matches!(self, Self::Position(_))
    }

    /// `ticks` 是距上个周期的 tick 数。`yaw` 和 `pitch` 是打包后的角度。
    fn frames(self, player: &Player, ticks: i32, yaw: u8, pitch: u8) -> Vec<u8> {
        let entity_id = player.id.0;
        let on_ground = player.on_ground;
        match self {
            Self::Sync => encode_frames(&EntityPositionSync {
                entity_id,
                path: position_path(player.position(), ticks),
                yaw: player.yaw,
                pitch: player.pitch,
                on_ground,
            }),
            Self::Position(delta) => encode_frames(&MoveEntityPos {
                entity_id,
                delta: vec_delta(delta, ticks),
                on_ground,
            }),
            Self::PositionRotation(delta) => encode_frames(&MoveEntityPosRot {
                entity_id,
                delta: vec_delta(delta, ticks),
                yaw,
                pitch,
                on_ground,
            }),
            Self::Rotation => encode_frames(&MoveEntityRot {
                entity_id,
                yaw,
                pitch,
                on_ground,
            }),
        }
    }
}

/// 官方的服务端玩家用 `SteppedInterpolationTracker`，所以除了第一个周期都是 1 步的 Stepped。
fn vec_delta(delta: PositionDelta, ticks: i32) -> VecDelta {
    if ticks > 0 {
        VecDelta::Stepped { ticks, delta }
    } else {
        VecDelta::Linear(delta)
    }
}

fn position_path(position: Vec3, ticks: i32) -> PositionPath {
    if ticks > 0 {
        PositionPath::Stepped { position, ticks }
    } else {
        PositionPath::Linear(position)
    }
}

fn choose_movement(player: &mut Player, yaw: u8, pitch: u8) -> Option<Movement> {
    let position = player.position();
    let sync = &mut player.sync;
    if sync.teleport_delay > FORCED_SYNC_CYCLES || sync.was_on_ground != player.on_ground {
        sync.was_on_ground = player.on_ground;
        sync.teleport_delay = 0;
        return Some(Movement::Sync);
    }

    let send_rotation = yaw != sync.last_yaw || pitch != sync.last_pitch;
    let send_position =
        has_moved(sync.base, position) || sync.tick_count.is_multiple_of(FORCED_POSITION_PERIOD);
    if !send_position {
        return send_rotation.then_some(Movement::Rotation);
    }
    // 位移放不进 Short 时也是全量同步，但和上面不同，这里不重置 `teleport_delay`。
    Some(match PositionDelta::between(sync.base, position) {
        None => Movement::Sync,
        Some(delta) if send_rotation => Movement::PositionRotation(delta),
        Some(delta) => Movement::Position(delta),
    })
}

fn has_moved(base: Vec3, position: Vec3) -> bool {
    let dx = position.x - base.x;
    let dy = position.y - base.y;
    let dz = position.z - base.z;
    dx * dx + dy * dy + dz * dz >= POSITION_TOLERANCE
}

/// 取走这个周期要发的元数据：只有变了的项。什么都没变时是空的。
fn take_metadata_frames(player: &mut Player) -> Vec<u8> {
    if !mem::take(&mut player.sync.metadata_dirty) {
        return Vec::new();
    }
    let current = Metadata::of(player);
    let items = current.changes_since(player.sync.sent_metadata);
    player.sync.sent_metadata = current;
    if items.is_empty() {
        return Vec::new();
    }
    encode_frames(&SetEntityData {
        entity_id: player.id.0,
        items: &items,
    })
}

/// tick 第 7 阶段：比较装备，发 `set_equipment`。`entities.md` 第 11 节。
pub(crate) fn send_equipment_changes(region: &mut Region) {
    for player in region.players.values_mut() {
        send_equipment_change(player, &mut region.links);
    }
}

/// 只发变了的槽，只发给观察者：玩家自己的物品栏是客户端说了算。
fn send_equipment_change(player: &mut Player, links: &mut Links) {
    let mut changed = Vec::new();
    for (slot, sent) in EQUIPMENT_SLOTS
        .into_iter()
        .zip(&mut player.sync.sent_equipment)
    {
        let Some(stack) = player.inventory.equipment(slot) else {
            continue;
        };
        if !sent.matches(stack) {
            *sent = stack.clone();
            changed.push((slot, stack));
        }
    }
    if let Some(packet) = SetEquipment::new(player.id.0, &changed) {
        player.sync.send_to_viewers(links, &encode_frames(&packet));
    }
}

/// 离开第 3、4 步：给这名玩家的 `seen_by` 发 `remove_entities`，再把它从所有人的
/// `seen_by` 里删掉。这时玩家还在 `region.players` 里。`coverage.md` 4.2，`entities.md` 第 6 节。
pub(crate) fn on_leave(region: &mut Region, id: PlayerId) {
    if let Some(player) = region.players.get(&id) {
        let frames = encode_frames(&RemoveEntities {
            entity_ids: &[id.0],
        });
        player.sync.send_to_viewers(&mut region.links, &frames);
    }
    for player in region.players.values_mut() {
        player.sync.seen_by.remove(&id);
    }
}

/// 给看得见这名玩家的人发一次 `swing_animation`，不发给玩家自己。`entities.md` 第 9 节。
/// 右键方块成功时 `interaction` 也调用它。
pub(crate) fn broadcast_swing(region: &mut Region, id: PlayerId, hand: Hand) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let frames = encode_frames(&SwingAnimation::default_swing(id.0, hand));
    player.sync.send_to_viewers(&mut region.links, &frames);
}

/// 客户端没有预测的挥手，也就是 `SwingSource.SERVER_ONLY`：玩家自己也要收到。
pub(crate) fn broadcast_swing_with_self(region: &mut Region, id: PlayerId, hand: Hand) {
    broadcast_swing(region, id, hand);
    let frames = encode_frames(&SwingAnimation::default_swing(id.0, hand));
    region.links.send_frames(id, &frames);
}

/// `punch`：给观察者发 `swing_animation`。`entities.md` 第 9 节。
pub(crate) fn on_punch(region: &mut Region, id: PlayerId) {
    broadcast_swing(region, id, Hand::Main);
}

/// `player_input`：潜行。姿势由派生状态算出来，和标志位落在同一个 `set_entity_data` 里。
/// `entities.md` 第 10 节。
pub(crate) fn on_player_input(region: &mut Region, id: PlayerId, flags: u8) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let shift = flags & INPUT_FLAG_SHIFT != 0;
    player.sync.shift_key_down = shift;
    player
        .sync
        .set_shared_flag(ENTITY_FLAG_SHIFT_KEY_DOWN, shift);
}

/// `player_abilities`：飞行状态。`entities.md` 第 10 节。
pub(crate) fn on_player_abilities(region: &mut Region, id: PlayerId, flags: u8) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    player.sync.flying = flags & ABILITY_FLAG_FLYING != 0 && may_fly(player.game_mode);
}

/// 官方 `Abilities.mayfly` 的取值。
fn may_fly(game_mode: GameMode) -> bool {
    matches!(game_mode, GameMode::Creative | GameMode::Spectator)
}

/// `player_command`：冲刺和拒绝滑翔，其余动作都忽略。`entities.md` 第 10 节。
pub(crate) fn on_player_command(region: &mut Region, id: PlayerId, action: PlayerCommandKind) {
    match action {
        PlayerCommandKind::StartSprinting => set_sprinting(region, id, true),
        PlayerCommandKind::StopSprinting => set_sprinting(region, id, false),
        PlayerCommandKind::StartFallFlying => refuse_fall_flying(region, id),
        PlayerCommandKind::StopSleeping
        | PlayerCommandKind::StartRidingJump
        | PlayerCommandKind::StopRidingJump
        | PlayerCommandKind::OpenInventory => {}
    }
}

fn set_sprinting(region: &mut Region, id: PlayerId, sprinting: bool) {
    if let Some(player) = region.players.get_mut(&id) {
        player
            .sync
            .set_shared_flag(ENTITY_FLAG_SPRINTING, sprinting);
    }
}

/// 客户端已经在本地开始滑翔了，而这个状态只有服务端能清，不回应的话玩家落地后走不了路。
/// M2 不做滑翔，所以总是发回不带滑翔位的标志字节，只发给这名玩家。
fn refuse_fall_flying(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let flags = player.sync.shared_flags & !ENTITY_FLAG_FALL_FLYING;
    region.links.send(
        id,
        &SetEntityData {
            entity_id: id.0,
            items: &[EntityData::SharedFlags(flags)],
        },
    );
}

#[cfg(test)]
mod tests;
