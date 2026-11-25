//! `WardenAi`：监守者的大脑结构表与全部专属行为。
//!
//! 活动表照 `WardenAi.getActivities` 的 8 项，`updateActivity` 照
//! `setActiveActivityToFirstValid([EMERGE,DIG,ROAR,FIGHT,INVESTIGATE,SNIFF,IDLE])`。
//! 每个行为的时长、记忆条件、姿态 / 愤怒 / 冷却都按 26.3 字节码。

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::registries::particle_type;

use crate::{
    entity::{
        Entity, EntityUid,
        ctx::{Command, EntityCtx},
    },
    game_event::{GameEvent, vibration::VibrationUser},
};

use super::{
    DamageKind, DamageSource, Mob, MobEvent, MobType,
    brain::{
        Activity, ActivityData, Brain,
        behavior::{Class, Cx, Gate, Kind, MoveToTargetSink},
        memory::{MemoryStatus, MemoryType, MemoryValue, PositionTracker, WalkTarget},
        sensor::SensorKind,
    },
    warden::WardenPose,
};

/// `EMERGE_DURATION = ceil(133.6)`。
pub const EMERGE_DURATION: i32 = 134;
/// `DIGGING_DURATION = ceil(100.0)`。
pub const DIGGING_DURATION: i32 = 100;
/// `ROAR_DURATION = ceil(84.0)`。
pub const ROAR_DURATION: i32 = 84;
/// `SNIFFING_DURATION = ceil(83.2)`。
pub const SNIFFING_DURATION: i32 = 84;
/// `SonicBoom.DURATION = ceil(60.0)`。
pub const SONIC_BOOM_DURATION: i32 = 60;
/// `SonicBoom.TICKS_BEFORE_PLAYING_SOUND = ceil(34.0)`。
pub const SONIC_BOOM_SOUND_DELAY: i32 = 34;
/// `Roar.TICKS_BEFORE_PLAYING_ROAR_SOUND`。
pub const ROAR_SOUND_DELAY: i32 = 25;
/// `DIGGING_COOLDOWN`。
pub const DIGGING_COOLDOWN: i32 = 1200;
/// `DISTURBANCE_LOCATION_EXPIRY_TIME`。
pub const DISTURBANCE_EXPIRY: i64 = 100;
/// `ANGERMANAGEMENT_TICK_DELAY`。
pub const ANGER_TICK_DELAY: i32 = 20;

/// 监守者专属行为（`WardenAi` 里各 `BehaviorControl`）。
///
/// The warden-specific behaviours (`BehaviorControl`s in `WardenAi`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behavior {
    Emerging,
    Digging,
    ForceUnmount,
    Roar,
    Sniffing,
    SonicBoom,
    TryToSniff,
    SetRoarTarget,
    SetWardenLookTarget,
    GoToTargetLocation,
    DigCooldownSetter,
    StopAttackingIfTargetInvalid,
    SetEntityLookTarget,
    SetWalkTargetFromAttackTargetIfTargetOutOfReach,
    MeleeAttack,
}

use MemoryStatus::{Registered, ValueAbsent, ValuePresent};
use MemoryType as M;

impl Behavior {
    pub(super) fn class(&self) -> Class {
        match self {
            Self::Emerging => Class::Timed {
                min: EMERGE_DURATION,
                max: EMERGE_DURATION,
            },
            Self::Digging => Class::Timed {
                min: DIGGING_DURATION,
                max: DIGGING_DURATION,
            },
            Self::Roar => Class::Timed {
                min: ROAR_DURATION,
                max: ROAR_DURATION,
            },
            Self::Sniffing => Class::Timed {
                min: SNIFFING_DURATION,
                max: SNIFFING_DURATION,
            },
            Self::SonicBoom => Class::Timed {
                min: SONIC_BOOM_DURATION,
                max: SONIC_BOOM_DURATION,
            },
            _ => Class::OneShot,
        }
    }

