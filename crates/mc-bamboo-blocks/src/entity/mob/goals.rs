//! 目标：官方 `GoalSelector`、`WrappedGoal` 和牛注册的八个目标，外加 `RandomPos` 一族。
//!
//! 调度规则和官方逐条一致：满刻（`(tickCount + id) % 2 == 0` 或前两刻）先停、再按
//! 加入顺序尝试启动，标志位按 MOVE、LOOK、JUMP、TARGET 的顺序锁；非满刻只跑
//! 每刻都要更新的目标。随机数一律取实体自己的随机源，取数的先后和官方相同。
//!
//! Goals: vanilla `GoalSelector`, `WrappedGoal`, the eight cow goals and the `RandomPos`
//! family, with identical scheduling and random draw order.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{
    blocks,
    tags::{fluid_tags, item_tags},
};

use super::{Mob, living, navigation::Navigation, path::static_path_type};
use crate::{
    entity::{
        Entity, EntityUid, KindPublic,
        ctx::{BlockView, EntityCtx, PlayerView},
    },
    shape::{self, CollisionContext},
    state::item_is_tagged,
};

/// `Goal.Flag`，按枚举顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    Move = 0,
    Look = 1,
    Jump = 2,
    Target = 3,
}

/// 一个目标和它自己的状态。
#[derive(Debug, Clone, PartialEq)]
pub enum Goal {
    Float,
    Panic {
        speed: f64,
        target: Vec3,
    },
    Breed {
        speed: f64,
        partner: Option<EntityUid>,
        love_time: i32,
    },
    Tempt {
        speed: f64,
        player: Option<EntityUid>,
        calm_down: i32,
    },
    FollowParent {
        speed: f64,
        parent: Option<EntityUid>,
        time_to_recalc_path: i32,
    },
    Stroll {
        speed: f64,
        interval: i32,
        probability: f32,
        wanted: Vec3,
        force_trigger: bool,
    },
    LookAtPlayer {
        look_distance: f32,
        probability: f32,
        look_at: Option<EntityUid>,
        look_time: i32,
    },
    RandomLookAround {
        rel_x: f64,
        rel_z: f64,
        look_time: i32,
    },
    /// 官方已注册、这一期还不建模的目标。保留优先级和标志位锁顺序；
    /// `canUse` 恒假、不取随机数，名字仍进对拍的 running names。
    Unavailable(&'static str),
    /// `ZombieAttackGoal` / `MeleeAttackGoal`：追着当前目标走并在 20 刻间隔挥手。
    MeleeAttack {
        speed: f64,
        ticks_until_next_attack: i32,
        ticks_until_path_recalc: i32,
        pathed_target: Vec3,
        raise_arm_ticks: i32,
        last_can_use_check: i64,
    },
    /// `HurtByTargetGoal`：记住上一次伤害的时间戳，把攻击者设成目标。
    HurtByTarget {
        timestamp: i32,
    },
    /// `NearestAttackableTargetGoal`：按 `TargetType` 选最近目标，缺省类型跳过。
    NearestAttackableTarget {
        kind: TargetType,
        interval: i32,
        unseen_ticks: i32,
        found: Option<EntityUid>,
    },
    /// `RestrictSunGoal`：白天且没有头盔时只打开导航的躲太阳标志。
    RestrictSun,
    /// `FleeSunGoal`：白天着火且看得见天时找个躲处。
    FleeSun {
        speed: f64,
        wanted: Vec3,
    },
    /// `BreakDoorGoal`：HARD 难度、开着 `canBreakDoors` 时逐刻推进 240 刻破坏进度。
    BreakDoor,
    /// `ZombieAttackTurtleEggGoal`（`RemoveBlockGoal` 一系）：倒计时、扫描、走过去踩蛋。
    TurtleEgg {
        speed: f64,
    },
}

/// `NearestAttackableTargetGoal` 的目标类型。缺的实体在 Bamboo 里跳过，
/// 但注册项、优先级和 `Target` 标志位仍和官方一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetType {
    Player,
    Villager,
    IronGolem,
    Turtle,
}

impl TargetType {
    /// Bamboo 现在有没有这种实体；没有时对应的目标直接跳过。
    pub const fn available(self) -> bool {
        matches!(self, Self::Player)
    }
}

impl Goal {
    /// 构造函数里 `setFlags` 设的标志。
    pub const fn flags(&self) -> &'static [Flag] {
        match self {
            Self::Float => &[Flag::Jump],
            Self::Panic { .. } | Self::Stroll { .. } => &[Flag::Move],
            Self::Breed { .. } | Self::Tempt { .. } | Self::RandomLookAround { .. } => {
                &[Flag::Move, Flag::Look]
            }
            Self::FollowParent { .. } => &[],
            Self::LookAtPlayer { .. } => &[Flag::Look],
            Self::Unavailable(_) => &[],
            Self::MeleeAttack { .. } => &[Flag::Move, Flag::Look],
            Self::HurtByTarget { .. } | Self::NearestAttackableTarget { .. } => &[Flag::Target],
            Self::RestrictSun => &[],
            Self::FleeSun { .. } | Self::TurtleEgg { .. } => &[Flag::Move],
            Self::BreakDoor => &[Flag::Move],
        }
    }

    /// `requiresUpdateEveryTick`。
    pub const fn every_tick(&self) -> bool {
        matches!(
            self,
            Self::Float
                | Self::RandomLookAround { .. }
                | Self::MeleeAttack { .. }
                | Self::BreakDoor
                | Self::TurtleEgg { .. }
                | Self::Unavailable(_)
        )
    }

    /// 对拍和调试用的名字，官方的类名。
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Float => "FloatGoal",
            Self::Panic { .. } => "PanicGoal",
            Self::Breed { .. } => "BreedGoal",
            Self::Tempt { .. } => "TemptGoal",
            Self::FollowParent { .. } => "FollowParentGoal",
            Self::Stroll { .. } => "WaterAvoidingRandomStrollGoal",
            Self::LookAtPlayer { .. } => "LookAtPlayerGoal",
            Self::RandomLookAround { .. } => "RandomLookAroundGoal",
            Self::Unavailable(name) => name,
            Self::MeleeAttack { .. } => "ZombieAttackGoal",
            Self::HurtByTarget { .. } => "HurtByTargetGoal",
            Self::NearestAttackableTarget { .. } => "NearestAttackableTargetGoal",
            Self::RestrictSun => "RestrictSunGoal",
            Self::FleeSun { .. } => "FleeSunGoal",
            Self::BreakDoor => "BreakDoorGoal",
            Self::TurtleEgg { .. } => "ZombieAttackTurtleEggGoal",
        }
    }
}

