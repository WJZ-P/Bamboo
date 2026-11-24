//! 实体骑实体：官方 `Entity.startRiding` / `stopRiding` / `rideTick` / `positionRider`。
//!
//! 玩家不在实体表里，玩家上下载具仍由游戏层的 `riding` 模块管；这里只管实体表里的
//! 乘客。载具的乘客名单是 `Mob.passengers`（按上车顺序，玩家 ID 和生物 ID 混在一起），
//! 乘客自己的 `Entity.vehicle` 记着它的载具。`ServerLevel.tickNonPassenger` 按根实体
//! 递归 tick 乘客，具体驱动在 [`crate::world::World::tick_entities`]。
//!
//! Entity-on-entity riding: vanilla `Entity.startRiding` / `stopRiding` / `rideTick`
//! / `positionRider`. Players are not in the entity table and keep their own
//! mounting code in the game layer. A vehicle's passenger list lives on
//! `Mob.passengers` (players and entity ids in boarding order), while each entity
//! passenger remembers `Entity.vehicle`. `ServerLevel.tickNonPassenger` ticks
//! passengers recursively from their root; the drivers live in `World::tick_entities`.

use mc_bamboo_core::Vec3;
use mc_bamboo_core::mth;

use crate::entity::mob::MobType;
use crate::entity::{EntityPublic, EntityUid, KindPublic};

/// 官方 `Entity.addPassenger`：调用方负责保证 `passenger.vehicle` 已经先设成
/// `Some(vehicle)`（官方的前提），这里只把它挂进载具的乘客名单。服务端上「新乘客是
/// 玩家、第一位乘客不是玩家」时插到 0 号，否则追加；玩家 ID 都小于
/// [`FIRST_ENTITY_ID`](crate::entity::FIRST_ENTITY_ID)，世界实体 ID 都大于等于它。
/// `startRiding` 的 `canAddPassenger` 前置判定在 [`can_start_riding`]。
///
/// Vanilla `Entity.addPassenger`. The caller must already have set
/// `passenger.vehicle` (vanilla's precondition). On the server a new player
/// passenger goes ahead of a first non-player passenger; otherwise it appends.
/// `startRiding`'s `canAddPassenger` check lives in [`can_start_riding`].
pub fn add_passenger(
    passengers: &mut Vec<EntityUid>,
    passenger: EntityUid,
    passenger_is_player: bool,
) -> bool {
    if passengers.contains(&passenger) {
        return false;
    }
    if passenger_is_player
        && passengers
            .first()
            .is_some_and(|&first| first >= crate::entity::FIRST_ENTITY_ID)
    {
        passengers.insert(0, passenger);
    } else {
        passengers.push(passenger);
    }
    true
}

/// 官方 `startRiding(vehicle, force=false, ...)` 的前置判定，按字节码的顺序：
/// 载具链不能绕回自己、`canRide`（不潜行、冷却已过）、`canAddPassenger`
/// （官方基类要求载具当前没有乘客；船之类的覆写不在建模范围里）。
/// `vehicle_chain` 是从载具沿 `vehicle` 往上走的链条。
///
/// The preconditions of vanilla `startRiding(vehicle, false, ...)`, in bytecode
/// order: no cycle through the vehicle chain, `canRide` (not sneaking, cooldown
/// over) and `canAddPassenger` (the vanilla base class needs an empty passenger
/// list; overrides such as boats are out of scope). `vehicle_chain` walks the
/// vehicle's own `vehicle` links.
pub fn can_start_riding(
    passenger: EntityUid,
    passenger_vehicle: Option<EntityUid>,
    passenger_cooldown: i32,
    passenger_is_sneaking: bool,
    vehicle: EntityUid,
    vehicle_chain: &[EntityUid],
    vehicle_passengers: &[EntityUid],
) -> bool {
    if passenger_vehicle == Some(vehicle) || vehicle_chain.contains(&passenger) {
        return false;
    }
    !passenger_is_sneaking && passenger_cooldown <= 0 && vehicle_passengers.is_empty()
}

/// `EntityAttachments.transformPoint`：`Vec3.yRot(-yRot * 0.017453292f)`，全 f32 乘法。
///
/// `EntityAttachments.transformPoint`: `Vec3.yRot(-yRot * 0.017453292f)` in f32.
fn rotate(offset: Vec3, yaw: f32) -> Vec3 {
    let angle = -yaw * 0.017_453_292_f32;
    let cos = f64::from(mth::cos(f64::from(angle)));
    let sin = f64::from(mth::sin(f64::from(angle)));
    Vec3::new(
        offset.x * cos + offset.z * sin,
        offset.y,
        offset.z * cos - offset.x * sin,
    )
}

