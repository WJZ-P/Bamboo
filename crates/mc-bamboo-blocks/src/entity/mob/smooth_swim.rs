//! 官方 `SmoothSwimmingMoveControl` 和 `SmoothSwimmingLookControl`：海豚、青蛙、蝌蚪、
//! 美西螈共用的游泳移动和视角控制。五个构造参数（竖直转向上限、水平转向上限、水里速度
//! 系数、陆上速度系数、是否上浮）由各自在 [`MoveParams`] 里给出。
//!
//! Vanilla `SmoothSwimmingMoveControl` and `SmoothSwimmingLookControl`, shared by the
//! dolphin, frog, tadpole and axolotl. The five constructor parameters (vertical and
//! horizontal turn limits, in-water and out-of-water speed factors, gravity) live in
//! each caller's [`MoveParams`].

use mc_bamboo_core::{Vec3, mth};
use mc_bamboo_data::registries::attribute;

use super::{
    Mob,
    control::{self, Operation},
    living,
};
use crate::entity::Entity;

/// `SmoothSwimmingMoveControl` 的构造参数。
///
/// The `SmoothSwimmingMoveControl` constructor parameters.
pub(super) struct MoveParams {
    /// `maxTurnX`。
    pub max_turn_x: f32,
    /// `maxTurnY`。
    pub max_turn_y: f32,
    /// `inWaterSpeedModifier`。
    pub in_water_speed: f32,
    /// `outsideWaterSpeedModifier`。
    pub outside_water_speed: f32,
    /// `applyGravity`。
    pub apply_gravity: bool,
}

/// 青蛙和蝌蚪的 `(85, 10, 0.02F, 0.1F, true)`。
///
/// The frog's and tadpole's `(85, 10, 0.02F, 0.1F, true)`.
pub(super) const FROG: MoveParams = MoveParams {
    max_turn_x: 85.0,
    max_turn_y: 10.0,
    in_water_speed: 0.02,
    outside_water_speed: 0.1,
    apply_gravity: true,
};

/// 美西螈的 `(85, 10, 0.1F, 0.5F, false)`。
///
/// The axolotl's `(85, 10, 0.1F, 0.5F, false)`.
pub(super) const AXOLOTL: MoveParams = MoveParams {
    max_turn_x: 85.0,
    max_turn_y: 10.0,
    in_water_speed: 0.1,
    outside_water_speed: 0.5,
    apply_gravity: false,
};

/// `SmoothSwimmingMoveControl.tick`：水里每刻按 `applyGravity` 上浮 0.005；有路要走时
/// 身体每刻最多转 `max_turn_y` 度，水里按俯仰把推进分给前进和竖直，陆地上转身越多走得
/// 越慢。它从不把操作改回 `WAIT`。
///
/// `SmoothSwimmingMoveControl.tick`: rise 0.005 a tick in water when `applyGravity`;
/// while following a path the body turns at most `max_turn_y` degrees a tick, thrust in
/// water is split between forward and vertical by pitch and on land turning slows it
/// down. It never resets the operation to `WAIT`.
pub(super) fn move_tick(me: &mut Entity, mob: &mut Mob, params: &MoveParams) {
    if params.apply_gravity && me.in_water {
        me.delta = me.delta.add(Vec3::new(0.0, 0.005, 0.0));
    }
    if mob.mover.operation != Operation::MoveTo || mob.navigation.is_done() {
        living::set_speed(mob, 0.0);
        mob.xxa = 0.0;
        mob.yya = 0.0;
        mob.zza = 0.0;
        return;
    }
    let xd = mob.mover.wanted_x - me.position.x;
    let yd = mob.mover.wanted_y - me.position.y;
    let zd = mob.mover.wanted_z - me.position.z;
    let dd = xd * xd + yd * yd + zd * zd;
    if dd < f64::from(2.500_000_3E-7_f32) {
        mob.zza = 0.0;
        return;
    }
    let y_rot_d = degrees(mth::atan2(zd, xd)) - 90.0;
    me.y_rot = control::move_rotlerp(me.y_rot, y_rot_d, params.max_turn_y);
    mob.y_body_rot = me.y_rot;
    mob.y_head_rot = me.y_rot;
    let speed = (mob.mover.speed_modifier * mob.attributes.value(attribute::MOVEMENT_SPEED)) as f32;
    if me.in_water {
        living::set_speed(mob, speed * params.in_water_speed);
        let horizontal = (xd * xd + zd * zd).sqrt();
        let epsilon = f64::from(1.0E-5_f32);
        if yd.abs() > epsilon || horizontal.abs() > epsilon {
            let x_rot_d = -degrees(mth::atan2(yd, horizontal));
            let x_rot_d = mth::clamp_f32(
                mth::wrap_degrees(x_rot_d),
                -params.max_turn_x,
                params.max_turn_x,
            );
            mob.x_rot = control::rotate_towards(mob.x_rot, x_rot_d, 5.0);
        }
        let radians = mob.x_rot * (std::f64::consts::PI / 180.0) as f32;
        let cos = mth::cos(f64::from(radians));
        let sin = mth::sin(f64::from(radians));
        mob.zza = cos * speed;
        mob.yya = -sin * speed;
    } else {
        let left_to_turn = mth::wrap_degrees(me.y_rot - y_rot_d).abs();
        let factor = 1.0 - mth::clamp_f32((left_to_turn - 10.0) / 50.0, 0.0, 1.0);
        living::set_speed(mob, speed * params.outside_water_speed * factor);
    }
}

