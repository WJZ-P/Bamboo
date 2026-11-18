//! 世界实体（下落的方块、点燃的 TNT）和玩家之间的同步。
//!
//! 对应官方的 `ChunkMap.TrackedEntity` 和 `ServerEntity`：按实体的跟踪范围配对，
//! 配对时发 `add_entity` 和元数据，之后按实体类型的更新间隔发位移、速度和元数据，
//! 实体没了就发 `remove_entities`。
//!
//! 这两种实体都没有朝向、乘客、属性、装备和拴绳，所以配对包只有 `add_entity`
//! 和 `set_entity_data` 两项。

use std::collections::{BTreeMap, BTreeSet, HashMap};

use mc_bamboo_blocks::{
    entity::{EntityPublic, KindPublic, PrimedTnt},
    state::BlockState,
};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::entity_types;
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{
        AddEntity, BundleDelimiter, EntityData, EntityPositionSync, MoveEntityPos, PositionDelta,
        PositionPath, RemoveEntities, SetEntityData, SetEntityMotion, TakeItemEntity, VecDelta,
        pack_degrees,
    },
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

impl EntityTracking {
    /// 岛拆分（岛的实施计划 3.3「实体追踪的配对」）：配对按实体跟着走，`seen_by` 原样带走，
    /// 不清空重建（重建会重发生成包，玩家看得见）。`entities` 是搬走的实体的网络 ID。
    /// 实体已经没了、还等着下一刻发移除包的条目跟着它的观察者走。视野不跨岛
    /// （4.2），所以观察者和实体总在同一边（调试构建断言）。
    ///
    /// Island split: pairings follow their entity with `seen_by` intact (a
    /// rebuild would resend spawn packets). Entries whose entity is already
    /// gone follow their viewers. Viewers and entity are always on the same
    /// island (4.2), asserted in debug builds.
    pub(crate) fn split_off(
        &mut self,
        entities: &BTreeSet<i32>,
        players: &BTreeSet<PlayerId>,
    ) -> Self {
        let tracked = std::mem::take(&mut self.tracked);
        let mut out = Self::default();
        for (id, entry) in tracked {
            let moves = entities.contains(&id)
                || (!entry.seen_by.is_empty() && entry.seen_by.is_subset(players));
            debug_assert!(
                if moves {
                    entry.seen_by.is_subset(players)
                } else {
                    entry.seen_by.is_disjoint(players)
                },
                "实体 {id} 的观察者跨了两个岛"
            );
            if moves {
                out.tracked.insert(id, entry);
            } else {
                self.tracked.insert(id, entry);
            }
        }
        out
    }

    /// 岛合并：配对并进来。网络 ID 全世界唯一，不会撞。
    ///
    /// Island merge: network ids are world-unique, so nothing collides.
    pub(crate) fn absorb(&mut self, other: Self) {
        for (id, entry) in other.tracked {
            let previous = self.tracked.insert(id, entry);
            debug_assert!(previous.is_none(), "实体 {id} 在两个岛上都有配对");
        }
    }

