//! 海龟：官方 `Turtle`（`Animal` 一支），两栖寻路、家方块、产卵和长大掉鳞甲。
//!
//! 做了属性、尺寸、家方块和回家、求爱后产卵（`TurtleBreedGoal` /
//! `TurtleLayEggGoal`）、产卵时的动画和音效、长大时按 `turtle_grow` 战利品表掉鳞甲，
//! 以及 `TurtleMoveControl`、找水（`TurtleGoToWaterGoal`）和远行（`TurtleTravelGoal`）。
//!
//! Turtles: vanilla `Turtle` on the `Animal` branch, with amphibious path finding, a home
//! position, egg laying, the scute dropped when a baby grows up, the turtle move control
//! and the go-to-water and travel goals.

use mc_bamboo_core::{BlockPos, LightLayer, Vec3, mth};
use mc_bamboo_data::{
    PathType, entity_types,
    registries::attribute,
    tags::{block_tags, fluid_tags},
};

use super::{
    Mob, MobType,
    attributes::Attributes,
    control::{Operation, move_rotlerp},
    goals::{Goal, GoalSelector, WrappedGoal},
    living,
    path::Malus,
    spawn::SpawnGroup,
    water,
};
use crate::{
    entity::{
        Entity,
        ctx::{BlockView, EntityCtx},
    },
    random::RandomSource,
};

/// `Turtle.getAgeScale()` 的幼年比例，也是 `BABY_DIMENSIONS.scale(0.3F)`。
const BABY_SCALE: f32 = 0.3;

pub(crate) const fn is_turtle(kind: MobType) -> bool {
    matches!(kind, MobType::Turtle)
}

/// 海龟专属的状态：家方块、产卵计数和回家 / 产卵两个目标的进度。
///
/// Turtle-only state: the home position, the laying counter and the progress of the
/// go-home and lay-egg goals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurtleState {
    /// `Turtle.homePos`。
    pub home_pos: BlockPos,
    /// `Turtle.travelPos`。
    pub travel_pos: Option<BlockPos>,
    /// `Turtle.goingHome`。
    pub going_home: bool,
    /// `Turtle.layEggCounter`。
    pub lay_egg_counter: i32,
    /// `Turtle.DATA_HAS_EGG`。
    pub has_egg: bool,
    /// `Turtle.DATA_LAYING_EGG`。
    pub laying_egg: bool,
    /// `TurtleGoHomeGoal` 的 `stuck` 和 `closeToHomeTryTicks`。
    pub home_stuck: bool,
    pub close_to_home_ticks: i32,
    /// `MoveToBlockGoal` 的状态：目标格、尝试刻数、停留上限、已到达刻数。
    ///
    /// `MoveToBlockGoal` state: target cell, try ticks, stay limit, ticks since reached.
    pub block_next_start: i32,
    pub block_pos: Option<BlockPos>,
    pub block_try_ticks: i32,
    pub block_max_stay: i32,
    pub block_ticks_since_reached: i32,
    /// `TurtleGoToWaterGoal` 自己的 `MoveToBlockGoal` 状态：倒计时、目标水格、尝试刻数。
    ///
    /// The go-to-water goal's own `MoveToBlockGoal` state: countdown, target water
    /// cell and try ticks.
    pub water_next_start: i32,
    pub water_pos: Option<BlockPos>,
    pub water_try_ticks: i32,
}

impl Default for TurtleState {
    fn default() -> Self {
        Self {
            home_pos: BlockPos::new(0, 0, 0),
            travel_pos: None,
            going_home: false,
            lay_egg_counter: 0,
            has_egg: false,
            laying_egg: false,
            home_stuck: false,
            close_to_home_ticks: 0,
            block_next_start: 0,
            block_pos: None,
            block_try_ticks: 0,
            block_max_stay: 0,
            block_ticks_since_reached: 0,
            water_next_start: 0,
            water_pos: None,
            water_try_ticks: 0,
        }
    }
}

/// 成年用实体类型表的尺寸（眼高等于高度）；幼年是 `Turtle.BABY_DIMENSIONS` 的 0.3 倍。
///
/// Adults use the entity type's size (eye height equals height); babies are the
/// `Turtle.BABY_DIMENSIONS` at 0.3 scale.
pub(super) const fn dimensions(kind: MobType, baby: bool) -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[kind.entity_type() as usize];
    let eye = facts.height * 0.85;
    if baby {
        let width = facts.width * BABY_SCALE;
        let height = facts.height * BABY_SCALE;
        (width, height, eye * BABY_SCALE)
    } else {
        (facts.width, facts.height, eye)
    }
}