    pub(super) fn required(&self) -> &'static [(MemoryType, MemoryStatus)] {
        const EMERGING: &[(M, MemoryStatus)] = &[
            (M::IsEmerging, ValuePresent),
            (M::WalkTarget, ValueAbsent),
            (M::LookTarget, Registered),
        ];
        const DIGGING: &[(M, MemoryStatus)] =
            &[(M::AttackTarget, ValueAbsent), (M::WalkTarget, ValueAbsent)];
        const ROAR: &[(M, MemoryStatus)] = &[
            (M::RoarTarget, ValuePresent),
            (M::AttackTarget, ValueAbsent),
            (M::RoarSoundCooldown, Registered),
            (M::RoarSoundDelay, Registered),
        ];
        const SNIFFING: &[(M, MemoryStatus)] = &[
            (M::IsSniffing, ValuePresent),
            (M::AttackTarget, ValueAbsent),
            (M::WalkTarget, ValueAbsent),
            (M::LookTarget, Registered),
            (M::NearestAttackable, Registered),
            (M::DisturbanceLocation, Registered),
            (M::SniffCooldown, Registered),
        ];
        const SONIC_BOOM: &[(M, MemoryStatus)] = &[
            (M::AttackTarget, ValuePresent),
            (M::SonicBoomCooldown, ValueAbsent),
            (M::SonicBoomSoundCooldown, Registered),
            (M::SonicBoomSoundDelay, Registered),
        ];
        const TRY_TO_SNIFF: &[(M, MemoryStatus)] = &[
            (M::IsSniffing, Registered),
            (M::WalkTarget, Registered),
            (M::SniffCooldown, ValueAbsent),
            (M::NearestAttackable, ValuePresent),
            (M::DisturbanceLocation, ValueAbsent),
        ];
        const SET_ROAR_TARGET: &[(M, MemoryStatus)] = &[
            (M::RoarTarget, ValueAbsent),
            (M::AttackTarget, ValueAbsent),
            (M::CantReachWalkTargetSince, Registered),
        ];
        const SET_LOOK_TARGET: &[(M, MemoryStatus)] = &[
            (M::LookTarget, Registered),
            (M::DisturbanceLocation, Registered),
            (M::RoarTarget, Registered),
            (M::AttackTarget, ValueAbsent),
        ];
        const GO_TO_TARGET: &[(M, MemoryStatus)] = &[
            (M::DisturbanceLocation, ValuePresent),
            (M::AttackTarget, ValueAbsent),
            (M::WalkTarget, ValueAbsent),
            (M::LookTarget, Registered),
        ];
        const DIG_COOLDOWN: &[(M, MemoryStatus)] = &[(M::DigCooldown, Registered)];
        const STOP_ATTACKING: &[(M, MemoryStatus)] = &[(M::AttackTarget, ValuePresent)];
        const SET_ENTITY_LOOK: &[(M, MemoryStatus)] =
            &[(M::AttackTarget, ValuePresent), (M::LookTarget, Registered)];
        const SET_WALK: &[(M, MemoryStatus)] =
            &[(M::AttackTarget, ValuePresent), (M::WalkTarget, Registered)];
        const MELEE: &[(M, MemoryStatus)] = &[
            (M::LookTarget, Registered),
            (M::AttackTarget, ValuePresent),
            (M::AttackCoolingDown, ValueAbsent),
            (M::NearestVisibleLivingEntities, ValuePresent),
        ];
        match self {
            Self::Emerging => EMERGING,
            Self::Digging => DIGGING,
            Self::Roar => ROAR,
            Self::Sniffing => SNIFFING,
            Self::SonicBoom => SONIC_BOOM,
            Self::TryToSniff => TRY_TO_SNIFF,
            Self::SetRoarTarget => SET_ROAR_TARGET,
            Self::SetWardenLookTarget => SET_LOOK_TARGET,
            Self::GoToTargetLocation => GO_TO_TARGET,
            Self::DigCooldownSetter => DIG_COOLDOWN,
            Self::StopAttackingIfTargetInvalid => STOP_ATTACKING,
            Self::SetEntityLookTarget => SET_ENTITY_LOOK,
            Self::SetWalkTargetFromAttackTargetIfTargetOutOfReach => SET_WALK,
            Self::MeleeAttack => MELEE,
            Self::ForceUnmount => &[],
        }
    }

    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Emerging => "Emerging",
            Self::Digging => "Digging",
            Self::ForceUnmount => "ForceUnmount",
            Self::Roar => "Roar",
            Self::Sniffing => "Sniffing",
            Self::SonicBoom => "SonicBoom",
            Self::TryToSniff => "TryToSniff",
            Self::SetRoarTarget => "SetRoarTarget",
            Self::SetWardenLookTarget => "SetWardenLookTarget",
            Self::GoToTargetLocation => "GoToTargetLocation",
            Self::DigCooldownSetter => "DIG_COOLDOWN_SETTER",
            Self::StopAttackingIfTargetInvalid => "StopAttackingIfTargetInvalid",
            Self::SetEntityLookTarget => "SetEntityLookTarget",
            Self::SetWalkTargetFromAttackTargetIfTargetOutOfReach => {
                "SetWalkTargetFromAttackTargetIfTargetOutOfReach"
            }
            Self::MeleeAttack => "MeleeAttack",
        }
    }

    pub(super) fn extra_start(&mut self, cx: &mut Cx) -> bool {
        match self {
            // `Digging.checkExtraStartConditions`：`onGround() || isInLiquid()`。
            Self::Digging => cx.me.on_ground || cx.me.in_water,
            // `SonicBoom.checkExtraStartConditions`：目标在水平 15 / 垂直 20 内。
            Self::SonicBoom => {
                let Some(target) = cx.memories.entity(M::AttackTarget) else {
                    return false;
                };
                let Some(pos) = entity_position(cx, target) else {
                    return false;
                };
                let d = pos.subtract(cx.me.position);
                d.x.abs() <= 15.0 && d.z.abs() <= 15.0 && d.y.abs() <= 20.0
            }
            _ => true,
        }
    }

    pub(super) fn start(&mut self, cx: &mut Cx) {
        match self {
            Self::Emerging => {
                cx.mob.warden.pose = WardenPose::Emerging;
            }
            Self::Digging => {
                if cx.me.on_ground {
                    cx.mob.warden.pose = WardenPose::Digging;
                } else {
                    // `Digging.start` 不在地面时当场 `stop()`。
                    cx.me.removed = true;
                }
            }
            Self::Roar => {
                cx.memories.set_with_ttl(
                    M::RoarSoundDelay,
                    MemoryValue::Unit,
                    i64::from(ROAR_SOUND_DELAY),
                );
                cx.memories.erase(M::WalkTarget);
                // `BehaviorUtils.lookAtEntity` 只把 LOOK_TARGET 设成 EntityTracker，
                // 朝向由 CORE 的 LookAtTargetSink 逐步转过去。
                // `BehaviorUtils.lookAtEntity` only sets LOOK_TARGET to an EntityTracker;
                // CORE's LookAtTargetSink turns the head gradually.
                if let Some(target) = cx.memories.entity(M::RoarTarget) {
                    cx.memories.set(
                        M::LookTarget,
                        MemoryValue::Look(PositionTracker::Entity {
                            uid: target,
                            track_eye: true,
                            target_eye: false,
                        }),
                    );
                }
                cx.mob.warden.pose = WardenPose::Roaring;
                if let Some(target) = cx.memories.entity(M::RoarTarget) {
                    increase_anger_at(cx, target, 20, false);
                }
            }
            Self::Sniffing => {
                // `Sniffing.start`：嗅探声（声音事件由同步层发）。
            }
            Self::SonicBoom => {
                cx.memories.set_with_ttl(
                    M::AttackCoolingDown,
                    MemoryValue::Bool(true),
                    i64::from(SONIC_BOOM_DURATION),
                );
                cx.memories.set_with_ttl(
                    M::SonicBoomSoundDelay,
                    MemoryValue::Unit,
                    i64::from(SONIC_BOOM_SOUND_DELAY),
                );
                entity_event(cx, 62);
            }
            _ => {}
        }
    }

    pub(super) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        // `Digging.canStillUse`：还没被移除。
        !matches!(self, Self::Digging) || !cx.me.removed
    }

    pub(super) fn tick(&mut self, cx: &mut Cx) {
        match self {
            Self::Roar => {
                // 声音延迟过了、冷却也不在，才播一次咆哮并起冷却。
                if !cx.memories.has(M::RoarSoundDelay) && !cx.memories.has(M::RoarSoundCooldown) {
                    cx.memories.set_with_ttl(
                        M::RoarSoundCooldown,
                        MemoryValue::Unit,
                        i64::from(ROAR_DURATION - ROAR_SOUND_DELAY),
                    );
                    sound(cx, "minecraft:entity.warden.roar", 3.0, 1.0);
                }
            }
            Self::SonicBoom => {
                // `SonicBoom.tick` 的 `getLookControl().setLookAt(target.position())`
                // 追的是脚底位置（没有眼睛高度）。
                // `SonicBoom.tick`'s `getLookControl().setLookAt(target.position())`
                // tracks the feet rather than the eyes.
                if let Some(target) = cx.memories.entity(M::AttackTarget)
                    && let Some(pos) = entity_position(cx, target)
                {
                    cx.mob.look.set_look_at_default(pos.x, pos.y, pos.z);
                }
                if cx.memories.has(M::SonicBoomSoundDelay)
                    || cx.memories.has(M::SonicBoomSoundCooldown)
                {
                    return;
                }
                cx.memories.set_with_ttl(
                    M::SonicBoomSoundCooldown,
                    MemoryValue::Unit,
                    i64::from(SONIC_BOOM_DURATION - SONIC_BOOM_SOUND_DELAY),
                );
                if let Some(target) = cx.memories.entity(M::AttackTarget)
                    && can_target_entity(cx, target)
                    && let Some(eye) = super::brain::sensor::living(cx, target).map(|view| {
                        view.position
                            .add(Vec3::new(0.0, f64::from(view.eye_height), 0.0))
                    })
                {
                    sonic_boom_damage(cx, target, eye);
                }
            }
            _ => {}
        }
    }

    pub(super) fn stop(&mut self, cx: &mut Cx, _timed_out: bool) {
        match self {
            Self::Emerging => {
                if cx.mob.warden.pose == WardenPose::Emerging {
                    cx.mob.warden.pose = WardenPose::Standing;
                }
            }
            Self::Digging => {
                // `Digging.stop`：钻回地下即移除实体。
                if !cx.me.removed {
                    cx.me.removed = true;
                }
            }
            Self::Sniffing => {
                if cx.mob.warden.pose == WardenPose::Sniffing {
                    cx.mob.warden.pose = WardenPose::Standing;
                }
                cx.memories.erase(M::IsSniffing);
                if let Some(target) = cx.memories.entity(M::NearestAttackable)
                    && can_target_entity(cx, target)
                {
                    if let Some(pos) = entity_position(cx, target) {
                        let d = pos.subtract(cx.me.position);
                        if d.x.abs() <= 6.0 && d.z.abs() <= 6.0 && d.y.abs() <= 20.0 {
                            increase_anger_at(cx, target, 35, true);
                        }
                    }
                    if !cx.memories.has(M::DisturbanceLocation) {
                        set_disturbance_location(
                            cx,
                            BlockPos::containing(
                                entity_position(cx, target).unwrap_or(cx.me.position),
                            ),
                        );
                    }
                }
            }
            Self::Roar => {
                if cx.mob.warden.pose == WardenPose::Roaring {
                    cx.mob.warden.pose = WardenPose::Standing;
                }
                if let Some(target) = cx.memories.entity(M::RoarTarget) {
                    set_attack_target(cx, target);
                }
                cx.memories.erase(M::RoarTarget);
            }
            Self::SonicBoom => {
                cx.memories
                    .set_with_ttl(M::SonicBoomCooldown, MemoryValue::Unit, 40);
            }
            _ => {}
        }
    }

    pub(super) fn trigger(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::ForceUnmount => cx.me.vehicle.is_some(),
            Self::TryToSniff => {
                cx.memories.set(M::IsSniffing, MemoryValue::Unit);
                // `UniformInt.of(100, 200).sample(level.getRandom())`：`100 + nextInt(101)`。
                let ttl = 100 + cx.level_next_int(101);
                cx.memories
                    .set_with_ttl(M::SniffCooldown, MemoryValue::Unit, i64::from(ttl));
                cx.memories.erase(M::WalkTarget);
                cx.mob.warden.pose = WardenPose::Sniffing;
                true
            }
            Self::SetRoarTarget => {
                // `Warden::getEntityAngryAt`：ANGRY 时取愤怒最高的嫌疑。
                let angry = cx.mob.warden.anger.anger_level().is_angry();
                let candidate = angry
                    .then(|| cx.mob.warden.anger.get_active_entity())
                    .flatten();
                let Some(uid) = candidate else {
                    return false;
                };
                if !can_target_entity(cx, uid) {
                    return false;
                }
                cx.memories.set(M::RoarTarget, MemoryValue::Entity(uid));
                cx.memories.erase(M::CantReachWalkTargetSince);
                true
            }
            Self::SetWardenLookTarget => {
                // ROAR_TARGET 的位置优先，否则 DISTURBANCE_LOCATION。
                let pos = cx
                    .memories
                    .entity(M::RoarTarget)
                    .and_then(|uid| entity_position(cx, uid))
                    .map(BlockPos::containing)
                    .or_else(|| cx.memories.block_pos(M::DisturbanceLocation));
                let Some(pos) = pos else {
                    return false;
                };
                cx.memories.set(
                    M::LookTarget,
                    MemoryValue::Look(PositionTracker::at_block(pos)),
                );
                true
            }
            Self::GoToTargetLocation => {
                let Some(pos) = cx.memories.block_pos(M::DisturbanceLocation) else {
                    return false;
                };
                let center = Vec3::new(
                    f64::from(pos.x) + 0.5,
                    f64::from(pos.y),
                    f64::from(pos.z) + 0.5,
                );
                if center.subtract(cx.me.position).length_sqr() > 4.0 {
                    // `getNearbyPos`：两次 `nextInt(3) - 1` 偏移。
                    let dx = cx.level_next_int(3) - 1;
                    let dz = cx.level_next_int(3) - 1;
                    let nearby = pos.offset(dx, 0, dz);
                    let point = Vec3::new(
                        f64::from(nearby.x) + 0.5,
                        f64::from(nearby.y),
                        f64::from(nearby.z) + 0.5,
                    );
                    cx.memories.set(
                        M::WalkTarget,
                        MemoryValue::Walk(WalkTarget::to_point(point, 0.7, 2)),
                    );
                    cx.memories.set(
                        M::LookTarget,
                        MemoryValue::Look(PositionTracker::at_block(nearby)),
                    );
                }
                true
            }
            Self::DigCooldownSetter => {
                if cx.memories.has(M::DigCooldown) {
                    cx.memories.set_with_ttl(
                        M::DigCooldown,
                        MemoryValue::Unit,
                        i64::from(DIGGING_COOLDOWN),
                    );
                }
                true
            }
            Self::StopAttackingIfTargetInvalid => {
                let Some(target) = cx.memories.entity(M::AttackTarget) else {
                    return false;
                };
                // `!isAngry() || !canTargetEntity(target)` 时停手。
                let stop =
                    !cx.mob.warden.anger.anger_level().is_angry() || !can_target_entity(cx, target);
                if stop {
                    if !can_target_entity(cx, target) {
                        cx.mob.warden.anger.clear_anger(target);
                    }
                    set_dig_cooldown(cx);
                    cx.memories.erase(M::AttackTarget);
                }
                true
            }
            Self::SetEntityLookTarget => {
                let Some(target) = cx.memories.entity(M::AttackTarget) else {
                    return false;
                };
                cx.memories.set(
                    M::LookTarget,
                    MemoryValue::Look(PositionTracker::Entity {
                        uid: target,
                        track_eye: true,
                        target_eye: false,
                    }),
                );
                true
            }
            Self::SetWalkTargetFromAttackTargetIfTargetOutOfReach => {
                let Some(target) = cx.memories.entity(M::AttackTarget) else {
                    return false;
                };
                // 看得见且在近战范围里就擦掉行走目标，否则追着 EntityTracker 走。
                // Erase the walk target when visible and within melee reach;
                // otherwise follow an EntityTracker.
                if within_melee_reach(cx, target) {
                    cx.memories.erase(M::WalkTarget);
                } else {
                    cx.memories.set(
                        M::LookTarget,
                        MemoryValue::Look(PositionTracker::Entity {
                            uid: target,
                            track_eye: true,
                            target_eye: false,
                        }),
                    );
                    cx.memories.set(
                        M::WalkTarget,
                        MemoryValue::Walk(WalkTarget::to_entity(target, 1.2, 0)),
                    );
                }
                true
            }
            Self::MeleeAttack => {
                let Some(target) = cx.memories.entity(M::AttackTarget) else {
                    return false;
                };
                if cx.memories.has(M::AttackCoolingDown) || !can_target_entity(cx, target) {
                    return false;
                }
                if !within_melee_reach(cx, target) {
                    return false;
                }
                // `MeleeAttack` 把 LOOK_TARGET 设成眼睛追踪器。
                // `MeleeAttack` sets LOOK_TARGET to an eye-height EntityTracker.
                cx.memories.set(
                    M::LookTarget,
                    MemoryValue::Look(PositionTracker::Entity {
                        uid: target,
                        track_eye: true,
                        target_eye: false,
                    }),
                );
                entity_event(cx, 4);
                cx.memories
                    .set_with_ttl(M::AttackCoolingDown, MemoryValue::Bool(true), 18);
                cx.memories
                    .set_with_ttl(M::SonicBoomCooldown, MemoryValue::Unit, 40);
                melee_damage(cx, target);
                true
            }
            _ => false,
        }
    }
}

