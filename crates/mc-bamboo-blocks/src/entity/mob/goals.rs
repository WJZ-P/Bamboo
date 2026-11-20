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
        /// `MeleeAttackGoal.canUse` 算出的路径，官方存在字段里由 `start()` 装入导航。
        start_path: Option<super::path::Path>,
    },
    /// `HurtByTargetGoal`：记住上一次伤害的时间戳，把攻击者设成目标。
    HurtByTarget {
        timestamp: i32,
        /// `TargetGoal.unseenTicks`：连续看不见目标的刻数（`mustSee` 为真）。
        unseen_ticks: i32,
        /// `TargetGoal.targetMob`：`start` 时记下的目标，`getTarget()` 读成空时回退到它。
        target_mob: Option<EntityUid>,
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
            // `DoorInteractGoal` 的构造函数不设标志位。
            Self::BreakDoor => &[],
        }
    }

    /// `requiresUpdateEveryTick`。
    pub const fn every_tick(&self) -> bool {
        // 字节码逐个核过 `Goal.requiresUpdateEveryTick`：
        // Float/RandomLookAround/MeleeAttack/MoveToBlock/DoorInteract 为真，
        // LookAtPlayer/RandomStroll/LookAround 的其它目标为假。
        matches!(
            self,
            Self::Float
                | Self::RandomLookAround { .. }
                | Self::MeleeAttack { .. }
                | Self::TurtleEgg { .. }
                | Self::BreakDoor
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

    /// `Zombie.registerGoals` + `addBehaviourGoals`。
    /// `GoalSelector.availableGoals` 是 `ObjectLinkedOpenHashSet`：按 `addGoal`
    /// 的调用顺序迭代，不按优先级排。官方字节码里 `registerGoals` 先注册
    /// `(4, ZombieAttackTurtleEggGoal)`、`(8, LookAtPlayerGoal)`、
    /// `(8, RandomLookAroundGoal)`，随后 `addBehaviourGoals` 依次注册
    /// `(2, SpearUseGoal)`、`(3, ZombieAttackGoal)`、
    /// `(6, MoveThroughVillageGoal)`、`(7, WaterAvoidingRandomStrollGoal)`。
    /// 近战第一次启动前，LOOK 还没被锁住，两个看的目标先各取一次 `nextFloat`；
    /// 按优先级排序会让近战先锁 LOOK，随机源少两次取数。
    /// `BreakDoorGoal(1)` 只在 `setCanBreakDoors(true)` 时追加到末尾。
    pub fn zombie(_navigation: &mut Navigation) -> Self {
        let wrap = |priority, goal| WrappedGoal {
            priority,
            running: false,
            goal,
        };
        let turtle_egg = wrap(4, Goal::TurtleEgg { speed: 1.0 });
        let look = wrap(
            8,
            Goal::LookAtPlayer {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        );
        let look_around = wrap(
            8,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        );
        let goals = vec![
            turtle_egg,
            look,
            look_around,
            wrap(2, Goal::Unavailable("SpearUseGoal")),
            wrap(
                3,
                Goal::MeleeAttack {
                    speed: 1.0,
                    ticks_until_next_attack: 0,
                    ticks_until_path_recalc: 0,
                    pathed_target: Vec3::default(),
                    raise_arm_ticks: 0,
                    last_can_use_check: 0,
                    start_path: None,
                },
            ),
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
        ];
        Self {
            goals,
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
                wrap(
                    1,
                    Goal::HurtByTarget {
                        timestamp: -1,
                        unseen_ticks: 0,
                        target_mob: None,
                    },
                ),
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

    /// `setCanBreakDoors(true)` 时官方在优先级 1 追加 `BreakDoorGoal`。
    /// `ObjectLinkedOpenHashSet` 的新元素排在末尾，移除后再加仍排在末尾。
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

    /// `setCanBreakDoors(false)` 时官方 `removeGoal(BreakDoorGoal)`。
    pub fn remove_break_door(&mut self) {
        self.goals
            .retain(|wrapped| !matches!(wrapped.goal, Goal::BreakDoor));
    }

    /// 对拍的改坏确认用：把目标按优先级排序（官方按注册顺序）。
    pub fn sort_by_priority_for_oracle(&mut self) {
        self.goals.sort_by_key(|wrapped| wrapped.priority);
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

/// `Goal.adjustedTickDelay`：`requiresUpdateEveryTick()` 为真时原样返回，
/// 为假时才折半。
const fn adjusted_tick_delay(every_tick: bool, ticks: i32) -> i32 {
    if every_tick {
        ticks
    } else {
        reduced_tick_delay(ticks)
    }
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
                // 官方 `RandomStrollGoal.canUse` 用 `nextInt(reducedTickDelay(interval))`；
                // 牛和僵尸都用 120，所以是 `nextInt(60)`。
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
            last_can_use_check,
            start_path,
            ..
        } => {
            // 官方 `lastCanUseCheck` 初值是 0：第一个满刻（1001）就允许，
            // 之后每 20 刻一次。Bamboo 之前给了一个很大的负数，语义等价，
            // 但 `gameTime - last` 用 long 减法，这里按官方改成初值 0。
            let now = ctx.blocks().game_time();
            if now - *last_can_use_check < 20 {
                return false;
            }
            *last_can_use_check = now;
            let Some(target) = target_player(mob, ctx) else {
                return false;
            };
            if !target.alive {
                return false;
            }
            let nav = living::nav_mob(me, mob, ctx.blocks());
            *start_path = mob.navigation.create_path(
                ctx.blocks(),
                &nav,
                BlockPos::containing(target.position),
                0,
            );
            start_path.is_some() || within_melee_range(me, mob, target)
        }
        Goal::HurtByTarget { timestamp, .. } => {
            let Some(last) = mob.last_hurt_by_mob else {
                return false;
            };
            if mob.last_hurt_by_mob_timestamp == *timestamp {
                return false;
            }
            // `canAttack(last, HURT_BY_TARGETING)`：战斗条件，不看距离、视线和隐身。
            // 打它的是别的生物时（本期只有玩家会打僵尸）不在这里拦。
            match player_by_uid(ctx.blocks(), last) {
                Some(player) => {
                    player.alive
                        && !player.spectator
                        && can_attack_player(ctx.blocks(), &player)
                        && !allied(mob, ctx.blocks(), &player)
                }
                None => true,
            }
        }
        Goal::NearestAttackableTarget {
            kind,
            interval,
            found,
            ..
        } => {
            // 官方先按 `randomInterval` 取数，再按类型找目标。缺实体的目标
            // 虽跳过搜索，取数次数必须和官方一致，否则随机源会错开。
            if *interval > 0 && me.random.next_int_bounded(*interval) != 0 {
                return false;
            }
            // 官方每个 `NearestAttackableTargetGoal` 都先取 `nextInt(interval)`；
            // 缺实体的目标仍注册、仍取数，只是查不到目标（场景里明确无此类实体）。
            *found = if kind.available() {
                nearest_attackable(me, mob, ctx, *kind)
            } else {
                None
            };
            found.is_some()
        }
        // `RestrictSunGoal.canUse`：白天、头上没东西、地面导航（僵尸恒是）。
        // 官方这里不看 `MONSTERS_BURN`。
        Goal::RestrictSun => {
            ctx.blocks().is_bright_outside()
                && mob
                    .equipment
                    .get(mc_bamboo_protocol::packet::play::EquipmentSlot::Head)
                    .is_empty()
        }
        Goal::FleeSun { wanted, .. } => {
            mob.target.is_none()
                && ctx.blocks().is_bright_outside()
                && me.remaining_fire_ticks > 0
                && ctx.blocks().can_see_sky(me.block_position())
                && mob
                    .equipment
                    .get(mc_bamboo_protocol::packet::play::EquipmentSlot::Head)
                    .is_empty()
                && flee_sun_target(me, mob, ctx).is_some_and(|at| {
                    *wanted = at;
                    true
                })
        }
        // `BreakDoorGoal.canUse`：先 `DoorInteractGoal.canUse`（会写 `doorPos` / `hasDoor`），
        // 再看 `mobGriefing`、困难难度和门没开。
        Goal::BreakDoor => {
            door_interact_can_use(me, mob, ctx.blocks())
                && ctx.blocks().block_game_rules().mob_griefing
                && ctx.blocks().difficulty() == 3
                && !door_is_open(mob, ctx.blocks())
        }
        Goal::TurtleEgg { .. } => {
            // `RemoveBlockGoal.canUse` 覆写了 `MoveToBlockGoal`：
            // mobGriefing → 倒计时 → 先 `findNearestBlock()`，命中才设 10；
            // 未命中才取 `nextInt(200)` 设下次倒计时。
            if !ctx.blocks().block_game_rules().mob_griefing {
                return false;
            }
            if mob.turtle_egg_next_start_tick > 0 {
                mob.turtle_egg_next_start_tick -= 1;
                return false;
            }
            let egg = nearest_turtle_egg(me, ctx);
            mob.turtle_egg_pos = egg;
            if egg.is_some() {
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
        Goal::MeleeAttack { .. } => {
            // 官方 `followingTargetEvenIfNotSeen=false`：目标活着且
            // `!navigation.isDone()` 才继续；路径走完就停。
            target_player(mob, ctx).is_some() && !mob.navigation.is_done()
        }
        Goal::HurtByTarget {
            unseen_ticks,
            target_mob,
            ..
        } => {
            // `HurtByTargetGoal.start` 把 `unseenMemoryTicks` 改成 300。
            let target = target_player(mob, ctx)
                .map(|player| player.uid)
                .or(*target_mob);
            target_continues(me, mob, ctx, target, unseen_ticks, 300)
        }
        Goal::NearestAttackableTarget { unseen_ticks, .. } => {
            // 这个目标不写 `targetMob`，`getTarget()` 读成空就停。
            let target = target_player(mob, ctx).map(|player| player.uid);
            target_continues(me, mob, ctx, target, unseen_ticks, 60)
        }
        Goal::RestrictSun => {
            ctx.blocks().is_bright_outside()
                && mob
                    .equipment
                    .get(mc_bamboo_protocol::packet::play::EquipmentSlot::Head)
                    .is_empty()
        }
        Goal::FleeSun { .. } => !mob.navigation.is_done(),
        // `BreakDoorGoal.canContinueToUse`：没到时间、门没开、离门中心不到 2 格、仍是困难。
        Goal::BreakDoor => {
            mob.door_break_time <= DOOR_BREAK_TIME
                && !door_is_open(mob, ctx.blocks())
                && center_distance_sqr(mob.door_pos, me.position) < 4.0
                && ctx.blocks().difficulty() == 3
        }
        Goal::TurtleEgg { .. } => {
            // `MoveToBlockGoal.canContinueToUse`：尝试刻数在
            // `[-maxStayTicks, 1200]` 内且目标方块仍然有效。
            mob.turtle_egg_try_ticks >= -mob.turtle_egg_max_stay
                && mob.turtle_egg_try_ticks <= 1200
                && mob
                    .turtle_egg_pos
                    .is_some_and(|pos| valid_turtle_egg_target(ctx.blocks(), pos))
        }
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
            speed,
            ticks_until_next_attack,
            ticks_until_path_recalc,
            raise_arm_ticks,
            start_path,
            ..
        } => {
            if let Some(path) = start_path.take() {
                let nav = living::nav_mob(me, mob, ctx.blocks());
                mob.navigation
                    .move_to_path(ctx.blocks(), &nav, Some(path), *speed);
            }
            *ticks_until_next_attack = 0;
            *ticks_until_path_recalc = 0;
            *raise_arm_ticks = 0;
            // 官方 `MeleeAttackGoal.start()` 显式置真；同一刻的
            // `ZombieAttackGoal.tick()` 会按抬手计时再覆盖。
            mob.aggressive = true;
        }
        Goal::HurtByTarget {
            timestamp,
            unseen_ticks,
            target_mob,
        } => {
            // `setTarget(lastHurtByMob)` 经过 `asValidTarget`，`targetMob = getTarget()`。
            mob.target = mob.last_hurt_by_mob;
            *target_mob = target_player(mob, ctx).map(|player| player.uid);
            *timestamp = mob.last_hurt_by_mob_timestamp;
            *unseen_ticks = 0;
        }
        Goal::NearestAttackableTarget {
            found,
            unseen_ticks,
            ..
        } => {
            mob.target = *found;
            *unseen_ticks = 0;
        }
        // `RestrictSunGoal.start`：`setAvoidSun(true)`。
        Goal::RestrictSun => mob.navigation.avoid_sun = true,
        Goal::FleeSun { speed, wanted } => {
            let nav = living::nav_mob(me, mob, ctx.blocks());
            mob.navigation.move_to(ctx.blocks(), &nav, *wanted, *speed);
        }
        // `DoorInteractGoal.start` 记下开门方向，`BreakDoorGoal.start` 清计时。
        Goal::BreakDoor => {
            mob.door_passed = false;
            mob.door_open_dir = [
                (f64::from(mob.door_pos.x) + 0.5 - me.position.x) as f32,
                (f64::from(mob.door_pos.z) + 0.5 - me.position.z) as f32,
            ];
            mob.door_break_time = 0;
        }
        Goal::TurtleEgg { speed } => {
            // `MoveToBlockGoal.start()`：先 `moveMobToBlock()`，再清 `tryTicks`，
            // 最后取 `maxStayTicks = 1200 + nextInt(1200 + nextInt(1200))`。
            if let Some(pos) = mob.turtle_egg_pos {
                let nav = living::nav_mob(me, mob, ctx.blocks());
                mob.navigation.move_to(
                    ctx.blocks(),
                    &nav,
                    Vec3::new(
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y + 1),
                        f64::from(pos.z) + 0.5,
                    ),
                    *speed,
                );
            }
            mob.turtle_egg_try_ticks = 0;
            let inner = me.random.next_int_bounded(1200);
            mob.turtle_egg_max_stay = 1200 + me.random.next_int_bounded(1200 + inner);
            mob.turtle_egg_reached = false;
            mob.turtle_egg_ticks_since_reached = 0;
        }
    }
}

fn stop_goal(goal: &mut Goal, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
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
        Goal::Float | Goal::Panic { .. } | Goal::RandomLookAround { .. } | Goal::Unavailable(_) => {
        }
        // `RestrictSunGoal.stop`：`setAvoidSun(false)`。
        Goal::RestrictSun => mob.navigation.avoid_sun = false,
        // `TargetGoal.stop`：`setTarget(null)`，`targetMob = null`。
        Goal::HurtByTarget { target_mob, .. } => {
            mob.target = None;
            *target_mob = None;
        }
        Goal::NearestAttackableTarget { found, .. } => {
            mob.target = None;
            *found = None;
        }
        Goal::MeleeAttack { .. } => {
            // `MeleeAttackGoal.stop()`：目标是创造或旁观模式的玩家才清掉
            // （`NO_CREATIVE_OR_SPECTATOR`），普通玩家目标保留。
            let excluded = mob
                .target
                .and_then(|uid| player_by_uid(ctx.blocks(), uid))
                .is_some_and(|player| player.creative || player.spectator);
            if excluded {
                mob.target = None;
            }
            mob.aggressive = false;
            mob.navigation.stop();
        }
        Goal::FleeSun { .. } => mob.navigation.stop(),
        // `BreakDoorGoal.stop`：清掉裂纹（`lastBreakProgress` 不重置）。
        Goal::BreakDoor => {
            ctx.emit(
                crate::entity::ctx::Command::DestroyProgress {
                    pos: mob.door_pos,
                    progress: -1,
                },
                me,
            );
        }
        Goal::TurtleEgg { .. } => {
            // `RemoveBlockGoal.stop`：落地前的摔落距离按 1 格算。
            me.fall_distance = 1.0;
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
                .move_to_entity(ctx.blocks(), &nav, other.position, *speed, 1);
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
                    .move_to_entity(ctx.blocks(), &nav, player.position, *speed, 1);
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
                    .move_to_entity(ctx.blocks(), &nav, other.position, *speed, 1);
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
        // `TargetGoal` 没有 `tick`，视线记忆在 `canContinueToUse` 里推进。
        Goal::Unavailable(_)
        | Goal::HurtByTarget { .. }
        | Goal::NearestAttackableTarget { .. }
        | Goal::RestrictSun => {}
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
            // 官方 `MeleeAttackGoal.tick`：僵尸的 `followingTargetEvenIfNotSeen` 为假，
            // 看不见目标时不重新寻路；后面三个短路串行，
            // `nextFloat` 只在「pathed 非零且目标离它 < 1」时被求值。
            if living::has_line_of_sight(me, mob, ctx.blocks(), target.uid, target.eye_position())
                && *ticks_until_path_recalc == 0
                && (*pathed_target == Vec3::default()
                    || target.position.distance_to_sqr(*pathed_target) >= 1.0
                    || me.random.next_float() < 0.05)
            {
                *pathed_target = target.position;
                *ticks_until_path_recalc = 4 + me.random.next_int_bounded(7);
                let distance = me.position.distance_to_sqr(target.position);
                if distance > 1024.0 {
                    *ticks_until_path_recalc += 10;
                } else if distance > 256.0 {
                    *ticks_until_path_recalc += 5;
                }
                let nav = living::nav_mob(me, mob, ctx.blocks());
                if !mob
                    .navigation
                    .move_to_entity(ctx.blocks(), &nav, target.position, *speed, 0)
                {
                    *ticks_until_path_recalc += 15;
                }
                *ticks_until_path_recalc = adjusted_tick_delay(true, *ticks_until_path_recalc);
            }
            *ticks_until_next_attack = (*ticks_until_next_attack - 1).max(0);
            if *ticks_until_next_attack == 0
                && within_melee_range(me, mob, target)
                && living::has_line_of_sight(
                    me,
                    mob,
                    ctx.blocks(),
                    target.uid,
                    target.eye_position(),
                )
            {
                *ticks_until_next_attack = adjusted_tick_delay(true, 20);
                super::living::do_hurt_target(me, mob, ctx, target.uid);
            }
            // `ZombieAttackGoal.tick`：`super.tick()` 之后抬手计时加一，按新的
            // `ticksUntilNextAttack` 决定举不举手。
            *raise_arm_ticks += 1;
            mob.aggressive = *raise_arm_ticks >= 5
                && *ticks_until_next_attack < adjusted_tick_delay(true, 20) / 2;
        }
        Goal::FleeSun { .. } => {}
        Goal::BreakDoor => {
            // `DoorInteractGoal.tick`：走过门中心就记成已通过。
            let to_x = (f64::from(mob.door_pos.x) + 0.5 - me.position.x) as f32;
            let to_z = (f64::from(mob.door_pos.z) + 0.5 - me.position.z) as f32;
            if mob.door_open_dir[0] * to_x + mob.door_open_dir[1] * to_z < 0.0 {
                mob.door_passed = true;
            }
            // `BreakDoorGoal.tick`：1/20 敲门声（挥手动画这一期不建模，不取随机数），
            // 每十分之一进度发一次裂纹，到点在困难难度下拆门。
            if me.random.next_int_bounded(20) == 0 {
                ctx.emit(
                    crate::entity::ctx::Command::MobLevelEvent {
                        event: 1019,
                        pos: mob.door_pos,
                        data: 0,
                    },
                    me,
                );
            }
            mob.door_break_time += 1;
            let progress = (mob.door_break_time as f32 / DOOR_BREAK_TIME as f32 * 10.0) as i32;
            if progress != mob.door_break_progress {
                ctx.emit(
                    crate::entity::ctx::Command::DestroyProgress {
                        pos: mob.door_pos,
                        progress,
                    },
                    me,
                );
                mob.door_break_progress = progress;
            }
            if mob.door_break_time == DOOR_BREAK_TIME && ctx.blocks().difficulty() == 3 {
                ctx.emit(
                    crate::entity::ctx::Command::BreakDoor { pos: mob.door_pos },
                    me,
                );
            }
        }
        Goal::TurtleEgg { speed } => {
            let Some(egg) = mob.turtle_egg_pos else {
                return;
            };
            // `MoveToBlockGoal.tick`：目标格是蛋的上方，`closerToCenterThan`
            // 按 1.14 的欧氏距离（含 Y）判定；没到时每 40 个尝试刻才重算一次路径。
            let target = egg.above();
            let centre = Vec3::new(
                f64::from(target.x) + 0.5,
                f64::from(target.y) + 0.5,
                f64::from(target.z) + 0.5,
            );
            if me.position.distance_to_sqr(centre) >= 1.14 * 1.14 {
                mob.turtle_egg_reached = false;
                mob.turtle_egg_try_ticks += 1;
                if mob.turtle_egg_try_ticks % 40 == 0 {
                    let nav = living::nav_mob(me, mob, ctx.blocks());
                    mob.navigation.move_to(
                        ctx.blocks(),
                        &nav,
                        Vec3::new(
                            f64::from(target.x) + 0.5,
                            f64::from(target.y),
                            f64::from(target.z) + 0.5,
                        ),
                        *speed,
                    );
                }
            } else {
                mob.turtle_egg_reached = true;
                mob.turtle_egg_try_ticks -= 1;
            }
            // `RemoveBlockGoal.tick`：到达目标且脚边找得到蛋时，先往上一跳、撒蛋壳，
            // 隔刻往下压并每 6 刻响一次啄蛋声，超过 60 刻把蛋移除。
            // 粒子包这一期不发，只按官方顺序取生物随机数。
            let found = turtle_egg_at(ctx.blocks(), me.block_position());
            if !mob.turtle_egg_reached {
                return;
            }
            let Some(pos) = found else {
                return;
            };
            let since = mob.turtle_egg_ticks_since_reached;
            if since > 0 {
                me.delta = Vec3::new(me.delta.x, 0.3, me.delta.z);
                for _ in 0..3 {
                    me.random.next_float();
                }
            }
            if since % 2 == 0 {
                me.delta = Vec3::new(me.delta.x, -0.3, me.delta.z);
                if since % 6 == 0 {
                    // `ZombieAttackTurtleEggGoal.playDestroyProgressSound`：放在目标蛋上，
                    // 音高取生物自己的随机数。
                    let pitch = 0.9 + me.random.next_float() * 0.2;
                    ctx.emit(
                        crate::entity::ctx::Command::MobBlockSound {
                            pos: egg,
                            sound: "minecraft:entity.zombie.destroy_egg",
                            source: mc_bamboo_data::sounds::HOSTILE,
                            volume: 0.5,
                            pitch,
                        },
                        me,
                    );
                }
            }
            if since > 60 {
                // `level.removeBlock(pos, false)`：只换成流体或空气，没有 2001 事件。
                ctx.emit(crate::entity::ctx::Command::RemoveMobBlock { pos }, me);
                // 20 团烟雾，每团三个生物随机数的高斯偏移。
                for _ in 0..60 {
                    me.random.next_gaussian();
                }
                // `playBreakSound`：音高取世界随机数，落地时取。
                ctx.emit(crate::entity::ctx::Command::TurtleEggBreakSound { pos }, me);
            }
            mob.turtle_egg_ticks_since_reached += 1;
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

/// `BreakDoorGoal.getDoorBreakTime()`：`max(240, doorBreakTime)`，僵尸用的构造函数给 -1。
const DOOR_BREAK_TIME: i32 = 240;

/// `DoorBlock.isWoodenDoor`：是门方块，且门的类型能用手开（铁门不行）。
fn is_wooden_door(view: &dyn BlockView, pos: BlockPos) -> bool {
    let state = view.block_state(pos);
    state.is_a(mc_bamboo_data::BlockAncestor::DoorBlock)
        && crate::behaviour::common::field_bool(state.block(), "type.canOpenByHand")
}

/// `DoorInteractGoal.isOpen`：没有门时假；门已经不是门时顺手清掉 `hasDoor`。
fn door_is_open(mob: &mut Mob, view: &dyn BlockView) -> bool {
    if !mob.has_door {
        return false;
    }
    let state = view.block_state(mob.door_pos);
    if !state.is_a(mc_bamboo_data::BlockAncestor::DoorBlock) {
        mob.has_door = false;
        return false;
    }
    state.get(crate::state::OPEN)
}

/// `DoorInteractGoal.canUse`：撞墙了、路径没走完时，在当前和下一个节点里找离自己
/// 1.5 格内的木门（按节点上方一格、水平距离用节点整数坐标）；都没有就看头顶这一格。
fn door_interact_can_use(me: &Entity, mob: &mut Mob, view: &dyn BlockView) -> bool {
    if !me.horizontal_collision {
        return false;
    }
    let Some(path) = &mob.navigation.path else {
        return false;
    };
    if path.is_done() {
        return false;
    }
    let end = (path.next_node_index + 2).min(path.node_count());
    let mut found = None;
    for node in &path.nodes[..end] {
        let door = BlockPos::new(node.x, node.y + 1, node.z);
        let dx = f64::from(door.x) - me.position.x;
        let dz = f64::from(door.z) - me.position.z;
        // `distanceToSqr(doorX, getY(), doorZ)`：Y 用自己的，差为 0。
        if dx * dx + dz * dz > 2.25 {
            continue;
        }
        let wooden = is_wooden_door(view, door);
        mob.door_pos = door;
        mob.has_door = wooden;
        if wooden {
            found = Some(door);
            break;
        }
    }
    if found.is_some() {
        return true;
    }
    let door = me.block_position().above();
    mob.door_pos = door;
    mob.has_door = is_wooden_door(view, door);
    mob.has_door
}

/// `Vec3i.distToCenterSqr`：到方块中心的距离平方。
fn center_distance_sqr(pos: BlockPos, at: Vec3) -> f64 {
    let dx = f64::from(pos.x) + 0.5 - at.x;
    let dy = f64::from(pos.y) + 0.5 - at.y;
    let dz = f64::from(pos.z) + 0.5 - at.z;
    dx * dx + dy * dy + dz * dz
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

/// `LivingEntity.canAttack(player)`：和平难度打不了玩家，否则看 `canBeSeenAsEnemy`
/// （活着、不是旁观、不是创造的无敌）。
fn can_attack_player(view: &dyn BlockView, player: &PlayerView) -> bool {
    view.difficulty() != 0 && player.alive && !player.spectator && !player.creative
}

/// `Entity.isAlliedTo(player)`：生物在队伍里，而且和玩家是同一队。
fn allied(mob: &Mob, view: &dyn BlockView, player: &PlayerView) -> bool {
    view.entity_team(&mob.uuid)
        .is_some_and(|team| player.team == Some(team))
}

/// `TargetGoal.canContinueToUse`（`mustSee` 为真，`unseenMemoryTicks` 由调用方给）：
/// 目标打得了、不同队、在跟随距离内；看得见就清零记忆，看不见超过
/// `reducedTickDelay(memory)` 刻就放弃。继续时 `setTarget(target)`。
fn target_continues(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    target: Option<EntityUid>,
    unseen_ticks: &mut i32,
    memory: i32,
) -> bool {
    let view = ctx.blocks();
    let Some(player) = target.and_then(|uid| player_by_uid(view, uid)) else {
        return false;
    };
    if !can_attack_player(view, &player) {
        return false;
    }
    // `mob.getTeam() != null && target.getTeam() == mob.getTeam()`。
    if allied(mob, view, &player) {
        return false;
    }
    let range = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE);
    if me.position.distance_to_sqr(player.position) > range * range {
        return false;
    }
    if living::has_line_of_sight(me, mob, view, player.uid, player.eye_position()) {
        *unseen_ticks = 0;
    } else {
        *unseen_ticks += 1;
        if *unseen_ticks > reduced_tick_delay(memory) {
            return false;
        }
    }
    mob.target = Some(player.uid);
    true
}

/// `TargetingConditions.test` 里和玩家有关的部分，`range` 为 0 时不看距离。
fn targeting_test(
    me: &Entity,
    mob: &mut Mob,
    view: &dyn BlockView,
    player: &PlayerView,
    range: f64,
    check_line_of_sight: bool,
    combat: bool,
) -> bool {
    if player.spectator || !player.alive {
        return false;
    }
    // `isCombat`：`attacker.canAttack(target) && !attacker.isAlliedTo(target)`。
    if combat && (!can_attack_player(view, player) || allied(mob, view, player)) {
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
        if !targeting_test(me, mob, view, &player, range, true, false) {
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
        if !targeting_test(me, mob, view, &player, range, false, false) {
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

/// `Animal.getWalkTargetValue`：脚下是草方块给 10，否则按亮度；
/// `Monster.getWalkTargetValue` 取负的光照路径代价。
fn walk_target_value(mob: &Mob, view: &dyn BlockView, pos: BlockPos) -> f32 {
    if mob.mob_type.is_monster() {
        return monster_walk_target_value(view, pos);
    }
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
fn pick_best(
    mob: &Mob,
    view: &dyn BlockView,
    mut supplier: impl FnMut() -> Option<BlockPos>,
) -> Option<Vec3> {
    let mut best = f64::NEG_INFINITY;
    let mut best_pos = None;
    for _ in 0..10 {
        let Some(pos) = supplier() else {
            continue;
        };
        let weight = f64::from(walk_target_value(mob, view, pos));
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
    pick_best(mob, view, || {
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
    pick_best(mob, view, || {
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
    // 官方 `TargetingConditions.forCombat().test` 的视线要求由
    // `NearestAttackableTargetGoal` 在选择目标时已判；近战目标只看活着、非旁观/创造。
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
        if !targeting_test(me, mob, view, &player, range, true, true) {
            continue;
        }
        // `getNearestEntity`：玩家脚下的位置到生物 (x, eyeY, z) 的距离。
        let distance = player.position.distance_to_sqr(eye);
        if distance < best {
            best = distance;
            found = Some(player.uid);
        }
    }
    found
}

/// `PathfinderMob.isWithinMeleeAttackRange`：用实体交互范围算的方形距离。
fn within_melee_range(me: &Entity, _mob: &Mob, player: PlayerView) -> bool {
    // `Mob.DEFAULT_ATTACK_REACH = sqrt(2.04f) - 0.6f`（26.3 字节码）。
    // 官方用 `getAttackBoundingBox(reach).intersects(target.getHitbox())`，
    // 这里按双方 Aabb 膨胀后的相交判定，而不是旧的「交互范围 + 1」方格。
    let reach = 2.039_999_961_853_027_3_f64.sqrt() - f64::from(0.6_f32);
    let (width, height) = _mob.dimensions();
    let half = f64::from(width) / 2.0;
    let ours = crate::shape::Aabb::new(
        me.position.x - half - reach,
        me.position.y - reach,
        me.position.z - half - reach,
        me.position.x + half + reach,
        me.position.y + f64::from(height) + reach,
        me.position.z + half + reach,
    );
    let theirs = crate::shape::Aabb::new(
        player.position.x - 0.3,
        player.position.y,
        player.position.z - 0.3,
        player.position.x + 0.3,
        player.position.y + 1.8,
        player.position.z + 0.3,
    );
    ours.intersects(theirs)
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

/// `RemoveBlockGoal.isValidTarget`：区块在、蛋在、上面两格是空气。
fn valid_turtle_egg_target(view: &dyn BlockView, pos: BlockPos) -> bool {
    view.is_loaded(pos)
        && view.block_state(pos).is(mc_bamboo_data::blocks::TURTLE_EGG)
        && view.block_state(pos.above()).is_air()
        && view.block_state(pos.above().above()).is_air()
}

/// `RemoveBlockGoal.getPosWithBlock`：实体脚下附近 6 格里第一块海龟蛋。
fn turtle_egg_at(view: &dyn BlockView, pos: BlockPos) -> Option<BlockPos> {
    let egg = mc_bamboo_data::blocks::TURTLE_EGG;
    if view.block_state(pos).is(egg) {
        return Some(pos);
    }
    let below = pos.below();
    [
        below,
        pos.west(),
        pos.east(),
        pos.north(),
        pos.south(),
        below.below(),
    ]
    .into_iter()
    .find(|candidate| view.block_state(*candidate).is(egg))
}

/// `MoveToBlockGoal.findNearestBlock`：以实体脚下一格为基准搜海龟蛋。
///
/// `RemoveBlockGoal` 构造 `MoveToBlockGoal(mob, speed, 24, 3)`。官方的三层循环都按
/// `0, 1, -1, 2, -2, …` 的顺序走：纵向 `y` 从 0 到 3，水平半径 `r` 从 0 到 23，
/// 每圈先 `x` 后 `z`，`|x| < r` 时 `z` 只取 `±r`。第一个 `isValidTarget` 的格子就是目标。
fn nearest_turtle_egg(me: &Entity, ctx: &dyn EntityCtx) -> Option<BlockPos> {
    const SEARCH_RANGE: i32 = 24;
    const VERTICAL_RANGE: i32 = 3;
    let next = |v: i32| if v > 0 { -v } else { 1 - v };
    let centre = me.block_position();
    let view = ctx.blocks();
    let mut y = 0;
    while y <= VERTICAL_RANGE {
        for r in 0..SEARCH_RANGE {
            let mut x = 0;
            while x <= r {
                let mut z = if x < r && x > -r { r } else { 0 };
                while z <= r {
                    let pos = centre.offset(x, y - 1, z);
                    if valid_turtle_egg_target(view, pos) {
                        return Some(pos);
                    }
                    z = next(z);
                }
                x = next(x);
            }
        }
        y = next(y);
    }
    None
}
