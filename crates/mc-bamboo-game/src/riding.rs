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

use mc_bamboo_blocks::entity::{EntityUid, KindPublic};
use mc_bamboo_core::{Vec3, mth};
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
    // 官方 `canAddPassenger`：一只载具只坐一个人。
    //
    // Vanilla `canAddPassenger`: one passenger per vehicle.
    let KindPublic::Mob(mob) = &public.kind else {
        return;
    };
    if !mob.passengers.is_empty() {
        return;
    }
    let (yaw, pitch, position) = (public.yaw, mob.x_rot, public.position);
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    if player.riding.vehicle == Some(vehicle) {
        return;
    }
    // 官方 `canRide`：按着潜行键或还在下车冷却里就不能上。
    //
    // Vanilla `canRide`: the sneak key or the dismount cooldown blocks mounting.
    if player.sync.shift_key_down || player.riding.boarding_cooldown > 0 {
        return;
    }
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
    region.entities.with_mut(vehicle, |entity| {
        entity.with_mob(|_, mob| mob.passengers.push(id.0))
    });
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
        dismount::dismount_location(
            &region.chunks,
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
        entity.with_mob(|_, mob| mob.passengers.retain(|passenger| *passenger != id.0))
    });
}

/// 载具上骑手该站的位置：官方 `getPassengerAttachmentPoint` 的缺省点减去骑手的
/// `VEHICLE` 附着点（玩家在脚底，是 0）。
///
/// Where the rider sits on the vehicle: vanilla `getPassengerAttachmentPoint`'s
/// default point minus the rider's own `VEHICLE` attachment point (the feet, zero
/// for a player).
fn passenger_offset(mob: &mc_bamboo_blocks::entity::mob::MobPublic, yaw: f32) -> Vec3 {
    let (y, z) = mc_bamboo_blocks::entity::mob::equine::passenger_attachment(mob.mob_type);
    // `Vec3.yRot(-yRot * PI / 180)`，和官方附着点的旋转方向相同。
    //
    // `Vec3.yRot(-yRot * PI / 180)`, the same direction as the vanilla attachment point.
    let angle = -yaw * (std::f32::consts::PI / 180.0);
    Vec3::new(
        f64::from(z * mth::sin(f64::from(angle))),
        f64::from(y),
        f64::from(z * mth::cos(f64::from(angle))),
    )
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
            dismount(region, id);
            continue;
        }
        let vehicle = player.riding.vehicle.expect("刚刚筛过");
        let Some(public) = region.entities.public_at(vehicle) else {
            dismount(region, id);
            continue;
        };
        let KindPublic::Mob(mob) = &public.kind else {
            dismount(region, id);
            continue;
        };
        // 官方 `ejectPassengers`：载具的乘客名单里没有这名玩家了，就当场摘掉两边的关系。
        // 未驯服的马科被骑时 `RunAroundLikeCrazyGoal` 就是这样甩人的。
        //
        // Vanilla `ejectPassengers`: once the vehicle's passenger list no longer
        // holds this player, both sides are cleared. That is how
        // `RunAroundLikeCrazyGoal` throws the rider off an untamed equine.
        if !mob.passengers.contains(&id.0) {
            dismount(region, id);
            continue;
        }
        let offset = passenger_offset(mob, public.yaw);
        let target = public.position.add(offset);
        let yaw = public.yaw;
        if let Some(player) = region.players.get_mut(&id) {
            player.motion.fall_distance = 0.0;
            player.riding.first_good = public.position;
            player.riding.last_good = public.position;
            player.riding.last_vehicle = Some(vehicle);
            let _ = yaw;
            player.set_position(&mut region.player_grid, target);
        }
    }
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
    //
    // Only the vehicle this player controlled last tick is accepted; otherwise
    // the vehicle position is sent back, as vanilla does.
    if player.riding.last_vehicle != Some(vehicle) {
        if now.saturating_sub(player.riding.last_reset_at) > 20 {
            player.riding.last_reset_at = now;
            let mut snap = None;
            if let Some(public) = region.entities.public_at(vehicle) {
                snap = Some(public.position);
            }
            if let Some(position) = snap {
                let yaw = player.yaw;
                let pitch = player.pitch;
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
    // 「移动太快」：和上一刻末的位置差太多就拉回去，官方阈值是 100 个平方格。
    //
    // "Moved too quickly": too far from the end of the last tick snaps back; the
    // vanilla threshold is 100 squared blocks.
    let moved = target
        .add(player.riding.first_good.scale(-1.0))
        .length_sqr();
    let expected = public.delta.length_sqr();
    if moved - expected > 100.0 {
        let back = MoveVehicle {
            position: public.position,
            yaw: player.yaw,
            pitch: player.pitch,
        };
        region.links.send(id, &back);
        return;
    }
    player.riding.last_good = target;
    region.entities.with_mut(vehicle, |entity| {
        entity.position = target;
        entity.y_rot = target_yaw;
        entity.on_ground = on_ground;
        entity.with_mob(|_, mob| {
            mob.x_rot =
                mc_bamboo_blocks::entity::mob::control::clamp_x_rot(target_pitch, mob.x_rot);
            mob.y_body_rot = target_yaw;
            mob.y_head_rot = target_yaw;
        });
    });
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
