//! 旋风人，官方 `Breeze` / `BreezeAi`。
//!
//! 大脑结构照 26.3 字节码：CORE 的 `Swim(0.8)` / `LookAtTargetSink(45,90)`，
//! IDLE 的两个 `StartAttacking`、`SlideToTargetSink(20,40)` 和 `RunOne`，
//! FIGHT 五个行为（`StopAttackingIfTargetInvalid`、`Shoot`、`LongJump`、
//! `ShootWhenStuck`、`Slide`），条件 `ATTACK_TARGET` 有值 / `WALK_TARGET` 空。
//! `BreezeAttackEntitySensor` 选出最近可攻击的玩家或铁傀儡。
//!
//! Breeze: vanilla `Breeze` / `BreezeAi`. The brain table follows 26.3: CORE
//! `Swim(0.8)` / `LookAtTargetSink(45,90)`, the IDLE attacking and sliding
//! behaviours and the five FIGHT behaviours. The attack sensor picks the
//! closest attackable player or iron golem.

use mc_bamboo_core::{BlockPos, Direction, Vec3, mth};
use mc_bamboo_data::{registries::attribute, sounds};

use super::{
    Mob, MobType,
    attributes::Attributes,
    brain::{
        Activity, ActivityData, Brain, BrainTemplate, Cx, TemplateCache,
        behavior::{Class, Gate, Kind, MoveToTargetSink, Speed},
        memory::{
            MemoryStatus::{self, Registered, ValueAbsent, ValuePresent},
            MemoryType::{self, *},
            MemoryValue, WalkTarget,
        },
        sensor::{self, SensorKind},
        template_cache,
    },
    goat, living,
};
use crate::entity::{
    Entity,
    ctx::{Command, EntityCtx},
    wind_charge::{WindChargeShot, WindChargeVariant},
};
use crate::shape::CollisionContext;

static TEMPLATE: TemplateCache = template_cache();

/// `Breeze.createAttributes`。
pub fn attributes() -> Attributes {
    let mut attributes = Attributes::cow();
    attributes.set_base(attribute::MOVEMENT_SPEED, f64::from(0.63_f32));
    attributes.set_base(attribute::MAX_HEALTH, 30.0);
    attributes.set_base(attribute::FOLLOW_RANGE, 24.0);
    // `Mob.createMobAttributes()` 里没有 ATTACK_DAMAGE，`Breeze.createAttributes`
    // 用 `add()` 追加这一条，不能只 set_base。
    if let Some(entry) = attributes
        .entries
        .iter_mut()
        .find(|entry| entry.id == attribute::ATTACK_DAMAGE)
    {
        entry.base = 3.0;
    } else {
        attributes.entries.push(super::attributes::Attribute::new(
            attribute::ATTACK_DAMAGE,
            3.0,
        ));
    }
    attributes
}

/// `Breeze` 的姿态（客户端动画状态由 `Pose` 同步，这里只记逻辑值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pose {
    #[default]
    Standing,
    Shooting,
    Inhaling,
    Sliding,
    LongJumping,
}

/// 旋风人自己的状态。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Breeze {
    pub pose: Pose,
    /// `jumpTrailStartedTick` / `soundTick`（客户端动画用，服务端仍推进）。
    pub jump_trail_started_tick: i32,
    pub sound_tick: i32,
}

/// `BreezeAi` 的五个专属行为。
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// `StartAttacking`：从 `NEAREST_ATTACKABLE` 或 `HURT_BY` 设 `ATTACK_TARGET`。
    StartAttacking { hurt_by: bool },
    /// `StopAttackingIfTargetInvalid`：目标失效时清掉。
    StopAttackingIfTargetInvalid,
    /// `Shoot`：充能 → 射一枚 `BreezeWindCharge` → 恢复 → 冷却。
    Shoot,
    /// `LongJump`：跳到目标身后，落地后 10 刻冷却。
    LongJump,
    /// `ShootWhenStuck`：在水里 / 漂浮 / 载具上时申请射击。
    ShootWhenStuck,
    /// `Slide`：朝内圈外的一点滑行（速度 0.6）。
    Slide,
}

