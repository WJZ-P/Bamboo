//! 各个具体行为：动物通用的那一批（游泳、恐慌、看目标、走向目标、冷却倒数、偶尔看人、
//! 繁殖、跟随诱惑、幼崽跟大人、随机环顾、随机漫步、看哪走哪、发呆），以及把物种专属
//! 行为接进来的分派。逻辑逐条照官方类，随机数的来源和先后一样。
//!
//! The concrete behaviours: the shared animal set (swim, panic, look at target, move to
//! target, cooldown countdown, look at players sometimes, breeding, following a
//! temptation, babies following adults, looking around, strolling, walking to the look
//! target, doing nothing) plus the dispatch into species behaviours. Each follows its
//! vanilla class with the same random sources and order.

use mc_bamboo_core::{BlockPos, Vec3, mth};

use super::{
    super::{MobType, goals, living},
    behavior::{Class, Cx, Gate},
    memory::{
        MemoryStatus::{self, Registered, ValueAbsent, ValuePresent},
        MemoryType::{self, *},
        MemoryValue, PositionTracker, WalkTarget,
    },
    sensor,
};
use crate::entity::EntityUid;

/// `BehaviorBuilder.triggerIf(predicate)` 的实体谓词：青蛙用 `Entity::onGround` /
/// `Entity::isInWater`。声明式 `OneShot`，`getRequiredMemories()` 是空集、不取随机数，
/// 谓词成立就在同一刻启动又停下（`OneShot.triggerOrStop`）。匿名子类在对拍里记父类
/// `OneShot`，和其余声明式行为一致。
///
/// The entity predicate of `BehaviorBuilder.triggerIf(predicate)`: frogs use
/// `Entity::onGround` / `Entity::isInWater`. A declarative `OneShot` with an empty
/// `getRequiredMemories()` and no random draw; when the predicate holds it starts and
/// stops within the same tick. Its anonymous subclass dumps as the parent `OneShot` in
/// the oracle, like the other declarative behaviours.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Trigger {
    OnGround,
    InWater,
}

impl Trigger {
    fn matches(self, on_ground: bool, in_water: bool) -> bool {
        match self {
            Self::OnGround => on_ground,
            Self::InWater => in_water,
        }
    }
}

/// 一个具体行为和它自己的字段。
///
/// One concrete behaviour and its own fields.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// `Swim(chance)`。
    Swim { chance: f32 },
    /// `AnimalPanic`；`environmental` 为真时只认 `#panic_environmental_causes`
    /// （犰狳），`armadillo` 为真时启动前先 `rollOut`。
    ///
    /// `AnimalPanic`; `environmental` restricts it to `#panic_environmental_causes`
    /// (armadillos), `armadillo` rolls out before starting.
    AnimalPanic {
        speed: f32,
        environmental: bool,
        armadillo: bool,
    },
    /// `LookAtTargetSink(min, max)`。
    LookAtTargetSink { min: i32, max: i32 },
    /// `MoveToTargetSink`；`armadillo` 是 `ArmadilloAi` 里那个受惊时不启动的匿名子类。
    ///
    /// `MoveToTargetSink`; `armadillo` is the anonymous subclass in `ArmadilloAi` that
    /// refuses to start while scared.
    MoveToTargetSink(MoveToTargetSink),
    /// `CountDownCooldownTicks(memory)`。
    CountDownCooldown(MemoryType),
    /// `SetEntityLookTargetSometimes.create(PLAYER, maxDist, interval)`。
    LookAtPlayerSometimes {
        max_dist: f32,
        min_interval: i32,
        max_interval: i32,
        ticks_until_next_start: i32,
    },
    /// `AnimalMakeLove(partnerType, speed, closeEnough)`。
    MakeLove {
        partner: MobType,
        speed: f32,
        close_enough: i32,
        spawn_child_at: i64,
    },
    /// `FollowTemptation(speed, closeEnough)`：`close_baby` / `close_adult` 是两种年龄的
    /// 停步距离（犰狳 1 / 2，其余 2.5 / 2.5）。
    ///
    /// `FollowTemptation(speed, closeEnough)`: `close_baby` / `close_adult` are the stop
    /// distances by age (armadillo 1 / 2, others 2.5 / 2.5).
    FollowTemptation {
        speed: f32,
        close_adult: f64,
        close_baby: f64,
    },
    /// `BabyFollowAdult.create(UniformInt(min, max), speed)`。
    BabyFollowAdult { min: i32, max: i32, speed: f32 },
    /// `RandomLookAround(UniformInt(min, max), maxYaw, minPitch, maxPitch)`。
    RandomLookAround {
        min_interval: i32,
        max_interval: i32,
        max_yaw: f32,
        min_pitch: f32,
        pitch_range: f32,
    },
    /// `RandomStroll.stroll(speed)`。
    RandomStroll { speed: f32 },
    /// `SetWalkTargetFromLookTarget.create(speed, closeEnough)`。
    WalkTargetFromLook { speed: f32, close_enough: i32 },
    /// `DoNothing(min, max)`。
    DoNothing { min: i32, max: i32 },
    /// `BehaviorBuilder.triggerIf(predicate)`：声明式一次性触发（青蛙的着地 / 在水里）。
    ///
    /// `BehaviorBuilder.triggerIf(predicate)`: a declarative one-shot trigger (the frog's
    /// on-ground / in-water predicates).
    TriggerIf(Trigger),
    /// `GateBehavior` / `RunOne`。
    Gate(Box<Gate>),
    /// 犰狳自己的行为。
    ///
    /// The armadillo's own behaviours.
    Armadillo(super::super::armadillo::Behavior),
    /// 山羊专属行为。
    ///
    /// Goat-only behaviours.
    Goat(super::super::goat::Behavior),
    /// 青蛙专属行为。
    ///
    /// Frog-only behaviours.
    Frog(super::super::frog::Behavior),
}

