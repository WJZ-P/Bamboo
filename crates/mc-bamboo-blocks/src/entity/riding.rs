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

use crate::entity::ctx::BlockView;
use crate::entity::mob::control::{self, MoveControl};
use crate::entity::mob::living::nav_mob_own;
use crate::entity::mob::navigation::Navigation;
use crate::entity::mob::{Mob, MobType};
use crate::entity::{Entity, EntityPublic, EntityUid, KindPublic};

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

/// `Camel.canAddPassenger` 放宽到两个乘客；`startRiding(force=true)` 还会跳过
/// `canRide` / `canAddPassenger`，所以这里只保留防环和冷却。
///
/// `Camel.canAddPassenger` allows two passengers. `startRiding(force=true)` also
/// skips `canRide` / `canAddPassenger`, leaving only the cycle and cooldown checks.
pub fn can_start_riding_camel(
    passenger: EntityUid,
    passenger_vehicle: Option<EntityUid>,
    vehicle_chain: &[EntityUid],
    vehicle_passengers: &[EntityUid],
) -> bool {
    passenger_vehicle.is_none()
        && !vehicle_chain.contains(&passenger)
        && vehicle_passengers.len() < 2
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
    if mob.mob_type.is_camel() {
        // `Camel.getPassengerAttachmentPoint` 覆写了默认的 PASSENGER 点。
        // 站直且不在换姿势过渡时 `getBodyAnchorAnimationYOffset` 返回
        // `height - 0.375`；两名乘客时第一名在 z=+0.5，第二名在 z=-0.7
        // （乘客是 `Animal` 时再加 0.2）。坐着 / 过渡的动画偏移留给已知限制。
        //
        // `Camel.getPassengerAttachmentPoint` overrides the default PASSENGER
        // point. Standing and out of a pose transition, the body-anchor offset
        // is `height - 0.375`; with two passengers the first sits at z=+0.5 and
        // the second at z=-0.7 (plus 0.2 when the passenger is an `Animal`).
        // Sitting / transition animation offsets remain a known limitation.
        let first = index == 0;
        // `f6` 是 f32：`0.5F` 或者 `-0.7F`（`Animal` 乘客再加 `0.2F`），
        // 乘 `partialTick=1.0F` 后才转成双精度，所以 -0.7 要用 f32 的位。
        // `f6` is f32: `0.5F` or `-0.7F` (plus `0.2F` for an `Animal`),
        // multiplied by `partialTick=1.0F` before widening; keep the f32 bits.
        let z = f64::from(if mob.passengers.len() <= 1 || first {
            0.5_f32
        } else {
            -0.7_f32
        });
        return Vec3::new(0.0, f64::from(height - 0.375_f32), z);
    }
    // 注册表里的 PASSENGER 点是 JSON 里的双精度数，按 f64 原样用；`ridingOffset`
    // 给 VEHICLE 点是 f32，两者不能混（0.7 和 0.7f 差 1.19e-8）。
    // Registered PASSENGER points are doubles from JSON and stay f64 here; the
    // VEHICLE points from `ridingOffset` are f32, and mixing them would show.
    let (x, y, z): (f64, f64, f64) = match mob.mob_type {
        // 数值按官方 `EntityTypes` 里注册表实际存的 f64 / f32 位模式抄：有的点是
        // JSON 双精度字面量，有的是 f32 算出来的（0.765 是 0.7649999856948853）。
        // Values are the exact bits registered in vanilla `EntityTypes`: some are
        // JSON doubles, some came out of f32 math (0.765 is 0.7649999856948853).
        MobType::Spider => (0.0, 0.7649999856948853, 0.0),
        MobType::Chicken => (0.0, 0.7, -0.1),
        MobType::Cow | MobType::Mooshroom => (0.0, 1.368749976158142, 0.0),
        MobType::Sheep => (0.0, 1.2374999523162842, 0.0),
        MobType::Pig => (0.0, 0.8687499761581421, 0.0),
        MobType::Goat => (0.0, 1.1124999523162842, 0.0),
        MobType::Cat => (0.0, 0.512499988079071, 0.0),
        MobType::Ocelot => (0.0, 0.637499988079071, 0.0),
        MobType::Parrot => (0.0, 0.4625000059604645, 0.0),
        MobType::Silverfish | MobType::Endermite => (0.0, 0.23749999701976776, 0.0),
        MobType::Witch => (0.0, 2.262500047683716, 0.0),
        MobType::Enderman => (0.0, 2.8062500953674316, 0.0),
        MobType::Zombie | MobType::Drowned => (0.0, 2.012500047683716, 0.0),
        MobType::Husk => (0.0, 2.075000047683716, 0.0),
        MobType::Skeleton => (0.0, 1.9900000095367432, 0.0),
        MobType::Wolf => (0.0, 0.81875, -0.0625),
        MobType::Fox => (0.0, 0.6375, -0.25),
        MobType::Frog => (0.0, 0.375, -0.25),
        MobType::Turtle => (0.0, 0.55625, -0.25),
        MobType::Llama | MobType::TraderLlama => (0.0, 1.37, -0.3),
        MobType::Phantom => (0.0, 0.3375000059604645, 0.0),
        MobType::Horse => (0.0, 1.443750023841858, 0.0),
        MobType::Donkey => (0.0, 1.1124999523162842, 0.0),
        MobType::Mule => (0.0, 1.212499976158142, 0.0),
        // `EntityTypes.SKELETON_HORSE` / `ZOMBIE_HORSE` 都注册
        // `passengerAttachments([1.31875f])`，和马的高度 1.6 不同；没上鞍被生物骑时
        // `AbstractHorse.getPassengerAttachmentPoint` 的 standAnim 偏移为 0，退化成这个点。
        // Both SKELETON_HORSE and ZOMBIE_HORSE register
        // passengerAttachments([1.31875f]), unlike the horse's 1.6 height; with
        // standAnimO == 0 the AbstractHorse override reduces to this point.
        MobType::SkeletonHorse | MobType::ZombieHorse => (0.0, 1.318750023841858, 0.0),
        // `EntityTypes.NAUTILUS` / `ZOMBIE_NAUTILUS` 的 `passengerAttachments(1.1375F)`。
        // `EntityTypes.NAUTILUS` / `ZOMBIE_NAUTILUS` register
        // `passengerAttachments(1.1375F)`.
        MobType::Nautilus | MobType::ZombieNautilus => (0.0, 1.137_500_047_683_715_8, 0.0),
        _ => (0.0, f64::from(height), 0.0),
    };
    // 除骆驼外，已建模的载具都只有一个附着点，`getClamped` 恒取 0 号。
    // Other modelled vehicles have a single attachment point, so `getClamped`
    // always returns index 0.
    let _ = index;
    Vec3::new(x, y, z)
}