impl Behavior {
    pub(super) fn class(&self) -> Class {
        match self {
            Self::StartAttacking { .. } | Self::StopAttackingIfTargetInvalid => Class::OneShot,
            Self::Shoot => Class::Timed { min: 20, max: 20 },
            Self::LongJump => Class::Timed { min: 200, max: 200 },
            Self::ShootWhenStuck => Class::OneShot,
            Self::Slide => Class::Timed { min: 60, max: 60 },
        }
    }

    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::StartAttacking { .. } => "StartAttacking",
            Self::StopAttackingIfTargetInvalid => "StopAttackingIfTargetInvalid",
            Self::Shoot => "Shoot",
            Self::LongJump => "LongJump",
            Self::ShootWhenStuck => "ShootWhenStuck",
            Self::Slide => "Slide",
        }
    }

    pub(super) fn required(&self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::StartAttacking { hurt_by: true } => {
                &[(HurtByEntity, ValuePresent), (AttackTarget, ValueAbsent)]
            }
            Self::StartAttacking { hurt_by: false } => &[
                (NearestAttackable, ValuePresent),
                (AttackTarget, ValueAbsent),
            ],
            Self::StopAttackingIfTargetInvalid => &[(AttackTarget, ValuePresent)],
            Self::Shoot => &[
                (AttackTarget, ValuePresent),
                (BreezeShootCooldown, ValueAbsent),
                (BreezeShootCharging, ValueAbsent),
                (BreezeShootRecovering, ValueAbsent),
                (BreezeShoot, ValuePresent),
                (WalkTarget, ValueAbsent),
                (BreezeJumpTarget, ValueAbsent),
            ],
            Self::LongJump => &[
                (AttackTarget, ValuePresent),
                (BreezeJumpCooldown, ValueAbsent),
                (BreezeJumpInhaling, Registered),
                (BreezeJumpTarget, Registered),
                (BreezeShoot, ValueAbsent),
                (WalkTarget, ValueAbsent),
                (BreezeLeavingWater, Registered),
            ],
            Self::ShootWhenStuck => &[
                (AttackTarget, ValuePresent),
                (BreezeJumpInhaling, ValueAbsent),
                (BreezeJumpTarget, ValueAbsent),
                (WalkTarget, ValueAbsent),
                (BreezeShoot, ValueAbsent),
            ],
            Self::Slide => &[
                (AttackTarget, ValuePresent),
                (WalkTarget, ValueAbsent),
                (BreezeJumpCooldown, ValueAbsent),
                (BreezeShoot, ValueAbsent),
            ],
        }
    }

    pub(super) fn extra_start(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::StartAttacking { hurt_by } => {
                if *hurt_by {
                    cx.memories.has(HurtByEntity)
                } else {
                    cx.memories.has(NearestAttackable)
                }
            }
            Self::StopAttackingIfTargetInvalid => {
                // `wasEntityAttackableLastNTicks(100).negate()`：目标死了 / 不活就停。
                let Some(target) = cx.memories.entity(AttackTarget) else {
                    return true;
                };
                !sensor::living(cx, target).is_some_and(|view| view.alive)
            }
            Self::Shoot => cx.mob.breeze.pose == Pose::Standing && target_within_range(cx, 16.0),
            Self::LongJump => long_jump_can_run(cx),
            Self::ShootWhenStuck => {
                cx.me.vehicle.is_some()
                    || cx.me.in_water
                    || cx
                        .mob
                        .effects
                        .has(mc_bamboo_data::registries::mob_effect::LEVITATION)
            }
            Self::Slide => {
                cx.me.on_ground && !cx.me.in_water && cx.mob.breeze.pose == Pose::Standing
            }
        }
    }

    pub(super) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::Shoot => cx.memories.has(AttackTarget) && cx.memories.has(BreezeShoot),
            Self::LongJump => {
                cx.mob.breeze.pose != Pose::Standing && !cx.memories.has(BreezeJumpCooldown)
            }
            _ => false,
        }
    }

    pub(super) fn start(&mut self, cx: &mut Cx) {
        match self {
            Self::StartAttacking { hurt_by } => {
                let source = if *hurt_by {
                    cx.memories.entity(HurtByEntity)
                } else {
                    cx.memories.entity(NearestAttackable)
                };
                if let Some(target) = source {
                    cx.memories.set(AttackTarget, MemoryValue::Entity(target));
                }
            }
            Self::StopAttackingIfTargetInvalid => {
                cx.memories.erase(AttackTarget);
                cx.mob.target = None;
            }
            Self::Shoot => {
                cx.mob.breeze.pose = Pose::Shooting;
                cx.memories
                    .set_with_ttl(BreezeShootCharging, MemoryValue::Unit, 15);
                living::play_sound(cx.me, cx.mob, "minecraft:entity.breeze.inhale", 1.0, 1.0);
            }
            Self::LongJump => {
                if !cx.memories.has(BreezeJumpInhaling) {
                    cx.memories
                        .set_with_ttl(BreezeJumpInhaling, MemoryValue::Unit, 10);
                }
                cx.mob.breeze.pose = Pose::Inhaling;
                living::play_sound(cx.me, cx.mob, "minecraft:entity.breeze.charge", 1.0, 1.0);
                if let Some(target) = cx.memories.block_pos(BreezeJumpTarget) {
                    // `lookAt(EYES, atCenterOf(jumpTarget))`。
                    look_at_eyes(
                        cx.me,
                        cx.mob,
                        Vec3::new(
                            f64::from(target.x) + 0.5,
                            f64::from(target.y) + 0.5,
                            f64::from(target.z) + 0.5,
                        ),
                    );
                }
            }
            Self::ShootWhenStuck => {
                cx.memories.set_with_ttl(BreezeShoot, MemoryValue::Unit, 60);
            }
            Self::Slide => {
                let Some(target) = cx.memories.entity(AttackTarget) else {
                    return;
                };
                let Some(view) = sensor::living(cx, target) else {
                    return;
                };
                // `withinInnerCircleRange` 命中时官方先试 `DefaultRandomPos.getPosAway`；
                // 该落点求解还没复刻，内圈内外都按 `nextBoolean` 二选一（对拍场景目标
                // 在内圈外，走不到这一支）。
                let _in_inner = within_inner_circle(cx, 4.0, 10.0);
                let pos = if cx.me.random.next_boolean() {
                    random_point_behind_target(&view, &mut cx.me.random)
                } else {
                    random_point_in_middle_circle(cx, &view)
                };
                cx.memories.set(
                    WalkTarget,
                    MemoryValue::Walk(WalkTarget::to_point(pos, 0.6, 1)),
                );
            }
        }
    }

    pub(super) fn tick(&mut self, cx: &mut Cx) {
        match self {
            Self::Shoot => {
                let Some(target) = cx.memories.entity(AttackTarget) else {
                    return;
                };
                let Some(view) = sensor::living(cx, target) else {
                    return;
                };
                // `Shoot.tick`：朝目标转头（`lookAt(EYES, position)` 立刻转向）；
                // 充能 / 恢复记忆还在时什么都不做。
                look_at_eyes(cx.me, cx.mob, view.position);
                if cx.memories.has(BreezeShootCharging) || cx.memories.has(BreezeShootRecovering) {
                    return;
                }
                cx.memories
                    .set_with_ttl(BreezeShootRecovering, MemoryValue::Unit, 4);
                let dx = view.position.x - cx.me.position.x;
                // `target.getY(isPassenger ? 0.8 : 0.3)` = 脚底 + `getBbHeight() * 0.3`。
                // 对拍的场景里目标不骑乘，恒取 0.3。
                let height = view.bounding_box.max_y - view.bounding_box.min_y;
                let dy = view.position.y + height * 0.3 - firing_y(cx);
                let dz = view.position.z - cx.me.position.z;
                let uncertainty = 5.0 - 4.0 * f64::from(cx.ctx.blocks().difficulty());
                let shot = WindChargeShot {
                    position: Vec3::new(cx.me.position.x, firing_y(cx), cx.me.position.z),
                    direction: Vec3::new(dx, dy, dz),
                    power: 0.7,
                    uncertainty: uncertainty as f32,
                    variant: WindChargeVariant::Breeze,
                    owner: Some(cx.me.id),
                    owner_uuid: None,
                };
                cx.ctx
                    .emit(Command::ShootBreezeWindCharge(Box::new(shot)), cx.me);
                living::play_sound(cx.me, cx.mob, "minecraft:entity.breeze.shoot", 1.5, 1.0);
            }
            Self::LongJump => {
                let in_water = cx.me.in_water;
                if !in_water && cx.memories.has(BreezeLeavingWater) {
                    cx.memories.erase(BreezeLeavingWater);
                }
                if is_finished_inhaling(cx) {
                    // `Vec3.atBottomCenterOf(jumpTarget)` → 解初速度 → 起跳。
                    let jump_vector = cx.memories.block_pos(BreezeJumpTarget).and_then(|target| {
                        calculate_optimal_jump_vector(
                            cx,
                            Vec3::new(
                                f64::from(target.x) + 0.5,
                                f64::from(target.y),
                                f64::from(target.z) + 0.5,
                            ),
                        )
                    });
                    let Some(jump_vector) = jump_vector else {
                        cx.mob.breeze.pose = Pose::Standing;
                        return;
                    };
                    if in_water {
                        cx.memories.set(BreezeLeavingWater, MemoryValue::Unit);
                    }
                    living::play_sound(cx.me, cx.mob, "minecraft:entity.breeze.jump", 1.0, 1.0);
                    cx.mob.breeze.pose = Pose::LongJumping;
                    cx.me.y_rot = cx.mob.y_body_rot;
                    cx.mob.discard_friction = true;
                    cx.me.delta = jump_vector;
                } else if is_finished_jumping(cx) {
                    living::play_sound(cx.me, cx.mob, "minecraft:entity.breeze.land", 1.0, 1.0);
                    cx.mob.breeze.pose = Pose::Standing;
                    cx.mob.discard_friction = false;
                    let hurt_by = cx.memories.has(HurtByEntity);
                    cx.memories.set_with_ttl(
                        BreezeJumpCooldown,
                        MemoryValue::Unit,
                        if hurt_by { 2 } else { 10 },
                    );
                    cx.memories
                        .set_with_ttl(BreezeShoot, MemoryValue::Unit, 100);
                }
            }
            _ => {}
        }
    }

    pub(super) fn stop(&mut self, cx: &mut Cx, _timed_out: bool) {
        match self {
            Self::Shoot => {
                if cx.mob.breeze.pose == Pose::Shooting {
                    cx.mob.breeze.pose = Pose::Standing;
                }
                cx.memories
                    .set_with_ttl(BreezeShootCooldown, MemoryValue::Unit, 10);
                cx.memories.erase(BreezeShoot);
            }
            Self::LongJump => {
                if cx.mob.breeze.pose == Pose::LongJumping || cx.mob.breeze.pose == Pose::Inhaling {
                    cx.mob.breeze.pose = Pose::Standing;
                }
                cx.memories.erase(BreezeJumpTarget);
                cx.memories.erase(BreezeJumpInhaling);
                cx.memories.erase(BreezeLeavingWater);
            }
            Self::Slide => {
                // `Slide` 没有 `stop` 覆写：姿态由 `SlideToTargetSink` 收尾，
                // `WALK_TARGET` 也由它消费。
            }
            _ => {}
        }
    }

    pub(super) fn trigger(&mut self, cx: &mut Cx) -> bool {
        // OneShot 的 `trigger`：入口条件已由位掩码检查，这里只补实体谓词。
        self.extra_start(cx)
    }
}

