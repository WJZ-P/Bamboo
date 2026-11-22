//! 幻翼：官方 `Phantom`（`Mob implements Enemy`，不是 `Monster` 也不寻路）。
//!
//! 四个目标只写 `moveTargetPoint`、`anchorPoint` 和攻击阶段，`PhantomMoveControl` 每刻
//! 把朝向和速度往目标点掰，`travel` 是 `travelFlying(input, 0.2F)`（没有重力）。白天
//! 着火走 `#burn_in_daylight` 的公共路径。
//!
//! Phantoms: vanilla `Phantom` (a `Mob implements Enemy`, neither a `Monster` nor a
//! path-finder). Its four goals only write `moveTargetPoint`, `anchorPoint` and the attack
//! phase; `PhantomMoveControl` steers rotation and velocity at the target point each tick
//! and `travel` is `travelFlying(input, 0.2F)` with no gravity. Daylight burning uses the
//! shared `#burn_in_daylight` path.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{entity_types, registries::attribute};

use super::{
    Mob, MobType,
    attributes::Attributes,
    goals::{self, Flag, Goal, GoalSelector, WrappedGoal, reduced_tick_delay},
    living, motion,
};
use crate::{
    entity::{
        Entity,
        ctx::{Command, EntityCtx, PlayerView},
    },
    random::RandomSource,
};

/// `Mth.DEG_TO_RAD`：`(float) (Math.PI / 180.0)`。
const DEG_TO_RAD: f32 = std::f32::consts::PI / 180.0;
/// `Mth.RAD_TO_DEG`：`180.0F / (float) Math.PI`。
const RAD_TO_DEG: f32 = 180.0 / std::f32::consts::PI;
/// `(float) Math.PI`。
const PI: f32 = std::f32::consts::PI;
/// `LevelEvent.SOUND_PHANTOM_BITE`。
const SOUND_PHANTOM_BITE: i32 = 1039;
/// `PhantomAttackPlayerTargetGoal.attackTargeting`：`forCombat().range(64.0)`。
const SCAN_RANGE: f64 = 64.0;
/// `setPhantomSize` 的上限。
///
/// The `setPhantomSize` cap.
pub(super) const MAX_SIZE: i32 = 64;

/// 幻翼专属的状态。
///
/// Phantom-only state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhantomState {
    /// `Phantom.ID_SIZE`（0 到 64）。
    pub size: i32,
    /// `Phantom.moveTargetPoint`，不存档。
    ///
    /// `Phantom.moveTargetPoint`; not saved.
    pub move_target: Vec3,
    /// `Phantom.anchorPoint`（存档键 `anchor_pos`）。
    pub anchor: Option<BlockPos>,
    /// `Phantom.attackPhase == SWOOP`，不存档。
    ///
    /// `Phantom.attackPhase == SWOOP`; not saved.
    pub swooping: bool,
    /// `PhantomMoveControl.speed`，不存档。
    ///
    /// `PhantomMoveControl.speed`; not saved.
    pub speed: f32,
}

impl Default for PhantomState {
    fn default() -> Self {
        Self {
            size: 0,
            move_target: Vec3::default(),
            anchor: None,
            swooping: false,
            speed: 0.1,
        }
    }
}

/// `Phantom.getDefaultDimensions`：类型表的尺寸乘 `1 + 0.15 * size`，眼高一起缩放。
///
/// `Phantom.getDefaultDimensions`: the type's size scaled by `1 + 0.15 * size`, eye
/// height included.
pub(super) fn dimensions(size: i32) -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[MobType::Phantom.entity_type() as usize];
    let scale = 1.0 + 0.15 * size as f32;
    (facts.width * scale, facts.height * scale, 0.175 * scale)
}

/// `Monster.createMonsterAttributes()`：`Mob` 的缺省属性加攻击力（缺省 2）。
///
/// `Monster.createMonsterAttributes()`: the mob defaults plus attack damage (2).
pub(super) fn attributes() -> Attributes {
    Attributes::monster()
}

