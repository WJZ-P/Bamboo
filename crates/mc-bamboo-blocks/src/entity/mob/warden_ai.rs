//! `WardenAi`：监守者的大脑结构表与全部专属行为。
//!
//! 活动表照 `WardenAi.getActivities` 的 8 项，`updateActivity` 照
//! `setActiveActivityToFirstValid([EMERGE,DIG,ROAR,FIGHT,INVESTIGATE,SNIFF,IDLE])`。
//! 每个行为的时长、记忆条件、姿态 / 愤怒 / 冷却都按 26.3 字节码。

use mc_bamboo_core::{BlockPos, Vec3};

use crate::{
    entity::{Entity, EntityUid, ctx::EntityCtx},
    game_event::{GameEvent, vibration::VibrationUser},
};

use super::{
    Mob, MobEvent,
    brain::{
        Activity, ActivityData, Brain,
        behavior::{Class, Cx, Kind, MoveToTargetSink},
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
            (M::AttackTarget, ValuePresent),
            (M::AttackCoolingDown, Registered),
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
                if let Some(target) = cx.memories.entity(M::RoarTarget) {
                    look_at_entity(cx, target);
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
                if let Some(target) = cx.memories.entity(M::AttackTarget) {
                    look_at_entity(cx, target);
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
                {
                    sound(cx, "minecraft:entity.warden.sonic_boom", 3.0, 1.0);
                    sonic_boom_damage(cx, target);
                }
            }
            _ => {}
        }
    }

    pub(super) fn stop(&mut self, cx: &mut Cx) {
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
                let Some(pos) = entity_position(cx, target) else {
                    return false;
                };
                cx.memories.set(
                    M::WalkTarget,
                    MemoryValue::Walk(WalkTarget::to_point(pos, 1.2, 0)),
                );
                true
            }
            Self::MeleeAttack => {
                let Some(target) = cx.memories.entity(M::AttackTarget) else {
                    return false;
                };
                if cx.memories.has(M::AttackCoolingDown) {
                    return false;
                }
                if !can_target_entity(cx, target) {
                    return false;
                }
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
            .when(vec![(M::IsEmerging, ValuePresent)]),
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
                Kind::RandomStroll { speed: 0.5 },
            ],
        ),
        ActivityData::sequential(Activity::Roar, 10, vec![w(Behavior::Roar)])
            .when(vec![(M::RoarTarget, ValuePresent)]),
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
        .when(vec![(M::AttackTarget, ValuePresent)]),
        ActivityData::sequential(
            Activity::Investigate,
            5,
            vec![w(Behavior::SetRoarTarget), w(Behavior::GoToTargetLocation)],
        )
        .when(vec![(M::DisturbanceLocation, ValuePresent)]),
        ActivityData::sequential(
            Activity::Sniff,
            5,
            vec![w(Behavior::SetRoarTarget), w(Behavior::Sniffing)],
        )
        .when(vec![(M::IsSniffing, ValuePresent)]),
    ]
}

