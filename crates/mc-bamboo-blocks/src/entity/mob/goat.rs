//! 山羊：官方 `Goat` 和 `GoatAi`。第二种跑大脑的生物：核心、闲逛、长跳、冲撞四个活动，
//! 尖叫山羊、撞到会折角的方块掉角、挤奶。
//!
//! 长跳和冲撞的四个行为（`LongJumpMidJump`、`LongJumpToRandomPos`、`RamTarget`、
//! `PrepareRamNearestTarget`）也放在这里，逐条照官方类。
//!
//! Goats: vanilla `Goat` and `GoatAi`, the second brain-driven mob with core, idle, long
//! jump and ram activities, screaming goats, horns snapped by ramming blocks and milking.
//!
//! The four long-jump and ram behaviours (`LongJumpMidJump`, `LongJumpToRandomPos`,
//! `RamTarget`, `PrepareRamNearestTarget`) live here too, each following its vanilla class.

use mc_bamboo_core::{Aabb, BlockPos, Direction, Vec3, mth};
use mc_bamboo_data::{
    blocks, items,
    registries::{attribute, mob_effect},
    tags::block_tags,
};
use mc_bamboo_protocol::item::ItemStack;

use super::{
    DamageKind, DamageSource, Mob, MobEvent, MobType,
    attributes::{Attribute, Attributes},
    brain::{
        Activity, ActivityData, Brain, Cx,
        behavior::{Class, Gate, Kind, MoveToTargetSink},
        memory::{
            MemoryStatus::{self, Registered, ValueAbsent, ValuePresent},
            MemoryType, MemoryValue, PositionTracker, WalkTarget,
        },
        sensor::{self, LivingView, SensorKind},
    },
    equine::Voice,
    interact::{MobInteractRequest, MobInteraction},
    living,
    navigation::Navigation,
    path::static_path_type,
};
use crate::{
    InteractionResult,
    entity::{
        Entity, EntityUid, JavaRandom, KindPublic,
        ctx::{BlockView, Command},
    },
};

/// `EntityTypes.GOAT`：0.9×1.3，眼高缺省 `height * 0.85F`；幼年是 `BABY_DIMENSIONS`
/// （0.45×0.65，眼高 0.59375），`getDefaultDimensions` 不再乘年龄缩放。
///
/// `EntityTypes.GOAT`: 0.9 by 1.3 with the default `height * 0.85F` eye; babies use
/// `BABY_DIMENSIONS` (0.45 by 0.65, eye 0.59375) without the age scale.
pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.45, 0.65, 0.593_75)
    } else {
        (0.9, 1.3, 1.3 * 0.85)
    }
}

/// `Goat.getDefaultDimensions(Pose.LONG_JUMPING)`：整体乘 `0.7F`。
///
/// `Goat.getDefaultDimensions(Pose.LONG_JUMPING)`: everything times `0.7F`.
pub(super) fn pose_dimensions(baby: bool, long_jumping: bool) -> (f32, f32, f32) {
    let (width, height, eye) = dimensions(baby);
    if long_jumping {
        (width * 0.7, height * 0.7, eye * 0.7)
    } else {
        (width, height, eye)
    }
}

/// `Goat.setYHeadRot`：头相对身体最多偏 15 度，结果写成「身体朝向加夹好的差」。
/// 读档时 `Entity.load` 先 `setYHeadRot(yRot)` 再设身体朝向，所以按旧的身体朝向夹。
///
/// `Goat.setYHeadRot`: the head may turn at most 15 degrees from the body, stored as the
/// body yaw plus the clamped difference. `Entity.load` calls it before setting the body
/// yaw, so it clamps against the old body yaw.
pub(super) fn clamped_head_rot(y_body_rot: f32, y_head_rot: f32) -> f32 {
    let delta = mth::degrees_difference(y_body_rot, y_head_rot);
    y_body_rot + mth::clamp_f32(delta, -15.0, 15.0)
}

/// `Goat.createAttributes`：动物那一套（生命 10、速度 `0.2F`）加攻击力 2。
///
/// `Goat.createAttributes`: the animal set (10 health, `0.2F` speed) plus 2 attack damage.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table
        .entries
        .push(Attribute::new(attribute::ATTACK_DAMAGE, 2.0));
    table
}

/// 山羊自己的字段：三个同步数据、`Pose.LONG_JUMPING`。
///
/// The goat's own fields: its three synced booleans and `Pose.LONG_JUMPING`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Goat {
    pub screaming: bool,
    pub left_horn: bool,
    pub right_horn: bool,
    pub long_jumping: bool,
}

impl Default for Goat {
    fn default() -> Self {
        Self {
            screaming: false,
            left_horn: true,
            right_horn: true,
            long_jumping: false,
        }
    }
}

/// `TIME_BETWEEN_LONG_JUMPS`、`TIME_BETWEEN_RAMS`、`TIME_BETWEEN_RAMS_SCREAMER`。
///
/// `TIME_BETWEEN_LONG_JUMPS`, `TIME_BETWEEN_RAMS` and `TIME_BETWEEN_RAMS_SCREAMER`.
const LONG_JUMP_INTERVAL: (i32, i32) = (600, 1200);
const RAM_INTERVAL: (i32, i32) = (600, 6000);
const RAM_INTERVAL_SCREAMER: (i32, i32) = (100, 300);

/// `UniformInt.sample(level.getRandom())`。
///
/// `UniformInt.sample(level.getRandom())`.
fn sample_level(view: &dyn BlockView, (min, max): (i32, i32)) -> i32 {
    view.level_random_int(max - min + 1) + min
}

fn ram_interval(mob: &Mob) -> (i32, i32) {
    if mob.goat.screaming {
        RAM_INTERVAL_SCREAMER
    } else {
        RAM_INTERVAL
    }
}

/// `GoatAi.initMemories`：用世界随机源取长跳和冲撞的冷却，先长跳后冲撞。
///
/// `GoatAi.initMemories`: draws the long-jump and then the ram cooldown from the level random.
pub(crate) fn init_memories(mob: &mut Mob, next_int: &mut dyn FnMut(i32) -> i32) {
    let Some(brain) = mob.brain.as_mut() else {
        return;
    };
    let (min, max) = LONG_JUMP_INTERVAL;
    let jump = next_int(max - min + 1) + min;
    let (min, max) = RAM_INTERVAL;
    let ram = next_int(max - min + 1) + min;
    brain
        .memories
        .set(MemoryType::LongJumpCooldownTicks, MemoryValue::Int(jump));
    brain
        .memories
        .set(MemoryType::RamCooldownTicks, MemoryValue::Int(ram));
}

/// `Goat.ageBoundaryReached`：攻击力幼年 1、成年 2。`getAttribute` 会建出实例，
/// 所以之后存盘的 `attributes` 里总有攻击力这一项。
///
/// `Goat.ageBoundaryReached`: 1 attack damage for babies, 2 for adults. `getAttribute`
/// instantiates the attribute, so the saved `attributes` always list it afterwards.
pub fn age_boundary_reached(mob: &mut Mob) {
    let damage = if mob.is_baby() { 1.0 } else { 2.0 };
    if let Some(instance) = mob.attributes.instance(attribute::ATTACK_DAMAGE) {
        instance.base = damage;
    }
}