/// `WrappedGoal`。
#[derive(Debug, Clone, PartialEq)]
pub struct WrappedGoal {
    pub priority: i32,
    pub running: bool,
    pub goal: Goal,
}

/// `GoalSelector`。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GoalSelector {
    pub goals: Vec<WrappedGoal>,
    locked: [Option<usize>; 4],
    disabled: [bool; 4],
}

impl GoalSelector {
    /// `AbstractCow.registerGoals`。`FloatGoal` 的构造函数顺手打开导航的浮水。
    pub fn cow(navigation: &mut Navigation) -> Self {
        navigation.flags.can_float = true;
        let goals = vec![
            (0, Goal::Float),
            (
                1,
                Goal::Panic {
                    speed: 2.0,
                    target: Vec3::default(),
                },
            ),
            (
                2,
                Goal::Breed {
                    speed: 1.0,
                    partner: None,
                    love_time: 0,
                },
            ),
            (
                3,
                Goal::Tempt {
                    speed: 1.25,
                    player: None,
                    calm_down: 0,
                },
            ),
            (
                4,
                Goal::FollowParent {
                    speed: 1.25,
                    parent: None,
                    time_to_recalc_path: 0,
                },
            ),
            (
                5,
                Goal::Stroll {
                    speed: 1.0,
                    interval: 120,
                    probability: 0.001,
                    wanted: Vec3::default(),
                    force_trigger: false,
                },
            ),
            (
                6,
                Goal::LookAtPlayer {
                    look_distance: 6.0,
                    probability: 0.02,
                    look_at: None,
                    look_time: 0,
                },
            ),
            (
                7,
                Goal::RandomLookAround {
                    rel_x: 0.0,
                    rel_z: 0.0,
                    look_time: 0,
                },
            ),
        ];
        Self {
            goals: goals
                .into_iter()
                .map(|(priority, goal)| WrappedGoal {
                    priority,
                    running: false,
                    goal,
                })
                .collect(),
            locked: [None; 4],
            disabled: [false; 4],
        }
    }

    /// `Zombie.registerGoals` + `addBehaviourGoals`。官方优先级按字节码：
    /// 2 长矛（无 `KINETIC_WEAPON` 时恒不启动）、3 近战、4 海龟蛋、
    /// 6 穿村（无 POI 时恒不启动）、7 避水漫游、8 看玩家/随便看。
    /// `BreakDoorGoal(1)` 只在 `setCanBreakDoors(true)` 时插入，见 `Mob::set_can_break_doors`。
    pub fn zombie(_navigation: &mut Navigation) -> Self {
        let wrap = |priority, goal| WrappedGoal {
            priority,
            running: false,
            goal,
        };
        Self {
            goals: vec![
                wrap(2, Goal::Unavailable("SpearUseGoal")),
                wrap(
                    3,
                    Goal::MeleeAttack {
                        speed: 1.0,
                        ticks_until_next_attack: 0,
                        ticks_until_path_recalc: 0,
                        pathed_target: Vec3::default(),
                        raise_arm_ticks: 0,
                        last_can_use_check: i64::MIN,
                    },
                ),
                wrap(4, Goal::TurtleEgg { speed: 1.0 }),
                wrap(6, Goal::Unavailable("MoveThroughVillageGoal")),
                wrap(
                    7,
                    Goal::Stroll {
                        speed: 1.0,
                        interval: 120,
                        probability: 0.001,
                        wanted: Vec3::default(),
                        force_trigger: false,
                    },
                ),
                wrap(
                    8,
                    Goal::LookAtPlayer {
                        look_distance: 8.0,
                        probability: 0.02,
                        look_at: None,
                        look_time: 0,
                    },
                ),
                wrap(
                    8,
                    Goal::RandomLookAround {
                        rel_x: 0.0,
                        rel_z: 0.0,
                        look_time: 0,
                    },
                ),
            ],
            locked: [None; 4],
            disabled: [false; 4],
        }
    }

    /// `Zombie.addBehaviourGoals` 里的 `targetSelector`。缺实体的目标仍注册，
    /// 但 `TargetType::available()` 为假时 `canUse` 不查世界、不取随机数。
    pub fn zombie_targets() -> Self {
        let wrap = |priority, goal| WrappedGoal {
            priority,
            running: false,
            goal,
        };
        Self {
            goals: vec![
                wrap(1, Goal::HurtByTarget { timestamp: -1 }),
                wrap(
                    2,
                    Goal::NearestAttackableTarget {
                        kind: TargetType::Player,
                        interval: reduced_tick_delay(10),
                        unseen_ticks: 0,
                        found: None,
                    },
                ),
                wrap(
                    3,
                    Goal::NearestAttackableTarget {
                        kind: TargetType::Villager,
                        interval: reduced_tick_delay(10),
                        unseen_ticks: 0,
                        found: None,
                    },
                ),
                wrap(
                    3,
                    Goal::NearestAttackableTarget {
                        kind: TargetType::IronGolem,
                        interval: reduced_tick_delay(10),
                        unseen_ticks: 0,
                        found: None,
                    },
                ),
                wrap(
                    5,
                    Goal::NearestAttackableTarget {
                        kind: TargetType::Turtle,
                        interval: reduced_tick_delay(10),
                        unseen_ticks: 0,
                        found: None,
                    },
                ),
            ],
            locked: [None; 4],
            disabled: [false; 4],
        }
    }

    /// `setCanBreakDoors(true)` 时官方在优先级 1 插入 `BreakDoorGoal`。
    pub fn insert_break_door(&mut self) {
        if self
            .goals
            .iter()
            .any(|wrapped| matches!(wrapped.goal, Goal::BreakDoor))
        {
            return;
        }
        self.goals.push(WrappedGoal {
            priority: 1,
            running: false,
            goal: Goal::BreakDoor,
        });
    }