    /// 测试核对拆合用：每个被跟踪的实体和看得见它的玩家。
    #[cfg(test)]
    pub(crate) fn pairs(&self) -> Vec<(i32, Vec<PlayerId>)> {
        self.tracked
            .iter()
            .map(|(&id, entry)| (id, entry.seen_by.iter().copied().collect()))
            .collect()
    }
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
    /// 上次发出去的物品堆。空堆是默认值，不单独发。
    ///
    /// The item stack last sent. An empty stack is the default and is not sent on its own.
    last_stack: ItemStack,
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
            last_stack: snapshot
                .data
                .stack()
                .cloned()
                .unwrap_or_else(ItemStack::empty),
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
#[derive(Debug, Clone)]
struct Snapshot {
    id: i32,
    entity_type: i32,
    position: Vec3,
    delta: Vec3,
    on_ground: bool,
    precise: bool,
    /// `yRot`。物品构造时取一次，下落方块和 TNT 一直是 0。
    ///
    /// `yRot`. An item draws it once. Falling blocks and TNT stay at 0.
    yaw: f32,
    chunk: ChunkPos,
    data: KindData,
}

/// 配对和同步时要发的、按实体类型不同的元数据。
#[derive(Debug, Clone, PartialEq)]
enum KindData {
    FallingBlock { state: BlockState, start: BlockPos },
    Tnt { fuse: i32 },
    Item(ItemStack),
    Mob(Box<mc_bamboo_blocks::entity::mob::MobPublic>),
}

impl Snapshot {
    /// 从公开数据抄一份。字段和 [`Entity::split`](mc_bamboo_blocks::entity::Entity::split)
    /// 的 public 半边一一对应，不克隆私有状态、随机数、移动记录和物品堆以外的东西。
    fn of(public: &EntityPublic) -> Self {
        Self {
            id: public.uid,
            entity_type: public.kind.entity_type(),
            position: public.position,
            delta: public.delta,
            on_ground: public.on_ground,
            precise: public.precise,
            yaw: public.yaw,
            chunk: public.chunk_position(),
            data: KindData::of(&public.kind),
        }
    }

    /// `getAddEntityPacket` 的 `data`：下落的方块是它的方块状态，TNT 和物品是 0。
    fn add_data(&self) -> i32 {
        match &self.data {
            KindData::FallingBlock { state, .. } => state.0 as i32,
            KindData::Tnt { .. } | KindData::Item(_) | KindData::Mob(_) => 0,
        }
    }

    fn track_deltas(&self) -> bool {
        entity_types::FACTS[self.entity_type as usize].track_deltas
    }
}

impl KindData {
    fn of(kind: &KindPublic) -> Self {
        match kind {
            KindPublic::FallingBlock { state, start_pos } => Self::FallingBlock {
                state: *state,
                start: *start_pos,
            },
            KindPublic::Tnt { fuse } => Self::Tnt { fuse: *fuse },
            KindPublic::Item { stack, .. } => Self::Item(stack.clone()),
            KindPublic::Mob(mob) => Self::Mob(mob.clone()),
        }
    }

    /// 同步用到的那个数字。下落的方块和物品没有。
    fn fuse(&self) -> Option<i32> {
        match self {
            Self::FallingBlock { .. } | Self::Item(_) | Self::Mob(_) => None,
            Self::Tnt { fuse } => Some(*fuse),
        }
    }

    fn stack(&self) -> Option<&ItemStack> {
        match self {
            Self::Item(stack) => Some(stack),
            Self::FallingBlock { .. } | Self::Tnt { .. } | Self::Mob(_) => None,
        }
    }

    /// 配对时发全部非默认值。下落的方块的起点永远不是默认值；TNT 的引信只在
    /// 被缩短过时不是。物品堆不是空的就要发。
    ///
    /// Pairing sends every non-default value. A non-empty item stack is not the default.
    fn pairing_values(&self) -> Vec<EntityData> {
        match self {
            Self::FallingBlock { start, .. } => vec![EntityData::FallingBlockStart(*start)],
            Self::Tnt { fuse } if *fuse != PrimedTnt::DEFAULT_FUSE => {
                vec![EntityData::TntFuse(*fuse)]
            }
            Self::Tnt { .. } => Vec::new(),
            Self::Item(stack) if !stack.is_empty() => vec![EntityData::Item(stack.clone())],
            Self::Item(_) | Self::Mob(_) => Vec::new(),
        }
    }

