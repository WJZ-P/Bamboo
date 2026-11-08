//! 世界实体（下落的方块、点燃的 TNT）和玩家之间的同步。
//!
//! 对应官方的 `ChunkMap.TrackedEntity` 和 `ServerEntity`：按实体的跟踪范围配对，
//! 配对时发 `add_entity` 和元数据，之后按实体类型的更新间隔发位移、速度和元数据，
//! 实体没了就发 `remove_entities`。
//!
//! 这两种实体都没有朝向、乘客、属性、装备和拴绳，所以配对包只有 `add_entity`
//! 和 `set_entity_data` 两项。

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_blocks::entity::{Entity, EntityKind, PrimedTnt};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::entity_types;
use mc_bamboo_protocol::packet::play::{
    AddEntity, BundleDelimiter, EntityData, EntityPositionSync, MoveEntityPos, PositionDelta,
    PositionPath, RemoveEntities, SetEntityData, SetEntityMotion, VecDelta,
};
use uuid::Uuid;

use crate::{
    inbound::PlayerId,
    links::{Links, encode_frames},
    player::Player,
    region::Region,
};

/// 位移的平方小于这个数就不发位置，官方的 `7.6293945E-6F`，也就是 `2^-17`。
const POSITION_TOLERANCE: f64 = 1.0 / 131_072.0;
/// 速度的平方变化小于这个数就不发 `set_entity_motion`，官方的 `1.0E-7`。
const MOTION_TOLERANCE: f64 = 1.0e-7;
/// 静止的实体每隔这么多周期也发一次位置。
const FORCED_POS_UPDATE_PERIOD: i32 = 60;
/// 连续这么多次全量同步以后强制再来一次。
const FORCED_TELEPORT_PERIOD: u32 = 400;
/// 跟踪范围按区块算，一格区块 16 格。
const BLOCKS_PER_CHUNK: i32 = 16;

/// 一个世界里全部被跟踪的实体。
#[derive(Debug, Default)]
pub(crate) struct EntityTracking {
    tracked: BTreeMap<i32, TrackedEntity>,
}

/// 官方 `ServerEntity` 里和这两种实体有关的字段。
#[derive(Debug)]
struct TrackedEntity {
    /// `positionCodec` 的基准点，上次发给客户端的位置。
    base: Vec3,
    /// 上次发出去的速度。
    last_movement: Vec3,
    /// 距上次全量同步的周期数。
    teleport_delay: u32,
    /// 距上次发位置过了多少个周期。
    tick_count: i32,
    was_on_ground: bool,
    /// 上次发出去的引信，TNT 每 tick 都变。
    last_fuse: i32,
    /// 看得见这个实体的玩家。它们的客户端里有这个实体。
    seen_by: BTreeSet<PlayerId>,
}

impl TrackedEntity {
    /// 和 `ServerEntity` 的构造函数一样，记下实体现在的样子当作已经发过。
    fn new(snapshot: &Snapshot) -> Self {
        Self {
            base: snapshot.position,
            last_movement: snapshot.delta,
            teleport_delay: 0,
            tick_count: 0,
            was_on_ground: snapshot.on_ground,
            last_fuse: snapshot.data.fuse().unwrap_or(0),
            seen_by: BTreeSet::new(),
        }
    }

    fn send_to_viewers(&self, links: &mut Links, frames: &[u8]) {
        for &viewer in &self.seen_by {
            links.send_frames(viewer, frames);
        }
    }
}

/// 这一 tick 里实体的样子。借用检查器不允许一边读实体一边写链路，先抄出来。
#[derive(Debug, Clone, Copy)]
struct Snapshot {
    id: i32,
    entity_type: i32,
    position: Vec3,
    delta: Vec3,
    on_ground: bool,
    precise: bool,
    chunk: ChunkPos,
    data: KindData,
}

/// 配对和同步时要发的、按实体类型不同的元数据。
#[derive(Debug, Clone, Copy)]
enum KindData {
    FallingBlock { state: u32, start: BlockPos },
    Tnt { fuse: i32 },
}

