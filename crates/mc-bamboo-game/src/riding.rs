//! 骑乘：上下载具、载具的位置来自客户端、骑手的位置跟着载具走。
//!
//! 官方把玩家控制的载具当成客户端权威：服务端不模拟它的运动（`canSimulateMovement()`
//! 为假），位置只从 `ServerboundMoveVehiclePacket` 来，服务端做移动校验；
//! 骑手的位置在 `rideTick → positionRider` 里由载具决定。这里照这个形状实现：
//! 每刻 `tick` 先处理下车和骑手位置，`on_move_vehicle` 处理车里报上来的位置。
//!
//! Riding: mounting and dismounting, the client-authoritative vehicle position and
//! the rider following it. Vanilla treats a player-controlled vehicle as
//! client-authoritative: the server does not simulate it (`canSimulateMovement()`
//! is false), the position only comes from `ServerboundMoveVehiclePacket` and is
//! validated, and the rider's position is set by `rideTick → positionRider`.
//! This module keeps that shape: [`tick`] handles dismounting and rider
//! placement, [`on_move_vehicle`] takes the position the client reports.

use std::collections::BTreeMap;

use mc_bamboo_blocks::entity::{
    EntityUid, FIRST_ENTITY_ID, KindPublic, no_block_collision, sweep_movement,
};
use mc_bamboo_blocks::{CollisionContext, EntityContext, world::CollisionBlocks};
use mc_bamboo_core::{BlockPos, Hand, Vec3, mth};
use mc_bamboo_data::registries::attribute;
use mc_bamboo_protocol::packet::play::MoveVehicle;

use crate::{inbound::PlayerId, region::Region};

mod dismount;

/// 官方 `ServerGamePacketListenerImpl` 里和载具有关的那几个字段。
///
/// The vehicle fields of vanilla `ServerGamePacketListenerImpl`.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Riding {
    /// 这一刻骑着的载具。官方 `Entity.vehicle`。
    ///
    /// The vehicle ridden right now, vanilla `Entity.vehicle`.
    pub(crate) vehicle: Option<EntityUid>,
    /// 上一刻由这名玩家控制的载具。官方 `lastVehicle`，`move_vehicle` 只认它。
    ///
    /// The vehicle this player controlled last tick, vanilla `lastVehicle`;
    /// `move_vehicle` is only accepted for it.
    pub(crate) last_vehicle: Option<EntityUid>,
    /// 官方 `vehicleFirstGood*`：上一刻末的载具位置，用来判「移动太快」。
    ///
    /// Vanilla `vehicleFirstGood*`: the vehicle position at the end of the last
    /// tick, for the "moved too quickly" test.
    pub(crate) first_good: Vec3,
    /// 官方 `vehicleLastGood*`：上一次接受的载具位置。
    ///
    /// Vanilla `vehicleLastGood*`: the last accepted vehicle position.
    pub(crate) last_good: Vec3,
    /// 官方 `vehiclePositionLastResetAt`，用刻数差代替。
    ///
    /// Vanilla `vehiclePositionLastResetAt`, kept as a tick count.
    pub(crate) last_reset_at: i64,
    /// 官方 `Entity.boardingCooldown`：下车后 60 刻内不能再上。放在骑乘状态里，
    /// 因为它只由上下车改动。
    ///
    /// Vanilla `Entity.boardingCooldown`: 60 ticks after dismounting before another
    /// mount. It lives here because only mounting and dismounting change it.
    pub(crate) boarding_cooldown: i32,
}

impl Riding {
    /// `isPassenger()`。
    ///
    /// `isPassenger()`.
    pub(crate) fn is_passenger(&self) -> bool {
        self.vehicle.is_some()
    }
}