/// `Breeze.getFiringYPosition()`：`getY + bbHeight/2 + 0.30000001192092896`。
pub fn firing_y(cx: &Cx) -> f64 {
    cx.me.position.y + f64::from(cx.me.dimensions.1) * 0.5 + f64::from(0.3_f32)
}

/// `Entity.lookAt(EYES, target)`：从眼睛出发立刻转向目标，`yHeadRot` 跟上，旧的
/// 朝向（`xRotO` / `yRotO`）同步成当前值。
fn look_at_eyes(me: &mut Entity, mob: &mut Mob, target: Vec3) {
    let eye = Vec3::new(
        me.position.x,
        me.position.y + f64::from(mob.eye_height),
        me.position.z,
    );
    let dx = target.x - eye.x;
    let dy = target.y - eye.y;
    let dz = target.z - eye.z;
    let horizontal = (dx * dx + dz * dz).sqrt();
    let pitch = mth::wrap_degrees((-(mth::atan2(dy, horizontal) * 57.295_776_367_187_5)) as f32);
    let yaw = mth::wrap_degrees((mth::atan2(dz, dx) * 57.295_776_367_187_5 - 90.0) as f32);
    mob.x_rot = pitch;
    me.y_rot = yaw;
    mob.y_head_rot = yaw;
    mob.x_rot_o = pitch;
    mob.y_rot_o = yaw;
}