/// `WardenAi.getActivities`：8 个活动，顺序 / 优先级 / 记忆条件照字节码。
fn activities() -> Vec<ActivityData> {
    let w = |behavior| Kind::Warden(behavior);
    vec![
        ActivityData::sequential(
            Activity::Core,
            0,
            vec![
                Kind::Swim { chance: 0.8 },
                w(Behavior::SetWardenLookTarget),
                Kind::LookAtTargetSink { min: 45, max: 90 },
                Kind::MoveToTargetSink(MoveToTargetSink::new(false)),
            ],
        ),
        ActivityData::sequential(Activity::Emerge, 5, vec![w(Behavior::Emerging)])
            .when(vec![(M::IsEmerging, ValuePresent)])
            .erasing(vec![M::IsEmerging]),
        ActivityData::with_priorities(
            Activity::Dig,
            vec![(0, w(Behavior::ForceUnmount)), (1, w(Behavior::Digging))],
        )
        .when(vec![
            (M::RoarTarget, ValueAbsent),
            (M::DigCooldown, ValueAbsent),
        ]),
        ActivityData::sequential(
            Activity::Idle,
            10,
            vec![
                w(Behavior::SetRoarTarget),
                w(Behavior::TryToSniff),
                // `RunOne(IS_SNIFFING ABSENT; [RandomStroll(0.5) 权2, DoNothing(30,60) 权1])`。
                // `RunOne(IS_SNIFFING ABSENT; [RandomStroll(0.5) w2, DoNothing(30,60) w1])`.
                Kind::Gate(Box::new(Gate::run_one(
                    vec![(M::IsSniffing, ValueAbsent)],
                    vec![
                        (
                            Kind::RandomStroll {
                                speed: 0.5,
                                horizontal: 10,
                                vertical: 7,
                            },
                            2,
                        ),
                        (Kind::DoNothing { min: 30, max: 60 }, 1),
                    ],
                ))),
            ],
        ),
        ActivityData::sequential(Activity::Roar, 10, vec![w(Behavior::Roar)])
            .when(vec![(M::RoarTarget, ValuePresent)])
            .erasing(vec![M::RoarTarget]),
        ActivityData::sequential(
            Activity::Fight,
            10,
            vec![
                w(Behavior::DigCooldownSetter),
                w(Behavior::StopAttackingIfTargetInvalid),
                w(Behavior::SetEntityLookTarget),
                w(Behavior::SetWalkTargetFromAttackTargetIfTargetOutOfReach),
                w(Behavior::SonicBoom),
                w(Behavior::MeleeAttack),
            ],
        )
        .when(vec![(M::AttackTarget, ValuePresent)])
        .erasing(vec![M::AttackTarget]),
        ActivityData::sequential(
            Activity::Investigate,
            5,
            vec![w(Behavior::SetRoarTarget), w(Behavior::GoToTargetLocation)],
        )
        .when(vec![(M::DisturbanceLocation, ValuePresent)])
        .erasing(vec![M::DisturbanceLocation]),
        ActivityData::sequential(
            Activity::Sniff,
            5,
            vec![w(Behavior::SetRoarTarget), w(Behavior::Sniffing)],
        )
        .when(vec![(M::IsSniffing, ValuePresent)])
        .erasing(vec![M::IsSniffing]),
    ]
}