    /// `setControlFlag`。
    pub fn set_control_flag(&mut self, flag: Flag, enabled: bool) {
        self.disabled[flag as usize] = !enabled;
    }

    /// 正在跑的目标名，按加入顺序。对拍用。
    pub fn running_names(&self) -> Vec<&'static str> {
        self.goals
            .iter()
            .filter(|wrapped| wrapped.running)
            .map(|wrapped| wrapped.goal.name())
            .collect()
    }

    pub fn is_panicking(&self) -> bool {
        self.goals
            .iter()
            .any(|wrapped| wrapped.running && matches!(wrapped.goal, Goal::Panic { .. }))
    }

    fn contains_disabled(&self, index: usize) -> bool {
        self.goals[index]
            .goal
            .flags()
            .iter()
            .any(|flag| self.disabled[*flag as usize])
    }

    fn can_replace_all(&self, index: usize) -> bool {
        let priority = self.goals[index].priority;
        self.goals[index].goal.flags().iter().all(|flag| {
            // `NO_GOAL` 的优先级是 `Integer.MAX_VALUE`。
            let holder =
                self.locked[*flag as usize].map_or(i32::MAX, |holder| self.goals[holder].priority);
            priority < holder
        })
    }

    fn stop(&mut self, index: usize, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        if !self.goals[index].running {
            return;
        }
        self.goals[index].running = false;
        stop_goal(&mut self.goals[index].goal, me, mob, ctx);
    }

    /// `GoalSelector.tick`。
    pub fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        for index in 0..self.goals.len() {
            if self.goals[index].running
                && (self.contains_disabled(index)
                    || !can_continue(&mut self.goals[index].goal, me, mob, ctx))
            {
                self.stop(index, me, mob, ctx);
            }
        }
        for slot in &mut self.locked {
            if let Some(holder) = *slot
                && !self.goals[holder].running
            {
                *slot = None;
            }
        }
        for index in 0..self.goals.len() {
            if self.goals[index].running
                || self.contains_disabled(index)
                || !self.can_replace_all(index)
                || !can_use(&mut self.goals[index].goal, me, mob, ctx)
            {
                continue;
            }
            for &flag in self.goals[index].goal.flags() {
                if let Some(old) = self.locked[flag as usize] {
                    self.stop(old, me, mob, ctx);
                }
                self.locked[flag as usize] = Some(index);
            }
            self.goals[index].running = true;
            start_goal(&mut self.goals[index].goal, me, mob, ctx);
        }
        self.tick_running(true, me, mob, ctx);
    }

    /// `tickRunningGoals`。
    pub fn tick_running(
        &mut self,
        all: bool,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
    ) {
        for wrapped in &mut self.goals {
            if wrapped.running && (all || wrapped.goal.every_tick()) {
                tick_goal(&mut wrapped.goal, me, mob, ctx);
            }
        }
    }
}

/// `Goal.reducedTickDelay`。
const fn reduced_tick_delay(ticks: i32) -> i32 {
    mth::positive_ceil_div(ticks, 2)
}

fn can_use(goal: &mut Goal, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) -> bool {
    match goal {
        Goal::Float => float_can_use(me, mob),
        Goal::Panic { target, .. } => {
            if !should_panic(mob, ctx) {
                return false;
            }
            if me.remaining_fire_ticks > 0
                && let Some(water) = look_for_water(ctx.blocks(), me.block_position(), 5)
            {
                *target = Vec3::new(f64::from(water.x), f64::from(water.y), f64::from(water.z));
                return true;
            }
            match default_random_pos(me, mob, ctx.blocks(), 5, 4) {
                Some(found) => {
                    *target = found;
                    true
                }
                None => false,
            }
        }
        Goal::Breed { partner, .. } => {
            if mob.in_love <= 0 {
                return false;
            }
            *partner = free_partner(me, ctx);
            partner.is_some()
        }
        Goal::Tempt {
            player, calm_down, ..
        } => {
            if *calm_down > 0 {
                *calm_down -= 1;
                return false;
            }
            *player = nearest_tempting_player(me, mob, ctx);
            player.is_some()
        }
        Goal::FollowParent { parent, .. } => {
            if mob.age >= 0 {
                return false;
            }
            let area = me.bounding_box().inflate(8.0, 4.0, 8.0);
            let mut best = None;
            let mut best_distance = f64::MAX;
            ctx.for_each_other_in(area, &mut |other| {
                let KindPublic::Mob(public) = &other.kind else {
                    return;
                };
                if public.mob_type != mob.mob_type || public.age < 0 {
                    return;
                }
                let distance = me.position.distance_to_sqr(other.position);
                if distance > best_distance {
                    return;
                }
                best_distance = distance;
                best = Some(other.uid);
            });
            if best.is_none() || best_distance < 9.0 {
                return false;
            }
            *parent = best;
            true
        }
        Goal::Stroll {
            interval,
            probability,
            wanted,
            force_trigger,
            ..
        } => {
            if !*force_trigger {
                if mob.no_action_time >= 100 {
                    return false;
                }
                if me.random.next_int_bounded(reduced_tick_delay(*interval)) != 0 {
                    return false;
                }
            }
            let found = if me.in_water {
                land_random_pos(me, mob, ctx.blocks(), 15, 7)
                    .or_else(|| default_random_pos(me, mob, ctx.blocks(), 10, 7))
            } else if me.random.next_float() >= *probability {
                land_random_pos(me, mob, ctx.blocks(), 10, 7)
            } else {
                default_random_pos(me, mob, ctx.blocks(), 10, 7)
            };
            let Some(found) = found else {
                return false;
            };
            *wanted = found;
            *force_trigger = false;
            true
        }
        Goal::LookAtPlayer {
            probability,
            look_distance,
            look_at,
            ..
        } => {
            if me.random.next_float() >= *probability {
                return false;
            }
            *look_at = nearest_look_target(me, mob, ctx, f64::from(*look_distance));
            look_at.is_some()
        }
        Goal::RandomLookAround { .. } => me.random.next_float() < 0.02,
        Goal::Unavailable(_) => false,
        Goal::MeleeAttack {
            last_can_use_check, ..
        } => {
            // 官方 canUse 的 20 刻冷却；到点才查目标、算一次路径。
            let now = ctx.blocks().game_time();
            if now.wrapping_sub(*last_can_use_check) < 20 {
                return false;
            }
            *last_can_use_check = now;
            let Some(target) = target_player(mob, ctx) else {
                return false;
            };
            let nav = living::nav_mob(me, mob, ctx.blocks());
            mob.navigation
                .create_path(ctx.blocks(), &nav, BlockPos::containing(target.position), 0)
                .is_some()
                || within_melee_range(me, mob, target)
        }
        Goal::HurtByTarget { timestamp } => {
            let last = mob.last_hurt_by_mob;
            let stamp = mob.last_hurt_by_mob_timestamp;
            last.is_some() && stamp != *timestamp
        }
        Goal::NearestAttackableTarget {
            kind,
            interval,
            found,
            ..
        } => {
            if !kind.available() {
                return false;
            }
            if *interval > 0 && me.random.next_int_bounded(*interval) != 0 {
                return false;
            }
            *found = nearest_attackable(me, mob, ctx, *kind);
            found.is_some()
        }
        Goal::RestrictSun => {
            ctx.blocks().is_bright_outside() && mob.helmet.is_none() && ctx.blocks().monsters_burn()
        }
        Goal::FleeSun { wanted, .. } => {
            mob.target.is_none()
                && ctx.blocks().is_bright_outside()
                && me.remaining_fire_ticks > 0
                && ctx.blocks().can_see_sky(me.block_position())
                && mob.helmet.is_none()
                && flee_sun_target(me, mob, ctx).is_some_and(|at| {
                    *wanted = at;
                    true
                })
        }
        Goal::BreakDoor => {
            mob.can_break_doors
                && ctx.blocks().block_game_rules().mob_griefing
                && ctx.blocks().difficulty() == 3
        }
        Goal::TurtleEgg { .. } => {
            if !ctx.blocks().block_game_rules().mob_griefing {
                return false;
            }
            if mob.turtle_egg_next_start_tick > 0 {
                mob.turtle_egg_next_start_tick -= 1;
                return false;
            }
            if nearest_turtle_egg(me, ctx).is_some() {
                mob.turtle_egg_next_start_tick = reduced_tick_delay(20);
                true
            } else {
                mob.turtle_egg_next_start_tick =
                    reduced_tick_delay(200 + me.random.next_int_bounded(200));
                false
            }
        }
    }
}

