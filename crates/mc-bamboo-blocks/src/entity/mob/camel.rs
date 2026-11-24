//! 骆驼：官方 `Camel` 和 `CamelAi`。第四种跑大脑的生物（犰狳、山羊、青蛙、蝌蚪之后）。
//! 它是 `AbstractHorse`（马科），所以属性、背包、骑乘骨架复用 `equine`，但 `registerGoals()`
//! 是空的，行为全由 `Brain` 驱动；另有坐下 / 站起的姿势状态机和冲刺。
//!
//! `LAST_POSE_CHANGE_TICK` 是同步的 long：负号表示坐着，绝对值是上次换姿势的 `gameTime`，
//! `getPoseTime() = gameTime - abs(...)`；坐下过渡 40 刻、站起过渡 52 刻，`refuseToMove()`
//! 是坐着或在过渡里。场景里的骆驼不走 `finalizeSpawn`，这个字段保持 0，两侧同一套
//! `gameTime` 算出的 `getPoseTime()` 就一致。
//!
//! Camels: vanilla `Camel` and `CamelAi`, the fourth brain-driven mob. It is an
//! `AbstractHorse` (equine), so attributes, inventory and riding reuse `equine`, but
//! `registerGoals()` is empty and the brain drives everything; plus a sit/stand pose
//! state machine and the dash.
//!
//! `LAST_POSE_CHANGE_TICK` is a synced long: the sign marks sitting, the magnitude is the
//! `gameTime` of the last pose change, `getPoseTime() = gameTime - abs(...)`; sitting-down
//! transitions last 40 ticks, standing-up 52, and `refuseToMove()` means sitting or in a
//! transition. Scene camels skip `finalizeSpawn`, so this field stays 0 and both sides
//! agree on `getPoseTime()` from the same `gameTime`.

use mc_bamboo_core::mth;
use mc_bamboo_data::registries::attribute;

use super::{
    Mob, MobType,
    attributes::Attributes,
    brain::{
        Activity, ActivityData, Brain, Cx,
        behavior::{Class, Gate, Kind, MoveToTargetSink, Speed},
        memory::{MemoryStatus, MemoryStatus::ValueAbsent, MemoryType},
        sensor::SensorKind,
    },
    living,
};
use crate::entity::{Entity, ctx::EntityCtx};

/// `Camel.DASH_COOLDOWN_TICKS`。
pub const DASH_COOLDOWN_TICKS: i32 = 55;
/// 坐下过渡 40 刻、站起过渡 52 刻。
const SITDOWN_DURATION_TICKS: i64 = 40;
const STANDUP_DURATION_TICKS: i64 = 52;
/// `resetLastPoseChangeTickToFullStand`：`max(0, gameTime - 52 - 1)`。
const FULL_STAND_BACKDATE: i64 = 53;
/// 成年坐姿比站姿矮 1.43。
const SITTING_HEIGHT_DIFFERENCE: f32 = 1.43;
/// 骆驼 `getMaxHeadYRot()`。
const MAX_HEAD_Y_ROT: f32 = 30.0;

/// `Pose.STANDING / SITTING`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pose {
    #[default]
    Standing,
    Sitting,
}

/// 骆驼自己的同步字段和冲刺计时。
///
/// The camel's own synced fields and dash timer.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Camel {
    /// `LAST_POSE_CHANGE_TICK`（同步、存 `LastPoseTick`）。
    pub last_pose_change_tick: i64,
    /// `Pose`，和 long 的符号一起由换姿势的方法维护。
    pub pose: Pose,
    /// `DASH`（同步布尔）。
    pub dashing: bool,
    /// `dashCooldown`。
    pub dash_cooldown: i32,
}

/// `EntityTypes.CAMEL` / `BABY_*` / `*_SITTING_DIMENSIONS` 的（宽，高，眼高）。
///
/// (width, height, eye height) of `EntityTypes.CAMEL` and the baby / sitting dimensions.
pub(super) const fn dimensions(baby: bool, sitting: bool) -> (f32, f32, f32) {
    match (baby, sitting) {
        (false, false) => (1.7, 2.375, 2.275),
        (false, true) => (1.7, 2.375 - SITTING_HEIGHT_DIFFERENCE, 0.845),
        (true, false) => (0.95, 1.4, 1.38),
        (true, true) => (0.95, 0.425, 0.41),
    }
}

