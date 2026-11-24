//! 美西螈的大脑：官方 `AxolotlAi`、`PlayDead`、`ValidatePlayDead` 和
//! `AxolotlAttackablesSensor`。传感器和行为的顺序、优先级、进入条件照官方登记表。
//!
//! 改世界的那两部分没有建模：`swingForAttack` 的挥手动画，和 `Axolotl.onStopAttacking`
//! 给击杀者附近玩家加再生的回调。见 README 的已知限制；其余分支逐条按官方源码翻译。
//!
//! The axolotl's brain: vanilla `AxolotlAi`, `PlayDead`, `ValidatePlayDead` and
//! `AxolotlAttackablesSensor`. The sensors and behaviours keep vanilla's registration
//! order, priorities and entry conditions.
//!
//! Two branches that write to the world are not modelled: the `swingForAttack` arm-swing
//! animation and `Axolotl.onStopAttacking`'s regeneration for a nearby player. They are
//! listed in the README's known limits; every other branch is a line-by-line translation.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{registries::mob_effect, tags::fluid_tags};

use crate::entity::ctx::EntityCtx;
use crate::entity::mob::brain::{self as brain_mod, sensor};
use crate::entity::mob::brain::{
    Activity, ActivityData, Brain, Cx,
    behavior::{Class, Gate, Kind, MoveToTargetSink, Speed, Trigger},
    memory::{MemoryStatus, MemoryType, MemoryValue, PositionTracker, WalkTarget},
};
use crate::entity::mob::{Mob, MobType, effects, living};
use crate::entity::{Entity, EntityUid};

use MemoryStatus::{Registered, ValueAbsent, ValuePresent};
use MemoryType::{
    AttackCoolingDown, AttackTarget, CantReachWalkTargetSince, HurtByEntity, LookTarget,
    NearestVisibleLivingEntities, PlayDeadTicks,
};

/// `AxolotlAi.ADULT_FOLLOW_RANGE`。
const ADULT_FOLLOW_RANGE: (i32, i32) = (5, 16);

/// `Axolotl.TOTAL_PLAYDEAD_TIME`。
const TOTAL_PLAYDEAD_TIME: i32 = 200;

/// `EntityTypeTags.AXOLOTL_HUNT_TARGETS`。
const HUNT_TARGETS_TAG: &str = "minecraft:axolotl_hunt_targets";

/// `EntityTypeTags.AXOLOTL_ALWAYS_HOSTILES`。
const ALWAYS_HOSTILES_TAG: &str = "minecraft:axolotl_always_hostiles";

/// `AxolotlAi.getSpeedModifier`：水里 0.5，岸上 0.15。
///
/// `AxolotlAi.getSpeedModifier`: 0.5 in water, 0.15 on land.
const SPEED_IDLE: Speed = Speed::WaterOrLand {
    water: 0.5,
    land: 0.15,
};

/// `AxolotlAi.getSpeedModifierChasing` / `getSpeedModifierFollowingAdult`：
/// 水里 0.6，岸上 0.15。
///
/// `AxolotlAi.getSpeedModifierChasing` and `getSpeedModifierFollowingAdult`: 0.6 in
/// water, 0.15 on land.
const SPEED_CHASE: Speed = Speed::WaterOrLand {
    water: 0.6,
    land: 0.15,
};

/// `Axolotl.BRAIN_PROVIDER`：五个传感器（顺序照官方）和 `AxolotlAi.getActivities()`。
///
/// `Axolotl.BRAIN_PROVIDER`: five sensors in vanilla order and `AxolotlAi.getActivities()`.
pub(in crate::entity::mob) fn brain() -> Brain {
    use crate::entity::mob::brain::{BrainTemplate, TemplateCache, template_cache};
    static TEMPLATE: TemplateCache = template_cache();
    Brain::cached(&TEMPLATE, || {
        BrainTemplate::new(
            &[
                sensor::SensorKind::NearestLivingEntities,
                sensor::SensorKind::NearestAdult,
                sensor::SensorKind::HurtBy,
                sensor::SensorKind::AxolotlAttackables,
                sensor::SensorKind::FoodTemptations,
            ],
            vec![
                core_activity(),
                idle_activity(),
                fight_activity(),
                play_dead_activity(),
            ],
        )
    })
}