fn can_continue(goal: &mut Goal, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) -> bool {
    match goal {
        Goal::Panic { .. } | Goal::Stroll { .. } => !mob.navigation.is_done(),
        Goal::Breed {
            partner, love_time, ..
        } => partner
            .and_then(|uid| ctx.other(uid))
            .is_some_and(|other| match &other.kind {
                KindPublic::Mob(public) => {
                    public.is_alive() && public.in_love > 0 && *love_time < 60 && !public.panicking
                }
                _ => false,
            }),
        Goal::Tempt { .. } => can_use(goal, me, mob, ctx),
        Goal::FollowParent { parent, .. } => {
            if mob.age >= 0 {
                return false;
            }
            let Some(other) = parent.and_then(|uid| ctx.other(uid)) else {
                return false;
            };
            let alive = matches!(&other.kind, KindPublic::Mob(public) if public.is_alive());
            if !alive {
                return false;
            }
            let distance = me.position.distance_to_sqr(other.position);
            !(distance < 9.0 || distance > 256.0)
        }
        Goal::LookAtPlayer {
            look_distance,
            look_at,
            look_time,
            ..
        } => {
            let Some(player) = look_at.and_then(|uid| player_by_uid(ctx.blocks(), uid)) else {
                return false;
            };
            if !player.alive {
                return false;
            }
            let limit = f64::from(*look_distance * *look_distance);
            if me.position.distance_to_sqr(player.position) > limit {
                return false;
            }
            *look_time > 0
        }
        Goal::RandomLookAround { look_time, .. } => *look_time >= 0,
        Goal::Float => can_use(goal, me, mob, ctx),
        Goal::Unavailable(_) => false,
        Goal::MeleeAttack { .. } => target_player(mob, ctx).is_some(),
        Goal::HurtByTarget { .. } | Goal::NearestAttackableTarget { .. } => mob.target.is_some(),
        Goal::RestrictSun => ctx.blocks().is_bright_outside() && mob.helmet.is_none(),
        Goal::FleeSun { .. } => !mob.navigation.is_done(),
        Goal::BreakDoor => mob.door_break_time <= 240 && ctx.blocks().difficulty() == 3,
        Goal::TurtleEgg { .. } => mob.turtle_egg_try_ticks < 120,
    }
}

fn start_goal(goal: &mut Goal, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    match goal {
        Goal::Panic { speed, target } => {
            let nav = living::nav_mob(me, mob, ctx.blocks());
            mob.navigation.move_to(ctx.blocks(), &nav, *target, *speed);
        }
        Goal::FollowParent {
            time_to_recalc_path,
            ..
        } => *time_to_recalc_path = 0,
        Goal::Stroll { speed, wanted, .. } => {
            let nav = living::nav_mob(me, mob, ctx.blocks());
            mob.navigation.move_to(ctx.blocks(), &nav, *wanted, *speed);
        }
        Goal::LookAtPlayer { look_time, .. } => {
            *look_time = reduced_tick_delay(40 + me.random.next_int_bounded(40));
        }
        Goal::RandomLookAround {
            rel_x,
            rel_z,
            look_time,
        } => {
            let angle = std::f64::consts::TAU * me.random.next_double();
            *rel_x = angle.cos();
            *rel_z = angle.sin();
            *look_time = 20 + me.random.next_int_bounded(20);
        }
        Goal::Float | Goal::Breed { .. } | Goal::Tempt { .. } | Goal::Unavailable(_) => {}
        Goal::MeleeAttack {
            ticks_until_next_attack,
            ticks_until_path_recalc,
            raise_arm_ticks,
            ..
        } => {
            *ticks_until_next_attack = 0;
            *ticks_until_path_recalc = 0;
            *raise_arm_ticks = 0;
            if let Some(target) = target_player(mob, ctx) {
                let nav = living::nav_mob(me, mob, ctx.blocks());
                mob.navigation
                    .move_to_entity(ctx.blocks(), &nav, target.position, 1.0);
            }
        }
        Goal::HurtByTarget { timestamp } => {
            *timestamp = mob.last_hurt_by_mob_timestamp;
            mob.target = mob.last_hurt_by_mob;
        }
        Goal::NearestAttackableTarget {
            found,
            unseen_ticks,
            ..
        } => {
            mob.target = *found;
            *unseen_ticks = 0;
        }
        Goal::RestrictSun => mob.navigation.flags.can_open_doors = true,
        Goal::FleeSun { speed, wanted } => {
            let nav = living::nav_mob(me, mob, ctx.blocks());
            mob.navigation.move_to(ctx.blocks(), &nav, *wanted, *speed);
        }
        Goal::BreakDoor => {
            mob.door_break_time = 0;
            mob.door_break_progress = -1;
            mob.door_break_pos = None;
        }
        Goal::TurtleEgg { .. } => {
            mob.turtle_egg_try_ticks = 0;
            mob.turtle_egg_ticks_since_reached = 0;
        }
    }
}