/// `Camel.createAttributes`：`createBaseHorseAttributes()` 再盖四个值。
/// 马科基线的 `SAFE_FALL_DISTANCE 6` / `FALL_DAMAGE_MULTIPLIER 0.5` 照 `equine::attributes`。
///
/// `Camel.createAttributes`: `createBaseHorseAttributes()` overridden with four values.
/// The equine baseline's safe-fall / fall-damage values follow `equine::attributes`.
pub(super) fn attributes() -> Attributes {
    let mut attrs = Attributes::cow();
    for (id, base) in [
        (attribute::MAX_HEALTH, 32.0),
        (attribute::MOVEMENT_SPEED, f64::from(0.09_f32)),
        (attribute::JUMP_STRENGTH, f64::from(0.42_f32)),
        (attribute::STEP_HEIGHT, 1.5),
        (attribute::SAFE_FALL_DISTANCE, 6.0),
        (attribute::FALL_DAMAGE_MULTIPLIER, 0.5),
    ] {
        attrs.set_base(id, base);
    }
    attrs
}

/// `Camel` 构造函数里的 `setCanFloat(true)` / `setCanWalkOverFences(true)`。
///
/// The `Camel` constructor's `setCanFloat(true)` / `setCanWalkOverFences(true)`.
pub(super) fn configure(mob: &mut Mob) {
    mob.navigation.flags.can_float = true;
}

// ---- 姿势状态机 ----
// ---- Pose state machine ----

/// `isCamelSitting()`：`LAST_POSE_CHANGE_TICK < 0`。
pub fn is_camel_sitting(mob: &Mob) -> bool {
    mob.camel.last_pose_change_tick < 0
}

/// `getPoseTime()`：`gameTime - abs(LAST_POSE_CHANGE_TICK)`。
pub fn pose_time(mob: &Mob, time: i64) -> i64 {
    time - mob.camel.last_pose_change_tick.abs()
}

/// `isInPoseTransition()`：`poseTime < (坐着 ? 40 : 52)`。
pub fn is_in_pose_transition(mob: &Mob, time: i64) -> bool {
    let limit = if is_camel_sitting(mob) {
        SITDOWN_DURATION_TICKS
    } else {
        STANDUP_DURATION_TICKS
    };
    pose_time(mob, time) < limit
}

/// `refuseToMove()`：坐着或正在换姿势。`triggerIf(!refuseToMove, ...)` 用它。
///
/// `refuseToMove()`: sitting or changing pose; used by `triggerIf(!refuseToMove, ...)`.
pub fn refuse_to_move(mob: &Mob, time: i64) -> bool {
    is_camel_sitting(mob) || is_in_pose_transition(mob, time)
}

/// `canCamelChangePose()`：目标姿势不会窒息。对拍场景都在开阔平地，站着坐着都放得下，
/// 恒为真；真正做 `wouldNotSuffocateAtTargetPose` 需要碰撞箱，留给已知限制。
///
/// `canCamelChangePose()`: the target pose would not suffocate. Oracle scenes are on open
/// flat ground where both poses fit, so this is always true; a faithful
/// `wouldNotSuffocateAtTargetPose` needs the bounding box and is left a known limitation.
fn can_camel_change_pose(_mob: &Mob) -> bool {
    true
}

/// `resetLastPoseChangeTick(long)`：同步字段赋值。
fn reset_last_pose_change_tick(mob: &mut Mob, value: i64) {
    mob.camel.last_pose_change_tick = value;
}

/// `setPose` 换姿势时 `refreshDimensions()` 会重算眼高（坐姿 0.845、站姿 2.275，
/// 幼年另算）。`LookControl.getXRotD` 读的就是这个眼高，所以每次换姿势都同步写回。
///
/// `setPose` triggers `refreshDimensions()`, which recomputes the eye height (0.845
/// sitting, 2.275 standing, separate baby values). `LookControl.getXRotD` reads that
/// eye height, so every pose change writes it back.
fn set_pose(mob: &mut Mob, pose: Pose) {
    mob.camel.pose = pose;
    mob.eye_height = dimensions(mob.is_baby(), pose == Pose::Sitting).2;
}