/// `Phantom.setPhantomSize` 加 `onSyncedDataUpdated`：值变了才 `updatePhantomSizeInfo`
/// （换尺寸，攻击力基础值改成 `6 + size`）。大小 0 的幻翼因此一直是 2 点攻击力。
///
/// `Phantom.setPhantomSize` with `onSyncedDataUpdated`: only a changed value runs
/// `updatePhantomSizeInfo` (new dimensions, attack damage base `6 + size`). A size-0
/// phantom therefore keeps its 2 attack damage.
pub(crate) fn set_size(me: &mut Entity, mob: &mut Mob, size: i32) {
    let size = size.clamp(0, MAX_SIZE);
    if mob.phantom.size == size {
        return;
    }
    mob.phantom.size = size;
    let (width, height, eye) = dimensions(size);
    me.dimensions = (width, height);
    mob.eye_height = eye;
    mob.attributes
        .set_base(attribute::ATTACK_DAMAGE, f64::from(6 + size));
}

/// `Phantom.registerGoals` 的 `goalSelector`：1 攻击策略，2 俯冲，3 绕锚点盘旋。
///
/// The `goalSelector` of `Phantom.registerGoals`: attack strategy at 1, the sweep at 2 and
/// circling the anchor at 3.
pub(super) fn goals() -> GoalSelector {
    selector(vec![
        (1, PhantomGoal::AttackStrategy { next_sweep_tick: 0 }),
        (
            2,
            PhantomGoal::SweepAttack {
                scared_of_cat: false,
                cat_search_tick: 0,
            },
        ),
        (
            3,
            PhantomGoal::CircleAroundAnchor {
                angle: 0.0,
                distance: 0.0,
                height: 0.0,
                clockwise: 0.0,
            },
        ),
    ])
}

/// `targetSelector`：1 找玩家。`nextScanTick` 的初值是 `reducedTickDelay(20)`。
///
/// The `targetSelector`: the player scan at 1, with `nextScanTick` starting at
/// `reducedTickDelay(20)`.
pub(super) fn target_goals() -> GoalSelector {
    selector(vec![(
        1,
        PhantomGoal::AttackPlayerTarget {
            next_scan_tick: reduced_tick_delay(20),
        },
    )])
}

fn selector(goals: Vec<(i32, PhantomGoal)>) -> GoalSelector {
    let mut selector = GoalSelector::default();
    selector.goals = goals
        .into_iter()
        .map(|(priority, goal)| WrappedGoal {
            priority,
            running: false,
            goal: Goal::Phantom(goal),
        })
        .collect();
    selector
}

/// 幻翼自己的四个目标（`Phantom` 的内部类）。
///
/// The phantom's four goals (inner classes of `Phantom`).
#[derive(Debug, Clone, PartialEq)]
pub enum PhantomGoal {
    /// `PhantomAttackPlayerTargetGoal`。
    AttackPlayerTarget { next_scan_tick: i32 },
    /// `PhantomAttackStrategyGoal`。
    AttackStrategy { next_sweep_tick: i32 },
    /// `PhantomSweepAttackGoal`。
    SweepAttack {
        scared_of_cat: bool,
        cat_search_tick: i32,
    },
    /// `PhantomCircleAroundAnchorGoal`。
    CircleAroundAnchor {
        angle: f32,
        distance: f32,
        height: f32,
        clockwise: f32,
    },
}