fn stop_goal(goal: &mut Goal, _me: &mut Entity, mob: &mut Mob, _ctx: &mut dyn EntityCtx) {
    match goal {
        Goal::Breed {
            partner, love_time, ..
        } => {
            *partner = None;
            *love_time = 0;
        }
        Goal::Tempt {
            player, calm_down, ..
        } => {
            *player = None;
            mob.navigation.stop();
            *calm_down = reduced_tick_delay(100);
        }
        Goal::FollowParent { parent, .. } => *parent = None,
        Goal::Stroll { .. } => mob.navigation.stop(),
        Goal::LookAtPlayer { look_at, .. } => *look_at = None,
        Goal::Float
        | Goal::Panic { .. }
        | Goal::RandomLookAround { .. }
        | Goal::Unavailable(_)
        | Goal::RestrictSun
        | Goal::HurtByTarget { .. }
        | Goal::NearestAttackableTarget { .. } => {}
        Goal::MeleeAttack { .. } => {
            mob.navigation.stop();
            mob.aggressive = false;
            if mob.target.is_some() {
                mob.target = None;
            }
        }
        Goal::FleeSun { .. } => mob.navigation.stop(),
        Goal::BreakDoor => {
            mob.door_break_time = 0;
            mob.door_break_progress = -1;
        }
        Goal::TurtleEgg { .. } => {
            mob.turtle_egg_try_ticks = 0;
        }
    }
}

fn tick_goal(goal: &mut Goal, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    match goal {
        Goal::Float => {
            if me.random.next_float() < 0.8 {
                mob.jump.jump = true;
            }
        }
        Goal::Breed {
            speed,
            partner,
            love_time,
        } => {
            let Some(other) = partner.and_then(|uid| ctx.other(uid)) else {
                return;
            };
            let eye = other.position.y + f64::from(mob.mob_type.dimensions(false).2);
            mob.look.set_look_at(
                other.position.x,
                eye,
                other.position.z,
                10.0,
                super::control::MAX_HEAD_X_ROT as f32,
            );
            let nav = living::nav_mob(me, mob, ctx.blocks());
            mob.navigation
                .move_to_entity(ctx.blocks(), &nav, other.position, *speed);
            *love_time += 1;
            // `BreedGoal.tick`：累计 `adjustedTickDelay(60)` 刻且距离小于 3 格时生小牛。
            // 非每刻目标把 60 折成 30，因为整套目标隔刻才跑一次。
            if *love_time >= reduced_tick_delay(60)
                && me.position.distance_to_sqr(other.position) < 9.0
            {
                // 生物状态此刻被从 `me.kind` 里借出来，`Command` 落地要等
                // `living::tick` 把 `kind` 放回去之后；见 `Mob::pending_breed`。
                mob.pending_breed = Some(other.uid);
            }
        }
        Goal::Tempt { speed, player, .. } => {
            let Some(player) = player.and_then(|uid| player_by_uid(ctx.blocks(), uid)) else {
                return;
            };
            mob.look.set_look_at(
                player.position.x,
                player.eye_y(),
                player.position.z,
                (super::control::MAX_HEAD_Y_ROT + 20) as f32,
                super::control::MAX_HEAD_X_ROT as f32,
            );
            if me.position.distance_to_sqr(player.position) < 2.5 * 2.5 {
                mob.navigation.stop();
            } else {
                let nav = living::nav_mob(me, mob, ctx.blocks());
                mob.navigation
                    .move_to_entity(ctx.blocks(), &nav, player.position, *speed);
            }
        }
        Goal::FollowParent {
            speed,
            parent,
            time_to_recalc_path,
        } => {
            *time_to_recalc_path -= 1;
            if *time_to_recalc_path > 0 {
                return;
            }
            *time_to_recalc_path = reduced_tick_delay(10);
            if let Some(other) = parent.and_then(|uid| ctx.other(uid)) {
                let nav = living::nav_mob(me, mob, ctx.blocks());
                mob.navigation
                    .move_to_entity(ctx.blocks(), &nav, other.position, *speed);
            }
        }
        Goal::LookAtPlayer {
            look_at, look_time, ..
        } => {
            let Some(player) = look_at.and_then(|uid| player_by_uid(ctx.blocks(), uid)) else {
                return;
            };
            if !player.alive {
                return;
            }
            mob.look
                .set_look_at_default(player.position.x, player.eye_y(), player.position.z);
            *look_time -= 1;
        }
        Goal::RandomLookAround {
            rel_x,
            rel_z,
            look_time,
        } => {
            *look_time -= 1;
            let eye = me.position.y + f64::from(mob.eye_height);
            mob.look
                .set_look_at_default(me.position.x + *rel_x, eye, me.position.z + *rel_z);
        }
        Goal::Panic { .. } | Goal::Stroll { .. } => {}
        Goal::Unavailable(_) | Goal::HurtByTarget { .. } | Goal::RestrictSun => {}
        Goal::NearestAttackableTarget {
            unseen_ticks,
            found,
            ..
        } => {
            if let Some(uid) = found
                && let Some(target) = player_by_uid(ctx.blocks(), *uid)
                && !target.alive
            {
                mob.target = None;
                *found = None;
            }
            *unseen_ticks += 1;
        }
        Goal::MeleeAttack {
            speed,
            ticks_until_next_attack,
            ticks_until_path_recalc,
            pathed_target,
            raise_arm_ticks,
            ..
        } => {
            let Some(target) = target_player(mob, ctx) else {
                return;
            };
            mob.look.set_look_at(
                target.position.x,
                target.eye_y(),
                target.position.z,
                30.0,
                30.0,
            );
            *ticks_until_path_recalc = (*ticks_until_path_recalc - 1).max(0);
            if *ticks_until_path_recalc == 0
                && (*pathed_target == Vec3::default()
                    || target.position.distance_to_sqr(*pathed_target) >= 1.0
                    || me.random.next_float() >= 0.05)
            {
                *pathed_target = target.position;
                *ticks_until_path_recalc = 4 + me.random.next_int_bounded(7);
                let nav = living::nav_mob(me, mob, ctx.blocks());
                if !mob
                    .navigation
                    .move_to_entity(ctx.blocks(), &nav, target.position, *speed)
                {
                    *ticks_until_path_recalc += 15;
                }
                *ticks_until_path_recalc = reduced_tick_delay(*ticks_until_path_recalc);
            }
            *ticks_until_next_attack = (*ticks_until_next_attack - 1).max(0);
            *raise_arm_ticks += 1;
            mob.aggressive =
                *raise_arm_ticks >= 5 && *ticks_until_next_attack < reduced_tick_delay(20) / 2;
            if *ticks_until_next_attack == 0 && within_melee_range(me, mob, target) {
                *ticks_until_next_attack = reduced_tick_delay(20);
                super::living::do_hurt_target(me, mob, ctx, target.uid);
            }
        }
        Goal::FleeSun { .. } => {}
        Goal::BreakDoor => {
            // 进度和 1019/1021/2001 事件落地在命令层，这里只推倒计时和取随机数。
            mob.door_break_time += 1;
            if me.random.next_int_bounded(20) == 0 {
                ctx.emit(
                    crate::entity::ctx::Command::MobLevelEvent {
                        event: 1019,
                        pos: mob.door_break_pos.unwrap_or(me.block_position()),
                        data: 0,
                    },
                    me,
                );
            }
            if mob.door_break_time >= 240 {
                mob.door_break_time = 0;
                let pos = mob.door_break_pos.unwrap_or(me.block_position());
                ctx.emit(crate::entity::ctx::Command::RemoveMobBlock { pos }, me);
            }
        }
        Goal::TurtleEgg { speed } => {
            mob.turtle_egg_try_ticks += 1;
            if let Some(pos) = mob.turtle_egg_pos {
                let nav = living::nav_mob(me, mob, ctx.blocks());
                mob.navigation.move_to(
                    ctx.blocks(),
                    &nav,
                    Vec3::new(
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y),
                        f64::from(pos.z) + 0.5,
                    ),
                    *speed,
                );
                if me.position.distance_to_sqr(Vec3::new(
                    f64::from(pos.x) + 0.5,
                    f64::from(pos.y),
                    f64::from(pos.z) + 0.5,
                )) <= 1.14 * 1.14
                {
                    mob.turtle_egg_ticks_since_reached += 1;
                    if mob.turtle_egg_ticks_since_reached >= 20 {
                        ctx.emit(crate::entity::ctx::Command::RemoveMobBlock { pos }, me);
                        mob.turtle_egg_ticks_since_reached = 0;
                        mob.turtle_egg_pos = None;
                    }
                }
            }
        }
    }
}

