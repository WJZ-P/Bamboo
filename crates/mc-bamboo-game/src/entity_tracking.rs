//! 世界实体和玩家之间的同步。
//!
//! 对应官方的 `ChunkMap.TrackedEntity` 和 `ServerEntity`：按实体的跟踪范围配对，
//! 配对时发 `add_entity` 和元数据，之后按实体类型的更新间隔发位移、速度和元数据，
//! 实体没了就发 `remove_entities`。
//!
//! 生物走同一套流程：[`TrackedEntity::pairing_packets`] 和 [`TrackedEntity::step`]
//! 是官方 `ServerEntity.sendPairingData` / `sendChanges` 的发包部分。两个函数只产出
//! 「包 ID 的 VarInt + 包体」字节序列，游戏路径和逐刻对拍都调用它们，不另写一份。
//!
//! World entities and their viewers. [`TrackedEntity::pairing_packets`] and
//! [`TrackedEntity::step`] emit the same "VarInt packet id + body" bytes that
//! vanilla's `ServerEntity` does; the live path and the tick-by-tick oracle
//! both call these and no second implementation exists.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use mc_bamboo_blocks::{
    entity::{EntityPublic, KindPublic, PrimedTnt},
    state::BlockState,
};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::{entity_types, is_client_syncable_attribute};
use mc_bamboo_protocol::{
    frame::packet_body,
    item::ItemStack,
    packet::play::{
        AddEntity, AttributeModifier, AttributeOperation, AttributeSnapshot, BundleDelimiter,
        EntityData, EntityPositionSync, MoveEntityPos, MoveEntityPosRot, MoveEntityRot,
        PositionDelta, PositionPath, RemoveEntities, RotateHead, SetEntityData, SetEntityMotion,
        SetPassengers, TakeItemEntity, UpdateAttributes, VecDelta, pack_degrees,
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

/// `update_attributes` 上一次发过的属性状态（ID、基础值、修饰符、脏代数）。
///
/// Last synced state for `update_attributes` (id, base, modifiers, dirty generation).
type AttributeState = (i32, f64, Vec<(String, f64, i32)>, u64);

/// 一个世界里全部被跟踪的实体。
#[derive(Debug, Default)]
pub struct EntityTracking {
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
        let mut out = Self::default();
        // 只扫键、挑出要走的再逐个取出，不重建整张表（配对只有玩家附近的实体，和岛的
        // 大小无关）。
        // Scan keys and take out the moving entries instead of rebuilding the
        // table (pairings exist only near players, independent of island size).
        let going: Vec<i32> = self
            .tracked
            .iter()
            .filter(|(id, entry)| {
                let moves = entities.contains(id)
                    || (!entry.seen_by.is_empty() && entry.seen_by.is_subset(players));
                debug_assert!(
                    if moves {
                        entry.seen_by.is_subset(players)
                    } else {
                        entry.seen_by.is_disjoint(players)
                    },
                    "实体 {id} 的观察者跨了两个岛"
                );
                moves
            })
            .map(|(&id, _)| id)
            .collect();
        for id in going {
            let entry = self.tracked.remove(&id).expect("刚看过");
            out.tracked.insert(id, entry);
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

    /// 这名玩家还看着哪个实体（跨岛搬玩家之前要等它为假）。
    ///
    /// Whether this player still watches any entity (an island crossing waits
    /// until it is false).
    pub(crate) fn watched_by(&self, player: PlayerId) -> bool {
        self.tracked
            .values()
            .any(|entry| entry.seen_by.contains(&player))
    }

    /// 有没有一个实体同时被 `players` 里和外的玩家看着（拆岛之前要等它为假）。
    ///
    /// Whether some entity is watched both by players in `players` and by
    /// players outside it (a split waits until it is false).
    pub(crate) fn straddles(&self, players: &std::collections::BTreeSet<PlayerId>) -> bool {
        self.tracked.values().any(|entry| {
            let inside = entry
                .seen_by
                .iter()
                .filter(|id| players.contains(id))
                .count();
            inside != 0 && inside != entry.seen_by.len()
        })
    }

    /// 只给改坏确认用：清掉一个实体的全部观察者，模拟「合并后追踪条目丢了」。
    ///
    /// Break-it checks only: clear an entity's watchers, simulating a lost
    /// tracking entry after a merge.
    #[cfg(test)]
    pub(crate) fn forget_watchers(&mut self, entity: i32) -> bool {
        match self.tracked.get_mut(&entity) {
            Some(entry) => {
                entry.seen_by.clear();
                true
            }
            None => false,
        }
    }

    /// 只给改坏确认用：整条丢掉一个实体的跟踪状态，模拟拆合时漏搬了它。
    ///
    /// Break-it checks only: drop an entity's whole tracking state, simulating
    /// a split that left it behind.
    #[cfg(test)]
    pub(crate) fn forget_entity(&mut self, entity: i32) -> bool {
        self.tracked.remove(&entity).is_some()
    }

    /// 测试核对拆合用：每个被跟踪的实体和看得见它的玩家。
    #[cfg(test)]
    pub(crate) fn pairs(&self) -> Vec<(i32, Vec<PlayerId>)> {
        self.tracked
            .iter()
            .map(|(&id, entry)| (id, entry.seen_by.iter().copied().collect()))
            .collect()
    }

    /// 逐刻对拍用：这个实体是不是已经有过同步状态。
    pub fn has_tracked_for_oracle(&self, id: i32) -> bool {
        self.tracked.contains_key(&id)
    }

    /// 逐刻对拍用：一头牛的同步状态。没有时按当时实体样子新建一个。
    ///
    /// The sync state of one entity for the tick-by-tick oracle. A fresh one is
    /// created from the entity's current appearance when absent.
    pub fn tracked_for_oracle(&mut self, snapshot: &Snapshot) -> &mut TrackedEntity {
        self.tracked
            .entry(snapshot.id)
            .or_insert_with(|| TrackedEntity::new(snapshot))
    }
}

/// 官方 `ServerEntity` 里同步要用的字段。
///
/// The fields of vanilla `ServerEntity` that matter for sync.
#[derive(Debug)]
pub struct TrackedEntity {
    /// `positionCodec` 的基准点，上次发给客户端的位置。
    base: Vec3,
    /// 上次发出去的速度。
    last_movement: Vec3,
    /// 配对包里的速度：`ServerEntity` 在实体刚进世界时把 `getDeltaMovement()`
    /// 记进 `lastSentMovement`，之后不再跟着实体变。
    ///
    /// Velocity for the pairing packet: vanilla's `ServerEntity` snapshots
    /// `getDeltaMovement()` when the entity joins the world and never follows
    /// later changes.
    pairing_movement: Vec3,
    /// 上次发出去的打包角度（`yRot`、`xRot`、`yHeadRot`）。
    last_yaw: u8,
    last_pitch: u8,
    last_head: u8,
    /// 上次发出去的全部非默认元数据，配对时按原样再发一遍。
    last_data: Vec<EntityData>,
    /// `SynchedEntityData` 里还没发过的脏项。官方构造实体时就设过的字段
    /// （例如血量）是脏的，配对包发了全部非默认值，但第一次 `sendChanges`
    /// 仍会重发一遍脏项。
    ///
    /// Dirty metadata not yet sent. Vanilla sets some fields in the constructor
    /// (health), so the first `sendChanges` repeats them.
    pending_dirty: Vec<EntityData>,
    /// 上一次发过的可同步属性的状态；新实例化或改过的要发 `update_attributes`。
    last_attributes: Vec<AttributeState>,
    /// 距上次全量同步的周期数。
    teleport_delay: u32,
    /// `SteppedInterpolationTracker.ticksSinceLastStep`：每隔多少刻记一步。
    ticks_since_step: i32,
    /// `ServerEntity.tickCount`，每刻加一，周期判定和强制位置同步都用。
    tick_count: i32,
    was_on_ground: bool,
    /// 上次发出去的乘客名单。官方 `ServerEntity.lastPassengers`。
    ///
    /// The passenger list last sent, vanilla `ServerEntity.lastPassengers`.
    last_passengers: Vec<i32>,
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
        let last_data = snapshot.data.pairing_values();
        let mut pending_dirty = last_data.clone();
        // 官方 `SynchedEntityData.packDirty`：若某字段在出生前被改过又改回默认值
        // （如出巢蜜蜂先读入 `HasNectar=true` 再由 `dropOffNectar` 清回 0），
        // `getNonDefaultValues` 不含它，但首次 `packDirty` 仍会发出该脏项。
        // Vanilla `SynchedEntityData.packDirty`: a field modified before spawn and reset to its
        // default (e.g. a released bee loading `HasNectar=true` then cleared by `dropOffNectar`)
        // is omitted from `getNonDefaultValues` yet still emitted by the first `packDirty`.
        if let KindData::Mob(mob) = &snapshot.data
            && mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Bee
            && mob.bee_flags_dirty
            && mob.bee_flags == 0
        {
            pending_dirty.push(EntityData::BeeFlags(0));
        }
        Self {
            base: snapshot.position,
            last_movement: snapshot.delta,
            pairing_movement: snapshot.delta,
            last_yaw: pack_degrees(snapshot.yaw),
            last_pitch: snapshot.data.mob_x_rot().map_or(0, pack_degrees),
            last_head: snapshot.data.mob_head_rot().map_or(0, pack_degrees),
            last_data,
            pending_dirty,
            last_attributes: Vec::new(),
            teleport_delay: 0,
            ticks_since_step: -1,
            tick_count: 0,
            was_on_ground: snapshot.on_ground,
            last_passengers: snapshot.passengers.clone(),
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

    /// 官方 `ServerEntity.sendPairingData` 的包序列，按 bundle 里的顺序。
    /// 返回的是每个包「ID + 包体」的字节，不含帧长和 bundle 分隔符。这里不带载具
    /// 名单，乘客那一侧的 `set_passengers` 见 [`Self::pairing_packets_in`]。
    ///
    /// The packet sequence of vanilla `ServerEntity.sendPairingData`, in bundle
    /// order. Each item is one packet's "id + body" bytes, without the frame
    /// length prefix or the bundle delimiters. Without a vehicle index; see
    /// [`Self::pairing_packets_in`] for a passenger's `set_passengers`.
    pub fn pairing_packets(&self, snapshot: &Snapshot) -> Vec<Vec<u8>> {
        self.pairing_packets_in(snapshot, &PassengerIndex::default())
    }

    /// 同 [`Self::pairing_packets`]，但用这一 tick 的全表索引补上官方最后那句
    /// 「自己在骑就发载具的 `set_passengers`」。
    ///
    /// Like [`Self::pairing_packets`], but with this tick's whole-table index for
    /// vanilla's trailing `set_passengers(vehicle)` when this entity is a passenger.
    pub fn pairing_packets_in(&self, snapshot: &Snapshot, index: &PassengerIndex) -> Vec<Vec<u8>> {
        let mut out = vec![packet_body(&AddEntity {
            entity_id: snapshot.id,
            uuid: snapshot
                .data
                .stable_uuid()
                .map_or_else(|| entity_uuid(snapshot.id), Uuid::from_bytes),
            entity_type: snapshot.entity_type,
            position: self.base,
            movement: self.pairing_movement,
            pitch: self.last_pitch,
            yaw: self.last_yaw,
            head_yaw: self.last_head,
            data: snapshot.add_data(),
        })];
        // 官方箭的 trackedDataValues 为空时不发；其它类型保留现有空表包行为。
        // Vanilla skips empty arrow metadata; other types retain their existing empty packet.
        if !matches!(snapshot.data, KindData::Arrow(_) | KindData::LlamaSpit(_))
            || !self.last_data.is_empty()
        {
            out.push(packet_body(&SetEntityData {
                entity_id: snapshot.id,
                items: &self.last_data,
            }));
        }
        // `LivingEntity` 的可同步属性，属性表里哪些是同步的由数据层给出。
        // 官方 `AttributeSnapshot` 带 `getModifiers()` 的全部修饰符，临时的（效果、装备、
        // 细雪减速）也在里面。
        // Syncable `LivingEntity` attributes; the data layer says which ones.
        if let KindData::Mob(mob) = &snapshot.data {
            let mut snapshots = Vec::new();
            let mut modifiers: Vec<Vec<AttributeModifier<'_>>> = Vec::new();
            for entry in &mob.attributes.entries {
                if !entry.instantiated || !is_client_syncable_attribute(entry.id) {
                    continue;
                }
                modifiers.push(
                    entry
                        .modifiers
                        .iter()
                        .map(|modifier| AttributeModifier {
                            id: &modifier.id,
                            amount: modifier.amount,
                            operation: match modifier.operation {
                                mc_bamboo_blocks::entity::mob::attributes::Operation::AddValue => {
                                    AttributeOperation::AddValue
                                }
                                mc_bamboo_blocks::entity::mob::attributes::Operation::AddMultipliedBase => {
                                    AttributeOperation::AddMultipliedBase
                                }
                                mc_bamboo_blocks::entity::mob::attributes::Operation::AddMultipliedTotal => {
                                    AttributeOperation::AddMultipliedTotal
                                }
                            },
                        })
                        .collect(),
                );
                snapshots.push((entry.id, entry.base));
            }
            let attributes: Vec<AttributeSnapshot<'_>> = snapshots
                .iter()
                .zip(&modifiers)
                .map(|(&(id, base), list)| AttributeSnapshot {
                    attribute: id,
                    base,
                    modifiers: list,
                })
                .collect();
            if !attributes.is_empty() {
                out.push(packet_body(&UpdateAttributes {
                    entity_id: snapshot.id,
                    attributes: &attributes,
                }));
            }
            // `sendPairingData`：非空的装备槽一个 `set_equipment`。
            let slots: Vec<_> = mob
                .equipment
                .iter()
                .map(|(slot, stack)| (*slot, stack))
                .collect();
            if let Some(packet) =
                mc_bamboo_protocol::packet::play::SetEquipment::new(snapshot.id, &slots)
            {
                out.push(packet_body(&packet));
            }
            // `sendPairingData`：拴着就发 `set_entity_link`，客户端才画得出绳子。
            if let Some(holder) = mob.leash_holder {
                out.push(packet_body(
                    &mc_bamboo_protocol::packet::play::SetEntityLink {
                        source_id: snapshot.id,
                        dest_id: holder,
                    },
                ));
            }
        }
        // 官方 `sendPairingData` 最后两句：自己有乘客就发自己的名单，自己在骑就发
        // 载具的名单（乘客那一侧客户端才知道自己挂在谁身上）。
        if !snapshot.passengers.is_empty() {
            out.push(packet_body(&SetPassengers {
                vehicle: snapshot.id,
                passengers: &snapshot.passengers,
            }));
        }
        if let Some(vehicle) = snapshot.vehicle
            && let Some(list) = index.list(vehicle)
        {
            out.push(packet_body(&SetPassengers {
                vehicle,
                passengers: list,
            }));
        }
        out
    }

    /// 官方 `ServerEntity.sendChanges` 这一刻的全部包，按发出顺序。
    /// 返回每个包「ID + 包体」的字节。`needs_sync` 是 `Entity.needsSync`，
    /// `sync_velocity` 是 `Entity.syncVelocity`（`hurtMarked`）。
    ///
    /// All packets of vanilla `ServerEntity.sendChanges` for one tick, in send
    /// order, as "id + body" bytes. `needs_sync` is `Entity.needsSync` and
    /// `sync_velocity` is `Entity.syncVelocity` (`hurtMarked`).
    pub fn step(
        &mut self,
        snapshot: &Snapshot,
        needs_sync: bool,
        sync_velocity: bool,
    ) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();
        // `SteppedInterpolationTracker.updateTracking` 每刻都加一，`getPositionPath`
        // 在发位置包时把攒下的刻数记成一步，然后清空。
        if !snapshot.data.is_projectile() {
            self.ticks_since_step += 1;
        }
        if snapshot.sync_position {
            self.ticks_since_step = 0;
        }
        let interval = update_interval(snapshot.entity_type);
        // `syncPosition` 时先把 tickCount 对齐到下一个周期，官方在周期判定之前做。
        // Vanilla aligns tickCount to the next interval before testing it.
        if let (true, Some(period)) = (snapshot.sync_position, interval) {
            self.tick_count = self.tick_count / period * period + period;
        }
        let due = interval.is_some_and(|period| self.tick_count % period == 0);
        let data = snapshot.data.pairing_values();
        let dirty = data != self.last_data;
        let mut head_packet = None;
        if needs_sync || due || dirty {
            let yaw = pack_degrees(snapshot.yaw);
            let pitch = snapshot.data.mob_x_rot().map_or(0, pack_degrees);
            let rotation_changed = yaw != self.last_yaw || pitch != self.last_pitch;
            self.teleport_delay += 1;
            let movement = self.choose_movement(snapshot, rotation_changed);
            // `trackDelta` 为真、或者生物在滑翔、或者 needsSync 时都查速度。
            // Vanilla checks velocity for trackDelta, fall-flying mobs, or needsSync.
            // 官方：`!needsSync && !trackDelta && !fallFlying` 时才跳过速度检查。
            // Vanilla skips the velocity check only for `!needsSync && !trackDelta && !fallFlying`.
            if needs_sync || snapshot.track_deltas() || snapshot.fall_flying {
                let diff = snapshot.delta.distance_to_sqr(self.last_movement);
                if diff > MOTION_TOLERANCE || (diff > 0.0 && snapshot.delta.length_sqr() == 0.0) {
                    self.last_movement = snapshot.delta;
                    packets.push(packet_body(&SetEntityMotion {
                        entity_id: snapshot.id,
                        movement: snapshot.delta,
                    }));
                }
            }
            if let Some(movement) = movement {
                packets.push(self.movement_body(snapshot, movement, yaw, pitch));
                if movement.has_position() {
                    self.base = snapshot.position;
                }
                if movement.has_rotation() {
                    self.last_yaw = yaw;
                    self.last_pitch = pitch;
                }
            }
            // `sendDirtyEntityData` 排在移动包后面；没有移动包时也照样发。
            // `sendDirtyEntityData` follows the movement packet, or stands alone.
            let mut items = if dirty {
                self.dirty_items(snapshot)
            } else {
                Vec::new()
            };
            for item in std::mem::take(&mut self.pending_dirty) {
                if !items
                    .iter()
                    .any(|other| EntityData::index(other) == EntityData::index(&item))
                {
                    items.push(item);
                }
            }
            // `packDirty` 按 ID 顺序遍历，补进来的脏项也要排进去。
            // `packDirty` walks the IDs in order, including the forced items.
            items.sort_by_key(EntityData::index);
            self.last_data = data;
            if !items.is_empty() {
                packets.push(packet_body(&SetEntityData {
                    entity_id: snapshot.id,
                    items: &items,
                }));
            }
            // `sendDirtyEntityData` 里紧接着发 `update_attributes`。
            packets.extend(self.attribute_packets(snapshot));
            // 官方每个周期都 `interpolationTracker.clear()`，把攒下的刻数清零。
            self.ticks_since_step = 0;
        }
        // 官方只在周期分支里查头部朝向；`Math.abs(byte差)` 等于打包字节不等。
        // Vanilla checks head rotation only inside the cycle branch.
        let head = snapshot.data.mob_head_rot().map_or(0, pack_degrees);
        if (needs_sync || due || dirty) && head != self.last_head {
            self.last_head = head;
            head_packet = Some(packet_body(&RotateHead {
                entity_id: snapshot.id,
                head_yaw: head,
            }));
        }
        if let Some(packet) = head_packet {
            packets.push(packet);
        }
        self.tick_count += 1;
        if sync_velocity {
            // 箭的 syncVelocity 来自 markHurt；官方这个分支不更新常规速度基线。
            // Arrow syncVelocity comes from markHurt; vanilla leaves its regular velocity baseline intact.
            if !snapshot.data.is_projectile() {
                self.last_movement = snapshot.delta;
            }
            packets.push(packet_body(&SetEntityMotion {
                entity_id: snapshot.id,
                movement: snapshot.delta,
            }));
        }
        packets
    }

    /// 只发变了的元数据；`last_data` 是上一刻发出去的非默认值。
    /// 官方 `packDirty` 的关键是：从「非默认」变回「默认」也要发一条默认值
    /// （例如小牛长大时 index 16 发 `false`），所以按 index 做并集比较。
    ///
    /// Only changed metadata is sent. Crucially, vanilla's `packDirty` also
    /// sends the default value when an item goes from non-default back to
    /// default (e.g. a baby growing up sends `false` at index 16), so this is
    /// an index-based union diff rather than a diff of non-default lists.
    fn dirty_items(&self, snapshot: &Snapshot) -> Vec<EntityData> {
        let current = snapshot.data.pairing_values();
        let mut indices: Vec<u8> = current
            .iter()
            .chain(&self.last_data)
            .map(EntityData::index)
            .collect();
        indices.sort_unstable();
        indices.dedup();
        let mut out = Vec::new();
        for index in indices {
            let now = current.iter().find(|item| EntityData::index(item) == index);
            let before = self
                .last_data
                .iter()
                .find(|item| EntityData::index(item) == index);
            let item = match now {
                Some(item) if Some(item) != before => item.clone(),
                None if matches!(snapshot.data, KindData::Arrow(_)) => match index {
                    0 => EntityData::SharedFlags(0),
                    5 => EntityData::NoGravity(false),
                    entity_types::ARROW_FLAGS => EntityData::ArrowFlags(0),
                    entity_types::ARROW_PIERCE => EntityData::ArrowPierce(0),
                    entity_types::ARROW_IN_GROUND => EntityData::ArrowInGround(false),
                    entity_types::ARROW_COLOR => EntityData::ArrowColor(-1),
                    _ => continue,
                },
                None if before.is_some() => default_data_for_type(
                    index,
                    match &snapshot.data {
                        KindData::Mob(mob) => mob.mob_type,
                        _ => mc_bamboo_blocks::entity::mob::MobType::Cow,
                    },
                ),
                _ => continue,
            };
            out.push(item);
        }
        out
    }

    /// 把选中的移动包写成「ID + 包体」字节。
    fn movement_body(
        &mut self,
        snapshot: &Snapshot,
        movement: Movement,
        yaw: u8,
        pitch: u8,
    ) -> Vec<u8> {
        match movement {
            Movement::Sync => packet_body(&EntityPositionSync {
                entity_id: snapshot.id,
                path: self.position_path(snapshot.position),
                yaw: snapshot.yaw,
                pitch: snapshot.data.mob_x_rot().unwrap_or(0.0),
                on_ground: snapshot.on_ground,
            }),
            Movement::Position(delta) => packet_body(&MoveEntityPos {
                entity_id: snapshot.id,
                delta: self.vec_delta(delta),
                on_ground: snapshot.on_ground,
            }),
            Movement::PositionRotation(delta) => packet_body(&MoveEntityPosRot {
                entity_id: snapshot.id,
                delta: self.vec_delta(delta),
                yaw,
                pitch,
                on_ground: snapshot.on_ground,
            }),
            Movement::Rotation => packet_body(&MoveEntityRot {
                entity_id: snapshot.id,
                yaw,
                pitch,
                on_ground: snapshot.on_ground,
            }),
        }
    }

    /// `SteppedInterpolationTracker` 的一步：`ticks` 是攒下来的刻数。
    /// 没有攒够一步时是 `Linear`（stepCount 0）。
    /// `sendDirtyEntityData` 里的 `getAttributesToSync`：把变了的可同步属性
    /// 合成一个 `update_attributes` 包。
    fn attribute_packets(&mut self, snapshot: &Snapshot) -> Vec<Vec<u8>> {
        let KindData::Mob(mob) = &snapshot.data else {
            return Vec::new();
        };
        let mut current = Vec::new();
        for entry in &mob.attributes.entries {
            if !entry.instantiated || !is_client_syncable_attribute(entry.id) {
                continue;
            }
            let modifiers = entry
                .modifiers
                .iter()
                .map(|modifier| {
                    let operation = match modifier.operation {
                        mc_bamboo_blocks::entity::mob::attributes::Operation::AddValue => 0,
                        mc_bamboo_blocks::entity::mob::attributes::Operation::AddMultipliedBase => 1,
                        mc_bamboo_blocks::entity::mob::attributes::Operation::AddMultipliedTotal => 2,
                    };
                    (modifier.id.clone(), modifier.amount, operation)
                })
                .collect();
            current.push((entry.id, entry.base, modifiers, entry.sync_version));
        }
        if current == self.last_attributes {
            return Vec::new();
        }
        let changed: Vec<i32> = current
            .iter()
            .filter(|entry| {
                !self.last_attributes.iter().any(|previous| {
                    previous.0 == entry.0
                        && previous.1 == entry.1
                        && previous.2 == entry.2
                        && previous.3 == entry.3
                })
            })
            .map(|entry| entry.0)
            .collect();
        self.last_attributes = current.clone();
        if changed.is_empty() {
            return Vec::new();
        }
        let mut snapshots = Vec::new();
        let mut modifiers: Vec<Vec<AttributeModifier<'_>>> = Vec::new();
        for (id, base, list, _) in &current {
            if !changed.contains(id) {
                continue;
            }
            modifiers.push(
                list.iter()
                    .map(|(id, amount, operation)| AttributeModifier {
                        id,
                        amount: *amount,
                        operation: match operation {
                            0 => AttributeOperation::AddValue,
                            1 => AttributeOperation::AddMultipliedBase,
                            _ => AttributeOperation::AddMultipliedTotal,
                        },
                    })
                    .collect(),
            );
            snapshots.push((*id, *base));
        }
        let attributes: Vec<AttributeSnapshot<'_>> = snapshots
            .iter()
            .zip(&modifiers)
            .map(|(&(id, base), list)| AttributeSnapshot {
                attribute: id,
                base,
                modifiers: list,
            })
            .collect();
        vec![packet_body(&UpdateAttributes {
            entity_id: snapshot.id,
            attributes: &attributes,
        })]
    }

    fn vec_delta(&self, delta: PositionDelta) -> VecDelta {
        if self.ticks_since_step > 0 {
            VecDelta::Stepped {
                ticks: self.ticks_since_step,
                delta,
            }
        } else {
            VecDelta::Linear(delta)
        }
    }

    fn position_path(&self, position: Vec3) -> PositionPath {
        if self.ticks_since_step > 0 {
            PositionPath::Stepped {
                position,
                ticks: self.ticks_since_step,
            }
        } else {
            PositionPath::Linear(position)
        }
    }

    /// `ServerEntity.createMovePacket`。
    fn choose_movement(&mut self, snapshot: &Snapshot, rotation_changed: bool) -> Option<Movement> {
        if snapshot.precise
            || self.teleport_delay > FORCED_TELEPORT_PERIOD
            || self.was_on_ground != snapshot.on_ground
        {
            self.was_on_ground = snapshot.on_ground;
            self.teleport_delay = 0;
            return Some(Movement::Sync);
        }
        let arrow = matches!(snapshot.data, KindData::Arrow(_));
        let rotation_changed = arrow || rotation_changed;
        let send_position = arrow
            || has_moved(self.base, snapshot.position)
            || self.tick_count % FORCED_POS_UPDATE_PERIOD == 0;
        if !send_position {
            return rotation_changed.then_some(Movement::Rotation);
        }
        Some(match PositionDelta::between(self.base, snapshot.position) {
            Some(delta) if rotation_changed => Movement::PositionRotation(delta),
            Some(delta) => Movement::Position(delta),
            None => Movement::Sync,
        })
    }
}

/// 这一 tick 里实体的样子。借用检查器不允许一边读实体一边写链路，先抄出来。
/// 逐刻对拍也用它来喂 [`TrackedEntity::step`]。
///
/// One tick's view of an entity. The oracle feeds the same snapshot to
/// [`TrackedEntity::step`].
#[derive(Debug, Clone)]
pub struct Snapshot {
    id: i32,
    entity_type: i32,
    position: Vec3,
    delta: Vec3,
    on_ground: bool,
    /// `Entity.getRequiresPrecisePosition()`：撞到过东西，这一刻必须发全量同步。
    pub precise: bool,
    /// `yRot`。物品构造时取一次，下落方块和 TNT 一直是 0。
    ///
    /// `yRot`. An item draws it once. Falling blocks and TNT stay at 0.
    yaw: f32,
    chunk: ChunkPos,
    data: KindData,
    /// `Entity.needsSync`：这一刻被推过或传送过。逐刻对拍从官方一侧传进来。
    ///
    /// `Entity.needsSync`: pushed or teleported this tick. The oracle passes the
    /// official value in.
    pub needs_sync: bool,
    /// `Entity.syncVelocity`（`markHurt`）：速度包要发给观察者加自己。
    ///
    /// `Entity.syncVelocity` (`markHurt`): a velocity packet to viewers and self.
    pub sync_velocity: bool,
    /// `Entity.syncPosition`：这一刻周期对齐过。
    pub sync_position: bool,
    /// `LivingEntity.isFallFlying()`：为真时即使 `trackDelta` 为假也查速度。
    pub fall_flying: bool,
    /// 骑在上面的玩家，官方的 `Entity.getPassengers()`。
    ///
    /// The players riding this entity, vanilla `Entity.getPassengers()`.
    pub passengers: Vec<i32>,
    /// 骑着的载具，官方的 `Entity.getVehicle()`；配对时乘客也要发载具的名单。
    ///
    /// The ridden vehicle, vanilla `Entity.getVehicle()`; pairing a passenger
    /// also sends the vehicle's passenger list.
    pub vehicle: Option<i32>,
}

/// 配对和同步时要发的、按实体类型不同的元数据。
#[derive(Debug, Clone, PartialEq)]
pub enum KindData {
    FallingBlock {
        state: BlockState,
        start: BlockPos,
    },
    Tnt {
        fuse: i32,
    },
    Item(ItemStack),
    /// `ExperienceOrb.DATA_VALUE`：非默认的经验值。
    ExperienceOrb {
        value: i32,
    },
    Mob(Box<mc_bamboo_blocks::entity::mob::MobPublic>),
    Arrow(mc_bamboo_blocks::entity::arrow::ArrowPublic),
    /// 闪电没有可同步的元数据；客户端从生成包拿类型，闪光种子走实体位置上的种子字段。
    ///
    /// A lightning bolt has no synced metadata; the client reads the seed from the entity.
    LightningBolt,
    /// 羊驼口水：没有自己的元数据。
    ///
    /// Llama spit, with no metadata of its own.
    LlamaSpit(mc_bamboo_blocks::entity::spit::LlamaSpitPublic),
    /// 拴绳结：没有自己的元数据。
    ///
    /// A fence leash knot, with no metadata of its own.
    LeashKnot,
    Egg(Box<mc_bamboo_blocks::entity::thrown_egg::EggPublic>),
    SplashPotion(Box<mc_bamboo_blocks::entity::splash_potion::SplashPotionPublic>),
}

/// 这一 tick 全表的载具名单。配对乘客时官方发的是载具那一侧的完整名单，而乘客
/// 自己的快照里只有载具 ID。
///
/// A whole-tick index of vehicle passenger lists. Pairing a passenger sends the
/// vehicle's complete list, which the passenger's own snapshot cannot see.
#[derive(Debug, Clone, Default)]
pub struct PassengerIndex {
    lists: std::collections::BTreeMap<i32, Vec<i32>>,
}

impl PassengerIndex {
    pub fn of(snapshots: &[Snapshot]) -> Self {
        Self {
            lists: snapshots
                .iter()
                .map(|snapshot| (snapshot.id, snapshot.passengers.clone()))
                .collect(),
        }
    }

    pub(crate) fn list(&self, vehicle: i32) -> Option<&[i32]> {
        self.lists.get(&vehicle).map(Vec::as_slice)
    }
}

impl Snapshot {
    /// 从公开数据抄一份。字段和 [`Entity::split`](mc_bamboo_blocks::entity::Entity::split)
    /// 的 public 半边一一对应，不克隆私有状态、随机数、移动记录和物品堆以外的东西。
    pub fn of(public: &EntityPublic) -> Self {
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
            needs_sync: match &public.kind {
                KindPublic::Arrow(arrow) => arrow.needs_sync,
                KindPublic::Mob(mob) => mob.needs_sync,
                KindPublic::LlamaSpit(spit) => spit.needs_sync,
                KindPublic::Egg(egg) => egg.needs_sync,
                KindPublic::SplashPotion(potion) => potion.needs_sync,
                _ => false,
            },
            sync_velocity: match &public.kind {
                KindPublic::Arrow(arrow) => arrow.sync_velocity,
                KindPublic::Egg(egg) => egg.sync_velocity,
                KindPublic::SplashPotion(potion) => potion.sync_velocity,
                _ => false,
            },
            sync_position: false,
            fall_flying: false,
            passengers: match &public.kind {
                KindPublic::Mob(mob) => mob.passengers.clone(),
                _ => Vec::new(),
            },
            vehicle: public.vehicle,
        }
    }

    /// `getAddEntityPacket` 的 `data`：下落的方块是它的方块状态，TNT 和物品是 0。
    fn add_data(&self) -> i32 {
        match &self.data {
            KindData::FallingBlock { state, .. } => state.0 as i32,
            KindData::Arrow(arrow) => arrow.owner.unwrap_or(0),
            KindData::LlamaSpit(spit) => spit.owner.unwrap_or(0),
            KindData::Egg(egg) => egg.owner.unwrap_or(0),
            KindData::SplashPotion(potion) => potion.owner.unwrap_or(0),
            KindData::Tnt { .. }
            | KindData::Item(_)
            | KindData::ExperienceOrb { .. }
            | KindData::Mob(_)
            | KindData::LightningBolt
            | KindData::LeashKnot => 0,
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
            KindPublic::ExperienceOrb { value } => Self::ExperienceOrb { value: *value },
            KindPublic::Mob(mob) => Self::Mob(mob.clone()),
            KindPublic::Arrow(arrow) => Self::Arrow(*arrow),
            KindPublic::LlamaSpit(spit) => Self::LlamaSpit(*spit),
            KindPublic::Egg(egg) => Self::Egg(egg.clone()),
            KindPublic::SplashPotion(potion) => Self::SplashPotion(potion.clone()),
            KindPublic::LightningBolt(_) => Self::LightningBolt,
            KindPublic::LeashKnot { .. } => Self::LeashKnot,
        }
    }

    /// `InterpolationHandler.NO_OP` 的弹射物：位置包不带攒下的步数，`syncVelocity` 不动速度基线。
    ///
    /// Projectiles with a no-op interpolation handler: no stepped position paths, and
    /// `syncVelocity` leaves the velocity baseline alone.
    fn is_projectile(&self) -> bool {
        matches!(
            self,
            Self::Arrow(_) | Self::Egg(_) | Self::LlamaSpit(_) | Self::SplashPotion(_)
        )
    }

    /// 同步用到的那个数字。下落的方块和物品没有。
    fn fuse(&self) -> Option<i32> {
        match self {
            Self::FallingBlock { .. }
            | Self::Item(_)
            | Self::ExperienceOrb { .. }
            | Self::Mob(_)
            | Self::Arrow(_)
            | Self::LlamaSpit(_)
            | Self::Egg(_)
            | Self::SplashPotion(_)
            | Self::LightningBolt
            | Self::LeashKnot => None,
            Self::Tnt { fuse } => Some(*fuse),
        }
    }

    fn stack(&self) -> Option<&ItemStack> {
        match self {
            Self::Item(stack) => Some(stack),
            Self::FallingBlock { .. }
            | Self::Tnt { .. }
            | Self::ExperienceOrb { .. }
            | Self::Mob(_)
            | Self::Arrow(_)
            | Self::LlamaSpit(_)
            | Self::Egg(_)
            | Self::SplashPotion(_)
            | Self::LightningBolt
            | Self::LeashKnot => None,
        }
    }

    /// 生物自己的 UUID；其它三类按网络 ID 派生。
    fn stable_uuid(&self) -> Option<[u8; 16]> {
        match self {
            Self::Mob(mob) => Some(mob.uuid),
            Self::Arrow(arrow) => Some(arrow.uuid),
            Self::LlamaSpit(spit) => Some(spit.uuid),
            Self::Egg(egg) => Some(egg.uuid),
            Self::SplashPotion(potion) => Some(potion.uuid),
            _ => None,
        }
    }

    /// `getXRot()`。只有生物有俯仰角。
    fn mob_x_rot(&self) -> Option<f32> {
        match self {
            Self::Mob(mob) => Some(mob.x_rot),
            Self::Arrow(arrow) => Some(arrow.pitch),
            Self::LlamaSpit(spit) => Some(spit.pitch),
            Self::Egg(egg) => Some(egg.x_rot),
            Self::SplashPotion(potion) => Some(potion.x_rot),
            _ => None,
        }
    }

    /// `getYHeadRot()`。只有生物有独立的头部朝向。
    fn mob_head_rot(&self) -> Option<f32> {
        match self {
            Self::Mob(mob) => Some(mob.y_head_rot),
            _ => None,
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
            Self::Item(_) => Vec::new(),
            Self::ExperienceOrb { value } if *value != 0 => {
                vec![EntityData::ExperienceValue(*value)]
            }
            Self::ExperienceOrb { .. }
            | Self::LlamaSpit(_)
            | Self::LightningBolt
            | Self::LeashKnot => Vec::new(),
            Self::Arrow(arrow) => {
                let mut data = Vec::new();
                if arrow.on_fire {
                    data.push(EntityData::SharedFlags(1));
                }
                if arrow.no_gravity {
                    data.push(EntityData::NoGravity(true));
                }
                // `AbstractArrow.ID_FLAGS`：crit 是 bit 0，noPhysics 是 bit 1
                // （`setNoPhysics` 用 setFlag(2, ...)）。忠诚返回时两者都要同步。
                // `AbstractArrow.ID_FLAGS`: crit is bit 0, noPhysics is bit 1
                // (`setNoPhysics` uses setFlag(2, ...)); both must be synced
                // while a loyal trident returns.
                let flags = u8::from(arrow.critical) | (u8::from(arrow.no_physics) << 1);
                if flags != 0 {
                    data.push(EntityData::ArrowFlags(flags));
                }
                if arrow.pierce != 0 {
                    data.push(EntityData::ArrowPierce(arrow.pierce));
                }
                if arrow.in_ground {
                    data.push(EntityData::ArrowInGround(true));
                }
                if matches!(
                    arrow.variant,
                    mc_bamboo_blocks::entity::arrow::ArrowVariant::Trident
                ) {
                    // `ThrownTrident` 的 11/12 是忠诚和附魔光效，不是药水箭的颜色。
                    // A trident's 11/12 are loyalty and foil, not the tipped-arrow colour.
                    if arrow.loyalty != 0 {
                        data.push(EntityData::TridentLoyalty(arrow.loyalty));
                    }
                    if arrow.foil {
                        data.push(EntityData::TridentFoil(true));
                    }
                } else if arrow.color != -1 {
                    data.push(EntityData::ArrowColor(arrow.color));
                }
                data
            }
            Self::Mob(mob) => {
                if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Creaking {
                    creaking_non_default_data(mob)
                } else if mob.mob_type.is_monster() {
                    monster_non_default_data(mob)
                } else {
                    mob_non_default_data(mob)
                }
            }
            Self::Egg(egg) => {
                let mut data = Vec::new();
                if egg.on_fire {
                    data.push(EntityData::SharedFlags(1));
                }
                if egg.no_gravity {
                    data.push(EntityData::NoGravity(true));
                }
                data.push(EntityData::Item(egg.item.clone()));
                data
            }
            Self::SplashPotion(potion) => {
                let mut data = Vec::new();
                if potion.on_fire {
                    data.push(EntityData::SharedFlags(1));
                }
                if potion.no_gravity {
                    data.push(EntityData::NoGravity(true));
                }
                data.push(EntityData::Item(potion.item.clone()));
                data
            }
        }
    }
}

/// 某个元数据 index 的默认值，用于「从非默认变回默认」时发一条。
/// 牛能变成非默认的只有这些项，其余 index 官方不会走到这里。
///
/// The default value of a metadata index, sent when an item returns to its
/// default. Only these cow indices can become non-default.
fn default_data_for_type(index: u8, kind: mc_bamboo_blocks::entity::mob::MobType) -> EntityData {
    use mc_bamboo_data::entity_types as facts;
    if kind == mc_bamboo_blocks::entity::mob::MobType::Creaking {
        return match index {
            facts::CREAKING_CAN_MOVE => EntityData::CreakingCanMove(true),
            facts::CREAKING_IS_ACTIVE => EntityData::CreakingActive(false),
            facts::CREAKING_IS_TEARING_DOWN => EntityData::CreakingTearingDown(false),
            facts::CREAKING_HOME_POS => EntityData::CreakingHomePos(None),
            _ => EntityData::SharedFlags(0),
        };
    }
    // `Warden.CLIENT_ANGER_LEVEL` 占 index 16 的 INT（不是 `AgeableMob.Baby` 的 BOOL）。
    // `Warden.CLIENT_ANGER_LEVEL` is the INT at index 16 (not `AgeableMob.Baby`'s BOOL).
    if kind == mc_bamboo_blocks::entity::mob::MobType::Warden && index == 16 {
        return EntityData::ClientAngerLevel(0);
    }
    if kind == mc_bamboo_blocks::entity::mob::MobType::ZombieVillager {
        use mc_bamboo_data::villager;
        return match index {
            villager::ZOMBIE_VILLAGER_CONVERTING_INDEX => {
                EntityData::ZombieVillagerConverting(false)
            }
            villager::ZOMBIE_VILLAGER_DATA_INDEX => EntityData::ZombieVillagerData {
                kind: villager::type_id(villager::DEFAULT_TYPE).unwrap_or(0),
                profession: villager::profession_id(villager::NONE_PROFESSION).unwrap_or(0),
                level: villager::MIN_VILLAGER_LEVEL,
            },
            villager::ZOMBIE_VILLAGER_DATA_FINALIZED_INDEX => {
                EntityData::ZombieVillagerDataFinalized(false)
            }
            _ => EntityData::SharedFlags(0),
        };
    }
    if kind == mc_bamboo_blocks::entity::mob::MobType::Villager {
        use mc_bamboo_data::villager;
        match index {
            villager::UNHAPPY_COUNTER_INDEX => return EntityData::VillagerUnhappy(0),
            villager::VILLAGER_DATA_INDEX => {
                return EntityData::VillagerData {
                    kind: villager::type_id(villager::DEFAULT_TYPE).unwrap_or(0),
                    profession: villager::profession_id(villager::NONE_PROFESSION).unwrap_or(0),
                    level: villager::MIN_VILLAGER_LEVEL,
                };
            }
            villager::VILLAGER_DATA_FINALIZED_INDEX => {
                return EntityData::VillagerDataFinalized(false);
            }
            _ => {}
        }
    }
    if kind == mc_bamboo_blocks::entity::mob::MobType::Bee {
        return match index {
            facts::BEE_FLAGS => EntityData::BeeFlags(0),
            facts::BEE_ANGER_END_TIME => EntityData::BeeAngerEndTime(-1),
            _ => EntityData::MobFlags(0),
        };
    }
    if kind == mc_bamboo_blocks::entity::mob::MobType::Wolf
        && let Some(data) = wolf_default_data(index)
    {
        return data;
    }
    if kind.is_equine() && index >= mc_bamboo_data::entity_types::HORSE_FLAGS {
        return equine_default_data(index, kind);
    }
    if kind == mc_bamboo_blocks::entity::mob::MobType::Armadillo
        && index == mc_bamboo_data::entity_types::ARMADILLO_STATE
    {
        return EntityData::ArmadilloState(0);
    }
    if kind == mc_bamboo_blocks::entity::mob::MobType::Frog {
        use mc_bamboo_data::entity_types as facts;
        match index {
            facts::FROG_VARIANT => {
                let variant = mc_bamboo_blocks::entity::mob::frog::Variant::default();
                return EntityData::FrogVariant(variant.network_id());
            }
            facts::FROG_TONGUE_TARGET => return EntityData::FrogTongueTarget(None),
            _ => {}
        }
    }
    if kind == mc_bamboo_blocks::entity::mob::MobType::Goat {
        use mc_bamboo_data::entity_types as facts;
        match index {
            facts::GOAT_SCREAMING => return EntityData::GoatScreaming(false),
            facts::GOAT_LEFT_HORN => return EntityData::GoatLeftHorn(true),
            facts::GOAT_RIGHT_HORN => return EntityData::GoatRightHorn(true),
            _ => {}
        }
    }
    if mc_bamboo_blocks::entity::mob::guardian::is_guardian(kind) {
        use mc_bamboo_data::entity_types as facts;
        match index {
            facts::GUARDIAN_MOVING => return EntityData::GuardianMoving(false),
            facts::GUARDIAN_ATTACK_TARGET => return EntityData::GuardianAttackTarget(0),
            _ => {}
        }
    }
    if matches!(
        kind,
        mc_bamboo_blocks::entity::mob::MobType::IronGolem
            | mc_bamboo_blocks::entity::mob::MobType::SnowGolem
    ) {
        use mc_bamboo_data::entity_types as facts;
        let default = if kind == mc_bamboo_blocks::entity::mob::MobType::SnowGolem {
            16_u8
        } else {
            0
        };
        if index == facts::IRON_GOLEM_FLAGS {
            return EntityData::GolemFlags(default);
        }
    }
    if let Some(data) = farm_default_data(index, kind) {
        return data;
    }
    if mc_bamboo_blocks::entity::mob::nautilus::is_nautilus(kind) {
        use mc_bamboo_data::entity_types as facts;
        match index {
            facts::NAUTILUS_DASH => return EntityData::NautilusDash(false),
            facts::ZOMBIE_NAUTILUS_VARIANT => {
                return EntityData::ZombieNautilusVariant(0);
            }
            facts::TAME_FLAGS => return EntityData::TameFlags(0),
            facts::TAME_OWNER => return EntityData::TameOwner(None),
            _ => {}
        }
    }
    if let Some(data) = fish_default_data(index, kind) {
        return data;
    }
    // 海豚 / 美西螈的 18、19、20 是它们自己的元数据，不是牛的花色和叫声。
    // A dolphin / axolotl's 18, 19, 20 are their own accessors, not cow variants.
    use mc_bamboo_blocks::entity::mob::MobType;
    use mc_bamboo_data::entity_types as water_facts;
    match (kind, index) {
        (MobType::Dolphin, water_facts::DOLPHIN_GOT_FISH) => {
            return EntityData::DolphinGotFish(false);
        }
        (MobType::Dolphin, water_facts::DOLPHIN_MOISTNESS) => {
            return EntityData::DolphinMoistness(2400);
        }
        (MobType::Axolotl, water_facts::AXOLOTL_VARIANT) => {
            return EntityData::AxolotlVariant(0);
        }
        (MobType::Axolotl, water_facts::AXOLOTL_PLAYING_DEAD) => {
            return EntityData::AxolotlPlayingDead(false);
        }
        (MobType::Axolotl, water_facts::AXOLOTL_FROM_BUCKET) => {
            return EntityData::AxolotlFromBucket(false);
        }
        _ => {}
    }
    // 嗅探兽的 18、19 是 `DATA_STATE`（缺省 IDLING=0）和 `DATA_DROP_SEED_AT_TICK`
    // （缺省 0）；从非默认变回默认时发这两条。
    // A sniffer's 18 and 19 are `DATA_STATE` (default IDLING=0) and
    // `DATA_DROP_SEED_AT_TICK` (default 0); send these when they flip back.
    if kind == mc_bamboo_blocks::entity::mob::MobType::Sniffer {
        use mc_bamboo_data::entity_types as facts;
        match index {
            facts::SNIFFER_STATE => return EntityData::SnifferState(0),
            facts::SNIFFER_DROP_SEED_AT_TICK => {
                return EntityData::SnifferDropSeedAtTick(0);
            }
            _ => {}
        }
    }
    // 海龟的 18、19 是「有蛋」「正在产卵」两个布尔，不是牛的花色和叫声；
    // 从 true 变回 false 时要发这两条默认值。
    // A turtle's 18 and 19 are the has-egg / laying-egg booleans, not the cow
    // variants; send these defaults when either flips back to false.
    if kind == mc_bamboo_blocks::entity::mob::MobType::Turtle {
        use mc_bamboo_data::entity_types as facts;
        match index {
            facts::TURTLE_HAS_EGG => return EntityData::TurtleHasEgg(false),
            facts::TURTLE_LAYING_EGG => return EntityData::TurtleLayingEgg(false),
            _ => {}
        }
    }
    if kind == mc_bamboo_blocks::entity::mob::MobType::Pig {
        use mc_bamboo_data::entity_types as facts;
        match index {
            facts::PIG_BOOST_TIME => return EntityData::PigBoostTime(0),
            facts::PIG_VARIANT => {
                return EntityData::PigVariant(pig_variant_id("minecraft:temperate"));
            }
            facts::PIG_SOUND_VARIANT => {
                return EntityData::PigSoundVariant(pig_sound_variant_default());
            }
            _ => {}
        }
    }
    if kind == mc_bamboo_blocks::entity::mob::MobType::Enderman {
        use mc_bamboo_data::entity_types as facts;
        match index {
            facts::ENDERMAN_CARRY_STATE => return EntityData::EndermanCarried(None),
            facts::ENDERMAN_CREEPY => return EntityData::EndermanCreepy(false),
            facts::ENDERMAN_STARED_AT => return EntityData::EndermanStaredAt(false),
            _ => {}
        }
    }
    if kind == mc_bamboo_blocks::entity::mob::MobType::Creeper {
        use mc_bamboo_data::entity_types as facts;
        match index {
            facts::CREEPER_SWELL_DIR => return EntityData::CreeperSwellDir(-1),
            facts::CREEPER_POWERED => return EntityData::CreeperPowered(false),
            facts::CREEPER_IGNITED => return EntityData::CreeperIgnited(false),
            _ => {}
        }
    }
    if kind.climbs_walls() {
        use mc_bamboo_data::entity_types as facts;
        if index == facts::SPIDER_FLAGS {
            return EntityData::SpiderFlags(0);
        }
    }
    match (index, kind.uses_baby_metadata()) {
        (16, true) => EntityData::Baby(false),
        // 女巫的 index 17 是 `DATA_USING_ITEM`（BOOLEAN），不是僵尸的特殊类型。
        //
        // The witch's index 17 is `DATA_USING_ITEM` (a boolean), not the zombie special type.
        (17, true) if kind == mc_bamboo_blocks::entity::mob::MobType::Witch => {
            EntityData::WitchUsingItem(false)
        }
        (17, true) => EntityData::SpecialType(0),
        (18, true) => EntityData::DrownedConverting(false),
        _ => match index {
            0 => EntityData::SharedFlags(0),
            1 => EntityData::AirSupply(300),
            3 => EntityData::CustomNameVisible(false),
            4 => EntityData::Silent(false),
            5 => EntityData::NoGravity(false),
            6 => EntityData::Pose(mc_bamboo_protocol::packet::play::Pose::Standing),
            7 => EntityData::TicksFrozen(0),
            8 => EntityData::LivingFlags(0),
            9 => EntityData::Health(1.0),
            14 => EntityData::SleepingPos(None),
            15 => EntityData::MobFlags(0),
            16 => EntityData::Baby(false),
            17 => EntityData::AgeLocked(false),
            entity_types::RABBIT_VARIANT
                if kind == mc_bamboo_blocks::entity::mob::MobType::Rabbit =>
            {
                EntityData::RabbitVariant(0)
            }
            entity_types::FOX_TYPE if kind == mc_bamboo_blocks::entity::mob::MobType::Fox => {
                EntityData::FoxType(0)
            }
            entity_types::FOX_FLAGS if kind == mc_bamboo_blocks::entity::mob::MobType::Fox => {
                EntityData::FoxFlags(0)
            }
            entity_types::FOX_TRUSTED_0 | entity_types::FOX_TRUSTED_1
                if kind == mc_bamboo_blocks::entity::mob::MobType::Fox =>
            {
                EntityData::FoxTrusted(index, None)
            }
            18 => EntityData::CowVariant(1),
            19 => EntityData::CowSoundVariant(0),
            // `CustomName` 是 optional component，空时写 boolean false。
            // `CustomName` is an optional component; absent is a false boolean.
            2 => EntityData::NoCustomName,
            _ => EntityData::SharedFlags(0),
        },
    }
}

/// 牛的全部非默认元数据，按 index 升序。官方 `SynchedEntityData.getNonDefaultValues`
/// 就是这个顺序；index 0 的标志字节、血量、幼年、花色、叫声都在里面。
///
/// Every non-default metadata item of a cow, in ascending index order. This is
/// vanilla `SynchedEntityData.getNonDefaultValues`, including shared flags,
/// health, the baby flag, variant and sound variant.
/// 僵尸/尸壳的元数据。官方 `ClassTreeIdRegistry` 按类树导出：
/// 16 `ZombieBaby`(BOOL)、17 `SpecialType`(INT)、18 `DrownedConverting`(BOOL)，
/// 和牛的 `AgeableMob.Baby`(16) 不是同一个字段。
/// `LivingEntity.DATA_EFFECT_PARTICLES` / `DATA_EFFECT_AMBIENCE_ID`：可见效果的颜色列表和
/// 是否有环境效果。只发有内容的项。
///
/// `LivingEntity.DATA_EFFECT_PARTICLES` / `DATA_EFFECT_AMBIENCE_ID`: the visible effects'
/// colours and whether any effect is ambient. Only non-empty values are sent.
fn effect_data(mob: &mc_bamboo_blocks::entity::mob::MobPublic, out: &mut Vec<EntityData>) {
    let colors: Vec<i32> = mob
        .status_effects
        .iter()
        .filter(|effect| effect.visible)
        .map(|effect| {
            // `DustParticleOptions` 把颜色补成不透明的 ARGB。
            // `DustParticleOptions` makes the colour opaque ARGB.
            mc_bamboo_data::potions::EFFECT_COLORS
                .get(effect.effect as usize)
                .copied()
                .unwrap_or(0)
                | -16_777_216
        })
        .collect();
    if !colors.is_empty() {
        out.push(EntityData::EffectParticles(colors));
    }
    if mob.status_effects.iter().any(|effect| effect.ambient) {
        out.push(EntityData::EffectAmbience(true));
    }
}

/// `Creaking` 四项元数据中偏离官方缺省值的部分。
fn creaking_non_default_data(mob: &mc_bamboo_blocks::entity::mob::MobPublic) -> Vec<EntityData> {
    let mut out = Vec::new();
    let state = &mob.creaking;
    if !state.can_move {
        out.push(EntityData::CreakingCanMove(false));
    }
    if state.active {
        out.push(EntityData::CreakingActive(true));
    }
    if state.tearing_down {
        out.push(EntityData::CreakingTearingDown(true));
    }
    if state.home_pos.is_some() {
        out.push(EntityData::CreakingHomePos(state.home_pos));
    }
    out
}

fn monster_non_default_data(mob: &mc_bamboo_blocks::entity::mob::MobPublic) -> Vec<EntityData> {
    use mc_bamboo_protocol::packet::play::Pose;
    let mut flags = 0u8;
    if mob.on_fire {
        flags |= 1;
    }
    if mob.glowing {
        flags |= 1 << 6;
    }
    // `Entity.FLAG_SWIMMING` = 1<<4。
    // `Entity.FLAG_SWIMMING` is 1<<4.
    if mob.swimming {
        flags |= 1 << 4;
    }
    let mut out = Vec::new();
    if flags != 0 {
        out.push(EntityData::SharedFlags(flags));
    }
    if mob.dying {
        out.push(EntityData::Pose(Pose::Dying));
    }
    if mob.air_supply != 300 {
        out.push(EntityData::AirSupply(mob.air_supply));
    }
    if let Some(name) = &mob.custom_name {
        out.push(EntityData::CustomName(name.clone()));
    }
    if mob.custom_name_visible {
        out.push(EntityData::CustomNameVisible(true));
    }
    if mob.silent {
        out.push(EntityData::Silent(true));
    }
    if mob.no_gravity {
        out.push(EntityData::NoGravity(true));
    }
    if mob.ticks_frozen != 0 {
        out.push(EntityData::TicksFrozen(mob.ticks_frozen));
    }
    out.push(EntityData::Health(mob.health));
    effect_data(mob, &mut out);
    if mob.using_item_flags != 0 {
        out.push(EntityData::LivingFlags(mob.using_item_flags));
    }
    if mob.sleeping_pos.is_some() {
        out.push(EntityData::SleepingPos(mob.sleeping_pos));
    }
    let mut mob_flags = 0u8;
    if mob.no_ai {
        mob_flags |= 1;
    }
    if mob.left_handed {
        mob_flags |= 2;
    }
    if mob.aggressive {
        mob_flags |= 4;
    }
    if mob_flags != 0 {
        out.push(EntityData::MobFlags(mob_flags));
    }
    // `Warden.CLIENT_ANGER_LEVEL`：非缺省值发 INT（缺省 0 走 default_data_for_type）。
    // `Warden.CLIENT_ANGER_LEVEL`: send the INT when non-default (0 goes to defaults).
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Warden && mob.client_anger_level != 0
    {
        out.push(EntityData::ClientAngerLevel(mob.client_anger_level));
    }
    // 骷髅的 `DATA_STRAY_CONVERSION_ID` 和沼骸的 `DATA_SHEARED` 都是 index 16 的 BOOLEAN，
    // 和这里的 `Baby` 字节完全相同；变回假时走 `default_data_for_type` 的 16 号缺省。
    // `Skeleton.DATA_STRAY_CONVERSION_ID` and `Bogged.DATA_SHEARED` are index-16 booleans,
    // byte-identical to `Baby`; the reset goes through `default_data_for_type`.
    if mob.zombie_baby || mob.skeleton_flag {
        out.push(EntityData::Baby(true));
    }
    if mob.zombie_special_type != 0 {
        out.push(EntityData::SpecialType(mob.zombie_special_type));
    }
    if mob.drowned_converting {
        out.push(EntityData::DrownedConverting(true));
    }
    // `Pillager.IS_CHARGING_CROSSBOW` 也是 index 17 的 BOOLEAN，和女巫那一项字节相同。
    // `Pillager.IS_CHARGING_CROSSBOW` is an index-17 boolean too, byte-identical to the
    // witch's item.
    if mob.witch_using_item || mob.pillager_charging {
        out.push(EntityData::WitchUsingItem(true));
    }
    if let Some(carried) = mob.enderman_carried {
        out.push(EntityData::EndermanCarried(Some(carried.0 as i32)));
    }
    if mob.enderman_creepy {
        out.push(EntityData::EndermanCreepy(true));
    }
    if mob.enderman_stared_at {
        out.push(EntityData::EndermanStaredAt(true));
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::ZombieNautilus {
        if mob.nautilus_variant != 0 {
            out.push(EntityData::ZombieNautilusVariant(mob.nautilus_variant));
        }
        if mob.nautilus_dashing {
            out.push(EntityData::NautilusDash(true));
        }
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Creeper {
        // `Creeper.defineSynchedData`：膨胀方向缺省 -1，带电和点燃缺省 false。
        // `Creeper.defineSynchedData`: swell direction defaults to -1, the flags to false.
        if mob.creeper_swell_dir != -1 {
            out.push(EntityData::CreeperSwellDir(mob.creeper_swell_dir));
        }
        if mob.creeper_powered {
            out.push(EntityData::CreeperPowered(true));
        }
        if mob.creeper_ignited {
            out.push(EntityData::CreeperIgnited(true));
        }
    }
    if mob.mob_type.climbs_walls() && mob.spider_flags != 0 {
        // `Spider.DATA_FLAGS_ID`：缺省 0，攀爬位是第 0 位。
        // `Spider.DATA_FLAGS_ID`: default 0, bit 0 is climbing.
        out.push(EntityData::SpiderFlags(mob.spider_flags));
    }
    out
}

/// 狐狸自己的四项元数据，按 index 升序：品种 18、标志 19、两个信任格 20 和 21。
///
/// The four fox items in index order: variant, flags and two trusted slots.
fn fox_non_default_data(mob: &mc_bamboo_blocks::entity::mob::MobPublic) -> Vec<EntityData> {
    let mut out = Vec::new();
    if mob.fox_variant != 0 {
        out.push(EntityData::FoxType(mob.fox_variant));
    }
    if mob.fox_flags != 0 {
        out.push(EntityData::FoxFlags(mob.fox_flags));
    }
    for (index, trusted) in [entity_types::FOX_TRUSTED_0, entity_types::FOX_TRUSTED_1]
        .into_iter()
        .zip(mob.fox_trusted)
    {
        if trusted.is_some() {
            out.push(EntityData::FoxTrusted(index, trusted));
        }
    }
    out
}

/// 马科自己那几项的默认值：标志 0、花色 0、没箱子、力量 0。
///
/// Defaults of the equine items: no flags, variant 0, no chest, strength 0.
fn equine_default_data(index: u8, kind: mc_bamboo_blocks::entity::mob::MobType) -> EntityData {
    use mc_bamboo_blocks::entity::mob::MobType;
    use mc_bamboo_data::entity_types::{HORSE_FLAGS, LLAMA_STRENGTH, LLAMA_VARIANT};
    match index {
        HORSE_FLAGS => EntityData::HorseFlags(0),
        LLAMA_STRENGTH => EntityData::LlamaStrength(0),
        LLAMA_VARIANT => EntityData::LlamaVariant(0),
        _ if kind == MobType::Horse => EntityData::HorseVariant(0),
        _ => EntityData::HorseChest(false),
    }
}

/// 马科在 `AgeableMob` 之后的元数据，按 index 升序：`AbstractHorse` 的标志，
/// 然后马的花色，或者箱子马的箱子、羊驼的力量和毛色。
///
/// Equine metadata after `AgeableMob`, by ascending index: the `AbstractHorse`
/// flags, then the horse variant, or the chest, llama strength and variant.
fn equine_non_default_data(
    kind: mc_bamboo_blocks::entity::mob::MobType,
    equine: &mc_bamboo_blocks::entity::mob::equine::EquinePublic,
) -> Vec<EntityData> {
    use mc_bamboo_blocks::entity::mob::MobType;
    let mut out = Vec::new();
    if equine.flags != 0 {
        out.push(EntityData::HorseFlags(equine.flags));
    }
    if kind == MobType::Horse {
        if equine.variant != 0 {
            out.push(EntityData::HorseVariant(equine.variant));
        }
        return out;
    }
    if equine.chested {
        out.push(EntityData::HorseChest(true));
    }
    if kind == MobType::Llama {
        if equine.strength != 0 {
            out.push(EntityData::LlamaStrength(equine.strength));
        }
        if equine.variant != 0 {
            out.push(EntityData::LlamaVariant(equine.variant));
        }
    }
    out
}

/// 狼（含 `TamableAnimal`）各项元数据的默认值：标志 0、没有主人、不歪头、红项圈、
/// 没有仇恨、苍白狼、经典叫声。
///
/// Wolf defaults, including the two `TamableAnimal` items.
fn wolf_default_data(index: u8) -> Option<EntityData> {
    use mc_bamboo_blocks::entity::mob::wolf;
    Some(match index {
        entity_types::TAME_FLAGS => EntityData::TameFlags(0),
        entity_types::TAME_OWNER => EntityData::TameOwner(None),
        entity_types::WOLF_INTERESTED => EntityData::WolfInterested(false),
        entity_types::WOLF_COLLAR => EntityData::WolfCollar(WOLF_DEFAULT_COLLAR),
        entity_types::WOLF_ANGER_END_TIME => EntityData::WolfAngerEndTime(-1),
        entity_types::WOLF_VARIANT => EntityData::WolfVariant(wolf::default_variant()),
        entity_types::WOLF_SOUND_VARIANT => {
            EntityData::WolfSoundVariant(wolf::default_sound_variant())
        }
        _ => return None,
    })
}

/// `Wolf.DEFAULT_COLLAR_COLOR`：`DyeColor.RED` 的 ID。
const WOLF_DEFAULT_COLLAR: i32 = 14;

/// 狼的非默认元数据，按 index 升序（18 到 24）。
///
/// The non-default wolf items in index order (18 to 24).
fn wolf_non_default_data(mob: &mc_bamboo_blocks::entity::mob::MobPublic) -> Vec<EntityData> {
    use mc_bamboo_blocks::entity::mob::wolf;
    let mut out = Vec::new();
    if mob.tame_flags != 0 {
        out.push(EntityData::TameFlags(mob.tame_flags));
    }
    if mob.owner.is_some() {
        out.push(EntityData::TameOwner(mob.owner));
    }
    if mob.wolf_interested {
        out.push(EntityData::WolfInterested(true));
    }
    if mob.wolf_collar != WOLF_DEFAULT_COLLAR {
        out.push(EntityData::WolfCollar(mob.wolf_collar));
    }
    if mob.wolf_anger_end_time != -1 {
        out.push(EntityData::WolfAngerEndTime(mob.wolf_anger_end_time));
    }
    if mob.wolf_variant != wolf::default_variant() {
        out.push(EntityData::WolfVariant(mob.wolf_variant));
    }
    if mob.wolf_sound_variant != wolf::default_sound_variant() {
        out.push(EntityData::WolfSoundVariant(mob.wolf_sound_variant));
    }
    out
}

fn mob_non_default_data(mob: &mc_bamboo_blocks::entity::mob::MobPublic) -> Vec<EntityData> {
    use mc_bamboo_protocol::packet::play::{
        ENTITY_FLAG_FALL_FLYING, ENTITY_FLAG_SHIFT_KEY_DOWN, ENTITY_FLAG_SPRINTING,
    };
    let mut flags = 0u8;
    // `FLAG_ONFIRE` = 1<<0，`FLAG_GLOWING` = 1<<6。死亡时官方把 `Pose` 设成
    // `DYING`（元数据 index 6），不是 SharedFlags 的滑翔位。
    // `FLAG_ONFIRE` is 1<<0 and `FLAG_GLOWING` is 1<<6. On death vanilla sets
    // the `Pose` metadata (index 6) to `DYING`, not the shared-flags bit.
    if mob.on_fire {
        flags |= 1;
    }
    if mob.sprinting {
        flags |= ENTITY_FLAG_SPRINTING;
    }
    if mob.glowing {
        flags |= 1 << 6;
    }
    let _ = (ENTITY_FLAG_FALL_FLYING, ENTITY_FLAG_SHIFT_KEY_DOWN);
    let mut out = Vec::new();
    if flags != 0 {
        out.push(EntityData::SharedFlags(flags));
    }
    if mob.dying {
        out.push(EntityData::Pose(
            mc_bamboo_protocol::packet::play::Pose::Dying,
        ));
    } else if mob.sleeping_pos.is_some() {
        out.push(EntityData::Pose(
            mc_bamboo_protocol::packet::play::Pose::Sleeping,
        ));
    } else if mob.goat.long_jumping {
        out.push(EntityData::Pose(
            mc_bamboo_protocol::packet::play::Pose::LongJumping,
        ));
    } else if mob.ocelot_crouching || mob.cat.crouching {
        out.push(EntityData::Pose(
            mc_bamboo_protocol::packet::play::Pose::Crouching,
        ));
    } else if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Frog {
        use mc_bamboo_blocks::entity::mob::frog::Pose as FrogPose;
        use mc_bamboo_protocol::packet::play::Pose;
        match mob.frog.pose {
            FrogPose::Standing => {}
            FrogPose::Croaking => out.push(EntityData::Pose(Pose::Croaking)),
            FrogPose::LongJumping => out.push(EntityData::Pose(Pose::LongJumping)),
            FrogPose::UsingTongue => out.push(EntityData::Pose(Pose::UsingTongue)),
        }
    }
    if mob.air_supply != 300 {
        out.push(EntityData::AirSupply(mob.air_supply));
    }
    if let Some(name) = &mob.custom_name {
        out.push(EntityData::CustomName(name.clone()));
    }
    if mob.custom_name_visible {
        out.push(EntityData::CustomNameVisible(true));
    }
    if mob.silent {
        out.push(EntityData::Silent(true));
    }
    if mob.no_gravity {
        out.push(EntityData::NoGravity(true));
    }
    if mob.ticks_frozen != 0 {
        out.push(EntityData::TicksFrozen(mob.ticks_frozen));
    }
    // `LivingEntity` 的 `DATA_LIVING_ENTITY_FLAGS` 恒为 0，`DATA_HEALTH_ID` 默认 1。
    // The living flags are always 0 and health defaults to 1.
    out.push(EntityData::Health(mob.health));
    // `Guardian.DATA_ID_MOVING` / `DATA_ID_ATTACK_TARGET`：只在偏离默认时发。
    // `Guardian.DATA_ID_MOVING` / `DATA_ID_ATTACK_TARGET`: sent only when non-default.
    if mc_bamboo_blocks::entity::mob::guardian::is_guardian(mob.mob_type) {
        if mob.guardian_moving {
            out.push(EntityData::GuardianMoving(true));
        }
        if mob.guardian_attack_target != 0 {
            out.push(EntityData::GuardianAttackTarget(mob.guardian_attack_target));
        }
    }
    effect_data(mob, &mut out);
    if mob.using_item_flags != 0 {
        out.push(EntityData::LivingFlags(mob.using_item_flags));
    }
    if mob.sleeping_pos.is_some() {
        out.push(EntityData::SleepingPos(mob.sleeping_pos));
    }
    let mut mob_flags = 0u8;
    if mob.no_ai {
        mob_flags |= 1;
    }
    if mob.left_handed {
        mob_flags |= 2;
    }
    // `Mob.setAggressive` 也写这一位，官方对所有生物都同步（狼、狐狸用近战目标时置位）。
    // `Mob.setAggressive` writes this bit too, for every mob (set while a wolf or fox
    // melee goal runs).
    if mob.aggressive {
        mob_flags |= 4;
    }
    if mob_flags != 0 {
        out.push(EntityData::MobFlags(mob_flags));
    }
    if mob.baby {
        out.push(EntityData::Baby(true));
    }
    if mob.age_locked {
        out.push(EntityData::AgeLocked(true));
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Bee {
        if mob.bee_flags != 0 {
            out.push(EntityData::BeeFlags(mob.bee_flags));
        }
        if mob.bee_anger_end_time != -1 {
            out.push(EntityData::BeeAngerEndTime(mob.bee_anger_end_time));
        }
        return out;
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Nautilus {
        if mob.nautilus_dashing {
            out.push(EntityData::NautilusDash(true));
        }
        out.extend(wolf_non_default_data(mob));
        return out;
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Wolf {
        // 狼的 18、19 是驯服标志和主人，不是牛的花色和叫声。
        // A wolf's 18 and 19 are the taming items, not the cow variants.
        out.extend(wolf_non_default_data(mob));
        return out;
    }
    if let Some(equine) = &mob.equine {
        out.extend(equine_non_default_data(mob.mob_type, equine));
        return out;
    }
    // 注册表默认的是 temperate / classic。
    // The registry defaults are temperate / classic.
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Rabbit && mob.rabbit_variant != 0 {
        out.push(EntityData::RabbitVariant(mob.rabbit_variant));
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Fox {
        out.extend(fox_non_default_data(mob));
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Armadillo {
        // 犰狳只有 `ARMADILLO_STATE` 一项，缺省 `IDLE`（0）。
        // Armadillos only add `ARMADILLO_STATE`, `IDLE` (0) by default.
        if mob.armadillo_state != 0 {
            out.push(EntityData::ArmadilloState(mob.armadillo_state));
        }
        return out;
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Frog {
        // 青蛙多两项：品种（缺省温带）和舌头目标（缺省空）。
        // Frogs add two: the variant (temperate by default) and the tongue target (empty
        // by default).
        if mob.frog.variant != mc_bamboo_blocks::entity::mob::frog::Variant::default() {
            out.push(EntityData::FrogVariant(mob.frog.variant.network_id()));
        }
        if mob.frog.tongue_target.is_some() {
            out.push(EntityData::FrogTongueTarget(mob.frog.tongue_target));
        }
        return out;
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Sniffer {
        // 嗅探兽的 18、19 是状态和掉种子时刻，缺省 IDLING（0）和 0。
        // A sniffer's 18 and 19 are the state and the drop-seed tick, defaults IDLING (0)
        // and 0.
        let state = mob.sniffer_state.id();
        if state != 0 {
            out.push(EntityData::SnifferState(state));
        }
        if mob.sniffer_drop_seed_at_tick != 0 {
            out.push(EntityData::SnifferDropSeedAtTick(
                mob.sniffer_drop_seed_at_tick,
            ));
        }
        return out;
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Tadpole {
        // 蝌蚪只有鱼的 `FROM_BUCKET`（从来不写）和自己的 `AGE_LOCKED`（上面已经放了）。
        // Tadpoles only have the fish `FROM_BUCKET` (never written) and their own
        // `AGE_LOCKED`, pushed above.
        return out;
    }
    if matches!(
        mob.mob_type,
        mc_bamboo_blocks::entity::mob::MobType::IronGolem
            | mc_bamboo_blocks::entity::mob::MobType::SnowGolem
    ) {
        let default = if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::SnowGolem {
            16
        } else {
            0
        };
        if mob.golem_flags != default {
            out.push(EntityData::GolemFlags(mob.golem_flags));
        }
        return out;
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Goat {
        // 山羊的 18、19、20 是尖叫和两只角，缺省假、真、真。
        // A goat's 18, 19 and 20 are screaming and the two horns: false, true, true.
        if mob.goat.screaming {
            out.push(EntityData::GoatScreaming(true));
        }
        if !mob.goat.left_horn {
            out.push(EntityData::GoatLeftHorn(false));
        }
        if !mob.goat.right_horn {
            out.push(EntityData::GoatRightHorn(false));
        }
        return out;
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::ZombieVillager {
        use mc_bamboo_data::villager;
        if mob.zombie_villager_converting {
            out.push(EntityData::ZombieVillagerConverting(true));
        }
        let default_kind = villager::type_id(villager::DEFAULT_TYPE).unwrap_or(0);
        let default_profession = villager::profession_id(villager::NONE_PROFESSION).unwrap_or(0);
        if mob.villager_kind != default_kind
            || mob.villager_profession != default_profession
            || mob.villager_level != villager::MIN_VILLAGER_LEVEL
        {
            out.push(EntityData::ZombieVillagerData {
                kind: mob.villager_kind,
                profession: mob.villager_profession,
                level: mob.villager_level,
            });
        }
        if mob.villager_finalized {
            out.push(EntityData::ZombieVillagerDataFinalized(true));
        }
        return out;
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Villager {
        use mc_bamboo_data::villager;
        // 按 index 升序：18 不高兴计数、19 `VillagerData`、20 finalized。
        // Ascending indices: 18 unhappy, 19 `VillagerData`, 20 finalized.
        if mob.villager_unhappy != 0 {
            out.push(EntityData::VillagerUnhappy(mob.villager_unhappy));
        }
        let default_kind = villager::type_id(villager::DEFAULT_TYPE).unwrap_or(0);
        let default_profession = villager::profession_id(villager::NONE_PROFESSION).unwrap_or(0);
        if mob.villager_kind != default_kind
            || mob.villager_profession != default_profession
            || mob.villager_level != villager::MIN_VILLAGER_LEVEL
        {
            out.push(EntityData::VillagerData {
                kind: mob.villager_kind,
                profession: mob.villager_profession,
                level: mob.villager_level,
            });
        }
        if mob.villager_finalized {
            out.push(EntityData::VillagerDataFinalized(true));
        }
        return out;
    }
    if fish_non_default_data(mob, &mut out) {
        return out;
    }
    if farm_non_default_data(mob, &mut out) {
        return out;
    }
    if water_mob_non_default_data(mob, &mut out) {
        return out;
    }
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Pig {
        let variant = mob.variant.name();
        if variant != "minecraft:temperate" {
            out.push(EntityData::PigVariant(pig_variant_id(variant)));
        }
        let sound = mob.pig_sound_variant.registry_id();
        if sound != pig_sound_variant_default() {
            out.push(EntityData::PigSoundVariant(sound));
        }
        if mob.pig_boost_time != 0 {
            out.push(EntityData::PigBoostTime(mob.pig_boost_time));
        }
    } else {
        if mob.variant.registry_id() != 1 {
            out.push(EntityData::CowVariant(mob.variant.registry_id()));
        }
        if mob.sound_variant.registry_id() != 0 {
            out.push(EntityData::CowSoundVariant(mob.sound_variant.registry_id()));
        }
    }
    out
}

/// 玩家音源的音效包（挤奶、染色、剪毛）。
///
/// A player-category sound packet (milking, dyeing, shearing).
fn player_sound(sound: &str, position: mc_bamboo_core::Vec3, seed: i64) -> EventPacket {
    EventPacket::Sound(mc_bamboo_protocol::packet::play::Sound {
        sound: i32::from(mc_bamboo_data::sounds::sound_event(sound)),
        inline: None,
        source: mc_bamboo_data::sounds::PLAYERS,
        x: position.x,
        y: position.y,
        z: position.z,
        volume: 1.0,
        pitch: 1.0,
        seed,
    })
}

/// 羊和鸡的默认实体数据：羊毛 0，鸡 temperate / classic。
///
/// Sheep and chicken defaults: wool 0, chicken temperate / classic.
fn farm_default_data(
    index: u8,
    kind: mc_bamboo_blocks::entity::mob::MobType,
) -> Option<EntityData> {
    use mc_bamboo_blocks::entity::mob::MobType;
    use mc_bamboo_data::entity_types as facts;
    match (kind, index) {
        (MobType::Sheep, facts::SHEEP_WOOL) => Some(EntityData::SheepWool(0)),
        (MobType::Slime, facts::CUBE_MOB_SIZE) => Some(EntityData::CubeMobSize(1)),
        // `AbstractCubeMob.ID_SIZE` 也是硫磺方块怪的 index 18，默认是 1。
        (MobType::SulfurCube, facts::CUBE_MOB_SIZE) => Some(EntityData::CubeMobSize(1)),
        (MobType::SulfurCube, facts::SULFUR_CUBE_MAX_FUSE) => {
            Some(EntityData::SulfurCubeMaxFuse(-1))
        }
        (MobType::SulfurCube, facts::SULFUR_CUBE_FROM_BUCKET) => {
            Some(EntityData::SulfurCubeFromBucket(false))
        }
        (MobType::Mooshroom, facts::MOOSHROOM_TYPE) => Some(EntityData::MooshroomType(0)),
        (MobType::PolarBear, facts::POLAR_BEAR_STANDING) => {
            Some(EntityData::PolarBearStanding(false))
        }
        (MobType::Panda, facts::PANDA_UNHAPPY) => Some(EntityData::PandaUnhappy(0)),
        (MobType::Panda, facts::PANDA_SNEEZE) => Some(EntityData::PandaSneeze(0)),
        (MobType::Panda, facts::PANDA_EAT) => Some(EntityData::PandaEat(0)),
        (MobType::Panda, facts::PANDA_MAIN_GENE) => Some(EntityData::PandaMainGene(0)),
        (MobType::Panda, facts::PANDA_HIDDEN_GENE) => Some(EntityData::PandaHiddenGene(0)),
        (MobType::Panda, facts::PANDA_FLAGS) => Some(EntityData::PandaFlags(0)),
        (MobType::Ocelot, facts::OCELOT_TRUSTING) => Some(EntityData::OcelotTrusting(false)),
        (MobType::IronGolem, facts::IRON_GOLEM_FLAGS) => Some(EntityData::GolemFlags(0)),
        (MobType::SnowGolem, facts::SNOW_GOLEM_PUMPKIN) => Some(EntityData::GolemFlags(16)),
        (MobType::Cat, facts::TAME_FLAGS) => Some(EntityData::TameFlags(0)),
        (MobType::Cat, facts::TAME_OWNER) => Some(EntityData::TameOwner(None)),
        (MobType::Cat, facts::CAT_VARIANT) => Some(EntityData::CatVariant(
            mc_bamboo_blocks::entity::mob::cat::default_variant(),
        )),
        (MobType::Cat, facts::CAT_IS_LYING) => Some(EntityData::CatIsLying(false)),
        (MobType::Cat, facts::CAT_RELAX_STATE_ONE) => Some(EntityData::CatRelaxStateOne(false)),
        (MobType::Cat, facts::CAT_COLLAR) => Some(EntityData::CatCollar(
            mc_bamboo_blocks::entity::mob::cat::DEFAULT_COLLAR,
        )),
        (MobType::Cat, facts::CAT_SOUND_VARIANT) => Some(EntityData::CatSoundVariant(
            mc_bamboo_blocks::entity::mob::cat::default_sound_variant(),
        )),
        (MobType::Parrot, facts::PARROT_VARIANT) => Some(EntityData::ParrotVariant(
            mc_bamboo_blocks::entity::mob::parrot::DEFAULT_VARIANT,
        )),
        (MobType::GlowSquid, facts::GLOW_SQUID_DARK_TICKS) => {
            Some(EntityData::GlowSquidDarkTicks(0))
        }
        (MobType::Bat, facts::BAT_FLAGS) => Some(EntityData::BatFlags(0)),
        (MobType::Phantom, facts::PHANTOM_SIZE) => Some(EntityData::PhantomSize(0)),
        (MobType::Chicken, facts::CHICKEN_VARIANT) => Some(EntityData::ChickenVariant(
            registry_index(chicken_variants(), "minecraft:temperate"),
        )),
        (MobType::Chicken, facts::CHICKEN_SOUND_VARIANT) => Some(EntityData::ChickenSoundVariant(
            registry_index(chicken_sound_variants(), "minecraft:classic"),
        )),
        _ => None,
    }
}

/// 鱼的默认实体数据：不在桶里、鲑鱼 medium、热带鱼 KOB 白/白、河豚不鼓。
///
/// Fish defaults: not from a bucket, salmon medium, tropical KOB white/white,
/// pufferfish deflated.
fn fish_default_data(
    index: u8,
    kind: mc_bamboo_blocks::entity::mob::MobType,
) -> Option<EntityData> {
    use mc_bamboo_blocks::entity::mob::MobType;
    use mc_bamboo_data::entity_types as facts;
    match (kind, index) {
        (
            MobType::Cod | MobType::Salmon | MobType::TropicalFish | MobType::Pufferfish,
            facts::FISH_FROM_BUCKET,
        ) => Some(EntityData::FishFromBucket(false)),
        (MobType::Salmon, facts::SALMON_VARIANT) => Some(EntityData::SalmonVariant(1)),
        (MobType::TropicalFish, facts::TROPICAL_FISH_VARIANT) => {
            Some(EntityData::TropicalFishVariant(0))
        }
        (MobType::Pufferfish, facts::PUFFERFISH_PUFF_STATE) => {
            Some(EntityData::PufferfishPuffState(0))
        }
        _ => None,
    }
}

/// 海豚 / 美西螈偏离默认值的实体数据；不是这两种时返回假。
///
/// Dolphin / axolotl data that differs from the defaults; false for other kinds.
fn water_mob_non_default_data(
    mob: &mc_bamboo_blocks::entity::mob::MobPublic,
    out: &mut Vec<EntityData>,
) -> bool {
    use mc_bamboo_blocks::entity::mob::MobType;
    match mob.mob_type {
        MobType::Dolphin => {
            if mob.dolphin_got_fish {
                out.push(EntityData::DolphinGotFish(true));
            }
            // `MOISTNESS_LEVEL` 的默认 2400；只有偏离时才发。
            // `MOISTNESS_LEVEL` defaults to 2400; only a deviation is sent.
            if mob.dolphin_moistness != 2400 {
                out.push(EntityData::DolphinMoistness(mob.dolphin_moistness));
            }
            true
        }
        MobType::Axolotl => {
            // `DATA_VARIANT` 默认 0（lucy）。
            // `DATA_VARIANT` defaults to 0 (lucy).
            if mob.axolotl_variant != 0 {
                out.push(EntityData::AxolotlVariant(mob.axolotl_variant));
            }
            if mob.axolotl_playing_dead {
                out.push(EntityData::AxolotlPlayingDead(true));
            }
            if mob.axolotl_from_bucket {
                out.push(EntityData::AxolotlFromBucket(true));
            }
            true
        }
        _ => false,
    }
}

/// 鱼偏离默认的实体数据。
///
/// Fish data that differs from the defaults.
fn fish_non_default_data(
    mob: &mc_bamboo_blocks::entity::mob::MobPublic,
    out: &mut Vec<EntityData>,
) -> bool {
    use mc_bamboo_blocks::entity::mob::MobType;
    if !matches!(
        mob.mob_type,
        MobType::Cod | MobType::Salmon | MobType::TropicalFish | MobType::Pufferfish
    ) {
        return false;
    }
    if mob.fish_from_bucket {
        out.push(EntityData::FishFromBucket(true));
    }
    match mob.mob_type {
        MobType::Salmon if mob.fish_variant != 1 => {
            out.push(EntityData::SalmonVariant(mob.fish_variant));
        }
        MobType::TropicalFish if mob.fish_variant != 0 => {
            out.push(EntityData::TropicalFishVariant(mob.fish_variant));
        }
        MobType::Pufferfish if mob.puff_state != 0 => {
            out.push(EntityData::PufferfishPuffState(mob.puff_state));
        }
        _ => {}
    }
    true
}

/// 羊和鸡偏离默认的实体数据；不是这两种时返回假。
///
/// Sheep and chicken data that differs from the defaults; false for other kinds.
fn farm_non_default_data(
    mob: &mc_bamboo_blocks::entity::mob::MobPublic,
    out: &mut Vec<EntityData>,
) -> bool {
    use mc_bamboo_blocks::entity::mob::MobType;
    match mob.mob_type {
        MobType::Sheep => {
            if mob.wool != 0 {
                out.push(EntityData::SheepWool(mob.wool));
            }
            true
        }
        // `AbstractCubeMob.ID_SIZE` 缺省是 1。
        // `AbstractCubeMob.ID_SIZE` defaults to 1.
        MobType::Slime => {
            if mob.cube_size != 1 {
                out.push(EntityData::CubeMobSize(mob.cube_size));
            }
            true
        }
        // 硫磺方块怪：`ID_SIZE`、`MAX_FUSE`、`FROM_BUCKET` 三项元数据。
        MobType::SulfurCube => {
            if mob.cube_size != 1 {
                out.push(EntityData::CubeMobSize(mob.cube_size));
            }
            if mob.sulfur_cube_max_fuse != -1 {
                out.push(EntityData::SulfurCubeMaxFuse(mob.sulfur_cube_max_fuse));
            }
            if mob.sulfur_cube_from_bucket {
                out.push(EntityData::SulfurCubeFromBucket(true));
            }
            true
        }
        MobType::Mooshroom => {
            if mob.mooshroom_brown {
                out.push(EntityData::MooshroomType(1));
            }
            true
        }
        MobType::PolarBear => {
            if mob.polar_bear_standing {
                out.push(EntityData::PolarBearStanding(true));
            }
            true
        }
        MobType::Panda => {
            let panda = &mob.panda;
            if panda.unhappy_counter != 0 {
                out.push(EntityData::PandaUnhappy(panda.unhappy_counter));
            }
            if panda.sneeze_counter != 0 {
                out.push(EntityData::PandaSneeze(panda.sneeze_counter));
            }
            if panda.eat_counter != 0 {
                out.push(EntityData::PandaEat(panda.eat_counter));
            }
            if panda.main_gene != 0 {
                out.push(EntityData::PandaMainGene(panda.main_gene));
            }
            if panda.hidden_gene != 0 {
                out.push(EntityData::PandaHiddenGene(panda.hidden_gene));
            }
            if panda.flags != 0 {
                out.push(EntityData::PandaFlags(panda.flags));
            }
            true
        }
        MobType::Ocelot => {
            if mob.ocelot_trusting {
                out.push(EntityData::OcelotTrusting(true));
            }
            true
        }
        MobType::Cat => {
            use mc_bamboo_blocks::entity::mob::cat;
            if mob.tame_flags != 0 {
                out.push(EntityData::TameFlags(mob.tame_flags));
            }
            if mob.owner.is_some() {
                out.push(EntityData::TameOwner(mob.owner));
            }
            if mob.cat.variant != cat::default_variant() {
                out.push(EntityData::CatVariant(mob.cat.variant));
            }
            if mob.cat.lying {
                out.push(EntityData::CatIsLying(true));
            }
            if mob.cat.relax_state_one {
                out.push(EntityData::CatRelaxStateOne(true));
            }
            if mob.cat.collar != cat::DEFAULT_COLLAR {
                out.push(EntityData::CatCollar(mob.cat.collar));
            }
            if mob.cat.sound_variant != cat::default_sound_variant() {
                out.push(EntityData::CatSoundVariant(mob.cat.sound_variant));
            }
            true
        }
        MobType::Parrot => {
            use mc_bamboo_blocks::entity::mob::parrot;
            if mob.tame_flags != 0 {
                out.push(EntityData::TameFlags(mob.tame_flags));
            }
            if mob.owner.is_some() {
                out.push(EntityData::TameOwner(mob.owner));
            }
            if mob.parrot.variant != parrot::DEFAULT_VARIANT {
                out.push(EntityData::ParrotVariant(mob.parrot.variant));
            }
            true
        }
        MobType::Nautilus => {
            if mob.tame_flags != 0 {
                out.push(EntityData::TameFlags(mob.tame_flags));
            }
            if mob.owner.is_some() {
                out.push(EntityData::TameOwner(mob.owner));
            }
            if mob.nautilus_dashing {
                out.push(EntityData::NautilusDash(true));
            }
            true
        }
        MobType::Chicken => {
            let variant = registry_index(chicken_variants(), mob.variant.name());
            if variant != registry_index(chicken_variants(), "minecraft:temperate") {
                out.push(EntityData::ChickenVariant(variant));
            }
            let sound = mob.chicken_sound_variant.registry_id();
            if sound != registry_index(chicken_sound_variants(), "minecraft:classic") {
                out.push(EntityData::ChickenSoundVariant(sound));
            }
            true
        }
        // 鱿鱼没有花色；发光鱿鱼多一项变暗计时。
        // Squid have no variant; glow squid add the dark countdown.
        MobType::Squid | MobType::GlowSquid => {
            if mob.dark_ticks != 0 {
                out.push(EntityData::GlowSquidDarkTicks(mob.dark_ticks));
            }
            true
        }
        // 蝙蝠只多一项标志字节，缺省 0（飞行）。
        // Bats add one flag byte, 0 (flying) by default.
        MobType::Bat => {
            if mob.bat_flags != 0 {
                out.push(EntityData::BatFlags(mob.bat_flags));
            }
            true
        }
        // 幻翼只多一项大小，缺省 0。
        // Phantoms add one size item, 0 by default.
        MobType::Phantom => {
            if mob.phantom_size != 0 {
                out.push(EntityData::PhantomSize(mob.phantom_size));
            }
            true
        }
        MobType::Turtle => {
            if mob.turtle_has_egg {
                out.push(EntityData::TurtleHasEgg(true));
            }
            if mob.turtle_laying_egg {
                out.push(EntityData::TurtleLayingEgg(true));
            }
            true
        }
        _ => false,
    }
}

fn chicken_variants() -> &'static [&'static str] {
    mc_bamboo_data::component_registries::registry_names::CHICKEN_VARIANT
}

fn chicken_sound_variants() -> &'static [&'static str] {
    mc_bamboo_data::component_registries::registry_names::CHICKEN_SOUND_VARIANT
}

fn registry_index(names: &[&str], name: &str) -> i32 {
    names
        .iter()
        .position(|entry| *entry == name)
        .expect("variant in the registry") as i32
}

fn pig_variant_id(name: &str) -> i32 {
    mc_bamboo_data::component_registries::registry_names::PIG_VARIANT
        .iter()
        .position(|entry| *entry == name)
        .expect("pig variant in the registry") as i32
}

fn pig_sound_variant_default() -> i32 {
    mc_bamboo_data::component_registries::registry_names::PIG_SOUND_VARIANT
        .iter()
        .position(|entry| *entry == "minecraft:classic")
        .expect("classic pig sound variant in the registry") as i32
}

/// 把这一刻的生物事件换成「包 ID + 包体」字节，按事件发生顺序。
/// 游戏路径直接把结果交给 `Links`，逐刻对拍也调这个函数，不另写一份。
/// 音效的种子由 `sound_seed` 回调给出（游戏里是 `Region.sound_seed_generator`）。
///
/// Turn this tick's mob events into "id + body" bytes in event order. The live
/// path hands these straight to `Links`; the tick oracle calls the same function.
/// `sound_seed` supplies the seed (the region's generator in the live game).
pub fn event_packets(
    events: &[(i32, Vec<mc_bamboo_blocks::entity::mob::MobEvent>)],
    mut sound_seed: impl FnMut() -> i64,
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for (id, mob_events) in events {
        for event in mob_events {
            out.extend(event_packet(*id, event, &mut sound_seed).body());
        }
    }
    out
}

/// 一次生物事件的包；种类决定发给谁。
///
/// One mob event's packet; its kind decides who receives it.
enum EventPacket {
    /// `broadcastEntityEvent` / `broadcastDamageEvent`：官方
    /// `ServerChunkCache.sendToTrackingPlayersAndSelf`，发给正在跟踪这个实体的玩家。
    ///
    /// Sent to the players tracking the entity (vanilla
    /// `sendToTrackingPlayersAndSelf`).
    Tracking(Vec<u8>),
    /// `playSeededSound`：官方 `PlayerList.broadcast`，发给音效范围内的玩家。
    ///
    /// Sent to the players within the sound's range (vanilla `PlayerList.broadcast`).
    Sound(mc_bamboo_protocol::packet::play::Sound),
    /// `ServerLevel.sendParticles`：发给所在方块中心离粒子不到 32 格的玩家。
    ///
    /// `ServerLevel.sendParticles`: sent to the players whose block centre is within 32
    /// blocks of the particle.
    Particles {
        body: Vec<u8>,
        position: mc_bamboo_core::Vec3,
    },
    /// 没有包（`set_equipment` 至少要一个槽）。
    Nothing,
}

impl EventPacket {
    fn body(&self) -> Option<Vec<u8>> {
        match self {
            Self::Tracking(body) => Some(body.clone()),
            Self::Sound(sound) => Some(packet_body(sound)),
            Self::Particles { body, .. } => Some(body.clone()),
            Self::Nothing => None,
        }
    }
}

/// 一次生物事件的包。音效的种子由 `sound_seed` 当场取一个。
///
/// One mob event's packet; a sound takes its seed from `sound_seed` on the spot.
fn event_packet(
    id: i32,
    event: &mc_bamboo_blocks::entity::mob::MobEvent,
    sound_seed: &mut impl FnMut() -> i64,
) -> EventPacket {
    use mc_bamboo_blocks::entity::mob::MobEvent;
    use mc_bamboo_protocol::packet::play::{DamageEvent, EntityEvent, Sound};

    match event {
        MobEvent::Death => EventPacket::Tracking(packet_body(&EntityEvent {
            entity_id: id,
            event: 3,
        })),
        MobEvent::Poof => EventPacket::Tracking(packet_body(&EntityEvent {
            entity_id: id,
            event: 60,
        })),
        MobEvent::Hearts => EventPacket::Tracking(packet_body(&EntityEvent {
            entity_id: id,
            event: 18,
        })),
        // 驯服事件不是网络包；在 `send_mob_events` 里按 UUID 触发进度。
        MobEvent::Tamed { .. } | MobEvent::CuredZombieVillager { .. } => EventPacket::Nothing,
        MobEvent::EntityEvent { event } => EventPacket::Tracking(packet_body(&EntityEvent {
            entity_id: id,
            event: *event,
        })),
        MobEvent::Equipment(changes) => {
            let slots: Vec<_> = changes.iter().map(|(slot, stack)| (*slot, stack)).collect();
            match mc_bamboo_protocol::packet::play::SetEquipment::new(id, &slots) {
                Some(packet) => EventPacket::Tracking(packet_body(&packet)),
                None => EventPacket::Nothing,
            }
        }
        MobEvent::LeashLink { holder } => EventPacket::Tracking(packet_body(
            &mc_bamboo_protocol::packet::play::SetEntityLink {
                source_id: id,
                dest_id: holder.unwrap_or(0),
            },
        )),
        MobEvent::MilkSound { position } => {
            player_sound("minecraft:entity.cow.milk", *position, sound_seed())
        }
        MobEvent::PlayerSound { sound, position } => player_sound(sound, *position, sound_seed()),
        MobEvent::PlayerSoundAt {
            sound,
            position,
            volume,
        } => {
            let mut packet = player_sound(sound, *position, sound_seed());
            if let EventPacket::Sound(sound) = &mut packet {
                sound.volume = *volume;
            }
            packet
        }
        MobEvent::Particles {
            particle,
            item,
            position,
            offset,
            speed,
            count,
        } => {
            // 简单粒子的 `ParticleTypes.STREAM_CODEC` 只有类型 ID；带物品的粒子
            // （`ItemParticleOption`）再按 `ItemStackTemplate.STREAM_CODEC` 追加
            // 物品 ID、数量 1、空的组件补丁（两个 0 varint）。
            // A simple particle's codec is the type id alone; an item particle appends
            // `ItemStackTemplate.STREAM_CODEC`: item id, count 1, empty component patch.
            let mut options = Vec::new();
            mc_bamboo_protocol::codec::write_varint(&mut options, *particle);
            if let Some(item) = *item {
                mc_bamboo_protocol::codec::write_varint(&mut options, item);
                mc_bamboo_protocol::codec::write_varint(&mut options, 1);
                mc_bamboo_protocol::codec::write_varint(&mut options, 0);
                mc_bamboo_protocol::codec::write_varint(&mut options, 0);
            }
            EventPacket::Particles {
                body: packet_body(&mc_bamboo_protocol::packet::play::LevelParticles {
                    options: &options,
                    override_limiter: false,
                    always_show: false,
                    x: position.x,
                    y: position.y,
                    z: position.z,
                    x_dist: offset.x as f32,
                    y_dist: offset.y as f32,
                    z_dist: offset.z as f32,
                    max_speed: *speed,
                    count: *count,
                    randomization: mc_bamboo_protocol::packet::play::Randomization::Default,
                }),
                position: *position,
            }
        }
        MobEvent::Damage {
            kind,
            attacker,
            direct,
        } => {
            let damage_type = mc_bamboo_data::damage_type_id(kind.name()).unwrap_or(0);
            EventPacket::Tracking(packet_body(&DamageEvent {
                entity_id: id,
                damage_type,
                source_cause: *attacker,
                source_direct: direct.or(*attacker),
                position: None,
            }))
        }
        MobEvent::Sound {
            sound,
            volume,
            pitch,
            position,
            hostile,
            seed,
        } => EventPacket::Sound(Sound {
            sound: i32::from(mc_bamboo_data::sounds::sound_event(sound)),
            inline: None,
            source: if *hostile {
                mc_bamboo_data::sounds::HOSTILE
            } else {
                mc_bamboo_data::sounds::NEUTRAL
            },
            x: position.x,
            y: position.y,
            z: position.z,
            volume: *volume,
            pitch: *pitch,
            seed: seed.unwrap_or_else(sound_seed),
        }),
    }
}

/// 把这一刻的生物事件按官方的收件人发出去：实体事件和伤害事件给正在跟踪这个实体的
/// 玩家（跟踪关系是上一刻同步阶段定的，和官方实体 tick 时的 `seenBy` 一样），音效给
/// 范围内的玩家（`interaction::send_sound`，和方块音效同一条路）。事件按发生顺序，
/// 每名玩家收到的先后不变。以前发给岛上的全体玩家：远处的人也收得到，多岛时收件人
/// 还和岛的划分有关。
///
/// Send this tick's mob events to vanilla's recipients: entity and damage
/// events to the players tracking the entity (pairings from last tick's sync,
/// like vanilla's `seenBy` during entity ticks), sounds to the players in range
/// (`interaction::send_sound`, the block-sound path). Events keep their order.
/// They used to reach every player on the island, so distant players got them
/// and, with several islands, the recipients depended on the partition.
fn send_mob_events(
    region: &mut Region,
    events: &[(i32, Vec<mc_bamboo_blocks::entity::mob::MobEvent>)],
) {
    for (id, mob_events) in events {
        for event in mob_events {
            if let mc_bamboo_blocks::entity::mob::MobEvent::CuredZombieVillager { player } = event {
                let players: Vec<crate::inbound::PlayerId> = region
                    .players
                    .iter()
                    .filter(|(_, candidate)| candidate.uuid.as_bytes() == player)
                    .map(|(id, _)| *id)
                    .collect();
                for player_id in players {
                    crate::advancement_triggers::record(
                        region,
                        player_id,
                        "minecraft:cured_zombie_villager",
                        serde_json::json!({
                            "zombie": {"type": "minecraft:zombie_villager"},
                            "villager": {"type": "minecraft:villager"}
                        }),
                    );
                }
                continue;
            }
            if let mc_bamboo_blocks::entity::mob::MobEvent::Tamed { player } = event {
                let kind = region
                    .entities
                    .public_at(*id)
                    .and_then(|public| match &public.kind {
                        mc_bamboo_blocks::entity::KindPublic::Mob(mob) => Some(mob.mob_type.name()),
                        _ => None,
                    })
                    .unwrap_or("");
                let players: Vec<crate::inbound::PlayerId> = region
                    .players
                    .iter()
                    .filter(|(_, candidate)| candidate.uuid.as_bytes() == player)
                    .map(|(id, _)| *id)
                    .collect();
                for player_id in players {
                    crate::advancement_triggers::record(
                        region,
                        player_id,
                        "minecraft:tame_animal",
                        serde_json::json!({"entity": {"type": kind}}),
                    );
                }
                continue;
            }
            let packet = event_packet(*id, event, &mut || {
                region.sound_seed_generator.source().next_long()
            });
            match packet {
                EventPacket::Tracking(body) => {
                    let Some(tracked) = region.entity_tracking.tracked.get(id) else {
                        continue;
                    };
                    let frames = frame_from_body(&body);
                    for &player in &tracked.seen_by {
                        region.links.send_frames(player, &frames);
                    }
                }
                EventPacket::Sound(sound) => crate::interaction::send_sound(region, None, &sound),
                EventPacket::Particles { body, position } => {
                    send_particles(region, &body, position)
                }
                EventPacket::Nothing => {}
            }
        }
    }
}

/// `ServerLevel.sendParticles(player, false, x, y, z, packet)`：玩家所在方块的中心离粒子
/// 不到 32 格才发（`BlockPos.closerToCenterThan`）。
///
/// `ServerLevel.sendParticles(player, false, x, y, z, packet)`: only players whose block
/// centre is within 32 blocks get the packet (`BlockPos.closerToCenterThan`).
/// 方块实体发出的振动粒子包，距离判定和普通粒子包一样。
pub(crate) fn send_vibration_particle(
    region: &mut Region,
    body: &[u8],
    position: mc_bamboo_core::Vec3,
) {
    send_particles(region, body, position);
}

fn send_particles(region: &mut Region, body: &[u8], position: mc_bamboo_core::Vec3) {
    let frames = frame_from_body(body);
    let near: Vec<_> = region
        .players
        .iter()
        .filter(|(_, player)| {
            let block = mc_bamboo_core::BlockPos::containing(player.position());
            let dx = f64::from(block.x) + 0.5 - position.x;
            let dy = f64::from(block.y) + 0.5 - position.y;
            let dz = f64::from(block.z) + 0.5 - position.z;
            dx * dx + dy * dy + dz * dz < 32.0 * 32.0
        })
        .map(|(id, _)| *id)
        .collect();
    for player in near {
        region.links.send_frames(player, &frames);
    }
}

/// 每刻同步前的公共准备：生物事件、消失的实体、新实体的跟踪状态。
/// `sync` 和测试里的全表扫描参照实现都调它，两边字节才一致。
///
/// Common pre-sync work: mob events, departed entities and spawn-time
/// tracking state. Both `sync` and the full-scan test oracle call this.
fn prepare_sync(region: &mut Region) {
    // 生物事件（受伤、死亡、音效）先发，再发这一刻的配对和同步包。
    // Mob events go out before this tick's pairing and sync packets.
    let events = region.entities.take_mob_events();
    send_mob_events(region, &events);
    let live: BTreeSet<i32> = region
        .entities
        .iter_public()
        .map(|public| public.uid)
        .collect();
    remove_missing(region, &live);
    // 新实体的 `TrackedEntity` 用生成那一刻的样子建，配对包的速度才和官方一致。
    // New entities get their `TrackedEntity` from the spawn-time snapshot, so
    // the pairing packet's velocity matches vanilla.
    for public in region.entities.take_spawn_snapshots() {
        let tracked = &mut region.entity_tracking.tracked;
        if let std::collections::btree_map::Entry::Vacant(entry) = tracked.entry(public.uid) {
            let snapshot = Snapshot::of(&public);
            entry.insert(TrackedEntity::new(&snapshot));
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
    prepare_sync(region);
    let snapshots: Vec<Snapshot> = region.entities.iter_public().map(Snapshot::of).collect();
    let index = PassengerIndex::of(&snapshots);

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
            &index,
        );
        send_changes(&snapshot, tracked, &mut region.links);
    }
}

/// 对拍专用：只发这一刻攒下的生物事件，不跑配对和同步包。
/// 玩家受伤对拍里的 `damage_event` 走的就是正式的事件分发路径。
///
/// Oracle-only: sends this tick's mob events without pairing or sync packets.
#[cfg(feature = "oracle-samples")]
pub(crate) fn drain_events_for_oracle(region: &mut Region, viewer: crate::inbound::PlayerId) {
    // 对拍假世界没有区块加载流程，`seen_by` 是空的。这里用同一份
    // `event_packets` 编码器直接把事件包写给观察者，不复制编码逻辑。
    let events = region.entities.take_mob_events();
    let sound_seed = region.sound_seed_generator.source().seed();
    let source = mc_bamboo_blocks::LegacyRandom::new(sound_seed);
    for body in event_packets(&events, &mut || source.next_long()) {
        let frames = frame_from_body(&body);
        region.links.send_frames(viewer, &frames);
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
    index: &PassengerIndex,
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
        let frames = pairing_frames(snapshot, tracked, index);
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
fn pairing_frames(snapshot: &Snapshot, tracked: &TrackedEntity, index: &PassengerIndex) -> Vec<u8> {
    let mut frames = encode_frames(&BundleDelimiter);
    for body in tracked.pairing_packets_in(snapshot, index) {
        frames.extend(frame_from_body(&body));
    }
    frames.extend(encode_frames(&BundleDelimiter));
    frames
}

/// 把「ID + 包体」加上帧长前缀；对拍比的是前者，线上帧是后者。
///
/// Add the frame length prefix to an "id + body" sequence; the oracle compares
/// the former, the wire carries the latter.
fn frame_from_body(body: &[u8]) -> Vec<u8> {
    let mut frames = Vec::with_capacity(body.len() + 3);
    let header = mc_bamboo_protocol::codec::encode_varint(body.len() as i32);
    frames.extend_from_slice(&header.0[..header.1]);
    frames.extend_from_slice(body);
    frames
}

/// 官方实体的 UUID 是随机的；这里由实体 ID 推出来，同一个实体每次重启都一样，
/// 测试才比得了。客户端只要求它唯一。存档里的 `UUID` 用同一份推导。
fn entity_uuid(id: i32) -> Uuid {
    Uuid::from_u128(mc_bamboo_blocks::archive::entity_uuid(id))
}

/// 每 tick 调用一次。对应 `ServerEntity.sendChanges`：核心逻辑全在
/// [`TrackedEntity::step`]，这里只把产出的包加上帧长、发给观察者。
///
/// Once per tick, vanilla `ServerEntity.sendChanges`. All decisions live in
/// [`TrackedEntity::step`]; this wrapper only frames the packets and sends them.
fn send_changes(snapshot: &Snapshot, tracked: &mut TrackedEntity, links: &mut Links) {
    // 官方 `ServerEntity.sendChanges` 先比乘客名单，再发位置和元数据。
    // Vanilla `ServerEntity.sendChanges` compares the passengers first, then position and metadata.
    if snapshot.passengers != tracked.last_passengers {
        let body = packet_body(&SetPassengers {
            vehicle: snapshot.id,
            passengers: &snapshot.passengers,
        });
        let frames = frame_from_body(&body);
        for &viewer in &tracked.seen_by {
            links.send_frames(viewer, &frames);
        }
        tracked.last_passengers.clone_from(&snapshot.passengers);
    }
    let needs_sync = snapshot.needs_sync;
    let sync_velocity = snapshot.sync_velocity;
    let mut frames = Vec::new();
    for body in tracked.step(snapshot, needs_sync, sync_velocity) {
        frames.extend(frame_from_body(&body));
    }
    if !frames.is_empty() {
        tracked.send_to_viewers(links, &frames);
    }
    let data = snapshot.data.pairing_values();
    if data != tracked.last_data {
        if let Some(fuse) = snapshot.data.fuse() {
            tracked.last_fuse = fuse;
        }
        if let Some(stack) = snapshot.data.stack() {
            tracked.last_stack = stack.clone();
        }
    }
}

/// `UpdateInterval.periodic`。`update_interval` 是 -1 时表示 `NEVER`。
fn update_interval(entity_type: i32) -> Option<i32> {
    let interval = entity_types::FACTS[entity_type as usize].update_interval;
    (interval > 0).then_some(interval)
}

/// `positionCodec.delta(position).lengthSqr() >= 7.6293945E-6F`。
fn has_moved(base: Vec3, position: Vec3) -> bool {
    position.distance_to_sqr(base) >= POSITION_TOLERANCE
}

/// 这一周期要发的移动包。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Movement {
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