/// `resetLastPoseChangeTickToFullStand(currentTime)`。
fn reset_last_pose_change_tick_to_full_stand(mob: &mut Mob, time: i64) {
    reset_last_pose_change_tick(mob, (time - FULL_STAND_BACKDATE).max(0));
}

/// `sitDown()`：没坐着才 `makeSound(SIT)`、换姿势、记 `-gameTime`。
fn sit_down(cx: &mut Cx) {
    if is_camel_sitting(cx.mob) {
        return;
    }
    living::make_sound(cx.me, cx.mob, "minecraft:entity.camel.sit");
    set_pose(cx.mob, Pose::Sitting);
    reset_last_pose_change_tick(cx.mob, -cx.time);
}

/// `standUp()`：坐着才 `makeSound(STAND)`、换姿势、记 `gameTime`。
fn stand_up(cx: &mut Cx) {
    if !is_camel_sitting(cx.mob) {
        return;
    }
    living::make_sound(cx.me, cx.mob, "minecraft:entity.camel.stand");
    set_pose(cx.mob, Pose::Standing);
    reset_last_pose_change_tick(cx.mob, cx.time);
}

/// `standUpInstantly()`：不发声，直接站直并把姿势计时推到满（`CamelPanic.start`、
/// `actuallyHurt`、坐在水里时调用）。
///
/// `standUpInstantly()`: no sound, stand up at once and backdate the pose change to a full
/// stand (`CamelPanic.start`, `actuallyHurt`, sitting in water).
pub fn stand_up_instantly(mob: &mut Mob, time: i64) {
    set_pose(mob, Pose::Standing);
    reset_last_pose_change_tick_to_full_stand(mob, time);
}

/// `Camel.actuallyHurt` 在 `super` 之前先 `standUpInstantly()`。
pub(super) fn after_actually_hurt(mob: &mut Mob, time: i64) {
    stand_up_instantly(mob, time);
}

/// `CamelMoveControl.tick`：坐着、过了换姿势过渡、移动控制还想走，就站起来
/// （`super.tick()` 之前）。Bamboo 没有拴绳，所以 `!isLeashed()` 恒真。
///
/// `CamelMoveControl.tick`: before `super.tick()`, stand up when sitting, past the
/// pose transition, and the move control still wants to move. Bamboo has no leash,
/// so `!isLeashed()` always holds.
pub(super) fn before_move_control(me: &mut Entity, mob: &mut Mob, time: i64) {
    if mob.mover.operation == super::control::Operation::MoveTo
        && is_camel_sitting(mob)
        && !is_in_pose_transition(mob, time)
        && can_camel_change_pose(mob)
    {
        living::make_sound(me, mob, "minecraft:entity.camel.stand");
        set_pose(mob, Pose::Standing);
        reset_last_pose_change_tick(mob, time);
    }
}

/// `Camel.tick` 在 `super.tick()` 之后：冲刺标志复位、冷却倒数、`refuseToMove` 夹头、
/// 坐在水里立刻站起。
///
/// `Camel.tick` after `super.tick()`: clear the dash flag, count down the cooldown, clamp
/// the head while refusing to move, and stand up instantly when sitting in water.
pub(super) fn tick_after(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if mob.camel.dashing
        && mob.camel.dash_cooldown < 50
        && (me.on_ground || me.in_water || !mob.passengers.is_empty())
    {
        mob.camel.dashing = false;
    }
    if mob.camel.dash_cooldown > 0 {
        mob.camel.dash_cooldown -= 1;
        if mob.camel.dash_cooldown == 0 {
            living::play_sound(me, mob, "minecraft:entity.camel.dash_ready", 1.0, 1.0);
        }
    }
    let time = ctx.blocks().game_time();
    if refuse_to_move(mob, time) {
        // `LookControl.clampHeadRotationToBody()`：`rotateIfNecessary(head, body, 30)`。
        // `LookControl.clampHeadRotationToBody()`: `rotateIfNecessary(head, body, 30)`.
        mob.y_head_rot = mth::rotate_if_necessary(mob.y_head_rot, mob.y_body_rot, MAX_HEAD_Y_ROT);
    }
    if is_camel_sitting(mob) && me.in_water {
        stand_up_instantly(mob, time);
    }
}