// ---- 目标用到的查询 ----

/// `FloatGoal.canUse`。
fn float_can_use(me: &Entity, mob: &Mob) -> bool {
    living::fluid_height(mob, fluid_tags::ENTITY_FLOATABLE) > living::fluid_jump_threshold(mob)
        || living::is_in_lava(me, mob)
}

/// `PanicGoal.shouldPanic`：最近 40 刻的伤害在 `#panic_causes` 里。
fn should_panic(mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    living::last_damage_source(mob, ctx.blocks().game_time())
        .is_some_and(|source| source.kind.panic_causes())
}

/// `PanicGoal.lookForWater`：脚下没有碰撞时，按曼哈顿距离找最近的水。
fn look_for_water(view: &dyn BlockView, centre: BlockPos, range: i32) -> Option<BlockPos> {
    let state = view.block_state(centre);
    if !shape::collision_boxes(state, centre, CollisionContext::Empty).is_empty() {
        return None;
    }
    let (rx, ry, rz) = (range, 1, range);
    let max_depth = rx + ry + rz;
    for depth in 0..=max_depth {
        let max_x = rx.min(depth);
        for x in -max_x..=max_x {
            let max_y = ry.min(depth - x.abs());
            for y in -max_y..=max_y {
                let dz = depth - x.abs() - y.abs();
                if dz > rz {
                    continue;
                }
                let candidates = if dz == 0 { vec![dz] } else { vec![dz, -dz] };
                for z in candidates {
                    let pos = centre.offset(x, y, z);
                    if pos.y < view.min_y() || pos.y > view.max_y() {
                        continue;
                    }
                    if view.fluid_state(pos).is_tagged(fluid_tags::WATER) {
                        return Some(pos);
                    }
                }
            }
        }
    }
    None
}

/// 按 uid 找一个玩家。
pub(crate) fn player_by_uid(view: &dyn BlockView, uid: EntityUid) -> Option<PlayerView> {
    let mut found = None;
    view.for_each_player(&mut |player| {
        if player.uid == uid {
            found = Some(*player);
        }
    });
    found
}

/// `TargetingConditions.test` 里和玩家有关的部分，`range` 为 0 时不看距离。
fn targeting_test(
    me: &Entity,
    mob: &mut Mob,
    view: &dyn BlockView,
    player: &PlayerView,
    range: f64,
    check_line_of_sight: bool,
) -> bool {
    if player.spectator || !player.alive {
        return false;
    }
    if range > 0.0 {
        let mut visibility = 1.0_f64;
        if player.sneaking {
            visibility *= 0.8;
        }
        if player.invisible {
            let cover = mth::java_max_f32(player.armor_cover, 0.1);
            visibility *= 0.7 * f64::from(cover);
        }
        let visibility = mth::clamp(visibility, 0.0, 10.0);
        let limit = mth::java_max(range * visibility, 2.0);
        if me.position.distance_to_sqr(player.position) > limit * limit {
            return false;
        }
    }
    if check_line_of_sight
        && !living::has_line_of_sight(me, mob, view, player.uid, player.eye_position())
    {
        return false;
    }
    true
}