    /// 这一周期要发的元数据；和上次发的一样时是空的。
    fn changes_since(&self, last_fuse: i32, last_stack: &ItemStack) -> Vec<EntityData> {
        match self {
            // 起点生成以后就不再变。
            Self::FallingBlock { .. } => Vec::new(),
            Self::Tnt { fuse } if *fuse != last_fuse => vec![EntityData::TntFuse(*fuse)],
            Self::Tnt { .. } => Vec::new(),
            Self::Item(stack) if stack != last_stack => vec![EntityData::Item(stack.clone())],
            Self::Item(_) | Self::Mob(_) => Vec::new(),
        }
    }
}

/// tick 第 6 阶段：世界实体的配对、同步和移除。对应 `ChunkMap.tick`。
pub(crate) fn sync(region: &mut Region) {
    // 外层仍是全表按 uid 升序：每个实体都要发同步包，消失的也要发移除包。
    // 只读公开数据，不克隆整行；配对改走每刻预建的粗网格候选表。
    //
    // The outer loop still walks the whole table in uid order: every entity sends
    // sync packets and departed ones send removals. Only public data is read, and
    // pairing uses a coarse grid of players built once per tick.
    let snapshots: Vec<Snapshot> = region.entities.iter_public().map(Snapshot::of).collect();
    let live: BTreeSet<i32> = snapshots.iter().map(|snapshot| snapshot.id).collect();
    remove_missing(region, &live);

    let view_distance = region.view_distance;
    let cells = PlayerCells::build(&region.players, view_distance);
    for snapshot in snapshots {
        let tracked = &mut region.entity_tracking.tracked;
        tracked
            .entry(snapshot.id)
            .or_insert_with(|| TrackedEntity::new(&snapshot));
        let tracked = tracked.get_mut(&snapshot.id).expect("刚放进去");
        update_pairings(
            &region.players,
            &cells,
            &mut region.links,
            &snapshot,
            tracked,
            view_distance,
        );
        send_changes(&snapshot, tracked, &mut region.links);
    }
}

/// 每刻建一次的粗网格：格子边长不小于任何玩家可能看得见实体的最大水平距离，
/// 每名玩家登记进自己可见范围覆盖到的少数几个格子（边长不小于半径，最多 3×3 格），
/// 一个实体只要查自己那一格就能拿到全部候选。格子里的玩家按 `PlayerId` 升序，
/// 和原来遍历 `players.values()` 的顺序相同。
///
/// A coarse grid rebuilt once per tick. Its cell edge is no smaller than the
/// greatest horizontal distance at which `is_visible` can accept a player, so
/// each player is registered in the few cells their visible range covers (at
/// most 3x3 when the edge is no smaller than the radius), and one lookup per
/// entity yields every candidate. Ids in each cell stay in PlayerId order,
/// matching the old walk over `players.values()`.
struct PlayerCells {
    edge: f64,
    cells: HashMap<(i64, i64), Vec<PlayerId>>,
}

impl PlayerCells {
    fn build(players: &BTreeMap<PlayerId, Player>, view_distance: i32) -> Self {
        // 视距乘 16 是 `is_visible` 的上界：实体类型的范围和玩家视距取小的那个，
        // 玩家视距又不超过服务端视距。格子不小于它，落在半径内的玩家必定和
        // 实体同格或相邻一格；这里把每名玩家登记进自己能看见实体的所有格子，
        // 等价于在实体侧做一次哈希查找。
        let edge = f64::from(view_distance * BLOCKS_PER_CHUNK).max(1.0);
        let mut cells: HashMap<(i64, i64), Vec<PlayerId>> = HashMap::new();
        for (&id, player) in players {
            let range = f64::from(player.view_distance(view_distance) * BLOCKS_PER_CHUNK);
            let x = player.position().x;
            let z = player.position().z;
            let first_x = ((x - range) / edge).floor() as i64;
            let last_x = ((x + range) / edge).floor() as i64;
            let first_z = ((z - range) / edge).floor() as i64;
            let last_z = ((z + range) / edge).floor() as i64;
            for cell_x in first_x..=last_x {
                for cell_z in first_z..=last_z {
                    cells.entry((cell_x, cell_z)).or_default().push(id);
                }
            }
        }
        Self { edge, cells }
    }