/// `Turtle.createAttributes`：`Animal.createAnimalAttributes()` 加 30 点血、0.25 速度、
/// 1.0 台阶高度。
///
/// `Turtle.createAttributes`: the animal defaults with thirty health, 0.25 speed and a
/// step height of one.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::cow();
    attributes.set_base(attribute::MAX_HEALTH, 30.0);
    attributes.set_base(attribute::MOVEMENT_SPEED, f64::from(0.25_f32));
    attributes.set_base(attribute::STEP_HEIGHT, 1.0);
    attributes
}

/// `Turtle` 构造函数里的 `setPathfindingMalus`：水 0，三种门 -1。
///
/// The constructor's `setPathfindingMalus` calls: water zero, three door types -1.
pub(super) fn malus() -> Malus {
    let mut malus = Malus::animal();
    malus.set(PathType::Water, 0.0);
    malus.set(PathType::DoorIronClosed, -1.0);
    malus.set(PathType::DoorWoodClosed, -1.0);
    malus.set(PathType::DoorOpen, -1.0);
    malus
}

/// `Turtle.registerGoals`。
///
/// The `Turtle.registerGoals` list.
pub(super) fn goals(navigation: &mut super::navigation::Navigation) -> GoalSelector {
    navigation.flags.amphibious = true;
    let goals = vec![
        (
            0,
            Goal::Panic {
                speed: 1.2,
                target: Vec3::default(),
            },
        ),
        (
            1,
            Goal::Breed {
                speed: 1.0,
                partner: None,
                love_time: 0,
            },
        ),
        (1, Goal::TurtleLayEgg { speed: 1.0 }),
        (
            2,
            Goal::Tempt {
                speed: 1.1,
                steering_item: false,
                player: None,
                calm_down: 0,
                scare: None,
            },
        ),
        (3, Goal::TurtleGoToWater { speed: 1.0 }),
        (4, Goal::TurtleGoHome { speed: 1.0 }),
        (
            7,
            Goal::TurtleTravel {
                speed: 1.0,
                stuck: false,
            },
        ),
        (
            8,
            Goal::LookAtPlayer {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        (
            9,
            Goal::Stroll {
                speed: 1.0,
                interval: 100,
                probability: super::goals::PLAIN_STROLL,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
    ];
    let mut selector = GoalSelector::default();
    selector.goals = goals
        .into_iter()
        .map(|(priority, goal)| WrappedGoal {
            priority,
            running: false,
            goal,
        })
        .collect();
    selector
}

/// `Turtle.finalizeSpawn`：先记住家方块，再走 `AgeableMob` / `Mob` 那两层。
///
/// `Turtle.finalizeSpawn`: remember the home position, then run the `AgeableMob` /
/// `Mob` layers.
pub(super) fn finalize_spawn(
    entity: &mut Entity,
    random: &dyn RandomSource,
    group: &mut Option<SpawnGroup>,
) {
    let pos = entity.block_position();
    entity.with_mob(|_, mob| mob.turtle.home_pos = pos);
    water::finalize_ageable(entity, random, group, 0.05);
}

/// `Turtle.aiStep` 的尾部：产卵中每 5 刻发一次 2001 号方块事件，脚下是沙子才发。
///
/// The tail of `Turtle.aiStep`: while laying, a level event 2001 every five ticks when
/// the turtle stands on sand.
pub(super) fn ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !living::is_alive(me, mob) || !mob.turtle.laying_egg || mob.turtle.lay_egg_counter < 1 {
        return;
    }
    if mob.turtle.lay_egg_counter % 5 != 0 {
        return;
    }
    let pos = me.block_position();
    if on_sand(ctx.blocks(), pos) {
        let data = ctx.blocks().block_state(pos.below()).0 as i32;
        ctx.emit(
            Command::MobLevelEvent {
                event: 2001,
                pos,
                data,
            },
            me,
        );
    }
}

/// `Turtle.travelInWater`：`moveRelative(0.1F)`，乘 0.9 后没目标时再下沉 0.005。
/// 回家途中离家方块中心不到 20 格（`closerToCenterThan`，三维距离）时不下沉。
///
/// `Turtle.travelInWater`: `moveRelative(0.1F)`, scale by 0.9 and sink by 0.005 without
/// a target unless it is going home and within twenty blocks of home.
pub(super) fn travel_in_water(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    input: Vec3,
) {
    living::move_relative(me, 0.1, input);
    let movement = me.delta;
    super::motion::move_living(me, mob, ctx, movement);
    me.delta = me.delta.scale(0.9);
    let near_home = mob.turtle.going_home && home_closer_than(mob, me.position, 20.0);
    if mob.target.is_none() && !near_home {
        let delta = me.delta;
        me.delta = Vec3::new(delta.x, delta.y - 0.005, delta.z);
    }
}

/// `Turtle.TurtleMoveControl.tick`：先按水里 / 地上压速度，再朝目标转身、把速度按
/// 0.125 插值、按高度差补竖直速度。和 `MoveControl` 不同，它不跳也不把操作改回 `WAIT`。
///
/// `Turtle.TurtleMoveControl.tick`: clamp the speed for water or ground, then turn
/// towards the target, lerp the speed by 0.125 and add vertical speed from the height
/// difference. Unlike `MoveControl` it never jumps and never resets the operation.
pub(super) fn move_control_tick(me: &mut Entity, mob: &mut Mob) {
    update_speed(me, mob);
    if mob.mover.operation != Operation::MoveTo || mob.navigation.is_done() {
        living::set_speed(mob, 0.0);
        return;
    }
    let xd = mob.mover.wanted_x - me.position.x;
    let yd = mob.mover.wanted_y - me.position.y;
    let zd = mob.mover.wanted_z - me.position.z;
    let dd = (xd * xd + yd * yd + zd * zd).sqrt();
    if dd < f64::from(1.0e-5_f32) {
        living::set_speed(mob, 0.0);
        return;
    }
    let yd = yd / dd;
    let yaw = (mth::atan2(zd, xd) * 57.295_776_367_187_5) as f32 - 90.0;
    me.y_rot = move_rotlerp(me.y_rot, yaw, 90.0);
    mob.y_body_rot = me.y_rot;
    let target =
        (mob.mover.speed_modifier * mob.attributes.value(attribute::MOVEMENT_SPEED)) as f32;
    living::set_speed(mob, mth::lerp_f32(0.125, mob.speed, target));
    let delta = me.delta;
    me.delta = Vec3::new(delta.x, delta.y + f64::from(mob.speed) * yd * 0.1, delta.z);
}

/// `TurtleMoveControl.updateSpeed`：水里每刻上浮 0.005，离家 16 格外速度减半（不低于
/// 0.08），幼年再除以 3（不低于 0.06）；地上速度减半（不低于 0.06）。
///
/// `TurtleMoveControl.updateSpeed`: in water rise by 0.005 a tick, halve the speed
/// (at least 0.08) more than 16 blocks from home and divide by three for babies (at
/// least 0.06); on the ground halve it (at least 0.06).
fn update_speed(me: &mut Entity, mob: &mut Mob) {
    if me.in_water {
        let delta = me.delta;
        me.delta = Vec3::new(delta.x, delta.y + 0.005, delta.z);
        if !home_closer_than(mob, me.position, 16.0) {
            living::set_speed(mob, mth::java_max_f32(mob.speed / 2.0, 0.08));
        }
        if mob.is_baby() {
            living::set_speed(mob, mth::java_max_f32(mob.speed / 3.0, 0.06));
        }
    } else if me.on_ground {
        living::set_speed(mob, mth::java_max_f32(mob.speed / 2.0, 0.06));
    }
}

/// `BlockPos.closerToCenterThan(position, distance)`：到方块中心的三维距离。
///
/// `BlockPos.closerToCenterThan(position, distance)`: the 3D distance to the cell
/// centre.
fn closer_to_center_than(pos: BlockPos, position: Vec3, distance: f64) -> bool {
    let dx = f64::from(pos.x) + 0.5 - position.x;
    let dy = f64::from(pos.y) + 0.5 - position.y;
    let dz = f64::from(pos.z) + 0.5 - position.z;
    dx * dx + dy * dy + dz * dz < distance * distance
}

/// 目标是否比给定距离更近（`closerToCenterThan`），给回家目标用。
pub(super) fn home_closer_than(mob: &Mob, position: Vec3, distance: f64) -> bool {
    closer_to_center_than(mob.turtle.home_pos, position, distance)
}

/// `Turtle.canFallInLove`：`Animal` 的条件再加一条「没有蛋」。
///
/// `Turtle.canFallInLove`: the animal rule plus "no egg".
pub(super) fn can_fall_in_love(mob: &Mob) -> bool {
    !mob.turtle.has_egg
}

/// `Turtle.TurtleBreedGoal.breed`：自己挂上蛋，父母都 6000 刻，再按 `mob_drops` 生成
/// `1 + nextInt(7)` 的经验球。
///
/// `Turtle.TurtleBreedGoal.breed`: the mother carries an egg, both parents age by 6000
/// ticks, then one experience orb of `1 + nextInt(7)` when `mob_drops` is on.
pub(super) fn breed(
    me: &mut Entity,
    partner: crate::entity::EntityUid,
    position: Vec3,
    world: &mut crate::world::World,
) {
    world.with_entity_mut(partner, &mut |entity| {
        entity.with_mob(|me, mob| {
            mob.in_love = 0;
            living::set_age(me, mob, 6000);
        });
    });
    me.with_mob(|me, mob| {
        mob.turtle.has_egg = true;
        mob.in_love = 0;
        living::set_age(me, mob, 6000);
    });
    if world.block_game_rules().mob_drops {
        let xp = 1 + me.random.next_int_bounded(7);
        let orb = Entity::experience_orb(position, xp);
        world.spawn_entity(orb);
    }
}

/// `Turtle.ageBoundaryReached` 用的战利品表。
///
/// The loot table used by `Turtle.ageBoundaryReached`.
pub(super) fn grow_loot() -> &'static str {
    "minecraft:entities/turtle_grow"
}

/// `Turtle.getWalkTargetValue`：回家途中不把水当目标；否则水里和沙子上都是 10，
/// 其余按亮度代价。
///
/// `Turtle.getWalkTargetValue`: water is not attractive on the way home; otherwise
/// water and sand score ten and everything else uses the light-level cost.
pub(super) fn walk_target_value(mob: &Mob, view: &dyn BlockView, pos: BlockPos) -> f32 {
    if !mob.turtle.going_home && view.fluid_state(pos).is_tagged(fluid_tags::WATER) {
        return 10.0;
    }
    if view.block_state(pos.below()).is_tagged(block_tags::SAND) {
        return 10.0;
    }
    let block = view.brightness(LightLayer::Block, pos);
    let sky = view.brightness(LightLayer::Sky, pos) - view.sky_darken();
    let raw = block.max(sky);
    let level = raw as f32 / 15.0;
    let magic = level / (4.0 - 3.0 * level);
    magic - 0.5
}

/// `Turtle.nextStep()`：`moveDist + 0.15F`，步子比别的动物密。
///
/// `Turtle.nextStep()`: `moveDist + 0.15F`, so steps come more often.
pub(super) const fn next_step(move_dist: f32) -> f32 {
    move_dist + 0.15
}

/// `Turtle.playStepSound`：幼年用 `turtle.shamble_baby`。
///
/// `Turtle.playStepSound`: babies use `turtle.shamble_baby`.
pub(super) const fn step_sound(baby: bool) -> &'static str {
    if baby {
        "minecraft:entity.turtle.shamble_baby"
    } else {
        "minecraft:entity.turtle.shamble"
    }
}

/// 产卵时给 `layEggCounter` 加上一刻；`TurtleLayEggGoal.tick` 的尾部。
///
/// Bump the laying counter by one tick; the tail of `TurtleLayEggGoal.tick`.
pub(super) fn tick_laying(mob: &mut Mob) {
    if mob.turtle.laying_egg {
        mob.turtle.lay_egg_counter += 1;
    }
}

/// `TurtleEggBlock.isSand` / `onSand`。
pub(super) fn on_sand(view: &dyn BlockView, pos: BlockPos) -> bool {
    view.block_state(pos.below()).is_tagged(block_tags::SAND)
}

/// `Turtle.setLayingEgg`：`layEggCounter` 同时置 1 或 0。
///
/// `Turtle.setLayingEgg`: the counter goes to one or zero with the flag.
pub(super) fn set_laying_egg(mob: &mut Mob, on: bool) {
    mob.turtle.lay_egg_counter = i32::from(on);
    mob.turtle.laying_egg = on;
}

// ---- 目标 ----

use super::goals::{default_random_pos_towards, reduced_tick_delay, within_home};
use crate::entity::ctx::Command;

/// `MoveToBlockGoal.findNearestBlock`：竖直从 `vertical_start` 起按 0、1、-1、2… 交替
/// （不超过 `vertical_range`），每层由内向外一圈圈找；一圈里 x 在两边之间时只看 z = ±r
/// 两格，x = ±r 两列才从 z = 0 起扫全。检查的是 `blockPosition + (x, y - 1, z)`。
///
/// `MoveToBlockGoal.findNearestBlock`: y alternates 0, 1, -1, 2… from
/// `vertical_start` up to `vertical_range`; each layer is searched ring by ring, where
/// interior x only checks z = ±r and the x = ±r columns sweep z from 0. The checked cell
/// is `blockPosition + (x, y - 1, z)`.
fn find_nearest_block(
    me: &Entity,
    mob: &Mob,
    (range, vertical_range, vertical_start): (i32, i32, i32),
    valid: &dyn Fn(BlockPos) -> bool,
) -> Option<BlockPos> {
    let centre = me.block_position();
    let mut y = vertical_start;
    while y <= vertical_range {
        for r in 0..range {
            let mut x = 0;
            while x <= r {
                let mut z = if x < r && x > -r { r } else { 0 };
                while z <= r {
                    let pos = centre.offset(x, y - 1, z);
                    if within_home(mob, pos) && valid(pos) {
                        return Some(pos);
                    }
                    z = if z > 0 { -z } else { 1 - z };
                }
                x = if x > 0 { -x } else { 1 - x };
            }
        }
        y = if y > 0 { -y } else { 1 - y };
    }
    None
}

/// `TurtleLayEggGoal` 的搜索：半径 16、竖直 1、从 0 起（y 依次 0、1、-1）。
///
/// The lay-egg search: range 16, vertical range 1, starting at 0 (y is 0, 1, -1).
fn find_nearest_lay_spot(me: &Entity, mob: &Mob, view: &dyn BlockView) -> Option<BlockPos> {
    find_nearest_block(me, mob, (16, 1, 0), &|pos| valid_lay_target(view, pos))
}

/// `TurtleLayEggGoal.isValidTarget`：上面是空方块，脚下是沙子。
///
/// `TurtleLayEggGoal.isValidTarget`: an empty cell above and sand below.
fn valid_lay_target(view: &dyn BlockView, pos: BlockPos) -> bool {
    view.block_state(pos.above()).is_air() && view.block_state(pos).is_tagged(block_tags::SAND)
}

/// `TurtleLayEggGoal.canUse`。
///
/// `TurtleLayEggGoal.canUse`.
pub(super) fn lay_egg_can_use(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) -> bool {
    if !mob.turtle.has_egg || !home_closer_than(mob, me.position, 9.0) {
        return false;
    }
    if mob.turtle.block_next_start > 0 {
        mob.turtle.block_next_start -= 1;
        return false;
    }
    mob.turtle.block_next_start = reduced_tick_delay(200 + me.random.next_int_bounded(200));
    match find_nearest_lay_spot(me, mob, ctx.blocks()) {
        Some(pos) => {
            mob.turtle.block_pos = Some(pos);
            true
        }
        None => false,
    }
}

/// `MoveToBlockGoal.canContinueToUse` 加海龟自己的两个条件。
///
/// `MoveToBlockGoal.canContinueToUse` plus the turtle's own two conditions.
pub(super) fn lay_egg_can_continue(me: &Entity, mob: &Mob, ctx: &mut dyn EntityCtx) -> bool {
    mob.turtle.block_try_ticks >= -mob.turtle.block_max_stay
        && mob.turtle.block_try_ticks <= 1200
        && mob
            .turtle
            .block_pos
            .is_some_and(|pos| valid_lay_target(ctx.blocks(), pos))
        && mob.turtle.has_egg
        && home_closer_than(mob, me.position, 9.0)
}

/// `MoveToBlockGoal.start`：走到目标上方，再抽停留上限。
///
/// `MoveToBlockGoal.start`: walk to the cell above the target, then roll the stay limit.
pub(super) fn lay_egg_start(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, speed: f64) {
    if let Some(pos) = mob.turtle.block_pos {
        let nav = living::nav_mob(me, mob, ctx.blocks());
        mob.navigation.move_to(
            ctx.blocks(),
            &nav,
            Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y + 1),
                f64::from(pos.z) + 0.5,
            ),
            speed,
        );
    }
    mob.turtle.block_try_ticks = 0;
    let inner = me.random.next_int_bounded(1200);
    mob.turtle.block_max_stay = me.random.next_int_bounded(inner + 1200) + 1200;
    mob.turtle.block_ticks_since_reached = 0;
}

