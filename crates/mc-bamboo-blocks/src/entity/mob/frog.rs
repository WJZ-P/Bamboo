//! 青蛙：官方 `Frog`、`FrogAi` 和它的专属传感器 / 行为。第三种跑大脑的生物，六个活动
//! （core / idle / swim / lay_spawn / tongue / long_jump）和五个传感器。
//!
//! 长跳复用山羊那套 `LongJumpToRandomPos`（[`goat::JumpSpec`]），多一层
//! `LongJumpToPreferredBlock` 的偏好落点；移动控制是 `SmoothSwimmingMoveControl`，寻路是
//! `Frog.FrogPathNavigation`（两栖，见 `path.rs` 的 `frog` 开关）。
//!
//! Frogs: vanilla `Frog`, `FrogAi` and its own sensors / behaviours. The third
//! brain-driven mob, with six activities (core / idle / swim / lay_spawn / tongue /
//! long_jump) and five sensors.
//!
//! Long jumps reuse the goat's `LongJumpToRandomPos` ([`goat::JumpSpec`]) plus the
//! preferred landing blocks of `LongJumpToPreferredBlock`; the move control is
//! `SmoothSwimmingMoveControl` and the navigation `Frog.FrogPathNavigation` (amphibious,
//! see the `frog` flag in `path.rs`).

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{
    PathType,
    registries::attribute,
    tags::{block_tags, fluid_tags},
};

use super::{
    DamageKind, DamageSource, Mob, MobType,
    attributes::{Attribute, Attributes},
    brain::{
        Activity, ActivityData, Brain, Cx,
        behavior::{Class, Gate, Kind, MoveToTargetSink, Speed, Trigger},
        memory::{
            MemoryStatus::{self, Registered, ValueAbsent, ValuePresent},
            MemoryType::{self, *},
            MemoryValue, PositionTracker, WalkTarget,
        },
        sensor::{self, SensorKind},
    },
    goat::{self, JumpSpec, LongJump},
    living,
    path::static_path_type,
};
use crate::{
    SupportType,
    entity::{
        Entity, EntityUid, KindPublic,
        ctx::{Command, EntityCtx},
    },
    shape::{self, CollisionContext},
};

/// `EntityTypes.FROG`：0.5×0.5，眼高是缺省的 `height * 0.85F`。`canBeABaby` 为假，
/// 没有幼年尺寸。
///
/// `EntityTypes.FROG`: 0.5 by 0.5 with the default `height * 0.85F` eye. `canBeABaby`
/// is false, so there is no baby size.
pub const fn dimensions(_baby: bool) -> (f32, f32, f32) {
    (0.5, 0.5, 0.5 * 0.85)
}

/// `Frog.createAttributes`：动物那一套，速度 1.0、生命 10、攻击力 10、台阶高 1.0。
///
/// `Frog.createAttributes`: the animal set with 1.0 speed, 10 health, 10 attack damage
/// and a 1.0 step height.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    for (id, base) in [
        (attribute::MOVEMENT_SPEED, 1.0),
        (attribute::MAX_HEALTH, 10.0),
        (attribute::STEP_HEIGHT, 1.0),
    ] {
        // 改的是属性表的缺省值，不创建实例（实例会被存进存档）。
        // This changes the supplier default without creating an instance (instances are saved).
        table.set_base(id, base);
    }
    table
        .entries
        .push(Attribute::new(attribute::ATTACK_DAMAGE, 10.0));
    table
}

/// `FrogVariant`：`temperate` / `warm` / `cold`。数字是 `frog_variant` 注册表的网络 ID
/// （数据包按名字排序：cold 0、temperate 1、warm 2）。
///
/// `FrogVariant`: `temperate` / `warm` / `cold`. The numbers are the network ids of the
/// `frog_variant` registry (datapack entries sort by name: cold 0, temperate 1, warm 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variant {
    Cold,
    #[default]
    Temperate,
    Warm,
}

impl Variant {
    pub const ALL: [Self; 3] = [Self::Cold, Self::Temperate, Self::Warm];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Temperate => "temperate",
            Self::Warm => "warm",
            Self::Cold => "cold",
        }
    }

    /// 存档里带命名空间的名字；读的时候两种都认。
    ///
    /// The namespaced save name; reading accepts both forms.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Temperate => "minecraft:temperate",
            Self::Warm => "minecraft:warm",
            Self::Cold => "minecraft:cold",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        Self::ALL.into_iter().find(|variant| variant.name() == name)
    }

    pub const fn network_id(self) -> i32 {
        self as i32
    }
}

/// 青蛙会用到的四种 `Pose`。
///
/// The four `Pose`s a frog uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pose {
    #[default]
    Standing,
    Croaking,
    LongJumping,
    UsingTongue,
}

impl Pose {
    /// `Pose` 的网络 ID。
    ///
    /// The network id of the `Pose`.
    pub const fn network_id(self) -> i32 {
        match self {
            Self::Standing => 0,
            Self::LongJumping => 6,
            Self::Croaking => 8,
            Self::UsingTongue => 9,
        }
    }
}

/// 青蛙自己的字段：品种、姿势、`DATA_TONGUE_TARGET_ID`。
///
/// The frog's own fields: variant, pose and `DATA_TONGUE_TARGET_ID`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Frog {
    pub variant: Variant,
    pub pose: Pose,
    pub tongue_target: Option<EntityUid>,
}

/// `ShootTongue.State`。
///
/// `ShootTongue.State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TongueState {
    MoveToTarget,
    CatchAnimation,
    EatAnimation,
    #[default]
    Done,
}

/// `ShootTongue` 的字段。
///
/// The fields of `ShootTongue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tongue {
    eat_animation_timer: i32,
    calculate_path_counter: i32,
    state: TongueState,
}