/// `Goat.BRAIN_PROVIDER`：六个传感器，核心、闲逛、长跳、冲撞四个活动。
///
/// `Goat.BRAIN_PROVIDER`: six sensors and the core, idle, long-jump and ram activities.
pub(super) fn brain() -> Brain {
    Brain::new(
        &[
            SensorKind::NearestLivingEntities,
            SensorKind::NearestPlayers,
            SensorKind::NearestItems,
            SensorKind::NearestAdult,
            SensorKind::HurtBy,
            SensorKind::FoodTemptations,
        ],
        vec![
            core_activity(),
            idle_activity(),
            long_jump_activity(),
            ram_activity(),
        ],
    )
}

fn core_activity() -> ActivityData {
    ActivityData::sequential(
        Activity::Core,
        0,
        vec![
            Kind::Swim { chance: 0.8 },
            Kind::AnimalPanic {
                speed: 2.0,
                environmental: false,
                armadillo: false,
            },
            Kind::LookAtTargetSink { min: 45, max: 90 },
            Kind::MoveToTargetSink(MoveToTargetSink::new(false)),
            Kind::CountDownCooldown(MemoryType::TemptationCooldownTicks),
            Kind::CountDownCooldown(MemoryType::LongJumpCooldownTicks),
            Kind::CountDownCooldown(MemoryType::RamCooldownTicks),
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
                0,
                Kind::MakeLove {
                    partner: MobType::Goat,
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
            (
                2,
                Kind::BabyFollowAdult {
                    min: 5,
                    max: 16,
                    speed: 1.25,
                },
            ),
            (
                3,
                Kind::Gate(Box::new(Gate::run_one(
                    Vec::new(),
                    vec![
                        (Kind::RandomStroll { speed: 1.0 }, 2),
                        (
                            Kind::WalkTargetFromLook {
                                speed: 1.0,
                                close_enough: 3,
                            },
                            2,
                        ),
                        (Kind::DoNothing { min: 30, max: 60 }, 1),
                    ],
                ))),
            ),
        ],
    )
    .when(vec![
        (MemoryType::RamCooldownTicks, ValuePresent),
        (MemoryType::LongJumpCooldownTicks, ValuePresent),
    ])
}

fn long_jump_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::LongJump,
        vec![
            (0, Kind::Goat(Behavior::MidJump)),
            (1, Kind::Goat(Behavior::LongJump(Box::default()))),
        ],
    )
    .when(vec![
        (MemoryType::TemptingPlayer, ValueAbsent),
        (MemoryType::BreedTarget, ValueAbsent),
        (MemoryType::WalkTarget, ValueAbsent),
        (MemoryType::LongJumpCooldownTicks, ValueAbsent),
    ])
}

fn ram_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Ram,
        vec![
            (
                0,
                Kind::Goat(Behavior::Ram {
                    direction: Vec3::new(0.0, 0.0, 0.0),
                }),
            ),
            (1, Kind::Goat(Behavior::PrepareRam(PrepareRam::default()))),
        ],
    )
    .when(vec![
        (MemoryType::TemptingPlayer, ValueAbsent),
        (MemoryType::BreedTarget, ValueAbsent),
        (MemoryType::RamCooldownTicks, ValueAbsent),
    ])
}

/// `LongJumpToRandomPos` 的状态。
///
/// The state of `LongJumpToRandomPos`.
#[derive(Debug, Clone, PartialEq)]
pub struct LongJump {
    candidates: Vec<(BlockPos, i32)>,
    initial_position: Option<Vec3>,
    chosen: Option<Vec3>,
    tries: i32,
    prepare_start: i64,
    /// `Collections.shuffle(allowedAngles)` 的随机源。官方用 `Collections` 里那个不带种子的
    /// 静态 `Random`；Bamboo 和 `RunOne` 的洗牌一样，出生时按生物随机源的状态播种。
    ///
    /// The random of `Collections.shuffle(allowedAngles)`. Vanilla uses the unseeded
    /// static `Random` inside `Collections`; Bamboo seeds it at birth from the mob's
    /// random state, like the `RunOne` shuffles.
    pub(super) angle_random: JavaRandom,
}

impl Default for LongJump {
    fn default() -> Self {
        Self {
            candidates: Vec::new(),
            initial_position: None,
            chosen: None,
            tries: 0,
            prepare_start: 0,
            angle_random: JavaRandom::new(0),
        }
    }
}

/// `PrepareRamNearestTarget.RamCandidate`。
///
/// `PrepareRamNearestTarget.RamCandidate`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RamCandidate {
    start: BlockPos,
    target_pos: BlockPos,
    target: EntityUid,
}

/// `PrepareRamNearestTarget` 的状态。官方 `stop` 不清候选，下一次启动找不到新目标时
/// 旧候选还在，这里照样保留。
///
/// The state of `PrepareRamNearestTarget`. Vanilla's `stop` keeps the candidate, so a
/// later start that finds no new target still uses the old one; kept as is.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PrepareRam {
    reached_at: Option<i64>,
    candidate: Option<RamCandidate>,
}

/// `GoatAi` 里的山羊专属行为。
///
/// The goat-only behaviours of `GoatAi`.
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// `LongJumpMidJump(TIME_BETWEEN_LONG_JUMPS, GOAT_STEP)`。
    MidJump,
    /// `LongJumpToRandomPos(TIME_BETWEEN_LONG_JUMPS, 5, 5, 3.5714288F, ...)`。
    LongJump(Box<LongJump>),
    /// `RamTarget`；`direction` 是 `ramDirection`。
    ///
    /// `RamTarget`; `direction` is `ramDirection`.
    Ram { direction: Vec3 },
    /// `PrepareRamNearestTarget(.., 4, 7, 1.25F, .., 20, ..)`。
    PrepareRam(PrepareRam),
}

const MID_JUMP_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (MemoryType::LookTarget, Registered),
    (MemoryType::LongJumpMidJump, ValuePresent),
];
const LONG_JUMP_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (MemoryType::LookTarget, Registered),
    (MemoryType::LongJumpCooldownTicks, ValueAbsent),
    (MemoryType::LongJumpMidJump, ValueAbsent),
];
const RAM_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (MemoryType::RamCooldownTicks, ValueAbsent),
    (MemoryType::RamTarget, ValuePresent),
];
const PREPARE_RAM_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (MemoryType::LookTarget, Registered),
    (MemoryType::RamCooldownTicks, ValueAbsent),
    (MemoryType::NearestVisibleLivingEntities, ValuePresent),
    (MemoryType::RamTarget, ValueAbsent),
];

/// `MAX_LONG_JUMP_WIDTH` / `MAX_LONG_JUMP_HEIGHT`、`MAX_JUMP_VELOCITY_MULTIPLIER`。
///
/// `MAX_LONG_JUMP_WIDTH` / `MAX_LONG_JUMP_HEIGHT` and `MAX_JUMP_VELOCITY_MULTIPLIER`.
const LONG_JUMP_RANGE: i32 = 5;
const MAX_JUMP_VELOCITY_MULTIPLIER: f32 = 3.571_428_8;
/// `RAM_MIN_DISTANCE`、`RAM_MAX_DISTANCE`、`RAM_PREPARE_TIME`。
///
/// `RAM_MIN_DISTANCE`, `RAM_MAX_DISTANCE` and `RAM_PREPARE_TIME`.
const RAM_MIN_DISTANCE: i32 = 4;
const RAM_MAX_DISTANCE: i32 = 7;
const RAM_PREPARE_TIME: i64 = 20;