/// `Axolotl.customServerAiStep` 的前半：跑大脑，再跑 `AxolotlAi.updateActivity`。
///
/// The first half of `Axolotl.customServerAiStep`: tick the brain, then
/// `AxolotlAi.updateActivity`.
pub(in crate::entity::mob) fn custom_server_ai_step(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
) {
    if let Some(mut brain) = mob.brain.take() {
        let gap = ctx.throttle_plan().decision_gap;
        brain.tick_by(me, mob, ctx, gap);
        update_activity(&mut brain);
        mob.brain = Some(brain);
    }
}

/// `AxolotlAi.initCoreActivity`。
fn core_activity() -> ActivityData {
    ActivityData::sequential(
        Activity::Core,
        0,
        vec![
            Kind::LookAtTargetSink { min: 45, max: 90 },
            Kind::MoveToTargetSink(MoveToTargetSink::new(false)),
            Kind::Axolotl(Behavior::ValidatePlayDead),
            Kind::CountDownCooldown(MemoryType::TemptationCooldownTicks),
        ],
    )
}

/// `AxolotlAi.initIdleActivity`。
fn idle_activity() -> ActivityData {
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
                1,
                Kind::MakeLove {
                    partner: MobType::Axolotl,
                    speed: 0.2,
                    close_enough: 2,
                    spawn_child_at: 0,
                },
            ),
            (
                2,
                Kind::Gate(Box::new(Gate::run_one(
                    Vec::new(),
                    vec![
                        (
                            Kind::FollowTemptation {
                                speed: SPEED_IDLE,
                                close_adult: 2.5,
                                close_baby: 2.5,
                            },
                            1,
                        ),
                        (
                            Kind::BabyFollowAdult {
                                min: ADULT_FOLLOW_RANGE.0,
                                max: ADULT_FOLLOW_RANGE.1,
                                speed: SPEED_CHASE,
                            },
                            1,
                        ),
                    ],
                ))),
            ),
            (3, Kind::Axolotl(Behavior::StartAttacking)),
            (
                3,
                Kind::Axolotl(Behavior::TryFindLiquid {
                    range: 6,
                    speed: 0.15,
                    next_ok: 0,
                }),
            ),
            (
                4,
                Kind::Gate(Box::new(Gate::ordered_try_all(
                    vec![(MemoryType::WalkTarget, ValueAbsent)],
                    vec![
                        (Kind::Axolotl(Behavior::SwimStroll { speed: 0.5 }), 2),
                        (Kind::RandomStroll { speed: 0.15 }, 2),
                        (
                            Kind::Axolotl(Behavior::WalkTargetFromLook {
                                speed: SPEED_IDLE,
                                close_enough: 3,
                            }),
                            3,
                        ),
                        (Kind::TriggerIf(Trigger::InWater), 5),
                        (Kind::TriggerIf(Trigger::OnGround), 5),
                    ],
                ))),
            ),
        ],
    )
}

/// `AxolotlAi.initFightActivity`：需要 `ATTACK_TARGET`，退出时清掉它。
///
/// `AxolotlAi.initFightActivity`: needs `ATTACK_TARGET` and erases it on exit.
fn fight_activity() -> ActivityData {
    let mut data = ActivityData::sequential(
        Activity::Fight,
        0,
        vec![
            Kind::Axolotl(Behavior::StopAttackingIfInvalid),
            Kind::Axolotl(Behavior::SetWalkTargetFromAttackTarget),
            Kind::Axolotl(Behavior::MeleeAttack { cooldown: 20 }),
            Kind::Axolotl(Behavior::EraseMemoryIfBreeding(AttackTarget)),
        ],
    )
    .when(vec![(AttackTarget, ValuePresent)]);
    // `ActivityData.create(..., MemoryModuleType.ATTACK_TARGET)`。
    data.erase_when_stopped = vec![AttackTarget];
    data
}