/// `FrogAi` 里青蛙专属的行为。
///
/// The frog-only behaviours of `FrogAi`.
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    /// `Croak`（时长 100）。
    Croak { counter: i32 },
    /// `TryFindLand.create(range, speed)`；`next_ok` 是 `nextOkStartTime`。
    ///
    /// `TryFindLand.create(range, speed)`; `next_ok` is `nextOkStartTime`.
    TryFindLand {
        range: i32,
        speed: f32,
        next_ok: i64,
    },
    /// `StartAttacking.create(canAttack, NEAREST_ATTACKABLE)`。
    StartAttacking,
    /// `StopAttackingIfTargetInvalid.create()`。
    StopAttackingIfInvalid,
    /// `TryFindLandNearLiquid.create(8, 1.0F, FROG_TRIES_TO_FIND_LAND_NEAR)`。
    TryFindLandNearLiquid { next_ok: i64 },
    /// `TryLaySpawnOnFluidNearLand.create(FROGSPAWN)`。
    TryLaySpawn,
    /// `LongJumpMidJump(TIME_BETWEEN_LONG_JUMPS, FROG_STEP)`。
    MidJump,
    /// `LongJumpToPreferredBlock(TIME_BETWEEN_LONG_JUMPS, 2, 4, 3.5714288F, ...)`。
    LongJump(Box<LongJump>),
    /// `ShootTongue(FROG_TONGUE, FROG_EAT)`。
    ShootTongue(Tongue),
    /// `RandomStroll.swim(speed)`；蝌蚪也用。
    ///
    /// `RandomStroll.swim(speed)`; tadpoles use it too.
    SwimStroll { speed: f32 },
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
const TRY_LAY_SPAWN_REQUIRED: &[(MemoryType, MemoryStatus)] = &[
    (AttackTarget, ValueAbsent),
    (WalkTarget, ValuePresent),
    (IsPregnant, ValuePresent),
];
const STROLL_REQUIRED: &[(MemoryType, MemoryStatus)] = &[(WalkTarget, ValueAbsent)];

/// `FrogAi.TIME_BETWEEN_LONG_JUMPS`。
///
/// `FrogAi.TIME_BETWEEN_LONG_JUMPS`.
pub const LONG_JUMP_INTERVAL: (i32, i32) = (100, 140);

/// `FrogAi.initJumpActivity` 给 `LongJumpToPreferredBlock` 的实参。
///
/// The `LongJumpToPreferredBlock` arguments of `FrogAi.initJumpActivity`.
fn jump_spec() -> JumpSpec {
    let (width, height, _) = dimensions(false);
    JumpSpec {
        height: 2,
        width: 4,
        dimensions: (width, height),
        sound: "minecraft:entity.frog.long_jump",
        landing: acceptable_landing_spot,
        preferred: Some((block_tags::FROG_PREFER_JUMP_TO, 0.5)),
    }
}

impl Behavior {
    pub(super) fn class(&self) -> Class {
        let duration = match self {
            Self::Croak { .. } | Self::MidJump | Self::ShootTongue(_) => 100,
            Self::LongJump(_) => 200,
            Self::TryFindLand { .. }
            | Self::StartAttacking
            | Self::StopAttackingIfInvalid
            | Self::TryFindLandNearLiquid { .. }
            | Self::TryLaySpawn
            | Self::SwimStroll { .. } => return Class::OneShot,
        };
        Class::Timed {
            min: duration,
            max: duration,
        }
    }