impl PhantomGoal {
    /// 两个 `PhantomMoveTargetGoal` 子类占 `MOVE`，另外两个不占标志位。
    ///
    /// The two `PhantomMoveTargetGoal` subclasses take `MOVE`; the others take nothing.
    pub(super) const fn flags(&self) -> &'static [Flag] {
        match self {
            Self::SweepAttack { .. } | Self::CircleAroundAnchor { .. } => &[Flag::Move],
            Self::AttackPlayerTarget { .. } | Self::AttackStrategy { .. } => &[],
        }
    }

    pub(super) const fn name(&self) -> &'static str {
        match self {
            Self::AttackPlayerTarget { .. } => "PhantomAttackPlayerTargetGoal",
            Self::AttackStrategy { .. } => "PhantomAttackStrategyGoal",
            Self::SweepAttack { .. } => "PhantomSweepAttackGoal",
            Self::CircleAroundAnchor { .. } => "PhantomCircleAroundAnchorGoal",
        }
    }

    pub(super) fn can_use(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
        match self {
            Self::AttackPlayerTarget { next_scan_tick } => {
                if *next_scan_tick > 0 {
                    *next_scan_tick -= 1;
                    return false;
                }
                *next_scan_tick = reduced_tick_delay(60);
                scan_for_player(me, mob, ctx)
            }
            Self::AttackStrategy { .. } => can_attack_target(me, mob, ctx),
            Self::SweepAttack { .. } => target_player(mob, ctx).is_some() && mob.phantom.swooping,
            Self::CircleAroundAnchor { .. } => {
                target_player(mob, ctx).is_none() || !mob.phantom.swooping
            }
        }
    }

    pub(super) fn can_continue(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &dyn EntityCtx,
    ) -> bool {
        match self {
            Self::AttackPlayerTarget { .. } => can_attack_target(me, mob, ctx),
            Self::SweepAttack {
                scared_of_cat,
                cat_search_tick,
            } => {
                let Some(target) = target_player(mob, ctx) else {
                    return false;
                };
                if !target.alive || target.spectator || target.creative || !mob.phantom.swooping {
                    return false;
                }
                if me.tick_count > *cat_search_tick {
                    *cat_search_tick = me.tick_count + 20;
                    // Bamboo 还没有猫：16 格内的猫恒为空，不会被吓跑。
                    // Bamboo has no cats yet: the 16-block cat search is always empty.
                    *scared_of_cat = false;
                }
                !*scared_of_cat
            }
            Self::AttackStrategy { .. } | Self::CircleAroundAnchor { .. } => {
                self.can_use(me, mob, ctx)
            }
        }
    }

    pub(super) fn start(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
        match self {
            Self::AttackStrategy { next_sweep_tick } => {
                *next_sweep_tick = reduced_tick_delay(10);
                mob.phantom.swooping = false;
                set_anchor_above_target(me, mob, ctx);
            }
            Self::CircleAroundAnchor {
                angle,
                distance,
                height,
                clockwise,
            } => {
                *distance = 5.0 + me.random.next_float() * 10.0;
                *height = -4.0 + me.random.next_float() * 9.0;
                *clockwise = if me.random.next_boolean() { 1.0 } else { -1.0 };
                select_next(me, mob, angle, *distance, *height, *clockwise);
            }
            Self::AttackPlayerTarget { .. } | Self::SweepAttack { .. } => {}
        }
    }

    pub(super) fn stop(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
        match self {
            Self::AttackStrategy { .. } => {
                if let Some(anchor) = mob.phantom.anchor {
                    let y = ctx.blocks().motion_blocking_y(anchor);
                    let lift = 10 + me.random.next_int_bounded(20);
                    mob.phantom.anchor = Some(BlockPos::new(anchor.x, y + lift, anchor.z));
                }
            }
            Self::SweepAttack { .. } => {
                mob.target = None;
                mob.phantom.swooping = false;
            }
            Self::AttackPlayerTarget { .. } | Self::CircleAroundAnchor { .. } => {}
        }
    }

    pub(super) fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        match self {
            Self::AttackStrategy { next_sweep_tick } => {
                strategy_tick(next_sweep_tick, me, mob, ctx);
            }
            Self::SweepAttack { .. } => sweep_tick(me, mob, ctx),
            Self::CircleAroundAnchor {
                angle,
                distance,
                height,
                clockwise,
            } => circle_tick(me, mob, ctx, angle, distance, height, clockwise),
            Self::AttackPlayerTarget { .. } => {}
        }
    }
}

/// `Phantom.getTarget()`：幻翼只会把玩家当目标；玩家不在了就是 `null`。
///
/// `Phantom.getTarget()`: phantoms only ever target players; a missing player is `null`.
fn target_player(mob: &Mob, ctx: &dyn EntityCtx) -> Option<PlayerView> {
    goals::player_by_uid(ctx.blocks(), mob.target?)
}

/// `Phantom.canAttack(target, TargetingConditions.DEFAULT)`：`forCombat()` 不限距离，
/// 要活着、不是旁观或创造、不是和平难度、看得见。
///
/// `Phantom.canAttack(target, TargetingConditions.DEFAULT)`: `forCombat()` without a
/// range, so alive, neither spectator nor creative, not peaceful, and in sight.
fn can_attack(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx, player: &PlayerView) -> bool {
    !player.creative
        && ctx.blocks().difficulty() != 0
        && goals::targeting_test(me, mob, ctx.blocks(), player, 0.0, true)
}