/// `TurtleLayEggGoal.tick` 加 `MoveToBlockGoal.tick`。
///
/// `TurtleLayEggGoal.tick` on top of `MoveToBlockGoal.tick`.
pub(super) fn lay_egg_tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, speed: f64) {
    let Some(pos) = mob.turtle.block_pos else {
        return;
    };
    let target = pos.above();
    let centre = Vec3::new(
        f64::from(target.x) + 0.5,
        f64::from(target.y) + 0.5,
        f64::from(target.z) + 0.5,
    );
    let mut reached = false;
    if me.position.distance_to_sqr(centre) >= 1.0 {
        mob.turtle.block_try_ticks += 1;
        if mob.turtle.block_try_ticks % 40 == 0 {
            let nav = living::nav_mob(me, mob, ctx.blocks());
            mob.navigation.move_to(
                ctx.blocks(),
                &nav,
                Vec3::new(
                    f64::from(target.x) + 0.5,
                    f64::from(target.y),
                    f64::from(target.z) + 0.5,
                ),
                speed,
            );
        }
    } else {
        reached = true;
        mob.turtle.block_try_ticks -= 1;
    }
    if me.in_water || !reached {
        return;
    }
    if mob.turtle.lay_egg_counter < 1 {
        set_laying_egg(mob, true);
    } else if mob.turtle.lay_egg_counter > 200 {
        // `level.playSound` 的随机音高在落地层取，蛋数用海龟自己的随机源。
        // The sound pitch uses the world random in the landing layer; the egg count uses
        // the turtle's own random.
        let eggs = me.random.next_int_bounded(4) + 1;
        ctx.emit(Command::LayTurtleEgg { pos: target, eggs }, me);
        mob.turtle.has_egg = false;
        set_laying_egg(mob, false);
        mob.in_love = 600;
    }
    tick_laying(mob);
}