/// `MoveToTargetSink` 的字段。`path` 只记有没有：官方比的是对象身份，只影响要不要重写
/// `PATH` 记忆，而记忆的有无和导航的路径有无总是一致。
///
/// The `MoveToTargetSink` fields. `path` only records presence: vanilla compares object
/// identity, which only decides whether `PATH` is rewritten, and the memory's presence
/// always follows the navigation path's.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveToTargetSink {
    pub min: i32,
    pub max: i32,
    pub armadillo: bool,
    remaining_cooldown: i32,
    path: Option<super::super::path::Path>,
    last_target_pos: Option<BlockPos>,
    speed: f32,
}

impl MoveToTargetSink {
    /// `new MoveToTargetSink()`：150 到 250 刻。
    ///
    /// `new MoveToTargetSink()`: 150 to 250 ticks.
    pub fn new(armadillo: bool) -> Self {
        Self {
            min: 150,
            max: 250,
            armadillo,
            remaining_cooldown: 0,
            path: None,
            last_target_pos: None,
            speed: 0.0,
        }
    }
}

const PANIC_REQUIRED: &[(MemoryType, MemoryStatus)] =
    &[(IsPanicking, Registered), (HurtBy, Registered)];
const LOOK_SINK_REQUIRED: &[(MemoryType, MemoryStatus)] = &[(LookTarget, ValuePresent)];
const MOVE_SINK_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (CantReachWalkTargetSince, Registered),
    (Path, ValueAbsent),
    (WalkTarget, ValuePresent),
];
const TEMPTATION_COOLDOWN: &[(MemoryType, MemoryStatus)] =
    &[(TemptationCooldownTicks, ValuePresent)];
const GAZE_COOLDOWN: &[(MemoryType, MemoryStatus)] = &[(GazeCooldownTicks, ValuePresent)];
const LONG_JUMP_COOLDOWN: &[(MemoryType, MemoryStatus)] = &[(LongJumpCooldownTicks, ValuePresent)];
const RAM_COOLDOWN: &[(MemoryType, MemoryStatus)] = &[(RamCooldownTicks, ValuePresent)];
const LOOK_SOMETIMES_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (LookTarget, ValueAbsent),
    (NearestVisibleLivingEntities, ValuePresent),
];
const MAKE_LOVE_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (NearestVisibleLivingEntities, ValuePresent),
    (BreedTarget, ValueAbsent),
    (WalkTarget, Registered),
    (LookTarget, Registered),
    (IsPanicking, ValueAbsent),
];
const TEMPT_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (LookTarget, Registered),
    (WalkTarget, Registered),
    (TemptationCooldownTicks, ValueAbsent),
    (TemptingPlayer, ValuePresent),
    (BreedTarget, ValueAbsent),
    (IsPanicking, ValueAbsent),
];
const FOLLOW_ADULT_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (NearestVisibleAdult, ValuePresent),
    (LookTarget, Registered),
    (WalkTarget, ValueAbsent),
];
const LOOK_AROUND_REQUIRED: &[(MemoryType, MemoryStatus)] =
    &[(LookTarget, ValueAbsent), (GazeCooldownTicks, ValueAbsent)];
const STROLL_REQUIRED: &[(MemoryType, MemoryStatus)] = &[(WalkTarget, ValueAbsent)];
const WALK_FROM_LOOK_REQUIRED: &[(MemoryType, MemoryStatus)] =
    &[(WalkTarget, ValueAbsent), (LookTarget, ValuePresent)];

