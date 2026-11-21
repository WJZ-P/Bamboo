//! 海龟：官方 `Turtle`（`Animal` 一支），两栖寻路、家方块、产卵和长大掉鳞甲。
//!
//! 这一期做了属性、尺寸、家方块和回家、求爱后产卵（`TurtleBreedGoal` /
//! `TurtleLayEggGoal`）、产卵时的动画和音效、长大时按 `turtle_grow` 战利品表掉鳞甲。
//! `TurtleGoToWaterGoal` 和 `TurtleTravelGoal` 还没建模，见 README 的已知限制。
//!
//! Turtles: vanilla `Turtle` on the `Animal` branch, with amphibious path finding, a home
//! position, egg laying and the scute dropped when a baby grows up. The go-to-water and
//! travel goals are not modelled yet; see the README's known limitations.

use mc_bamboo_core::{BlockPos, LightLayer, Vec3};
use mc_bamboo_data::{
    PathType, entity_types,
    registries::attribute,
    tags::{block_tags, fluid_tags},
};

use super::{
    Mob, MobType,
    attributes::Attributes,
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
        }
    }
}

/// 成年用实体类型表的尺寸（眼高等于高度）；幼年是 `Turtle.BABY_DIMENSIONS` 的 0.3 倍。
///
/// Adults use the entity type's size (eye height equals height); babies are the
/// `Turtle.BABY_DIMENSIONS` at 0.3 scale.
pub(super) const fn dimensions(kind: MobType, baby: bool) -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[kind.entity_type() as usize];
    if baby {
        let width = facts.width * BABY_SCALE;
        let height = facts.height * BABY_SCALE;
        (width, height, height)
    } else {
        (facts.width, facts.height, facts.height)
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

/// `Turtle.registerGoals`。`TurtleGoToWaterGoal`（3）和 `TurtleTravelGoal`（7）还没建模。
///
/// The `Turtle.registerGoals` list. The go-to-water goal (3) and the travel goal (7)
/// are not modelled yet.
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
            },
        ),
        (4, Goal::TurtleGoHome { speed: 1.0 }),
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
                probability: 0.001,
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
/// 回家途中离家的水平距离小于 20 格时不下沉。
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
    let near_home =
        mob.turtle.going_home && horizontal_distance(me.position, mob.turtle.home_pos) < 20.0;
    if mob.target.is_none() && !near_home {
        let delta = me.delta;
        me.delta = Vec3::new(delta.x, delta.y - 0.005, delta.z);
    }
}

/// `BlockPos.closerToCenterThan(position, distance)` 的水平版本（官方比较的是三维，
/// 这里照官方用三维距离）。
///
/// `BlockPos.closerToCenterThan(position, distance)`.
fn closer_to_center_than(pos: BlockPos, position: Vec3, distance: f64) -> bool {
    let dx = f64::from(pos.x) + 0.5 - position.x;
    let dy = f64::from(pos.y) + 0.5 - position.y;
    let dz = f64::from(pos.z) + 0.5 - position.z;
    dx * dx + dy * dy + dz * dz < distance * distance
}

fn horizontal_distance(position: Vec3, pos: BlockPos) -> f64 {
    let dx = f64::from(pos.x) + 0.5 - position.x;
    let dz = f64::from(pos.z) + 0.5 - position.z;
    (dx * dx + dz * dz).sqrt()
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

/// `MoveToBlockGoal.findNearestBlock`：搜索半径 16、竖直 1，从脚下和上一格开始。
///
/// `MoveToBlockGoal.findNearestBlock` with search range 16 and vertical range 1.
fn find_nearest_lay_spot(me: &Entity, mob: &Mob, view: &dyn BlockView) -> Option<BlockPos> {
    let centre = me.block_position();
    for y in [0_i32, 1] {
        for r in 0..16_i32 {
            for x in 0..=r {
                let xs = if x == 0 { vec![0] } else { vec![x, -x] };
                for x in xs {
                    let zs: Vec<i32> = if x.abs() < r && x > -r {
                        (0..=r)
                            .flat_map(|z| if z == 0 { vec![0] } else { vec![z, -z] })
                            .collect()
                    } else {
                        vec![0]
                    };
                    for z in zs {
                        let pos = centre.offset(x, y - 1, z);
                        if within_home(mob, pos) && valid_lay_target(view, pos) {
                            return Some(pos);
                        }
                    }
                }
            }
        }
    }
    None
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