/// `TurtleGoHomeGoal.canUse`。
///
/// `TurtleGoHomeGoal.canUse`.
pub(super) fn go_home_can_use(me: &mut Entity, mob: &mut Mob) -> bool {
    if mob.is_baby() {
        return false;
    }
    if mob.turtle.has_egg {
        return true;
    }
    // 非每刻目标：官方 `nextInt(reducedTickDelay(700))`，折半后是 `nextInt(350)`。
    // The goal does not run every tick, so this is `nextInt(350)`.
    me.random.next_int_bounded(reduced_tick_delay(700)) == 0
        && !home_closer_than(mob, me.position, 64.0)
}

/// `TurtleGoHomeGoal.start`。
///
/// `TurtleGoHomeGoal.start`.
pub(super) fn go_home_start(mob: &mut Mob) {
    mob.turtle.going_home = true;
    mob.turtle.home_stuck = false;
    mob.turtle.close_to_home_ticks = 0;
}

/// `TurtleGoHomeGoal.canContinueToUse`。
///
/// `TurtleGoHomeGoal.canContinueToUse`.
pub(super) fn go_home_can_continue(me: &Entity, mob: &Mob) -> bool {
    !home_closer_than(mob, me.position, 7.0)
        && !mob.turtle.home_stuck
        && mob.turtle.close_to_home_ticks <= reduced_tick_delay(600)
}

