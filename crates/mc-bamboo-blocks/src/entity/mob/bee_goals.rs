//! 蜜蜂的目标：官方 `Bee.registerGoals` 里所有内部子类，逐条照 26.3 字节码。
//!
//! 官方的 `BaseBeeGoal` 把 `canUse` / `canContinueToUse` 包了一层「没在生气」；
//! 这里统一在 [`BeeGoal::can_use`] / [`BeeGoal::can_continue`] 入口判断。
//! 写方块、进巢、粒子这类要动世界的操作走 [`Command`]，两种驱动顺序一致。
//!
//! Bee goals: every inner class of vanilla `Bee.registerGoals`, translated from
//! the 26.3 bytecode. World mutations go through `Command` so both drives agree.

use std::collections::HashMap;

use super::{
    GoalSelector, Mob,
    goals::{self, Flag, Goal, WrappedGoal},
    living,
    navigation::Navigation,
};
use crate::entity::{
    Entity, EntityUid, KindPublic,
    ctx::{BlockView, Command, EntityCtx},
};
use crate::{
    behaviour::machines::beehive_entity::BeehiveEntity,
    state::{AGE_3, AGE_7, BERRIES, BlockState, DOUBLE_BLOCK_HALF, DoubleBlockHalf, WATERLOGGED},
    world::World,
};
use mc_bamboo_core::{Aabb, BlockPos, Vec3};
use mc_bamboo_data::{
    blocks,
    registries::{attribute, mob_effect},
    tags::block_tags,
};

/// `Bee` 的内部目标。每个变体自带逐刻状态，名字是官方类名。
#[derive(Debug, Clone, PartialEq)]
pub enum BeeGoal {
    /// `BeeAttackGoal(1.4, true)`：生气且没蜇过时走常规近战。
    Attack,
    /// `BeeEnterHiveGoal`：在巢 2 格内、想进巢、巢没满时把自己存进去。
    EnterHive,
    /// `ValidateHiveGoal`：每 20–40 刻（构造时抽）检查已加载的巢是否还有效。
    ///
    /// `ValidateHiveGoal`: every 20–40 ticks (drawn at construction), drop the hive if loaded and invalid.
    ValidateHive {
        cooldown: i32,
        last_validate_tick: i64,
    },
    /// `ValidateFlowerGoal`：每 20–40 刻（构造时抽）检查已加载的花是否还吸引蜜蜂。
    ///
    /// `ValidateFlowerGoal`: every 20–40 ticks (drawn at construction), drop the flower if loaded and invalid.
    ValidateFlower {
        cooldown: i32,
        last_validate_tick: i64,
    },
    /// `BeePollinateGoal`：找花、悬停、采粉 400 刻后拿蜜。
    Pollinate(Pollinate),
    /// `BeeLocateHiveGoal`：200 刻一次找 20 格内没满的巢。
    LocateHive,
    /// `BeeGoToHiveGoal`：飞回巢，2400 刻 / 卡住 60 刻就放弃。
    GoToHive(HiveTravel),
    /// `BeeGoToKnownFlowerGoal`：600 刻没蜜后飞回记住的花。
    GoToFlower(FlowerTravel),
    /// `BeeGrowCropGoal`：带蜜且巢有效时给下面的作物催熟。
    GrowCrops,
    /// `BeeWanderGoal`：飞行随机落点。
    Wander,
    /// `BeeHurtByOtherGoal`：`HurtByTargetGoal(this).setAlertOthers()`，愤怒消失就停。
    HurtByOther { timestamp: i32, unseen_ticks: i32 },
    /// `BeeBecomeAngryTargetGoal`：`NearestAttackableTargetGoal(Player, 10, true, false, isAngryAt)`。
    BecomeAngryTarget {
        found: Option<EntityUid>,
        unseen_ticks: i32,
    },
    /// `ResetUniversalAngerTargetGoal`：占位（全局怒气规则默认关）。
    ResetUniversalAnger,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pollinate {
    pub successful_pollinating_ticks: i32,
    pub last_sound_played_tick: i32,
    pub pollinating: bool,
    pub hover_pos: Option<Vec3>,
    pub pollinating_ticks: i32,
    pub unreachable_flowers: HashMap<BlockPos, i64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HiveTravel {
    pub travelling_ticks: i32,
    pub ticks_stuck: i32,
    pub blacklist: Vec<BlockPos>,
    pub last_path: Option<crate::entity::mob::path::Path>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowerTravel {
    pub travelling_ticks: i32,
}

impl BeeGoal {
    pub const fn flags(&self) -> &'static [Flag] {
        match self {
            Self::Attack => &[Flag::Move, Flag::Look],
            Self::Pollinate(_) | Self::GoToHive(_) | Self::GoToFlower(_) | Self::Wander => {
                &[Flag::Move]
            }
            Self::HurtByOther { .. } | Self::BecomeAngryTarget { .. } => &[Flag::Target],
            Self::EnterHive
            | Self::ValidateHive { .. }
            | Self::ValidateFlower { .. }
            | Self::LocateHive
            | Self::GrowCrops
            | Self::ResetUniversalAnger => &[],
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::Attack => "BeeAttackGoal",
            Self::EnterHive => "BeeEnterHiveGoal",
            Self::ValidateHive { .. } => "ValidateHiveGoal",
            Self::ValidateFlower { .. } => "ValidateFlowerGoal",
            Self::Pollinate(_) => "BeePollinateGoal",
            Self::LocateHive => "BeeLocateHiveGoal",
            Self::GoToHive(_) => "BeeGoToHiveGoal",
            Self::GoToFlower(_) => "BeeGoToKnownFlowerGoal",
            Self::GrowCrops => "BeeGrowCropGoal",
            Self::Wander => "BeeWanderGoal",
            Self::HurtByOther { .. } => "BeeHurtByOtherGoal",
            Self::BecomeAngryTarget { .. } => "BeeBecomeAngryTargetGoal",
            Self::ResetUniversalAnger => "ResetUniversalAngerTargetGoal",
        }
    }

    pub const fn every_tick(&self) -> bool {
        matches!(self, Self::Pollinate(_))
    }

    /// `BaseBeeGoal.canUse` 的闸门：愤怒时不跑生活目标。
    /// 注意：`BeeWanderGoal` 直接继承 `Goal` 而非 `BaseBeeGoal`，生气时照样运行。
    fn calm(&self, mob: &Mob, now: i64) -> bool {
        match self {
            Self::Attack
            | Self::Wander
            | Self::HurtByOther { .. }
            | Self::BecomeAngryTarget { .. }
            | Self::ResetUniversalAnger => true,
            _ => !mob.anger.is_angry(now),
        }
    }
}

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        running: false,
        goal,
    }
}