/// 监守者的大脑：两个传感器（`NEAREST_PLAYERS` / `WARDEN_ENTITY_SENSOR`）+ 8 个活动。
/// 官方 `BRAIN_PROVIDER` 还显式登记四个只由实体方法读写的记忆。
///
/// The warden brain: two sensors and eight activities. Vanilla's `BRAIN_PROVIDER`
/// also registers four memories read/written only by the entity's own methods.
pub fn brain() -> Brain {
    use crate::entity::mob::brain::{BrainTemplate, TemplateCache, template_cache};
    static TEMPLATE: TemplateCache = template_cache();
    Brain::cached(&TEMPLATE, || {
        BrainTemplate::new_with_memories(
            &[SensorKind::NearestPlayers, SensorKind::WardenEntitySensor],
            &[
                M::NearestVisibleNemesis,
                M::RecentProjectile,
                M::TouchCooldown,
                M::VibrationCooldown,
            ],
            activities(),
        )
    })
}

/// `WardenAi.updateActivity`：按这个顺序选第一个有效的活动。
pub fn update_activity(brain: &mut Brain) {
    brain.set_active_activity_to_first_valid(&[
        Activity::Emerge,
        Activity::Dig,
        Activity::Roar,
        Activity::Fight,
        Activity::Investigate,
        Activity::Sniff,
        Activity::Idle,
    ]);
}

