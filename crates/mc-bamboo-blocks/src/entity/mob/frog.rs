//! 青蛙：官方 `Frog`、`FrogAi` 和它的专属传感器 / 行为。第三种跑大脑的生物，六个活动
//! （core / idle / swim / lay_spawn / tongue / long_jump）和五个传感器。
//!
//! 核心、闲逛、游泳三个活动里的行为逐条照官方类；长跳、产卵、卷舌三个活动也照官方的
//! 优先级、进入条件和 `getRequiredMemories()` 建好（活动条件记忆靠传感器 / 行为登记，
//! 见 `brain/mod.rs` 的疑点 5 单测），但它们的内部物理（长跳向量、产卵方块、卷舌攻击）
//! 留到接线那一轮逐刻对拍后再补，本轮先把框架铺全，对既有生物完全惰性。
//!
//! Frogs: vanilla `Frog`, `FrogAi` and its own sensors / behaviours. The third
//! brain-driven mob, with six activities (core / idle / swim / lay_spawn / tongue /
//! long_jump) and five sensors.
//!
//! The core, idle and swim behaviours follow their vanilla classes; the long-jump,
//! lay-spawn and tongue activities are also built with their vanilla priorities, entry
//! conditions and `getRequiredMemories()` (activity-condition memories are registered
//! by sensors / behaviours — see the doubt-5 test in `brain/mod.rs`), but their internal
//! physics (jump vectors, frogspawn block, tongue attack) waits for the wiring round's
//! tick-by-tick oracle. This round lays out the full framework, inert to existing mobs.

use mc_bamboo_core::mth;
use mc_bamboo_data::registries::attribute;

use super::{
    Mob, MobType,
    attributes::Attributes,
    brain::{
        Activity, ActivityData, Brain, Cx,
        behavior::{Class, Gate, Kind, MoveToTargetSink},
        memory::{
            MemoryStatus::{self, Registered, ValueAbsent, ValuePresent},
            MemoryType::{self, *},
            MemoryValue, WalkTarget,
        },
        sensor::{self, SensorKind},
    },
    goals, living,
};
use crate::entity::{Entity, EntityUid, ctx::EntityCtx};

/// `EntityTypes.FROG`：0.5×0.5，眼高 0.45；幼年不缩放（`canBeABaby` 为假，青蛙无幼体）。
///
/// `EntityTypes.FROG`: 0.5 by 0.5, eye 0.45; no baby scaling (`canBeABaby` is false).
pub const fn dimensions(_baby: bool) -> (f32, f32, f32) {
    (0.5, 0.5, 0.45)
}

/// `Frog.createAttributes`：动物那一套加速度 1.0、生命 10、攻击力 10、台阶高 1.0。
///
/// `Frog.createAttributes`: the animal set with 1.0 speed, 10 health, 10 attack damage
/// and 1.0 step height.
pub fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(attribute::MOVEMENT_SPEED, 1.0);
    table.set_base(attribute::MAX_HEALTH, 10.0);
    table
}

/// `FrogVariant`：`temperate` / `warm` / `cold`，按注册 ID。
///
/// `FrogVariant`: `temperate` / `warm` / `cold`, by registry id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variant {
    #[default]
    Temperate,
    Warm,
    Cold,
}

impl Variant {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Temperate => "temperate",
            Self::Warm => "warm",
            Self::Cold => "cold",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Temperate, Self::Warm, Self::Cold]
            .into_iter()
            .find(|variant| variant.name() == name)
    }
}

/// `Pose`：青蛙蹲叫、长跳、伸舌头时切姿势（`onSyncedDataUpdated` 只影响客户端动画，
/// 服务端这边只用来判 `Croak` 的入口和 `ShootTongue` 的清理）。
///
/// `Pose`: the frog's croaking / long-jump / tongue poses (client animations only on the
/// vanilla side; here they gate `Croak` and `ShootTongue`'s cleanup).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pose {
    #[default]
    Standing,
    Croaking,
    LongJumping,
    UsingTongue,
}

/// 青蛙自己的字段：品种、姿势、伸舌头的目标。
///
/// The frog's own fields: variant, pose and tongue target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Frog {
    pub variant: Variant,
    pub pose: Pose,
    pub tongue_target: Option<EntityUid>,
}