impl Behavior {
    pub(super) fn class(&self) -> Class {
        let duration = match self {
            Self::MidJump => 100,
            Self::LongJump(_) | Self::Ram { .. } => 200,
            Self::PrepareRam(_) => 160,
        };
        Class::Timed {
            min: duration,
            max: duration,
        }
    }

    pub(super) fn required(&self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::MidJump => MID_JUMP_REQUIRED,
            Self::LongJump(_) => LONG_JUMP_REQUIRED,
            Self::Ram { .. } => RAM_REQUIRED,
            Self::PrepareRam(_) => PREPARE_RAM_REQUIRED,
        }
    }

    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::MidJump => "LongJumpMidJump",
            Self::LongJump(_) => "LongJumpToRandomPos",
            Self::Ram { .. } => "RamTarget",
            Self::PrepareRam(_) => "PrepareRamNearestTarget",
        }
    }

    pub(super) fn extra_start(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::LongJump(_) => long_jump_extra_start(cx),
            Self::Ram { .. } => cx.memories.has(MemoryType::RamTarget),
            Self::MidJump | Self::PrepareRam(_) => true,
        }
    }

    pub(super) fn start(&mut self, cx: &mut Cx) {
        match self {
            Self::MidJump => {
                cx.mob.discard_friction = true;
                set_long_jumping(cx, true);
            }
            Self::LongJump(jump) => long_jump_start(jump, cx),
            Self::Ram { direction } => {
                let Some(target) = cx.memories.vec3(MemoryType::RamTarget) else {
                    return;
                };
                let here = cx.me.block_position();
                *direction = Vec3::new(
                    f64::from(here.x) - target.x,
                    0.0,
                    f64::from(here.z) - target.z,
                )
                .normalize();
                cx.memories.set(
                    MemoryType::WalkTarget,
                    MemoryValue::Walk(WalkTarget::to_point(target, 3.0, 0)),
                );
            }
            Self::PrepareRam(prepare) => {
                if let Some(target) = closest_ram_target(cx) {
                    choose_ram_position(prepare, cx, target);
                }
            }
        }
    }

    pub(super) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::MidJump => !cx.me.on_ground,
            Self::LongJump(jump) => long_jump_can_still_use(jump, cx),
            Self::Ram { .. } => cx.memories.has(MemoryType::RamTarget),
            Self::PrepareRam(prepare) => prepare.candidate.is_some_and(|candidate| {
                sensor::living(cx, candidate.target).is_some_and(|view| view.alive)
            }),
        }
    }

    pub(super) fn tick(&mut self, cx: &mut Cx) {
        match self {
            Self::MidJump => {}
            Self::LongJump(jump) => long_jump_tick(jump, cx),
            Self::Ram { direction } => ram_tick(*direction, cx),
            Self::PrepareRam(prepare) => prepare_ram_tick(prepare, cx),
        }
    }

    pub(super) fn stop(&mut self, cx: &mut Cx) {
        match self {
            Self::MidJump => mid_jump_stop(cx),
            Self::LongJump(_) | Self::Ram { .. } => {}
            Self::PrepareRam(_) => {
                if !cx.memories.has(MemoryType::RamTarget) {
                    cx.mob.events.push(MobEvent::EntityEvent { event: 59 });
                    let (cooldown, _) = ram_interval(cx.mob);
                    cx.memories
                        .set(MemoryType::RamCooldownTicks, MemoryValue::Int(cooldown));
                }
            }
        }
    }
}

/// `level.playSound(null, body, sound, NEUTRAL, volume, pitch)`：不看 `isSilent`。
///
/// `level.playSound(null, body, sound, NEUTRAL, volume, pitch)`, which ignores `isSilent`.
fn level_sound(cx: &mut Cx, sound: &'static str, volume: f32, pitch: f32) {
    cx.mob.events.push(MobEvent::Sound {
        sound,
        volume,
        pitch,
        position: cx.me.position,
        hostile: false,
        seed: None,
    });
}

// ---- LongJumpMidJump ----

/// `setPose` 加 `refreshDimensions`：换尺寸和眼高；变大时 `fudgePositionAfterSizeChange`。
///
/// `setPose` plus `refreshDimensions`: swap size and eye height, and fudge the position
/// when growing.
fn set_long_jumping(cx: &mut Cx, long_jumping: bool) {
    if cx.mob.goat.long_jumping == long_jumping {
        return;
    }
    cx.mob.goat.long_jumping = long_jumping;
    refresh_dimensions(cx.me, cx.mob, cx.ctx.blocks());
}

/// `Entity.refreshDimensions`，按现在的年龄和姿势。
///
/// `Entity.refreshDimensions` for the current age and pose.
pub(crate) fn refresh_dimensions(me: &mut Entity, mob: &mut Mob, view: &dyn BlockView) {
    let old = me.dimensions;
    let (width, height, eye) = pose_dimensions(mob.is_baby(), mob.goat.long_jumping);
    me.dimensions = (width, height);
    mob.eye_height = eye;
    if !me.first_tick
        && (width > old.0 || height > old.1)
        && let Some(position) = crate::entity::thrown_egg::fudge_after_size_change(view, me, old)
    {
        me.position = position;
    }
}

/// `LongJumpMidJump.stop`：落地时水平速度乘 `0.1F`、放落地声，恢复摩擦和姿势，
/// 清掉跳跃中的记忆，用世界随机源重取冷却。
///
/// `LongJumpMidJump.stop`: on landing scale the horizontal speed by `0.1F` and play the
/// landing sound; restore friction and pose, forget the mid-jump memory and redraw the
/// cooldown from the level random.
fn mid_jump_stop(cx: &mut Cx) {
    if cx.me.on_ground {
        let tenth = f64::from(0.1_f32);
        cx.me.delta = cx.me.delta.multiply(tenth, 1.0, tenth);
        level_sound(cx, "minecraft:entity.goat.step", 2.0, 1.0);
    }
    cx.mob.discard_friction = false;
    set_long_jumping(cx, false);
    cx.memories.erase(MemoryType::LongJumpMidJump);
    let cooldown = sample_level(cx.ctx.blocks(), LONG_JUMP_INTERVAL);
    cx.memories.set(
        MemoryType::LongJumpCooldownTicks,
        MemoryValue::Int(cooldown),
    );
}

// ---- LongJumpToRandomPos ----

fn half_long_jump_cooldown(cx: &mut Cx) {
    let cooldown = sample_level(cx.ctx.blocks(), LONG_JUMP_INTERVAL) / 2;
    cx.memories.set(
        MemoryType::LongJumpCooldownTicks,
        MemoryValue::Int(cooldown),
    );
}

/// `checkExtraStartConditions`：着地、不在水和岩浆里、脚下那格不是蜂蜜块；否则设一半冷却。
///
/// `checkExtraStartConditions`: on the ground, out of water and lava and not inside a
/// honey block; otherwise half a cooldown.
fn long_jump_extra_start(cx: &mut Cx) -> bool {
    let can_start = cx.me.on_ground
        && !cx.me.in_water
        && !living::is_in_lava(cx.me, cx.mob)
        && !cx
            .ctx
            .blocks()
            .block_state(cx.me.block_position())
            .is(blocks::HONEY_BLOCK);
    if !can_start {
        half_long_jump_cooldown(cx);
    }
    can_start
}