/// 载具的 `EntityAttachment.PASSENGER` 附着点（相对载具位置，按第 `index` 个乘客
/// clamp）。`EntityTypes` 给每个注册表类型定了一个点；没定点的类型回退到
/// `(0, height, 0)`。这里列的是已建模、可能被骑的类型。
///
/// The vehicle's `EntityAttachment.PASSENGER` point relative to its position,
/// clamped to the passenger's index. `EntityTypes` registers one point per type;
/// unregistered types fall back to `(0, height, 0)`. Only modelled rideable types
/// are listed.
fn passenger_attachment(vehicle: &EntityPublic, index: usize) -> Vec3 {
    let KindPublic::Mob(mob) = &vehicle.kind else {
        // 非生物载具（船、矿车）不在建模范围里；回退按尺寸算。
        let (_, height) = vehicle.kind.dimensions();
        return Vec3::new(0.0, f64::from(height), 0.0);
    };
    let (_, height) = mob.dimensions();
    let (x, y, z): (f32, f32, f32) = match mob.mob_type {
        MobType::Spider => (0.0, 0.765_f32, 0.0),
        MobType::Chicken => (0.0, 0.7_f32, -0.1_f32),
        MobType::Cow | MobType::Mooshroom => (0.0, 1.368_75_f32, 0.0),
        MobType::Sheep => (0.0, 1.2375_f32, 0.0),
        MobType::Pig => (0.0, 0.868_75_f32, 0.0),
        MobType::Goat => (0.0, 1.1125_f32, 0.0),
        MobType::Cat => (0.0, 0.5125_f32, 0.0),
        MobType::Ocelot => (0.0, 0.6375_f32, 0.0),
        MobType::Parrot => (0.0, 0.4625_f32, 0.0),
        MobType::Silverfish | MobType::Endermite => (0.0, 0.2375_f32, 0.0),
        MobType::Witch => (0.0, 2.2625_f32, 0.0),
        MobType::Enderman => (0.0, 2.806_25_f32, 0.0),
        MobType::Zombie | MobType::Drowned => (0.0, 2.0125_f32, 0.0),
        MobType::Husk => (0.0, 2.075_f32, 0.0),
        MobType::Wolf => (0.0, 0.818_75_f32, -0.0625_f32),
        MobType::Fox => (0.0, 0.6375_f32, -0.25_f32),
        MobType::Frog => (0.0, 0.375_f32, -0.25_f32),
        MobType::Turtle => (0.0, 0.556_25_f32, -0.25_f32),
        MobType::Llama => (0.0, 1.37_f32, -0.3_f32),
        MobType::Phantom => (0.0, 0.3375_f32, 0.0),
        MobType::Horse => (0.0, 1.443_75_f32, 0.0),
        MobType::Donkey => (0.0, 1.1125_f32, 0.0),
        MobType::Mule => (0.0, 1.2125_f32, 0.0),
        _ => (0.0, height, 0.0),
    };
    let _ = index; // 已建模的载具都只有一个附着点，`getClamped` 恒取 0 号。
    Vec3::new(x.into(), y.into(), z.into())
}

/// 乘客的 `EntityAttachment.VEHICLE` 附着点：缺省脚底 0；`EntityTypes.ridingOffset(-0.7)`
/// 的亡灵/骷髅/掠夺者等是 `(0, 0.7, 0)`，幻翼是 `(0, 0.125, 0)`。
///
/// The passenger's `EntityAttachment.VEHICLE` point: zero at the feet by default;
/// types registered with `ridingOffset(-0.7)` (undead, skeletons, pillagers) use
/// `(0, 0.7, 0)`, and phantoms `(0, 0.125, 0)`.
fn vehicle_attachment(passenger: &EntityPublic) -> Vec3 {
    let KindPublic::Mob(mob) = &passenger.kind else {
        return Vec3::default();
    };
    let offset = match mob.mob_type {
        MobType::Zombie
        | MobType::Husk
        | MobType::Drowned
        | MobType::Skeleton
        | MobType::Stray
        | MobType::Bogged
        | MobType::Pillager => 0.7_f32,
        MobType::Phantom => 0.125_f32,
        _ => 0.0,
    };
    let point = Vec3::new(0.0, f64::from(offset), 0.0);
    rotate(point, passenger.yaw)
}

/// `Entity.positionRider` 算出的乘客位置：载具位置 + 载具的 `PASSENGER` 点
/// （按载具朝向旋转）− 乘客自己的 `VEHICLE` 点（按乘客朝向旋转）。
///
/// The position computed by `Entity.positionRider`: vehicle position + the
/// vehicle's rotated `PASSENGER` point − the passenger's rotated `VEHICLE` point.
pub fn rider_position(vehicle: &EntityPublic, passenger: &EntityPublic, index: usize) -> Vec3 {
    let mount = rotate(passenger_attachment(vehicle, index), vehicle.yaw);
    let feet = vehicle_attachment(passenger);
    vehicle.position.add(mount).subtract(feet)
}

/// `Entity.getDismountLocationForPassenger` 的缺省实现：载具 x、AABB 顶、载具 z。
/// `Animal` 的 `DismountHelper` 版本没有建模（README 的已知限制）。
///
/// The default `Entity.getDismountLocationForPassenger`: vehicle x, bounding-box
/// top, vehicle z. `Animal`'s `DismountHelper` version is not modelled.
pub fn dismount_location(vehicle: &EntityPublic) -> Vec3 {
    Vec3::new(
        vehicle.position.x,
        vehicle.bounding_box().max_y,
        vehicle.position.z,
    )
}

/// `LivingEntity.stopRiding → dismountVehicle` 的落地：算好落点、摘掉两边关系、
/// 给乘客 60 刻冷却。乘客自己（`Entity.vehicle`）与载具名单的清理分开做，因为
/// 载具可能已经不在表里（被移除时）。
///
/// The landing half of `LivingEntity.stopRiding → dismountVehicle`: compute the
/// landing spot, clear both sides and give the passenger a 60-tick cooldown.
pub fn dismount(
    passenger: &mut crate::entity::Entity,
    vehicle_position: Option<Vec3>,
    passengers: Option<&mut Vec<EntityUid>>,
) {
    if passenger.vehicle.is_none() {
        return;
    }
    passenger.vehicle = None;
    passenger.boarding_cooldown = 60;
    if let Some(position) = vehicle_position {
        passenger.position = position;
        passenger.old_position = position;
        passenger.delta = Vec3::default();
    }
    if let Some(list) = passengers {
        list.retain(|&uid| uid != passenger.id);
    }
}

#[cfg(test)]
mod tests;