    pub(super) fn required(&self) -> &'static [(MemoryType, MemoryStatus)] {
        match self {
            Self::Croak { .. } => CROAK_REQUIRED,
            Self::TryFindLand { .. } | Self::TryFindLandNearLiquid { .. } => TRY_FIND_LAND_REQUIRED,
            Self::StartAttacking => START_ATTACKING_REQUIRED,
            Self::StopAttackingIfInvalid => STOP_ATTACKING_REQUIRED,
            Self::TryLaySpawn => TRY_LAY_SPAWN_REQUIRED,
            Self::MidJump => MID_JUMP_REQUIRED,
            Self::LongJump(_) => LONG_JUMP_REQUIRED,
            Self::ShootTongue(_) => SHOOT_TONGUE_REQUIRED,
            Self::SwimStroll { .. } => STROLL_REQUIRED,
        }
    }

    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Croak { .. } => "Croak",
            Self::TryFindLand { .. }
            | Self::TryFindLandNearLiquid { .. }
            | Self::TryLaySpawn
            | Self::StartAttacking
            | Self::StopAttackingIfInvalid
            | Self::SwimStroll { .. } => "OneShot",
            Self::MidJump => "LongJumpMidJump",
            Self::LongJump(_) => "LongJumpToPreferredBlock",
            Self::ShootTongue(_) => "ShootTongue",
        }
    }

    pub(super) fn extra_start(&mut self, cx: &mut Cx) -> bool {
        match self {
            // `Croak.checkExtraStartConditions`。
            Self::Croak { .. } => cx.mob.frog.pose == Pose::Standing,
            Self::LongJump(_) => goat::long_jump_extra_start(cx, LONG_JUMP_INTERVAL),
            Self::ShootTongue(_) => tongue_extra_start(cx),
            _ => true,
        }
    }

    pub(super) fn start(&mut self, cx: &mut Cx) {
        match self {
            // `Croak.start`：不在液体里才切姿势、清计数器。
            // `Croak.start`: only out of liquids does it change pose and reset the counter.
            Self::Croak { counter } => {
                if !living::is_in_liquid(cx.me, cx.mob) {
                    cx.mob.frog.pose = Pose::Croaking;
                    *counter = 0;
                }
            }
            Self::MidJump => {
                cx.mob.discard_friction = true;
                cx.mob.frog.pose = Pose::LongJumping;
            }
            Self::LongJump(jump) => goat::long_jump_start(jump, cx, &jump_spec()),
            Self::ShootTongue(tongue) => tongue_start(tongue, cx),
            _ => {}
        }
    }

    pub(super) fn can_still_use(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::Croak { counter } => *counter < 60,
            Self::MidJump => !cx.me.on_ground,
            Self::LongJump(jump) => goat::long_jump_can_still_use(jump, cx, LONG_JUMP_INTERVAL),
            Self::ShootTongue(tongue) => {
                cx.memories.has(AttackTarget)
                    && tongue.state != TongueState::Done
                    && !cx.memories.has(IsPanicking)
            }
            _ => false,
        }
    }

    pub(super) fn tick(&mut self, cx: &mut Cx) {
        match self {
            Self::Croak { counter } => *counter += 1,
            Self::LongJump(jump) => goat::long_jump_tick(jump, cx, &jump_spec()),
            Self::ShootTongue(tongue) => tongue_tick(tongue, cx),
            _ => {}
        }
    }

    pub(super) fn stop(&mut self, cx: &mut Cx) {
        match self {
            Self::Croak { .. } => cx.mob.frog.pose = Pose::Standing,
            Self::MidJump => mid_jump_stop(cx),
            // `ShootTongue.stop`。
            Self::ShootTongue(_) => {
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
    pub(super) fn trigger(&mut self, cx: &mut Cx) -> bool {
        match self {
            Self::TryFindLand {
                range,
                speed,
                next_ok,
            } => try_find_land(cx, *range, *speed, next_ok),
            Self::StartAttacking => start_attacking(cx),
            Self::StopAttackingIfInvalid => stop_attacking_if_invalid(cx),
            Self::TryFindLandNearLiquid { next_ok } => try_find_land_near_liquid(cx, next_ok),
            Self::TryLaySpawn => try_lay_spawn(cx),
            // `RandomStroll.swim`：`canRun` 是 `Entity::isInWater`。
            // `RandomStroll.swim`: `canRun` is `Entity::isInWater`.
            Self::SwimStroll { speed } => {
                if !cx.me.in_water {
                    return false;
                }
                let pos = target_swim_pos(cx);
                cx.memories.set_or_erase(
                    WalkTarget,
                    pos.map(|pos| MemoryValue::Walk(WalkTarget::to_point(pos, *speed, 0))),
                );
                true
            }
            _ => false,
        }
    }
}

// ---- LongJumpMidJump / LongJumpToPreferredBlock ----

/// `LongJumpMidJump.stop`：落地时水平速度乘 `0.1F`、放落地声，恢复摩擦和姿势，清掉
/// 跳跃中的记忆，用世界随机源重取冷却。
///
/// `LongJumpMidJump.stop`: on landing scale the horizontal speed by `0.1F` and play the
/// landing sound; restore friction and pose, forget the mid-jump memory and redraw the
/// cooldown from the level random.
fn mid_jump_stop(cx: &mut Cx) {
    if cx.me.on_ground {
        let tenth = f64::from(0.1_f32);
        cx.me.delta = cx.me.delta.multiply(tenth, 1.0, tenth);
        goat::level_sound(cx, "minecraft:entity.frog.step", 2.0, 1.0);
    }
    cx.mob.discard_friction = false;
    cx.mob.frog.pose = Pose::Standing;
    cx.memories.erase(LongJumpMidJump);
    let (min, max) = LONG_JUMP_INTERVAL;
    let cooldown = cx.level_next_int(max - min + 1) + min;
    cx.memories
        .set(LongJumpCooldownTicks, MemoryValue::Int(cooldown));
}

/// `FrogAi.isAcceptableLandingSpot`：落点和上下两格都没有流体；落点或脚下是偏好方块就行；
/// 否则落点不是活板门（落点是空气时脚下也不是）才问缺省判定，是活板门直接算可以。
///
/// `FrogAi.isAcceptableLandingSpot`: no fluid at the spot or the cells above and below;
/// a preferred block there or underneath is enough; otherwise the default test applies
/// unless the spot is a trapdoor (or air above one), which is accepted outright.
fn acceptable_landing_spot(cx: &Cx, target: BlockPos) -> bool {
    let view = cx.ctx.blocks();
    let below = target.below();
    let state = view.block_state(target);
    let state_below = view.block_state(below);
    if !state.fluid().is_empty()
        || !state_below.fluid().is_empty()
        || !view.block_state(target.above()).fluid().is_empty()
    {
        return false;
    }
    if state.is_tagged(block_tags::FROG_PREFER_JUMP_TO)
        || state_below.is_tagged(block_tags::FROG_PREFER_JUMP_TO)
    {
        return true;
    }
    let path_type = static_path_type(view, target);
    let path_type_below = static_path_type(view, below);
    if path_type != PathType::Trapdoor && (!state.is_air() || path_type_below != PathType::Trapdoor)
    {
        goat::default_landing(cx, target)
    } else {
        true
    }
}

// ---- Croak 之外的声明式行为 / the declarative behaviours ----

/// `BlockPos.withinBoxByManhattanDistance(origin, reach, reach, reach)`：按曼哈顿距离从近到
/// 远，同一层里 x、y 升序，z 先正后负。`visit` 返回真就停。
///
/// `BlockPos.withinBoxByManhattanDistance(origin, reach, reach, reach)`: nearest first by
/// Manhattan distance; within a shell x and y ascend and z comes positive first, then
/// mirrored. Stops when `visit` returns true.
pub(super) fn within_box_by_manhattan(
    origin: BlockPos,
    reach: i32,
    visit: &mut dyn FnMut(BlockPos) -> bool,
) -> Option<BlockPos> {
    for depth in 0..=reach * 3 {
        let max_x = reach.min(depth);
        for x in -max_x..=max_x {
            let max_y = reach.min(depth - x.abs());
            for y in -max_y..=max_y {
                let z = depth - x.abs() - y.abs();
                if z > reach {
                    continue;
                }
                let pos = origin.offset(x, y, z);
                if visit(pos) {
                    return Some(pos);
                }
                if z != 0 {
                    let mirrored = origin.offset(x, y, -z);
                    if visit(mirrored) {
                        return Some(mirrored);
                    }
                }
            }
        }
    }
    None
}

/// `state.getCollisionShape(level, pos, CollisionContext.of(body)).isEmpty()`。
///
/// `state.getCollisionShape(level, pos, CollisionContext.of(body)).isEmpty()`.
fn no_collision_shape(cx: &Cx, pos: BlockPos) -> bool {
    let view = cx.ctx.blocks();
    let state = view.block_state(pos);
    shape::block_collision_boxes(
        state,
        pos,
        cx.me.mob_collision_context(cx.mob),
        view.moving_piston_shape(pos).as_deref(),
        view.shulker_box_shape(pos),
    )
    .is_empty()
}

fn set_land_target(cx: &mut Cx, pos: BlockPos, speed: f32, close_enough: i32) {
    cx.memories.set(
        LookTarget,
        MemoryValue::Look(PositionTracker::at_block(pos)),
    );
    cx.memories.set(
        WalkTarget,
        MemoryValue::Walk(WalkTarget {
            target: PositionTracker::at_block(pos),
            speed,
            close_enough,
        }),
    );
}

/// `TryFindLand`：身体那格是水才找；60 刻内再触发只续冷却。找曼哈顿距离最近的、不在
/// 自己这一列的、没有流体和碰撞箱、下面那格顶面完整的方块，走过去（走到 1 格内）。
///
/// `TryFindLand`: only with water at the body block; a retrigger within 60 ticks just
/// renews the cooldown. It walks (to within 1) to the Manhattan-nearest block of another
/// column that has no fluid or collision shape and stands on a sturdy top face.
fn try_find_land(cx: &mut Cx, range: i32, speed: f32, next_ok: &mut i64) -> bool {
    let here = cx.me.block_position();
    if !cx.ctx.blocks().block_state(here).fluid().is_water() {
        return false;
    }
    if cx.time < *next_ok {
        *next_ok = cx.time + 60;
        return true;
    }
    let found = within_box_by_manhattan(here, range, &mut |pos| {
        if pos.x == here.x && pos.z == here.z {
            return false;
        }
        let view = cx.ctx.blocks();
        view.block_state(pos).fluid().is_empty()
            && no_collision_shape(cx, pos)
            && view
                .block_state(pos.below())
                .is_face_sturdy(Direction::Up, SupportType::Full)
    });
    if let Some(pos) = found {
        set_land_target(cx, pos, speed, 1);
    }
    *next_ok = cx.time + 60;
    true
}

/// `TryFindLandNearLiquid.create(8, 1.0F, FROG_TRIES_TO_FIND_LAND_NEAR)`：不在那种流体里时，
/// 找最近的、没有碰撞箱、下面有碰撞箱、四邻里有一格是空气且其下是那种流体的方块。
///
/// `TryFindLandNearLiquid.create(8, 1.0F, FROG_TRIES_TO_FIND_LAND_NEAR)`: out of that
/// fluid, find the nearest block without a collision shape, with one underneath, and with
/// a horizontal neighbour that is air above that fluid.
fn try_find_land_near_liquid(cx: &mut Cx, next_ok: &mut i64) -> bool {
    let tag = fluid_tags::FROG_TRIES_TO_FIND_LAND_NEAR;
    let here = cx.me.block_position();
    if cx.ctx.blocks().block_state(here).fluid().is_tagged(tag) {
        return false;
    }
    if cx.time < *next_ok {
        *next_ok = cx.time + 40;
        return true;
    }
    let found = within_box_by_manhattan(here, 8, &mut |pos| {
        if pos.x == here.x && pos.z == here.z {
            return false;
        }
        if !no_collision_shape(cx, pos) || no_collision_shape(cx, pos.below()) {
            return false;
        }
        let view = cx.ctx.blocks();
        Direction::HORIZONTAL.into_iter().any(|direction| {
            let side = pos.relative(direction);
            view.block_state(side).is_air() && view.block_state(side.below()).fluid().is_tagged(tag)
        })
    });
    if let Some(pos) = found {
        set_land_target(cx, pos, 1.0, 0);
    }
    *next_ok = cx.time + 40;
    true
}

/// `TryLaySpawnOnFluidNearLand`：站在地上、不在水里时，看脚下那格的四邻：顶面没有碰撞、
/// 能托住蛙卵（流体或方块标签）、上面是空气，就在上面放蛙卵并忘掉怀孕。
///
/// `TryLaySpawnOnFluidNearLand`: on the ground and out of water, check the four
/// neighbours of the block underfoot: no collision on the top face, able to support
/// frogspawn (fluid or block tag) and air above — then place frogspawn there and forget
/// the pregnancy.
fn try_lay_spawn(cx: &mut Cx) -> bool {
    if cx.me.in_water || !cx.me.on_ground {
        return false;
    }
    let below = cx.me.block_position().below();
    for direction in Direction::HORIZONTAL {
        let side = below.relative(direction);
        let view = cx.ctx.blocks();
        let state = view.block_state(side);
        let boxes = shape::collision_boxes(state, side, CollisionContext::Empty);
        // `getFaceShape(Direction.UP)`：取 y = 0.9999999 处的截面。
        // `getFaceShape(Direction.UP)`: the cross-section at y = 0.9999999.
        let top_empty = !boxes
            .iter()
            .any(|part| part.min_y <= 0.999_999_9 && part.max_y > 0.999_999_9);
        let supports = state.fluid().is_tagged(fluid_tags::SUPPORTS_FROGSPAWN)
            || state.is_tagged(block_tags::SUPPORTS_FROGSPAWN);
        if top_empty && supports {
            let spawn_pos = side.above();
            if view.block_state(spawn_pos).is_air() {
                cx.ctx.emit(Command::LayFrogspawn { pos: spawn_pos }, cx.me);
                goat::level_sound(cx, "minecraft:entity.frog.lay_spawn", 1.0, 1.0);
                cx.memories.erase(IsPregnant);
                return true;
            }
        }
    }
    true
}

/// `StartAttacking.create(canAttack, NEAREST_ATTACKABLE)`：没在繁殖、有最近的猎物、
/// `canAttack(target)`，就记成攻击目标。
///
/// `StartAttacking.create(canAttack, NEAREST_ATTACKABLE)`: not breeding, a nearest prey
/// and `canAttack(target)` make it the attack target.
fn start_attacking(cx: &mut Cx) -> bool {
    if cx.memories.has(BreedTarget) {
        return false;
    }
    let Some(target) = cx.memories.entity(NearestAttackable) else {
        return false;
    };
    if !sensor::living(cx, target).is_some_and(|view| view.alive) {
        return false;
    }
    cx.memories.set(AttackTarget, MemoryValue::Entity(target));
    cx.memories.erase(CantReachWalkTargetSince);
    true
}

/// `StopAttackingIfTargetInvalid.create()`：目标还能打、没追烦（够不到超过 200 刻）、
/// 还活着就留着，否则清掉；两种情况都算触发。
///
/// `StopAttackingIfTargetInvalid.create()`: keep a target that is still attackable, not
/// given up on (unreachable for over 200 ticks) and alive, else erase it; it triggers
/// either way.
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
        cx.memories.erase(AttackTarget);
    }
    true
}