    /// 实体所在的格子里的玩家，按 `PlayerId` 升序。
    fn candidates(&self, x: f64, z: f64) -> &[PlayerId] {
        let cell = (
            (x / self.edge).floor() as i64,
            (z / self.edge).floor() as i64,
        );
        self.cells.get(&cell).map(Vec::as_slice).unwrap_or(&[])
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
    cells: &PlayerCells,
    links: &mut Links,
    snapshot: &Snapshot,
    tracked: &mut TrackedEntity,
    view_distance: i32,
) {
    // 「停止追踪」直接遍历 `seen_by`：一名玩家是否还看得见，和它在不在候选格
    // 里无关，传送走或视距变小照样会算到。「开始追踪」只遍历粗网格给出的
    // 候选：任何看得见的玩家都登记进了实体所在的格子，所以名单不漏。
    // 两个名单都按 PlayerId 升序，顺序和原来遍历 `players.values()` 逐项相同；
    // 常见的一刻里两者都是空的，不分配堆内存。
    //
    // Stop-tracking walks `seen_by` directly: visibility does not depend on the
    // candidate cell, so teleports and smaller view distances are still caught.
    // Start-tracking walks only the coarse-grid candidates, which by construction
    // include every visible player. Both lists stay in ascending PlayerId order,
    // exactly like the old walk over `players.values()`, and allocate nothing on
    // the common tick where nothing changes.
    let is_visible_to = |id: PlayerId| {
        players
            .get(&id)
            .is_some_and(|player| is_visible(player, snapshot, view_distance))
    };
    let mut lost: Vec<PlayerId> = Vec::new();
    for &id in &tracked.seen_by {
        if !is_visible_to(id) {
            lost.push(id);
        }
    }
    if !lost.is_empty() {
        let frames = encode_frames(&RemoveEntities {
            entity_ids: &[snapshot.id],
        });
        for viewer in lost {
            tracked.seen_by.remove(&viewer);
            links.send_frames(viewer, &frames);
        }
    }

    let nearby = cells.candidates(snapshot.position.x, snapshot.position.z);
    let mut gained: Vec<PlayerId> = Vec::new();
    for &id in nearby {
        if !tracked.seen_by.contains(&id) && is_visible_to(id) {
            gained.push(id);
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
    let dx = player.position().x - snapshot.position.x;
    let dz = player.position().z - snapshot.position.z;
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
        // 物品的 `yRot` 在构造时取过。下落方块和 TNT 一直是 0。头上的角度不跟着走。
        // An item's `yRot` was drawn in the constructor. Falling blocks and TNT stay at 0.
        pitch: 0,
        yaw: pack_degrees(snapshot.yaw),
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
/// 测试才比得了。客户端只要求它唯一。存档里的 `UUID` 用同一份推导。
fn entity_uuid(id: i32) -> Uuid {
    Uuid::from_u128(mc_bamboo_blocks::archive::entity_uuid(id))
}

/// 每 tick 调用一次。对应 `ServerEntity.sendChanges`。
fn send_changes(snapshot: &Snapshot, tracked: &mut TrackedEntity, links: &mut Links) {
    let interval = update_interval(snapshot.entity_type);
    let data_dirty = !snapshot
        .data
        .changes_since(tracked.last_fuse, &tracked.last_stack)
        .is_empty();
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
    let items = snapshot
        .data
        .changes_since(tracked.last_fuse, &tracked.last_stack);
    if !items.is_empty() {
        if let Some(fuse) = snapshot.data.fuse() {
            tracked.last_fuse = fuse;
        }
        if let Some(stack) = snapshot.data.stack() {
            tracked.last_stack = stack.clone();
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
    Some(
        match PositionDelta::between(tracked.base, snapshot.position) {
            Some(delta) => Movement::Position(delta),
            None => Movement::Sync,
        },
    )
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
                yaw: snapshot.yaw,
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

/// 捡起动画。看得见这件物品的玩家，以及捡起它的人，都收到。
///
/// The pickup animation goes to everyone who can see the item, and to the player who took it.
pub(crate) fn broadcast_take(region: &mut Region, item_id: i32, player: PlayerId, amount: i32) {
    let frames = encode_frames(&TakeItemEntity {
        item_id,
        player_id: player.0,
        amount,
    });
    let mut viewers = region
        .entity_tracking
        .tracked
        .get(&item_id)
        .map(|tracked| tracked.seen_by.clone())
        .unwrap_or_default();
    viewers.insert(player);
    for viewer in viewers {
        region.links.send_frames(viewer, &frames);
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