impl Kind {
    pub(super) fn class(&self) -> Class {
        match self {
            Self::Swim { .. } | Self::CountDownCooldown(_) | Self::FollowTemptation { .. } => {
                Class::Timed { min: 60, max: 60 }
            }
            Self::RandomLookAround { .. } => Class::Timed { min: 60, max: 60 },
            Self::AnimalPanic { .. } => Class::Timed { min: 100, max: 120 },
            Self::LookAtTargetSink { min, max } => Class::Timed {
                min: *min,
                max: *max,
            },
            Self::MoveToTargetSink(sink) => Class::Timed {
                min: sink.min,
                max: sink.max,
            },
            Self::MakeLove { .. } => Class::Timed { min: 110, max: 110 },
            Self::LookAtPlayerSometimes { .. }
            | Self::BabyFollowAdult { .. }
            | Self::RandomStroll { .. }
            | Self::WalkTargetFromLook { .. }
            | Self::TriggerIf(_) => Class::OneShot,
            Self::DoNothing { min, max } => Class::DoNothing {
                min: *min,
                max: *max,
            },
            Self::Gate(_) => Class::Gate,
            Self::Armadillo(behavior) => behavior.class(),
            Self::Goat(behavior) => behavior.class(),
            Self::Frog(behavior) => behavior.class(),
        }
    }

    /// `getRequiredMemories()` 加各自的入口状态。
    ///
    /// `getRequiredMemories()` with each entry status.
    pub(super) fn required(&self) -> &[(MemoryType, MemoryStatus)] {
        match self {
            Self::Swim { .. } | Self::DoNothing { .. } | Self::TriggerIf(_) => &[],
            Self::AnimalPanic { .. } => PANIC_REQUIRED,
            Self::LookAtTargetSink { .. } => LOOK_SINK_REQUIRED,
            Self::MoveToTargetSink(_) => MOVE_SINK_REQUIRED,
            Self::CountDownCooldown(memory) => match memory {
                TemptationCooldownTicks => TEMPTATION_COOLDOWN,
                GazeCooldownTicks => GAZE_COOLDOWN,
                LongJumpCooldownTicks => LONG_JUMP_COOLDOWN,
                _ => RAM_COOLDOWN,
            },
            Self::LookAtPlayerSometimes { .. } => LOOK_SOMETIMES_REQUIRED,
            Self::MakeLove { .. } => MAKE_LOVE_REQUIRED,
            Self::FollowTemptation { .. } => TEMPT_REQUIRED,
            Self::BabyFollowAdult { .. } => FOLLOW_ADULT_REQUIRED,
            Self::RandomLookAround { .. } => LOOK_AROUND_REQUIRED,
            Self::RandomStroll { .. } => STROLL_REQUIRED,
            Self::WalkTargetFromLook { .. } => WALK_FROM_LOOK_REQUIRED,
            Self::Gate(gate) => &gate.entry,
            Self::Armadillo(behavior) => behavior.required(),
            Self::Goat(behavior) => behavior.required(),
            Self::Frog(behavior) => behavior.required(),
        }
    }