/// `AxolotlAi.initPlayDeadActivity`：需要 `PLAY_DEAD_TICKS`，退出时清掉它。
///
/// `AxolotlAi.initPlayDeadActivity`: needs `PLAY_DEAD_TICKS` and erases it on exit.
fn play_dead_activity() -> ActivityData {
    let mut data = ActivityData::sequential(
        Activity::PlayDead,
        0,
        vec![
            Kind::Axolotl(Behavior::PlayDead),
            // `EraseMemoryIf.create(BehaviorUtils::isBreeding, PLAY_DEAD_TICKS)`。
            Kind::Axolotl(Behavior::EraseMemoryIfBreeding(PlayDeadTicks)),
        ],
    )
    .when(vec![(PlayDeadTicks, ValuePresent)]);
    data.erase_when_stopped = vec![PlayDeadTicks];
    data
}

/// 美西螈专属的行为。
///
/// The axolotl's own behaviours.
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// `PlayDead`：200 刻；开始时清目标并给 200 刻再生。
    ///
    /// `PlayDead`: 200 ticks; clears the targets and grants 200 ticks of regeneration.
    PlayDead,
    /// `ValidatePlayDead.create()`。
    ///
    /// `ValidatePlayDead.create()`.
    ValidatePlayDead,
    /// `TryFindLiquid.create(range, speed, #axolotl_tries_to_find)`。
    ///
    /// `TryFindLiquid.create(range, speed, #axolotl_tries_to_find)`.
    TryFindLiquid {
        range: i32,
        speed: f32,
        next_ok: i64,
    },
    /// `RandomStroll.swim(speed)`。
    ///
    /// `RandomStroll.swim(speed)`.
    SwimStroll { speed: f32 },
    /// `StartAttacking.create(AxolotlAi::findNearestValidAttackTarget)`。
    ///
    /// `StartAttacking.create(AxolotlAi::findNearestValidAttackTarget)`.
    StartAttacking,
    /// `StopAttackingIfTargetInvalid.create(Axolotl::onStopAttacking)`。
    ///
    /// `StopAttackingIfTargetInvalid.create(Axolotl::onStopAttacking)`.
    StopAttackingIfInvalid,
    /// `SetWalkTargetFromAttackTargetIfTargetOutOfReach.create(chasing speed)`。
    ///
    /// `SetWalkTargetFromAttackTargetIfTargetOutOfReach.create(chasing speed)`.
    SetWalkTargetFromAttackTarget,
    /// `MeleeAttack.create(cooldown)`。
    ///
    /// `MeleeAttack.create(cooldown)`.
    MeleeAttack { cooldown: i32 },
    /// `EraseMemoryIf.create(BehaviorUtils::isBreeding, memory)`。
    ///
    /// `EraseMemoryIf.create(BehaviorUtils::isBreeding, memory)`.
    EraseMemoryIfBreeding(MemoryType),
    /// `SetWalkTargetFromLookTarget.create(canSetWalkTargetFromLookTarget, speed, close)`。
    ///
    /// `SetWalkTargetFromLookTarget.create(...)` with the axolotl's predicate.
    WalkTargetFromLook { speed: Speed, close_enough: i32 },
}

const PLAY_DEAD_REQUIRED: &[(MemoryType, MemoryStatus)] =
    &[(PlayDeadTicks, ValuePresent), (HurtByEntity, ValuePresent)];
const VALIDATE_PLAY_DEAD_REQUIRED: &[(MemoryType, MemoryStatus)] =
    &[(PlayDeadTicks, ValuePresent), (HurtByEntity, Registered)];
const TRY_FIND_LIQUID_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (AttackTarget, ValueAbsent),
    (MemoryType::WalkTarget, ValueAbsent),
    (LookTarget, Registered),
];
const SWIM_STROLL_REQUIRED: &[(MemoryType, MemoryStatus)] =
    &[(MemoryType::WalkTarget, ValueAbsent)];