/// 青蛙专属行为。声明式 `OneShot` 的 `trigger` 返回是否命中；`Timed` 的行为照官方
/// `Behavior` 的时长和条件；三个活动里的复杂物理先留成官方形状的占位（见模块文档）。
///
/// The frog-only behaviours. Declarative `OneShot`s answer through `trigger`; the timed
/// ones follow vanilla durations and conditions; the complex physics in three activities
/// stays as vanilla-shaped placeholders (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// `Croak`（时长 100）。
    Croak { counter: i32 },
    /// `TryFindLand.create(range, speed)`，声明式 `OneShot`。
    TryFindLand {
        range: i32,
        speed: f32,
        next_ok: i64,
    },
    /// `StartAttacking.create(...)`，声明式 `OneShot`。
    StartAttacking,
    /// `StopAttackingIfTargetInvalid.create()`，声明式 `OneShot`。
    StopAttackingIfInvalid,
    /// `TryFindLandNearLiquid.create(8, 1.0F, ...)`，声明式 `OneShot`。
    TryFindLandNearLiquid,
    /// `TryLaySpawnOnFluidNearLand.create(FROGSPAWN)`，声明式 `OneShot`。
    TryLaySpawn,
    /// `LongJumpMidJump(TIME_BETWEEN_LONG_JUMPS, FROG_STEP)`（时长 100）。
    MidJump,
    /// `LongJumpToPreferredBlock(...)`（时长 200）。
    LongJumpPreferred,
    /// `ShootTongue(FROG_TONGUE, FROG_EAT)`（时长 100）。
    ShootTongue { timer: i32, path_counter: i32 },
    /// `RandomStroll.swim(0.75F)`，声明式 `OneShot`。
    SwimStroll { speed: f32 },
    /// `RandomStroll.stroll(1.0F, true)`，声明式 `OneShot`，和默认漫步同形。
    StrollFromWater { speed: f32 },
}

const CROAK_REQUIRED: &[(MemoryType, MemoryStatus)] = &[(WalkTarget, ValueAbsent)];
const TRY_FIND_LAND_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (AttackTarget, ValueAbsent),
    (WalkTarget, ValueAbsent),
    (LookTarget, Registered),
];
const START_ATTACKING_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (AttackTarget, ValueAbsent),
    (CantReachWalkTargetSince, Registered),
];
const STOP_ATTACKING_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (AttackTarget, ValuePresent),
    (CantReachWalkTargetSince, Registered),
];
const MID_JUMP_REQUIRED: &[(MemoryType, MemoryStatus)] =
    &[(LookTarget, Registered), (LongJumpMidJump, ValuePresent)];
const LONG_JUMP_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (LookTarget, Registered),
    (LongJumpCooldownTicks, ValueAbsent),
    (LongJumpMidJump, ValueAbsent),
];
const SHOOT_TONGUE_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (WalkTarget, ValueAbsent),
    (LookTarget, Registered),
    (AttackTarget, ValuePresent),
    (IsPanicking, ValueAbsent),
];
// `TryLaySpawnOnFluidNearLand.create`: 这条也负责登记 `IS_PREGNANT`（活动条件记忆）。
// `TryLaySpawnOnFluidNearLand.create`: this behaviour also registers `IS_PREGNANT`
// (an activity-condition memory).
const TRY_LAY_SPAWN_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (AttackTarget, ValueAbsent),
    (WalkTarget, ValuePresent),
    (IsPregnant, ValuePresent),
];
const STROLL_REQUIRED: &[(MemoryType, MemoryStatus)] = &[(WalkTarget, ValueAbsent)];

/// 长跳间隔 `UniformInt.of(100, 140)`，官方 `FrogAi.initMemories` / `LongJumpMidJump.stop`
/// 都用它从世界随机源取样。
///
/// `UniformInt.of(100, 140)`, sampled from the level random by `FrogAi.initMemories` and
/// `LongJumpMidJump.stop`.
pub const LONG_JUMP_INTERVAL: (i32, i32) = (100, 140);

impl Behavior {
    pub fn class(&self) -> Class {
        match self {
            Self::Croak { .. } => Class::Timed { min: 100, max: 100 },
            Self::MidJump => Class::Timed { min: 100, max: 100 },
            Self::LongJumpPreferred | Self::ShootTongue { .. } => {
                Class::Timed { min: 200, max: 200 }
            }
            Self::TryFindLand { .. }
            | Self::StartAttacking
            | Self::StopAttackingIfInvalid
            | Self::TryFindLandNearLiquid
            | Self::TryLaySpawn
            | Self::SwimStroll { .. }
            | Self::StrollFromWater { .. } => Class::OneShot,
        }
    }