/// `TurtleGoHomeGoal.tick`。
///
/// `TurtleGoHomeGoal.tick`.
pub(super) fn go_home_tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, speed: f64) {
    let home = mob.turtle.home_pos;
    let close = home_closer_than(mob, me.position, 16.0);
    if close {
        mob.turtle.close_to_home_ticks += 1;
    }
    if !mob.navigation.is_done() {
        return;
    }
    let home_vec = Vec3::new(
        f64::from(home.x) + 0.5,
        f64::from(home.y),
        f64::from(home.z) + 0.5,
    );
    let mut next = default_random_pos_towards(
        me,
        mob,
        ctx.blocks(),
        16,
        3,
        home_vec,
        std::f32::consts::PI / 10.0,
    )
    .or_else(|| {
        default_random_pos_towards(
            me,
            mob,
            ctx.blocks(),
            8,
            7,
            home_vec,
            std::f32::consts::FRAC_PI_2,
        )
    });
    if next.is_some_and(|next| {
        !close
            && !ctx
                .blocks()
                .block_state(BlockPos::containing(next))
                .is(mc_bamboo_data::blocks::WATER)
    }) {
        next = default_random_pos_towards(
            me,
            mob,
            ctx.blocks(),
            16,
            5,
            home_vec,
            std::f32::consts::FRAC_PI_2,
        );
    }
    let Some(next) = next else {
        mob.turtle.home_stuck = true;
        return;
    };
    let nav = living::nav_mob(me, mob, ctx.blocks());
    mob.navigation.move_to(ctx.blocks(), &nav, next, speed);
}