const START_ATTACKING_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (AttackTarget, ValueAbsent),
    (CantReachWalkTargetSince, Registered),
];
const STOP_ATTACKING_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (AttackTarget, ValuePresent),
    (CantReachWalkTargetSince, Registered),
];
const WALK_TO_ATTACK_TARGET_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (MemoryType::WalkTarget, Registered),
    (LookTarget, Registered),
    (AttackTarget, ValuePresent),
    (NearestVisibleLivingEntities, Registered),
];
const MELEE_ATTACK_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (LookTarget, Registered),
    (AttackTarget, ValuePresent),
    (AttackCoolingDown, ValueAbsent),
    (NearestVisibleLivingEntities, ValuePresent),
];
const WALK_FROM_LOOK_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (MemoryType::WalkTarget, ValueAbsent),
    (LookTarget, ValuePresent),
];
const ERASE_PLAY_DEAD_REQUIRED: &[(MemoryType, MemoryStatus)] = &[(PlayDeadTicks, ValuePresent)];
const ERASE_ATTACK_TARGET_REQUIRED: &[(MemoryType, MemoryStatus)] = &[(AttackTarget, ValuePresent)];

impl Behavior {
    pub(in crate::entity::mob) fn class(&self) -> Class {
        match self {
            Self::PlayDead => Class::Timed {
                min: TOTAL_PLAYDEAD_TIME,
                max: TOTAL_PLAYDEAD_TIME,
            },
            _ => Class::OneShot,
        }
    }

    pub(in crate::entity::mob) fn required(&self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::PlayDead => PLAY_DEAD_REQUIRED,
            Self::ValidatePlayDead => VALIDATE_PLAY_DEAD_REQUIRED,
            Self::TryFindLiquid { .. } => TRY_FIND_LIQUID_REQUIRED,
            Self::SwimStroll { .. } => SWIM_STROLL_REQUIRED,
            Self::StartAttacking => START_ATTACKING_REQUIRED,
            Self::StopAttackingIfInvalid => STOP_ATTACKING_REQUIRED,
            Self::SetWalkTargetFromAttackTarget => WALK_TO_ATTACK_TARGET_REQUIRED,
            Self::MeleeAttack { .. } => MELEE_ATTACK_REQUIRED,
            Self::EraseMemoryIfBreeding(memory) => match memory {
                AttackTarget => ERASE_ATTACK_TARGET_REQUIRED,
                _ => ERASE_PLAY_DEAD_REQUIRED,
            },
            Self::WalkTargetFromLook { .. } => WALK_FROM_LOOK_REQUIRED,
        }
    }

    pub(in crate::entity::mob) fn name(&self) -> &'static str {
        match self {
            Self::PlayDead => "PlayDead",
            _ => "OneShot",
        }
    }

    pub(in crate::entity::mob) fn extra_start(&mut self, cx: &mut Cx) -> bool {
        match self {
            // `PlayDead.checkExtraStartConditions`。
            Self::PlayDead => cx.me.in_water,
            _ => true,
        }
    }

    pub(in crate::entity::mob) fn start(&mut self, cx: &mut Cx) {
        if let Self::PlayDead = self {
            cx.memories.erase(MemoryType::WalkTarget);
            cx.memories.erase(LookTarget);
            living::add_effect(
                cx.mob,
                effects::EffectInstance::new(mob_effect::REGENERATION, 200),
            );
        }
    }

    pub(in crate::entity::mob) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        match self {
            // `PlayDead.canStillUse`。
            Self::PlayDead => cx.me.in_water && cx.memories.has(PlayDeadTicks),
            _ => false,
        }
    }

    pub(in crate::entity::mob) fn tick(&mut self, _cx: &mut Cx) {}

    pub(in crate::entity::mob) fn stop(&mut self, _cx: &mut Cx) {}

    pub(in crate::entity::mob) fn trigger(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::ValidatePlayDead => validate_play_dead(cx),
            Self::TryFindLiquid {
                range,
                speed,
                next_ok,
            } => try_find_liquid(cx, *range, *speed, next_ok),
            Self::SwimStroll { speed } => swim_stroll(cx, *speed),
            Self::StartAttacking => start_attacking(cx),
            Self::StopAttackingIfInvalid => stop_attacking_if_invalid(cx),
            Self::SetWalkTargetFromAttackTarget => set_walk_target_from_attack_target(cx),
            Self::MeleeAttack { cooldown } => melee_attack(cx, *cooldown),
            Self::EraseMemoryIfBreeding(memory) => {
                if cx.mob.in_love == 0 {
                    return false;
                }
                cx.memories.erase(*memory);
                true
            }
            Self::WalkTargetFromLook {
                speed,
                close_enough,
            } => walk_target_from_look(cx, *speed, *close_enough),
            Self::PlayDead => false,
        }
    }
}

