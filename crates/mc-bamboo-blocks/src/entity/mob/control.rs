//! 生物的四个控制器：`MoveControl`、`LookControl`、`JumpControl`、`BodyRotationControl`。
//!
//! The four mob controls.

use mc_bamboo_core::{BlockPos, mth};
use mc_bamboo_data::tags::block_tags;

/// `MoveControl.Operation`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Operation {
    #[default]
    Wait,
    MoveTo,
    Strafe,
    Jumping,
}

/// `MoveControl` 的状态。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MoveControl {
    pub wanted_x: f64,
    pub wanted_y: f64,
    pub wanted_z: f64,
    pub speed_modifier: f64,
    pub strafe_forwards: f32,
    pub strafe_right: f32,
    pub operation: Operation,
}

impl MoveControl {
    /// `setWantedPosition`。
    pub fn set_wanted_position(&mut self, x: f64, y: f64, z: f64, speed: f64) {
        self.wanted_x = x;
        self.wanted_y = y;
        self.wanted_z = z;
        self.speed_modifier = speed;
        if self.operation != Operation::Jumping {
            self.operation = Operation::MoveTo;
        }
    }

    pub fn has_wanted(&self) -> bool {
        self.operation == Operation::MoveTo
    }
}

/// `MoveControl.rotlerp`：结果落在 0 到 360 之间。
pub fn move_rotlerp(current: f32, target: f32, max: f32) -> f32 {
    let mut difference = mth::wrap_degrees(target - current);
    if difference > max {
        difference = max;
    }
    if difference < -max {
        difference = -max;
    }
    let mut result = current + difference;
    if result < 0.0 {
        result += 360.0;
    } else if result > 360.0 {
        result -= 360.0;
    }
    result
}

/// `MoveControl.tick` 跳起来那一支要看的格子事实。
pub fn blocks_jump(state: crate::state::BlockState) -> bool {
    !state.is_tagged(block_tags::DOORS) && !state.is_tagged(block_tags::FENCES)
}

/// `LookControl` 的状态。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LookControl {
    pub y_max_rot_speed: f32,
    pub x_max_rot_angle: f32,
    pub look_at_cooldown: i32,
    pub wanted_x: f64,
    pub wanted_y: f64,
    pub wanted_z: f64,
}

impl LookControl {
    /// `setLookAt(x, y, z, yMax, xMax)`。
    pub fn set_look_at(&mut self, x: f64, y: f64, z: f64, y_max: f32, x_max: f32) {
        self.wanted_x = x;
        self.wanted_y = y;
        self.wanted_z = z;
        self.y_max_rot_speed = y_max;
        self.x_max_rot_angle = x_max;
        self.look_at_cooldown = 2;
    }

    /// `setLookAt(x, y, z)`：转速取 `getHeadRotSpeed()` 和 `getMaxHeadXRot()`。
    pub fn set_look_at_default(&mut self, x: f64, y: f64, z: f64) {
        self.set_look_at(x, y, z, HEAD_ROT_SPEED as f32, MAX_HEAD_X_ROT as f32);
    }
}

/// `Mob.getMaxHeadXRot()`。
pub const MAX_HEAD_X_ROT: i32 = 40;
/// `Mob.getMaxHeadYRot()`。
pub const MAX_HEAD_Y_ROT: i32 = 75;
/// `Mob.getHeadRotSpeed()`。
pub const HEAD_ROT_SPEED: i32 = 10;

/// `Control.rotateTowards`：不折回一圈之内。
pub fn rotate_towards(current: f32, target: f32, max_step: f32) -> f32 {
    let difference = mth::degrees_difference(current, target);
    let clamped = mth::clamp_f32(difference, -max_step, max_step);
    current + clamped
}

/// `LookControl.getYRotD`。
pub fn look_y_rot(look: &LookControl, x: f64, z: f64) -> Option<f32> {
    let dx = look.wanted_x - x;
    let dz = look.wanted_z - z;
    if dz.abs() > f64::from(1.0e-5_f32) || dx.abs() > f64::from(1.0e-5_f32) {
        Some((mth::atan2(dz, dx) * 57.295_776_367_187_5) as f32 - 90.0)
    } else {
        None
    }
}

/// `LookControl.getXRotD`。
pub fn look_x_rot(look: &LookControl, x: f64, eye_y: f64, z: f64) -> Option<f32> {
    let dx = look.wanted_x - x;
    let dy = look.wanted_y - eye_y;
    let dz = look.wanted_z - z;
    let horizontal = (dx * dx + dz * dz).sqrt();
    if dy.abs() > f64::from(1.0e-5_f32) || horizontal.abs() > f64::from(1.0e-5_f32) {
        Some((-(mth::atan2(dy, horizontal) * 57.295_776_367_187_5)) as f32)
    } else {
        None
    }
}

/// `Entity.setXRot`：有限值才接受，夹到 ±90。
pub fn clamp_x_rot(value: f32, current: f32) -> f32 {
    if !value.is_finite() {
        return current;
    }
    (value % 360.0).clamp(-90.0, 90.0)
}

/// `BodyRotationControl` 的状态。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BodyRotationControl {
    pub head_stable_time: i32,
    pub last_stable_y_head_rot: f32,
}

/// `JumpControl` 的状态。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct JumpControl {
    pub jump: bool,
}

/// 方块坐标的中心偏移，`MoveControl.isWalkable` 用。
pub fn walkable_probe(x: f64, y: i32, z: f64, dx: f32, dz: f32) -> BlockPos {
    BlockPos::new(
        mth::floor(x + f64::from(dx)),
        y,
        mth::floor(z + f64::from(dz)),
    )
}