fn can_attack_target(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
    target_player(mob, ctx).is_some_and(|player| can_attack(me, mob, ctx, &player))
}

/// `PhantomAttackPlayerTargetGoal.canUse` 的扫描：碰撞箱外扩 (16, 64, 16) 里、64 格内能
/// 打的玩家按 Y 从高到低排，第一个 `canAttack` 的当目标。
///
/// The scan of `PhantomAttackPlayerTargetGoal.canUse`: attackable players inside the
/// box inflated by (16, 64, 16) and within 64 blocks, highest first; the first one that
/// passes `canAttack` becomes the target.
fn scan_for_player(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
    let view = ctx.blocks();
    let area = me.bounding_box().inflate(16.0, 64.0, 16.0);
    let mut all: Vec<PlayerView> = Vec::new();
    view.for_each_player(&mut |player| all.push(*player));
    let peaceful = view.difficulty() == 0;
    let mut players: Vec<PlayerView> = Vec::new();
    for player in all {
        if area.contains(player.position.x, player.position.y, player.position.z)
            && !player.creative
            && !peaceful
            && goals::targeting_test(me, mob, view, &player, SCAN_RANGE, true)
        {
            players.push(player);
        }
    }
    // `Comparator.comparing(Entity::getY).reversed()`，稳定排序。
    // `Comparator.comparing(Entity::getY).reversed()`, a stable sort.
    players.sort_by(|a, b| b.position.y.total_cmp(&a.position.y));
    for player in players {
        if can_attack(me, mob, ctx, &player) {
            mob.target = Some(player.uid);
            return true;
        }
    }
    false
}

/// `PhantomAttackStrategyGoal.setAnchorAboveTarget`：锚点搬到目标头上 20 到 39 格，
/// 低于海平面时改成海平面上一格。
///
/// `PhantomAttackStrategyGoal.setAnchorAboveTarget`: the anchor moves 20 to 39 blocks
/// above the target, or to one above sea level when that is lower.
fn set_anchor_above_target(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    if mob.phantom.anchor.is_none() {
        return;
    }
    let Some(target) = target_player(mob, ctx) else {
        return;
    };
    let feet = BlockPos::containing(target.position);
    let mut anchor = BlockPos::new(feet.x, feet.y + 20 + me.random.next_int_bounded(20), feet.z);
    let sea_level = ctx.blocks().sea_level();
    if anchor.y < sea_level {
        anchor = BlockPos::new(anchor.x, sea_level + 1, anchor.z);
    }
    mob.phantom.anchor = Some(anchor);
}

/// `PhantomAttackStrategyGoal.tick`：盘旋阶段倒数，到点转成俯冲、重设锚点、排下一次
/// 俯冲（`(8 + nextInt(4)) * 20` 刻，折半），放俯冲声。
///
/// `PhantomAttackStrategyGoal.tick`: count down while circling, then switch to the swoop,
/// move the anchor, schedule the next swoop (`(8 + nextInt(4)) * 20` ticks, halved) and
/// play the swoop sound.
fn strategy_tick(next_sweep_tick: &mut i32, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    if mob.phantom.swooping {
        return;
    }
    *next_sweep_tick -= 1;
    if *next_sweep_tick > 0 {
        return;
    }
    mob.phantom.swooping = true;
    set_anchor_above_target(me, mob, ctx);
    *next_sweep_tick = reduced_tick_delay((8 + me.random.next_int_bounded(4)) * 20);
    let pitch = 0.95 + me.random.next_float() * 0.1;
    living::play_sound(me, mob, "minecraft:entity.phantom.swoop", 10.0, pitch);
}