/// `WardenEntitySensor.doTick`：先跑 super 的 `NearestLivingEntitySensor.doTick`
/// 写 `NEAREST_LIVING_ENTITIES` / `NEAREST_VISIBLE_LIVING_ENTITIES`，再从前者里先找
/// 玩家、再找非玩家，写 `NEAREST_ATTACKABLE`；都没有就擦掉。
pub fn warden_entity_sensor(cx: &mut Cx) {
    super::brain::sensor::sense_nearest_living(cx);
    // 官方 `Brain.forEach` 对空集合走 `acceptEmpty`（不打印）；`mobs` 为空时擦掉，
    // `visible_mobs` 是 `NearestVisibleLivingEntities` 对象，空也照样打印。
    // Vanilla's `Brain.forEach` skips empty collections via `acceptEmpty`: erase `mobs`
    // when empty, while `visible_mobs` (an object) is printed even when empty.
    if matches!(cx.memories.get(M::NearestLivingEntities), Some(MemoryValue::Entities(ids)) if ids.is_empty())
    {
        cx.memories.erase(M::NearestLivingEntities);
    }
    let candidates: Vec<_> = match cx.memories.get(M::NearestLivingEntities) {
        Some(MemoryValue::Entities(uids)) => uids.iter().copied().collect(),
        Some(MemoryValue::Visible(visible)) => visible.entities.iter().copied().collect(),
        _ => Vec::new(),
    };
    let is_player = |uid: crate::entity::EntityUid| uid < crate::entity::FIRST_ENTITY_ID;
    let pick = |player: bool| {
        candidates
            .iter()
            .copied()
            .find(|uid| is_player(*uid) == player && can_target_entity(cx, *uid))
    };
    let chosen = pick(true).or_else(|| pick(false));
    cx.memories
        .set_or_erase(M::NearestAttackable, chosen.map(MemoryValue::Entity));
}

/// `Warden.canTargetEntity`：活着的 LivingEntity、不是自己、不在创造 / 旁观模式、
/// 不是另一个监守者、还没被移除、不是无敌、碰撞箱在世界边界内。
/// （盔甲架和同队 `isAlliedTo` 两支 Bamboo 还没建模：没有盔甲架生物，
/// 生物也不会加入队伍。）
///
/// `Warden.canTargetEntity`: a living entity, not self, not creative / spectator,
/// not another warden, not removed, not invulnerable and inside the world border.
/// (The armour-stand and `isAlliedTo` branches are not modelled yet: Bamboo has
/// no armour stands and mobs never join teams.)
fn can_target_entity(cx: &Cx, uid: crate::entity::EntityUid) -> bool {
    if uid == cx.me.id || cx.me.removed {
        return false;
    }
    let Some(view) = super::brain::sensor::living(cx, uid) else {
        return false;
    };
    if !view.alive {
        return false;
    }
    if view.player {
        // `NO_CREATIVE_OR_SPECTATOR`：生存(0) / 冒险(2) 可以，创造(1) / 旁观(3) 不行。
        return !matches!(
            cx.ctx.player(uid).map(|player| player.game_mode),
            Some(1 | 3)
        );
    }
    if view.mob_type == Some(MobType::Warden) || view.invulnerable {
        return false;
    }
    // `level().getWorldBorder().isWithinBounds(livingEntity.getBoundingBox())`。
    cx.ctx
        .blocks()
        .world_border()
        .is_within_bounds_aabb(view.bounding_box)
}

/// `BehaviorUtils.isWithinAttackRange` 与 `MeleeAttack` 共用的判定：看得见且在
/// `getBoundingBox().inflate(DEFAULT_ATTACK_REACH, 0, ...)` 命中范围内。
///
/// The shared check for `BehaviorUtils.isWithinAttackRange` and `MeleeAttack`:
/// visible and inside the inflated attack bounding box.
fn within_melee_reach(cx: &Cx, uid: crate::entity::EntityUid) -> bool {
    let visible = cx
        .memories
        .get(M::NearestVisibleLivingEntities)
        .is_some_and(
            |value| matches!(value, MemoryValue::Visible(list) if list.entities.contains(&uid)),
        );
    if !visible {
        return false;
    }
    let Some(view) = super::brain::sensor::living(cx, uid) else {
        return false;
    };
    let reach = 2.04_f64.sqrt() - 0.6000000238418579_f64;
    cx.me
        .bounding_box()
        .inflate(reach, 0.0, reach)
        .intersects(view.bounding_box)
}

/// 只读实体位置（`EntityCtx` 提供视图）。
fn entity_position(cx: &Cx, uid: crate::entity::EntityUid) -> Option<Vec3> {
    super::brain::sensor::living(cx, uid).map(|view| view.position)
}

fn entity_event(cx: &mut Cx, event: u8) {
    cx.mob.events.push(MobEvent::EntityEvent { event });
}

fn sound(cx: &mut Cx, name: &'static str, volume: f32, pitch: f32) {
    cx.mob.events.push(MobEvent::Sound {
        sound: name,
        volume,
        pitch,
        position: cx.me.position,
        hostile: true,
        seed: None,
    });
}

/// `LivingEntity.getVoicePitch()`：两次实体随机数决定音高。
///
/// `LivingEntity.getVoicePitch()`: two entity random draws determine the pitch.
fn voice_pitch(me: &mut Entity) -> f32 {
    let first = me.random.next_float();
    let second = me.random.next_float();
    (first - second) * 0.2 + 1.0
}

/// `Warden.doHurtTarget`：`super.doHurtTarget`（ATTACK_DAMAGE = 30 的近战伤害）、
/// `WARDEN_ATTACK_IMPACT`（`getVoicePitch` 消耗实体随机源两次）、40 刻音波冷却。
fn melee_damage(cx: &mut Cx, target: crate::entity::EntityUid) {
    super::living::do_hurt_target(cx.me, cx.mob, cx.ctx, target);
    let pitch = voice_pitch(cx.me);
    cx.mob.events.push(MobEvent::Sound {
        sound: "minecraft:entity.warden.attack_impact",
        volume: 10.0,
        pitch,
        position: cx.me.position,
        hostile: true,
        seed: None,
    });
}