// ---- RandomStroll.swim ----

/// `RandomStroll.SWIM_XY_DISTANCE_TIERS`。
///
/// `RandomStroll.SWIM_XY_DISTANCE_TIERS`.
const SWIM_TIERS: [(i32, i32); 6] = [(1, 1), (3, 3), (5, 5), (6, 5), (7, 7), (10, 7)];

/// `RandomStroll.getTargetSwimPos`：先在最小一档抽一个能游的点，之后每一档把这个方向
/// 按档位放大；哪一档出了水（或抽不到）就用上一档的。
///
/// `RandomStroll.getTargetSwimPos`: draw a swimmable spot in the smallest tier, then
/// stretch that direction tier by tier; the first tier that leaves the water (or finds
/// nothing) falls back to the previous one.
pub(super) fn target_swim_pos(cx: &mut Cx) -> Option<Vec3> {
    let mut fallback: Option<Vec3> = None;
    let mut target = None;
    for (horizontal, vertical) in SWIM_TIERS {
        target = match fallback {
            None => super::fish::random_swimmable_pos(
                cx.me,
                cx.mob,
                cx.ctx.blocks(),
                horizontal,
                vertical,
            ),
            Some(previous) => {
                let here = cx.me.position;
                let step = previous.subtract(here).normalize().multiply(
                    f64::from(horizontal),
                    f64::from(vertical),
                    f64::from(horizontal),
                );
                Some(here.add(step))
            }
        };
        let Some(pos) = target else {
            return fallback;
        };
        let state = cx.ctx.blocks().block_state(BlockPos::containing(pos));
        if state.fluid().is_empty() {
            return fallback;
        }
        fallback = target;
    }
    target
}