impl Snapshot {
    fn of(entity: &Entity) -> Self {
        Self {
            id: entity.id,
            entity_type: entity.kind.entity_type(),
            position: entity.position,
            delta: entity.delta,
            on_ground: entity.on_ground,
            precise: entity.requires_precise_position(),
            chunk: ChunkPos::containing(entity.position.x, entity.position.z),
            data: KindData::of(entity.kind),
        }
    }

    /// `getAddEntityPacket` 的 `data`：下落的方块是它的方块状态，TNT 是 0。
    fn add_data(self) -> i32 {
        match self.data {
            KindData::FallingBlock { state, .. } => state as i32,
            KindData::Tnt { .. } => 0,
        }
    }

    fn track_deltas(self) -> bool {
        entity_types::FACTS[self.entity_type as usize].track_deltas
    }
}

impl KindData {
    fn of(kind: EntityKind) -> Self {
        match kind {
            EntityKind::FallingBlock(falling) => Self::FallingBlock {
                state: falling.state.0,
                start: falling.start_pos,
            },
            EntityKind::Tnt(tnt) => Self::Tnt { fuse: tnt.fuse },
        }
    }

    /// 同步用到的那个数字。下落的方块没有。
    fn fuse(self) -> Option<i32> {
        match self {
            Self::FallingBlock { .. } => None,
            Self::Tnt { fuse } => Some(fuse),
        }
    }

    /// 配对时发全部非默认值。下落的方块的起点永远不是默认值；TNT 的引信只在
    /// 被缩短过时不是。
    fn pairing_values(self) -> Vec<EntityData> {
        match self {
            Self::FallingBlock { start, .. } => vec![EntityData::FallingBlockStart(start)],
            Self::Tnt { fuse } if fuse != PrimedTnt::DEFAULT_FUSE => {
                vec![EntityData::TntFuse(fuse)]
            }
            Self::Tnt { .. } => Vec::new(),
        }
    }

    /// 这一周期要发的元数据；和上次发的一样时是空的。
    fn changes_since(self, last_fuse: i32) -> Vec<EntityData> {
        match self {
            // 起点生成以后就不再变。
            Self::FallingBlock { .. } => Vec::new(),
            Self::Tnt { fuse } if fuse != last_fuse => vec![EntityData::TntFuse(fuse)],
            Self::Tnt { .. } => Vec::new(),
        }
    }
}

/// tick 第 6 阶段：世界实体的配对、同步和移除。对应 `ChunkMap.tick`。
pub(crate) fn sync(region: &mut Region) {
    let snapshots: Vec<Snapshot> = region.entities.iter().map(Snapshot::of).collect();
    let live: BTreeSet<i32> = snapshots.iter().map(|snapshot| snapshot.id).collect();
    remove_missing(region, &live);

    let view_distance = region.view_distance;
    for snapshot in snapshots {
        let tracked = &mut region.entity_tracking.tracked;
        tracked
            .entry(snapshot.id)
            .or_insert_with(|| TrackedEntity::new(&snapshot));
        let tracked = tracked.get_mut(&snapshot.id).expect("刚放进去");
        update_pairings(
            &region.players,
            &mut region.links,
            &snapshot,
            tracked,
            view_distance,
        );
        send_changes(&snapshot, tracked, &mut region.links);
    }
}

/// 已经不在世界里的实体：给看得见它的玩家发 `remove_entities`。
fn remove_missing(region: &mut Region, live: &BTreeSet<i32>) {
    let stale: Vec<i32> = region
        .entity_tracking
        .tracked
        .keys()
        .copied()
        .filter(|id| !live.contains(id))
        .collect();
    for id in stale {
        let Some(tracked) = region.entity_tracking.tracked.remove(&id) else {
            continue;
        };
        let frames = encode_frames(&RemoveEntities { entity_ids: &[id] });
        for viewer in tracked.seen_by {
            region.links.send_frames(viewer, &frames);
        }
    }
}