    /// 对拍输出用的官方类名（匿名子类按父类记）。
    ///
    /// The vanilla class name for oracle dumps (anonymous subclasses use the parent).
    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Swim { .. } => "Swim",
            Self::AnimalPanic { armadillo, .. } => {
                if *armadillo {
                    "ArmadilloPanic"
                } else {
                    "AnimalPanic"
                }
            }
            Self::LookAtTargetSink { .. } => "LookAtTargetSink",
            Self::MoveToTargetSink(_) => "MoveToTargetSink",
            Self::CountDownCooldown(_) => "CountDownCooldownTicks",
            Self::LookAtPlayerSometimes { .. }
            | Self::BabyFollowAdult { .. }
            | Self::RandomStroll { .. }
            | Self::WalkTargetFromLook { .. }
            | Self::TriggerIf(_) => "OneShot",
            Self::MakeLove { .. } => "AnimalMakeLove",
            Self::FollowTemptation { .. } => "FollowTemptation",
            Self::RandomLookAround { .. } => "RandomLookAround",
            Self::DoNothing { .. } => "DoNothing",
            Self::Gate(gate) => gate.name(),
            Self::Armadillo(behavior) => behavior.name(),
            Self::Goat(behavior) => behavior.name(),
            Self::Frog(behavior) => behavior.name(),
        }
    }

    /// `Behavior.timedOut` 被覆写成恒假的那几个。
    ///
    /// The behaviours whose `timedOut` is overridden to always be false.
    pub(super) fn never_times_out(&self) -> bool {
        matches!(
            self,
            Self::CountDownCooldown(_) | Self::FollowTemptation { .. }
        )
    }

    /// `Behavior.checkExtraStartConditions`。
    ///
    /// `Behavior.checkExtraStartConditions`.
    pub(super) fn extra_start(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::Swim { .. } => goals::float_can_use(cx.me, cx.mob),
            Self::AnimalPanic { environmental, .. } => {
                let tag = if *environmental {
                    "minecraft:panic_environmental_causes"
                } else {
                    "minecraft:panic_causes"
                };
                cx.memories
                    .damage(HurtBy)
                    .is_some_and(|source| goals::damage_type_tagged(tag, source.kind))
                    || cx.memories.has(IsPanicking)
            }
            Self::MoveToTargetSink(sink) => {
                if sink.armadillo && super::super::armadillo::is_scared(cx.mob) {
                    return false;
                }
                move_sink_extra_start(sink, cx)
            }
            Self::MakeLove { partner, .. } => {
                cx.mob.in_love > 0 && find_breed_partner(cx, *partner).is_some()
            }
            Self::Armadillo(behavior) => behavior.extra_start(cx),
            Self::Goat(behavior) => behavior.extra_start(cx),
            Self::Frog(behavior) => behavior.extra_start(cx),
            _ => true,
        }
    }

    /// `Behavior.start`。
    ///
    /// `Behavior.start`.
    pub(super) fn start(&mut self, cx: &mut Cx) {
        match self {
            Self::AnimalPanic { armadillo, .. } => {
                if *armadillo {
                    super::super::armadillo::roll_out(cx.me, cx.mob);
                }
                cx.memories.set(IsPanicking, MemoryValue::Bool(true));
                cx.memories.erase(WalkTarget);
                cx.mob.navigation.stop();
            }
            Self::MoveToTargetSink(sink) => move_sink_start(sink, cx),
            Self::MakeLove {
                partner,
                speed,
                close_enough,
                spawn_child_at,
            } => {
                let Some(found) = find_breed_partner(cx, *partner) else {
                    return;
                };
                cx.memories.set(BreedTarget, MemoryValue::Entity(found));
                sensor::set_partner_memory(
                    cx,
                    found,
                    BreedTarget,
                    Some(MemoryValue::Entity(cx.me.id)),
                );
                lock_gaze_and_walk_to_each_other(cx, found, *speed, *close_enough);
                let duration = 60 + cx.me.random.next_int_bounded(50);
                *spawn_child_at = cx.time + i64::from(duration);
            }
            Self::RandomLookAround {
                min_interval,
                max_interval,
                max_yaw,
                min_pitch,
                pitch_range,
            } => {
                let pitch = mth::clamp_f32(
                    cx.me.random.next_float() * *pitch_range + *min_pitch,
                    -90.0,
                    90.0,
                );
                let rotation = mth::wrap_degrees(
                    cx.me.y_rot + 2.0 * cx.me.random.next_float() * *max_yaw - *max_yaw,
                );
                let eye = Vec3::new(
                    cx.me.position.x,
                    cx.me.position.y + f64::from(cx.mob.eye_height),
                    cx.me.position.z,
                );
                let look = eye.add(direction_from_rotation(pitch, rotation));
                cx.memories.set(
                    LookTarget,
                    MemoryValue::Look(PositionTracker::at_point(look)),
                );
                let cooldown = cx
                    .me
                    .random
                    .next_int_bounded(*max_interval - *min_interval + 1)
                    + *min_interval;
                cx.memories
                    .set(GazeCooldownTicks, MemoryValue::Int(cooldown));
            }
            Self::Armadillo(behavior) => behavior.start(cx),
            Self::Goat(behavior) => behavior.start(cx),
            Self::Frog(behavior) => behavior.start(cx),
            _ => {}
        }
    }

    /// `Behavior.canStillUse`。
    ///
    /// `Behavior.canStillUse`.
    pub(super) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::Swim { .. } => goals::float_can_use(cx.me, cx.mob),
            Self::AnimalPanic { .. } => true,
            Self::LookAtTargetSink { .. } => cx
                .memories
                .look_target()
                .is_some_and(|target| is_visible_by(cx, target)),
            Self::MoveToTargetSink(sink) => move_sink_can_still_use(sink, cx),
            Self::CountDownCooldown(memory) => {
                cx.memories.int(*memory).is_some_and(|ticks| ticks > 0)
            }
            Self::MakeLove {
                partner,
                spawn_child_at,
                ..
            } => make_love_can_still_use(cx, *partner, *spawn_child_at),
            Self::FollowTemptation { .. } => {
                cx.memories.has(TemptingPlayer)
                    && !cx.memories.has(BreedTarget)
                    && !cx.memories.has(IsPanicking)
            }
            Self::Armadillo(behavior) => behavior.can_still_use(cx),
            Self::Goat(behavior) => behavior.can_still_use(cx),
            Self::Frog(behavior) => behavior.can_still_use(cx),
            _ => false,
        }
    }

    /// `Behavior.tick`。
    ///
    /// `Behavior.tick`.
    pub(super) fn tick(&mut self, cx: &mut Cx) {
        match self {
            Self::Swim { chance } => {
                if cx.me.random.next_float() < *chance {
                    cx.mob.jump.jump = true;
                }
            }
            Self::AnimalPanic { speed, .. } => {
                if cx.mob.navigation.is_done()
                    && let Some(pos) = panic_pos(cx)
                {
                    cx.memories.set(
                        WalkTarget,
                        MemoryValue::Walk(WalkTarget::to_point(pos, *speed, 0)),
                    );
                }
            }
            Self::LookAtTargetSink { .. } => {
                if let Some(target) = cx.memories.look_target()
                    && let Some(at) = current_position(cx, target)
                {
                    // `setLookAt(pos)` 用 `getHeadRotSpeed()` 和 `getMaxHeadXRot()`。
                    // `setLookAt(pos)` uses `getHeadRotSpeed()` and `getMaxHeadXRot()`.
                    let speed = living::head_rot_speed(cx.mob);
                    cx.mob.look.set_look_at(
                        at.x,
                        at.y,
                        at.z,
                        speed,
                        super::super::control::MAX_HEAD_X_ROT as f32,
                    );
                }
            }
            Self::MoveToTargetSink(sink) => move_sink_tick(sink, cx),
            Self::CountDownCooldown(memory) => {
                if let Some(ticks) = cx.memories.int(*memory) {
                    cx.memories.set(*memory, MemoryValue::Int(ticks - 1));
                }
            }
            Self::MakeLove {
                speed,
                close_enough,
                spawn_child_at,
                ..
            } => {
                let Some(partner) = cx.memories.entity(BreedTarget) else {
                    return;
                };
                lock_gaze_and_walk_to_each_other(cx, partner, *speed, *close_enough);
                let Some(view) = sensor::living(cx, partner) else {
                    return;
                };
                if cx.me.position.distance_to_sqr(view.position) < 9.0 && cx.time >= *spawn_child_at
                {
                    living::breed_with(cx.me, cx.mob, cx.ctx, partner);
                    // `Frog.spawnChildFromBreeding`：不生幼崽，自己记成怀孕。
                    // `Frog.spawnChildFromBreeding`: no child; the frog becomes pregnant.
                    if cx.mob.mob_type == MobType::Frog {
                        cx.memories.set(IsPregnant, MemoryValue::Unit);
                    }
                    cx.memories.erase(BreedTarget);
                    sensor::set_partner_memory(cx, partner, BreedTarget, None);
                }
            }
            Self::FollowTemptation {
                speed,
                close_adult,
                close_baby,
            } => {
                let Some(player) = cx.memories.entity(TemptingPlayer) else {
                    return;
                };
                cx.memories.set(
                    LookTarget,
                    MemoryValue::Look(PositionTracker::Entity {
                        uid: player,
                        track_eye: true,
                        target_eye: false,
                    }),
                );
                let close = if cx.mob.is_baby() {
                    *close_baby
                } else {
                    *close_adult
                };
                let Some(view) = sensor::living(cx, player) else {
                    return;
                };
                if cx.me.position.distance_to_sqr(view.position) < close * close {
                    cx.memories.erase(WalkTarget);
                } else {
                    cx.memories.set(
                        WalkTarget,
                        MemoryValue::Walk(WalkTarget {
                            target: PositionTracker::Entity {
                                uid: player,
                                track_eye: false,
                                target_eye: false,
                            },
                            speed: *speed,
                            close_enough: 2,
                        }),
                    );
                }
            }
            Self::Armadillo(behavior) => behavior.tick(cx),
            Self::Goat(behavior) => behavior.tick(cx),
            Self::Frog(behavior) => behavior.tick(cx),
            _ => {}
        }
    }

    /// `Behavior.stop`。
    ///
    /// `Behavior.stop`.
    pub(super) fn stop(&mut self, cx: &mut Cx) {
        match self {
            Self::AnimalPanic { .. } => cx.memories.erase(IsPanicking),
            Self::LookAtTargetSink { .. } => cx.memories.erase(LookTarget),
            Self::MoveToTargetSink(sink) => move_sink_stop(sink, cx),
            Self::CountDownCooldown(memory) => cx.memories.erase(*memory),
            Self::MakeLove { spawn_child_at, .. } => {
                cx.memories.erase(BreedTarget);
                cx.memories.erase(WalkTarget);
                cx.memories.erase(LookTarget);
                *spawn_child_at = 0;
            }
            Self::FollowTemptation { .. } => {
                cx.memories
                    .set(TemptationCooldownTicks, MemoryValue::Int(100));
                cx.memories.erase(WalkTarget);
                cx.memories.erase(LookTarget);
            }
            Self::Armadillo(behavior) => behavior.stop(cx),
            Self::Goat(behavior) => behavior.stop(cx),
            Self::Frog(behavior) => behavior.stop(cx),
            _ => {}
        }
    }

    /// `OneShot.trigger` 里记忆条件之后的那段 lambda。
    ///
    /// The `OneShot.trigger` lambda that runs after the memory conditions.
    pub(super) fn trigger(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::LookAtPlayerSometimes {
                max_dist,
                min_interval,
                max_interval,
                ticks_until_next_start,
            } => {
                let max_sqr = f64::from(*max_dist * *max_dist);
                let position = cx.me.position;
                let Some(target) = sensor::find_closest_visible(cx, &mut |view| {
                    view.player && view.position.distance_to_sqr(position) <= max_sqr
                }) else {
                    return false;
                };
                // `Ticker.tickDownAndCheck(level.getRandom())`。
                // `Ticker.tickDownAndCheck(level.getRandom())`.
                if *ticks_until_next_start == 0 {
                    let sample =
                        cx.level_next_int(*max_interval - *min_interval + 1) + *min_interval;
                    *ticks_until_next_start = sample - 1;
                    return false;
                }
                *ticks_until_next_start -= 1;
                if *ticks_until_next_start != 0 {
                    return false;
                }
                cx.memories.set(
                    LookTarget,
                    MemoryValue::Look(PositionTracker::Entity {
                        uid: target,
                        track_eye: true,
                        target_eye: false,
                    }),
                );
                true
            }
            Self::BabyFollowAdult { min, max, speed } => {
                if !cx.mob.is_baby() {
                    return false;
                }
                let Some(adult) = cx.memories.entity(NearestVisibleAdult) else {
                    return false;
                };
                let Some(view) = sensor::living(cx, adult) else {
                    return false;
                };
                let distance = cx.me.position.distance_to_sqr(view.position);
                let far = f64::from(*max + 1);
                let near = f64::from(*min);
                if distance < far * far && distance >= near * near {
                    let walk = WalkTarget {
                        target: PositionTracker::Entity {
                            uid: adult,
                            track_eye: false,
                            target_eye: false,
                        },
                        speed: *speed,
                        close_enough: *min - 1,
                    };
                    cx.memories.set(
                        LookTarget,
                        MemoryValue::Look(PositionTracker::Entity {
                            uid: adult,
                            track_eye: true,
                            target_eye: false,
                        }),
                    );
                    cx.memories.set(WalkTarget, MemoryValue::Walk(walk));
                    true
                } else {
                    false
                }
            }
            Self::RandomStroll { speed } => {
                let pos = goals::land_random_pos(cx.me, cx.mob, cx.ctx.blocks(), 10, 7);
                cx.memories.set_or_erase(
                    WalkTarget,
                    pos.map(|pos| MemoryValue::Walk(WalkTarget::to_point(pos, *speed, 0))),
                );
                true
            }
            Self::WalkTargetFromLook {
                speed,
                close_enough,
            } => {
                let Some(target) = cx.memories.look_target() else {
                    return false;
                };
                cx.memories.set(
                    WalkTarget,
                    MemoryValue::Walk(WalkTarget {
                        target,
                        speed: *speed,
                        close_enough: *close_enough,
                    }),
                );
                true
            }
            Self::TriggerIf(trigger) => trigger.matches(cx.me.on_ground, cx.me.in_water),
            Self::Armadillo(behavior) => behavior.trigger(cx),
            Self::Frog(behavior) => behavior.trigger(cx),
            _ => false,
        }
    }
}