// ---- ShootTongue ----

/// `Frog.canEat`：大小不是 1 的史莱姆类不吃，其余看 `#frog_food` 实体标签。
///
/// `Frog.canEat`: cube mobs of a size other than 1 are out; the rest goes by the
/// `#frog_food` entity tag.
fn can_eat(cx: &Cx, target: EntityUid) -> bool {
    let Some(other) = cx.ctx.other(target) else {
        return false;
    };
    let KindPublic::Mob(mob) = &other.kind else {
        return false;
    };
    if mob.mob_type == MobType::Slime && mob.cube_size != 1 {
        return false;
    }
    mob.mob_type.in_entity_tag("minecraft:frog_food")
}

/// `ShootTongue.addUnreachableTargetToMemory`：最多记 5 个，满了挤掉最早的；整张表 100 刻
/// 后过期。
///
/// `ShootTongue.addUnreachableTargetToMemory`: at most five, dropping the oldest; the
/// whole list expires after 100 ticks.
fn add_unreachable_target(cx: &mut Cx, target: EntityUid) {
    let mut list = cx
        .memories
        .entities(UnreachableTongueTargets)
        .map(<[EntityUid]>::to_vec)
        .unwrap_or_default();
    let should_add = !list.contains(&target);
    if list.len() == 5 && should_add {
        list.remove(0);
    }
    if should_add {
        list.push(target);
    }
    cx.memories.set_with_ttl(
        UnreachableTongueTargets,
        MemoryValue::Entities(list.into()),
        100,
    );
}

/// `ShootTongue.checkExtraStartConditions`：先看走不走得到（路径终点离目标不到 1.75），
/// 走不到就忘掉目标、记进够不到的名单。
///
/// `ShootTongue.checkExtraStartConditions`: first whether it can be reached (path end
/// within 1.75 of the target); if not, forget the target and list it as unreachable.
fn tongue_extra_start(cx: &mut Cx) -> bool {
    let Some(target) = cx.memories.entity(AttackTarget) else {
        return false;
    };
    // 独立寻路阶段：结果回来后按当时选定的目标格继续；等待期间别记进不可达名单。
    let pending_at = cx.mob.navigation.replay_target();
    if pending_at.is_none() && cx.mob.navigation.awaiting() {
        return false;
    }
    let reachable = sensor::living(cx, target).is_some_and(|view| {
        let at = pending_at.unwrap_or_else(|| BlockPos::containing(view.position));
        let nav = living::nav_mob(cx.me, cx.mob, cx.ctx.blocks());
        cx.mob
            .navigation
            .set_next_action(super::pathfinding::Action::Inspect(
                super::pathfinding::Use::FrogTongue,
            ));
        let path = cx.mob.navigation.create_path(cx.ctx.blocks(), &nav, at, 0);
        // 刚挂起请求：这一刻不算「够不到」。
        if cx.mob.navigation.awaiting() {
            return true;
        }
        path.is_some_and(|path| path.dist_to_target < 1.75)
    });
    if cx.mob.navigation.awaiting() {
        return false;
    }
    if !reachable {
        cx.memories.erase(AttackTarget);
        add_unreachable_target(cx, target);
    }
    reachable && cx.mob.frog.pose != Pose::Croaking && can_eat(cx, target)
}

fn walk_to_target(cx: &mut Cx, target: EntityUid) {
    if let Some(view) = sensor::living(cx, target) {
        cx.memories.set(
            WalkTarget,
            MemoryValue::Walk(WalkTarget::to_point(view.position, 2.0, 0)),
        );
    }
}

fn tongue_start(tongue: &mut Tongue, cx: &mut Cx) {
    let Some(target) = cx.memories.entity(AttackTarget) else {
        return;
    };
    // `BehaviorUtils.lookAtEntity`。
    cx.memories.set(
        LookTarget,
        MemoryValue::Look(PositionTracker::Entity {
            uid: target,
            track_eye: true,
            target_eye: false,
        }),
    );
    cx.mob.frog.tongue_target = Some(target);
    walk_to_target(cx, target);
    tongue.calculate_path_counter = 10;
    tongue.state = TongueState::MoveToTarget;
}