    pub fn required(&self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::Croak { .. } => CROAK_REQUIRED,
            Self::TryFindLand { .. } | Self::TryFindLandNearLiquid => TRY_FIND_LAND_REQUIRED,
            Self::StartAttacking => START_ATTACKING_REQUIRED,
            Self::StopAttackingIfInvalid => STOP_ATTACKING_REQUIRED,
            Self::TryLaySpawn => TRY_LAY_SPAWN_REQUIRED,
            Self::MidJump => MID_JUMP_REQUIRED,
            Self::LongJumpPreferred => LONG_JUMP_REQUIRED,
            Self::ShootTongue { .. } => SHOOT_TONGUE_REQUIRED,
            Self::SwimStroll { .. } | Self::StrollFromWater { .. } => STROLL_REQUIRED,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Croak { .. } => "Croak",
            Self::TryFindLand { .. }
            | Self::TryFindLandNearLiquid
            | Self::TryLaySpawn
            | Self::StartAttacking
            | Self::StopAttackingIfInvalid
            | Self::SwimStroll { .. }
            | Self::StrollFromWater { .. } => "OneShot",
            Self::MidJump => "LongJumpMidJump",
            Self::LongJumpPreferred => "LongJumpToPreferredBlock",
            Self::ShootTongue { .. } => "ShootTongue",
        }
    }

    pub fn extra_start(&mut self, cx: &mut Cx) -> bool {
        match self {
            // `Croak.checkExtraStartConditions`: 站着且不在液体里。
            // `Croak.checkExtraStartConditions`: standing and not in a liquid.
            Self::Croak { .. } => cx.mob.frog.pose == Pose::Standing && !cx.me.in_water,
            _ => true,
        }
    }

    pub fn start(&mut self, cx: &mut Cx) {
        match self {
            // `Croak.start`: 不在液体里就切 CROAKING，计数器清零。
            // `Croak.start`: switch to CROAKING unless in a liquid, reset the counter.
            Self::Croak { counter } => {
                if !cx.me.in_water {
                    cx.mob.frog.pose = Pose::Croaking;
                    *counter = 0;
                }
            }
            // `LongJumpMidJump.start`: 关摩擦、切 LONG_JUMPING。
            // `LongJumpMidJump.start`: discard friction, switch to LONG_JUMPING.
            Self::MidJump => cx.mob.frog.pose = Pose::LongJumping,
            Self::ShootTongue {
                timer,
                path_counter,
            } => {
                *timer = 0;
                *path_counter = 10;
                if let Some(target) = cx.memories.entity(AttackTarget) {
                    cx.mob.frog.tongue_target = Some(target);
                }
            }
            _ => {}
        }
    }

    pub fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        match self {
            // `Croak.canStillUse`: 计数器不到 60。
            // `Croak.canStillUse`: the counter is below 60.
            Self::Croak { counter } => *counter < 60,
            // `LongJumpMidJump.canStillUse`: 还没落地。
            // `LongJumpMidJump.canStillUse`: not on the ground yet.
            Self::MidJump => !cx.me.on_ground,
            Self::ShootTongue { .. } => {
                cx.memories.has(AttackTarget) && !cx.memories.has(IsPanicking)
            }
            _ => false,
        }
    }

    pub fn tick(&mut self, _cx: &mut Cx) {
        match self {
            // `Croak.tick`: 计数器加一。
            // `Croak.tick`: count up.
            Self::Croak { counter } => *counter += 1,
            Self::ShootTongue { timer, .. } => *timer += 1,
            _ => {}
        }
    }

    pub fn stop(&mut self, cx: &mut Cx) {
        match self {
            // `Croak.stop`: 回到 STANDING。
            // `Croak.stop`: back to STANDING.
            Self::Croak { .. } => cx.mob.frog.pose = Pose::Standing,
            // `LongJumpMidJump.stop`: 落地时设长跳冷却（世界随机源），清 `LONG_JUMP_MID_JUMP`。
            // `LongJumpMidJump.stop`: on landing set the long-jump cooldown (level random)
            // and clear `LONG_JUMP_MID_JUMP`.
            Self::MidJump => {
                cx.mob.frog.pose = Pose::Standing;
                cx.memories.erase(LongJumpMidJump);
                let (min, max) = LONG_JUMP_INTERVAL;
                let cooldown = cx.level_next_int(max - min + 1) + min;
                cx.memories
                    .set(LongJumpCooldownTicks, MemoryValue::Int(cooldown));
            }
            Self::ShootTongue { .. } => {
                cx.memories.erase(AttackTarget);
                cx.mob.frog.tongue_target = None;
                cx.mob.frog.pose = Pose::Standing;
            }
            _ => {}
        }
    }

    /// 声明式 `OneShot.trigger` 里记忆条件之后的那段 lambda。
    ///
    /// The `OneShot.trigger` lambda that runs after the memory conditions.
    pub fn trigger(&mut self, cx: &mut Cx) -> bool {
        match self {
            // `TryFindLand`: 只有身体那格是水才找陆地；本轮无水场景恒不命中。
            // `TryFindLand`: only looks for land when the body block is water; no water in
            // this round's scenes, so it never fires here.
            Self::TryFindLand { next_ok, .. } => {
                if !cx.me.in_water {
                    return false;
                }
                if cx.time < *next_ok {
                    *next_ok = cx.time + 60;
                    return true;
                }
                *next_ok = cx.time + 60;
                true
            }
            // `StartAttacking`: 目标来自 `NEAREST_ATTACKABLE`，没有就不启动。
            // `StartAttacking`: the target comes from `NEAREST_ATTACKABLE`; absent means no start.
            Self::StartAttacking => {
                let Some(target) = cx.memories.entity(NearestAttackable) else {
                    return false;
                };
                cx.memories.set(AttackTarget, MemoryValue::Entity(target));
                cx.memories.erase(CantReachWalkTargetSince);
                true
            }
            // `StopAttackingIfTargetInvalid.create()`：目标还能打就留着，否则清掉。
            // `StopAttackingIfTargetInvalid.create()`: keep a still-attackable target, else erase.
            Self::StopAttackingIfInvalid => {
                if let Some(target) = cx.memories.entity(AttackTarget)
                    && sensor::living(cx, target).is_some_and(|view| view.alive)
                {
                    return true;
                }
                cx.memories.erase(AttackTarget);
                true
            }
            // `RandomStroll.swim(0.75F)`：在水里才找游泳落点，本轮无游泳场景不取随机。
            // `RandomStroll.swim(0.75F)`: finds a swim target only in water; no swim scenes
            // this round, so no random draw.
            Self::SwimStroll { speed } => {
                if !cx.me.in_water {
                    return false;
                }
                let pos = goals::land_random_pos(cx.me, cx.mob, cx.ctx.blocks(), 10, 7);
                cx.memories.set_or_erase(
                    WalkTarget,
                    pos.map(|pos| MemoryValue::Walk(WalkTarget::to_point(pos, *speed, 0))),
                );
                true
            }
            // `RandomStroll.stroll(1.0F, true)`：和默认漫步同形。
            // `RandomStroll.stroll(1.0F, true)`: same shape as the default stroll.
            Self::StrollFromWater { speed } => {
                let pos = goals::land_random_pos(cx.me, cx.mob, cx.ctx.blocks(), 10, 7);
                cx.memories.set_or_erase(
                    WalkTarget,
                    pos.map(|pos| MemoryValue::Walk(WalkTarget::to_point(pos, *speed, 0))),
                );
                true
            }
            // 产卵两个声明式行为本轮不命中（没有怀孕记忆的场景）。
            // The two declarative spawn behaviours never fire this round (no pregnant scenes).
            Self::TryFindLandNearLiquid | Self::TryLaySpawn => false,
            _ => false,
        }
    }
}