/// `start`：20 次机会，记下起跳位置，把周围 11×11×11 里除自己那格以外的方块都列为候选，
/// 权重是到自己那格的距离平方；顺序是 `BlockPos.betweenClosed` 的 x、y、z。
///
/// `start`: 20 tries, remember the take-off position and list every block of the
/// surrounding 11×11×11 but the mob's own as a candidate weighted by its squared
/// distance, in `BlockPos.betweenClosed` order (x, then y, then z).
fn long_jump_start(jump: &mut LongJump, cx: &mut Cx) {
    jump.chosen = None;
    jump.tries = 20;
    jump.initial_position = Some(cx.me.position);
    let here = cx.me.block_position();
    jump.candidates.clear();
    for z in -LONG_JUMP_RANGE..=LONG_JUMP_RANGE {
        for y in -LONG_JUMP_RANGE..=LONG_JUMP_RANGE {
            for x in -LONG_JUMP_RANGE..=LONG_JUMP_RANGE {
                if (x, y, z) != (0, 0, 0) {
                    jump.candidates
                        .push((here.offset(x, y, z), x * x + y * y + z * z));
                }
            }
        }
    }
}

fn long_jump_can_still_use(jump: &mut LongJump, cx: &mut Cx) -> bool {
    let valid = jump.initial_position == Some(cx.me.position)
        && jump.tries > 0
        && !cx.me.in_water
        && (jump.chosen.is_some() || !jump.candidates.is_empty());
    if !valid && !cx.memories.has(MemoryType::LongJumpMidJump) {
        half_long_jump_cooldown(cx);
        cx.memories.erase(MemoryType::LookTarget);
    }
    valid
}

/// `tick`：选好了就等 40 刻起跳，没选好就再挑一轮。
///
/// `tick`: once a jump is chosen wait 40 ticks and leap; otherwise pick again.
fn long_jump_tick(jump: &mut LongJump, cx: &mut Cx) {
    let Some(chosen) = jump.chosen else {
        jump.tries -= 1;
        pick_candidate(jump, cx);
        return;
    };
    if cx.time - jump.prepare_start < 40 {
        return;
    }
    cx.me.y_rot = cx.mob.y_body_rot;
    cx.mob.discard_friction = true;
    let length = chosen.length();
    let boosted = length + f64::from(living::jump_boost_power(cx.mob));
    // 只是 `setDeltaMovement`，不置 `needsSync`。
    // Only `setDeltaMovement`; `needsSync` stays as it is.
    cx.me.delta = chosen.scale(boosted / length);
    cx.memories
        .set(MemoryType::LongJumpMidJump, MemoryValue::Bool(true));
    let sound = if cx.mob.goat.screaming {
        "minecraft:entity.goat.screaming.long_jump"
    } else {
        "minecraft:entity.goat.long_jump"
    };
    level_sound(cx, sound, 1.0, 1.0);
}

/// `pickCandidate`：按权重抽候选（世界随机源），直到抽到一个落点合格、算得出起跳速度、
/// 而且走不到的；每抽一个就从表里删掉。
///
/// `pickCandidate`: draw weighted candidates (level random) until one is an acceptable
/// landing spot with a jump vector that cannot be walked to; each draw leaves the list.
fn pick_candidate(jump: &mut LongJump, cx: &mut Cx) {
    while !jump.candidates.is_empty() {
        let total: i32 = jump.candidates.iter().map(|&(_, weight)| weight).sum();
        let mut selection = cx.level_next_int(total);
        let Some(index) = jump.candidates.iter().position(|&(_, weight)| {
            selection -= weight;
            selection < 0
        }) else {
            continue;
        };
        let (target, _) = jump.candidates.remove(index);
        if !acceptable_landing(cx, target) {
            continue;
        }
        let center = Vec3::new(
            f64::from(target.x) + 0.5,
            f64::from(target.y) + 0.5,
            f64::from(target.z) + 0.5,
        );
        let Some(vector) = optimal_jump_vector(jump, cx, center) else {
            continue;
        };
        cx.memories.set(
            MemoryType::LookTarget,
            MemoryValue::Look(PositionTracker::at_block(target)),
        );
        let nav = living::nav_mob(cx.me, cx.mob, cx.ctx.blocks());
        let path = cx
            .mob
            .navigation
            .create_path_within(cx.ctx.blocks(), &nav, target, 0, 8.0);
        if path.is_none_or(|path| !path.reached) {
            jump.chosen = Some(vector);
            jump.prepare_start = cx.time;
            return;
        }
    }
}

/// `isAcceptableLandingPosition` 加 `defaultAcceptableLandingSpot`：不在自己这一列，
/// 下面一格实心，落点的寻路代价是 0。
///
/// `isAcceptableLandingPosition` plus `defaultAcceptableLandingSpot`: another column,
/// solid below and a zero pathfinding malus.
fn acceptable_landing(cx: &Cx, target: BlockPos) -> bool {
    let here = cx.me.block_position();
    if here.x == target.x && here.z == target.z {
        return false;
    }
    let view = cx.ctx.blocks();
    view.block_state(target.below()).is_solid_render()
        && cx.mob.malus.get(static_path_type(view, target)) == 0.0
}

/// `calculateOptimalJumpVector`：把 65、70、75、80 四个角度洗牌，取第一个算得出来的。
///
/// `calculateOptimalJumpVector`: shuffle the angles 65, 70, 75 and 80 and take the first
/// that yields a vector.
fn optimal_jump_vector(jump: &mut LongJump, cx: &Cx, target: Vec3) -> Option<Vec3> {
    let mut angles = [65, 70, 75, 80];
    jump.angle_random.shuffle(&mut angles);
    let strength = cx.mob.attributes.value(attribute::JUMP_STRENGTH);
    let max_velocity = (strength * f64::from(MAX_JUMP_VELOCITY_MULTIPLIER)) as f32;
    angles
        .into_iter()
        .find_map(|angle| jump_vector_for_angle(cx, target, max_velocity, angle))
}

/// `LongJumpUtil.calculateJumpVectorForAngle(body, target, maxVelocity, angle, true)`。
///
/// `LongJumpUtil.calculateJumpVectorForAngle(body, target, maxVelocity, angle, true)`.
fn jump_vector_for_angle(cx: &Cx, target: Vec3, max_velocity: f32, angle: i32) -> Option<Vec3> {
    let from = cx.me.position;
    let plane = Vec3::new(target.x - from.x, 0.0, target.z - from.z)
        .normalize()
        .scale(0.5);
    let direction = target.subtract(plane).subtract(from);
    let radians = angle as f32 * std::f32::consts::PI / 180.0;
    let xz_angle = direction.z.atan2(direction.x);
    let r2 = direction
        .subtract(Vec3::new(0.0, direction.y, 0.0))
        .length_sqr();
    let r = r2.sqrt();
    let y = direction.y;
    let gravity = cx.mob.attributes.value(attribute::GRAVITY);
    let sin_double = f64::from(2.0 * radians).sin();
    let cos_squared = f64::from(radians).cos() * f64::from(radians).cos();
    let sin = f64::from(radians).sin();
    let cos = f64::from(radians).cos();
    let (sin_xz, cos_xz) = (xz_angle.sin(), xz_angle.cos());
    let v0_squared = r2 * gravity / (r * sin_double - 2.0 * y * cos_squared);
    if v0_squared < 0.0 {
        return None;
    }
    let v0 = v0_squared.sqrt();
    if v0 > f64::from(max_velocity) {
        return None;
    }
    let v0r = v0 * cos;
    let v0y = v0 * sin;
    let samples = mth::ceil(r / v0r) * 2;
    let (width, height, _) = pose_dimensions(cx.mob.is_baby(), true);
    let mut ri = 0.0;
    let mut previous: Option<Vec3> = None;
    for _ in 0..samples - 1 {
        ri += r / f64::from(samples);
        let yi = sin / cos * ri - ri * ri * gravity / (2.0 * v0_squared * (cos * cos));
        let sample = Vec3::new(from.x + ri * cos_xz, from.y + yi, from.z + ri * sin_xz);
        if let Some(previous) = previous
            && !clear_transition(cx, (width, height), previous, sample)
        {
            return None;
        }
        previous = Some(sample);
    }
    Some(Vec3::new(v0r * cos_xz, v0y, v0r * sin_xz).scale(f64::from(0.95_f32)))
}