/// `ShootTongue.tick`：走近（每 10 刻重设一次走路目标），1.75 格内伸舌头把猎物拉过来，
/// 6 刻后吃掉，再过 10 刻收工。
///
/// `ShootTongue.tick`: approach (renewing the walk target every 10 ticks), shoot the
/// tongue within 1.75 blocks and pull the prey in, eat it 6 ticks later and finish 10
/// ticks after that.
fn tongue_tick(tongue: &mut Tongue, cx: &mut Cx) {
    let Some(target) = cx.memories.entity(AttackTarget) else {
        return;
    };
    cx.mob.frog.tongue_target = Some(target);
    match tongue.state {
        TongueState::MoveToTarget => {
            let view = sensor::living(cx, target);
            let here = cx.me.position;
            let close = view.is_some_and(|view| {
                // `Entity.distanceTo`：三个分量先转成 `float`。
                // `Entity.distanceTo`: the three components are cast to `float` first.
                let dx = (view.position.x - here.x) as f32;
                let dy = (view.position.y - here.y) as f32;
                let dz = (view.position.z - here.z) as f32;
                (dx * dx + dy * dy + dz * dz).sqrt() < 1.75
            });
            if close {
                goat::level_sound(cx, "minecraft:entity.frog.tongue", 2.0, 1.0);
                cx.mob.frog.pose = Pose::UsingTongue;
                if let Some(view) = view {
                    let delta = here.subtract(view.position).normalize().scale(0.75);
                    cx.ctx.emit(Command::SetMobDelta { target, delta }, cx.me);
                }
                tongue.eat_animation_timer = 0;
                tongue.state = TongueState::CatchAnimation;
            } else if tongue.calculate_path_counter <= 0 {
                walk_to_target(cx, target);
                tongue.calculate_path_counter = 10;
            } else {
                tongue.calculate_path_counter -= 1;
            }
        }
        TongueState::CatchAnimation => {
            let timer = tongue.eat_animation_timer;
            tongue.eat_animation_timer += 1;
            if timer >= 6 {
                tongue.state = TongueState::EatAnimation;
                eat_entity(cx);
            }
        }
        TongueState::EatAnimation => {
            if tongue.eat_animation_timer >= 10 {
                tongue.state = TongueState::Done;
            } else {
                tongue.eat_animation_timer += 1;
            }
        }
        TongueState::Done => {}
    }
}

/// `ShootTongue.eatEntity`：放吃东西的声音；舌头目标还活着就 `doHurtTarget`，打死了当场
/// `remove(KILLED)`。
///
/// `ShootTongue.eatEntity`: play the eating sound; a living tongue target takes
/// `doHurtTarget` and is `remove(KILLED)`-ed at once when that kills it.
fn eat_entity(cx: &mut Cx) {
    goat::level_sound(cx, "minecraft:entity.frog.eat", 2.0, 1.0);
    let Some(target) = cx.mob.frog.tongue_target else {
        return;
    };
    if !sensor::living(cx, target).is_some_and(|view| view.alive) {
        return;
    }
    let amount = cx.mob.attributes.value(attribute::ATTACK_DAMAGE) as f32;
    let mut source = DamageSource::environment(DamageKind::MobAttack).with_non_player_cause(true);
    source.attacker = Some(cx.me.id);
    source.direct = Some(cx.me.id);
    source.attacker_uuid = Some(cx.mob.uuid);
    source.position = Some(cx.me.position);
    source.attacker_type = Some(MobType::Frog.entity_type());
    cx.ctx.emit(
        Command::FrogEat {
            target,
            source,
            amount,
        },
        cx.me,
    );
}

/// 落地 [`Command::FrogEat`]：`doHurtTarget` 的伤害，目标因此死了就当场从世界里拿掉
/// （`remove(KILLED)`，没有 20 刻的死亡动画）。
///
/// Applies [`Command::FrogEat`]: the `doHurtTarget` damage, and a target that died of it
/// leaves the world at once (`remove(KILLED)`, no 20-tick death animation).
pub(crate) fn apply_eat(
    world: &mut crate::world::World,
    target: EntityUid,
    source: DamageSource,
    amount: f32,
) {
    world.hurt_mob(target, source, amount);
    // 只标记丢弃：轮到它 tick 时循环会把它拿掉，这一刻攒下的事件（死亡动画）照样发出去。
    // Only mark it discarded: the tick loop drops it on its turn and still sends the
    // events it queued this tick (the death animation).
    world.with_listed_entity_mut(target, &mut |entity| {
        if entity.kind.mob().is_some_and(|mob| mob.health <= 0.0) {
            entity.discard();
        }
    });
}

/// `Frog.getTongueTarget().isPresent()`：按 ID 在世界里还找得到那个实体。
///
/// `Frog.getTongueTarget().isPresent()`: the entity can still be found by id.
pub(super) fn has_tongue_target(mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    mob.frog
        .tongue_target
        .is_some_and(|uid| ctx.other(uid).is_some_and(|other| !other.removed))
}

// ---- 大脑 / the brain ----

/// `Frog.BRAIN_PROVIDER`：五个传感器（顺序照官方）和 `FrogAi.getActivities()`。
///
/// `Frog.BRAIN_PROVIDER`: five sensors in vanilla order and `FrogAi.getActivities()`.
pub(super) fn brain() -> Brain {
    use crate::entity::mob::brain::{BrainTemplate, TemplateCache, template_cache};
    static TEMPLATE: TemplateCache = template_cache();
    Brain::cached(&TEMPLATE, || {
        BrainTemplate::new(
            &[
                SensorKind::NearestLivingEntities,
                SensorKind::HurtBy,
                SensorKind::FrogAttackables,
                SensorKind::FrogTemptations,
                SensorKind::IsInWater,
            ],
            vec![
                core_activity(),
                idle_activity(),
                swim_activity(),
                lay_spawn_activity(),
                tongue_activity(),
                long_jump_activity(),
            ],
        )
    })
}

fn look_at_player_sometimes() -> Kind {
    Kind::LookAtPlayerSometimes {
        max_dist: 6.0,
        min_interval: 30,
        max_interval: 60,
        ticks_until_next_start: 0,
    }
}

fn follow_temptation() -> Kind {
    Kind::FollowTemptation {
        speed: Speed::fixed(1.25),
        close_adult: 2.5,
        close_baby: 2.5,
    }
}

fn walk_target_from_look(speed: f32) -> Kind {
    Kind::WalkTargetFromLook {
        speed: Speed::fixed(speed),
        close_enough: 3,
    }
}

