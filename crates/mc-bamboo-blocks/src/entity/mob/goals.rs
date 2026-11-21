//! 目标：官方 `GoalSelector`、`WrappedGoal` 和牛注册的八个目标，外加 `RandomPos` 一族。
//!
//! 调度规则和官方逐条一致：满刻（`(tickCount + id) % 2 == 0` 或前两刻）先停、再按
//! 加入顺序尝试启动，标志位按 MOVE、LOOK、JUMP、TARGET 的顺序锁；非满刻只跑
//! 每刻都要更新的目标。随机数一律取实体自己的随机源，取数的先后和官方相同。
//!
//! Goals: vanilla `GoalSelector`, `WrappedGoal`, the eight cow goals and the `RandomPos`
//! family, with identical scheduling and random draw order.

use mc_bamboo_core::{Aabb, BlockPos, Vec3, mth};
use mc_bamboo_data::{blocks, tags::fluid_tags};

use super::{Mob, living, navigation::Navigation, path::static_path_type};
use crate::{
    entity::{
        Entity, EntityUid, KindPublic,
        ctx::{BlockView, EntityCtx, PlayerView},
    },
    shape::{self, CollisionContext},
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
        steering_item: bool,
        speed: f64,
        player: Option<EntityUid>,
        calm_down: i32,
    },
    FollowParent {
        speed: f64,
        parent: Option<EntityUid>,
        time_to_recalc_path: i32,
    },
    /// `WaterAvoidingRandomStrollGoal`；`probability` 取 [`PLAIN_STROLL`] 时是不避水的
    /// `RandomStrollGoal`，不抽那个 `nextFloat`。
    ///
    /// `WaterAvoidingRandomStrollGoal`; with `probability` set to [`PLAIN_STROLL`] it is the
    /// plain `RandomStrollGoal`, which skips that `nextFloat`.
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
    Bow(super::skeleton::BowGoal),
    /// `HurtByTargetGoal`：记住上一次伤害的时间戳，把攻击者设成目标。
    HurtByTarget {
        timestamp: i32,
        /// `TargetGoal.unseenTicks`；只有北极熊按 `mustSee` 记（300 刻的记忆）。
        ///
        /// `TargetGoal.unseenTicks`; only polar bears track `mustSee` (a 300 tick memory).
        unseen_ticks: i32,
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
    /// `RandomStandGoal`：马科偶尔扬蹄，`nextStand` 从 `-getAmbientStandInterval()` 往上数。
    ///
    /// `RandomStandGoal`: equines rear now and then; `nextStand` counts up from
    /// `-getAmbientStandInterval()`.
    RandomStand {
        next_stand: i32,
    },
    /// 羊驼的 `RangedAttackGoal(this, 1.25, 40, 20.0F)`：目标存在自己身上，
    /// 目标被目标表清掉以后还能凭「活着且路没走完」继续。
    ///
    /// The llama's `RangedAttackGoal(this, 1.25, 40, 20.0F)`. It keeps its own
    /// target, so it may continue after the target selector clears the mob's.
    RangedAttack {
        target: Option<EntityUid>,
        attack_time: i32,
        see_time: i32,
    },
    /// `Llama.LlamaHurtByTargetGoal`：`HurtByTargetGoal`，吐过口水就停。
    ///
    /// `Llama.LlamaHurtByTargetGoal`: a `HurtByTargetGoal` that stops after a spit.
    LlamaHurtByTarget {
        timestamp: i32,
        unseen_ticks: i32,
    },
    /// `EatBlockGoal`：羊低头吃草的 40 刻（非每刻目标，折半成 20）。
    ///
    /// `EatBlockGoal`: the sheep's grazing animation (40 ticks, halved to 20).
    EatBlock {
        eat_tick: i32,
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
    /// 狐狸（北极熊的猎物），Bamboo 还没有。
    ///
    /// Foxes (polar bear prey); Bamboo has none yet.
    Fox,
    /// `PolarBearAttackPlayersGoal`：身边有幼崽的成年北极熊找玩家，搜索和跟随距离减半。
    ///
    /// `PolarBearAttackPlayersGoal`: an adult polar bear with a cub nearby looks for
    /// players at half the follow distance.
    GuardedPlayer,
    /// 选择器是 `NeutralMob.isAngryAt` 的玩家目标。
    ///
    /// A player target whose selector is `NeutralMob.isAngryAt`.
    AngerPlayer,
    /// 羊驼的 `LlamaAttackWolfGoal`：没驯服的狼。
    ///
    /// The llama's `LlamaAttackWolfGoal`: untamed wolves.
    Wolf,
}

impl TargetType {
    /// Bamboo 现在有没有这种实体；没有时对应的目标直接跳过。
    pub const fn available(self) -> bool {
        matches!(self, Self::Player | Self::GuardedPlayer | Self::AngerPlayer)
    }

    /// `TargetGoal.getFollowDistance` 相对 `FOLLOW_RANGE` 的倍数。
    ///
    /// The `TargetGoal.getFollowDistance` factor on `FOLLOW_RANGE`.
    const fn follow_scale(self) -> f64 {
        if matches!(self, Self::GuardedPlayer) {
            0.5
        } else {
            1.0
        }
    }
}

/// [`Goal::Stroll`] 的 `probability` 取这个值时表示不避水的 `RandomStrollGoal`。
///
/// The [`Goal::Stroll`] `probability` that marks the plain `RandomStrollGoal`.
pub(super) const PLAIN_STROLL: f32 = -1.0;

/// 一个还没跑过的 `MeleeAttackGoal`。
///
/// A fresh `MeleeAttackGoal`.
pub(super) fn melee_attack(speed: f64) -> Goal {
    Goal::MeleeAttack {
        speed,
        ticks_until_next_attack: 0,
        ticks_until_path_recalc: 0,
        pathed_target: Vec3::default(),
        raise_arm_ticks: 0,
        last_can_use_check: 0,
        start_path: None,
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
            Self::Unavailable(_) | Self::RandomStand { .. } => &[],
            Self::MeleeAttack { .. } | Self::Bow(_) | Self::RangedAttack { .. } => {
                &[Flag::Move, Flag::Look]
            }
            Self::HurtByTarget { .. }
            | Self::NearestAttackableTarget { .. }
            | Self::LlamaHurtByTarget { .. } => &[Flag::Target],
            Self::RestrictSun => &[],
            Self::FleeSun { .. } | Self::TurtleEgg { .. } => &[Flag::Move],
            Self::BreakDoor => &[Flag::Move],
            Self::EatBlock { .. } => &[Flag::Move, Flag::Look, Flag::Jump],
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
                | Self::Bow(_)
                | Self::TurtleEgg { .. }
                | Self::RandomStand { .. }
                | Self::RangedAttack { .. }
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
            Self::Stroll { probability, .. } if *probability < 0.0 => "RandomStrollGoal",
            Self::Stroll { .. } => "WaterAvoidingRandomStrollGoal",
            Self::LookAtPlayer { .. } => "LookAtPlayerGoal",
            Self::RandomLookAround { .. } => "RandomLookAroundGoal",
            Self::Unavailable(name) => name,
            Self::MeleeAttack { speed, .. } if *speed == 1.2 => "MeleeAttackGoal",
            Self::MeleeAttack { speed, .. } if *speed == super::polar_bear::MELEE_SPEED => {
                "PolarBearMeleeAttackGoal"
            }
            Self::MeleeAttack { .. } => "ZombieAttackGoal",
            Self::Bow(_) => "RangedBowAttackGoal",
            Self::HurtByTarget { .. } => "HurtByTargetGoal",
            Self::NearestAttackableTarget { .. } => "NearestAttackableTargetGoal",
            Self::RestrictSun => "RestrictSunGoal",
            Self::FleeSun { .. } => "FleeSunGoal",
            Self::BreakDoor => "BreakDoorGoal",
            Self::TurtleEgg { .. } => "ZombieAttackTurtleEggGoal",
            Self::RandomStand { .. } => "RandomStandGoal",
            Self::RangedAttack { .. } => "RangedAttackGoal",
            Self::LlamaHurtByTarget { .. } => "LlamaHurtByTargetGoal",
            Self::EatBlock { .. } => "EatBlockGoal",
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
    /// 按注册顺序装好一组目标，标志位都没人占。
    ///
    /// A selector over goals in registration order with every flag free.
    pub(super) fn with_goals(goals: Vec<WrappedGoal>) -> Self {
        Self {
            goals,
            ..Self::default()
        }
    }

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
                    steering_item: false,
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

    /// AbstractSkeleton.registerGoals：按注册顺序而非优先级排序。
    ///
    /// AbstractSkeleton.registerGoals uses insertion order, not sorted priority.
    pub(crate) fn skeleton() -> Self {
        Self {
            goals: [
                (2, Goal::RestrictSun),
                (
                    3,
                    Goal::FleeSun {
                        speed: 1.0,
                        wanted: Vec3::default(),
                    },
                ),
                (3, Goal::Unavailable("AvoidEntityGoal")),
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
                        look_distance: 8.0,
                        probability: 0.02,
                        look_at: None,
                        look_time: 0,
                    },
                ),
                (
                    6,
                    Goal::RandomLookAround {
                        rel_x: 0.0,
                        rel_z: 0.0,
                        look_time: 0,
                    },
                ),
                (4, Self::skeleton_melee()),
            ]
            .into_iter()
            .map(|(priority, goal)| WrappedGoal {
                priority,
                goal,
                running: false,
            })
            .collect(),
            ..Self::default()
        }
    }

    pub(crate) fn rebase_clock(&mut self, delta: i64) {
        for goal in &mut self.goals {
            if let Goal::MeleeAttack {
                last_can_use_check, ..
            } = &mut goal.goal
            {
                *last_can_use_check = last_can_use_check.saturating_add(delta);
            }
        }
    }

    pub(crate) fn skeleton_targets() -> Self {
        let mut selector = Self::zombie_targets();
        selector.goals.remove(2);
        selector.goals[3].priority = 3;
        selector
    }

    fn skeleton_melee() -> Goal {
        Goal::MeleeAttack {
            speed: 1.2,
            ticks_until_next_attack: 0,
            ticks_until_path_recalc: 0,
            pathed_target: Vec3::default(),
            raise_arm_ticks: 0,
            last_can_use_check: 0,
            start_path: None,
        }
    }

    pub(crate) fn reassess_skeleton_weapon(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
    ) {
        let index = self.goals.len() - 1;
        let has_bow = super::skeleton::bow_slot(mob).is_some();
        if has_bow == matches!(self.goals[index].goal, Goal::Bow(_)) {
            return;
        }
        self.stop(index, me, mob, ctx);
        for holder in &mut self.locked {
            if *holder == Some(index) {
                *holder = None;
            }
        }
        self.goals[index].goal = if has_bow {
            Goal::Bow(super::skeleton::BowGoal::new(ctx.blocks().difficulty()))
        } else {
            Self::skeleton_melee()
        };
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

/// `EatBlockGoal.canUse`：先 `nextInt(adjustedTickDelay(幼年 50 / 成年 1000))`，再看脚下能吃的植物或下面的草方块。
///
/// `EatBlockGoal.canUse`: one `nextInt` gate (50 for babies, 1000 for adults, halved), then an
/// edible plant at the feet or a grass block below.
fn eat_block_can_use(me: &mut Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    let bound = adjusted_tick_delay(false, if mob.is_baby() { 50 } else { 1000 });
    if me.random.next_int_bounded(bound) != 0 {
        return false;
    }
    let pos = me.block_position();
    let view = ctx.blocks();
    view.block_state(pos)
        .is_tagged(mc_bamboo_data::tags::block_tags::EDIBLE_FOR_SHEEP)
        || view.block_state(pos.below()).is(blocks::GRASS_BLOCK)
}

/// `EatBlockGoal.tick`：倒计时到 `adjustedTickDelay(4)` 时吃。改方块交给命令，羊自己的 `ate` 当场做。
///
/// `EatBlockGoal.tick`: eat when the countdown hits `adjustedTickDelay(4)`. The block change is a
/// command; the sheep's own `ate` happens here.
fn eat_block_tick(eat_tick: &mut i32, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    *eat_tick = (*eat_tick - 1).max(0);
    if *eat_tick != adjusted_tick_delay(false, 4) {
        return;
    }
    let pos = me.block_position();
    let view = ctx.blocks();
    let griefing = view.block_game_rules().mob_griefing;
    let target = if view
        .block_state(pos)
        .is_tagged(mc_bamboo_data::tags::block_tags::EDIBLE_FOR_SHEEP)
    {
        Some((pos, false))
    } else if view.block_state(pos.below()).is(blocks::GRASS_BLOCK) {
        Some((pos.below(), true))
    } else {
        None
    };
    let Some((pos, grass_below)) = target else {
        return;
    };
    if griefing {
        ctx.emit(
            crate::entity::ctx::Command::EatBlock { pos, grass_below },
            me,
        );
    }
    super::sheep::ate(me, mob);
}

/// `Goal.reducedTickDelay`。
pub(super) const fn reduced_tick_delay(ticks: i32) -> i32 {
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
        Goal::Bow(_) => {
            super::skeleton::bow_slot(mob).is_some() && combat_target(mob, ctx).is_some()
        }
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
            *partner = free_partner(me, mob, ctx);
            partner.is_some()
        }
        Goal::Tempt {
            player,
            calm_down,
            steering_item,
            ..
        } => {
            if *calm_down > 0 {
                *calm_down -= 1;
                return false;
            }
            *player = nearest_tempting_player(me, mob, ctx, *steering_item);
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
            let found = if *probability < 0.0 {
                // `RandomStrollGoal.getPosition`：不看水，也不抽那个 `nextFloat`。
                // `RandomStrollGoal.getPosition`: no water check and no `nextFloat`.
                default_random_pos(me, mob, ctx.blocks(), 10, 7)
            } else if me.in_water {
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
        Goal::RandomStand { next_stand } => {
            super::equine::random_stand_can_use(me, mob, next_stand)
        }
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
            let Some(target) = combat_target(mob, ctx) else {
                return false;
            };
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
            let last = mob.last_hurt_by_mob;
            let stamp = mob.last_hurt_by_mob_timestamp;
            stamp != *timestamp
                && last.is_some_and(|uid| {
                    !matches!(
                        mob.mob_type,
                        super::MobType::Skeleton | super::MobType::PolarBear
                    ) || target_by_uid(mob, ctx, uid).is_some()
                })
        }
        // `HurtByTargetGoal.canUse`：时间戳变了，并且 `canAttack(lastHurtByMob, HURT_BY_TARGETING)`。
        // `HurtByTargetGoal.canUse`: a new timestamp and an attackable last attacker.
        Goal::LlamaHurtByTarget { timestamp, .. } => {
            mob.last_hurt_by_mob_timestamp != *timestamp
                && mob
                    .last_hurt_by_mob
                    .is_some_and(|uid| target_by_uid(mob, ctx, uid).is_some())
        }
        // `RangedAttackGoal.canUse`：`getTarget()` 不为空且活着，记进自己的字段。
        // `RangedAttackGoal.canUse`: a living `getTarget()`, remembered in the goal.
        Goal::RangedAttack { target, .. } => {
            let alive = mob.target.filter(|&uid| alive_target(ctx, uid).is_some());
            if alive.is_some() {
                *target = alive;
            }
            alive.is_some()
        }
        Goal::NearestAttackableTarget {
            kind,
            interval,
            found,
            ..
        } => {
            // `PolarBearAttackPlayersGoal.canUse`：幼崽在取数之前就返回。
            // `PolarBearAttackPlayersGoal.canUse`: a cub returns before any draw.
            if *kind == TargetType::GuardedPlayer && mob.is_baby() {
                return false;
            }
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
            // 找到了玩家，还要身边有幼崽才动手。
            // A player was found; a cub must be nearby as well.
            if *kind == TargetType::GuardedPlayer
                && found.is_some()
                && !super::polar_bear::cub_nearby(me, ctx)
            {
                return false;
            }
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
        Goal::EatBlock { .. } => eat_block_can_use(me, mob, ctx),
        Goal::BreakDoor => {
            mob.can_break_doors
                && ctx.blocks().block_game_rules().mob_griefing
                && ctx.blocks().difficulty() == 3
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
        Goal::Bow(_) => {
            super::skeleton::bow_slot(mob).is_some()
                && (combat_target(mob, ctx).is_some() || !mob.navigation.is_done())
        }
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
        Goal::Unavailable(_) | Goal::RandomStand { .. } => false,

        Goal::MeleeAttack { .. } if mob.mob_type == super::MobType::PolarBear => {
            // `followingTargetEvenIfNotSeen=true`：目标打得了、还在家的范围里就继续，不看导航。
            // `followingTargetEvenIfNotSeen=true`: continue while the target is attackable and
            // inside the home range, whatever the navigation says.
            combat_target(mob, ctx)
                .is_some_and(|target| within_home(mob, BlockPos::containing(target.position)))
        }
        Goal::MeleeAttack { .. } => {
            // 官方 `followingTargetEvenIfNotSeen=false`：目标活着且
            // `!navigation.isDone()` 才继续；路径走完就停。
            combat_target(mob, ctx).is_some() && !mob.navigation.is_done()
        }
        Goal::NearestAttackableTarget {
            unseen_ticks, kind, ..
        } if matches!(
            mob.mob_type,
            super::MobType::Skeleton | super::MobType::PolarBear
        ) =>
        {
            target_still_seen(me, mob, ctx, unseen_ticks, kind.follow_scale(), 60)
        }
        Goal::HurtByTarget { unseen_ticks, .. } if mob.mob_type == super::MobType::PolarBear => {
            // `HurtByTargetGoal.start` 把 `unseenMemoryTicks` 设成 300。`targetMob` 的回退
            // 没有建模：目标被清掉而原来的攻击者还打得了的情形在北极熊身上不会出现。
            // `HurtByTargetGoal.start` sets `unseenMemoryTicks` to 300. The `targetMob`
            // fallback is not modelled: a cleared target with a still attackable attacker
            // does not arise for polar bears.
            target_still_seen(me, mob, ctx, unseen_ticks, 1.0, 300)
        }
        Goal::RangedAttack { target, .. } => {
            let stored = *target;
            can_use(goal, me, mob, ctx)
                || (stored.is_some_and(|uid| alive_target(ctx, uid).is_some())
                    && !mob.navigation.is_done())
        }
        Goal::LlamaHurtByTarget { unseen_ticks, .. } => {
            if super::equine::take_did_spit(mob) {
                return false;
            }
            target_can_continue_seen(me, mob, ctx, unseen_ticks, reduced_tick_delay(300))
        }
        Goal::HurtByTarget { .. } | Goal::NearestAttackableTarget { .. } => {
            // `TargetGoal.canContinueToUse`：`getTarget()` 把打不了的目标（死了、创造、
            // 旁观、和平难度）读成 null，回退到的 `targetMob` 是同一个玩家，同样打不了；
            // 打得了还要在跟随距离内。`mustSee` 的视线记忆本期不建模（目标恒看得见）。
            combat_target(mob, ctx).is_some_and(|target| target_in_follow_range(me, mob, target))
        }
        Goal::RestrictSun => {
            ctx.blocks().is_bright_outside()
                && mob
                    .equipment
                    .get(mc_bamboo_protocol::packet::play::EquipmentSlot::Head)
                    .is_empty()
        }
        Goal::FleeSun { .. } => !mob.navigation.is_done(),
        Goal::BreakDoor => mob.door_break_time <= 240 && ctx.blocks().difficulty() == 3,
        Goal::EatBlock { eat_tick } => *eat_tick > 0,
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
        Goal::Bow(_) => mob.aggressive = true,
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
        Goal::RandomStand { .. } => super::equine::random_stand_start(me, mob),
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
        } => {
            *timestamp = mob.last_hurt_by_mob_timestamp;
            mob.target = mob.last_hurt_by_mob;
            *unseen_ticks = 0;
            // `PolarBearHurtByTargetGoal.start`：幼崽叫来成年北极熊，自己当场 `stop()`。
            // `PolarBearHurtByTargetGoal.start`: a cub alerts the adults and stops at once.
            if mob.mob_type == super::MobType::PolarBear && mob.is_baby() {
                super::polar_bear::alert_adults(me, mob, ctx);
                mob.target = None;
            }
        }
        // `HurtByTargetGoal.start` 再 `TargetGoal.start`：`unseenTicks` 清零。
        // `HurtByTargetGoal.start`, then `TargetGoal.start` clears `unseenTicks`.
        Goal::LlamaHurtByTarget {
            timestamp,
            unseen_ticks,
        } => {
            *timestamp = mob.last_hurt_by_mob_timestamp;
            mob.target = mob.last_hurt_by_mob;
            *unseen_ticks = 0;
        }
        Goal::RangedAttack { .. } => {}
        Goal::NearestAttackableTarget {
            found,
            unseen_ticks,
            ..
        } => {
            mob.target = *found;
            *unseen_ticks = 0;
        }
        Goal::RestrictSun => mob.navigation.avoid_sun = true,
        Goal::FleeSun { speed, wanted } => {
            let nav = living::nav_mob(me, mob, ctx.blocks());
            mob.navigation.move_to(ctx.blocks(), &nav, *wanted, *speed);
        }
        Goal::BreakDoor => {
            mob.door_break_time = 0;
            mob.door_break_progress = -1;
            mob.door_break_pos = None;
        }
        Goal::EatBlock { eat_tick } => {
            // `start`：倒计时、实体事件 10（低头动画）、停下导航。
            // `start`: the countdown, entity event 10 (head-down animation), stop navigation.
            *eat_tick = adjusted_tick_delay(false, 40);
            mob.events.push(super::MobEvent::EntityEvent { event: 10 });
            mob.navigation.stop();
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
        Goal::Bow(bow) => bow.stop(mob),
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
        | Goal::RandomStand { .. } => {}
        Goal::RestrictSun => mob.navigation.avoid_sun = false,
        // `TargetGoal.stop`：`setTarget(null)`，`targetMob = null`。
        Goal::HurtByTarget { .. } | Goal::LlamaHurtByTarget { .. } => mob.target = None,
        Goal::RangedAttack {
            target,
            attack_time,
            see_time,
        } => {
            *target = None;
            *see_time = 0;
            *attack_time = -1;
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
            // `PolarBearMeleeAttackGoal.stop`：先放下身子。
            // `PolarBearMeleeAttackGoal.stop`: drop down first.
            mob.polar_bear.standing = false;
            mob.aggressive = false;
            mob.navigation.stop();
        }
        Goal::FleeSun { .. } => {}
        Goal::BreakDoor => {
            mob.door_break_time = 0;
            mob.door_break_progress = -1;
        }
        Goal::EatBlock { eat_tick } => *eat_tick = 0,
        Goal::TurtleEgg { .. } => {
            // `RemoveBlockGoal.stop`：落地前的摔落距离按 1 格算。
            me.fall_distance = 1.0;
        }
    }
}

fn tick_goal(goal: &mut Goal, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    match goal {
        Goal::EatBlock { eat_tick } => eat_block_tick(eat_tick, me, mob, ctx),
        Goal::Bow(bow) => {
            if let Some(target) = combat_target(mob, ctx) {
                bow.tick(me, mob, ctx, target);
            }
        }
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
        Goal::Unavailable(_)
        | Goal::HurtByTarget { .. }
        | Goal::LlamaHurtByTarget { .. }
        | Goal::RestrictSun
        | Goal::RandomStand { .. } => {}
        Goal::RangedAttack {
            target,
            attack_time,
            see_time,
        } => {
            // 官方隔刻也照样 tick，不再查目标活不活。
            // Vanilla ticks on off ticks too without re-checking that the target lives.
            if let Some((target, _)) = target.and_then(|uid| target_view(ctx, uid)) {
                super::equine::ranged_attack_tick(me, mob, ctx, target, attack_time, see_time);
            }
        }
        Goal::NearestAttackableTarget { .. } if mob.mob_type == super::MobType::PolarBear => {
            // 北极熊的 `unseenTicks` 只在 `canContinueToUse` 里记（见 `target_still_seen`）。
            // Polar bears count `unseenTicks` in `canContinueToUse` only (`target_still_seen`).
        }
        Goal::NearestAttackableTarget { unseen_ticks, .. } => {
            *unseen_ticks += 1;
            // 官方 `TargetGoal.canContinueToUse` 在 `mustSee` 时按视线刷新
            // `unseenTicks`；本期的玩家目标恒可攻击，不改随机源。
        }
        Goal::MeleeAttack {
            speed,
            ticks_until_next_attack,
            ticks_until_path_recalc,
            pathed_target,
            raise_arm_ticks,
            ..
        } => {
            let Some(target) = combat_target(mob, ctx) else {
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
            // 官方 `MeleeAttackGoal.tick`：三个短路串行，
            // `nextFloat` 只在「pathed 非零且目标离它 < 1」时被求值。
            if *ticks_until_path_recalc == 0
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
            if mob.mob_type == super::MobType::PolarBear {
                super::polar_bear::check_and_perform_attack(
                    ticks_until_next_attack,
                    me,
                    mob,
                    ctx,
                    target,
                );
            } else if *ticks_until_next_attack == 0
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
            if matches!(mob.mob_type, super::MobType::Zombie | super::MobType::Husk) {
                *raise_arm_ticks += 1;
                mob.aggressive = *raise_arm_ticks >= 5
                    && *ticks_until_next_attack < adjusted_tick_delay(true, 20) / 2;
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
    // 成年北极熊只怕环境伤害（`PANIC_ENVIRONMENTAL_CAUSES`），幼崽和别的动物一样。
    // Adult polar bears panic from environmental damage only; cubs panic like any animal.
    let environmental = mob.mob_type == super::MobType::PolarBear && !mob.is_baby();
    living::last_damage_source(mob, ctx.blocks().game_time()).is_some_and(|source| {
        if environmental {
            source.kind.panic_environmental_causes()
        } else {
            source.kind.panic_causes()
        }
    })
}

/// `TargetGoal.canContinueToUse` 带 `mustSee` 的版本：目标打得了、在跟随距离里；看不见的
/// 满刻数超过 `reducedTickDelay(memory)` 就放弃。
///
/// `TargetGoal.canContinueToUse` with `mustSee`: the target is attackable and within the
/// follow distance; it is dropped once unseen for more than `reducedTickDelay(memory)`
/// full ticks.
fn target_still_seen(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    unseen_ticks: &mut i32,
    follow_scale: f64,
    memory: i32,
) -> bool {
    let Some(target) = combat_target(mob, ctx) else {
        return false;
    };
    let range = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE)
        * follow_scale;
    if me.position.distance_to_sqr(target.position) > range * range {
        return false;
    }
    if living::has_line_of_sight(me, mob, ctx.blocks(), target.uid, target.eye_position()) {
        *unseen_ticks = 0;
    } else {
        *unseen_ticks += 1;
    }
    *unseen_ticks <= reduced_tick_delay(memory)
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
fn nearest_tempting_player(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    steering_item: bool,
) -> Option<EntityUid> {
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
        let follows = [player.main_hand, player.off_hand].into_iter().any(|item| {
            if steering_item {
                item == mc_bamboo_data::items::CARROT_ON_A_STICK
            } else if mob.mob_type.is_equine() {
                crate::state::item_is_tagged(item, super::equine::tempt_tag(mob.mob_type))
            } else {
                super::interact::is_food(mob.mob_type, item)
            }
        });
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
/// 马科按 `canMate`：双方都 `canParent`，马和驴可以互配。
///
/// The nearest in-love, non-panicking partner within 8 blocks; equines use
/// `canMate`, where both must `canParent` and horses pair with donkeys.
fn free_partner(me: &Entity, mob: &Mob, ctx: &dyn EntityCtx) -> Option<EntityUid> {
    if mob.mob_type.is_equine() && !super::equine::can_parent(mob) {
        return None;
    }
    let area = me.bounding_box().inflate(8.0, 8.0, 8.0);
    let mut best = f64::MAX;
    let mut found = None;
    ctx.for_each_other_in(area, &mut |other| {
        let KindPublic::Mob(public) = &other.kind else {
            return;
        };
        let compatible = if mob.mob_type.is_equine() {
            public.can_parent_equine
                && super::equine::offspring_kind(mob.mob_type, public.mob_type).is_some()
        } else {
            public.mob_type == mob.mob_type
        };
        if !compatible
            || other.uid == me.id
            || public.in_love <= 0
            || public.panicking
            || !public.is_alive()
        {
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
pub(super) fn walk_target_value(mob: &Mob, view: &dyn BlockView, pos: BlockPos) -> f32 {
    if mob.mob_type.is_monster() {
        return monster_walk_target_value(view, pos);
    }
    if mob.mob_type == super::MobType::Mooshroom {
        return super::mooshroom::walk_target_value(view, pos);
    }
    animal_walk_target_value(view, pos)
}

/// `Animal.getWalkTargetValue`：脚下是草方块给 10，否则按亮度算路径代价。
///
/// `Animal.getWalkTargetValue`: 10 on a grass block, otherwise the light-based path cost.
pub(super) fn animal_walk_target_value(view: &dyn BlockView, pos: BlockPos) -> f32 {
    if view.block_state(pos.below()).is(blocks::GRASS_BLOCK) {
        return 10.0;
    }
    light_level_cost(view, pos)
}

/// `LevelReader.getPathfindingCostFromLightLevels`：按亮度算的路径代价。
///
/// `LevelReader.getPathfindingCostFromLightLevels`: the light-based path cost.
pub(super) fn light_level_cost(view: &dyn BlockView, pos: BlockPos) -> f32 {
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

/// 当前岛上可攻击的生物快照；跨岛目标读作缺失。
///
/// A local attack target snapshot. Unavailable cross-island targets are absent.
#[derive(Clone, Copy)]
pub(crate) struct CombatTarget {
    pub(crate) uid: EntityUid,
    pub(crate) position: Vec3,
    eye: f32,
    pub(crate) height: f64,
    /// `getBbWidth()`。
    pub(crate) width: f32,
    hitbox: Aabb,
}

impl CombatTarget {
    pub(crate) fn player(view: PlayerView, skeleton: bool) -> Self {
        let feet = view.position;
        let half = if skeleton {
            f64::from(0.6_f32) / 2.0
        } else {
            0.3
        };
        let height = if skeleton {
            f64::from(if view.sneaking { 1.5_f32 } else { 1.8_f32 })
        } else {
            1.8
        };
        Self {
            uid: view.uid,
            position: feet,
            eye: view.eye_height,
            height,
            width: 0.6,
            hitbox: Aabb::new(
                feet.x - half,
                feet.y,
                feet.z - half,
                feet.x + half,
                feet.y + height,
                feet.z + half,
            ),
        }
    }

    pub(crate) fn eye_y(&self) -> f64 {
        self.position.y + f64::from(self.eye)
    }

    pub(crate) fn eye_position(&self) -> Vec3 {
        Vec3::new(self.position.x, self.eye_y(), self.position.z)
    }
}

pub(super) fn combat_target(mob: &Mob, ctx: &dyn EntityCtx) -> Option<CombatTarget> {
    target_by_uid(mob, ctx, mob.target?)
}

fn target_in_follow_range(me: &Entity, mob: &Mob, target: CombatTarget) -> bool {
    let range = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE);
    me.position.distance_to_sqr(target.position) <= range * range
}

fn target_by_uid(mob: &Mob, ctx: &dyn EntityCtx, uid: EntityUid) -> Option<CombatTarget> {
    if ctx.blocks().difficulty() == 0 {
        return None;
    }
    // 骷髅、北极熊和羊驼的目标也可以是生物，玩家碰撞箱按潜行算准。
    // Skeletons, polar bears and llamas may target mobs too and use the exact player box.
    let mob_targets = matches!(
        mob.mob_type,
        super::MobType::Skeleton | super::MobType::PolarBear | super::MobType::Llama
    );
    if let Some(player) = player_by_uid(ctx.blocks(), uid) {
        return (player.alive && !player.spectator && !player.creative)
            .then(|| CombatTarget::player(player, mob_targets));
    }
    if !mob_targets {
        return None;
    }
    let (target, alive) = mob_target_view(ctx, uid)?;
    alive.then_some(target)
}

/// 别的生物当目标时的样子，和它活没活着（没被移除且血量大于 0）。
///
/// Another mob seen as a target, and whether it is alive (not removed, health above 0).
fn mob_target_view(ctx: &dyn EntityCtx, uid: EntityUid) -> Option<(CombatTarget, bool)> {
    let other = ctx.other(uid)?;
    let KindPublic::Mob(target) = &other.kind else {
        return None;
    };
    let (_, _, eye) = target.mob_type.dimensions(target.baby);
    let view = CombatTarget {
        uid,
        position: other.position,
        eye,
        height: f64::from(target.dimensions().1),
        width: target.dimensions().0,
        hitbox: other.bounding_box(),
    };
    Some((view, !other.removed && target.is_alive()))
}

/// `RangedAttackGoal` 的目标：不看难度、创造模式，只要找得到；第二项是 `isAlive()`。
///
/// The `RangedAttackGoal` target: no difficulty or creative filter, just
/// found; the flag is `isAlive()`.
pub(super) fn target_view(ctx: &dyn EntityCtx, uid: EntityUid) -> Option<(CombatTarget, bool)> {
    if let Some(player) = player_by_uid(ctx.blocks(), uid) {
        return Some((CombatTarget::player(player, true), player.alive));
    }
    mob_target_view(ctx, uid)
}

/// `TargetGoal.canContinueToUse`，`mustSee` 为真：打得了、在跟随距离内、看不见的刻数没超过
/// `limit`。`HurtByTargetGoal.start` 把 `unseenMemoryTicks` 设成 300，所以 `limit` 是它折半。
///
/// `TargetGoal.canContinueToUse` with `mustSee`: attackable, within follow
/// range, and unseen for no more than `limit` ticks (half of the 300 that
/// `HurtByTargetGoal.start` sets).
fn target_can_continue_seen(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    unseen_ticks: &mut i32,
    limit: i32,
) -> bool {
    let Some(target) = combat_target(mob, ctx) else {
        return false;
    };
    if !target_in_follow_range(me, mob, target) {
        return false;
    }
    if living::has_line_of_sight(me, mob, ctx.blocks(), target.uid, target.eye_position()) {
        *unseen_ticks = 0;
    } else {
        *unseen_ticks += 1;
        if *unseen_ticks > limit {
            return false;
        }
    }
    true
}

fn alive_target(ctx: &dyn EntityCtx, uid: EntityUid) -> Option<CombatTarget> {
    target_view(ctx, uid).and_then(|(target, alive)| alive.then_some(target))
}

/// `NearestAttackableTargetGoal.findTarget` 的玩家分支：按 `FOLLOW_RANGE`
/// 找最近、看得见、非创造/旁观且在战斗条件下能被打的目标。
fn nearest_attackable(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    kind: TargetType,
) -> Option<EntityUid> {
    if !kind.available() || ctx.blocks().difficulty() == 0 {
        return None;
    }
    let range = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE)
        * kind.follow_scale();
    let view = ctx.blocks();
    let difficulty = view.difficulty();
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
        // 选择器（`isAngryAt`）在 `TargetingConditions.test` 里先于距离和视线。
        // The selector (`isAngryAt`) runs before range and sight in `TargetingConditions.test`.
        if kind == TargetType::AngerPlayer
            && !super::neutral::is_angry_at_player(mob, &player, difficulty)
        {
            continue;
        }
        // `Player.canBeSeenAsEnemy`：创造模式的玩家不会被选中。僵尸和骷髅的旧行为不动。
        // `Player.canBeSeenAsEnemy`: creative players are never picked. Zombies and
        // skeletons keep their previous behaviour.
        if kind != TargetType::Player && player.creative {
            continue;
        }
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
pub(super) fn within_melee_range(me: &Entity, _mob: &Mob, target: CombatTarget) -> bool {
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
    ours.intersects(target.hitbox)
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