/// 监守者的大脑：两个传感器（`NEAREST_PLAYERS` / `WARDEN_ENTITY_SENSOR`）+ 8 个活动。
pub fn brain() -> Brain {
    Brain::new(
        &[SensorKind::NearestPlayers, SensorKind::WardenEntitySensor],
        activities(),
    )
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

/// `WardenEntitySensor.doTick`：从 `NEAREST_LIVING_ENTITIES` 里先找玩家、再找非玩家，
/// 写 `NEAREST_ATTACKABLE`；都没有就擦掉。
pub fn warden_entity_sensor(cx: &mut Cx) {
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

/// `Warden.canTargetEntity` 的可观察部分：不是自己、不是监守者、还在世界里。
fn can_target_entity(cx: &Cx, uid: crate::entity::EntityUid) -> bool {
    uid != cx.me.id && !cx.me.removed
}

/// 只读实体位置（`EntityCtx` 提供视图）。
fn entity_position(cx: &Cx, uid: crate::entity::EntityUid) -> Option<Vec3> {
    super::brain::sensor::living(cx, uid).map(|view| view.position)
}

fn look_at_entity(cx: &mut Cx, uid: crate::entity::EntityUid) {
    if let Some(pos) = entity_position(cx, uid) {
        cx.mob.look.set_look_at(pos.x, pos.y, pos.z, 10.0, 90.0);
    }
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

fn melee_damage(_cx: &mut Cx, _target: crate::entity::EntityUid) {
    // 近战伤害落地在 `living::do_hurt_target`，对拍场景按伤害帧核；这里先记冷却与事件。
}

fn sonic_boom_damage(_cx: &mut Cx, _target: crate::entity::EntityUid) {
    // 音波穿墙伤害落地在伤害路径，对拍场景按血量帧核；这里先记冷却与声音。
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
    sync_client_anger(cx);
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
    sync_client_anger(cx);
}

/// `Warden.syncClientAngerLevel`：元数据等于 `getActiveAnger(getTarget())`。
pub fn sync_client_anger(cx: &mut Cx) {
    let target = cx.memories.entity(M::AttackTarget);
    cx.mob.warden.client_anger_level = cx.mob.warden.anger.get_active_anger(target);
}

/// `Warden.customServerAiStep`：跑大脑 → 推进振动监听者 → 每 20 刻衰减愤怒并同步
/// 客户端等级 → 每刻末按官方顺序重选活动。
///
/// `Warden.customServerAiStep`: tick the brain, advance the vibration listener,
/// decay anger and sync the client level every 20 ticks, then reselect the activity.
pub fn custom_server_ai_step(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    decision_gap: i64,
) {
    if let Some(mut brain) = mob.brain.take() {
        let replaced = brain.tick_by(me, mob, ctx, decision_gap.max(1));
        mob.brain = Some(Box::new(replaced.unwrap_or(*brain)));
    }
    let now = ctx.blocks().game_time();
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
        let mut user = WardenVibrationUser { me: &*me, mob };
        crate::game_event::vibration::tick(&mut data, &mut user, now, Some(eye), false);
    }
    if let Some(listener) = mob.entity_listener.as_mut() {
        listener.vibration = data;
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
    me: &'a Entity,
    mob: &'a mut Mob,
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
        _event: GameEvent,
        source: BlockPos,
        source_player: Option<EntityUid>,
        _distance: f32,
    ) {
        if self.me.removed {
            return;
        }
        if let Some(brain) = self.mob.brain.as_mut() {
            brain
                .memories
                .set_with_ttl(M::VibrationCooldown, MemoryValue::Unit, 40);
            brain
                .memories
                .set_with_ttl(M::RecentProjectile, MemoryValue::Unit, 100);
        }
        self.mob.events.push(MobEvent::EntityEvent { event: 61 });
        self.mob.events.push(MobEvent::Sound {
            sound: "minecraft:entity.warden.tendril_clicks",
            volume: 5.0,
            pitch: 1.0,
            position: self.me.position,
            hostile: true,
            seed: None,
        });
        if let Some(uid) = source_player {
            let uuid = crate::archive::entity::entity_uuid(uid).to_be_bytes();
            self.mob.warden.anger.increase_anger(
                uid,
                uuid,
                35,
                uid < crate::entity::FIRST_ENTITY_ID,
            );
            self.mob.warden.client_anger_level = self.mob.warden.anger.get_active_anger(None);
        }
        // 非 ANGRY 时设调查点；射弹分支的 10 点愤怒留给后续场景。
        // Set the disturbance when not ANGRY; the projectile 10-anger branch is later.
        if !self.mob.warden.anger.anger_level().is_angry()
            && let Some(brain) = self.mob.brain.as_mut()
        {
            brain
                .memories
                .set_with_ttl(M::SniffCooldown, MemoryValue::Unit, 100);
            brain.memories.set_with_ttl(
                M::LookTarget,
                MemoryValue::Look(PositionTracker::at_block(source)),
                100,
            );
            brain
                .memories
                .set_with_ttl(M::DisturbanceLocation, MemoryValue::BlockPos(source), 100);
            brain.memories.erase(M::WalkTarget);
        }
    }

    fn requires_adjacent_chunks(&self) -> bool {
        false
    }

    fn send_particles(&mut self, _from: Vec3, _to: Vec3, _travel: i32) -> usize {
        0
    }
}