/// `AxolotlAi.updateActivity`：不在装死时挑第一个有效的活动；从 `FIGHT` 退出来时记下
/// 2400 刻的捕猎冷却。
///
/// `AxolotlAi.updateActivity`: pick the first valid activity unless playing dead; leaving
/// `FIGHT` records a 2400-tick hunting cooldown.
fn update_activity(brain: &mut Brain) {
    if brain.is_active(Activity::PlayDead) {
        return;
    }
    let was_fighting = brain.is_active(Activity::Fight);
    brain.set_active_activity_to_first_valid(&[
        Activity::PlayDead,
        Activity::Fight,
        Activity::Idle,
    ]);
    if was_fighting && !brain.is_active(Activity::Fight) {
        // `setMemoryWithExpiry(HAS_HUNTING_COOLDOWN, true, 2400L)`：`Codec.BOOL`，存盘时是布尔。
        // `setMemoryWithExpiry(HAS_HUNTING_COOLDOWN, true, 2400L)`: `Codec.BOOL`, saved as a bool.
        brain
            .memories
            .set_with_ttl(MemoryType::HuntingCooldown, MemoryValue::Bool(true), 2400);
    }
}

/// `ValidatePlayDead.create()`：记忆到 0 时清掉它和 `HURT_BY_ENTITY`，否则减一。
///
/// `ValidatePlayDead.create()`: at zero, erase it and `HURT_BY_ENTITY`; otherwise
/// decrement it.
fn validate_play_dead(cx: &mut Cx) -> bool {
    let ticks = cx.memories.int(PlayDeadTicks).unwrap_or(0);
    if ticks <= 0 {
        cx.memories.erase(PlayDeadTicks);
        cx.memories.erase(HurtByEntity);
        // `brain.useDefaultActivity()`：回到 `IDLE`。这一刻随后的 `update_activity`
        // 会重新挑第一个有效活动，装死记忆已经清掉，结果一致。
        // `brain.useDefaultActivity()` goes back to `IDLE`; the `update_activity` that
        // follows this tick re-picks the first valid activity, and the play-dead memory
        // is already gone, so the outcome is the same.
    } else {
        cx.memories.set(PlayDeadTicks, MemoryValue::Int(ticks - 1));
    }
    true
}

/// `AxolotlAttackablesSensor`：8 格内、在水里、是 `#axolotl_always_hostiles`，或没有捕猎
/// 冷却时是 `#axolotl_hunt_targets`，并且能被攻击，就记成 `NEAREST_ATTACKABLE`。
///
/// `AxolotlAttackablesSensor`: within eight blocks, in water, in
/// `#axolotl_always_hostiles` or — without the hunting cooldown — in
/// `#axolotl_hunt_targets`, and attackable; it becomes `NEAREST_ATTACKABLE`.
pub(in crate::entity::mob) fn sense_attackables(cx: &mut Cx) {
    let hunting = cx.memories.has(MemoryType::HuntingCooldown);
    // 先把符合条件的候选挑出来，搜索时只比 uid：`find_closest_visible` 要可变借用。
    // Collect the qualifying candidates first and match on uid inside the search, which
    // needs a mutable borrow.
    let mut candidates: Vec<EntityUid> = Vec::new();
    if let Some(MemoryValue::Visible(visible)) = cx.memories.get(NearestVisibleLivingEntities) {
        for &uid in visible.entities.iter() {
            let Some(other) = cx.ctx.other(uid) else {
                continue;
            };
            // `Mob.isInWater()`：公开快照没有这个字段，按脚下那格是不是水判断。
            // `Mob.isInWater()`: the public snapshot has no such field, so read the block
            // at its feet.
            if !cx
                .ctx
                .blocks()
                .block_state(BlockPos::containing(other.position))
                .fluid()
                .is_water()
            {
                continue;
            }
            let crate::entity::KindPublic::Mob(mob) = &other.kind else {
                continue;
            };
            let hostile = mob.mob_type.in_entity_tag(ALWAYS_HOSTILES_TAG);
            let hunt = !hunting && mob.mob_type.in_entity_tag(HUNT_TARGETS_TAG);
            if hostile || hunt {
                candidates.push(uid);
            }
        }
    }
    let found = sensor::find_closest_visible(cx, &mut |view| candidates.contains(&view.uid));
    cx.memories.set_or_erase(
        MemoryType::NearestAttackable,
        found.map(MemoryValue::Entity),
    );
}