/// `Camel.travel`：`refuseToMove && onGround` 时把水平速度和输入清零。
///
/// `Camel.travel`: zero the horizontal velocity and input while refusing to move on ground.
pub(super) fn travel(me: &mut Entity, mob: &mut Mob, time: i64, input: &mut mc_bamboo_core::Vec3) {
    if refuse_to_move(mob, time) && me.on_ground {
        use mc_bamboo_core::Vec3;
        me.delta = Vec3::new(0.0, me.delta.y, 0.0);
        *input = Vec3::new(0.0, input.y, 0.0);
    }
}

/// `Camel.getMaxHeadYRot()`：30（其余生物见 `living::max_head_y_rot`）。
pub(super) const fn max_head_y_rot() -> f32 {
    MAX_HEAD_Y_ROT
}

// ---- 行为 ----
// ---- Behaviours ----

/// `CamelAi` 里的骆驼专属行为。
///
/// The camel-only behaviour of `CamelAi`.
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// `CamelAi.RandomSitting(20)`：`Behavior`（60 刻），到点就坐下 / 站起。
    ///
    /// `CamelAi.RandomSitting(20)`: a `Behavior` (60 ticks) that sits down / stands up.
    RandomSitting,
}

/// `RandomSitting.minimalPoseTicks = 20 * 20`。
const RANDOM_SITTING_MINIMAL_POSE_TICKS: i64 = 400;

impl Behavior {
    /// `RandomSitting` 继承自 `Behavior`（`new Behavior(Map.of())` 是 60 / 60 刻）。
    pub(super) fn class(&self) -> Class {
        Class::Timed { min: 60, max: 60 }
    }

    pub(super) fn required(&self) -> &'static [(MemoryType, MemoryStatus)] {
        &[]
    }

    pub(super) fn name(&self) -> &'static str {
        "RandomSitting"
    }

    /// `RandomSitting.checkExtraStartConditions`。
    pub(super) fn extra_start(&self, cx: &mut Cx) -> bool {
        let mob = &cx.mob;
        // Bamboo 没有拴绳（leash），`!isLeashed()` 恒真，见已知限制。
        // Bamboo has no leash, so `!isLeashed()` is always true (known limitation).
        !cx.me.in_water
            && pose_time(mob, cx.time) >= RANDOM_SITTING_MINIMAL_POSE_TICKS
            && cx.me.on_ground
            && mob.controlling_passenger().is_none()
            && can_camel_change_pose(mob)
    }

    pub(super) fn start(&self, cx: &mut Cx) {
        if is_camel_sitting(cx.mob) {
            stand_up(cx);
        } else if !cx.memories.has(MemoryType::IsPanicking) {
            sit_down(cx);
        }
    }

    /// `RandomSitting` 没有覆盖 `Behavior.canStillUse`，基类恒假：坐下 / 站起是
    /// 一次性动作，起跑的这一刻 `tickOrStop` 就把它停下，闸门随即也停。
    ///
    /// `RandomSitting` does not override `Behavior.canStillUse`, whose base returns
    /// false: sitting / standing is a one-shot action, stopped by `tickOrStop` on the
    /// very tick it starts, which also stops the gate.
    pub(super) fn can_still_use(&self, _cx: &mut Cx) -> bool {
        false
    }

    pub(super) fn tick(&self, _cx: &mut Cx) {}

    pub(super) fn stop(&self, _cx: &mut Cx) {}

    pub(super) fn trigger(&self, _cx: &mut Cx) -> bool {
        false
    }
}

// ---- 大脑 ----
// ---- Brain ----

/// `Camel.BRAIN_PROVIDER`：四个传感器，核心、闲逛两个活动；`initMemories` 是空的，
/// 不取随机数。
///
/// `Camel.BRAIN_PROVIDER`: four sensors, core and idle activities; `initMemories` is empty
/// and draws nothing.
pub(super) fn brain() -> Brain {
    use crate::entity::mob::brain::{BrainTemplate, TemplateCache, template_cache};
    static TEMPLATE: TemplateCache = template_cache();
    Brain::cached(&TEMPLATE, || {
        BrainTemplate::new(
            &[
                SensorKind::NearestLivingEntities,
                SensorKind::HurtBy,
                SensorKind::FoodTemptations,
                SensorKind::NearestAdult,
            ],
            vec![core_activity(), idle_activity()],
        )
    })
}