/// `LookAtPlayerGoal.canUse` 里的 `getNearestPlayer(lookAtContext, mob, x, eyeY, z)`。
fn nearest_look_target(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    range: f64,
) -> Option<EntityUid> {
    let view = ctx.blocks();
    let eye = Vec3::new(
        me.position.x,
        me.position.y + f64::from(mob.eye_height),
        me.position.z,
    );
    let mut players = Vec::new();
    view.for_each_player(&mut |player| players.push(*player));
    let mut best = -1.0_f64;
    let mut found = None;
    for player in players {
        // `notRiding(mob)`：牛不当坐骑，玩家不会骑在它身上。
        if !targeting_test(me, mob, view, &player, range, true) {
            continue;
        }
        let distance = player.position.distance_to_sqr(eye);
        if best == -1.0 || distance < best {
            best = distance;
            found = Some(player.uid);
        }
    }
    found
}

/// `TemptGoal.canUse` 里的 `getNearestPlayer(targetingConditions.range(TEMPT_RANGE), mob)`。
fn nearest_tempting_player(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> Option<EntityUid> {
    let view = ctx.blocks();
    let range = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::TEMPT_RANGE);
    let mut players = Vec::new();
    view.for_each_player(&mut |player| players.push(*player));
    let mut best = -1.0_f64;
    let mut found = None;
    for player in players {
        if player.spectator || !player.alive {
            continue;
        }
        let follows = item_is_tagged(player.main_hand, item_tags::COW_FOOD)
            || item_is_tagged(player.off_hand, item_tags::COW_FOOD);
        if !follows {
            continue;
        }
        if !targeting_test(me, mob, view, &player, range, false) {
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

/// `BreedGoal.getFreePartner`：8 格内同种、在发情、不在恐慌的，取最近的。
fn free_partner(me: &Entity, ctx: &dyn EntityCtx) -> Option<EntityUid> {
    let area = me.bounding_box().inflate(8.0, 8.0, 8.0);
    let mut best = f64::MAX;
    let mut found = None;
    ctx.for_each_other_in(area, &mut |other| {
        let KindPublic::Mob(public) = &other.kind else {
            return;
        };
        if other.uid == me.id || public.in_love <= 0 || public.panicking || !public.is_alive() {
            return;
        }
        let distance = me.position.distance_to_sqr(other.position);
        if distance < best {
            best = distance;
            found = Some(other.uid);
        }
    });
    found
}

// ---- `RandomPos` / `DefaultRandomPos` / `LandRandomPos` / `GoalUtils` ----

/// `GoalUtils.mobRestricted`。
fn mob_restricted(me: &Entity, mob: &Mob, distance: f64) -> bool {
    mob.home.is_some_and(|(home, radius)| {
        let centre = Vec3::new(
            f64::from(home.x) + 0.5,
            f64::from(home.y) + 0.5,
            f64::from(home.z) + 0.5,
        );
        let limit = f64::from(radius) + distance + 1.0;
        centre.distance_to_sqr(me.position) < limit * limit
    })
}

/// `Mob.isWithinHome(BlockPos)`。
fn within_home(mob: &Mob, pos: BlockPos) -> bool {
    mob.home.is_none_or(|(home, radius)| {
        let dx = f64::from(home.x - pos.x);
        let dy = f64::from(home.y - pos.y);
        let dz = f64::from(home.z - pos.z);
        dx * dx + dy * dy + dz * dz < f64::from(radius * radius)
    })
}

/// `RandomPos.generateRandomDirection`。
fn random_direction(me: &mut Entity, horizontal: i32, vertical: i32) -> BlockPos {
    let x = me.random.next_int_bounded(2 * horizontal + 1) - horizontal;
    let y = me.random.next_int_bounded(2 * vertical + 1) - vertical;
    let z = me.random.next_int_bounded(2 * horizontal + 1) - horizontal;
    BlockPos::new(x, y, z)
}

/// `RandomPos.generateRandomPosTowardDirection`。
fn toward_direction(
    me: &mut Entity,
    mob: &Mob,
    max_distance: f64,
    direction: BlockPos,
) -> BlockPos {
    let mut dx = f64::from(direction.x);
    let mut dz = f64::from(direction.z);
    if let Some((home, _)) = mob.home
        && max_distance > 1.0
    {
        if me.position.x > f64::from(home.x) {
            dx -= me.random.next_double() * max_distance / 2.0;
        } else {
            dx += me.random.next_double() * max_distance / 2.0;
        }
        if me.position.z > f64::from(home.z) {
            dz -= me.random.next_double() * max_distance / 2.0;
        } else {
            dz += me.random.next_double() * max_distance / 2.0;
        }
    }
    BlockPos::containing(Vec3::new(
        dx + me.position.x,
        f64::from(direction.y) + me.position.y,
        dz + me.position.z,
    ))
}

fn outside_limits(view: &dyn BlockView, pos: BlockPos) -> bool {
    pos.y < view.min_y() || pos.y > view.max_y()
}

fn has_malus(mob: &Mob, view: &dyn BlockView, pos: BlockPos) -> bool {
    mob.malus.get(static_path_type(view, pos)) != 0.0
}

/// `Animal.getWalkTargetValue`：脚下是草方块给 10，否则按亮度。
fn walk_target_value(view: &dyn BlockView, pos: BlockPos) -> f32 {
    if view.block_state(pos.below()).is(blocks::GRASS_BLOCK) {
        return 10.0;
    }
    let block = view.brightness(mc_bamboo_core::LightLayer::Block, pos);
    let sky = view.brightness(mc_bamboo_core::LightLayer::Sky, pos) - view.sky_darken();
    let raw = block.max(sky);
    let level = raw as f32 / 15.0;
    let magic = level / (4.0 - 3.0 * level);
    let ambient = 0.0_f32;
    mth::lerp_f32(ambient, magic, 1.0) - 0.5
}

/// `RandomPos.generateRandomPos(mob, supplier)`：取 10 次，严格更大才换。
fn pick_best(view: &dyn BlockView, mut supplier: impl FnMut() -> Option<BlockPos>) -> Option<Vec3> {
    let mut best = f64::NEG_INFINITY;
    let mut best_pos = None;
    for _ in 0..10 {
        let Some(pos) = supplier() else {
            continue;
        };
        let weight = f64::from(walk_target_value(view, pos));
        if weight > best {
            best = weight;
            best_pos = Some(pos);
        }
    }
    best_pos.map(|pos| {
        Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y),
            f64::from(pos.z) + 0.5,
        )
    })
}

/// `DefaultRandomPos.getPos`。
pub(crate) fn default_random_pos(
    me: &mut Entity,
    mob: &Mob,
    view: &dyn BlockView,
    horizontal: i32,
    vertical: i32,
) -> Option<Vec3> {
    let restricted = mob_restricted(me, mob, f64::from(horizontal));
    pick_best(view, || {
        let direction = random_direction(me, horizontal, vertical);
        let pos = toward_direction(me, mob, f64::from(horizontal), direction);
        if outside_limits(view, pos)
            || (restricted && !within_home(mob, pos))
            || !Navigation::is_stable_destination(view, pos)
            || has_malus(mob, view, pos)
        {
            return None;
        }
        Some(pos)
    })
}

/// `LandRandomPos.getPos`。
pub(crate) fn land_random_pos(
    me: &mut Entity,
    mob: &Mob,
    view: &dyn BlockView,
    horizontal: i32,
    vertical: i32,
) -> Option<Vec3> {
    let restricted = mob_restricted(me, mob, f64::from(horizontal));
    pick_best(view, || {
        let direction = random_direction(me, horizontal, vertical);
        let pos = toward_direction(me, mob, f64::from(horizontal), direction);
        if outside_limits(view, pos)
            || (restricted && !within_home(mob, pos))
            || !Navigation::is_stable_destination(view, pos)
        {
            return None;
        }
        // `movePosUpOutOfSolid`。
        let mut moved = pos;
        if view.block_state(moved).is_solid() {
            moved = moved.above();
            while moved.y <= view.max_y() && view.block_state(moved).is_solid() {
                moved = moved.above();
            }
        }
        if view.fluid_state(moved).is_tagged(fluid_tags::WATER) || has_malus(mob, view, moved) {
            return None;
        }
        Some(moved)
    })
}

// ---- 敌对目标的查询 ----

/// `Mob.getTarget()` 在 Bamboo 里只可能是本岛玩家（别的生物还没做目标实体）。
fn target_player(mob: &Mob, ctx: &dyn EntityCtx) -> Option<PlayerView> {
    mob.target
        .and_then(|uid| player_by_uid(ctx.blocks(), uid))
        .filter(|player| player.alive && !player.spectator && !player.creative)
        .filter(|_| ctx.blocks().difficulty() != 0)
}

/// `NearestAttackableTargetGoal.findTarget` 的玩家分支：按 `FOLLOW_RANGE`
/// 找最近、看得见、非创造/旁观且在战斗条件下能被打的目标。
fn nearest_attackable(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    kind: TargetType,
) -> Option<EntityUid> {
    if kind != TargetType::Player || ctx.blocks().difficulty() == 0 {
        return None;
    }
    let range = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE);
    let view = ctx.blocks();
    let mut players = Vec::new();
    view.for_each_player(&mut |player| players.push(*player));
    let eye = Vec3::new(
        me.position.x,
        me.position.y + f64::from(mob.eye_height),
        me.position.z,
    );
    let mut best = f64::MAX;
    let mut found = None;
    for player in players {
        if !targeting_test(me, mob, view, &player, range, true) {
            continue;
        }
        let distance = eye.distance_to_sqr(player.eye_position());
        if distance < best {
            best = distance;
            found = Some(player.uid);
        }
    }
    found
}