/// `LongJumpUtil.isClearTransition`：沿两点连线按 0.9 倍的最小边长一步步放碰撞箱，
/// 最后一步正好落在终点。
///
/// `LongJumpUtil.isClearTransition`: step the box along the segment by 0.9 times its
/// smaller side, ending exactly on the second point.
fn clear_transition(cx: &Cx, (width, height): (f32, f32), from: Vec3, to: Vec3) -> bool {
    let direction = to.subtract(from);
    let smallest = f64::from(width).min(f64::from(height));
    let checks = mth::ceil(direction.length() / smallest);
    let unit = direction.normalize();
    let mut point = from;
    for index in 0..checks {
        point = if index == checks - 1 {
            to
        } else {
            point.add(unit.scale(smallest * f64::from(0.9_f32)))
        };
        if !no_collision(cx, box_at(point, width, height)) {
            return false;
        }
    }
    true
}

/// `EntityDimensions.makeBoundingBox(Vec3)`。
///
/// `EntityDimensions.makeBoundingBox(Vec3)`.
fn box_at(position: Vec3, width: f32, height: f32) -> Aabb {
    let half = f64::from(width / 2.0);
    Aabb::new(
        position.x - half,
        position.y,
        position.z - half,
        position.x + half,
        position.y + f64::from(height),
        position.z + half,
    )
}

/// `level.noCollision(body, box)`：没有方块碰撞箱，也不越出世界边界。
/// （硬碰撞实体——船、潜影贝——Bamboo 还没有。）
///
/// `level.noCollision(body, box)`: no block collision and no world-border collision.
/// (Hard-colliding entities such as boats and shulkers do not exist in Bamboo yet.)
fn no_collision(cx: &Cx, area: Aabb) -> bool {
    let view = cx.ctx.blocks();
    view.block_collisions(area, cx.me.mob_collision_context(cx.mob))
        .is_empty()
        && view
            .world_border()
            .no_border_collision(cx.me.position.x, cx.me.position.z, area)
}

// ---- PrepareRamNearestTarget / RamTarget ----

/// `RAM_TARGET_CONDITIONS.test(level, body, target)`：`forCombat()` 加选择器——不是山羊、
/// 碰撞箱在世界边界内。（盔甲架 Bamboo 还没有。）
///
/// `RAM_TARGET_CONDITIONS.test(level, body, target)`: `forCombat()` plus the selector —
/// not a goat and its box inside the world border. (Bamboo has no armour stands yet.)
fn ram_targetable(cx: &mut Cx, view: &LivingView) -> bool {
    if view.uid == cx.me.id || !view.alive || view.spectator {
        return false;
    }
    if view.mob_type == Some(MobType::Goat) {
        return false;
    }
    let blocks = cx.ctx.blocks();
    if !blocks
        .world_border()
        .is_within_bounds_aabb(view.bounding_box)
    {
        return false;
    }
    // `LivingEntity.canAttack`：和平难度不打玩家；`canBeSeenAsEnemy` 排除创造玩家。
    // `LivingEntity.canAttack`: no players on peaceful; `canBeSeenAsEnemy` rules out
    // creative players.
    if view.player && (blocks.difficulty() == 0 || view.creative) {
        return false;
    }
    let eye = view
        .position
        .add(Vec3::new(0.0, f64::from(view.eye_height), 0.0));
    living::has_line_of_sight(cx.me, cx.mob, cx.ctx.blocks(), view.uid, eye)
}

/// `NearestVisibleLivingEntities.findClosest(ramTargeting)`。
///
/// `NearestVisibleLivingEntities.findClosest(ramTargeting)`.
fn closest_ram_target(cx: &mut Cx) -> Option<EntityUid> {
    let list = match cx.memories.get(MemoryType::NearestVisibleLivingEntities) {
        Some(MemoryValue::Visible(visible)) => visible.entities.clone(),
        _ => return None,
    };
    list.into_iter().find(|&uid| {
        sensor::living(cx, uid).is_some_and(|view| ram_targetable(cx, &view))
            && sensor::visible_contains(cx, uid)
    })
}

/// `isWalkableBlock`：下面一格实心，寻路代价是 0。
///
/// `isWalkableBlock`: solid below and a zero pathfinding malus.
fn walkable_block(cx: &Cx, pos: BlockPos) -> bool {
    let view = cx.ctx.blocks();
    Navigation::is_stable_destination(view, pos)
        && cx.mob.malus.get(static_path_type(view, pos)) == 0.0
}

/// `calculateRammingStartPosition`：从目标那格往北、东、南、西各走最多 7 格能站的，
/// 留下离目标至少 4 格的，按离自己的距离排序，取第一个走得到的。
///
/// `calculateRammingStartPosition`: walk up to 7 standable blocks north, east, south and
/// west of the target, keep those at least 4 away, sort by distance to the goat and take
/// the first reachable one.
fn ramming_start_position(cx: &mut Cx, target_pos: BlockPos) -> Option<BlockPos> {
    if !walkable_block(cx, target_pos) {
        return None;
    }
    let mut possible: Vec<BlockPos> = Vec::new();
    for direction in [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ] {
        let mut cursor = target_pos;
        for _ in 0..RAM_MAX_DISTANCE {
            let next = cursor.relative(direction);
            if !walkable_block(cx, next) {
                break;
            }
            cursor = next;
        }
        let distance = (cursor.x - target_pos.x).abs()
            + (cursor.y - target_pos.y).abs()
            + (cursor.z - target_pos.z).abs();
        if distance >= RAM_MIN_DISTANCE {
            possible.push(cursor);
        }
    }
    let here = cx.me.block_position();
    let dist_sqr = |pos: &BlockPos| {
        let (dx, dy, dz) = (
            f64::from(pos.x - here.x),
            f64::from(pos.y - here.y),
            f64::from(pos.z - here.z),
        );
        dx * dx + dy * dy + dz * dz
    };
    possible.sort_by(|a, b| dist_sqr(a).total_cmp(&dist_sqr(b)));
    possible.into_iter().find(|&pos| {
        let nav = living::nav_mob(cx.me, cx.mob, cx.ctx.blocks());
        cx.mob
            .navigation
            .create_path(cx.ctx.blocks(), &nav, pos, 0)
            .is_some_and(|path| path.reached)
    })
}