fn core_activity() -> ActivityData {
    ActivityData::sequential(
        Activity::Core,
        0,
        vec![
            Kind::Swim { chance: 0.8 },
            // `CamelPanic(4.0)`：`AnimalPanic` 的子类，由 `Kind::AnimalPanic` 按 `MobType`
            // 补上 `!isMobControlled` 和 `start` 里的 `standUpInstantly`。
            // `CamelPanic(4.0)`: an `AnimalPanic` subclass; `Kind::AnimalPanic` adds
            // `!isMobControlled` and `standUpInstantly` on start when `MobType::Camel`.
            Kind::AnimalPanic {
                speed: 4.0,
                environmental: false,
                armadillo: false,
                camel: true,
            },
            Kind::LookAtTargetSink { min: 45, max: 90 },
            Kind::MoveToTargetSink(MoveToTargetSink::new(false)),
            Kind::CountDownCooldown(MemoryType::TemptationCooldownTicks),
            Kind::CountDownCooldown(MemoryType::GazeCooldownTicks),
        ],
    )
}

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
                    partner: MobType::Camel,
                    speed: 1.0,
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
                                speed: Speed::fixed(2.5),
                                close_adult: 3.5,
                                close_baby: 2.5,
                            },
                            1,
                        ),
                        (
                            Kind::trigger_if(
                                super::brain::behavior::BodyPredicate::CamelNotRefuseToMove,
                                Kind::BabyFollowAdult {
                                    min: 5,
                                    max: 16,
                                    speed: Speed::fixed(2.5),
                                },
                            ),
                            1,
                        ),
                    ],
                ))),
            ),
            (
                3,
                Kind::RandomLookAround {
                    min_interval: 150,
                    max_interval: 250,
                    max_yaw: 30.0,
                    min_pitch: 0.0,
                    pitch_range: 0.0,
                },
            ),
            (
                4,
                Kind::Gate(Box::new(Gate::run_one(
                    vec![(MemoryType::WalkTarget, ValueAbsent)],
                    vec![
                        (
                            Kind::trigger_if(
                                super::brain::behavior::BodyPredicate::CamelNotRefuseToMove,
                                Kind::RandomStroll { speed: 2.0 },
                            ),
                            1,
                        ),
                        (
                            Kind::trigger_if(
                                super::brain::behavior::BodyPredicate::CamelNotRefuseToMove,
                                Kind::WalkTargetFromLook {
                                    speed: Speed::fixed(2.0),
                                    close_enough: 3,
                                },
                            ),
                            1,
                        ),
                        (Kind::Camel(Behavior::RandomSitting), 1),
                        (Kind::DoNothing { min: 30, max: 60 }, 1),
                    ],
                ))),
            ),
        ],
    )
}

/// `Camel.customServerAiStep`：`getBrain().tick(...)` 再 `updateActivity`（只留 `IDLE`）。
///
/// `Camel.customServerAiStep`: `getBrain().tick(...)` then `updateActivity` (`IDLE` only).
pub(super) fn custom_server_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if let Some(mut brain) = mob.brain.take() {
        brain.tick(me, mob, ctx);
        brain.set_active_activity_to_first_valid(&[Activity::Idle]);
        mob.brain = Some(brain);
    }
}

/// `finalizeSpawn` 里的 `resetLastPoseChangeTickToFullStand(gameTime)`（自然刷怪 / 刷怪蛋）。
///
/// `finalizeSpawn`'s `resetLastPoseChangeTickToFullStand(gameTime)` (natural / spawn-egg).
pub(super) fn finalize_pose(entity: &mut Entity, time: i64) {
    entity.with_mob(|_, mob| reset_last_pose_change_tick_to_full_stand(mob, time));
}