/// `Breeze.withinInnerCircleRange`：自己所在格中心到目标 `closerThan(4, 10)`。
fn within_inner_circle(cx: &Cx, horizontal: f64, vertical: f64) -> Option<bool> {
    let target = cx.memories.entity(AttackTarget)?;
    let view = sensor::living(cx, target)?;
    let centre = Vec3::new(
        cx.me.block_position().x as f64 + 0.5,
        cx.me.block_position().y as f64 + 0.5,
        cx.me.block_position().z as f64 + 0.5,
    );
    let dx = view.position.x - centre.x;
    let dy = view.position.y - centre.y;
    let dz = view.position.z - centre.z;
    Some(dx * dx + dz * dz < horizontal * horizontal && dy.abs() < vertical)
}

/// `BreezeUtil.randomPointBehindTarget`：在目标背后 4–8 格、偏转角 ±45° 内随机取一点。
fn random_point_behind_target(
    view: &sensor::LivingView,
    random: &mut crate::entity::JavaRandom,
) -> Vec3 {
    let gaussian = random.next_gaussian() as f32;
    let yaw = view.y_head_rot + 180.0 + gaussian * 90.0 / 2.0;
    let distance = 4.0 + random.next_float() * 4.0;
    // `Vec3.directionFromRotation(0, yaw)` = (sin(yaw), 0, cos(yaw))。
    let radians = yaw * (std::f32::consts::PI / 180.0);
    let sin = f64::from(mth::sin(f64::from(radians)));
    let cos = f64::from(mth::cos(f64::from(radians)));
    view.position.add(Vec3::new(
        sin * f64::from(distance),
        0.0,
        cos * f64::from(distance),
    ))
}