/// `TurtleGoHomeGoal.stop`。
///
/// `TurtleGoHomeGoal.stop`.
pub(super) fn go_home_stop(mob: &mut Mob) {
    mob.turtle.going_home = false;
}

/// `TurtleGoToWaterGoal.canUse`：陆地上的幼年总要找水；其余要不在回家、不在水里、
/// 没有蛋，然后才走 `MoveToBlockGoal.canUse`（倒计时、抽下次间隔、找 24 格内的水）。
/// 构造时 `isBaby()` 恒假，所以速度总是 1.0。
///
/// `TurtleGoToWaterGoal.canUse`: a baby on land always looks for water; others must not
/// be going home, in water or carrying an egg before `MoveToBlockGoal.canUse` runs
/// (countdown, next interval, water within 24 blocks). `isBaby()` is always false at
/// construction, so the speed is always 1.0.
pub(super) fn go_to_water_can_use(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) -> bool {
    let baby_on_land = mob.is_baby() && !me.in_water;
    if !baby_on_land && (mob.turtle.going_home || me.in_water || mob.turtle.has_egg) {
        return false;
    }
    if mob.turtle.water_next_start > 0 {
        mob.turtle.water_next_start -= 1;
        return false;
    }
    mob.turtle.water_next_start = reduced_tick_delay(200 + me.random.next_int_bounded(200));
    let view = ctx.blocks();
    let found = find_nearest_block(me, mob, (24, 1, -1), &|pos| is_water_block(view, pos));
    if found.is_some() {
        mob.turtle.water_pos = found;
    }
    found.is_some()
}