fn choose_ram_position(prepare: &mut PrepareRam, cx: &mut Cx, target: EntityUid) {
    prepare.reached_at = None;
    prepare.candidate = sensor::living(cx, target).and_then(|view| {
        let target_pos = BlockPos::containing(view.position);
        ramming_start_position(cx, target_pos).map(|start| RamCandidate {
            start,
            target_pos,
            target,
        })
    });
}

/// `PrepareRamNearestTarget.tick`。
///
/// `PrepareRamNearestTarget.tick`.
fn prepare_ram_tick(prepare: &mut PrepareRam, cx: &mut Cx) {
    let Some(candidate) = prepare.candidate else {
        return;
    };
    cx.memories.set(
        MemoryType::WalkTarget,
        MemoryValue::Walk(WalkTarget {
            target: PositionTracker::at_block(candidate.start),
            speed: 1.25,
            close_enough: 0,
        }),
    );
    cx.memories.set(
        MemoryType::LookTarget,
        MemoryValue::Look(PositionTracker::Entity {
            uid: candidate.target,
            track_eye: true,
            target_eye: false,
        }),
    );
    let target_now =
        sensor::living(cx, candidate.target).map(|view| BlockPos::containing(view.position));
    if target_now.is_some_and(|pos| pos != candidate.target_pos) {
        cx.mob.events.push(MobEvent::EntityEvent { event: 59 });
        cx.mob.navigation.stop();
        choose_ram_position(prepare, cx, candidate.target);
        return;
    }
    let here = cx.me.block_position();
    if here != candidate.start {
        return;
    }
    cx.mob.events.push(MobEvent::EntityEvent { event: 58 });
    let reached_at = *prepare.reached_at.get_or_insert(cx.time);
    if cx.time - reached_at < RAM_PREPARE_TIME {
        return;
    }
    cx.memories.set(
        MemoryType::RamTarget,
        MemoryValue::Vec3(edge_of_block(here, candidate.target_pos)),
    );
    let sound = if cx.mob.goat.screaming {
        "minecraft:entity.goat.screaming.prepare_ram"
    } else {
        "minecraft:entity.goat.prepare_ram"
    };
    let pitch = living::voice_pitch(cx.me, cx.mob);
    level_sound(cx, sound, 1.0, pitch);
    prepare.candidate = None;
}

/// `getEdgeOfBlock`：目标那格的底面中心，朝远离自己的方向挪半格。
///
/// `getEdgeOfBlock`: the bottom centre of the target block moved half a block away from
/// the goat.
fn edge_of_block(start: BlockPos, target: BlockPos) -> Vec3 {
    let x_offset = 0.5 * f64::from((target.x - start.x).signum());
    let z_offset = 0.5 * f64::from((target.z - start.z).signum());
    Vec3::new(
        f64::from(target.x) + 0.5 + x_offset,
        f64::from(target.y),
        f64::from(target.z) + 0.5 + z_offset,
    )
}

/// `level.getNearbyEntities(LivingEntity.class, ramTargeting, body, body.getBoundingBox())`
/// 的第一个：碰撞箱和自己相交、过得了冲撞条件的活体。
///
/// The first of `level.getNearbyEntities(LivingEntity.class, ramTargeting, body,
/// body.getBoundingBox())`: a living entity touching the goat's box that passes the ram
/// conditions.
fn rammed_entity(cx: &mut Cx) -> Option<LivingView> {
    let area = cx.me.bounding_box();
    let mut touching: Vec<EntityUid> = Vec::new();
    cx.ctx.for_each_other_in(area, &mut |other| {
        if matches!(other.kind, KindPublic::Mob(_)) {
            touching.push(other.uid);
        }
    });
    cx.ctx.blocks().for_each_player(&mut |player| {
        touching.push(player.uid);
    });
    touching.into_iter().find_map(|uid| {
        let view = sensor::living(cx, uid)?;
        (view.bounding_box.intersects(area) && ram_targetable(cx, &view)).then_some(view)
    })
}

/// `RamTarget.tick`：撞到活体就打一下再击退；撞到会折角的方块就掉角；走到了或者
/// 走路目标没了就收工。
///
/// `RamTarget.tick`: hit and knock back a living entity, drop a horn against a
/// horn-snapping block, or finish once the walk target is reached or gone.
fn ram_tick(direction: Vec3, cx: &mut Cx) {
    if let Some(target) = rammed_entity(cx) {
        ram_entity(direction, cx, &target);
        finish_ram(cx);
        level_sound(cx, impact_sound(cx.mob), 1.0, 1.0);
    } else if rammed_horn_breaking_block(cx) {
        level_sound(cx, impact_sound(cx.mob), 1.0, 1.0);
        if drop_horn(cx) {
            level_sound(cx, "minecraft:entity.goat.horn_break", 1.0, 1.0);
        }
        finish_ram(cx);
    } else {
        let walk = cx.memories.walk_target();
        let ram = cx.memories.vec3(MemoryType::RamTarget);
        let done = match (walk, ram) {
            (Some(walk), Some(ram)) => super::brain::current_position(cx, walk.target)
                .is_none_or(|at| at.distance_to_sqr(ram) < 0.25 * 0.25),
            _ => true,
        };
        if done {
            finish_ram(cx);
        }
    }
}

fn impact_sound(mob: &Mob) -> &'static str {
    if mob.goat.screaming {
        "minecraft:entity.goat.screaming.ram_impact"
    } else {
        "minecraft:entity.goat.ram_impact"
    }
}

/// 撞到活体：`noAggroMobAttack` 伤害，再按速度和药水算力道击退。
/// （`applyItemBlocking` 的盾牌减半 Bamboo 还没有。）
///
/// Hitting a living entity: `noAggroMobAttack` damage, then a knockback scaled by speed
/// and potion levels. (Shield blocking, which halves it, is not in Bamboo yet.)
fn ram_entity(direction: Vec3, cx: &mut Cx, target: &LivingView) {
    let damage = cx.mob.attributes.value(attribute::ATTACK_DAMAGE) as f32;
    let mut source =
        DamageSource::environment(DamageKind::MobAttackNoAggro).with_non_player_cause(true);
    source.attacker = Some(cx.me.id);
    source.direct = Some(cx.me.id);
    source.attacker_uuid = Some(cx.mob.uuid);
    source.position = Some(cx.me.position);
    let level_of = |effect| {
        cx.mob
            .effects
            .amplifier(effect)
            .map_or(0, |amplifier| amplifier + 1)
    };
    let boost = 0.25_f32 * (level_of(mob_effect::SPEED) - level_of(mob_effect::SLOWNESS)) as f32;
    let speed_factor = mth::clamp_f32(cx.mob.speed * 1.65, 0.2, 3.0) + boost;
    let force = if cx.mob.is_baby() { 1.0 } else { 2.5 };
    let strength = f64::from(1.0_f32 * speed_factor) * force;
    cx.ctx.emit(
        Command::RamHit {
            target: target.uid,
            player: target.player,
            source,
            amount: damage,
            strength,
            dx: direction.x,
            dz: direction.z,
        },
        cx.me,
    );
}