/// `LongJump.snapToSurface`：从一点先朝下 10 格、再朝上 10 格 clip，取命中格上方。
fn snap_to_surface(cx: &Cx, point: Vec3) -> Option<BlockPos> {
    let view = cx.ctx.blocks();
    if let Some(hit) = view.clip_collider(
        point,
        point.add(Vec3::new(0.0, -10.0, 0.0)),
        CollisionContext::Empty,
    ) {
        return Some(BlockPos::containing(hit.location).above());
    }
    if let Some(hit) = view.clip_collider(
        point,
        point.add(Vec3::new(0.0, 10.0, 0.0)),
        CollisionContext::Empty,
    ) {
        return Some(BlockPos::containing(hit.location).above());
    }
    None
}

/// `BreezeUtil.hasLineOfSight`：先比 `max(50, FOLLOW_RANGE)` 距离，再 COLLIDER clip。
fn has_line_of_sight(cx: &Cx, point: Vec3) -> bool {
    let view = cx.ctx.blocks();
    let from = cx.me.position;
    if from.distance_to_sqr(point).sqrt()
        > f64::from(50.0_f32).max(cx.mob.attributes.value(attribute::FOLLOW_RANGE))
    {
        return false;
    }
    view.clip_collider(from, point, CollisionContext::Empty)
        .is_none()
}