/// `Frog.BRAIN_PROVIDER`：五个传感器，顺序照 `Frog.java:73`。`self_type` 是这只青蛙的
/// `MobType`（接线那一轮传 `MobType::Frog`），给 `AnimalMakeLove` 当配偶种类。
///
/// `Frog.BRAIN_PROVIDER`: five sensors in the order of `Frog.java:73`. `self_type` is the
/// frog's `MobType` (the wiring round passes `MobType::Frog`), used by `AnimalMakeLove`
/// as the partner type.
pub fn brain(self_type: MobType) -> Brain {
    Brain::new(
        &[
            SensorKind::NearestLivingEntities,
            SensorKind::HurtBy,
            SensorKind::FrogAttackables,
            SensorKind::FrogTemptations,
            SensorKind::IsInWater,
        ],
        vec![
            core_activity(),
            idle_activity(self_type),
            swim_activity(),
            lay_spawn_activity(),
            tongue_activity(),
            long_jump_activity(),
        ],
    )
}

/// `FrogAi.initCoreActivity`（优先级 0）。
///
/// `FrogAi.initCoreActivity` (priority 0).
fn core_activity() -> ActivityData {
    ActivityData::sequential(
        Activity::Core,
        0,
        vec![
            Kind::AnimalPanic {
                speed: 2.0,
                environmental: false,
                armadillo: false,
            },
            Kind::LookAtTargetSink { min: 45, max: 90 },
            Kind::MoveToTargetSink(MoveToTargetSink::new(false)),
            Kind::CountDownCooldown(TemptationCooldownTicks),
            Kind::CountDownCooldown(LongJumpCooldownTicks),
        ],
    )
}