/// 落地 [`Command::RamHit`]：先 `hurtServer`，再不管打没打中都 `knockback`。
///
/// Applies [`Command::RamHit`]: `hurtServer` first, then `knockback` whether or not the
/// hit landed.
pub(crate) fn apply_ram_hit(
    me: &Entity,
    world: &mut crate::world::World,
    target: EntityUid,
    player: bool,
    source: DamageSource,
    amount: f32,
    (strength, dx, dz): (f64, f64, f64),
) {
    if player {
        living::apply_hurt_player(world, me, target, source, amount);
        world.knock_back_player(target, strength, (dx, dz));
        return;
    }
    let _ = world.hurt_mob(target, source, amount);
    world.with_entity_mut(target, &mut |entity| {
        entity.with_mob(|me, mob| {
            // `LivingEntity.knockback` 只在扣掉击退抗性后还有力道时置 `needsSync`。
            // `LivingEntity.knockback` sets `needsSync` only when strength survives the
            // knockback resistance.
            let resisted = strength * (1.0 - mob.attributes.value(attribute::KNOCKBACK_RESISTANCE));
            living::knockback(me, mob, strength, dx, dz);
            if resisted > 0.0 {
                mob.needs_sync = true;
            }
        });
    });
}

/// `hasRammedHornBreakingBlock`：水平速度方向上前面一格或它上面一格在
/// `snaps_goat_horn` 里。
///
/// `hasRammedHornBreakingBlock`: the block one unit ahead along the horizontal motion, or
/// the one above it, is in `snaps_goat_horn`.
fn rammed_horn_breaking_block(cx: &Cx) -> bool {
    let ahead = cx.me.delta.multiply(1.0, 0.0, 1.0).normalize();
    let facing = BlockPos::containing(cx.me.position.add(ahead));
    let view = cx.ctx.blocks();
    view.block_state(facing)
        .is_tagged(block_tags::SNAPS_GOAT_HORN)
        || view
            .block_state(facing.above())
            .is_tagged(block_tags::SNAPS_GOAT_HORN)
}

/// `finishRam`：发 59 事件，用世界随机源取冲撞冷却，清掉冲撞目标。
///
/// `finishRam`: broadcast event 59, draw the ram cooldown from the level random and
/// forget the ram target.
fn finish_ram(cx: &mut Cx) {
    cx.mob.events.push(MobEvent::EntityEvent { event: 59 });
    let cooldown = sample_level(cx.ctx.blocks(), ram_interval(cx.mob));
    cx.memories
        .set(MemoryType::RamCooldownTicks, MemoryValue::Int(cooldown));
    cx.memories.erase(MemoryType::RamTarget);
}

/// `Goat.dropHorn`：幼年不掉；两只角都在时用自己的随机源挑一只，再取三个速度分量。
///
/// `Goat.dropHorn`: babies keep theirs; with both horns the goat's own random picks one,
/// then draws the three velocity components.
fn drop_horn(cx: &mut Cx) -> bool {
    if cx.mob.is_baby() {
        return false;
    }
    let goat = cx.mob.goat;
    if !goat.left_horn && !goat.right_horn {
        return false;
    }
    let left = if !goat.left_horn {
        false
    } else if !goat.right_horn {
        true
    } else {
        cx.me.random.next_boolean()
    };
    if left {
        cx.mob.goat.left_horn = false;
    } else {
        cx.mob.goat.right_horn = false;
    }
    let stack = create_horn(cx.mob);
    let mut between = |min: f32, max: f32| f64::from(min + cx.me.random.next_float() * (max - min));
    let delta = Vec3::new(between(-0.2, 0.2), between(0.3, 0.7), between(-0.2, 0.2));
    cx.ctx
        .emit(Command::DropItemWithDelta { stack, delta }, cx.me);
    true
}

/// `Goat.createHorn`：按 UUID 的 `hashCode` 播种一个随机源，从普通或尖叫的山羊角乐器
/// 标签里取一个。
///
/// `Goat.createHorn`: a random seeded with the UUID's `hashCode` picks an instrument from
/// the regular or screaming goat-horn tag.
pub(crate) fn create_horn(mob: &Mob) -> ItemStack {
    let most = i64::from_be_bytes(mob.uuid[..8].try_into().expect("八个字节"));
    let least = i64::from_be_bytes(mob.uuid[8..].try_into().expect("八个字节"));
    let mixed = most ^ least;
    let hash = ((mixed >> 32) as i32) ^ (mixed as i32);
    let mut random = JavaRandom::new(i64::from(hash));
    let tag = if mob.goat.screaming {
        "minecraft:screaming_goat_horns"
    } else {
        "minecraft:regular_goat_horns"
    };
    let mut stack = ItemStack::new(items::GOAT_HORN, 1);
    let ids = mc_bamboo_data::tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == "minecraft:instrument")
        .and_then(|group| group.tags.iter().find(|entry| entry.name == tag))
        .map_or(&[][..], |entry| entry.ids);
    if !ids.is_empty() {
        let id = ids[random.next_int_bounded(ids.len() as i32) as usize];
        crate::item_exchange::set_component(
            &mut stack,
            mc_bamboo_data::registries::data_component_type::INSTRUMENT,
            mc_bamboo_protocol::components::ComponentValue::Instrument(
                mc_bamboo_protocol::components::cursor::Holder::Id(id),
            ),
        );
    }
    stack
}

/// `Goat.getBreedOffspring` 在 `create` 之后的部分：用世界随机源 `initMemories`，再
/// `nextBoolean` 挑父母之一，它是尖叫山羊、或者 `nextDouble() < 0.02` 时幼崽尖叫。
///
/// The part of `Goat.getBreedOffspring` after `create`: `initMemories` from the level
/// random, then `nextBoolean` picks a parent; the kid screams if that parent does or if
/// `nextDouble() < 0.02`.
pub(crate) fn breed_offspring(
    kid: &mut Entity,
    father_screaming: bool,
    mother_screaming: bool,
    world: &mut crate::world::World,
) {
    kid.with_mob(|_, mob| {
        init_memories(mob, &mut |bound| world.random().next_int(bound));
        let parent = if world.random().next_boolean() {
            father_screaming
        } else {
            mother_screaming
        };
        mob.goat.screaming = parent || world.random().next_double() < 0.02;
    });
}

/// `Goat.finalizeSpawn` 在 `super` 之前的部分，随机源是 `level.getRandom()`：
/// `initMemories`、`nextDouble() < 0.02` 定尖叫、`ageBoundaryReached`，成年的再
/// `nextFloat() < 0.1F` 时用 `nextBoolean` 挑一只角去掉。
///
/// The part of `Goat.finalizeSpawn` before `super`, on `level.getRandom()`:
/// `initMemories`, `nextDouble() < 0.02` for screaming, `ageBoundaryReached`, and for
/// adults `nextFloat() < 0.1F` removes the horn `nextBoolean` picks.
pub(super) fn finalize_spawn(mob: &mut Mob, random: &dyn crate::random::RandomSource) {
    init_memories(mob, &mut |bound| random.next_int(bound));
    mob.goat.screaming = random.next_double() < 0.02;
    age_boundary_reached(mob);
    if !mob.is_baby() && random.next_float() < 0.1 {
        if random.next_boolean() {
            mob.goat.left_horn = false;
        } else {
            mob.goat.right_horn = false;
        }
    }
}

/// `Goat.customServerAiStep` 里 `super` 之前的部分：大脑和换活动。
///
/// The part of `Goat.customServerAiStep` before `super`: the brain and the activity update.
pub(super) fn custom_server_ai_step(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn crate::entity::ctx::EntityCtx,
) {
    if let Some(mut brain) = mob.brain.take() {
        brain.tick(me, mob, ctx);
        brain.set_active_activity_to_first_valid(&[
            Activity::Ram,
            Activity::LongJump,
            Activity::Idle,
        ]);
        mob.brain = Some(brain);
    }
}