/// `Vec3.directionFromRotation(xRot, yRot)`：乘法在 `float` 上做。
///
/// `Vec3.directionFromRotation(xRot, yRot)`, multiplied in `float`.
fn direction_from_rotation(x_rot: f32, y_rot: f32) -> Vec3 {
    const DEG: f32 = (std::f64::consts::PI / 180.0) as f32;
    const PI: f32 = std::f64::consts::PI as f32;
    let y_cos = mth::cos(f64::from(-y_rot * DEG - PI));
    let y_sin = mth::sin(f64::from(-y_rot * DEG - PI));
    let x_cos = -mth::cos(f64::from(-x_rot * DEG));
    let x_sin = mth::sin(f64::from(-x_rot * DEG));
    Vec3::new(
        f64::from(y_sin * x_cos),
        f64::from(x_sin),
        f64::from(y_cos * x_cos),
    )
}

/// `PositionTracker.currentPosition`；跟踪的实体不在了时没有位置。
///
/// `PositionTracker.currentPosition`; no position once the tracked entity is gone.
pub(in crate::entity::mob) fn current_position(cx: &Cx, target: PositionTracker) -> Option<Vec3> {
    match target {
        PositionTracker::Block { center, .. } => Some(center),
        PositionTracker::Entity { uid, track_eye, .. } => {
            let view = sensor::living(cx, uid)?;
            Some(if track_eye {
                view.position
                    .add(Vec3::new(0.0, f64::from(view.eye_height), 0.0))
            } else {
                view.position
            })
        }
    }
}