/// `SmoothSwimmingLookControl(mob, maxYRotFromCenter).tick`：有看的目标时头朝目标再偏
/// 20 度、俯仰偏 10 度；没有时路走完了才把俯仰慢慢归零，头转回身体的方向。头偏出身体
/// `max_y_rot_from_center` 度以上时身体每刻跟 4 度。
///
/// `SmoothSwimmingLookControl(mob, maxYRotFromCenter).tick`: with a look target the head
/// turns to it plus 20 degrees and the pitch plus 10; without one the pitch eases back
/// to zero once the path is done and the head returns to the body. A head more than
/// `max_y_rot_from_center` degrees off pulls the body along by 4 degrees a tick.
pub(super) fn look_tick(me: &Entity, mob: &mut Mob, max_y_rot_from_center: f32) {
    if mob.look.look_at_cooldown > 0 {
        mob.look.look_at_cooldown -= 1;
        if let Some(target) = control::look_y_rot(&mob.look, me.position.x, me.position.z) {
            mob.y_head_rot =
                control::rotate_towards(mob.y_head_rot, target + 20.0, mob.look.y_max_rot_speed);
        }
        let eye_y = me.position.y + f64::from(mob.eye_height);
        if let Some(target) = control::look_x_rot(&mob.look, me.position.x, eye_y, me.position.z) {
            mob.x_rot = control::rotate_towards(mob.x_rot, target + 10.0, mob.look.x_max_rot_angle);
        }
    } else {
        if mob.navigation.is_done() {
            mob.x_rot = control::rotate_towards(mob.x_rot, 0.0, 5.0);
        }
        mob.y_head_rot =
            control::rotate_towards(mob.y_head_rot, mob.y_body_rot, mob.look.y_max_rot_speed);
    }
    let head_diff_body = mth::wrap_degrees(mob.y_head_rot - mob.y_body_rot);
    if head_diff_body < -max_y_rot_from_center {
        mob.y_body_rot -= 4.0;
    } else if head_diff_body > max_y_rot_from_center {
        mob.y_body_rot += 4.0;
    }
}

/// `(float)(radians * Mth.RAD_TO_DEG)`：反编译出来是 `* 180.0F / (float)Math.PI`，字节码里是
/// 折叠好的一个 `float` 常量。
///
/// `(float)(radians * Mth.RAD_TO_DEG)`: decompiled as `* 180.0F / (float)Math.PI`, but the
/// bytecode holds one folded `float` constant.
fn degrees(radians: f64) -> f32 {
    (radians * 57.295_776_367_187_5) as f32
}