/// `SonicBoom.lambda$tick$2`：从胸口附着点到目标眼睛发一串 `sonic_boom` 粒子，
/// 播放音爆声，再打 10 点 `sonic_boom` 伤害；打中才补一次 `push`。
fn sonic_boom_damage(cx: &mut Cx, target: crate::entity::EntityUid, eye: Vec3) {
    // `EntityAttachments.get(WARDEN_CHEST, 0, yRot)`：官方胸口附着点是相对脚底
    // 的 (0, 1.6, 0)，X/Z 都是 0，绕 Y 旋转后不变。官方把 1.6 按 f32 存，
    // 转成 f64 是 1.600000023841858，这里照用这个值。
    let chest = Vec3::new(
        cx.me.position.x,
        cx.me.position.y + f64::from(1.6_f32),
        cx.me.position.z,
    );
    let to_eye = eye.subtract(chest);
    let length = to_eye.length();
    let direction = to_eye.normalize();
    let count = mth::floor(length) + 7;
    for step in 1..count {
        let at = chest.add(direction.scale(f64::from(step)));
        cx.mob.events.push(MobEvent::Particles {
            particle: particle_type::SONIC_BOOM,
            item: None,
            position: at,
            offset: Vec3::default(),
            speed: 0.0,
            count: 1,
        });
    }
    sound(cx, "minecraft:entity.warden.sonic_boom", 3.0, 1.0);
    let mut source = DamageSource::environment(DamageKind::SonicBoom).with_non_player_cause(true);
    source.attacker = Some(cx.me.id);
    source.direct = Some(cx.me.id);
    source.attacker_uuid = Some(cx.mob.uuid);
    source.attacker_type = Some(cx.mob.mob_type.entity_type());
    source.position = Some(cx.me.position);
    let is_player = cx.ctx.player(target).is_some();
    if is_player {
        cx.ctx.emit(
            Command::HurtPlayer {
                target,
                source,
                amount: 10.0,
            },
            cx.me,
        );
    } else {
        cx.ctx.emit(
            Command::HurtMob {
                target,
                source,
                amount: 10.0,
                on_hit: None,
            },
            cx.me,
        );
    }
    // 官方 `push` 用受害者的 KNOCKBACK_RESISTANCE 分别缩放水平 / 竖直量。
    // 玩家视图没有击退抗性字段，正式入口在同步层按官方值算；这里照对拍夹具
    // （无抗性修饰）按 0 处理。生物直接从公开属性表读。
    // The official `push` scales the horizontal / vertical amounts by the victim's
    // KNOCKBACK_RESISTANCE. Player views don't carry that attribute (the sync
    // layer computes it), so fixtures use 0; mobs read it from their public
    // attribute table.
    let mut resistance = 0.0_f64;
    if !is_player {
        cx.ctx.with_other(target, &mut |public| {
            if let crate::entity::KindPublic::Mob(mob) = &public.kind {
                resistance = mob
                    .attributes
                    .value(mc_bamboo_data::registries::attribute::KNOCKBACK_RESISTANCE);
            }
        });
    }
    let horizontal = 2.5 * (1.0 - resistance);
    let vertical = 0.5 * (1.0 - resistance);
    cx.ctx.emit(
        Command::SonicBoomPush {
            target,
            player: is_player,
            push: Vec3::new(
                direction.x * horizontal,
                direction.y * vertical,
                direction.z * horizontal,
            ),
        },
        cx.me,
    );
}

/// `Warden.increaseAngerAt(entity, amount, playSound)`：加愤怒、刷钻地冷却，
/// 从非玩家目标转成 ANGRY 的玩家时擦掉 `ATTACK_TARGET`。
pub fn increase_anger_at(
    cx: &mut Cx,
    uid: crate::entity::EntityUid,
    amount: i32,
    _play_sound: bool,
) {
    if cx.mob.no_ai || !can_target_entity(cx, uid) {
        return;
    }
    set_dig_cooldown(cx);
    let was_not_player = cx
        .memories
        .entity(M::AttackTarget)
        .is_none_or(|t| t >= crate::entity::FIRST_ENTITY_ID);
    let uuid = crate::archive::entity::entity_uuid(uid).to_be_bytes();
    let is_player = uid < crate::entity::FIRST_ENTITY_ID;
    let anger = cx
        .mob
        .warden
        .anger
        .increase_anger(uid, uuid, amount, is_player);
    if is_player && was_not_player && super::warden::AngerLevel::by_anger(anger).is_angry() {
        cx.memories.erase(M::AttackTarget);
    }
}

/// `Warden.doPush`：没有推挤冷却时起 20 刻冷却、加 35 点愤怒、把对方脚下的
/// 方块设成调查点。推挤本身由 `living::push_entities` 完成。
///
/// `Warden.doPush`: without a touch cooldown, start a 20-tick cooldown, add 35
/// anger and set the disturbance location to the pushed entity's block position.
pub fn do_push(me: &mut Entity, mob: &mut Mob, pushed: &[(EntityUid, Vec3)]) {
    for (uid, position) in pushed {
        if mob.no_ai
            || mob
                .brain
                .as_ref()
                .is_some_and(|brain| brain.memories.has(M::TouchCooldown))
            || *uid == me.id
        {
            continue;
        }
        let uuid = crate::archive::entity::entity_uuid(*uid).to_be_bytes();
        let is_player = *uid < crate::entity::FIRST_ENTITY_ID;
        if let Some(brain) = mob.brain.as_mut() {
            brain
                .memories
                .set_with_ttl(M::TouchCooldown, MemoryValue::Unit, 20);
        }
        let was_not_player = mob
            .brain
            .as_ref()
            .and_then(|brain| brain.memories.entity(M::AttackTarget))
            .is_none_or(|target| target >= crate::entity::FIRST_ENTITY_ID);
        let anger = mob.warden.anger.increase_anger(*uid, uuid, 35, is_player);
        if is_player
            && was_not_player
            && super::warden::AngerLevel::by_anger(anger).is_angry()
            && let Some(brain) = mob.brain.as_mut()
        {
            brain.memories.erase(M::AttackTarget);
        }
        // `increaseAngerAt(entity)` 带播放监听声，消耗实体随机源两次。
        if mob.warden.pose != WardenPose::Roaring {
            let _ = voice_pitch(me);
        }
        set_disturbance(mob, BlockPos::containing(*position));
    }
}

/// `WardenAi.setDigCooldown`：只在 `DIG_COOLDOWN` 已存在时刷新 1200 刻。
pub fn set_dig_cooldown(cx: &mut Cx) {
    if cx.memories.has(M::DigCooldown) {
        cx.memories.set_with_ttl(
            M::DigCooldown,
            MemoryValue::Unit,
            i64::from(DIGGING_COOLDOWN),
        );
    }
}