/// `Bee.registerGoals`：0..9 的目标优先级和三个目标目标。
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.fly = true;
    GoalSelector::from_goals(vec![
        wrap(
            0,
            goals::melee_attack(f64::from(1.4_f32), true, "BeeAttackGoal"),
        ),
        wrap(1, Goal::Bee(BeeGoal::EnterHive)),
        wrap(
            2,
            Goal::Breed {
                speed: 1.0,
                partner: None,
                love_time: 0,
            },
        ),
        wrap(
            3,
            Goal::Tempt {
                steering_item: false,
                speed: 1.25,
                player: None,
                calm_down: 0,
                scare: None,
            },
        ),
        wrap(
            3,
            Goal::Bee(BeeGoal::ValidateHive {
                cooldown: 20,
                last_validate_tick: -1,
            }),
        ),
        wrap(
            3,
            Goal::Bee(BeeGoal::ValidateFlower {
                cooldown: 20,
                last_validate_tick: -1,
            }),
        ),
        wrap(4, Goal::Bee(BeeGoal::Pollinate(Pollinate::default()))),
        wrap(
            5,
            Goal::FollowParent {
                speed: 1.25,
                parent: None,
                time_to_recalc_path: 0,
            },
        ),
        wrap(5, Goal::Bee(BeeGoal::LocateHive)),
        wrap(5, Goal::Bee(BeeGoal::GoToHive(HiveTravel::default()))),
        wrap(6, Goal::Bee(BeeGoal::GoToFlower(FlowerTravel::default()))),
        wrap(7, Goal::Bee(BeeGoal::GrowCrops)),
        wrap(8, Goal::Bee(BeeGoal::Wander)),
        wrap(9, Goal::Float),
    ])
}

pub(super) fn target_goals() -> GoalSelector {
    GoalSelector::from_goals(vec![
        wrap(
            1,
            Goal::Bee(BeeGoal::HurtByOther {
                timestamp: 0,
                unseen_ticks: 0,
            }),
        ),
        wrap(
            2,
            Goal::Bee(BeeGoal::BecomeAngryTarget {
                found: None,
                unseen_ticks: 0,
            }),
        ),
        wrap(3, Goal::Bee(BeeGoal::ResetUniversalAnger)),
    ])
}

/// `Bee.hurtServer`：受伤时立刻停掉授粉目标。
///
/// `Bee.hurtServer`: stop pollinating when damaged.
pub(super) fn stop_pollinating(mob: &mut Mob) {
    mob.bee.pollinating = false;
    for wrapped in &mut mob.goals.goals {
        if let Goal::Bee(BeeGoal::Pollinate(p)) = &mut wrapped.goal {
            p.pollinating = false;
        }
    }
}