/// 一份载具的乘客名单（官方 `Entity.getPassengers()`）。玩家和生物共用同一份
/// `MobPublic.passengers`：玩家 ID 小于 [`FIRST_ENTITY_ID`]，生物 ID 不小于它。
/// `getFirstPassenger()` 就是名单里的第 0 项，控制乘客由 `MobPublic` 按这份名单算。
///
/// One vehicle's passenger list (vanilla `Entity.getPassengers()`): players and
/// mobs share `MobPublic.passengers`, with player ids below
/// [`FIRST_ENTITY_ID`]. `getFirstPassenger()` is its first entry.
pub(crate) fn passengers(region: &Region, vehicle: EntityUid) -> Vec<EntityUid> {
    region
        .entities
        .public_at(vehicle)
        .map(|public| match &public.kind {
            KindPublic::Mob(mob) => mob.passengers.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

/// 玩家 → 载具的完整索引，由唯一的乘客名单反推。玩家侧的 `Riding.vehicle` 必须
/// 和这份名单双向一致；不一致的那一侧由 [`check_consistency`] 报出来。
///
/// The player→vehicle index derived from the one passenger list. A player's
/// `Riding.vehicle` must agree; [`check_consistency`] reports mismatches.
pub(crate) fn player_vehicles(region: &Region) -> BTreeMap<PlayerId, EntityUid> {
    let mut out = BTreeMap::new();
    for public in region.entities.iter_public() {
        let KindPublic::Mob(mob) = &public.kind else {
            continue;
        };
        for &passenger in &mob.passengers {
            if passenger < FIRST_ENTITY_ID {
                out.insert(PlayerId(passenger), public.uid);
            }
        }
    }
    out
}

/// 双向一致：每名在线玩家的 `Riding.vehicle` 恰好对应载具名单里的一个玩家 ID，
/// 名单里的玩家 ID 也都有在线玩家的 `Riding.vehicle`。返回不一致的说明（调试
/// 不变量检查和单测用），空表表示一致。
///
/// Bidirectional invariant: every online player's `Riding.vehicle` has that
/// player in the vehicle's list and vice versa. Returns human-readable
/// mismatches (empty when consistent).
pub(crate) fn check_consistency(region: &Region) -> Vec<String> {
    let mut problems = Vec::new();
    let listed = player_vehicles(region);
    for (id, player) in &region.players {
        match (player.riding.vehicle, listed.get(id)) {
            (Some(vehicle), Some(&other)) if vehicle == other => {}
            (None, None) => {}
            (vehicle, found) => problems.push(format!(
                "player {:?}: riding.vehicle={vehicle:?}, passenger list={found:?}",
                id.0
            )),
        }
    }
    for (id, vehicle) in &listed {
        if !region.players.contains_key(id) {
            problems.push(format!(
                "passenger list has player {:?} on {vehicle} without an online player",
                id.0
            ));
        }
    }
    problems
}

/// 玩家所在骑乘树的全部实体 uid（根 + 嵌套生物乘客，按 uid 升序）。
/// 跨岛搬迁时按这张表把整棵树从原表搬到目标表，链接原样保留。
///
/// Every entity uid in the player's riding tree (root plus nested mob
/// passengers, uid-ascending). Cross-island moves transfer the whole tree
/// with the links intact.
pub(crate) fn riding_entities(region: &Region, id: PlayerId) -> Vec<EntityUid> {
    let Some(vehicle) = region.players.get(&id).and_then(|p| p.riding.vehicle) else {
        return Vec::new();
    };
    let mut stack = vec![root_vehicle(region, vehicle)];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(uid) = stack.pop() {
        if !seen.insert(uid) || seen.len() > 64 {
            break;
        }
        stack.extend(
            passengers(region, uid)
                .into_iter()
                .filter(|&passenger| passenger >= FIRST_ENTITY_ID),
        );
    }
    seen.into_iter().collect()
}

/// 玩家所在骑乘树的全部实体所在区块（拆岛、跨岛搬迁时整组一起搬）。
/// 玩家不在实体表里，从他直接骑的载具开始，按双向链接遍历整棵树。
///
/// All chunks of the riding tree the player belongs to (an island split or a
/// cross-island move takes the whole group). Players are not in the entity
/// table, so the walk starts at the player's direct vehicle.
pub(crate) fn riding_tree_chunks(
    region: &Region,
    id: PlayerId,
) -> std::collections::BTreeSet<mc_bamboo_core::ChunkPos> {
    let mut out = std::collections::BTreeSet::new();
    let Some(vehicle) = region.players.get(&id).and_then(|p| p.riding.vehicle) else {
        return out;
    };
    let mut stack = vec![root_vehicle(region, vehicle)];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(uid) = stack.pop() {
        if !seen.insert(uid) || seen.len() > 64 {
            break;
        }
        if let Some(public) = region.entities.public_at(uid) {
            out.insert(public.chunk_position());
        }
        stack.extend(
            passengers(region, uid)
                .into_iter()
                .filter(|&passenger| passenger >= FIRST_ENTITY_ID),
        );
    }
    out
}

/// 一棵骑乘树的根（官方 `Entity.getRootVehicle()`）：从玩家直接骑的载具沿
/// `Entity.vehicle` 往上走。玩家不在实体表里，所以第一跳从玩家的载具开始。
/// 坏数据里成环时停在 64 跳，不 panic。
///
/// The root of a riding tree (vanilla `Entity.getRootVehicle()`): walk up from
/// the player's direct vehicle. Players are not in the entity table, so the
/// first hop is their vehicle.
pub(crate) fn root_vehicle(region: &Region, vehicle: EntityUid) -> EntityUid {
    let mut root = vehicle;
    let mut seen = 0;
    while seen < 64
        && let Some(public) = region.entities.public_at(root)
        && let Some(next) = public.vehicle
    {
        root = next;
        seen += 1;
    }
    root
}

/// 整棵树里的玩家乘客数（官方 `countPlayerPassengers` 的玩家侧近似：只数名单
/// 里的玩家 ID）。`hasExactlyOnePlayerPassenger` 用它决定下线时是否移除载具。
///
/// How many player passengers the root's whole tree has (vanilla
/// `countPlayerPassengers`). `hasExactlyOnePlayerPassenger` uses it on leave.
pub(crate) fn player_passenger_count(region: &Region, root: EntityUid) -> usize {
    let mut out = 0;
    let mut stack = vec![root];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(uid) = stack.pop() {
        if !seen.insert(uid) || seen.len() > 64 {
            break;
        }
        for passenger in passengers(region, uid) {
            if passenger < FIRST_ENTITY_ID {
                out += 1;
            } else {
                stack.push(passenger);
            }
        }
    }
    out
}

/// 官方 `ServerPlayer.saveParentVehicle`：只有「骑着东西、根载具不是自己、整棵树
/// 恰好一名玩家乘客」时写 `RootVehicle`。`Attach` 是玩家**直接**骑着的载具的 UUID，
/// `Entity` 是根载具连整棵 `Passengers` 的存档（`Entity.save`）。
///
/// Vanilla `ServerPlayer.saveParentVehicle`: writes `RootVehicle` only while
/// riding, when the root is not the player and the tree has exactly one player
/// passenger. `Attach` is the UUID of the player's direct vehicle; `Entity` is
/// the root's whole `Passengers` tree.
pub(crate) fn root_vehicle_save(region: &Region, id: PlayerId) -> Option<mc_bamboo_nbt::Tag> {
    use mc_bamboo_blocks::archive::entity::write_entity_tree;
    use mc_bamboo_nbt::Tag;

    let player = region.players.get(&id)?;
    let vehicle = player.riding.vehicle?;
    let root = root_vehicle(region, vehicle);
    if root == id.0 || player_passenger_count(region, root) != 1 {
        return None;
    }
    let root_row = entity_row(region, root)?;
    let entity = write_entity_tree(&root_row, &|uid| entity_row(region, uid)).ok()?;
    Some(Tag::Compound(vec![
        ("Attach".to_owned(), attach_of(region, vehicle)?),
        ("Entity".to_owned(), entity),
    ]))
}

/// 存档里的 UUID（官方 `UUIDUtil.CODEC` 的四项 int array）。
fn attach_of(region: &Region, vehicle: EntityUid) -> Option<mc_bamboo_nbt::Tag> {
    let row = entity_row(region, vehicle)?;
    let bytes = mc_bamboo_blocks::archive::entity::entity_uuid_of(&row);
    let mut words = Vec::new();
    for chunk in bytes.chunks(4) {
        words.push(i32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Some(mc_bamboo_nbt::Tag::IntArray(words))
}

fn entity_row(region: &Region, uid: EntityUid) -> Option<mc_bamboo_blocks::entity::EntityRow> {
    let row = region.entities.row_of(uid)?;
    region.entities.row_at(row).cloned()
}

/// 官方 `ServerPlayer.loadAndSpawnParentVehicle`：把 `RootVehicle/Entity` 的根载具
/// 连整棵 `Passengers` 读回实体表，按 `Attach` 找到玩家该骑上的那一只，再
/// `startRiding(vehicle, force=true)`。读不出、找不到挂载点或不是生物时不改世界。
///
/// Vanilla `ServerPlayer.loadAndSpawnParentVehicle`: load the root and its whole
/// passenger tree, find the direct vehicle by `Attach` and mount the player.
pub(crate) fn load_root_vehicle(region: &mut Region, id: PlayerId, tag: &mc_bamboo_nbt::Tag) {
    use mc_bamboo_blocks::archive::entity::{entity_uuid_of, read_entity_tree};
    use mc_bamboo_nbt::Tag;

    let Tag::Compound(entries) = tag else {
        return;
    };
    let attach = entries
        .iter()
        .rev()
        .find(|(key, _)| key == "Attach")
        .and_then(|(_, value)| match value {
            Tag::IntArray(words) if words.len() == 4 => {
                let mut bytes = [0_u8; 16];
                for (index, word) in words.iter().enumerate() {
                    bytes[index * 4..index * 4 + 4].copy_from_slice(&(*word as u32).to_be_bytes());
                }
                Some(bytes)
            }
            _ => None,
        });
    let Some(entity_tag) = entries
        .iter()
        .rev()
        .find(|(key, _)| key == "Entity")
        .map(|(_, value)| value)
    else {
        return;
    };
    let mut next_uid = FIRST_ENTITY_ID;
    let Ok(Some(rows)) = read_entity_tree(entity_tag, &mut next_uid) else {
        return;
    };
    if rows.is_empty() {
        return;
    }
    // 官方 `loadEntityRecursive` 按 UUID 找挂载点；找不到时整棵树 discard。
    let attach = match attach {
        Some(bytes) => bytes,
        None => entity_uuid_of(&rows[0]),
    };
    // `read_entity_tree` 给的是占位号；按块读盘时 `ChunkEntities::from_parts` 会
    // 重新取号，这里也一样。号码计数器是各岛共用的原子量，别的岛可能同时在取号，
    // 不能先预测号码再放：先把引用摘掉、逐行放进实体表拿到真正的号，再按
    // 「占位号 → 新号」把整棵树里的 `vehicle` 和 `Passengers` 引用写回去。
    //
    // `read_entity_tree` hands out placeholder ids; chunk loading reassigns them in
    // `ChunkEntities::from_parts`, and so do we. The id counter is an atomic shared by
    // every island and another island may be allocating at the same time, so the ids
    // cannot be predicted: strip the links, spawn each row to get its real id, then
    // write the tree's `vehicle` and `Passengers` links back through the mapping.
    let mut links = Vec::with_capacity(rows.len());
    let mut spawned = Vec::with_capacity(rows.len());
    for mut row in rows {
        let uuid = entity_uuid_of(&row);
        let placeholder = row.public.uid;
        let vehicle = row.public.vehicle.take();
        let passengers = match &mut row.public.kind {
            mc_bamboo_blocks::entity::KindPublic::Mob(mob) => std::mem::take(&mut mob.passengers),
            _ => Vec::new(),
        };
        if let mc_bamboo_blocks::entity::KindState::Mob(mob) = &mut row.state.kind {
            mob.passengers.clear();
        }
        let uid = region.entities.spawn_row(row);
        links.push((uid, vehicle, passengers));
        spawned.push((uid, uuid, placeholder));
    }
    let remap: BTreeMap<EntityUid, EntityUid> = spawned
        .iter()
        .map(|&(uid, _, placeholder)| (placeholder, uid))
        .collect();
    for (uid, vehicle, passengers) in links {
        let vehicle = vehicle.and_then(|old| remap.get(&old).copied());
        let passengers: Vec<EntityUid> = passengers
            .iter()
            .filter_map(|old| remap.get(old).copied())
            .collect();
        if vehicle.is_none() && passengers.is_empty() {
            continue;
        }
        region.entities.with_mut(uid, |entity| {
            entity.vehicle = vehicle;
            entity.with_mob(|_, mob| mob.passengers = passengers);
        });
    }
    let spawned: Vec<(EntityUid, [u8; 16])> = spawned
        .into_iter()
        .map(|(uid, uuid, _)| (uid, uuid))
        .collect();
    let Some(&(vehicle, _)) = spawned.iter().find(|(_, uuid)| *uuid == attach) else {
        for (uid, _) in spawned {
            region.entities.remove(uid);
        }
        return;
    };
    // `startRiding(vehicle, true, false)`：强制上车，不看冷却和已有乘客。
    // 玩家侧的 `Riding.vehicle` 和载具名单都从这一份状态建起来。
    if let Some(player) = region.players.get_mut(&id) {
        player.riding.vehicle = Some(vehicle);
        player.riding.last_vehicle = Some(vehicle);
        player.riding.boarding_cooldown = 0;
        if let Some(public) = region.entities.public_at(vehicle) {
            player.riding.first_good = public.position;
            player.riding.last_good = public.position;
        }
    }
    region.entities.with_mut(vehicle, |entity| {
        entity.with_mob(|_, mob| {
            mc_bamboo_blocks::entity::riding::add_passenger(&mut mob.passengers, id.0, true);
        })
    });
}

/// 官方 `handleMoveVehicle` 的水平和竖直夹取（`clampHorizontal` / `clampVertical`）。
///
/// Vanilla `handleMoveVehicle`'s horizontal and vertical clamps.
fn clamp_horizontal(value: f64) -> f64 {
    value.clamp(-3.0e7, 3.0e7)
}

fn clamp_vertical(value: f64) -> f64 {
    value.clamp(-2.0e7, 2.0e7)
}

/// `Mth.wrapDegrees`。
///
/// `Mth.wrapDegrees`.
fn wrap_degrees(value: f32) -> f32 {
    mth::wrap_degrees(value)
}

/// 让这名玩家骑上 `vehicle`：官方 `Player.startRiding`，加上 `AbstractHorse.doPlayerRide` 里
/// 先设朝向的那一步。已经在骑同一只时什么都不做。
///
/// Puts the player on `vehicle`: vanilla `Player.startRiding` plus the facing
/// assignment `AbstractHorse.doPlayerRide` does first. Riding the same vehicle
/// already is a no-op.
pub(crate) fn mount(region: &mut Region, id: PlayerId, vehicle: EntityUid) {
    let Some(public) = region.entities.public_at(vehicle) else {
        return;
    };
    // 官方 `canAddPassenger`：默认只坐一个人；`HappyGhast` 覆写成最多四人。
    //
    // Vanilla `canAddPassenger`: one by default; `HappyGhast` allows up to four.
    let KindPublic::Mob(mob) = &public.kind else {
        return;
    };
    let happy_ghast = mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::HappyGhast;
    if happy_ghast {
        if !mc_bamboo_blocks::entity::mob::happy_ghast::can_add_passenger(mob.passengers.len()) {
            return;
        }
    } else if !mob.passengers.is_empty() {
        return;
    }
    let (yaw, pitch, position) = (public.yaw, mob.x_rot, public.position);
    let (box_width, box_height) = public.kind.dimensions();
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if player.riding.vehicle == Some(vehicle) {
        return;
    }
    // 官方 `canRide`：按着潜行键或还在下车冷却里就不能上。官方先判这一条，
    // 再 `stopRiding` 旧载具，所以下车设的冷却不会拦住这次上车。
    //
    // Vanilla `canRide`: the sneak key or the dismount cooldown blocks mounting.
    // Vanilla checks this before `stopRiding` on the old vehicle, so the cooldown
    // that dismount sets does not block this mount.
    if player.sync.shift_key_down || player.riding.boarding_cooldown > 0 {
        return;
    }
    // 官方 `startRiding`：已经骑着别的载具时先 `stopRiding`。
    //
    // Vanilla `startRiding`: a passenger already on another vehicle dismounts first.
    if player.riding.is_passenger() {
        dismount(region, id);
    }
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    // `doPlayerRide` 先把玩家的朝向设成载具的，再 `startRiding`。
    //
    // `doPlayerRide` copies the vehicle's facing to the player before `startRiding`.
    player.yaw = yaw;
    player.head_yaw = yaw;
    player.pitch = pitch;
    player.riding.vehicle = Some(vehicle);
    player.riding.last_vehicle = Some(vehicle);
    player.riding.first_good = position;
    player.riding.last_good = position;
    // 官方 `Entity.addPassenger`：玩家乘客由服务端插到生物乘客前面，名单是
    // 一份，玩家和生物共用。先设 `vehicle` 再挂名单，正是 `startRiding` 的前提。
    // 快乐恶魂另跑 `HappyGhast.addPassenger` 的护目镜音效和悬停超时。
    // Happy ghasts also run `HappyGhast.addPassenger`'s goggle sound and hover.
    if happy_ghast {
        let (min, max) = mc_bamboo_blocks::entity::mob::happy_ghast::player_above_box(
            position, box_width, box_height,
        );
        let player_above = region.players.values().any(|other| {
            other.id != id
                && other.game_mode != mc_bamboo_protocol::packet::play::GameMode::Spectator
                && other.riding.vehicle != Some(vehicle)
                && mc_bamboo_blocks::entity::mob::happy_ghast::is_above(other.position(), min, max)
        });
        region.entities.with_mut(vehicle, |entity| {
            let was_vehicle = entity
                .kind
                .mob()
                .is_some_and(|mob| !mob.passengers.is_empty());
            let at = entity.position;
            entity.with_mob(|_, mob| {
                mc_bamboo_blocks::entity::riding::add_passenger(&mut mob.passengers, id.0, true);
                mc_bamboo_blocks::entity::mob::happy_ghast::after_add_passenger(
                    at,
                    mob,
                    was_vehicle,
                    player_above,
                );
            });
        });
    } else {
        region.entities.with_mut(vehicle, |entity| {
            entity.with_mob(|_, mob| {
                mc_bamboo_blocks::entity::riding::add_passenger(&mut mob.passengers, id.0, true);
            })
        });
    }
    // 官方 `StartRidingTrigger.trigger(ServerPlayer)`。
    crate::advancement_triggers::record(
        region,
        id,
        "minecraft:started_riding",
        serde_json::json!({}),
    );
}

/// 方块层生物乘客的 `stopRiding`：摘掉乘客自己的 `vehicle`、给 60 刻冷却，
/// 位置不动（官方只有玩家 `Avatar.removeVehicle` 会重新落位）。载具名单由
/// 调用方在同一刻摘，和玩家侧共用一份名单。
///
/// The block-layer mob passenger's `stopRiding`: clear its `vehicle` link and
/// set the 60-tick cooldown, without moving it (vanilla repositions only
/// players). The caller drops the vehicle's list in the same step.
pub(crate) fn entity_stop(region: &mut Region, passenger: EntityUid, vehicle: EntityUid) {
    if passenger < FIRST_ENTITY_ID {
        return;
    }
    region.entities.with_mut(passenger, |entity| {
        if entity.vehicle == Some(vehicle) {
            mc_bamboo_blocks::entity::riding::dismount(entity, None);
        }
    });
}

/// 共用的 `stopRiding` / `removePassenger`：载具死亡、被移除、`/ride dismount`、
/// 按潜行键下车全走这里。玩家按官方 `getDismountLocationForPassenger` 找落点，
/// 生物乘客由方块层在同一刻停原地；两边关系一起摘，`set_passengers` 由同步阶段
/// 按名单变化发一次。
///
/// The shared `stopRiding` / `removePassenger`: vehicle death, removal,
/// `/ride dismount` and sneaking all land here. Players use vanilla's
/// `getDismountLocationForPassenger`; mob passengers stay put in the block
/// layer. Both sides are cleared and `set_passengers` is sent once by the sync
/// stage when the list changes.
pub(crate) fn stop_riding(region: &mut Region, id: PlayerId) {
    dismount(region, id);
}

/// 下车。官方 `Player.rideTick → stopRiding`：按载具的位置算好落点，清掉两边的关系，
/// 60 刻内不能再上。
///
/// Dismounts. Vanilla `Player.rideTick → stopRiding`: compute the landing spot from
/// the vehicle, clear both sides and block mounting for 60 ticks.
pub(crate) fn dismount(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let Some(vehicle) = player.riding.vehicle else {
        return;
    };
    // 官方 `stopRiding → dismountVehicle → getDismountLocationForPassenger`：先按载具
    // 现在的姿态找落点，再摘关系。载具已经不在表里时官方也保持原位。
    //
    // Vanilla `stopRiding → dismountVehicle → getDismountLocationForPassenger`: find
    // the spot from the vehicle's current pose, then clear the links. A vehicle that
    // already left the table leaves the player where it is, as vanilla does.
    let target = region.entities.public_at(vehicle).and_then(|public| {
        let KindPublic::Mob(mob) = &public.kind else {
            return None;
        };
        let (width, height, _) = mob.mob_type.dimensions(mob.baby);
        // `HappyGhast.getDismountLocationForPassenger` = `(x, bb.maxY, z)`，
        // 不是 `Animal` 的八邻格版本。
        // `HappyGhast.getDismountLocationForPassenger` = `(x, bb.maxY, z)`,
        // not `Animal`'s eight-neighbour version.
        if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::HappyGhast {
            return Some(
                mc_bamboo_blocks::entity::mob::happy_ghast::dismount_location(
                    public.position,
                    height,
                ),
            );
        }
        // 猪没有自己的下车逻辑，走 `Animal` 的八邻格版本；马科走 `AbstractHorse` 的。
        //
        // Pigs have no dismount of their own and use `Animal`'s eight-neighbour version;
        // equines use `AbstractHorse`'s.
        if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Pig {
            return Some(dismount::animal_dismount_location(
                &region.chunks,
                &region.global.world_border,
                player,
                public.position,
                f64::from(height),
                public.yaw,
            ));
        }
        dismount::dismount_location(
            &region.chunks,
            &region.global.world_border,
            player,
            public.position,
            f64::from(width),
            f64::from(height),
            public.yaw,
        )
    });
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let Some(vehicle) = player.riding.vehicle.take() else {
        return;
    };
    player.riding.last_vehicle = None;
    // 官方 `Entity.removePassenger`：乘客下车后 60 刻内不能再上。
    //
    // Vanilla `Entity.removePassenger`: the passenger cannot mount again for 60 ticks.
    player.riding.boarding_cooldown = 60;
    if let Some(target) = target {
        player.set_position(&mut region.player_grid, target);
    }
    region.entities.with_mut(vehicle, |entity| {
        let at = entity.position;
        let happy = entity
            .kind
            .mob()
            .is_some_and(|mob| mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::HappyGhast);
        entity.with_mob(|_, mob| {
            mob.passengers.retain(|passenger| *passenger != id.0);
            // `HappyGhast.removePassenger`：设 10 刻悬停、没人时清家并抬起护目镜。
            // `HappyGhast.removePassenger`: ten ticks of hover, clear the home and
            // raise the goggles once empty.
            if happy {
                mc_bamboo_blocks::entity::mob::happy_ghast::after_remove_passenger(at, mob);
            }
        })
    });
}

/// `OPEN_INVENTORY`：骑着驯服的马科时打开坐骑物品栏。官方
/// `AbstractHorse.openCustomInventoryScreen` 的条件是「没人在骑，或者骑的就是这名玩家」
/// 再加 `isTamed()`；骑着的时候前一半恒真。
///
/// `OPEN_INVENTORY`: open the mount's inventory while riding a tamed equine.
/// Vanilla `AbstractHorse.openCustomInventoryScreen` needs "not ridden, or ridden
/// by this player" and `isTamed()`; the first half is always true while riding.
pub(crate) fn open_vehicle_inventory(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let Some(vehicle) = player.riding.vehicle else {
        return;
    };
    let Some(public) = region.entities.public_at(vehicle) else {
        return;
    };
    let KindPublic::Mob(mob) = &public.kind else {
        return;
    };
    if !mob.passengers.contains(&id.0) {
        return;
    }
    let tamed = region
        .entities
        .with_mut(vehicle, |row| {
            row.with_mob(|_, mob| {
                mob.tame.is_tame() || mob.equine.as_ref().is_some_and(|state| state.tame)
            })
        })
        .flatten()
        .unwrap_or(false);
    if !tamed {
        return;
    }
    crate::menu::station::open_mount(region, id, vehicle);
}

/// 载具上骑手该站的位置：官方 `AbstractHorse.getPassengerAttachmentPoint`（默认附着点加
/// 扬蹄偏移）减去骑手的 `VEHICLE` 附着点（玩家在脚底，是 0）。羊驼覆写时不加扬蹄偏移。
///
/// Where the rider sits on the vehicle: vanilla `AbstractHorse.getPassengerAttachmentPoint`
/// (the default point plus the rearing offset) minus the rider's own `VEHICLE`
/// attachment point (the feet, zero for a player). Llamas override it without rearing.
fn passenger_offset(
    mob: &mc_bamboo_blocks::entity::mob::MobPublic,
    yaw: f32,
    index: usize,
) -> Vec3 {
    // `Vec3.yRot(-yRot * PI / 180)`，和官方附着点的旋转方向相同。
    //
    // `Vec3.yRot(-yRot * PI / 180)`, the same direction as the vanilla attachment point.
    let angle = -yaw * (std::f32::consts::PI / 180.0);
    let sin = f64::from(mth::sin(f64::from(angle)));
    let cos = f64::from(mth::cos(f64::from(angle)));
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::HappyGhast {
        // `EntityTypes.HAPPY_GHAST` 的四个 PASSENGER 点；按乘客下标 clamp。
        // The four PASSENGER points of `EntityTypes.HAPPY_GHAST`, clamped by index.
        let at = mc_bamboo_blocks::entity::mob::happy_ghast::passenger_attachment(index);
        return Vec3::new(at.x * cos + at.z * sin, at.y, at.z * cos - at.x * sin);
    }
    let (y, z) = mc_bamboo_blocks::entity::mob::equine::passenger_attachment(mob.mob_type);
    let mut offset = Vec3::new(f64::from(z) * sin, f64::from(y), f64::from(z) * cos);
    if !mob.mob_type.is_llama()
        && let Some(equine) = mob.equine
        && equine.stand_anim_o != 0.0
    {
        let stand = f64::from(equine.stand_anim_o);
        let rear_z = -0.7 * stand;
        offset = offset.add(Vec3::new(rear_z * sin, 0.15 * stand, rear_z * cos));
    }
    offset
}

/// 这名玩家在同一份乘客名单里的下标（`getPassengers().indexOf`）。
///
/// This player's index in the shared passenger list (`getPassengers().indexOf`).
fn passenger_index(region: &Region, vehicle: EntityUid, player: EntityUid) -> usize {
    passengers(region, vehicle)
        .iter()
        .position(|&uid| uid == player)
        .unwrap_or(0)
}

/// 每刻一趟：要下车的下车，其余骑手的位置跟到载具上，最后记下这一刻末的载具位置。
///
/// One pass per tick: riders that want off dismount, the rest follow their
/// vehicle, and the vehicle position at the end of the tick is recorded.
pub(crate) fn tick(region: &mut Region) {
    // 官方 `Entity.baseTick`：下车冷却每刻减一，不管有没有在骑。
    //
    // Vanilla `Entity.baseTick`: the dismount cooldown ticks down every tick,
    // riding or not.
    for player in region.players.values_mut() {
        player.riding.boarding_cooldown = player.riding.boarding_cooldown.saturating_sub(1);
    }
    // 载具在实体阶段死亡 / 被移除 / `ejectPassengers`：方块层只记下被甩的玩家，
    // 官方的 `Avatar.removeVehicle` 落位在这里和玩家自己的阶段一起做。
    for ejected in region.entities.take_ejected_players() {
        stop_riding(region, PlayerId(ejected));
    }
    let riders: Vec<PlayerId> = region
        .players
        .iter()
        .filter(|(_, player)| player.riding.is_passenger())
        .map(|(id, _)| *id)
        .collect();
    for id in riders {
        let Some(player) = region.players.get(&id) else {
            continue;
        };
        // 官方 `Player.rideTick`：按着潜行键就下车。
        //
        // Vanilla `Player.rideTick`: the sneak key dismounts.
        if player.sync.shift_key_down {
            stop_riding(region, id);
            continue;
        }
        let vehicle = player.riding.vehicle.expect("刚刚筛过");
        let (is_passenger, position, offset) = {
            let Some(public) = region.entities.public_at(vehicle) else {
                stop_riding(region, id);
                continue;
            };
            let KindPublic::Mob(mob) = &public.kind else {
                stop_riding(region, id);
                continue;
            };
            (
                passengers(region, vehicle).contains(&id.0),
                public.position,
                passenger_offset(mob, public.yaw, passenger_index(region, vehicle, id.0)),
            )
        };
        // 官方 `ejectPassengers`：载具的乘客名单里没有这名玩家了，就当场摘掉两边的关系。
        // 未驯服的马科被骑时 `RunAroundLikeCrazyGoal` 就是这样甩人的。
        //
        // Vanilla `ejectPassengers`: once the vehicle's passenger list no longer
        // holds this player, both sides are cleared. That is how
        // `RunAroundLikeCrazyGoal` throws the rider off an untamed equine.
        if !is_passenger {
            stop_riding(region, id);
            continue;
        }
        let target = position.add(offset);
        // 官方 `ServerGamePacketListenerImpl.tickPlayer` 只在载具的
        // `getControllingPassenger()` 是这名玩家时才记 `lastVehicle`，而
        // `AbstractHorse.getControllingPassenger` 要求上了鞍。没上鞍的载具能骑，
        // 位置却由它自己的 AI 决定，`move_vehicle` 一律不认。
        //
        // Vanilla `tickPlayer` only records `lastVehicle` when the vehicle's
        // `getControllingPassenger()` is this player, and
        // `AbstractHorse.getControllingPassenger` needs a saddle. An unsaddled
        // mount can be ridden but its AI owns the position; `move_vehicle` is never
        // accepted for it.
        let saddled = controls_vehicle(region, id, vehicle);
        if let Some(player) = region.players.get_mut(&id) {
            player.motion.fall_distance = 0.0;
            player.riding.first_good = position;
            player.riding.last_good = position;
            player.riding.last_vehicle = saddled.then_some(vehicle);
            player.set_position(&mut region.player_grid, target);
        }
    }
    debug_assert!(
        check_consistency(region).is_empty(),
        "riding links disagree: {:?}",
        check_consistency(region)
    );
}

/// 官方 `getControllingPassenger() == player`：载具上了鞍，猪还要求这名玩家是第一位乘客、
/// 任一只手拿着胡萝卜钓竿。
///
/// Vanilla `getControllingPassenger() == player`: the vehicle is saddled, and a pig
/// additionally needs this player as first passenger holding a carrot on a stick in
/// either hand.
pub(crate) fn controls_vehicle(region: &mut Region, id: PlayerId, vehicle: EntityUid) -> bool {
    let holds_stick = region.players.get(&id).is_some_and(|player| {
        [Hand::Main, Hand::Off].into_iter().any(|hand| {
            player.inventory.held(hand).item == mc_bamboo_data::items::CARROT_ON_A_STICK
        })
    });
    let first_passenger = passengers(region, vehicle).first() == Some(&id.0);
    region
        .entities
        .with_mut(vehicle, |row| {
            row.with_mob(|_, mob| {
                if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Pig {
                    mc_bamboo_blocks::entity::mob::pig::is_saddled(mob)
                        && first_passenger
                        && holds_stick
                } else if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::HappyGhast {
                    // `HappyGhast.getControllingPassenger`：穿了挽具、不在悬停超时、
                    // 第一位乘客是玩家。
                    // `HappyGhast.getControllingPassenger`: harnessed, out of the
                    // hover timeout, with a player as first passenger.
                    mc_bamboo_blocks::entity::mob::happy_ghast::controls(
                        mob,
                        first_passenger.then_some(id.0),
                    )
                } else {
                    !mob.equipment
                        .get(mc_bamboo_protocol::packet::play::EquipmentSlot::Saddle)
                        .is_empty()
                }
            })
        })
        .flatten()
        .unwrap_or(false)
}

/// `handleMoveVehicle`：客户端报上来的载具位置。校验通过就落地，否则把它拉回去。
///
/// `handleMoveVehicle`: the vehicle position the client reported. It is stored
/// when it passes the checks, otherwise the client is snapped back.
pub(crate) fn on_move_vehicle(
    region: &mut Region,
    id: PlayerId,
    position: Vec3,
    yaw: f32,
    pitch: f32,
    on_ground: bool,
) {
    let now = region.tick.game_time();
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    if contains_invalid_values(position, yaw, pitch) {
        region.links.kick(
            id,
            &mc_bamboo_protocol::text::Component::translate(
                "multiplayer.disconnect.invalid_vehicle_movement",
                [],
            ),
        );
        return;
    }
    let Some(vehicle) = player.riding.vehicle else {
        return;
    };
    // 只有「上一刻由这名玩家控制的那只」才收位置；其余的按官方那样把载具位置发回去。
    // 官方在「距上次重置超过 20 刻」或「包里的位置离载具超过 1 格」时重置。
    //
    // Only the vehicle this player controlled last tick is accepted; otherwise the
    // vehicle position is sent back, as vanilla does. Vanilla resets after 20 ticks
    // or as soon as the packet position is more than one block from the vehicle.
    if player.riding.last_vehicle != Some(vehicle) {
        let snap = region.entities.public_at(vehicle).map(|public| {
            let pitch = match &public.kind {
                KindPublic::Mob(mob) => mob.x_rot,
                _ => player.pitch,
            };
            (public.position, public.yaw, pitch)
        });
        let far = snap.is_some_and(|(at, _, _)| position.distance_to_sqr(at) > 1.0);
        if far || now.saturating_sub(player.riding.last_reset_at) > 20 {
            player.riding.last_reset_at = now;
            if let Some((position, yaw, pitch)) = snap {
                region.links.send(
                    id,
                    &MoveVehicle {
                        position,
                        yaw,
                        pitch,
                    },
                );
            }
        }
        return;
    }
    let target = Vec3::new(
        clamp_horizontal(position.x),
        clamp_vertical(position.y),
        clamp_horizontal(position.z),
    );
    let target_yaw = wrap_degrees(yaw);
    let target_pitch = wrap_degrees(pitch);
    let Some(public) = region.entities.public_at(vehicle) else {
        return;
    };
    let KindPublic::Mob(mob) = &public.kind else {
        return;
    };
    // 「移动太快」：和上一刻末的位置差太多就拉回去，官方阈值是 100 个平方格。
    //
    // "Moved too quickly": too far from the end of the last tick snaps back; the
    // vanilla threshold is 100 squared blocks.
    let moved = target
        .add(player.riding.first_good.scale(-1.0))
        .length_sqr();
    let expected = public.delta.length_sqr();
    if moved - expected > 100.0 {
        // 官方同样用 `ClientboundMoveVehiclePacket.fromEntity(vehicle)`，位置和朝向
        // 都是载具的当前值。
        //
        // Vanilla also uses `fromEntity(vehicle)`: the vehicle's current position
        // and rotation.
        let back = MoveVehicle {
            position: public.position,
            yaw: public.yaw,
            pitch: mob.x_rot,
        };
        region.links.send(id, &back);
        return;
    }
    // 官方 `vehicle.move(MoverType.PLAYER, 从 lastGood 到包里的位置)`：先用方块碰撞
    // 走一遍，再和客户端报的位置比。载具的位置由客户端模拟，服务端这一步只做校验。
    //
    // Vanilla `vehicle.move(MoverType.PLAYER, from lastGood to the packet position)`:
    // sweep the blocks once, then compare with the client's position. The client
    // simulates the vehicle; the server only validates here.
    let start = public.position;
    let old_box = public.bounding_box();
    let step = mob.attributes.value(attribute::STEP_HEIGHT) as f32;
    let context = vehicle_context(start);
    let blocks = crate::player_move::PlayerBlocks::new(&region.chunks, &region.block_entities);
    let border = region.global.world_border.clone();
    let swept = start.add(sweep_movement(
        target.subtract(player.riding.last_good),
        old_box,
        step,
        public.on_ground,
        context,
        CollisionBlocks {
            block_at: |pos| blocks.state(pos),
            piston_at: |pos| blocks.piston(pos),
            shulker_at: |pos| blocks.shulker(pos),
        },
        &border,
    ));
    let x_error = target.x - swept.x;
    let mut y_error = target.y - swept.y;
    // 官方这个条件恒为真，竖直误差不计入 moved wrongly。
    //
    // This condition is always true in vanilla, so vertical error never counts as moved wrongly.
    if y_error > -0.5 || y_error < 0.5 {
        y_error = 0.0;
    }
    let z_error = target.z - swept.z;
    let moved_wrongly = x_error * x_error + y_error * y_error + z_error * z_error;
    // 官方 `level.noCollision(vehicle, oldAABB)` 是方块和边界都不碰。实体之间的碰撞
    // （`isEntityCollidingWithAnythingNew`）Bamboo 没有建模，见 README 的已知限制。
    //
    // Vanilla `level.noCollision(vehicle, oldAABB)` means no block or border collision.
    // Entity-entity collisions (`isEntityCollidingWithAnythingNew`) are not modelled in
    // Bamboo; see the known limitations in the README.
    let old_clear = no_block_collision(
        old_box,
        context,
        CollisionBlocks {
            block_at: |pos: BlockPos| blocks.state(pos),
            piston_at: |pos| blocks.piston(pos),
            shulker_at: |pos| blocks.shulker(pos),
        },
    ) && border.no_border_collision(start.x, start.z, old_box);
    if moved_wrongly > 0.0625 && old_clear {
        // 官方把位置拉回移动前，朝向仍用包里的，再把载具位置发回客户端。
        //
        // Vanilla snaps the position back to where it was before the move, keeps
        // the packet's rotation and sends the vehicle position back.
        let back = MoveVehicle {
            position: start,
            yaw: target_yaw,
            pitch: target_pitch,
        };
        region.links.send(id, &back);
        region.entities.with_mut(vehicle, |entity| {
            set_pose(entity, start, target_yaw, target_pitch)
        });
        return;
    }
    player.riding.last_good = target;
    region.entities.with_mut(vehicle, |entity| {
        set_pose(entity, target, target_yaw, target_pitch);
        entity.on_ground = on_ground;
    });
}

/// 官方的 `absSnapTo(x, y, z, yRot, xRot)`：位置、朝向、头身朝向一起设，不改落地标记。
///
/// Vanilla `absSnapTo(x, y, z, yRot, xRot)`: position, rotation and head/body yaw in
/// one go, without touching the ground flag.
fn set_pose(entity: &mut mc_bamboo_blocks::entity::Entity, position: Vec3, yaw: f32, pitch: f32) {
    entity.position = position;
    entity.y_rot = yaw;
    entity.with_mob(|_, mob| {
        mob.x_rot = mc_bamboo_blocks::entity::mob::control::clamp_x_rot(pitch, mob.x_rot);
        mob.y_body_rot = yaw;
        mob.y_head_rot = yaw;
    });
}

/// 载具的碰撞上下文：官方 `CollisionContext.of(vehicle)`。马科不会潜行，
/// `isDescending()` 恒假；也不在 `powder_snow_walkable_mobs` 里。
///
/// The vehicle's collision context, vanilla `CollisionContext.of(vehicle)`. Equines
/// never sneak, so `isDescending()` is false, and they are not powder-snow walkable.
fn vehicle_context(position: Vec3) -> CollisionContext {
    CollisionContext::Entity(EntityContext {
        fall_distance: 0.0,
        bottom: position.y,
        falling_block: false,
        descending: false,
        placement: false,
        powder_snow_walkable: false,
    })
}

/// 官方 `containsInvalidValues`。
///
/// Vanilla `containsInvalidValues`.
fn contains_invalid_values(position: Vec3, yaw: f32, pitch: f32) -> bool {
    !position.x.is_finite()
        || !position.y.is_finite()
        || !position.z.is_finite()
        || !yaw.is_finite()
        || !pitch.is_finite()
}

#[cfg(test)]
mod tests;