/// `FrogAi.initCoreActivity`。
fn core_activity() -> ActivityData {
    ActivityData::sequential(
        Activity::Core,
        0,
        vec![
            Kind::AnimalPanic {
                speed: 2.0,
                environmental: false,
                armadillo: false,
                camel: false,
            },
            Kind::LookAtTargetSink { min: 45, max: 90 },
            Kind::MoveToTargetSink(MoveToTargetSink::new(false)),
            Kind::CountDownCooldown(TemptationCooldownTicks),
            Kind::CountDownCooldown(LongJumpCooldownTicks),
        ],
    )
}

/// `FrogAi.initIdleActivity`。
fn idle_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Idle,
        vec![
            (0, look_at_player_sometimes()),
            (
                0,
                Kind::MakeLove {
                    partner: MobType::Frog,
                    speed: 1.0,
                    close_enough: 2,
                    spawn_child_at: 0,
                },
            ),
            (1, follow_temptation()),
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
                        (walk_target_from_look(1.0), 1),
                        (Kind::Frog(Behavior::Croak { counter: 0 }), 3),
                        (Kind::TriggerIf(Trigger::OnGround), 2),
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

/// `FrogAi.initSwimActivity`。
fn swim_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::Swim,
        vec![
            (0, look_at_player_sometimes()),
            (1, follow_temptation()),
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
                        (Kind::RandomStroll { speed: 1.0 }, 1),
                        (walk_target_from_look(1.0), 1),
                        (Kind::TriggerIf(Trigger::InWater), 5),
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
fn lay_spawn_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::LaySpawn,
        vec![
            (0, look_at_player_sometimes()),
            (1, Kind::Frog(Behavior::StartAttacking)),
            (
                2,
                Kind::Frog(Behavior::TryFindLandNearLiquid { next_ok: 0 }),
            ),
            (3, Kind::Frog(Behavior::TryLaySpawn)),
            (
                4,
                Kind::Gate(Box::new(Gate::run_one(
                    Vec::new(),
                    vec![
                        (Kind::RandomStroll { speed: 1.0 }, 2),
                        (walk_target_from_look(1.0), 1),
                        (Kind::Frog(Behavior::Croak { counter: 0 }), 2),
                        (Kind::TriggerIf(Trigger::OnGround), 1),
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
fn long_jump_activity() -> ActivityData {
    ActivityData::with_priorities(
        Activity::LongJump,
        vec![
            (0, Kind::Frog(Behavior::MidJump)),
            (1, Kind::Frog(Behavior::LongJump(Box::default()))),
        ],
    )
    .when(vec![
        (TemptingPlayer, ValueAbsent),
        (BreedTarget, ValueAbsent),
        (LongJumpCooldownTicks, ValueAbsent),
        (IsInWater, ValueAbsent),
    ])
}

/// `FrogAi.initTongueActivity`：优先级从 0 起顺排，进入条件是有攻击目标。
///
/// `FrogAi.initTongueActivity`: priorities count up from 0; it needs an attack target.
fn tongue_activity() -> ActivityData {
    ActivityData::sequential(
        Activity::Tongue,
        0,
        vec![
            Kind::Frog(Behavior::StopAttackingIfInvalid),
            Kind::Frog(Behavior::ShootTongue(Tongue::default())),
        ],
    )
    .when(vec![(AttackTarget, ValuePresent)])
}

/// `FrogAi.initMemories`：用世界随机源抽长跳冷却。
///
/// `FrogAi.initMemories`: the long-jump cooldown from the level random.
pub(crate) fn init_memories(mob: &mut Mob, next_int: &mut dyn FnMut(i32) -> i32) {
    let Some(brain) = mob.brain.as_mut() else {
        return;
    };
    let (min, max) = LONG_JUMP_INTERVAL;
    let cooldown = next_int(max - min + 1) + min;
    brain
        .memories
        .set(LongJumpCooldownTicks, MemoryValue::Int(cooldown));
}

/// `FrogAttackablesSensor.doTick`：看得见的里面最近的一个——能吃、不在够不到的名单里、
/// 10 格内。`Sensor.isEntityAttackable` 比 `findClosest` 自带的视线测试只多一条
/// `canAttack`，对史莱姆类就是「活着」，已经含在视线测试里。
///
/// `FrogAttackablesSensor.doTick`: the nearest visible entity that is edible, not listed
/// as unreachable and within 10 blocks. `Sensor.isEntityAttackable` only adds `canAttack`
/// to the line-of-sight test `findClosest` already runs; for cube mobs that means alive,
/// which that test covers.
pub(super) fn sense_attackables(cx: &mut Cx) {
    let position = cx.me.position;
    let unreachable = cx
        .memories
        .entities(UnreachableTongueTargets)
        .map(<[EntityUid]>::to_vec)
        .unwrap_or_default();
    let edible: Vec<EntityUid> = match cx.memories.get(NearestVisibleLivingEntities) {
        Some(MemoryValue::Visible(visible)) => visible
            .entities
            .iter()
            .copied()
            .filter(|&uid| can_eat(cx, uid))
            .collect(),
        _ => Vec::new(),
    };
    let found = sensor::find_closest_visible(cx, &mut |view| {
        edible.contains(&view.uid)
            && !unreachable.contains(&view.uid)
            && position.distance_to_sqr(view.position) < 100.0
    });
    cx.memories
        .set_or_erase(NearestAttackable, found.map(MemoryValue::Entity));
}

/// `IsInWaterSensor.doTick`。
pub(super) fn sense_is_in_water(cx: &mut Cx) {
    if cx.me.in_water {
        cx.memories.set(IsInWater, MemoryValue::Unit);
    } else {
        cx.memories.erase(IsInWater);
    }
}

/// `Frog.customServerAiStep` 里 `super` 之前的部分：大脑和 `FrogAi.updateActivity`。
///
/// The part of `Frog.customServerAiStep` before `super`: the brain and
/// `FrogAi.updateActivity`.
pub(super) fn custom_server_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if let Some(mut brain) = mob.brain.take() {
        let gap = ctx.throttle_plan().decision_gap;
        brain.tick_by(me, mob, ctx, gap);
        brain.set_active_activity_to_first_valid(&[
            Activity::Tongue,
            Activity::LaySpawn,
            Activity::LongJump,
            Activity::Swim,
            Activity::Idle,
        ]);
        mob.brain = Some(brain);
    }
}

/// `SmoothSwimmingMoveControl.tick`（青蛙、蝌蚪和鱼都是 `(85, 10, 0.02F, 0.1F, true)`）。
///
/// `SmoothSwimmingMoveControl.tick` (frogs, tadpoles and fish all use `(85, 10, 0.02F,
/// 0.1F, true)`).
pub(super) fn move_control_tick(me: &mut Entity, mob: &mut Mob) {
    super::smooth_swim::move_tick(me, mob, &super::smooth_swim::FROG);
}

/// `Frog.travelInWater`：按当前速度推进、移动、乘 0.9；不沉也不浮。
///
/// `Frog.travelInWater`: thrust at the current speed, move and scale by 0.9; no sinking
/// or floating.
pub(super) fn travel_in_water(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    input: Vec3,
) {
    living::move_relative(me, mob.speed, input);
    let movement = me.delta;
    super::motion::move_living(me, mob, ctx, movement);
    me.delta = me.delta.scale(0.9);
}

/// `Frog.spawnChildFromBreeding` 里 `finalizeSpawnChildFromBreeding(level, partner, null)`
/// 落在世界上的部分：伴侣 6000 刻冷却、清求爱，再按 `mob_drops` 生成 `nextInt(7) + 1`
/// 的经验球。自己这一半（冷却、清求爱、怀孕记忆）在行为里已经做了。
///
/// The world half of `finalizeSpawnChildFromBreeding(level, partner, null)` inside
/// `Frog.spawnChildFromBreeding`: the partner's 6000-tick cooldown and love reset, then
/// one experience orb of `nextInt(7) + 1` when `mob_drops` is on. The frog's own half
/// (cooldown, love reset, pregnancy memory) already ran in the behaviour.
pub(super) fn breed(
    me: &mut Entity,
    partner: EntityUid,
    position: Vec3,
    world: &mut crate::world::World,
) {
    world.with_entity_mut(partner, &mut |entity| {
        entity.with_mob(|me, mob| {
            mob.in_love = 0;
            living::set_age(me, mob, 6000);
        });
    });
    if world.block_game_rules().mob_drops {
        let xp = me.random.next_int_bounded(7) + 1;
        world.spawn_entity(Entity::experience_orb(position, xp));
    }
}

/// `Frog` 构造函数：水的寻路代价 4、活板门不走，寻路换成 `FrogPathNavigation`。
///
/// The `Frog` constructor: water costs 4, trapdoors are off limits and the navigation is
/// `FrogPathNavigation`.
pub(super) fn configure(mob: &mut Mob) {
    mob.malus.set(PathType::Water, 4.0);
    mob.malus.set(PathType::Trapdoor, -1.0);
    mob.navigation.flags.amphibious = true;
    mob.navigation.flags.frog = true;
}

/// `Frog.finalizeSpawn` 在 `super` 之前的部分：按群系选品种（调用方给），再
/// `FrogAi.initMemories`。
///
/// The part of `Frog.finalizeSpawn` before `super`: the biome variant (picked by the
/// caller), then `FrogAi.initMemories`.
pub(super) fn finalize_spawn(
    mob: &mut Mob,
    variant: Variant,
    random: &dyn crate::random::RandomSource,
) {
    mob.frog.variant = variant;
    init_memories(mob, &mut |bound| random.next_int(bound));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 六个活动、五个传感器，初始活动是 core + idle。
    ///
    /// Six activities, five sensors and core + idle to begin with.
    #[test]
    fn frog_brain_has_vanilla_activities_and_sensors() {
        let brain = brain();
        let kinds: Vec<SensorKind> = brain.sensors.iter().map(|sensor| sensor.kind).collect();
        assert_eq!(
            kinds,
            [
                SensorKind::NearestLivingEntities,
                SensorKind::HurtBy,
                SensorKind::FrogAttackables,
                SensorKind::FrogTemptations,
                SensorKind::IsInWater,
            ]
        );
        assert_eq!(brain.active_names(), vec!["core", "idle"]);
        let mut activities = brain.table_activities();
        activities.sort_by_key(|activity| activity.name());
        activities.dedup();
        assert_eq!(activities.len(), 6);
    }

    /// 活动条件记忆都由某个传感器 / 行为登记，否则那个活动永远选不中。
    ///
    /// Every activity-condition memory is registered by a sensor or behaviour; otherwise
    /// the activity could never be picked.
    #[test]
    fn frog_activity_condition_memories_are_registered() {
        let brain = brain();
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
            assert!(brain.memories.is_registered(kind), "{}", kind.name());
        }
    }

    /// `withinBoxByManhattanDistance` 的顺序：原点，然后距离 1 的六格按 x、y 升序、z 先正后负。
    ///
    /// `withinBoxByManhattanDistance` order: the origin, then the six cells at distance 1
    /// with x and y ascending and z positive before negative.
    #[test]
    fn manhattan_box_iterates_like_vanilla() {
        let mut seen = Vec::new();
        within_box_by_manhattan(BlockPos::new(0, 0, 0), 1, &mut |pos| {
            seen.push((pos.x, pos.y, pos.z));
            seen.len() == 7
        });
        assert_eq!(
            seen,
            [
                (0, 0, 0),
                (-1, 0, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
                (0, 1, 0),
                (1, 0, 0),
            ]
        );
        let mut count = 0;
        within_box_by_manhattan(BlockPos::new(0, 0, 0), 2, &mut |_| {
            count += 1;
            false
        });
        assert_eq!(count, 125);
    }

    #[test]
    fn frog_attributes_and_variants_match_vanilla() {
        let table = attributes();
        assert_eq!(table.value(attribute::MAX_HEALTH), 10.0);
        assert_eq!(table.value(attribute::MOVEMENT_SPEED), 1.0);
        assert_eq!(table.value(attribute::ATTACK_DAMAGE), 10.0);
        assert_eq!(table.value(attribute::STEP_HEIGHT), 1.0);
        assert_eq!(Variant::from_name("minecraft:warm"), Some(Variant::Warm));
        assert_eq!(Variant::from_name("cold"), Some(Variant::Cold));
        assert_eq!(Variant::default(), Variant::Temperate);
    }
}