/// `PhantomSweepAttackGoal.tick`：朝目标身体中间飞；碰撞箱外扩 0.2 碰到目标就咬一口、
/// 回到盘旋并发世界事件 1039；撞墙或刚受过伤也回到盘旋。
///
/// `PhantomSweepAttackGoal.tick`: fly at the middle of the target; bite once the box
/// inflated by 0.2 touches it, return to circling and send level event 1039; a wall hit
/// or a recent hurt also returns to circling.
fn sweep_tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let Some(target) = target_player(mob, ctx) else {
        return;
    };
    let body = goals::CombatTarget::player(target, true);
    mob.phantom.move_target = Vec3::new(
        target.position.x,
        target.position.y + body.height * 0.5,
        target.position.z,
    );
    let reach = f64::from(0.2_f32);
    if me
        .bounding_box()
        .inflate(reach, reach, reach)
        .intersects(body.hitbox())
    {
        living::do_hurt_target(me, mob, ctx, target.uid);
        mob.phantom.swooping = false;
        if !mob.silent {
            ctx.emit(
                Command::MobLevelEvent {
                    event: SOUND_PHANTOM_BITE,
                    pos: me.block_position(),
                    data: 0,
                },
                me,
            );
        }
    } else if me.horizontal_collision || mob.hurt_time > 0 {
        mob.phantom.swooping = false;
    }
}

/// `PhantomCircleAroundAnchorGoal.tick`。三个随机判断的间隔是 350 / 250 / 450 折半。
///
/// `PhantomCircleAroundAnchorGoal.tick`. The three random rolls use 350 / 250 / 450
/// halved.
fn circle_tick(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    angle: &mut f32,
    distance: &mut f32,
    height: &mut f32,
    clockwise: &mut f32,
) {
    if me.random.next_int_bounded(reduced_tick_delay(350)) == 0 {
        *height = -4.0 + me.random.next_float() * 9.0;
    }
    if me.random.next_int_bounded(reduced_tick_delay(250)) == 0 {
        *distance += 1.0;
        if *distance > 15.0 {
            *distance = 5.0;
            *clockwise = -*clockwise;
        }
    }
    if me.random.next_int_bounded(reduced_tick_delay(450)) == 0 {
        *angle = me.random.next_float() * 2.0 * PI;
        select_next(me, mob, angle, *distance, *height, *clockwise);
    }
    // `touchingTarget()`：离目标点不到 2 格。
    // `touchingTarget()`: within two blocks of the target point.
    if mob.phantom.move_target.distance_to_sqr(me.position) < 4.0 {
        select_next(me, mob, angle, *distance, *height, *clockwise);
    }
    let view = ctx.blocks();
    let pos = me.block_position();
    if mob.phantom.move_target.y < me.position.y && !view.block_state(pos.below()).is_air() {
        *height = mth::java_max_f32(1.0, *height);
        select_next(me, mob, angle, *distance, *height, *clockwise);
    }
    if mob.phantom.move_target.y > me.position.y && !view.block_state(pos.above()).is_air() {
        *height = mth::java_min_f32(-1.0, *height);
        select_next(me, mob, angle, *distance, *height, *clockwise);
    }
}

/// `PhantomCircleAroundAnchorGoal.selectNext`：没有锚点就用脚下那一格；角度走 15 度，
/// 目标点是锚点角上偏出 `distance`、高 `-4 + height`。
///
/// `PhantomCircleAroundAnchorGoal.selectNext`: a missing anchor becomes the current
/// block; the angle advances 15 degrees and the target point sits `distance` out from
/// the anchor's corner at `-4 + height`.
fn select_next(
    me: &Entity,
    mob: &mut Mob,
    angle: &mut f32,
    distance: f32,
    height: f32,
    clockwise: f32,
) {
    let anchor = *mob.phantom.anchor.get_or_insert(me.block_position());
    *angle += clockwise * 15.0 * DEG_TO_RAD;
    let radians = f64::from(*angle);
    mob.phantom.move_target = Vec3::new(
        f64::from(anchor.x) + f64::from(distance * mth::cos(radians)),
        f64::from(anchor.y) + f64::from(-4.0 + height),
        f64::from(anchor.z) + f64::from(distance * mth::sin(radians)),
    );
}

/// `Mth.approach`。
fn approach(current: f32, target: f32, step: f32) -> f32 {
    let step = step.abs();
    if current < target {
        mth::clamp_f32(current + step, current, target)
    } else {
        mth::clamp_f32(current - step, target, current)
    }
}