/// `FrogAi.initIdleActivity`。
///
/// `FrogAi.initIdleActivity`.
fn idle_activity(self_type: MobType) -> ActivityData {
    ActivityData::with_priorities(
        Activity::Idle,
        vec![
            (
                0,
                Kind::LookAtPlayerSometimes {
                    max_dist: 6.0,
                    min_interval: 30,
                    max_interval: 60,
                    ticks_until_next_start: 0,
                },
            ),
            (
                0,
                Kind::MakeLove {
                    partner: self_type,
                    speed: 1.0,
                    close_enough: 2,
                    spawn_child_at: 0,
                },
            ),
            (
                1,
                Kind::FollowTemptation {
                    speed: 1.25,
                    close_adult: 2.5,
                    close_baby: 2.5,
                },
            ),
            (2, Kind::Frog(Behavior::StartAttacking)),
            (
                3,
                Kind::Frog(Behavior::TryFindLand {
                    range: 6,
                    speed: 1.0,
                    next_ok: 0,
                }),
            ),
            (
                4,
                Kind::Gate(Box::new(Gate::run_one(
                    vec![(WalkTarget, ValueAbsent)],
                    vec![
                        (Kind::RandomStroll { speed: 1.0 }, 1),
                        (
                            Kind::WalkTargetFromLook {
                                speed: 1.0,
                                close_enough: 3,
                            },
                            1,
                        ),
                        (Kind::Frog(Behavior::Croak { counter: 0 }), 3),
                        (
                            Kind::TriggerIf(super::brain::behavior::Trigger::OnGround),
                            2,
                        ),
                    ],
                ))),
            ),
        ],
    )
    .when(vec![
        (LongJumpMidJump, ValueAbsent),
        (IsInWater, ValueAbsent),
    ])
}

/// `FrogAi.initSwimActivity`（`GateBehavior ORDERED + TRY_ALL`）。
///
/// `FrogAi.initSwimActivity` (`GateBehavior ORDERED + TRY_ALL`).
fn swim_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Swim,
        vec![
            (
                0,
                Kind::LookAtPlayerSometimes {
                    max_dist: 6.0,
                    min_interval: 30,
                    max_interval: 60,
                    ticks_until_next_start: 0,
                },
            ),
            (
                1,
                Kind::FollowTemptation {
                    speed: 1.25,
                    close_adult: 2.5,
                    close_baby: 2.5,
                },
            ),
            (2, Kind::Frog(Behavior::StartAttacking)),
            (
                3,
                Kind::Frog(Behavior::TryFindLand {
                    range: 8,
                    speed: 1.5,
                    next_ok: 0,
                }),
            ),
            (
                5,
                Kind::Gate(Box::new(Gate::ordered_try_all(
                    vec![(WalkTarget, ValueAbsent)],
                    vec![
                        (Kind::Frog(Behavior::SwimStroll { speed: 0.75 }), 1),
                        (Kind::Frog(Behavior::StrollFromWater { speed: 1.0 }), 1),
                        (
                            Kind::WalkTargetFromLook {
                                speed: 1.0,
                                close_enough: 3,
                            },
                            1,
                        ),
                        (Kind::TriggerIf(super::brain::behavior::Trigger::InWater), 5),
                    ],
                ))),
            ),
        ],
    )
    .when(vec![
        (LongJumpMidJump, ValueAbsent),
        (IsInWater, ValuePresent),
    ])
}