/// `AxolotlAi.findNearestValidAttackTarget`：繁殖时不捕猎。
///
/// `AxolotlAi.findNearestValidAttackTarget`: no hunting while breeding.
fn start_attacking(cx: &mut Cx) -> bool {
    if cx.mob.in_love > 0 {
        return false;
    }
    let Some(target) = cx.memories.entity(MemoryType::NearestAttackable) else {
        return false;
    };
    if !sensor::living(cx, target).is_some_and(|view| view.alive) {
        return false;
    }
    cx.memories.set(AttackTarget, MemoryValue::Entity(target));
    cx.memories.erase(CantReachWalkTargetSince);
    true
}

/// `StopAttackingIfTargetInvalid.create(Axolotl::onStopAttacking)`：目标还能打、没追烦、
/// 还活着就留着，否则清掉。
///
/// `StopAttackingIfTargetInvalid.create(Axolotl::onStopAttacking)`: keep a target that is
/// still attackable, not given up on and alive; otherwise erase it.
fn stop_attacking_if_invalid(cx: &mut Cx) -> bool {
    let alive = cx
        .memories
        .entity(AttackTarget)
        .and_then(|target| sensor::living(cx, target))
        .is_some_and(|view| view.alive);
    let tired = match cx.memories.get(CantReachWalkTargetSince) {
        Some(MemoryValue::Long(since)) => cx.time - since > 200,
        _ => false,
    };
    if !alive || tired {
        // `Axolotl.onStopAttacking` 的再生回调没有建模，见模块文档。
        // The regeneration callback of `Axolotl.onStopAttacking` is not modelled; see
        // the module docs.
        cx.memories.erase(AttackTarget);
    }
    true
}

/// `SetWalkTargetFromAttackTargetIfTargetOutOfReach`：目标在视线表里且在近战范围内就清掉
/// 走的目标，否则记下目标实体并走过去。
///
/// `SetWalkTargetFromAttackTargetIfTargetOutOfReach`: clear the walk target when the
/// target is visible and in melee range, else remember it and walk over.
fn set_walk_target_from_attack_target(cx: &mut Cx) -> bool {
    let Some(target) = cx.memories.entity(AttackTarget) else {
        return false;
    };
    let visible = visible_contains(cx, target);
    let in_range =
        sensor::living(cx, target).is_some_and(|view| within_melee_range(cx, &view.bounding_box));
    if visible && in_range {
        cx.memories.erase(MemoryType::WalkTarget);
    } else {
        cx.memories.set(
            LookTarget,
            MemoryValue::Look(PositionTracker::Entity {
                uid: target,
                track_eye: true,
                target_eye: false,
            }),
        );
        cx.memories.set(
            MemoryType::WalkTarget,
            MemoryValue::Walk(WalkTarget {
                target: PositionTracker::Entity {
                    uid: target,
                    track_eye: false,
                    target_eye: false,
                },
                speed: SPEED_CHASE.value(cx.me.in_water),
                close_enough: 0,
            }),
        );
    }
    true
}