/// `Mth.approachDegrees`。
fn approach_degrees(current: f32, target: f32, step: f32) -> f32 {
    let difference = mth::degrees_difference(current, target);
    approach(current, current + difference, step)
}

/// `PhantomMoveControl.tick`：撞墙掉头、速度回到 0.1；否则朝目标点转（每刻最多 4 度），
/// 转得少就加速到 1.8，转得多就减到 0.2，再把速度往新方向掰 20%。
///
/// `PhantomMoveControl.tick`: a wall hit turns around and resets the speed to 0.1;
/// otherwise turn at the target point by at most four degrees, speed up towards 1.8 when
/// the turn was small or slow towards 0.2, and blend the velocity 20% into the new
/// heading.
pub(super) fn move_control_tick(me: &mut Entity, mob: &mut Mob) {
    if me.horizontal_collision {
        me.y_rot += 180.0;
        mob.phantom.speed = 0.1;
    }
    let target = mob.phantom.move_target;
    let mut dx = target.x - me.position.x;
    let dy = target.y - me.position.y;
    let mut dz = target.z - me.position.z;
    let mut horizontal = (dx * dx + dz * dz).sqrt();
    if horizontal.abs() <= f64::from(1.0e-5_f32) {
        return;
    }
    let scale = 1.0 - (dy * f64::from(0.7_f32)).abs() / horizontal;
    dx *= scale;
    dz *= scale;
    horizontal = (dx * dx + dz * dz).sqrt();
    let length = (dx * dx + dz * dz + dy * dy).sqrt();
    let previous = me.y_rot;
    let heading = mth::atan2(dz, dx) as f32;
    let from = mth::wrap_degrees(me.y_rot + 90.0);
    let to = mth::wrap_degrees(heading * RAD_TO_DEG);
    me.y_rot = approach_degrees(from, to, 4.0) - 90.0;
    mob.y_body_rot = me.y_rot;
    let speed = mob.phantom.speed;
    mob.phantom.speed = if mth::degrees_difference_abs(previous, me.y_rot) < 3.0 {
        approach(speed, 1.8, 0.005 * (1.8 / speed))
    } else {
        approach(speed, 0.2, 0.025)
    };
    let speed = mob.phantom.speed;
    let pitch = (-(mth::atan2(-dy, horizontal) * f64::from(RAD_TO_DEG))) as f32;
    mob.x_rot = pitch;
    let yaw = me.y_rot + 90.0;
    let x = f64::from(speed * mth::cos(f64::from(yaw * DEG_TO_RAD))) * (dx / length).abs();
    let z = f64::from(speed * mth::sin(f64::from(yaw * DEG_TO_RAD))) * (dz / length).abs();
    let y = f64::from(speed * mth::sin(f64::from(pitch * DEG_TO_RAD))) * (dy / length).abs();
    let movement = me.delta;
    let wanted = Vec3::new(x - movement.x, y - movement.y, z - movement.z);
    me.delta = movement.add(wanted.scale(0.2));
}

/// `PhantomBodyRotationControl.clientTick`：头跟上一刻的身体，身体跟朝向。
///
/// `PhantomBodyRotationControl.clientTick`: the head takes last tick's body yaw and the
/// body takes the entity yaw.
pub(super) fn body_rotation_tick(me: &Entity, mob: &mut Mob) {
    mob.y_head_rot = mob.y_body_rot;
    mob.y_body_rot = me.y_rot;
}

/// `Phantom.travel`：`travelFlying(input, 0.2F)`。`moveRelative` 之后直接 `move`，
/// 速度在水里乘 0.8、岩浆里乘 0.5、空气里乘 0.91，没有重力。
///
/// `Phantom.travel`: `travelFlying(input, 0.2F)`. `moveRelative`, then a bare `move`;
/// the velocity is scaled by 0.8 in water, 0.5 in lava and 0.91 in air, with no gravity.
pub(super) fn travel(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, input: Vec3) {
    let (speed, drag) = if me.in_water {
        (0.02_f32, f64::from(0.8_f32))
    } else if living::is_in_lava(me, mob) {
        (0.02, 0.5)
    } else {
        (0.2, f64::from(0.91_f32))
    };
    living::move_relative(me, speed, input);
    let movement = me.delta;
    motion::move_living(me, mob, ctx, movement);
    me.delta = me.delta.scale(drag);
}