/// `PositionTracker.currentBlockPosition`。
///
/// `PositionTracker.currentBlockPosition`.
pub(super) fn current_block_position(cx: &Cx, target: PositionTracker) -> Option<BlockPos> {
    match target {
        PositionTracker::Block { pos, .. } => Some(pos),
        PositionTracker::Entity {
            uid, target_eye, ..
        } => {
            let view = sensor::living(cx, uid)?;
            Some(if target_eye {
                BlockPos::containing(view.position.add(Vec3::new(
                    0.0,
                    f64::from(view.eye_height),
                    0.0,
                )))
            } else {
                BlockPos::containing(view.position)
            })
        }
    }
}

/// `PositionTracker.isVisibleBy`：方块总看得见；活体要活着且在 `visible_mobs` 里看得见。
///
/// `PositionTracker.isVisibleBy`: blocks always are; living entities must be alive and
/// visible in `visible_mobs`.
fn is_visible_by(cx: &mut Cx, target: PositionTracker) -> bool {
    match target {
        PositionTracker::Block { .. } => true,
        PositionTracker::Entity { uid, .. } => match sensor::living(cx, uid) {
            Some(view) if view.alive => sensor::visible_contains(cx, uid),
            _ => false,
        },
    }
}

/// `AnimalPanic.getPanicPos`：着火时先找水，否则 `LandRandomPos.getPos(mob, 5, 4)`。
///
/// `AnimalPanic.getPanicPos`: water first while burning, else `LandRandomPos.getPos(mob, 5, 4)`.
fn panic_pos(cx: &mut Cx) -> Option<Vec3> {
    if cx.me.remaining_fire_ticks > 0
        && let Some(water) = goals::look_for_water(cx.ctx.blocks(), cx.me.block_position(), 5)
    {
        return Some(Vec3::new(
            f64::from(water.x) + 0.5,
            f64::from(water.y),
            f64::from(water.z) + 0.5,
        ));
    }
    goals::land_random_pos(cx.me, cx.mob, cx.ctx.blocks(), 5, 4)
}