/// `TurtleGoToWaterGoal.isValidTarget`：这一格是水方块。
///
/// `TurtleGoToWaterGoal.isValidTarget`: the cell is a water block.
fn is_water_block(view: &dyn BlockView, pos: BlockPos) -> bool {
    view.block_state(pos).is(mc_bamboo_data::blocks::WATER)
}

/// `TurtleGoToWaterGoal.canContinueToUse`：还在陆地上、没超过 1200 个尝试刻、目标仍是水。
///
/// `TurtleGoToWaterGoal.canContinueToUse`: still on land, within 1200 try ticks and the
/// target is still water.
pub(super) fn go_to_water_can_continue(me: &Entity, mob: &Mob, ctx: &mut dyn EntityCtx) -> bool {
    !me.in_water
        && mob.turtle.water_try_ticks <= 1200
        && mob
            .turtle
            .water_pos
            .is_some_and(|pos| is_water_block(ctx.blocks(), pos))
}

/// `MoveToBlockGoal.start`：走到水格上面一格，清尝试刻数，再按官方抽停留上限的两个
/// 随机数（这个目标的 `canContinueToUse` 不看它）。
///
/// `MoveToBlockGoal.start`: walk to the cell above the water, reset the try ticks and
/// draw the two stay-limit numbers (this goal's `canContinueToUse` ignores them).
pub(super) fn go_to_water_start(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    speed: f64,
) {
    if let Some(pos) = mob.turtle.water_pos {
        let nav = living::nav_mob(me, mob, ctx.blocks());
        let target = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y + 1),
            f64::from(pos.z) + 0.5,
        );
        mob.navigation.move_to(ctx.blocks(), &nav, target, speed);
    }
    mob.turtle.water_try_ticks = 0;
    let inner = me.random.next_int_bounded(1200);
    me.random.next_int_bounded(inner + 1200);
}

/// `MoveToBlockGoal.tick`：没到水格上面一格的中心 1 格内就加尝试刻数，每 160 刻
/// （`shouldRecalculatePath`）重新寻路；到了就减。
///
/// `MoveToBlockGoal.tick`: away from the centre of the cell above the water, count a try
/// tick and re-path every 160 (`shouldRecalculatePath`); once there, count down.
pub(super) fn go_to_water_tick(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    speed: f64,
) {
    let Some(pos) = mob.turtle.water_pos else {
        return;
    };
    let target = pos.above();
    if closer_to_center_than(target, me.position, 1.0) {
        mob.turtle.water_try_ticks -= 1;
        return;
    }
    mob.turtle.water_try_ticks += 1;
    if mob.turtle.water_try_ticks % 160 == 0 {
        let nav = living::nav_mob(me, mob, ctx.blocks());
        let wanted = Vec3::new(
            f64::from(target.x) + 0.5,
            f64::from(target.y),
            f64::from(target.z) + 0.5,
        );
        mob.navigation.move_to(ctx.blocks(), &nav, wanted, speed);
    }
}

/// `TurtleTravelGoal.canUse`：不在回家、没有蛋、在水里。
///
/// `TurtleTravelGoal.canUse`: not going home, no egg, in water.
pub(super) fn travel_can_use(me: &Entity, mob: &Mob) -> bool {
    !mob.turtle.going_home && !mob.turtle.has_egg && me.in_water
}

/// `TurtleTravelGoal.start`：在 ±512 格、±4 层里抽一个远点（会高过海平面减一时不改高度），
/// 记成 `travelPos`。返回新的 `stuck`（假）。
///
/// `TurtleTravelGoal.start`: draw a far point within ±512 blocks and ±4 layers (keeping
/// the height when it would rise above sea level minus one) as `travelPos`. Returns the
/// new `stuck` (false).
pub(super) fn travel_start(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) -> bool {
    let xt = me.random.next_int_bounded(1025) - 512;
    let mut yt = me.random.next_int_bounded(9) - 4;
    let zt = me.random.next_int_bounded(1025) - 512;
    if f64::from(yt) + me.position.y > f64::from(ctx.blocks().sea_level() - 1) {
        yt = 0;
    }
    mob.turtle.travel_pos = Some(BlockPos::containing(Vec3::new(
        f64::from(xt) + me.position.x,
        f64::from(yt) + me.position.y,
        f64::from(zt) + me.position.z,
    )));
    false
}