/// `Phantom.finalizeSpawn`：锚点设在头上 5 格，大小设成 0，再走 `Mob.finalizeSpawn`
/// （跟随距离加成和左撇子，各取一次世界随机数）。
///
/// `Phantom.finalizeSpawn`: the anchor goes five blocks up, the size becomes 0, then
/// `Mob.finalizeSpawn` (follow-range bonus and left-handedness, one level draw each).
pub(crate) fn finalize_spawn(entity: &mut Entity, random: &dyn RandomSource) {
    let anchor = entity.block_position();
    entity.with_mob(|me, mob| {
        mob.phantom.anchor = Some(BlockPos::new(anchor.x, anchor.y + 5, anchor.z));
        set_size(me, mob, 0);
    });
    super::creeper::finalize_spawn(entity, random);
}

#[cfg(test)]
mod tests {
    use mc_bamboo_nbt::Tag;

    use super::*;
    use crate::{archive::tag, entity::mob::nbt};

    #[test]
    fn a_size_zero_phantom_keeps_two_attack_damage() {
        let phantom = Mob::new(MobType::Phantom);
        assert_eq!(phantom.health, 20.0);
        assert_eq!(phantom.attributes.value(attribute::ATTACK_DAMAGE), 2.0);
        assert!(MobType::Phantom.is_enemy());
        assert!(!MobType::Phantom.is_monster());
        assert!(!MobType::Phantom.allowed_in_peaceful());
        assert!(MobType::Phantom.burns_in_daylight());
        assert_eq!(MobType::Phantom.xp_reward(), 5);
    }

    #[test]
    fn a_resized_phantom_grows_and_hits_harder() {
        let mut entity = Entity::by_mob_type(MobType::Phantom, Vec3::new(0.5, 80.0, 0.5));
        entity.with_mob(|me, mob| set_size(me, mob, 2));
        let mob = entity.kind.mob().expect("a mob");
        assert_eq!(mob.attributes.base(attribute::ATTACK_DAMAGE), 8.0);
        let (width, height, _) = dimensions(2);
        assert_eq!(entity.dimensions, (width, height));
        assert!(width > 0.9 && height > 0.5);
        entity.with_mob(|me, mob| set_size(me, mob, 200));
        assert_eq!(entity.kind.mob().expect("a mob").phantom.size, MAX_SIZE);
    }

    #[test]
    fn size_and_anchor_survive_a_save_and_load() {
        let mut entity = Entity::by_mob_type(MobType::Phantom, Vec3::new(0.5, 80.0, 0.5));
        entity.with_mob(|me, mob| {
            set_size(me, mob, 3);
            mob.phantom.anchor = Some(BlockPos::new(4, 90, -7));
        });
        let saved: Tag = nbt::write_mob(&entity.split()).expect("a phantom saves");
        assert_eq!(tag::int_or(&saved, "size", -1), 3);
        assert_eq!(tag::int_array(&saved, "anchor_pos"), vec![4, 90, -7]);
        assert!(tag::field(&saved, "Age").is_none());
        let row = nbt::read_mob(&saved, "minecraft:phantom", 7)
            .expect("the tag loads")
            .expect("phantoms are modelled");
        let entity = row.into_entity();
        let mob = entity.kind.mob().expect("a mob");
        assert_eq!(mob.phantom.size, 3);
        assert_eq!(mob.phantom.anchor, Some(BlockPos::new(4, 90, -7)));
        let (width, height, eye) = dimensions(3);
        assert_eq!(entity.dimensions, (width, height));
        assert_eq!(mob.eye_height, eye);
    }

    #[test]
    fn approach_matches_mth() {
        assert_eq!(approach(0.1, 1.8, 0.09), 0.1 + 0.09);
        assert_eq!(approach(1.79, 1.8, 0.09), 1.8);
        assert_eq!(approach(0.3, 0.2, 0.025), 0.3 - 0.025);
        assert_eq!(approach_degrees(170.0, -170.0, 4.0), 174.0);
    }
}