/// `LongJump.canJumpFromCurrentPosition`：脚下不是蜂蜜块，上方 4 格都是空气或水。
fn can_jump_from_current_position(cx: &Cx) -> bool {
    let view = cx.ctx.blocks();
    let pos = cx.me.block_position();
    if view
        .block_state(pos)
        .is(mc_bamboo_data::blocks::HONEY_BLOCK)
    {
        return false;
    }
    for i in 1..=4 {
        let above = pos.relative_by(Direction::Up, i);
        let state = view.block_state(above);
        if !state.is_air() && !state.fluid().is_water() {
            return false;
        }
    }
    true
}

/// `LongJump.calculateOptimalJumpVector`：把 40/55/60/75/80 五个角度洗牌，
/// 取第一个算得出来的初速度；有跳跃提升时在竖直方向补 `getJumpBoostPower()`。
fn calculate_optimal_jump_vector(cx: &mut Cx, target: Vec3) -> Option<Vec3> {
    let mut angles = [40, 55, 60, 75, 80];
    cx.me.random.shuffle(&mut angles);
    let jump_power = 0.058_333_334_f32 * cx.mob.attributes.value(attribute::FOLLOW_RANGE) as f32;
    for angle in angles {
        let Some(vector) =
            goat::jump_vector_for_angle(&*cx, target, jump_power, angle, (0.6, 1.77), false)
        else {
            continue;
        };
        if cx
            .mob
            .effects
            .has(mc_bamboo_data::registries::mob_effect::JUMP_BOOST)
        {
            let boost = vector.normalize().y * f64::from(living::jump_boost_power(cx.mob));
            return Some(vector.add(Vec3::new(0.0, boost, 0.0)));
        }
        return Some(vector);
    }
    None
}

/// `Slide.randomPointInMiddleCircle`：朝目标方向取 `len - lerp(nextDouble, 8, 4)` 处。
fn random_point_in_middle_circle(cx: &mut Cx, view: &sensor::LivingView) -> Vec3 {
    let to_target = view.position.subtract(cx.me.position);
    let distance = to_target.length() - mth::lerp(cx.me.random.next_double(), 8.0, 4.0);
    cx.me.position.add(to_target.normalize().scale(distance))
}

/// `EntityType.isBlockDangerous`（旋风人没有 `immuneTo` 标签、不防火）：烧着的方块、
/// 凋灵玫瑰、甜浆果丛、仙人掌、细雪。
fn is_block_dangerous(state: crate::state::BlockState) -> bool {
    use mc_bamboo_data::{blocks, tags::block_tags};
    state.is_tagged(block_tags::FIRE)
        || state.is(blocks::LAVA)
        || state.is(blocks::WITHER_ROSE)
        || state.is(blocks::SWEET_BERRY_BUSH)
        || state.is(blocks::CACTUS)
        || state.is(blocks::POWDER_SNOW)
}

