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

use mc_bamboo_core::{BlockPos, Vec3};
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
    living,
};
use crate::entity::{
    Entity,
    ctx::{Command, EntityCtx},
    wind_charge::{WindChargeShot, WindChargeVariant},
};

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
    /// 长跳落地前剩下的刻数，`LongJump` 用它推进跳跃。
    pub jump_ticks: i32,
    pub jump_target: Option<BlockPos>,
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
    /// `BreezeAi$SlideToTargetSink`（IDLE 活动）。
    SlideToTargetSink,
}

impl Behavior {
    pub(super) fn class(&self) -> Class {
        match self {
            Self::StartAttacking { .. } | Self::StopAttackingIfTargetInvalid => Class::OneShot,
            Self::Shoot => Class::Timed { min: 20, max: 20 },
            Self::LongJump => Class::Timed { min: 200, max: 200 },
            Self::ShootWhenStuck => Class::OneShot,
            Self::Slide => Class::Timed { min: 20, max: 20 },
            Self::SlideToTargetSink => Class::Timed { min: 20, max: 40 },
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
            Self::SlideToTargetSink => "SlideToTargetSink",
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
            Self::SlideToTargetSink => &[
                (WalkTarget, ValuePresent),
                (BreezeShootCooldown, Registered),
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
            Self::LongJump => {
                if !cx.me.on_ground || cx.me.in_water {
                    return false;
                }
                within_inner_circle(cx, 4.0, 10.0) == Some(false) && cx.memories.has(AttackTarget)
            }
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
            Self::SlideToTargetSink => cx.memories.has(WalkTarget),
        }
    }

    pub(super) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::Shoot => cx.memories.has(AttackTarget) && cx.memories.has(BreezeShoot),
            Self::LongJump => cx.mob.breeze.jump_ticks > 0,
            Self::SlideToTargetSink => cx.memories.has(WalkTarget),
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
                cx.mob.breeze.pose = Pose::LongJumping;
                cx.mob.breeze.jump_ticks = 20;
                cx.memories
                    .set_with_ttl(BreezeJumpInhaling, MemoryValue::Unit, 5);
                living::play_sound(cx.me, cx.mob, "minecraft:entity.breeze.jump", 1.0, 1.0);
            }
            Self::ShootWhenStuck => {
                cx.memories.set_with_ttl(BreezeShoot, MemoryValue::Unit, 60);
            }
            Self::Slide => {
                cx.mob.breeze.pose = Pose::Sliding;
                if let Some(target) = cx.memories.entity(AttackTarget)
                    && let Some(view) = sensor::living(cx, target)
                {
                    let to = cx.me.position.subtract(view.position);
                    let distance = 4.0 + cx.me.random.next_double() * 4.0;
                    let at = cx.me.position.add(to.normalize().scale(distance));
                    cx.memories.set(
                        WalkTarget,
                        MemoryValue::Walk(WalkTarget::to_point(at, 0.6, 1)),
                    );
                }
            }
            Self::SlideToTargetSink => {}
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
                // `Shoot.tick`：朝目标转头；充能 / 恢复记忆还在时什么都不做。
                cx.mob.look.set_look_at(
                    view.position.x,
                    view.position.y,
                    view.position.z,
                    30.0,
                    30.0,
                );
                if cx.memories.has(BreezeShootCharging) || cx.memories.has(BreezeShootRecovering) {
                    return;
                }
                cx.memories
                    .set_with_ttl(BreezeShootRecovering, MemoryValue::Unit, 4);
                let dx = view.position.x - cx.me.position.x;
                let dy = view.position.y + f64::from(view.eye_height * 0.3_f32) - firing_y(cx);
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
                cx.mob.breeze.jump_ticks -= 1;
                if cx.mob.breeze.jump_ticks == 15 {
                    // 官方在吸气结束后才给速度；这里用竖直起跳近似长跳速度。
                    cx.me.delta = cx.me.delta.add(Vec3::new(0.0, 0.9, 0.0));
                    cx.mob.jump.jump = true;
                }
            }
            Self::SlideToTargetSink => {
                // `MoveToTargetSink` 的变体：速度按状态乘 0.6。
                if let Some(MemoryValue::Walk(walk)) = cx.memories.get(WalkTarget).cloned() {
                    let mut walk = walk;
                    walk.speed *= 0.6;
                    cx.memories.set(WalkTarget, MemoryValue::Walk(walk));
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
                cx.mob.breeze.pose = Pose::Standing;
                cx.memories
                    .set_with_ttl(BreezeJumpCooldown, MemoryValue::Unit, 10);
                cx.memories.erase(BreezeJumpTarget);
                cx.memories.erase(BreezeJumpInhaling);
                cx.mob.breeze.jump_ticks = 0;
            }
            Self::Slide => {
                cx.mob.breeze.pose = Pose::Standing;
                cx.memories.erase(WalkTarget);
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
            Kind::Breeze(Behavior::SlideToTargetSink),
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
        assert_eq!(MobType::ALL.len(), 64);
        assert!(MobType::by_name("minecraft:breeze") == Some(MobType::Breeze));
        assert_eq!(MobType::Breeze.name(), "minecraft:breeze");
    }
}