/// `Goat.mobInteract` 的挤奶：成年山羊加空桶。声音由玩家发出。
///
/// The milking half of `Goat.mobInteract`: an adult goat and an empty bucket; the player
/// makes the sound.
pub(super) fn interact(mob: &mut Mob, req: &MobInteractRequest) -> Option<MobInteraction> {
    if req.held.item != items::BUCKET || mob.is_baby() {
        return None;
    }
    let sound = if mob.goat.screaming {
        "minecraft:entity.goat.screaming.milk"
    } else {
        "minecraft:entity.goat.milk"
    };
    mob.events.push(MobEvent::PlayerSound {
        sound,
        position: req.position,
    });
    Some(MobInteraction {
        result: InteractionResult::Success,
        shrink_held: !req.creative,
        replacement: Some(ItemStack::new(items::MILK_BUCKET, 1)),
        open_inventory: false,
        shear: false,
    })
}

/// 一次喂食要放几遍 `Goat.playEatingSound`：`Animal.mobInteract` 吃下时放一遍；
/// `Goat.mobInteract` 随后再看手里那一摞（`usePlayerItem` 之后的），还是山羊食物——
/// 创造模式不扣、或者原来不止一个——就再放一遍。
///
/// How many times one feeding plays `Goat.playEatingSound`: once inside
/// `Animal.mobInteract`, then `Goat.mobInteract` checks the held stack after
/// `usePlayerItem` and plays it again while it is still goat food (creative, or more than
/// one item to begin with).
pub(crate) fn eating_sounds(
    entity: &Entity,
    req: &MobInteractRequest,
    outcome: &MobInteraction,
) -> usize {
    let ate = entity.kind.mob().is_some_and(|mob| {
        mob.mob_type == MobType::Goat
            && outcome.replacement.is_none()
            && outcome.result.consumes_action()
            && super::interact::is_food(MobType::Goat, req.held.item)
    });
    if !ate {
        return 0;
    }
    let still_food = !outcome.shrink_held || req.held.count > 1;
    1 + usize::from(still_food)
}

/// `Goat.playEatingSound`：`level().playSound(null, this, EAT, NEUTRAL, 1,
/// Mth.randomBetween(level().getRandom(), 0.8F, 1.2F))`。
///
/// `Goat.playEatingSound`, pitched with the level random.
pub(crate) fn play_eating_sound(me: &Entity, mob: &mut Mob, level_float: f32) {
    let sound = if mob.goat.screaming {
        "minecraft:entity.goat.screaming.eat"
    } else {
        "minecraft:entity.goat.eat"
    };
    mob.events.push(MobEvent::Sound {
        sound,
        volume: 1.0,
        pitch: 0.8 + level_float * (1.2 - 0.8),
        position: me.position,
        hostile: false,
        seed: None,
    });
}

/// 山羊的叫声：尖叫山羊各有一套。
///
/// Goat voices; screaming goats have their own set.
pub(super) fn voice(mob: &Mob, voice: Voice) -> &'static str {
    match (voice, mob.goat.screaming) {
        (Voice::Hurt, false) => "minecraft:entity.goat.hurt",
        (Voice::Hurt, true) => "minecraft:entity.goat.screaming.hurt",
        (Voice::Death, false) => "minecraft:entity.goat.death",
        (Voice::Death, true) => "minecraft:entity.goat.screaming.death",
        (_, false) => "minecraft:entity.goat.ambient",
        (_, true) => "minecraft:entity.goat.screaming.ambient",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 普通山羊角从 `regular_goat_horns` 里取，带 `instrument` 组件；同一个 UUID 总取同一个。
    ///
    /// A regular horn comes from `regular_goat_horns` with an `instrument` component, and
    /// one UUID always picks the same one.
    #[test]
    fn horn_follows_the_uuid() {
        let mut mob = Mob::new(MobType::Goat);
        mob.uuid = [7; 16];
        let first = create_horn(&mob);
        assert_eq!(first.item, items::GOAT_HORN);
        assert_eq!(first, create_horn(&mob));
        assert!(!first.added.is_empty());
        mob.goat.screaming = true;
        assert!(!create_horn(&mob).added.is_empty());
    }

    /// 长跳姿势下尺寸乘 0.7，幼年用自己的尺寸。
    ///
    /// The long-jump pose scales by 0.7; babies use their own size.
    #[test]
    fn long_jump_pose_shrinks() {
        let (width, height, _) = pose_dimensions(false, true);
        assert_eq!(width, 0.9_f32 * 0.7);
        assert_eq!(height, 1.3_f32 * 0.7);
        assert_eq!(dimensions(true), (0.45, 0.65, 0.593_75));
    }

    /// 喂一个小麦：`usePlayerItem` 之后手里空了只放一遍吃东西的声音；创造模式或者原来
    /// 不止一个时 `Goat.mobInteract` 再放一遍；不是食物不放。
    ///
    /// Feeding one wheat plays the eating sound once when the hand ends up empty; in
    /// creative or with more than one item `Goat.mobInteract` plays it again; non-food
    /// plays nothing.
    #[test]
    fn eating_sounds_follow_the_stack() {
        let goat = Entity::by_mob_type(MobType::Goat, Vec3::new(0.0, 64.0, 0.0));
        let request = |count: i32, creative: bool, item: i32| MobInteractRequest {
            player_uid: 1,
            player_uuid: [0; 16],
            hand: mc_bamboo_core::Hand::Main,
            held: ItemStack::new(item, count),
            creative,
            position: Vec3::new(1.0, 64.0, 0.0),
            sneaking: false,
        };
        let fed = |creative: bool| MobInteraction {
            result: InteractionResult::SuccessServer,
            shrink_held: !creative,
            replacement: None,
            open_inventory: false,
            shear: false,
        };
        assert_eq!(
            eating_sounds(&goat, &request(1, false, items::WHEAT), &fed(false)),
            1
        );
        assert_eq!(
            eating_sounds(&goat, &request(5, false, items::WHEAT), &fed(false)),
            2
        );
        assert_eq!(
            eating_sounds(&goat, &request(1, true, items::WHEAT), &fed(true)),
            2
        );
        assert_eq!(
            eating_sounds(&goat, &request(1, false, items::STICK), &fed(false)),
            0
        );
    }

    /// 读档时头相对旧的身体朝向夹在 ±15 度内。
    ///
    /// On load the head is clamped to ±15 degrees around the old body yaw.
    #[test]
    fn head_clamps_around_the_body() {
        assert_eq!(clamped_head_rot(0.0, 10.0), 10.0);
        assert_eq!(clamped_head_rot(0.0, 90.0), 15.0);
        assert_eq!(clamped_head_rot(0.0, 350.0), -10.0);
    }

    /// 冲撞终点在目标那格远离山羊的一侧。
    ///
    /// The ram target sits on the far side of the target block.
    #[test]
    fn edge_is_past_the_target() {
        let edge = edge_of_block(BlockPos::new(0, 64, 0), BlockPos::new(5, 64, 0));
        assert_eq!(edge, Vec3::new(6.0, 64.0, 0.5));
    }
}