/// 这一 tick 哪些玩家应该看得到这个实体。官方只在实体的区块变了、或者玩家换了区块时
/// 才重新算；这里每 tick 都算一遍，结果一样，只是多花几次比较。
fn update_pairings(
    players: &BTreeMap<PlayerId, Player>,
    links: &mut Links,
    snapshot: &Snapshot,
    tracked: &mut TrackedEntity,
    view_distance: i32,
) {
    let mut visible: Vec<PlayerId> = Vec::new();
    for player in players.values() {
        if is_visible(player, snapshot, view_distance) {
            visible.push(player.id);
        }
    }
    let lost: Vec<PlayerId> = tracked
        .seen_by
        .iter()
        .copied()
        .filter(|id| !visible.contains(id))
        .collect();
    let gained: Vec<PlayerId> = visible
        .into_iter()
        .filter(|id| !tracked.seen_by.contains(id))
        .collect();

    if !lost.is_empty() {
        let frames = encode_frames(&RemoveEntities {
            entity_ids: &[snapshot.id],
        });
        for viewer in lost {
            tracked.seen_by.remove(&viewer);
            links.send_frames(viewer, &frames);
        }
    }
    if !gained.is_empty() {
        let frames = pairing_frames(snapshot, tracked);
        for viewer in gained {
            tracked.seen_by.insert(viewer);
            links.send_frames(viewer, &frames);
        }
    }
}

/// `ChunkMap.TrackedEntity.updatePlayer` 的可见性判断：跟踪范围和玩家的视距取小的那个，
/// 按水平距离比较。
///
/// 这两种实体对谁都可见，旁观模式也还没做，所以 `broadcastToPlayer` 总是真。
fn is_visible(player: &Player, snapshot: &Snapshot, view_distance: i32) -> bool {
    let entity_range = entity_tracking_range(snapshot.entity_type);
    let player_range = player.view_distance(view_distance) * BLOCKS_PER_CHUNK;
    let range = f64::from(entity_range.min(player_range));
    let dx = player.position.x - snapshot.position.x;
    let dz = player.position.z - snapshot.position.z;
    // 区块还没发到时不配对：`add_entity` 到达时区块要已经加载了。
    dx * dx + dz * dz <= range * range && player.chunks.is_chunk_tracked(snapshot.chunk)
}

/// 实体类型在 `clientTrackingRange` 里写的区块数换成格。
fn entity_tracking_range(entity_type: i32) -> i32 {
    entity_types::FACTS[entity_type as usize].client_tracking_range * BLOCKS_PER_CHUNK
}

/// 把实体介绍给新观察者的 bundle。位置和速度都取上次发出去的状态，
/// 下一个包才接得上。
fn pairing_frames(snapshot: &Snapshot, tracked: &TrackedEntity) -> Vec<u8> {
    let mut frames = encode_frames(&BundleDelimiter);
    frames.extend(encode_frames(&AddEntity {
        entity_id: snapshot.id,
        uuid: entity_uuid(snapshot.id),
        entity_type: snapshot.entity_type,
        position: tracked.base,
        movement: tracked.last_movement,
        // 这两种实体没有朝向，头上的角度也一直是 0。
        pitch: 0,
        yaw: 0,
        head_yaw: 0,
        data: snapshot.add_data(),
    }));
    let items = snapshot.data.pairing_values();
    if !items.is_empty() {
        frames.extend(encode_frames(&SetEntityData {
            entity_id: snapshot.id,
            items: &items,
        }));
    }
    frames.extend(encode_frames(&BundleDelimiter));
    frames
}

/// 官方实体的 UUID 是随机的；这里由实体 ID 推出来，同一个实体每次重启都一样，
/// 测试才比得了。客户端只要求它唯一。
fn entity_uuid(id: i32) -> Uuid {
    Uuid::from_u128(0x0000_BAAB_0000_0000_0000_0000_0000_0000_u128 | u128::from(id as u32))
}