/// `FrogAi.initLaySpawnActivity`。
///
/// `FrogAi.initLaySpawnActivity`.
fn lay_spawn_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::LaySpawn,
        vec![
            (
                0,
                Kind::LookAtPlayerSometimes {
                    max_dist: 6.0,
                    min_interval: 30,
                    max_interval: 60,
                    ticks_until_next_start: 0,
                },
            ),
            (1, Kind::Frog(Behavior::StartAttacking)),
            (2, Kind::Frog(Behavior::TryFindLandNearLiquid)),
            (3, Kind::Frog(Behavior::TryLaySpawn)),
            (
                4,
                Kind::Gate(Box::new(Gate::run_one(
                    Vec::new(),
                    vec![
                        (Kind::RandomStroll { speed: 1.0 }, 2),
                        (
                            Kind::WalkTargetFromLook {
                                speed: 1.0,
                                close_enough: 3,
                            },
                            1,
                        ),
                        (Kind::Frog(Behavior::Croak { counter: 0 }), 2),
                        (
                            Kind::TriggerIf(super::brain::behavior::Trigger::OnGround),
                            1,
                        ),
                    ],
                ))),
            ),
        ],
    )
    .when(vec![
        (LongJumpMidJump, ValueAbsent),
        (IsPregnant, ValuePresent),
    ])
}

/// `FrogAi.initJumpActivity`。
///
/// `FrogAi.initJumpActivity`.
fn long_jump_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::LongJump,
        vec![
            (0, Kind::Frog(Behavior::MidJump)),
            (1, Kind::Frog(Behavior::LongJumpPreferred)),
        ],
    )
    .when(vec![
        (TemptingPlayer, ValueAbsent),
        (BreedTarget, ValueAbsent),
        (LongJumpCooldownTicks, ValueAbsent),
        (IsInWater, ValueAbsent),
    ])
}

/// `FrogAi.initTongueActivity`（进入条件就是 `ATTACK_TARGET` 有值）。
///
/// `FrogAi.initTongueActivity` (its entry condition is simply a present `ATTACK_TARGET`).
fn tongue_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Tongue,
        vec![
            (0, Kind::Frog(Behavior::StopAttackingIfInvalid)),
            (
                1,
                Kind::Frog(Behavior::ShootTongue {
                    timer: 0,
                    path_counter: 10,
                }),
            ),
        ],
    )
    .when(vec![(AttackTarget, ValuePresent)])
}

/// `FrogAi.initMemories`：`finalizeSpawn` / `getBreedOffspring` 用世界随机源抽长跳冷却。
///
/// `FrogAi.initMemories`: `finalizeSpawn` / `getBreedOffspring` draw the long-jump
/// cooldown from the level random.
pub fn init_memories(mob: &mut Mob, next_int: &mut dyn FnMut(i32) -> i32) {
    let Some(brain) = mob.brain.as_mut() else {
        return;
    };
    let (min, max) = LONG_JUMP_INTERVAL;
    let cooldown = next_int(max - min + 1) + min;
    brain
        .memories
        .set(LongJumpCooldownTicks, MemoryValue::Int(cooldown));
}

/// `FrogAi.updateActivity`：活动按 `TONGUE, LAY_SPAWN, LONG_JUMP, SWIM, IDLE` 挑第一个
/// 满足的（`Frog.java:233`）。
///
/// `FrogAi.updateActivity`: pick the first valid of `TONGUE, LAY_SPAWN, LONG_JUMP, SWIM,
/// IDLE` (`Frog.java:233`).
pub fn update_activity(brain: &mut Brain) {
    brain.set_active_activity_to_first_valid(&[
        Activity::Tongue,
        Activity::LaySpawn,
        Activity::LongJump,
        Activity::Swim,
        Activity::Idle,
    ]);
}