/// `WardenAi.setDisturbanceLocation`：四项记忆 + 刷钻地冷却。
pub fn set_disturbance_location(cx: &mut Cx, pos: BlockPos) {
    let angry = cx.mob.warden.anger.anger_level().is_angry();
    if angry || cx.memories.has(M::AttackTarget) {
        return;
    }
    set_dig_cooldown(cx);
    cx.memories
        .set_with_ttl(M::SniffCooldown, MemoryValue::Unit, DISTURBANCE_EXPIRY);
    cx.memories.set_with_ttl(
        M::LookTarget,
        MemoryValue::Look(PositionTracker::at_block(pos)),
        DISTURBANCE_EXPIRY,
    );
    cx.memories.set_with_ttl(
        M::DisturbanceLocation,
        MemoryValue::BlockPos(pos),
        DISTURBANCE_EXPIRY,
    );
    cx.memories.erase(M::WalkTarget);
}

/// `Warden.setAttackTarget`：擦 ROAR_TARGET、设 ATTACK_TARGET、擦 CANT_REACH、起音波冷却。
pub fn set_attack_target(cx: &mut Cx, uid: crate::entity::EntityUid) {
    cx.memories.erase(M::RoarTarget);
    cx.memories.set(M::AttackTarget, MemoryValue::Entity(uid));
    cx.memories.erase(M::CantReachWalkTargetSince);
    cx.memories
        .set_with_ttl(M::SonicBoomCooldown, MemoryValue::Unit, 200);
}

/// `Warden.customServerAiStep`：官方在 `Warden.tick()` 里先跑
/// `VibrationSystem.Ticker.tick`（回调设的记忆当刻就在 `super.tick()` 的脑 tick 里
/// 递减），随后跑大脑、每 20 刻衰减愤怒并同步客户端等级、每刻末重选活动。
///
/// `Warden.customServerAiStep`: vanilla runs `VibrationSystem.Ticker.tick` first
/// (its memories decay in the same tick's brain tick), then ticks the brain,
/// decays anger / syncs the client level every 20 ticks and reselects the activity.
pub fn custom_server_ai_step(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    decision_gap: i64,
) {
    let now = ctx.blocks().game_time();
    mob.warden.tick_animations(me.tick_count);
    let eye = Vec3::new(
        me.position.x,
        me.position.y + f64::from(mob.eye_height),
        me.position.z,
    );
    let mut data = mob
        .entity_listener
        .map(|listener| listener.vibration)
        .unwrap_or_default();
    {
        let mut user = WardenVibrationUser {
            me: &mut *me,
            mob,
            ctx: &*ctx,
        };
        crate::game_event::vibration::tick(&mut data, &mut user, now, Some(eye), false);
    }
    if let Some(listener) = mob.entity_listener.as_mut() {
        listener.vibration = data;
    }
    if let Some(mut brain) = mob.brain.take() {
        let replaced = brain.tick_by(me, mob, ctx, decision_gap.max(1));
        mob.brain = Some(Box::new(replaced.unwrap_or(*brain)));
    }
    // `Warden.customServerAiStep`：`(tickCount + getId()) % 120 == 0` 时给半径
    // 20 内的玩家上 260 刻黑暗效果。
    //
    // `Warden.customServerAiStep`: every `(tickCount + id) % 120` ticks, darkness
    // for 260 ticks on players within radius 20.
    if (me.tick_count + me.id) % 120 == 0 {
        ctx.emit(
            Command::WardenDarkness {
                center: me.position,
                radius: 20,
                ticks: 260,
            },
            me,
        );
    }
    if me.tick_count % ANGER_TICK_DELAY == 0 {
        mob.warden.anger.tick();
        let target = mob
            .brain
            .as_ref()
            .and_then(|brain| brain.memories.entity(M::AttackTarget));
        mob.warden.client_anger_level = mob.warden.anger.get_active_anger(target);
    }
    if let Some(brain) = mob.brain.as_mut() {
        update_activity(brain);
    }
}

/// `Warden$VibrationUser`：半径 16、眼睛位置、收到振动加愤怒并设调查点。
///
/// `Warden$VibrationUser`: radius 16, eye position, anger and disturbance on arrival.
struct WardenVibrationUser<'a> {
    me: &'a mut Entity,
    mob: &'a mut Mob,
    ctx: &'a dyn EntityCtx,
}

/// `Warden.increaseAngerAt` 里刷钻地冷却，不经过行为的 `Cx`。
///
/// Refresh the dig cooldown like `Warden.increaseAngerAt`, without a behaviour `Cx`.
fn refresh_dig_cooldown(mob: &mut Mob) {
    if let Some(brain) = mob.brain.as_mut()
        && brain.memories.has(M::DigCooldown)
    {
        brain.memories.set_with_ttl(
            M::DigCooldown,
            MemoryValue::Unit,
            i64::from(DIGGING_COOLDOWN),
        );
    }
}

/// `WardenAi.setDisturbanceLocation` 的非 `Cx` 版本（振动回调）。
///
/// `WardenAi.setDisturbanceLocation` for the vibration callback, without a `Cx`.
fn set_disturbance(mob: &mut Mob, pos: BlockPos) {
    if mob.warden.anger.anger_level().is_angry()
        || mob
            .brain
            .as_ref()
            .is_some_and(|brain| brain.memories.has(M::AttackTarget))
    {
        return;
    }
    refresh_dig_cooldown(mob);
    if let Some(brain) = mob.brain.as_mut() {
        brain
            .memories
            .set_with_ttl(M::SniffCooldown, MemoryValue::Unit, DISTURBANCE_EXPIRY);
        brain.memories.set_with_ttl(
            M::LookTarget,
            MemoryValue::Look(PositionTracker::at_block(pos)),
            DISTURBANCE_EXPIRY,
        );
        brain.memories.set_with_ttl(
            M::DisturbanceLocation,
            MemoryValue::BlockPos(pos),
            DISTURBANCE_EXPIRY,
        );
        brain.memories.erase(M::WalkTarget);
    }
}