/// 每 tick 调用一次。对应 `ServerEntity.sendChanges`。
fn send_changes(snapshot: &Snapshot, tracked: &mut TrackedEntity, links: &mut Links) {
    let interval = update_interval(snapshot.entity_type);
    let data_dirty = !snapshot.data.changes_since(tracked.last_fuse).is_empty();
    let due = interval.is_some_and(|period| tracked.tick_count % period == 0);
    if !(due || data_dirty) {
        tracked.tick_count += 1;
        return;
    }

    // `trackDeltas` 为真的实体每周期都可能发一次速度，和位置包独立。
    if snapshot.track_deltas() {
        let diff = snapshot.delta.distance_to_sqr(tracked.last_movement);
        if diff > MOTION_TOLERANCE || (diff > 0.0 && snapshot.delta.length_sqr() == 0.0) {
            tracked.last_movement = snapshot.delta;
            let frames = encode_frames(&SetEntityMotion {
                entity_id: snapshot.id,
                movement: snapshot.delta,
            });
            tracked.send_to_viewers(links, &frames);
        }
    }

    tracked.teleport_delay += 1;
    if let Some(packet) = choose_movement(snapshot, tracked) {
        let frames = packet.frames(snapshot.id, snapshot);
        tracked.send_to_viewers(links, &frames);
        // 这两种包都带位置，所以基准点跟着走。
        tracked.base = snapshot.position;
    }

    // `sendDirtyEntityData` 排在移动包后面。
    let items = snapshot.data.changes_since(tracked.last_fuse);
    if !items.is_empty() {
        if let Some(fuse) = snapshot.data.fuse() {
            tracked.last_fuse = fuse;
        }
        let frames = encode_frames(&SetEntityData {
            entity_id: snapshot.id,
            items: &items,
        });
        tracked.send_to_viewers(links, &frames);
    }

    tracked.tick_count += 1;
}

/// `UpdateInterval.periodic`。`update_interval` 是 -1 时表示 `NEVER`。
fn update_interval(entity_type: i32) -> Option<i32> {
    let interval = entity_types::FACTS[entity_type as usize].update_interval;
    (interval > 0).then_some(interval)
}

/// `ServerEntity.createMovePacket`。这两种实体不转向，所以旋转那一半总是空的。
fn choose_movement(snapshot: &Snapshot, tracked: &mut TrackedEntity) -> Option<Movement> {
    if snapshot.precise
        || tracked.teleport_delay > FORCED_TELEPORT_PERIOD
        || tracked.was_on_ground != snapshot.on_ground
    {
        tracked.was_on_ground = snapshot.on_ground;
        tracked.teleport_delay = 0;
        return Some(Movement::Sync);
    }
    let should_send_position = has_moved(tracked.base, snapshot.position)
        || tracked.tick_count % FORCED_POS_UPDATE_PERIOD == 0;
    if !should_send_position {
        return None;
    }
    Some(match PositionDelta::between(tracked.base, snapshot.position) {
        Some(delta) => Movement::Position(delta),
        None => Movement::Sync,
    })
}

/// `positionCodec.delta(position).lengthSqr() >= 7.6293945E-6F`。
fn has_moved(base: Vec3, position: Vec3) -> bool {
    position.distance_to_sqr(base) >= POSITION_TOLERANCE
}

/// 这一周期要发的移动包。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Movement {
    /// `entity_position_sync`。
    Sync,
    /// `move_entity_pos`。
    Position(PositionDelta),
}

impl Movement {
    fn frames(self, entity_id: i32, snapshot: &Snapshot) -> Vec<u8> {
        match self {
            Self::Sync => encode_frames(&EntityPositionSync {
                entity_id,
                // 这两种实体没有插值，官方的 `InterpolationTracker.NO_OP` 总是返回 Linear。
                path: PositionPath::Linear(snapshot.position),
                yaw: 0.0,
                pitch: 0.0,
                on_ground: snapshot.on_ground,
            }),
            Self::Position(delta) => encode_frames(&MoveEntityPos {
                entity_id,
                // 没有插值时官方发的是 `VecDelta.Linear`，客户端按两步算。
                delta: VecDelta::Linear(delta),
                on_ground: snapshot.on_ground,
            }),
        }
    }
}

/// 离开世界：把玩家从所有实体的观察者名单里删掉。这时链路已经关了，不用发包。
pub(crate) fn on_leave(region: &mut Region, id: PlayerId) {
    for tracked in region.entity_tracking.tracked.values_mut() {
        tracked.seen_by.remove(&id);
    }
}

#[cfg(test)]
mod tests;