/// `FrogAttackablesSensor.doTick`：看得见、能吃、不在「够不到」名单里、10 格内的最近活体。
/// 没有 `#frog_food` 猎物时清掉 `NEAREST_ATTACKABLE`（本轮场景即此）。
///
/// `FrogAttackablesSensor.doTick`: the nearest visible, edible, reachable living entity
/// within 10 blocks. With no `#frog_food` prey it clears `NEAREST_ATTACKABLE` (this
/// round's scenes).
pub(super) fn sense_attackables(cx: &mut Cx) {
    let position = cx.me.position;
    let found = sensor::find_closest_visible(cx, &mut |view| {
        position.distance_to_sqr(view.position) < 100.0
            && view
                .mob_type
                .is_some_and(|kind| kind.in_entity_tag("minecraft:frog_food"))
    });
    cx.memories
        .set_or_erase(NearestAttackable, found.map(MemoryValue::Entity));
}

/// `IsInWaterSensor.doTick`：在水里设 `Unit`，否则清掉。
///
/// `IsInWaterSensor.doTick`: set `Unit` while in water, else erase.
pub(super) fn sense_is_in_water(cx: &mut Cx) {
    if cx.me.in_water {
        cx.memories.set(IsInWater, MemoryValue::Unit);
    } else {
        cx.memories.erase(IsInWater);
    }
}

/// 占位：`Frog.customServerAiStep` 里 `getBrain().tick` 之后的钩子，接线那一轮把它接进
/// `living::server_ai_step`（照 `armadillo::custom_server_ai_step`）。
///
/// Placeholder: the `getBrain().tick` hook of `Frog.customServerAiStep`; the wiring round
/// connects it into `living::server_ai_step` (like `armadillo::custom_server_ai_step`).
pub fn custom_server_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if let Some(mut brain) = mob.brain.take() {
        brain.tick(me, mob, ctx);
        update_activity(&mut brain);
        mob.brain = Some(brain);
    }
}