/// `readAdditionalSaveData`：读 `LastPoseTick`，负数设成坐姿。
///
/// `readAdditionalSaveData`: read `LastPoseTick`; a negative value means sitting.
pub(super) fn read_pose(mob: &mut Mob, pose_tick: i64) {
    set_pose(
        mob,
        if pose_tick < 0 {
            Pose::Sitting
        } else {
            Pose::Standing
        },
    );
    reset_last_pose_change_tick(mob, pose_tick);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 场景里的骆驼 `LAST_POSE_CHANGE_TICK = 0`：`gameTime` 小于是过渡、大于是站直。
    /// A scene camel has `LAST_POSE_CHANGE_TICK = 0`: transitioning early, standing later.
    #[test]
    fn initial_pose_transition_follows_game_time() {
        let mob = Mob::new(MobType::Camel);
        assert_eq!(mob.camel.last_pose_change_tick, 0);
        assert!(!is_camel_sitting(&mob));
        assert!(is_in_pose_transition(&mob, 0));
        assert!(refuse_to_move(&mob, 0));
        assert!(!refuse_to_move(&mob, 52));
        assert_eq!(pose_time(&mob, 52), 52);
    }

    /// 坐下过渡 40 刻、站起过渡 52 刻，符合同步值。
    /// Sitting-down transitions 40 ticks, standing-up 52, per the synced value.
    #[test]
    fn sit_and_stand_transitions() {
        let mut mob = Mob::new(MobType::Camel);
        reset_last_pose_change_tick(&mut mob, -100); // 坐着，100 刻前坐下
        assert!(is_camel_sitting(&mob));
        assert!(!is_in_pose_transition(&mob, 100 + 40));
        assert!(is_in_pose_transition(&mob, 100 + 39));
        reset_last_pose_change_tick(&mut mob, 100); // 站起
        assert!(!is_camel_sitting(&mob));
        assert!(is_in_pose_transition(&mob, 100 + 51));
        assert!(!is_in_pose_transition(&mob, 100 + 52));
    }

    /// `standUpInstantly` 把姿势计时推到满站，符号为正。
    /// `standUpInstantly` backdates to a full stand, with a positive sign.
    #[test]
    fn stand_up_instantly_reaches_full_stand() {
        let mut mob = Mob::new(MobType::Camel);
        reset_last_pose_change_tick(&mut mob, -200);
        stand_up_instantly(&mut mob, 300);
        assert_eq!(mob.camel.pose, Pose::Standing);
        assert!(!refuse_to_move(&mob, 300));
        assert_eq!(pose_time(&mob, 300), FULL_STAND_BACKDATE);
    }

    /// 大脑是 core + idle 两个活动，传感器四个，行为表按优先级排好。
    /// The brain is two activities (core, idle) with four sensors.
    #[test]
    fn camel_brain_activities_and_sensors() {
        let brain = brain();
        assert_eq!(brain.active_names(), vec!["core", "idle"]);
        assert_eq!(brain.sensors.len(), 4);
        // 两个活动：核心 6 个行为、闲逛 5 个优先级；没有别的活动。
        // Two activities: core (6 behaviours) and idle (5 priorities), nothing else.
        let activities = brain.table_activities();
        assert!(activities.contains(&Activity::Core));
        assert!(activities.contains(&Activity::Idle));
        assert!(
            activities
                .iter()
                .all(|a| matches!(a, Activity::Core | Activity::Idle))
        );
    }

    /// 尺寸：成年、成年坐姿、幼年、幼年坐姿。
    /// Dimensions: adult, adult sitting, baby, baby sitting.
    #[test]
    fn camel_dimensions_match_vanilla() {
        assert_eq!(dimensions(false, false), (1.7, 2.375, 2.275));
        assert_eq!(dimensions(true, false), (0.95, 1.4, 1.38));
        assert_eq!(dimensions(true, true), (0.95, 0.425, 0.41));
        let (_, height, eye) = dimensions(false, true);
        assert!((height - 0.945).abs() < 1e-6);
        assert_eq!(eye, 0.845);
    }

    /// `setPose` 换姿势时把动态眼高写回：坐下 0.845 / 站起 2.275，幼年另算。
    /// `setPose` writes back the live eye height: 0.845 sitting / 2.275 standing,
    /// with separate baby values.
    #[test]
    fn pose_change_updates_eye_height() {
        let mut mob = Mob::new(MobType::Camel);
        assert_eq!(mob.eye_height, 2.275);
        set_pose(&mut mob, Pose::Sitting);
        assert_eq!(mob.eye_height, 0.845);
        set_pose(&mut mob, Pose::Standing);
        assert_eq!(mob.eye_height, 2.275);
        mob.age = -24000;
        set_pose(&mut mob, Pose::Sitting);
        assert_eq!(mob.eye_height, 0.41);
    }
}