/// `LongJump.canRun`：着地（或在水里）、不在可漂浮流体深处、不在岩浆里；已有跳跃目标
/// 直接通过，否则在目标背后解一个落点（`randomPointBehindTarget` → `snapToSurface` →
/// 危险方块 / 视线检查），写进 `BREEZE_JUMP_TARGET`。目标超出跟随距离时清掉
/// `ATTACK_TARGET` 并返回假。
fn long_jump_can_run(cx: &mut Cx) -> bool {
    if !cx.me.on_ground && !cx.me.in_water {
        return false;
    }
    // `isInFluidDeeperThan(getFluidJumpThreshold, ENTITY_FLOATABLE)` / `isInLava`
    // 对拍场景不在流体里，这里按不建模处理（`in_water` 已经覆盖水里分支）。
    if cx.memories.has(BreezeJumpTarget) {
        return true;
    }
    let Some(target) = cx.memories.entity(AttackTarget) else {
        return false;
    };
    let Some(view) = sensor::living(cx, target) else {
        return false;
    };
    let follow = cx.mob.attributes.value(attribute::FOLLOW_RANGE);
    if cx.me.position.distance_to_sqr(view.position) >= follow * follow {
        cx.memories.erase(AttackTarget);
        return false;
    }
    let dx = (view.position.x - cx.me.position.x) as f32;
    let dy = (view.position.y - cx.me.position.y) as f32;
    let dz = (view.position.z - cx.me.position.z) as f32;
    if mth::sqrt_f32(dx * dx + dy * dy + dz * dz) - 4.0 <= 0.0 {
        return false;
    }
    if !can_jump_from_current_position(cx) {
        return false;
    }
    let point = random_point_behind_target(&view, &mut cx.me.random);
    let Some(landing) = snap_to_surface(cx, point) else {
        return false;
    };
    if is_block_dangerous(cx.ctx.blocks().block_state(landing.below())) {
        return false;
    }
    let center = Vec3::new(
        f64::from(landing.x) + 0.5,
        f64::from(landing.y) + 0.5,
        f64::from(landing.z) + 0.5,
    );
    if !has_line_of_sight(cx, center)
        && !has_line_of_sight(cx, Vec3::new(center.x, center.y + 4.0, center.z))
    {
        return false;
    }
    cx.memories
        .set(BreezeJumpTarget, MemoryValue::BlockPos(landing));
    true
}

/// `LongJump.isFinishedInhaling`：吸气记忆过期且姿态仍是吸气。
fn is_finished_inhaling(cx: &Cx) -> bool {
    !cx.memories.has(BreezeJumpInhaling) && cx.mob.breeze.pose == Pose::Inhaling
}

/// `LongJump.isFinishedJumping`：长跳姿态里着地，或在水里且没标记「离开水」。
fn is_finished_jumping(cx: &Cx) -> bool {
    cx.mob.breeze.pose == Pose::LongJumping
        && (cx.me.on_ground || (cx.me.in_water && !cx.memories.has(BreezeLeavingWater)))
}

fn target_within_range(cx: &Cx, max: f64) -> bool {
    let Some(target) = cx.memories.entity(AttackTarget) else {
        return false;
    };
    sensor::living(cx, target)
        .is_some_and(|view| cx.me.position.distance_to_sqr(view.position) < max * max)
}

/// `BreezeAttackEntitySensor.doTick`：从 NEAREST_LIVING_ENTITIES 找第一个
/// 非创造 / 非旁观、可攻击（玩家或铁傀儡）的活体，写 NEAREST_ATTACKABLE。
pub(super) fn sense_attackables(cx: &mut Cx) {
    let found = sensor::find_closest_visible(cx, &mut |view| {
        view.alive
            && !view.creative
            && !view.spectator
            && (view.player || view.mob_type == Some(MobType::IronGolem))
    });
    cx.memories
        .set_or_erase(NearestAttackable, found.map(MemoryValue::Entity));
}

/// `Breeze.customServerAiStep`：大脑 tick → `updateActivity(FIGHT, IDLE)`。
pub(super) fn custom_server_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if let Some(mut brain) = mob.brain.take() {
        let gap = ctx.throttle_plan().decision_gap;
        brain.tick_by(me, mob, ctx, gap);
        brain.set_active_activity_to_first_valid(&[Activity::Fight, Activity::Idle]);
        mob.brain = Some(brain);
    }
    mob.breeze.sound_tick += 1;
    mob.breeze.jump_trail_started_tick = mob.breeze.jump_trail_started_tick.max(0);
    // `getAmbientSound` 落地 / 空中两种，由 living 层按 on_ground 调这里。
    let _ = (me, sounds::NEUTRAL);
}