impl WardenVibrationUser<'_> {
    fn entity_position(&self, uid: EntityUid) -> Option<Vec3> {
        self.ctx
            .player(uid)
            .map(|player| player.position)
            .or_else(|| self.ctx.other(uid).map(|other| other.position))
    }

    /// `Warden.canTargetEntity` 的可观察部分：不是自己、还在世界里。
    fn can_target(&self, uid: EntityUid) -> bool {
        uid != self.me.id && !self.me.removed && self.entity_position(uid).is_some()
    }

    /// `warden.closerThan(entity, distance)`。
    fn closer_than(&self, uid: EntityUid, distance: f64) -> bool {
        self.entity_position(uid)
            .is_some_and(|p| p.subtract(self.me.position).length_sqr() <= distance * distance)
    }

    /// `Warden.increaseAngerAt(entity, amount, playSound)`：刷钻地冷却、加愤怒、
    /// 从非玩家目标转成 ANGRY 玩家时擦掉 `ATTACK_TARGET`、按 `playSound` 播
    /// 监听声（`getVoicePitch` 消耗实体随机源两次）。
    fn increase_anger(&mut self, uid: EntityUid, amount: i32, play_sound: bool) {
        if self.mob.no_ai || !self.can_target(uid) {
            return;
        }
        refresh_dig_cooldown(self.mob);
        let was_not_player = self
            .mob
            .brain
            .as_ref()
            .and_then(|brain| brain.memories.entity(M::AttackTarget))
            .is_none_or(|target| target >= crate::entity::FIRST_ENTITY_ID);
        let uuid = crate::archive::entity::entity_uuid(uid).to_be_bytes();
        let is_player = uid < crate::entity::FIRST_ENTITY_ID;
        let anger = self
            .mob
            .warden
            .anger
            .increase_anger(uid, uuid, amount, is_player);
        if is_player
            && was_not_player
            && super::warden::AngerLevel::by_anger(anger).is_angry()
            && let Some(brain) = self.mob.brain.as_mut()
        {
            brain.memories.erase(M::AttackTarget);
        }
        if play_sound && self.mob.warden.pose != super::warden::WardenPose::Roaring {
            let pitch = voice_pitch(self.me);
            self.mob.events.push(MobEvent::Sound {
                sound: self.mob.warden.anger.anger_level().listening_sound(),
                volume: 10.0,
                pitch,
                position: self.me.position,
                hostile: true,
                seed: None,
            });
        }
    }
}

impl VibrationUser for WardenVibrationUser<'_> {
    fn radius(&self) -> i32 {
        16
    }

    fn position(&self) -> Option<Vec3> {
        Some(Vec3::new(
            self.me.position.x,
            self.me.position.y + f64::from(self.mob.eye_height),
            self.me.position.z,
        ))
    }

    fn can_receive(&self, _event: GameEvent, _source: BlockPos) -> bool {
        if self.mob.no_ai || self.me.removed || self.mob.warden.pose.is_digging_or_emerging() {
            return false;
        }
        !self
            .mob
            .brain
            .as_ref()
            .is_some_and(|brain| brain.memories.has(M::VibrationCooldown))
    }

    fn on_receive(
        &mut self,
        event: GameEvent,
        source: BlockPos,
        source_player: Option<EntityUid>,
        distance: f32,
    ) {
        self.on_receive_full(event, source, source_player, None, None, distance);
    }

    /// `Warden$VibrationUser.onReceiveVibration`：e4 是源实体、e5 是射弹拥有者。
    ///
    /// `Warden$VibrationUser.onReceiveVibration`: e4 is the source entity and e5 the
    /// projectile owner.
    fn on_receive_full(
        &mut self,
        _event: GameEvent,
        source: BlockPos,
        source_player: Option<EntityUid>,
        source_entity: Option<EntityUid>,
        projectile_owner: Option<EntityUid>,
        _distance: f32,
    ) {
        if self.me.removed {
            return;
        }
        let mut target_pos = source;
        if let Some(brain) = self.mob.brain.as_mut() {
            brain
                .memories
                .set_with_ttl(M::VibrationCooldown, MemoryValue::Unit, 40);
        }
        // `Warden.handleEntityEvent(61)` 把触须动画置 10；事件本身仍发给客户端。
        // `Warden.handleEntityEvent(61)` resets the tendril animation to 10;
        // the event itself still goes to clients.
        self.mob.warden.tendril_animation = 10;
        self.mob.events.push(MobEvent::EntityEvent { event: 61 });
        // `playSound(WARDEN_TENDRIL_CLICKS, 5.0, getVoicePitch())`：两次实体随机数。
        // `playSound(WARDEN_TENDRIL_CLICKS, 5.0, getVoicePitch())`: two entity draws.
        let pitch = voice_pitch(self.me);
        self.mob.events.push(MobEvent::Sound {
            sound: "minecraft:entity.warden.tendril_clicks",
            volume: 5.0,
            pitch,
            position: self.me.position,
            hostile: true,
            seed: None,
        });
        let recent_projectile = self
            .mob
            .brain
            .as_ref()
            .is_some_and(|brain| brain.memories.has(M::RecentProjectile));
        if let Some(owner) = projectile_owner {
            // 官方：射弹拥有者非 null 时，先看 30 格内；已经有过射弹就按拥有者位置
            // 加 35 点愤怒，否则加 10 点（带监听声）。RECENT_PROJECTILE 两支都会刷新。
            // Vanilla: with a projectile owner, check the 30-block radius; an already
            // recent projectile adds 35 anger at the owner's position, otherwise add
            // 10 with the listening sound. Both branches refresh RECENT_PROJECTILE.
            if self.closer_than(owner, 30.0) {
                if recent_projectile {
                    if self.can_target(owner)
                        && let Some(pos) = self.entity_position(owner)
                    {
                        target_pos = BlockPos::containing(pos);
                    }
                    // 一参的 `increaseAngerAt(entity)` 也会播监听声（35 点）。
                    // The one-argument `increaseAngerAt(entity)` also plays the
                    // listening sound (35 anger).
                    self.increase_anger(owner, 35, true);
                } else {
                    self.increase_anger(owner, 10, true);
                }
            }
            if let Some(brain) = self.mob.brain.as_mut() {
                brain
                    .memories
                    .set_with_ttl(M::RecentProjectile, MemoryValue::Unit, 100);
            }
        } else if let Some(uid) = source_entity.or(source_player) {
            self.increase_anger(uid, 35, true);
        }

        // 非 ANGRY 且 (无射弹拥有者 / 没有活动嫌疑 / 活动嫌疑不是源实体) 时设调查点。
        let angry = self.mob.warden.anger.anger_level().is_angry();
        let active = angry
            .then(|| self.mob.warden.anger.get_active_entity())
            .flatten();
        let allow = projectile_owner.is_none() || active.is_none() || active != source_entity;
        if !angry && allow {
            set_disturbance(self.mob, target_pos);
        }
    }

    fn requires_adjacent_chunks(&self) -> bool {
        false
    }

    fn send_particles(&mut self, _from: Vec3, _to: Vec3, _travel: i32) -> usize {
        0
    }
}