// ---- MoveToTargetSink ----

fn reached_target(cx: &Cx, walk: WalkTarget) -> bool {
    current_block_position(cx, walk.target).is_some_and(|pos| {
        let me = cx.me.block_position();
        (pos.x - me.x).abs() + (pos.y - me.y).abs() + (pos.z - me.z).abs() <= walk.close_enough
    })
}

fn is_walk_target_spectator(cx: &Cx, walk: WalkTarget) -> bool {
    match walk.target {
        PositionTracker::Entity { uid, .. } => {
            sensor::living(cx, uid).is_some_and(|view| view.spectator)
        }
        PositionTracker::Block { .. } => false,
    }
}

fn move_sink_extra_start(sink: &mut MoveToTargetSink, cx: &mut Cx) -> bool {
    if sink.remaining_cooldown > 0 {
        sink.remaining_cooldown -= 1;
        return false;
    }
    let Some(walk) = cx.memories.walk_target() else {
        return false;
    };
    let reached = reached_target(cx, walk);
    if !reached && try_compute_path(sink, cx, walk) {
        sink.last_target_pos = current_block_position(cx, walk.target);
        return true;
    }
    cx.memories.erase(WalkTarget);
    if reached {
        cx.memories.erase(CantReachWalkTargetSince);
    }
    false
}

fn move_sink_can_still_use(sink: &mut MoveToTargetSink, cx: &mut Cx) -> bool {
    if sink.path.is_none() || sink.last_target_pos.is_none() {
        return false;
    }
    let walk = cx.memories.walk_target();
    let spectator = walk.is_some_and(|walk| is_walk_target_spectator(cx, walk));
    !cx.mob.navigation.is_done() && walk.is_some_and(|walk| !reached_target(cx, walk)) && !spectator
}

fn move_sink_stop(sink: &mut MoveToTargetSink, cx: &mut Cx) {
    if let Some(walk) = cx.memories.walk_target()
        && !reached_target(cx, walk)
        && cx.mob.navigation.is_stuck
    {
        sink.remaining_cooldown = cx.level_next_int(40);
    }
    cx.mob.navigation.stop();
    cx.memories.erase(WalkTarget);
    cx.memories.erase(Path);
    sink.path = None;
}

fn move_sink_start(sink: &mut MoveToTargetSink, cx: &mut Cx) {
    cx.memories
        .set_or_erase(Path, sink.path.as_ref().map(|_| MemoryValue::Unit));
    let nav = living::nav_mob(cx.me, cx.mob, cx.ctx.blocks());
    let path = sink.path.clone();
    cx.mob
        .navigation
        .move_to_path(cx.ctx.blocks(), &nav, path, f64::from(sink.speed));
}

fn move_sink_tick(sink: &mut MoveToTargetSink, cx: &mut Cx) {
    let current = cx.mob.navigation.path.clone();
    if sink.path != current {
        sink.path = current;
        cx.memories
            .set_or_erase(Path, sink.path.as_ref().map(|_| MemoryValue::Unit));
    }
    if sink.path.is_some()
        && let Some(last) = sink.last_target_pos
        && let Some(walk) = cx.memories.walk_target()
        && let Some(pos) = current_block_position(cx, walk.target)
    {
        let dx = f64::from(pos.x - last.x);
        let dy = f64::from(pos.y - last.y);
        let dz = f64::from(pos.z - last.z);
        if dx * dx + dy * dy + dz * dz > 4.0 && try_compute_path(sink, cx, walk) {
            sink.last_target_pos = current_block_position(cx, walk.target);
            move_sink_start(sink, cx);
        }
    }
}