/// `MeleeAttack.create(cooldown)`：目标在视线表里且进了近战范围就挥一次、加冷却。
///
/// `MeleeAttack.create(cooldown)`: swing once when the target is visible and in melee
/// range, then start the cooldown.
fn melee_attack(cx: &mut Cx, cooldown: i32) -> bool {
    let Some(target) = cx.memories.entity(AttackTarget) else {
        return false;
    };
    if !visible_contains(cx, target) {
        return false;
    }
    let Some(view) = sensor::living(cx, target) else {
        return false;
    };
    if !within_melee_range(cx, &view.bounding_box) {
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
    // `swingForAttack(InteractionHand.MAIN_HAND)`：挥手动画没有建模，见模块文档。
    // `swingForAttack(InteractionHand.MAIN_HAND)`: the swing animation is not modelled,
    // see the module docs.
    living::do_hurt_target(cx.me, cx.mob, cx.ctx, target);
    cx.memories
        .set_with_ttl(AttackCoolingDown, MemoryValue::Unit, i64::from(cooldown));
    true
}

/// 目标在 `NEAREST_VISIBLE_LIVING_ENTITIES` 里。
///
/// Whether the target is in `NEAREST_VISIBLE_LIVING_ENTITIES`.
fn visible_contains(cx: &Cx, target: EntityUid) -> bool {
    match cx.memories.get(NearestVisibleLivingEntities) {
        Some(MemoryValue::Visible(visible)) => visible.entities.contains(&target),
        _ => false,
    }
}

/// `Mob.isWithinMeleeAttackRange` 对另一个实体的碰撞箱。
///
/// `Mob.isWithinMeleeAttackRange` against another entity's box.
fn within_melee_range(cx: &Cx, target: &crate::shape::Aabb) -> bool {
    // `Mob.DEFAULT_ATTACK_REACH = sqrt(2.04f) - 0.6f`。
    let reach = 2.039_999_961_853_027_3_f64.sqrt() - f64::from(0.6_f32);
    let (width, height) = cx.mob.dimensions();
    let half = f64::from(width) / 2.0;
    let ours = crate::shape::Aabb::new(
        cx.me.position.x - half - reach,
        cx.me.position.y,
        cx.me.position.z - half - reach,
        cx.me.position.x + half + reach,
        cx.me.position.y + f64::from(height),
        cx.me.position.z + half + reach,
    );
    ours.intersects(*target)
}

/// `TryFindLiquid`：不在目标流体里时，找最近的、水平错开的、方块是液体且属于
/// `#axolotl_tries_to_find` 的格子；上面是空气就用它，否则用第一个离身体超过 1.5 格的。
///
/// `TryFindLiquid`: out of the fluid, find the nearest horizontally offset liquid block of
/// `#axolotl_tries_to_find`; take it when the block above is air, else the first one more
/// than 1.5 blocks from the body.
fn try_find_liquid(cx: &mut Cx, range: i32, speed: f32, next_ok: &mut i64) -> bool {
    let here = cx.me.block_position();
    let tag = fluid_tags::AXOLOTL_TRIES_TO_FIND;
    if cx.ctx.blocks().block_state(here).fluid().is_tagged(tag) {
        return false;
    }
    if cx.time < *next_ok {
        *next_ok = cx.time + 22;
        return true;
    }
    let position = cx.me.position;
    let mut fallback: Option<BlockPos> = None;
    let found = super::super::frog::within_box_by_manhattan(here, range, &mut |pos| {
        if pos.x == here.x && pos.z == here.z {
            return false;
        }
        let view = cx.ctx.blocks();
        let state = view.block_state(pos);
        if !state.is_liquid() || !state.fluid().is_tagged(tag) {
            return false;
        }
        if view.block_state(pos.above()).is_air() {
            return true;
        }
        if fallback.is_none() && !closer_to_center_than(pos, position, 1.5) {
            fallback = Some(pos);
        }
        false
    })
    .or(fallback);
    if let Some(pos) = found {
        cx.memories.set(
            LookTarget,
            MemoryValue::Look(PositionTracker::at_block(pos)),
        );
        cx.memories.set(
            MemoryType::WalkTarget,
            MemoryValue::Walk(WalkTarget {
                target: PositionTracker::at_block(pos),
                speed,
                close_enough: 0,
            }),
        );
    }
    *next_ok = cx.time + 40;
    true
}

/// `RandomStroll.swim(speed)`：在水里时朝一个能游的点游过去。
///
/// `RandomStroll.swim(speed)`: swim toward a swimmable spot while in water.
fn swim_stroll(cx: &mut Cx, speed: f32) -> bool {
    if !cx.me.in_water {
        return false;
    }
    let pos = super::super::frog::target_swim_pos(cx);
    cx.memories.set_or_erase(
        MemoryType::WalkTarget,
        pos.map(|pos| MemoryValue::Walk(WalkTarget::to_point(pos, speed, 0))),
    );
    true
}

/// `SetWalkTargetFromLookTarget.create(canSetWalkTargetFromLookTarget, speed, close)`：
/// 看向的那格是不是水要和自己在不在水里一致，然后照朝向走。
///
/// `SetWalkTargetFromLookTarget.create(...)`: the looked-at block must be water exactly when
/// the mob is in water, then walk toward it.
fn walk_target_from_look(cx: &mut Cx, speed: Speed, close_enough: i32) -> bool {
    let Some(tracker) = cx.memories.look_target() else {
        return false;
    };
    let Some(pos) = brain_mod::current_position(cx, tracker) else {
        return false;
    };
    let water = cx
        .ctx
        .blocks()
        .block_state(BlockPos::containing(pos))
        .fluid()
        .is_water();
    if water != cx.me.in_water {
        return false;
    }
    cx.memories.set(
        MemoryType::WalkTarget,
        MemoryValue::Walk(WalkTarget {
            target: tracker,
            speed: speed.value(cx.me.in_water),
            close_enough,
        }),
    );
    true
}

/// `BlockPos.closerToCenterThan(pos, distance)`：方块中心到点的距离小于 `distance`。
///
/// `BlockPos.closerToCenterThan(pos, distance)`: the block centre is closer than `distance`.
fn closer_to_center_than(pos: BlockPos, point: Vec3, distance: f64) -> bool {
    let centre = Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    );
    centre.distance_to_sqr(point) < distance * distance
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 传感器顺序和四个活动的行为表，和 `AxolotlAi` 的登记一致。
    ///
    /// The sensor order and the four activities' behaviour tables follow the
    /// `AxolotlAi` registration.
    #[test]
    fn axolotl_brain_table_matches_vanilla() {
        let brain = brain();
        let sensors: Vec<&str> = brain
            .sensor_table()
            .into_iter()
            .map(|(name, _, _)| name)
            .collect();
        assert_eq!(
            sensors,
            vec![
                "NearestLivingEntitySensor",
                "AdultSensor",
                "HurtBySensor",
                "AxolotlAttackablesSensor",
                "TemptingSensor",
            ]
        );
        let fights: Vec<(i32, &str, String)> = brain
            .behavior_table()
            .into_iter()
            .filter(|(_, activity, _)| *activity == "fight")
            .collect();
        assert_eq!(
            fights,
            vec![
                (0, "fight", "OneShot".to_owned()),
                (1, "fight", "OneShot".to_owned()),
                (2, "fight", "OneShot".to_owned()),
                (3, "fight", "OneShot".to_owned()),
            ]
        );
        let play_dead: Vec<String> = brain
            .behavior_table()
            .into_iter()
            .filter(|(_, activity, _)| *activity == "play_dead")
            .map(|(_, _, name)| name)
            .collect();
        assert_eq!(play_dead, vec!["PlayDead".to_owned(), "OneShot".to_owned()]);
        assert!(brain.memories.is_registered(PlayDeadTicks));
        assert!(brain.memories.is_registered(MemoryType::HuntingCooldown));
        assert!(brain.memories.is_registered(AttackCoolingDown));
    }
}