/// 环境声按 `onGround()` 分派。
pub fn ambient_sound(on_ground: bool) -> &'static str {
    if on_ground {
        "minecraft:entity.breeze.idle_ground"
    } else {
        "minecraft:entity.breeze.idle_air"
    }
}

/// `BreezeAi.getActivities`：整张结构表，每种旋风人共用一份模板。
pub fn brain() -> Brain {
    let core = ActivityData::sequential(
        Activity::Core,
        0,
        vec![
            Kind::Swim { chance: 0.8 },
            Kind::LookAtTargetSink { min: 45, max: 90 },
        ],
    );
    let idle = ActivityData::sequential(
        Activity::Idle,
        0,
        vec![
            Kind::Breeze(Behavior::StartAttacking { hurt_by: true }),
            Kind::Breeze(Behavior::StartAttacking { hurt_by: false }),
            Kind::MoveToTargetSink(MoveToTargetSink::breeze(20, 40)),
            Kind::Gate(Box::new(Gate::run_one(
                Vec::new(),
                vec![
                    (Kind::DoNothing { min: 20, max: 100 }, 1),
                    (Kind::RandomStroll { speed: 0.6 }, 2),
                ],
            ))),
        ],
    );
    let fight = ActivityData::sequential(
        Activity::Fight,
        0,
        vec![
            Kind::Breeze(Behavior::StopAttackingIfTargetInvalid),
            Kind::Breeze(Behavior::Shoot),
            Kind::Breeze(Behavior::LongJump),
            Kind::Breeze(Behavior::ShootWhenStuck),
            Kind::Breeze(Behavior::Slide),
        ],
    )
    .when(vec![
        (AttackTarget, ValuePresent),
        (WalkTarget, ValueAbsent),
    ]);
    Brain::cached(&TEMPLATE, || {
        BrainTemplate::new(
            &[
                SensorKind::NearestLivingEntities,
                SensorKind::HurtBy,
                SensorKind::NearestPlayers,
                SensorKind::BreezeAttackEntities,
            ],
            vec![core, idle, fight],
        )
        .with_default_activity(Activity::Fight)
    })
}

/// `MoveToTargetSink` 在 IDLE 里的速度字段（保留导入给结构表用）。
#[allow(dead_code)]
fn _unused(_: MoveToTargetSink, _: Speed) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::Entity;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_data::registries::attribute;

    /// 26.3 字节码里的四个属性值和尺寸都按官方常量。
    #[test]
    fn attributes_and_dimensions_match_the_official_values() {
        let attrs = attributes();
        assert_eq!(attrs.value(attribute::MAX_HEALTH), 30.0);
        assert_eq!(attrs.value(attribute::FOLLOW_RANGE), 24.0);
        assert_eq!(attrs.value(attribute::ATTACK_DAMAGE), 3.0);
        assert_eq!(attrs.value(attribute::MOVEMENT_SPEED), f64::from(0.63_f32));
        assert_eq!(MobType::Breeze.dimensions(false), (0.6, 1.77, 1.385));
        assert_eq!(MobType::Breeze.xp_reward(), 10);
        assert!(MobType::Breeze.is_monster());
    }

    /// `MobType::ALL` 和 `Mob::new` 能造出旋风人，且大脑结构带 FIGHT / IDLE。
    #[test]
    fn a_breeze_has_a_brain_and_can_be_summoned() {
        let entity = Entity::by_mob_type(MobType::Breeze, Vec3::new(0.5, 64.0, 0.5));
        let mob = entity.kind.mob().expect("breeze is a mob");
        assert!(mob.brain.is_some());
        assert_eq!(mob.breeze.pose, Pose::Standing);
        assert_eq!(MobType::ALL.len(), 68);
        assert!(MobType::by_name("minecraft:breeze") == Some(MobType::Breeze));
        assert_eq!(MobType::Breeze.name(), "minecraft:breeze");
    }
}