/// `TurtleTravelGoal.tick`：导航空闲时朝远点取下一段（先 16/3、π/10，再 8/7、π/2），
/// 落点周围 34 格的区块不全在就算卡住。
///
/// `TurtleTravelGoal.tick`: when the navigation is idle, pick the next leg towards the
/// far point (16/3 at π/10, then 8/7 at π/2); missing chunks within 34 blocks of it
/// count as stuck.
pub(super) fn travel_tick(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    speed: f64,
    stuck: &mut bool,
) {
    let Some(travel) = mob.turtle.travel_pos else {
        *stuck = true;
        return;
    };
    if !mob.navigation.is_done() {
        return;
    }
    let target = Vec3::new(
        f64::from(travel.x) + 0.5,
        f64::from(travel.y),
        f64::from(travel.z) + 0.5,
    );
    let next = default_random_pos_towards(
        me,
        mob,
        ctx.blocks(),
        16,
        3,
        target,
        std::f32::consts::PI / 10.0,
    )
    .or_else(|| {
        default_random_pos_towards(
            me,
            mob,
            ctx.blocks(),
            8,
            7,
            target,
            std::f32::consts::FRAC_PI_2,
        )
    })
    .filter(|next| has_chunks_around(ctx.blocks(), *next));
    let Some(next) = next else {
        *stuck = true;
        return;
    };
    let nav = living::nav_mob(me, mob, ctx.blocks());
    mob.navigation.move_to(ctx.blocks(), &nav, next, speed);
}

/// `LevelReader.hasChunksAt(xc - 34, zc - 34, xc + 34, zc + 34)`。
///
/// `LevelReader.hasChunksAt(xc - 34, zc - 34, xc + 34, zc + 34)`.
fn has_chunks_around(view: &dyn BlockView, next: Vec3) -> bool {
    let xc = next.x.floor() as i32;
    let zc = next.z.floor() as i32;
    let y = view.min_y();
    ((xc - 34) >> 4..=(xc + 34) >> 4).all(|cx| {
        ((zc - 34) >> 4..=(zc + 34) >> 4)
            .all(|cz| view.is_loaded(BlockPos::new(cx << 4, y, cz << 4)))
    })
}

/// `TurtleTravelGoal.canContinueToUse`：导航还在走、没卡住、不在回家、不在求爱、没有蛋。
///
/// `TurtleTravelGoal.canContinueToUse`: still navigating, not stuck, not going home,
/// not in love and no egg.
pub(super) fn travel_can_continue(mob: &Mob, stuck: bool) -> bool {
    !mob.navigation.is_done()
        && !stuck
        && !mob.turtle.going_home
        && mob.in_love <= 0
        && !mob.turtle.has_egg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{archive::tag, entity::mob::nbt};
    use mc_bamboo_core::Vec3;

    #[test]
    fn turtle_dimensions_scale_babies() {
        let (width, height, eye) = dimensions(MobType::Turtle, false);
        assert_eq!((width, height), (1.2, 0.4));
        assert!((eye - 0.34).abs() < 1.0e-6);
        let (width, height, eye) = dimensions(MobType::Turtle, true);
        assert!((width - 0.36).abs() < 1.0e-6);
        assert!((height - 0.12).abs() < 1.0e-6);
        assert!((eye - 0.102).abs() < 1.0e-6);
    }

    #[test]
    fn turtle_attributes_match_vanilla() {
        let table = attributes();
        assert_eq!(
            table.base(attribute::MAX_HEALTH),
            30.0,
            "Turtle.createAttributes sets thirty health"
        );
        assert_eq!(table.base(attribute::MOVEMENT_SPEED), 0.25);
        assert_eq!(table.base(attribute::STEP_HEIGHT), 1.0);
    }

    #[test]
    fn turtle_saves_home_and_egg_flag() {
        let mut entity = Entity::by_mob_type(MobType::Turtle, Vec3::new(0.5, 62.0, 0.5));
        let home = BlockPos::new(4, 61, 9);
        entity.with_mob(|_, mob| {
            mob.turtle.home_pos = home;
            mob.turtle.has_egg = true;
        });
        let tag = nbt::write_mob(&entity.split()).expect("a turtle saves");
        assert_eq!(tag::int_array(&tag, "home_pos"), vec![4, 61, 9]);
        assert!(tag::bool_or(&tag, "has_egg", false));
        let row = nbt::read_mob(&tag, MobType::Turtle.name(), 7)
            .expect("the tag loads")
            .expect("the turtle is modelled");
        let mob = row.into_entity().kind.mob().expect("a mob").clone();
        assert_eq!(mob.turtle.home_pos, home);
        assert!(mob.turtle.has_egg);
    }
}