/// `PathfinderMob.isWithinMeleeAttackRange`：用实体交互范围算的方形距离。
fn within_melee_range(me: &Entity, mob: &Mob, player: PlayerView) -> bool {
    let reach = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::ENTITY_INTERACTION_RANGE)
        + 1.0;
    let dx = (me.position.x - player.position.x).abs();
    let dz = (me.position.z - player.position.z).abs();
    dx < reach && dz < reach && (me.position.y - player.position.y).abs() < 2.0
}

/// `FleeSunGoal.getHidePos`：10 次随机采样，找看不见天且路径代价小于 0 的格子。
fn flee_sun_target(me: &mut Entity, _mob: &Mob, ctx: &dyn EntityCtx) -> Option<Vec3> {
    let centre = me.block_position();
    for _ in 0..10 {
        let x = me.random.next_int_bounded(20) - 10;
        let y = me.random.next_int_bounded(6) - 3;
        let z = me.random.next_int_bounded(20) - 10;
        let pos = centre.offset(x, y, z);
        if ctx.blocks().can_see_sky(pos) {
            continue;
        }
        let cost = monster_walk_target_value(ctx.blocks(), pos);
        if cost < 0.0 {
            return Some(Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y),
                f64::from(pos.z) + 0.5,
            ));
        }
    }
    None
}

/// `Monster.getWalkTargetValue = -LevelReader.getPathfindingCostFromLightLevels`。
pub(crate) fn monster_walk_target_value(view: &dyn BlockView, pos: BlockPos) -> f32 {
    let raw = view.max_local_raw_brightness(pos, view.sky_darken()) as f32 / 15.0;
    let magic = raw / (4.0 - 3.0 * raw);
    -(magic - 0.5)
}

/// `MoveToBlockGoal.findNearestBlock`：以实体为中心搜海龟蛋。
fn nearest_turtle_egg(me: &Entity, ctx: &dyn EntityCtx) -> Option<BlockPos> {
    let centre = me.block_position();
    let view = ctx.blocks();
    for dy in -3..=3 {
        for dz in -3..=3 {
            for dx in -3..=3 {
                let pos = centre.offset(dx, dy, dz);
                if view.block_state(pos).block_id() as u32 == mc_bamboo_data::blocks::TURTLE_EGG {
                    return Some(pos);
                }
            }
        }
    }
    None
}