/// `Bee.wantsToEnterHive`。
fn wants_to_enter_hive(me: &Entity, mob: &Mob, view: &dyn BlockView) -> bool {
    if mob.bee.stay_out_of_hive > 0
        || mob.bee.pollinating
        || mob.bee.has_stung()
        || mob.target.is_some()
    {
        return false;
    }
    let tired = mob.bee.ticks_without_nectar > 3600;
    if !(mob.bee.has_nectar() || tired || view.bees_stay_in_hive(me.block_position())) {
        return false;
    }
    // `isHiveNearFire`：仅在 `getBeehiveBlockEntity() != null` 时检查巢周围一格有没有火。
    if hive_valid(mob, view, me)
        && let Some(hive) = mob.bee.hive_pos
    {
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if view
                        .block_state(hive.offset(dx, dy, dz))
                        .is(mc_bamboo_data::blocks::FIRE)
                    {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// 官方 `Bee.attractsBees(BlockState)`：`#bee_attractive`、未含水，向日葵只认上半格。
///
/// Vanilla `Bee.attractsBees(BlockState)`: `#bee_attractive`, not waterlogged, and only
/// the upper half of sunflowers.
pub fn attracts_bees(state: BlockState) -> bool {
    if !state.is_tagged(block_tags::BEE_ATTRACTIVE) {
        return false;
    }
    if state.try_get(WATERLOGGED).unwrap_or(false) {
        return false;
    }
    if state.is(blocks::SUNFLOWER) {
        return state.try_get(DOUBLE_BLOCK_HALF) == Some(DoubleBlockHalf::Upper);
    }
    true
}

/// 构造函数里由 `Bee.registerGoals` 抽取 `ValidateHiveGoal` 与 `ValidateFlowerGoal` 的冷却。
///
/// Constructor RNG draws in `Bee.registerGoals` for `ValidateHiveGoal` and `ValidateFlowerGoal`.
pub(crate) fn init_validate_cooldowns(mob: &mut Mob, random: &mut crate::entity::JavaRandom) {
    for wrapped in &mut mob.goals.goals {
        match &mut wrapped.goal {
            Goal::Bee(BeeGoal::ValidateHive { cooldown, .. })
            | Goal::Bee(BeeGoal::ValidateFlower { cooldown, .. }) => {
                *cooldown = random.next_int_bounded(21) + 20;
            }
            _ => {}
        }
    }
}

pub(super) fn hive_valid(mob: &Mob, view: &dyn BlockView, me: &Entity) -> bool {
    let Some(hive) = mob.bee.hive_pos else {
        return false;
    };
    !too_far(me, hive, 48)
        && view.has_block_entity(hive)
        && view.block_state(hive).is_tagged(block_tags::BEEHIVES)
}

/// `Bee.closerThan(BlockPos, int)`：`pos.closerThan(blockPosition(), distance)`（整数方块距离）。
///
/// `Bee.closerThan(BlockPos, int)`: integer block distance `pos.closerThan(blockPosition(), distance)`.
fn closer_than(me: &Entity, pos: BlockPos, distance: i32) -> bool {
    hive_dist_sqr(me.block_position(), pos) < f64::from(distance) * f64::from(distance)
}

/// `BlockPos.closerToCenterThan(Position, double)`：实体坐标到方块中心的距离。
///
/// `BlockPos.closerToCenterThan(Position, double)`: distance from entity position to block center.
fn closer_to_center_than(me: &Entity, pos: BlockPos, distance: f64) -> bool {
    me.position.distance_to_sqr(center_vec(pos)) < distance * distance
}

fn is_angry(mob: &Mob, now: i64) -> bool {
    mob.anger.is_angry(now)
}

impl BeeGoal {
    pub(super) fn can_use(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
        let view = ctx.blocks();
        let now = view.game_time();
        if !self.calm(mob, now) {
            return false;
        }
        match self {
            Self::Attack => {
                is_angry(mob, now)
                    && !mob.bee.has_stung()
                    && goals::combat_target(mob, ctx).is_some()
            }
            Self::EnterHive => {
                let Some(hive) = mob.bee.hive_pos else {
                    return false;
                };
                if wants_to_enter_hive(me, mob, view)
                    && closer_to_center_than(me, hive, 2.0)
                    && hive_valid(mob, view, me)
                {
                    if view.beehive_full(hive) {
                        // 官方 `BeeEnterHiveGoal.canBeeUse`：贴近后发现满巢直接清空 `hivePos`。
                        // Vanilla `BeeEnterHiveGoal.canBeeUse`: clears `hivePos` when full.
                        mob.bee.hive_pos = None;
                    } else {
                        return true;
                    }
                }
                false
            }
            Self::ValidateHive {
                cooldown,
                last_validate_tick,
            }
            | Self::ValidateFlower {
                cooldown,
                last_validate_tick,
            } => now > *last_validate_tick + i64::from(*cooldown),
            Self::Pollinate(p) => {
                if mob.bee.remaining_cooldown_before_locating_new_flower > 0
                    || mob.bee.has_nectar()
                    || view.is_raining()
                {
                    return false;
                }
                if let Some(flower) = find_nearby_flower(me, mob, view, p, now) {
                    mob.bee.saved_flower_pos = Some(flower);
                    let nav = living::nav_mob(me, mob, view);
                    let _ = mob.navigation.move_to(
                        view,
                        &nav,
                        Vec3::new(
                            f64::from(flower.x) + 0.5,
                            f64::from(flower.y) + 0.5,
                            f64::from(flower.z) + 0.5,
                        ),
                        f64::from(1.2_f32),
                    );
                    return true;
                }
                // `Mth.nextInt(random, 20, 60)`。
                mob.bee.remaining_cooldown_before_locating_new_flower =
                    me.random.next_int_bounded(41) + 20;
                false
            }
            Self::LocateHive => {
                mob.bee.remaining_cooldown_before_locating_new_hive == 0
                    && mob.bee.hive_pos.is_none()
                    && wants_to_enter_hive(me, mob, view)
            }
            Self::GoToHive(_) => {
                let Some(hive) = mob.bee.hive_pos else {
                    return false;
                };
                !too_far(me, hive, 48)
                    && mob.home.is_none()
                    && wants_to_enter_hive(me, mob, view)
                    && !has_reached_hive(me, mob, hive)
                    && view.block_state(hive).is_tagged(block_tags::BEEHIVES)
            }
            Self::GoToFlower(_) => {
                let Some(flower) = mob.bee.saved_flower_pos else {
                    return false;
                };
                mob.home.is_none()
                    && mob.bee.ticks_without_nectar > 600
                    && !closer_than(me, flower, 2)
            }
            Self::GrowCrops => {
                mob.bee.crops_grown < 10
                    && me.random.next_float() >= 0.3
                    && mob.bee.has_nectar()
                    && hive_valid(mob, view, me)
            }
            Self::Wander => mob.navigation.is_done() && me.random.next_int_bounded(10) == 0,
            Self::HurtByOther { timestamp, .. } => {
                mob.last_hurt_by_mob_timestamp != *timestamp
                    && mob.last_hurt_by_mob.is_some_and(|uid| {
                        goals::attack_candidate(mob, ctx, uid).is_some_and(|found| {
                            goals::within_home(mob, BlockPos::containing(found.position))
                        })
                    })
            }
            Self::BecomeAngryTarget { found, .. } => {
                if !is_angry(mob, now) || mob.bee.has_stung() {
                    return false;
                }
                if me.random.next_int_bounded(goals::reduced_tick_delay(10)) != 0 {
                    return false;
                }
                *found = nearest_angry_player(me, mob, ctx);
                found.is_some()
            }
            Self::ResetUniversalAnger => false,
        }
    }

    pub(super) fn can_continue(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &dyn EntityCtx,
    ) -> bool {
        let view = ctx.blocks();
        let now = view.game_time();
        if !self.calm(mob, now) {
            return false;
        }
        match self {
            Self::Attack => {
                is_angry(mob, now)
                    && !mob.bee.has_stung()
                    && goals::target_goal_can_continue(me, mob, ctx)
            }
            Self::Pollinate(p) => {
                if !p.pollinating || mob.bee.saved_flower_pos.is_none() || view.is_raining() {
                    return false;
                }
                if p.successful_pollinating_ticks > 400 {
                    return me.random.next_float() < 0.2;
                }
                true
            }
            Self::GoToHive(_) => self.can_use(me, mob, ctx),
            Self::GoToFlower(_) => self.can_use(me, mob, ctx),
            Self::GrowCrops => self.can_use(me, mob, ctx),
            Self::Wander => !mob.navigation.is_done(),
            Self::HurtByOther { unseen_ticks, .. } => {
                is_angry(mob, now) && target_goal_continue(me, mob, ctx, unseen_ticks, 300)
            }
            Self::BecomeAngryTarget {
                found,
                unseen_ticks,
            } => {
                if !is_angry(mob, now) || mob.bee.has_stung() || mob.target.is_none() {
                    *found = None;
                    return false;
                }
                target_goal_continue(me, mob, ctx, unseen_ticks, 60)
            }
            Self::EnterHive
            | Self::ValidateHive { .. }
            | Self::ValidateFlower { .. }
            | Self::LocateHive
            | Self::ResetUniversalAnger => false,
        }
    }

    pub(super) fn start(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        let view = ctx.blocks();
        match self {
            Self::Attack => {
                let nav = living::nav_mob(me, mob, view);
                if let Some(target) = goals::combat_target(mob, ctx) {
                    let _ = mob
                        .navigation
                        .move_to_entity(view, &nav, target.position, 1.4, 0);
                }
            }
            Self::EnterHive => {
                if let Some(hive) = mob.bee.hive_pos
                    && let Some(occupant) =
                        crate::behaviour::machines::beehive_entity::Occupant::of_mob(me, mob)
                {
                    ctx.emit(
                        Command::BeeEnterHive {
                            hive,
                            occupant: Box::new(occupant),
                            saved_flower: mob.bee.saved_flower_pos,
                        },
                        me,
                    );
                }
            }
            Self::ValidateHive {
                last_validate_tick, ..
            } => {
                if let Some(hive) = mob.bee.hive_pos
                    && view.is_loaded(hive)
                    && !hive_valid(mob, view, me)
                {
                    drop_hive(mob, me);
                }
                *last_validate_tick = view.game_time();
            }
            Self::ValidateFlower {
                last_validate_tick, ..
            } => {
                if let Some(flower) = mob.bee.saved_flower_pos
                    && view.is_loaded(flower)
                    && !attracts_bees(view.block_state(flower))
                {
                    drop_flower(mob, me);
                }
                *last_validate_tick = view.game_time();
            }
            Self::Pollinate(p) => {
                p.successful_pollinating_ticks = 0;
                p.pollinating_ticks = 0;
                p.last_sound_played_tick = 0;
                p.pollinating = true;
                mob.bee.pollinating = true;
                mob.bee.ticks_without_nectar = 0;
            }
            Self::LocateHive => {
                mob.bee.remaining_cooldown_before_locating_new_hive = 200;
                let hives = nearby_hives_with_space(me.block_position(), view);
                let blacklist = locate_blacklist(mob);
                if let Some(hive) = hives.iter().find(|pos| !blacklist.contains(pos)) {
                    mob.bee.hive_pos = Some(*hive);
                } else if let Some(hive) = hives.first() {
                    if let Some(Goal::Bee(BeeGoal::GoToHive(t))) = mob
                        .goals
                        .goals
                        .iter_mut()
                        .map(|wrapped| &mut wrapped.goal)
                        .find(|goal| matches!(goal, Goal::Bee(BeeGoal::GoToHive(_))))
                    {
                        t.blacklist.clear();
                    }
                    mob.bee.hive_pos = Some(*hive);
                }
            }
            Self::GoToHive(t) => {
                t.travelling_ticks = 0;
                t.ticks_stuck = 0;
            }
            Self::GoToFlower(t) => t.travelling_ticks = 0,
            Self::Wander => {
                if let Some(pos) = wander_pos(me, mob, view) {
                    let nav = living::nav_mob(me, mob, view);
                    mob.navigation
                        .set_next_action(super::pathfinding::Action::MoveTo { speed: 1.0 });
                    let path = mob
                        .navigation
                        .create_path(view, &nav, BlockPos::containing(pos), 1);
                    let _ = mob.navigation.move_to_path(view, &nav, path, 1.0);
                }
            }
            Self::HurtByOther {
                timestamp,
                unseen_ticks,
            } => {
                mob.target = mob.last_hurt_by_mob;
                *timestamp = mob.last_hurt_by_mob_timestamp;
                *unseen_ticks = 0;
                alert_other_bees(me, mob, ctx);
            }
            Self::BecomeAngryTarget {
                found,
                unseen_ticks,
            } => {
                mob.target = *found;
                *unseen_ticks = 0;
            }
            Self::GrowCrops | Self::ResetUniversalAnger => {}
        }
    }

    pub(super) fn stop(&mut self, me: &mut Entity, mob: &mut Mob, _ctx: &mut dyn EntityCtx) {
        match self {
            Self::Pollinate(p) => {
                if p.successful_pollinating_ticks > 400 {
                    mob.bee.set_flag(super::bee::FLAG_HAS_NECTAR, true);
                    mob.bee.ticks_without_nectar = 0;
                }
                p.pollinating = false;
                mob.bee.pollinating = false;
                mob.navigation.stop();
                mob.bee.remaining_cooldown_before_locating_new_flower = 200;
            }
            Self::GoToHive(t) => {
                t.travelling_ticks = 0;
                t.ticks_stuck = 0;
                mob.navigation.stop();
                mob.navigation.max_visited_nodes_multiplier = 1.0;
            }
            Self::GoToFlower(t) => {
                t.travelling_ticks = 0;
                mob.navigation.stop();
                mob.navigation.max_visited_nodes_multiplier = 1.0;
            }
            Self::HurtByOther { .. } => mob.target = None,
            Self::BecomeAngryTarget { found, .. } => {
                mob.target = None;
                *found = None;
            }
            Self::Attack
            | Self::EnterHive
            | Self::ValidateHive { .. }
            | Self::ValidateFlower { .. }
            | Self::LocateHive
            | Self::GrowCrops
            | Self::Wander
            | Self::ResetUniversalAnger => {
                let _ = me;
            }
        }
    }

    pub(super) fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        let view = ctx.blocks();
        match self {
            Self::Attack => {
                let Some(target) = goals::combat_target(mob, ctx) else {
                    return;
                };
                let nav = living::nav_mob(me, mob, view);
                let _ = mob
                    .navigation
                    .move_to_entity(view, &nav, target.position, 1.4, 0);
                mob.look.set_look_at_default(
                    target.position.x,
                    target.eye_position().y,
                    target.position.z,
                );
                if me.position.distance_to_sqr(target.position) <= 4.0 && me.tick_count % 20 == 0 {
                    living::do_hurt_target(me, mob, ctx, target.uid);
                }
            }
            Self::Pollinate(p) => pollinate_tick(p, me, mob, ctx),
            Self::GoToHive(t) => hive_tick(t, me, mob, ctx),
            Self::GoToFlower(t) => flower_tick(t, me, mob, ctx),
            Self::GrowCrops => grow_crops_tick(me, mob, ctx),
            Self::Wander => {}
            Self::HurtByOther { .. } | Self::BecomeAngryTarget { .. } => {}
            Self::EnterHive
            | Self::ValidateHive { .. }
            | Self::ValidateFlower { .. }
            | Self::LocateHive
            | Self::ResetUniversalAnger => {}
        }
    }
}

fn too_far(me: &Entity, pos: BlockPos, distance: i32) -> bool {
    !closer_than(me, pos, distance)
}

/// `BeeGoToHiveGoal.hasReachedTarget(BlockPos)`。
fn has_reached_hive(me: &Entity, mob: &Mob, hive: BlockPos) -> bool {
    if closer_than(me, hive, 2) {
        return true;
    }
    mob.navigation
        .path
        .as_ref()
        .is_some_and(|path| path.target == hive && path.reached && path.is_done())
}

fn center_vec(pos: BlockPos) -> Vec3 {
    Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    )
}

/// 官方 `BlockPos.distSqr(Vec3i)`：整数坐标差的平方和，按 double 累加。
/// `findNearbyHivesWithSpace` 的稳定排序键，比较的是蜜蜂 `blockPosition()` 与
/// 巢 `BlockPos` 这两个整数位置，不是浮点位置到巢中心的距离。
fn hive_dist_sqr(from: BlockPos, to: BlockPos) -> f64 {
    let dx = f64::from(to.x - from.x);
    let dy = f64::from(to.y - from.y);
    let dz = f64::from(to.z - from.z);
    dx * dx + dy * dy + dz * dz
}

/// 官方 `Bee$BeeLocateHiveGoal.findNearbyHivesWithSpace()` 的候选表：
/// POI `getInRange(#bee_home, origin, 20, ANY)`（官方遍历序）→ 满巢过滤
/// （`BeehiveBlockEntity.isFull()`）→ `Comparator.comparingDouble(origin.distSqr(pos))`
/// 稳定排序。并列距离时保留 POI 遍历序里的先后。
///
/// `LocateHive.start` 和对拍都调这一个函数，保证测的就是生产路径。
///
/// Vanilla `findNearbyHivesWithSpace`: POI candidates in official order, minus
/// full hives, stably sorted by the integer `BlockPos` distance squared.
pub fn nearby_hives_with_space(origin: BlockPos, view: &dyn BlockView) -> Vec<BlockPos> {
    let mut hives = view.beehives_near(origin, 20);
    hives.retain(|pos| !view.beehive_full(*pos));
    hives.sort_by(|a, b| hive_dist_sqr(origin, *a).total_cmp(&hive_dist_sqr(origin, *b)));
    hives
}

fn locate_blacklist(mob: &Mob) -> Vec<BlockPos> {
    mob.goals
        .goals
        .iter()
        .find_map(|wrapped| match &wrapped.goal {
            Goal::Bee(BeeGoal::GoToHive(t)) => Some(t.blacklist.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn target_goal_continue(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    unseen_ticks: &mut i32,
    memory: i32,
) -> bool {
    if !goals::target_goal_can_continue(me, mob, ctx) {
        return false;
    }
    let Some(target) = goals::combat_target(mob, ctx) else {
        return false;
    };
    if living::has_line_of_sight(me, mob, ctx.blocks(), target.uid, target.eye_position()) {
        *unseen_ticks = 0;
    } else {
        *unseen_ticks += 1;
        if *unseen_ticks > goals::reduced_tick_delay(memory) {
            return false;
        }
    }
    true
}

/// `BeeHurtByOtherGoal.alertOthers`：通知跟随距离 × 10 × 跟随距离内没目标、不同队的其他蜜蜂，
/// 且当前蜂对攻击者有视线（`BeeHurtByOtherGoal.alertOther`）。
fn alert_other_bees(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let Some(enemy) = mob.last_hurt_by_mob else {
        return;
    };
    let Some(target) = goals::attack_candidate(mob, ctx, enemy) else {
        return;
    };
    if !living::has_line_of_sight(me, mob, ctx.blocks(), target.uid, target.eye_position()) {
        return;
    }
    let range = mob.attributes.value(attribute::FOLLOW_RANGE);
    let area = Aabb::new(
        me.position.x,
        me.position.y,
        me.position.z,
        me.position.x + 1.0,
        me.position.y + 1.0,
        me.position.z + 1.0,
    )
    .inflate(range, 10.0, range);
    let mut others = Vec::new();
    ctx.for_each_other_in(area, &mut |other| {
        if let KindPublic::Mob(public) = &other.kind
            && public.mob_type == super::MobType::Bee
            && !public.has_target
        {
            others.push(other.uid);
        }
    });
    if !others.is_empty() {
        ctx.emit(Command::AlertMobs { others, enemy }, me);
    }
}

fn nearest_angry_player(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> Option<EntityUid> {
    let view = ctx.blocks();
    let difficulty = view.difficulty();
    let range = mob.attributes.value(attribute::FOLLOW_RANGE);
    let area = me.bounding_box().inflate(range, 4.0, range);
    let mut players = Vec::new();
    view.for_each_player(&mut |player| players.push(*player));
    let mut best = -1.0_f64;
    let mut found = None;
    for player in players {
        if !super::neutral::is_angry_at_player(mob, &player, difficulty) {
            continue;
        }
        if !super::avoid::player_box(&player).intersects(area) {
            continue;
        }
        if !goals::targeting_test(me, mob, view, &player, range, false, true) {
            continue;
        }
        let distance = player.position.distance_to_sqr(me.position);
        if best == -1.0 || distance < best {
            best = distance;
            found = Some(player.uid);
        }
    }
    found
}

/// `BeePollinateGoal.findNearbyFlower`：按 `BlockPos.withinBoxByManhattanDistance(origin, 5, 5, 5)`
/// 顺序取第一朵 `attractsBees` 且可达的花。
fn find_nearby_flower(
    me: &mut Entity,
    mob: &mut Mob,
    view: &dyn BlockView,
    p: &mut Pollinate,
    now: i64,
) -> Option<BlockPos> {
    let origin = me.block_position();
    let nav = living::nav_mob(me, mob, view);
    // 独立寻路阶段：结果回来后直接核对当时选定的花，不再扫描、不重写不可达名单。
    if let Some(at) = mob.navigation.replay_target() {
        return mob
            .navigation
            .create_path(view, &nav, at, 1)
            .filter(|path| path.reached)
            .map(|_| at);
    }
    if mob.navigation.awaiting() {
        return None;
    }
    let mut new_cache = HashMap::new();
    let mut out = None;
    'outer: for depth in 0_i32..=15 {
        let max_x = depth.min(5);
        for x in -max_x..=max_x {
            let max_y = (depth - x.abs()).min(5);
            for y in -max_y..=max_y {
                let dz = depth - x.abs() - y.abs();
                if dz > 5 {
                    continue;
                }
                let zs: &[i32] = if dz == 0 { &[0] } else { &[dz, -dz] };
                for &z in zs {
                    let pos = origin.offset(x, y, z);
                    if !attracts_bees(view.block_state(pos)) {
                        continue;
                    }
                    let until = p.unreachable_flowers.get(&pos).copied().unwrap_or(i64::MIN);
                    if now < until {
                        new_cache.insert(pos, until);
                        continue;
                    }
                    mob.navigation
                        .set_next_action(super::pathfinding::Action::Inspect(
                            super::pathfinding::Use::BeeFlower,
                        ));
                    if mob
                        .navigation
                        .create_path(view, &nav, pos, 1)
                        .is_some_and(|path| path.reached)
                    {
                        out = Some(pos);
                        break 'outer;
                    }
                    if mob.navigation.awaiting() {
                        // 请求刚挂起，下一刻按这朵花继续。
                        return None;
                    }
                    new_cache.insert(pos, now + 600);
                }
            }
        }
    }
    p.unreachable_flowers = new_cache;
    out
}

fn pollinate_tick(p: &mut Pollinate, me: &mut Entity, mob: &mut Mob, _ctx: &mut dyn EntityCtx) {
    let Some(flower) = mob.bee.saved_flower_pos else {
        return;
    };
    p.pollinating_ticks += 1;
    if p.pollinating_ticks > 600 {
        drop_flower(mob, me);
        p.pollinating = false;
        mob.bee.pollinating = false;
        mob.bee.remaining_cooldown_before_locating_new_flower = 200;
        return;
    }
    let target = Vec3::new(
        f64::from(flower.x) + 0.5,
        f64::from(flower.y) + f64::from(0.6_f32),
        f64::from(flower.z) + 0.5,
    );
    let speed = f64::from(0.35_f32);
    if target.distance_to_sqr(me.position).sqrt() > 1.0 {
        p.hover_pos = Some(target);
        mob.mover
            .set_wanted_position(target.x, target.y, target.z, speed);
        return;
    }
    if p.hover_pos.is_none() {
        p.hover_pos = Some(target);
    }
    let close = me
        .position
        .distance_to_sqr(p.hover_pos.unwrap_or(target))
        .sqrt()
        <= 0.1;
    let mut move_wanted = true;
    if !close && p.pollinating_ticks > 600 {
        drop_flower(mob, me);
        return;
    }
    if close {
        if me.random.next_int_bounded(25) == 0 {
            let offset = (me.random.next_float() * 2.0 - 1.0) * 0.333_333_34;
            let another = (me.random.next_float() * 2.0 - 1.0) * 0.333_333_34;
            p.hover_pos = Some(Vec3::new(
                target.x + f64::from(offset),
                target.y,
                target.z + f64::from(another),
            ));
            mob.navigation.stop();
        } else {
            move_wanted = false;
        }
        mob.look.set_look_at_default(target.x, target.y, target.z);
    }
    if move_wanted && let Some(pos) = p.hover_pos {
        mob.mover.set_wanted_position(pos.x, pos.y, pos.z, speed);
    }
    p.successful_pollinating_ticks += 1;
    if me.random.next_float() < 0.05
        && p.successful_pollinating_ticks > p.last_sound_played_tick + 60
    {
        p.last_sound_played_tick = p.successful_pollinating_ticks;
        super::living::play_sound(me, mob, "minecraft:entity.bee.pollinate", 1.0, 1.0);
    }
}

fn drop_flower(mob: &mut Mob, me: &mut Entity) {
    mob.bee.saved_flower_pos = None;
    mob.bee.remaining_cooldown_before_locating_new_flower = me.random.next_int_bounded(41) + 20;
}

fn hive_tick(t: &mut HiveTravel, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let Some(hive) = mob.bee.hive_pos else {
        return;
    };
    t.travelling_ticks += 1;
    if t.travelling_ticks > goals::reduced_tick_delay(2400) {
        drop_and_blacklist_hive(t, mob, me);
        return;
    }
    if !mob.navigation.is_done() {
        return;
    }
    let view = ctx.blocks();
    if closer_than(me, hive, 16) {
        let nav = living::nav_mob(me, mob, view);
        let reach = if closer_than(me, hive, 3) { 1 } else { 2 };
        mob.navigation.max_visited_nodes_multiplier = 10.0;
        mob.navigation
            .set_next_action(super::pathfinding::Action::Inspect(
                super::pathfinding::Use::BeeHive,
            ));
        let path = mob.navigation.create_path(view, &nav, hive, reach);
        if mob.navigation.awaiting() {
            // 请求刚挂起，下一刻再判断能不能到；等待期间沿旧路走。
            return;
        }
        let _ = mob.navigation.move_to_path(view, &nav, path, 1.0);
        let reached = mob
            .navigation
            .path
            .as_ref()
            .is_some_and(|path| path.reached);
        if !reached {
            drop_and_blacklist_hive(t, mob, me);
            return;
        }
        if let Some(last) = &t.last_path
            && mob
                .navigation
                .path
                .as_ref()
                .is_some_and(|path| path.same_as(Some(last)))
        {
            t.ticks_stuck += 1;
            if t.ticks_stuck > 60 {
                drop_hive(mob, me);
                t.ticks_stuck = 0;
            }
        } else {
            t.last_path = mob.navigation.path.clone();
        }
    } else if too_far(me, hive, 48) {
        drop_hive(mob, me);
    } else {
        pathfind_randomly_towards(me, mob, ctx, hive);
    }
}

fn drop_and_blacklist_hive(t: &mut HiveTravel, mob: &mut Mob, me: &mut Entity) {
    if let Some(hive) = mob.bee.hive_pos {
        t.blacklist.push(hive);
        while t.blacklist.len() > 3 {
            t.blacklist.remove(0);
        }
    }
    drop_hive(mob, me);
}

fn drop_hive(mob: &mut Mob, _me: &mut Entity) {
    mob.bee.hive_pos = None;
    mob.bee.remaining_cooldown_before_locating_new_hive = 200;
}

fn flower_tick(t: &mut FlowerTravel, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let Some(flower) = mob.bee.saved_flower_pos else {
        return;
    };
    t.travelling_ticks += 1;
    if t.travelling_ticks > goals::reduced_tick_delay(2400) {
        drop_flower(mob, me);
        return;
    }
    if !mob.navigation.is_done() {
        return;
    }
    if too_far(me, flower, 48) {
        drop_flower(mob, me);
    } else {
        pathfind_randomly_towards(me, mob, ctx, flower);
    }
}

/// `Bee.pathfindRandomlyTowards`：`AirRandomPos.getPosTowards` + `moveTo(..., 1.0)`。
fn pathfind_randomly_towards(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    target: BlockPos,
) {
    let view = ctx.blocks();
    let dest = Vec3::new(
        f64::from(target.x) + 0.5,
        f64::from(target.y),
        f64::from(target.z) + 0.5,
    );
    let mob_pos = me.block_position();
    let dy = (dest.y as i32) - mob_pos.y;
    let y_offset = if dy > 2 {
        4
    } else if dy < -2 {
        -4
    } else {
        0
    };
    let mut horz = 6;
    let mut vert = 8;
    let dist =
        (mob_pos.x - target.x).abs() + (mob_pos.y - target.y).abs() + (mob_pos.z - target.z).abs();
    if dist < 15 {
        horz = dist / 2;
        vert = dist / 2;
    }
    let pos = goals::air_random_pos_towards(
        me,
        mob,
        view,
        horz,
        vert,
        y_offset,
        dest,
        f64::from(std::f32::consts::PI / 10.0_f32),
    );
    let Some(pos) = pos else {
        return;
    };
    mob.navigation.max_visited_nodes_multiplier = 0.5;
    let nav = living::nav_mob(me, mob, view);
    let _ = mob.navigation.move_to(view, &nav, pos, 1.0);
}

fn wander_pos(me: &mut Entity, mob: &mut Mob, view: &dyn BlockView) -> Option<Vec3> {
    let direction = if hive_valid(mob, view, me)
        && let Some(hive) = mob.bee.hive_pos
        && !closer_than(me, hive, wander_threshold(mob))
    {
        center_vec(hive).subtract(me.position).normalize()
    } else {
        // `Bee.getViewVector(0.0F)`：`LivingEntity.getViewYRot(0.0F)` 取 `yHeadRotO`，
        // `Entity.getViewXRot(0.0F)` 取 `xRotO`。
        // `Bee.getViewVector(0.0F)`: `LivingEntity.getViewYRot(0.0F)` uses `yHeadRotO`,
        // and `Entity.getViewXRot(0.0F)` uses `xRotO`.
        Vec3::view_vector(mob.y_head_rot_o, mob.x_rot_o)
    };
    let mut pos = goals::hover_random_pos(
        me,
        mob,
        view,
        8,
        7,
        direction.x,
        direction.z,
        f64::from(std::f32::consts::FRAC_PI_2),
        3,
        1,
    );
    if pos.is_none() {
        pos = goals::air_and_water_random_pos(
            me,
            mob,
            view,
            8,
            4,
            -2,
            direction.x,
            direction.z,
            f64::from(std::f32::consts::FRAC_PI_2),
        );
    }
    pos
}

fn wander_threshold(mob: &Mob) -> i32 {
    let reduced = if mob.bee.hive_pos.is_some() || mob.bee.saved_flower_pos.is_some() {
        24
    } else {
        16
    };
    48 - reduced
}

fn grow_crops_tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if me.random.next_int_bounded(goals::reduced_tick_delay(30)) != 0 {
        return;
    }
    let mut updates = Vec::new();
    for distance in 1..=2 {
        let pos = me
            .block_position()
            .relative_by(mc_bamboo_core::Direction::Down, distance);
        let state = ctx.blocks().block_state(pos);
        if !state.is_tagged(block_tags::BEE_GROWABLES) {
            continue;
        }
        if let Some(grown) = grown_state(state) {
            updates.push((pos, grown));
        }
    }
    for (pos, grown) in updates {
        ctx.emit(Command::BeeGrowCrop { pos, state: grown }, me);
        mob.bee.crops_grown += 1;
    }
}

fn grown_state(state: BlockState) -> Option<BlockState> {
    if state.is(blocks::WHEAT) || state.is(blocks::CARROTS) || state.is(blocks::POTATOES) {
        let age = state.try_get(AGE_7)?;
        return (age < 7).then(|| state.try_with(AGE_7, age + 1)).flatten();
    }
    if state.is(blocks::BEETROOTS) || state.is(blocks::SWEET_BERRY_BUSH) {
        let age = state.try_get(AGE_3)?;
        return (age < 3).then(|| state.try_with(AGE_3, age + 1)).flatten();
    }
    if state.is(blocks::MELON_STEM) || state.is(blocks::PUMPKIN_STEM) {
        let age = state.try_get(AGE_7)?;
        return (age < 7).then(|| state.try_with(AGE_7, age + 1)).flatten();
    }
    if (state.is(blocks::CAVE_VINES) || state.is(blocks::CAVE_VINES_PLANT))
        && state.try_get(BERRIES) == Some(false)
    {
        return state.try_with(BERRIES, true);
    }
    None
}

/// `Command::BeeEnterHive` 的落地：存进巢、发进巢声，然后把自己丢掉。
pub fn apply_enter_hive(
    me: &mut Entity,
    world: &mut World,
    hive: BlockPos,
    occupant: crate::behaviour::machines::beehive_entity::Occupant,
    saved_flower: Option<BlockPos>,
) {
    let stored = world.with_block_entity::<BeehiveEntity, _>(hive, |entity, world| {
        if entity.is_full() {
            return false;
        }
        // 官方 `BeehiveBlockEntity.addOccupant`：蜂和巢都有记住的花时才抽 `nextBoolean()`。
        // Vanilla `addOccupant`: draw `nextBoolean()` only when both bee and hive have a flower.
        if let Some(flower) = saved_flower
            && (entity.saved_flower_pos().is_none() || world.random().next_boolean())
        {
            entity.set_saved_flower_pos(Some(flower));
        }
        entity.store(occupant)
    });
    if stored == Some(true) {
        world.play_sound(
            Vec3::new(f64::from(hive.x), f64::from(hive.y), f64::from(hive.z)),
            mc_bamboo_data::sounds::sound_event("minecraft:block.beehive.enter"),
            mc_bamboo_data::sounds::BLOCKS,
            1.0,
            1.0,
            false,
        );
        me.removed = true;
    }
}

/// `Bee.tick` 的花蜜粒子：官方先 `nextFloat() < 0.05`，再在循环条件里每次重算
/// `i < random.nextInt(2) + 1`，每次循环抽两发 `level.random.nextDouble()`。
pub fn nectar_particles(me: &mut Entity, mob: &Mob, ctx: &mut dyn EntityCtx) {
    if !mob.bee.has_nectar() || mob.bee.crops_grown >= 10 || me.random.next_float() >= 0.05 {
        return;
    }
    let mut count = 0_usize;
    while (count as i32) < me.random.next_int_bounded(2) + 1 {
        count += 1;
    }
    if count > 0 {
        ctx.emit(Command::BeeNectarParticles { count }, me);
    }
}

/// 蜜蜂用到的 POISON 效果 ID（对拍状态列会读）。
pub const POISON: i32 = mob_effect::POISON;