/// `MoveToTargetSink.tryComputePath`。
///
/// `MoveToTargetSink.tryComputePath`.
fn try_compute_path(sink: &mut MoveToTargetSink, cx: &mut Cx, walk: WalkTarget) -> bool {
    let Some(target) = current_block_position(cx, walk.target) else {
        sink.path = None;
        return false;
    };
    let nav = living::nav_mob(cx.me, cx.mob, cx.ctx.blocks());
    sink.path = cx
        .mob
        .navigation
        .create_path(cx.ctx.blocks(), &nav, target, 0);
    sink.speed = walk.speed;
    if reached_target(cx, walk) {
        cx.memories.erase(CantReachWalkTargetSince);
        return false;
    }
    let can_reach = sink.path.as_ref().is_some_and(|path| path.reached);
    if can_reach {
        cx.memories.erase(CantReachWalkTargetSince);
    } else if !cx.memories.has(CantReachWalkTargetSince) {
        cx.memories
            .set(CantReachWalkTargetSince, MemoryValue::Long(cx.time));
    }
    if sink.path.is_some() {
        return true;
    }
    let toward = Vec3::new(
        f64::from(target.x) + 0.5,
        f64::from(target.y),
        f64::from(target.z) + 0.5,
    );
    let Some(step) = goals::default_random_pos_towards(
        cx.me,
        cx.mob,
        cx.ctx.blocks(),
        10,
        7,
        toward,
        std::f32::consts::FRAC_PI_2,
    ) else {
        return false;
    };
    let nav = living::nav_mob(cx.me, cx.mob, cx.ctx.blocks());
    sink.path = cx
        .mob
        .navigation
        .create_path(cx.ctx.blocks(), &nav, BlockPos::containing(step), 0);
    sink.path.is_some()
}

// ---- AnimalMakeLove ----

/// `AnimalMakeLove.findValidBreedPartner`：看得见的最近同种、能配对、不在恐慌的。
///
/// `AnimalMakeLove.findValidBreedPartner`: the nearest visible mate of the right type
/// that can mate and is not panicking.
fn find_breed_partner(cx: &mut Cx, partner: MobType) -> Option<EntityUid> {
    let me = cx.me.id;
    let in_love = cx.mob.in_love > 0;
    sensor::find_closest_visible(cx, &mut |view| {
        view.mob_type == Some(partner)
            && view.uid != me
            && in_love
            && view.in_love
            && !view.panicking
    })
}

fn make_love_can_still_use(cx: &mut Cx, partner_type: MobType, spawn_child_at: i64) -> bool {
    let Some(partner) = cx.memories.entity(BreedTarget) else {
        return false;
    };
    let Some(view) = sensor::living(cx, partner) else {
        return false;
    };
    if view.mob_type != Some(partner_type) {
        return false;
    }
    view.alive
        && cx.mob.in_love > 0
        && view.in_love
        && view.uid != cx.me.id
        && sensor::visible_contains(cx, partner)
        && cx.time <= spawn_child_at
        && !cx.memories.has(IsPanicking)
        && !view.panicking
}

/// `BehaviorUtils.lockGazeAndWalkToEachOther`：先互看，再互相设走路和看的目标。
///
/// `BehaviorUtils.lockGazeAndWalkToEachOther`: look at each other, then set each
/// other's walk and look targets.
fn lock_gaze_and_walk_to_each_other(cx: &mut Cx, partner: EntityUid, speed: f32, close: i32) {
    let me = cx.me.id;
    let look_at = |uid| {
        MemoryValue::Look(PositionTracker::Entity {
            uid,
            track_eye: true,
            target_eye: false,
        })
    };
    let walk_to = |uid| {
        MemoryValue::Walk(WalkTarget {
            target: PositionTracker::Entity {
                uid,
                track_eye: true,
                target_eye: false,
            },
            speed,
            close_enough: close,
        })
    };
    cx.memories.set(LookTarget, look_at(partner));
    sensor::set_partner_memory(cx, partner, LookTarget, Some(look_at(me)));
    cx.memories.set(LookTarget, look_at(partner));
    cx.memories.set(WalkTarget, walk_to(partner));
    sensor::set_partner_memory(cx, partner, LookTarget, Some(look_at(me)));
    sensor::set_partner_memory(cx, partner, WalkTarget, Some(walk_to(me)));
}

#[cfg(test)]
mod tests {
    use crate::entity::mob::brain::behavior::Class;

    use super::*;

    /// `BehaviorBuilder.triggerIf` 的两个谓词和官方 `Entity::onGround` / `isInWater`
    /// 对应；声明式行为是 `OneShot`、没有必需记忆、对拍类名 `OneShot`。
    ///
    /// The two `BehaviorBuilder.triggerIf` predicates match vanilla `Entity::onGround` /
    /// `isInWater`; the declarative behaviour is a `OneShot` with no required memories and
    /// dumps as `OneShot`.
    #[test]
    fn trigger_if_predicates_match_vanilla() {
        assert!(Trigger::OnGround.matches(true, false));
        assert!(!Trigger::OnGround.matches(false, true));
        assert!(Trigger::InWater.matches(false, true));
        assert!(!Trigger::InWater.matches(true, false));

        let kind = Kind::TriggerIf(Trigger::OnGround);
        assert!(matches!(kind.class(), Class::OneShot));
        assert!(kind.required().is_empty());
        assert_eq!(kind.name(), "OneShot");
    }
}