/// 乘客的 `EntityAttachment.VEHICLE` 附着点：缺省脚底 0；`EntityTypes.ridingOffset(-0.7)`
/// 的亡灵/骷髅等是 `(0, 0.7f, 0)`，掠夺者 `0.6f`，幻翼 `0.125f`。
///
/// 幼年僵尸（含尸壳、溺尸）不是把 0.7 折半：`Zombie` 覆写 `getDefaultDimensions`，
/// 幼年直接返回一张独立的 `BABY_DIMENSIONS = EntityDimensions.scalable(0.49f, 0.98f)
/// .withEyeHeight(0.775f).withAttachments(VEHICLE, 0, 0.1875f, 0)`，所以 VEHICLE 点是
/// 硬编码的 `0.1875f`。`getVehicleAttachmentPoint` 用乘客自己的 `getAttachments()`
/// （= `this.dimensions.attachments()`），因此这个点只按**乘客**的年龄变；载具的
/// PASSENGER 点则由 `getPassengerRidingPosition` 用**载具**自己的 `getDimensions`
/// （按载具 `getAgeScale() * getScale()` 缩放）决定——两处各按自己的实体缩放。
/// 鸡骑士里鸡是成年（不缩），幼年僵尸用 0.1875f，故骑手 y = 64 + 0.7 - 0.1875。
///
/// The passenger's `EntityAttachment.VEHICLE` point: zero at the feet by default;
/// `ridingOffset(-0.7)` (undead, skeletons) gives `(0, 0.7f, 0)`, pillagers `0.6f`,
/// phantoms `0.125f`. A baby zombie (incl. husk/drowned) is NOT half of 0.7:
/// `Zombie.getDefaultDimensions` returns a separate hardcoded `BABY_DIMENSIONS`
/// whose VEHICLE attachment is `0.1875f`. `getVehicleAttachmentPoint` reads the
/// passenger's own `getAttachments()`, so this point scales by the PASSENGER's age;
/// the vehicle's PASSENGER point (used by `getPassengerRidingPosition`) scales by
/// the VEHICLE's own `getAgeScale() * getScale()`. Each side uses its own entity.
/// In a chicken jockey the adult chicken is unscaled, so the rider sits at
/// y = 64 + 0.7 - 0.1875 = 64.5125.
fn vehicle_attachment(passenger: &EntityPublic) -> Vec3 {
    let KindPublic::Mob(mob) = &passenger.kind else {
        return Vec3::default();
    };
    let offset = match mob.mob_type {
        // `Zombie.BABY_DIMENSIONS` 的 VEHICLE 点；幼年用另一套尺寸，不是 0.7 折半。
        MobType::Zombie | MobType::Husk | MobType::Drowned if mob.baby => 0.1875_f32,
        MobType::Zombie
        | MobType::Husk
        | MobType::Drowned
        | MobType::Skeleton
        | MobType::Stray
        | MobType::Bogged
        | MobType::Parched => 0.7_f32,
        MobType::Pillager => 0.6_f32,
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

/// `Entity.removeVehicle` 的实体乘客版：摘掉两边关系，给乘客 60 刻冷却。
/// 官方只有玩家（`Avatar.removeVehicle`）会按 `getDismountLocationForPassenger` 挪位置；
/// 生物乘客停在被摘下的那一刻的位置，这里同样不挪。
///
/// The entity-passenger half of `Entity.removeVehicle`: clear both sides and give
/// the passenger a 60-tick cooldown. Vanilla repositions only players
/// (`Avatar.removeVehicle` calls `getDismountLocationForPassenger`); mob
/// passengers keep the position they had when unmounted, so this does not move.
pub fn dismount(passenger: &mut crate::entity::Entity, passengers: Option<&mut Vec<EntityUid>>) {
    if passenger.vehicle.is_none() {
        return;
    }
    passenger.vehicle = None;
    passenger.boarding_cooldown = 60;
    if let Some(list) = passengers {
        list.retain(|&uid| uid != passenger.id);
    }
}

/// `Entity.getDismountLocationForPassenger` 的缺省实现：载具 x、AABB 顶、载具 z。
/// `Animal` 的 `DismountHelper` 版本没有建模；这里只给「活着的载具」用。
///
/// Default `Entity.getDismountLocationForPassenger`: vehicle x, bounding-box top,
/// vehicle z. `Animal`'s `DismountHelper` version is not modelled; this is only for
/// a live vehicle.
pub fn dismount_location(vehicle: &EntityPublic) -> Vec3 {
    Vec3::new(
        vehicle.position.x,
        vehicle.bounding_box().max_y,
        vehicle.position.z,
    )
}

/// `LivingEntity.stopRiding()`：断关系后按载具是否已被移除决定落点。官方
/// `LivingEntity.dismountVehicle` 在载具还活着时用 `getDismountLocationForPassenger`
/// 重新落位（这里用缺省实现），载具已经离表时只保留 `max(自己 y, 载具 y)`，不水平挪动。
///
/// Vanilla `LivingEntity.stopRiding()` repositions to the dismount location while the
/// vehicle is alive; once the vehicle is removed it keeps `max(own y, vehicle y)` and
/// does not move horizontally.
pub fn stop_riding(
    passenger: &mut crate::entity::Entity,
    vehicle: &EntityPublic,
    passengers: Option<&mut Vec<EntityUid>>,
) {
    if passenger.vehicle.is_none() {
        return;
    }
    dismount(passenger, passengers);
    if !vehicle.removed {
        passenger.position = dismount_location(vehicle);
        passenger.old_position = passenger.position;
        // 官方 `teleportTo/snapTo` 不清速度：乘客这一刻的重力速度留着，下一刻作为根实体
        // 再叠一次重力。Vanilla `snapTo` keeps the velocity, so the root tick adds gravity again.
        passenger.on_ground = false;
    }
}

// ---- 骑手借载具导航（`Mob.getNavigation` / `getMoveControl` 的转交）----

/// 官方 `Mob.getControllingPassenger()`：载具不是 `NoAI`、第一位乘客就是这个实体、
/// 乘客是生物且 `canControlVehicle()`（不在 `#non_controlling_rider` 里；26.3 标签是
/// 史莱姆 / 岩浆怪 / 硫磺方块怪，建模范围内是史莱姆和硫磺方块怪）。满足时载具的
/// `getNavigation()` / `getMoveControl()` 会转给这个乘客所在的载具链顶端，所以骑手的
/// 目标与大脑用的是**载具**的导航。
///
/// Vanilla `Mob.getControllingPassenger()`: the vehicle is not NoAI, its first
/// passenger is this rider, and the rider can control vehicles. When true the
/// rider's goals and brain address the vehicle's navigation (and move control).
pub(crate) fn controls_vehicle(vehicle_mob: &Mob, rider_uid: EntityUid, rider_mob: &Mob) -> bool {
    if vehicle_mob.no_ai || vehicle_mob.first_passenger() != Some(rider_uid) {
        return false;
    }
    rider_mob.mob_type.can_control_vehicle()
}

/// 乘客 tick 之前：若它控制载具，就把载具的 `Navigation` / `MoveControl` 和按载具
/// 尺寸算出的 `NavMob` 快照换进骑手字段。`PathRequest`（独立寻路阶段的挂起请求）
/// 的 owner 就是 `snapshot.owner_uid`，这里天然是载具的 uid。
///
/// Before a passenger ticks: if it controls the vehicle, swap the vehicle's
/// navigation / move control and a vehicle-sized `NavMob` into the rider. The
/// pending `PathRequest` owner (`snapshot.owner_uid`) is then the vehicle's uid.
pub(crate) fn begin_nav_borrow(rider: &mut Entity, vehicle: &Entity, view: &dyn BlockView) -> bool {
    let (Some(rider_mob), Some(vehicle_mob)) = (rider.kind.mob_mut(), vehicle.kind.mob()) else {
        return false;
    };
    if !controls_vehicle(vehicle_mob, rider.id, rider_mob) {
        return false;
    }
    let snapshot = nav_mob_own(vehicle, vehicle_mob, view);
    let navigation = vehicle_mob.navigation.clone();
    let mover = vehicle_mob.mover;
    rider_mob.begin_nav_borrow(navigation, mover, snapshot);
    true
}

/// 乘客 tick 之后：取回共享状态，恢复骑手自己的导航与移动控制。调用方把返回值写回
/// 载具的对应字段。
///
/// After a passenger ticks: recover the shared state and restore the rider's
/// own. The caller writes the returned pair back to the vehicle.
pub(crate) fn end_nav_borrow(rider: &mut Entity) -> Option<(Navigation, MoveControl)> {
    rider.kind.mob_mut()?.end_nav_borrow()
}

/// 取走骑手这一刻记下的「载具也要看向目标」请求；只在借导航时有值。
///
/// Take the deferred `vehicle.lookAt` request recorded by the rider's bow goal
/// this tick; it is `None` unless the rider is borrowing the vehicle's navigation.
pub(crate) fn take_vehicle_look(rider: &mut Entity) -> Option<Vec3> {
    rider.kind.mob_mut()?.take_vehicle_look()
}

/// `RangedBowAttackGoal.tick()` 的 strafe 分支：官方对 `getControlledVehicle()` 的
/// `Mob` 调 `Mob.lookAt(target, 30, 30)`，从**载具**自身的位置/眼睛把载具的
/// `yRot`/`xRot` 用 `rotlerp` 转向目标（骑手自己也同时 `lookAt`，但那是另一份状态）。
/// BowGoal 在借导航的这一刻只记下目标眼睛位置，驱动在借导航写回时调用这里落到载具行。
///
/// `Mob.lookAt` 用实体自身 `getX/getZ/getEyeY` 和目标的 `getX/getZ/getEyeY` 算
/// yaw/pitch，再 `rotlerp(getYRot(), yaw, 30)` / `rotlerp(getXRot(), pitch, 30)`；
/// 只动 `yRot`、`xRot`，不碰 `yHeadRot` / `yBodyRot`（那两份由载具自己的
/// `LookControl.tick` 在这一刻早些时候算好，官方也不会被这次 lookAt 改写）。
///
/// `RangedBowAttackGoal.tick()`'s strafe branch calls `Mob.lookAt(target, 30, 30)`
/// on the controlled `Mob` vehicle, rotating the vehicle's own `yRot` / `xRot`
/// toward the target from its own eye via `rotlerp`. `Mob.lookAt` only writes
/// `yRot` / `xRot`; head/body rotation is left as the vehicle's own look-control
/// tick left it earlier this tick.
pub(crate) fn apply_vehicle_look(vehicle: &mut Entity, target_eye: Vec3) {
    let Some(mob) = vehicle.kind.mob_mut() else {
        return;
    };
    let dx = target_eye.x - vehicle.position.x;
    let dz = target_eye.z - vehicle.position.z;
    let dy = target_eye.y - (vehicle.position.y + f64::from(mob.eye_height));
    let horizontal = (dx * dx + dz * dz).sqrt();
    let degrees = |rad: f64| rad * f64::from(180.0_f32 / std::f32::consts::PI);
    let yaw = degrees(mth::atan2(dz, dx)) as f32 - 90.0;
    let pitch = -degrees(mth::atan2(dy, horizontal)) as f32;
    let new_y_rot = control::rotate_towards(vehicle.y_rot, yaw, 30.0);
    let new_x_rot = control::rotate_towards(mob.x_rot, pitch, 30.0);
    mob.x_rot = new_x_rot;
    vehicle.y_rot = new_y_rot;
}

/// 控制乘客的 `yBodyRot` 跟载具一致：载具转向时（例如追玩家转成 270°）乘客的
/// 身体朝向当场跟着跳到同一个角度，头朝向随后受 `clampHeadRotationToBody` 约束。
/// 官方 `AbstractSkeleton.rideTick()` / `Drowned.rideTick()` 显式这么抄
/// （`super.rideTick()` 后 `if (getControlledVehicle() instanceof PathfinderMob)
/// this.yBodyRot = vehicle.yBodyRot`）；对拍里蜘蛛骑士（骷髅）和鸡骑士（幼年僵尸）
/// 都要求这个结果，所以对所有控制乘客统一处理。`getControlledVehicle()` 要求
/// `vehicle.getControllingPassenger() == this`，正好是 [`controls_vehicle`]。
///
/// A controlling rider's `yBodyRot` matches the vehicle's: when the vehicle
/// turns the rider's body snaps to the same angle and the head is then clamped
/// to the body. `AbstractSkeleton.rideTick` / `Drowned.rideTick` do this
/// explicitly; the oracle requires the same result for both the spider jockey
/// (skeleton) and the chicken jockey (baby zombie), so it is applied to every
/// controlling rider here.
pub(crate) fn sync_rider_body_rotation(rider: &mut Entity, vehicle: &Entity) {
    let (Some(rider_mob), Some(vehicle_mob)) = (rider.kind.mob_mut(), vehicle.kind.mob()) else {
        return;
    };
    if controls_vehicle(vehicle_mob, rider.id, rider_mob) {
        rider_mob.y_body_rot = vehicle_mob.y_body_rot;
    }
}

/// 把共享导航/移动控制写回载具（乘客这一刻可能改了路径和 wanted 位置）。
///
/// Write the shared navigation / move control back to the vehicle (the rider
/// may have changed the path and wanted position this tick).
pub(crate) fn put_borrowed_nav(vehicle: &mut Entity, navigation: Navigation, mover: MoveControl) {
    if let Some(mob) = vehicle.kind.mob_mut() {
        mob.navigation = navigation;
        mob.mover = mover;
    }
}

/// 并行驱动用：直接改算阶段结果里的私有状态，不用拼回完整 [`Entity`]。
///
/// Parallel driver variant: mutate the private state in the computed result.
pub(crate) fn put_borrowed_nav_state(
    state: &mut crate::entity::EntityState,
    navigation: Navigation,
    mover: MoveControl,
) {
    if let crate::entity::KindState::Mob(mob) = &mut state.kind {
        mob.navigation = navigation;
        mob.mover = mover;
    }
}

#[cfg(test)]
mod tests;