/// 占位：`SmoothSwimmingMoveControl.tick` 在陆地上的那支（速度乘 `outsideWaterSpeedModifier`
/// 0.1 与转向系数），接线那一轮从 `living::move_control_tick` 分派过来。
///
/// Placeholder: `SmoothSwimmingMoveControl.tick`'s land branch (speed times the 0.1
/// `outsideWaterSpeedModifier` and the turning factor); the wiring round dispatches it
/// from `living::move_control_tick`.
pub fn move_control_tick(me: &mut Entity, mob: &mut Mob) {
    if mob.mover.operation != super::control::Operation::MoveTo || mob.navigation.is_done() {
        living::set_speed(mob, 0.0);
        mob.xxa = 0.0;
        mob.yya = 0.0;
        return;
    }
    mob.mover.operation = super::control::Operation::Wait;
    let dx = mob.mover.wanted_x - me.position.x;
    let dz = mob.mover.wanted_z - me.position.z;
    let distance = dx * dx + dz * dz;
    if distance < f64::from(2.500_000_3E-7_f32) {
        mob.zza = 0.0;
        return;
    }
    let yaw = (mth::atan2(dz, dx) * 57.295_776_367_187_5) as f32 - 90.0;
    let turned = super::control::move_rotlerp(me.y_rot, yaw, 10.0);
    me.y_rot = turned;
    mob.y_body_rot = turned;
    mob.y_head_rot = turned;
    let left_to_turn = mth::wrap_degrees(me.y_rot - yaw).abs();
    let turning = 1.0 - mth::clamp_f32((left_to_turn - 10.0) / 50.0, 0.0, 1.0);
    let speed = (mob.mover.speed_modifier * mob.attributes.value(attribute::MOVEMENT_SPEED)) as f32
        * 0.1
        * turning;
    living::set_speed(mob, speed);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 六个活动、五个传感器、核心 / 闲逛 / 游泳的优先级和进入条件照官方 `FrogAi`。
    ///
    /// Six activities, five sensors, and the core / idle / swim priorities and entry
    /// conditions follow vanilla `FrogAi`.
    #[test]
    fn frog_brain_has_vanilla_activities_and_sensors() {
        let brain = brain(MobType::Goat);
        assert_eq!(brain.sensors.len(), 5);
        assert_eq!(brain.sensors[0].kind, SensorKind::NearestLivingEntities);
        assert_eq!(brain.sensors[1].kind, SensorKind::HurtBy);
        assert_eq!(brain.sensors[2].kind, SensorKind::FrogAttackables);
        assert_eq!(brain.sensors[3].kind, SensorKind::FrogTemptations);
        assert_eq!(brain.sensors[4].kind, SensorKind::IsInWater);
        // 初始活动 core + idle。
        // The initial activity is core + idle.
        assert_eq!(brain.active_names(), vec!["core", "idle"]);
        // 六个活动都在行为表里。
        // All six activities appear in the behaviour table.
        let mut activities = brain.table_activities();
        activities.sort_by_key(|activity| activity.name());
        activities.dedup();
        assert_eq!(activities.len(), 6);
    }

    /// 活动条件记忆都由某个传感器 / 行为登记（`Brain::new` 的登记路径），否则
    /// `setActiveActivityToFirstValid` 永远选不中对应活动。
    ///
    /// Every activity-condition memory is registered by some sensor / behaviour (the
    /// `Brain::new` registration path), otherwise `setActiveActivityToFirstValid` can
    /// never pick that activity.
    #[test]
    fn frog_activity_condition_memories_are_registered() {
        let brain = brain(MobType::Goat);
        for kind in [
            IsInWater,
            IsPregnant,
            LongJumpMidJump,
            LongJumpCooldownTicks,
            TemptingPlayer,
            BreedTarget,
            AttackTarget,
            NearestAttackable,
            UnreachableTongueTargets,
        ] {
            assert!(
                brain.memories.is_registered(kind),
                "{} should be registered",
                kind.name()
            );
        }
    }

    /// 默认（core+idle）时 `TONGUE / LAY_SPAWN / LONG_JUMP / SWIM` 都因条件不满足而
    /// 选不中，`updateActivity` 停在 idle；灌入长跳冷却后 LONG_JUMP 也不选。
    ///
    /// In the default core+idle state, `TONGUE / LAY_SPAWN / LONG_JUMP / SWIM` all fail
    /// their conditions, so `updateActivity` stays on idle; a present long-jump cooldown
    /// also keeps LONG_JUMP out.
    #[test]
    fn update_activity_stays_idle_on_land_with_cooldown() {
        let mut brain = brain(MobType::Goat);
        brain
            .memories
            .set(LongJumpCooldownTicks, MemoryValue::Int(1000));
        update_activity(&mut brain);
        assert_eq!(brain.active_names(), vec!["core", "idle"]);
        // `IS_IN_WATER` 有值时 idle 条件不成立、swim 条件成立。
        // With `IS_IN_WATER` present, idle's condition fails and swim's holds.
        brain.memories.set(IsInWater, MemoryValue::Unit);
        update_activity(&mut brain);
        assert_eq!(brain.active_names(), vec!["core", "swim"]);
    }

    /// `Croak` 是时长 100、需要 `WALK_TARGET` 空缺的定时行为；`TryFindLand`、
    /// `StartAttacking` 是声明式 `OneShot`。
    ///
    /// `Croak` is a 100-tick timed behaviour needing an absent `WALK_TARGET`;
    /// `TryFindLand` and `StartAttacking` are declarative `OneShot`s.
    #[test]
    fn frog_behaviour_classes_and_requirements_match_vanilla() {
        let croak = Behavior::Croak { counter: 0 };
        assert!(matches!(croak.class(), Class::Timed { min: 100, max: 100 }));
        assert_eq!(croak.required(), &[(WalkTarget, ValueAbsent)]);
        assert_eq!(croak.name(), "Croak");

        let start = Behavior::StartAttacking;
        assert!(matches!(start.class(), Class::OneShot));
        assert_eq!(start.name(), "OneShot");
        assert_eq!(start.required().len(), 2);

        let mid = Behavior::MidJump;
        assert_eq!(mid.name(), "LongJumpMidJump");
        assert_eq!(mid.required(), MID_JUMP_REQUIRED);

        let swim = Behavior::SwimStroll { speed: 0.75 };
        assert!(matches!(swim.class(), Class::OneShot));
        assert_eq!(swim.required(), STROLL_REQUIRED);
    }

    /// 品种名字和 `Unit.CODEC` 记忆的可存档口径。
    ///
    /// Variant names and the serialisable `Unit.CODEC` memory convention.
    #[test]
    fn frog_variants_and_memory_serialisation() {
        assert_eq!(Variant::from_name("warm"), Some(Variant::Warm));
        assert_eq!(Variant::from_name("cold"), Some(Variant::Cold));
        assert_eq!(Variant::Temperate.name(), "temperate");
        assert!(IsInWater.serializable());
        assert!(IsPregnant.serializable());
        assert!(!UnreachableTongueTargets.serializable());
        assert_eq!(LONG_JUMP_INTERVAL, (100, 140));
    }
}
