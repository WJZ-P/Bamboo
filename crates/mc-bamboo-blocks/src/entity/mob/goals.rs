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
    state::SupportType,
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
    RabbitPowderSnow,
    RabbitRaid(super::rabbit::RaidGarden),
    Avoid(super::avoid::AvoidEntity),
    /// 狐狸自己的目标。
    ///
    /// The fox's own goals.
    Fox(super::fox_goals::FoxGoal),
    /// 可驯服动物共用的目标（坐下、跟随主人、保护主人）。
    ///
    /// The shared tamable-animal goals.
    Tame(super::tame::TameGoal),
    /// 狼自己的目标。
    ///
    /// The wolf's own goals.
    Wolf(super::wolf_goals::WolfGoal),
    /// 猫自己的目标。
    ///
    /// The cat's own goals.
    Cat(super::cat::CatGoal),
    /// 鹦鹉自己的目标。
    ///
    /// The parrot's own goals.
    Parrot(super::parrot::ParrotGoal),
    /// `LeapAtTargetGoal`：离目标 2 到 4 格、站在地上时扑过去。
    ///
    /// `LeapAtTargetGoal`: leap at a target 2 to 4 blocks away.
    LeapAtTarget {
        yd: f32,
    },
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
        /// `TemptGoal.canScare` 的状态：豹猫 / 猫对未信任或未驯服的玩家受惊。
        ///
        /// State for `TemptGoal.canScare`: ocelots and cats are scared by sudden movement.
        scare: Option<TemptScare>,
    },
    /// `OcelotAttackGoal`：豹猫和猫按距离切换潜行 / 冲刺并每 20 刻扑击。
    ///
    /// `OcelotAttackGoal`: ocelots and cats switch crouch / sprint by distance and strike
    /// every 20 ticks.
    OcelotAttack {
        target: Option<EntityUid>,
        attack_time: i32,
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
        /// `followingTargetEvenIfNotSeen`：目标看不见也继续追，`canContinueToUse`
        /// 也不再看路径是否走完。
        ///
        /// `followingTargetEvenIfNotSeen`: keep chasing an unseen target and ignore a
        /// finished path.
        track_target: bool,
        /// 官方的类名，对拍用。
        ///
        /// The vanilla class name, for the oracle.
        class: &'static str,
    },
    /// `SwellGoal`：苦力怕靠近目标时把膨胀方向设成 1，看不见或离远了设回 -1。
    ///
    /// `SwellGoal`: sets the creeper's swell direction to 1 near its target and back to
    /// -1 when the target is far or out of sight.
    Swell {
        target: Option<EntityUid>,
    },
    EndermanGaze(super::enderman::GazeGoal),
    EndermanFreeze,
    EndermanBlock {
        place: bool,
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
        /// `Spider.SpiderTargetGoal` 的亮处闸门。
        ///
        /// The bright-light gate of `Spider.SpiderTargetGoal`.
        light_gate: bool,
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
    /// `RunAroundLikeCrazyGoal`：没驯服的马科被人骑着时朝 `wanted` 乱跑，偶尔抽签驯服。
    ///
    /// `RunAroundLikeCrazyGoal`: an untamed equine with a rider bolts towards
    /// `wanted` and now and then rolls to be tamed.
    RunAroundLikeCrazy {
        speed: f64,
        wanted: Vec3,
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
    /// `AbstractCubeMob.CubeMobFloatGoal`：在液体里按 0.8 的概率跳，并要求以 1.2 倍速移动。
    ///
    /// `AbstractCubeMob.CubeMobFloatGoal`: jump with probability 0.8 while in liquid and ask
    /// for movement at 1.2 speed.
    CubeFloat,
    /// `AbstractCubeMob.CubeMobAttackGoal`：看向目标并朝它跳，300 刻（折半 150）后厌倦。
    ///
    /// `AbstractCubeMob.CubeMobAttackGoal`: face the target and hop at it, growing tired
    /// after 300 ticks (halved to 150).
    CubeAttack {
        grow_tired_timer: i32,
    },
    /// `AbstractCubeMob.CubeMobRandomDirectionGoal`：没有目标时隔一阵随机换个方向。
    ///
    /// `AbstractCubeMob.CubeMobRandomDirectionGoal`: pick a random heading now and then
    /// while there is no target.
    CubeRandomDirection {
        chosen_degrees: f32,
        next_randomize_time: i32,
    },
    /// `AbstractCubeMob.CubeMobKeepOnJumpingGoal`：一直要求移动。
    ///
    /// `AbstractCubeMob.CubeMobKeepOnJumpingGoal`: keep asking for movement.
    CubeKeepJumping,
    /// `Drowned.DrownedGoToWaterGoal`：天亮时上岸的溺尸往回找水。
    ///
    /// `Drowned.DrownedGoToWaterGoal`: a drowned on land heads back to water in daylight.
    DrownedGoToWater {
        speed: f64,
        wanted: Vec3,
    },
    /// `Drowned.DrownedSwimUpGoal`：夜里在水下时朝海平面游。
    ///
    /// `Drowned.DrownedSwimUpGoal`: swim up towards sea level at night.
    DrownedSwimUp {
        speed: f64,
        stuck: bool,
    },
    /// `Drowned.DrownedGoToBeachGoal`（`MoveToBlockGoal`）：夜里泡在近海面水里时找岸边。
    ///
    /// `Drowned.DrownedGoToBeachGoal` (`MoveToBlockGoal`): at night, a drowned in water near
    /// the surface looks for a beach.
    DrownedGoToBeach {
        speed: f64,
        next_start_tick: i32,
        try_ticks: i32,
        max_stay_ticks: i32,
        block_pos: Option<BlockPos>,
        reached_target: bool,
    },
    /// `RandomStrollGoal`（不避水）：`DefaultRandomPos.getPos(mob, 10, 7)`。
    ///
    /// `RandomStrollGoal` (not water avoiding): `DefaultRandomPos.getPos(mob, 10, 7)`.
    RandomStroll {
        speed: f64,
        interval: i32,
        wanted: Vec3,
        force_trigger: bool,
    },
    /// `NearestHealableRaiderTargetGoal`：Bamboo 没有袭击，但官方每刻照样抽一个
    /// `nextBoolean`，这里保住这次取数。
    ///
    /// `NearestHealableRaiderTargetGoal`: there are no raids, but vanilla still draws one
    /// `nextBoolean` each check, so the draw is kept.
    WitchHealRaiders {
        cooldown: i32,
    },
    /// `RangedAttackGoal`：女巫每 60 刻投一瓶喷溅药水，射程 10 格。
    ///
    /// `RangedAttackGoal`: the witch throws a splash potion every 60 ticks within 10 blocks.
    WitchRangedAttack {
        target: Option<EntityUid>,
        attack_time: i32,
        see_time: i32,
    },
    /// `PandaSitGoal`：拿着能吃的东西时坐下。
    ///
    /// `PandaSitGoal`: sit down while holding something edible.
    PandaSit,
    /// `PandaLieOnBackGoal`：懒惰熊猫仰面躺一会儿。
    ///
    /// `PandaLieOnBackGoal`: a lazy panda lies on its back for a while.
    PandaLieOnBack,
    /// `PandaSneezeGoal`：幼崽偶尔打个喷嚏。
    ///
    /// `PandaSneezeGoal`: a cub sneezes now and then.
    PandaSneeze,
    /// `PandaRollGoal`：幼崽或顽皮熊猫就地打滚。
    ///
    /// `PandaRollGoal`: a cub or a playful panda rolls on the spot.
    PandaRoll,
    /// `AvoidEntityGoal`：担心基因的熊猫躲开玩家或怪物。
    ///
    /// `AvoidEntityGoal`: a worried panda runs from players or monsters.
    AvoidEntity {
        target: AvoidTarget,
        max_dist: f32,
        walk_speed: f64,
        sprint_speed: f64,
        avoid: Option<EntityUid>,
        path: Option<super::path::Path>,
    },
    /// `Squid.SquidRandomMovementGoal`：只改鱿鱼的移动向量，不占标志位。
    ///
    /// `Squid.SquidRandomMovementGoal`: writes the squid movement vector, no flags.
    SquidRandomMovement,
    /// `Squid.SquidFleeGoal`：在水里远离最近打它的生物（每刻目标）。
    ///
    /// `Squid.SquidFleeGoal`: swim away from the last attacker (ticks every tick).
    SquidFlee {
        flee_ticks: i32,
    },
    /// `RandomSwimmingGoal`（鱼的 `FishSwimGoal`）：随机挑一个水里可寻路的点游过去。
    ///
    /// `RandomSwimmingGoal` (the fish `FishSwimGoal`): a random path-findable water spot.
    Swim {
        speed: f64,
        interval: i32,
        wanted: Vec3,
    },
    /// `Pufferfish.PufferfishPuffGoal`：附近有生物就鼓起来。
    ///
    /// `Pufferfish.PufferfishPuffGoal`: puff up when something is nearby.
    PufferfishPuff,
    /// `FollowFlockLeaderGoal`：成群的那几种鱼跟着领头的游。
    ///
    /// `FollowFlockLeaderGoal`: schooling fish follow a leader.
    FollowFlock {
        next_start: i32,
        time_to_recalc: i32,
    },
    /// `Turtle.TurtleLayEggGoal`：回到离家 9 格内，在沙子上产蛋。
    ///
    /// `Turtle.TurtleLayEggGoal`: lay an egg on sand within nine blocks of home.
    TurtleLayEgg {
        speed: f64,
    },
    /// `Turtle.TurtleGoHomeGoal`：往家方块走。
    ///
    /// `Turtle.TurtleGoHomeGoal`: swim or walk back to the home position.
    TurtleGoHome {
        speed: f64,
    },
    /// `Turtle.TurtleGoToWaterGoal`：在陆地上时往 24 格内的水走（`MoveToBlockGoal`）。
    ///
    /// `Turtle.TurtleGoToWaterGoal`: head for water within 24 blocks while on land.
    TurtleGoToWater {
        speed: f64,
    },
    /// `Turtle.TurtleTravelGoal`：在水里朝 512 格内的随机远点一段段游。
    ///
    /// `Turtle.TurtleTravelGoal`: swim leg by leg towards a random far point in water.
    TurtleTravel {
        speed: f64,
        stuck: bool,
    },
}

/// `TemptGoal.canScare` 记录的玩家上次位置与朝向。
///
/// The last player position and rotation remembered by `TemptGoal.canScare`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TemptScare {
    pub px: f64,
    pub py: f64,
    pub pz: f64,
    pub p_rot_x: f64,
    pub p_rot_y: f64,
}

/// [`Goal::AvoidEntity`] 躲的是哪一类。
///
/// What a [`Goal::AvoidEntity`] runs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvoidTarget {
    Player,
    Monster,
}

/// `NearestAttackableTargetGoal` 的目标类型。缺的实体在 Bamboo 里跳过，
/// 但注册项、优先级和 `Target` 标志位仍和官方一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetType {
    Player,
    Villager,
    IronGolem,
    Turtle,
    /// `Drowned` 的美西螈目标；Bamboo 没有这个实体，只占着顺序。
    ///
    /// The drowned's axolotl target; absent in Bamboo, kept for ordering.
    Axolotl,
    /// 狐狸（北极熊的猎物）。
    ///
    /// Foxes (polar bear prey).
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
    /// 狐狸的 `landTargetGoal`：鸡和兔子。
    ///
    /// The fox `landTargetGoal`: chickens and rabbits.
    FoxPrey,
    /// 狐狸的 `fishTargetGoal`：成群的鱼。
    ///
    /// The fox `fishTargetGoal`: schooling fish.
    Fish,
    /// 羊驼的 `LlamaAttackWolfGoal`：没驯服的狼。
    ///
    /// The llama's `LlamaAttackWolfGoal`: untamed wolves.
    Wolf,
    /// 末影人的 `NearestAttackableTargetGoal<Endermite>`；末影螨还没实现。
    ///
    /// The enderman's `NearestAttackableTargetGoal<Endermite>`; endermites are not modelled.
    Endermite,
    /// 豹猫的 `NearestAttackableTargetGoal<Chicken>`。
    ///
    /// The ocelot's `NearestAttackableTargetGoal<Chicken>`.
    Chicken,
    /// 豹猫 / 猫的 `NearestAttackableTargetGoal<Turtle, BABY_ON_LAND_SELECTOR>`。
    ///
    /// The ocelot / cat `NearestAttackableTargetGoal<Turtle, BABY_ON_LAND_SELECTOR>`.
    BabyTurtleOnLand,
}

impl TargetType {
    // 目标类型一律在 `canUse` 里显式穷举；不要加回 `available()` 兜底，
    // 否则北极熊的 Fox 目标会落进玩家搜索，愤怒时间与世界随机源从第 1 刻起分叉。
    // Every target kind is matched explicitly in `canUse`; do not reintroduce an
    // `available()` fallback, or the polar bear's Fox target falls into the player
    // search and the anger time and world random source diverge from tick 1.
    /// `TargetGoal.getFollowDistance` 相对 `FOLLOW_RANGE` 的倍数。
    ///
    /// The `TargetGoal.getFollowDistance` factor on `FOLLOW_RANGE`.
    const fn follow_scale(self) -> f64 {
        match self {
            Self::GuardedPlayer => 0.5,
            Self::Wolf => 0.25,
            _ => 1.0,
        }
    }
}

/// [`Goal::Stroll`] 的 `probability` 取这个值时表示不避水的 `RandomStrollGoal`。
///
/// The [`Goal::Stroll`] `probability` that marks the plain `RandomStrollGoal`.
pub(super) const PLAIN_STROLL: f32 = -1.0;

impl Goal {
    /// `MeleeAttackGoal(mob, speed, false)`；`raise_arms` 为真时是 `ZombieAttackGoal`。
    ///
    /// `MeleeAttackGoal(mob, speed, false)`, or `ZombieAttackGoal` with `raise_arms`.
    pub(super) fn melee(speed: f64, raise_arms: bool) -> Self {
        let class = if raise_arms {
            "ZombieAttackGoal"
        } else {
            "MeleeAttackGoal"
        };
        melee_attack(speed, false, class)
    }

    /// `Spider.SpiderAttackGoal`：`MeleeAttackGoal(mob, 1.0, true)` 加亮处放弃目标的检查。
    /// 亮处闸门在 `can_continue` 里按 `class == "SpiderAttackGoal"` 触发。
    ///
    /// `Spider.SpiderAttackGoal`: `MeleeAttackGoal(mob, 1.0, true)` plus the bright-light
    /// check, triggered in `can_continue` by `class == "SpiderAttackGoal"`.
    pub(super) fn spider_melee(speed: f64) -> Self {
        melee_attack(speed, true, "SpiderAttackGoal")
    }

    /// `Spider.SpiderTargetGoal`：`NearestAttackableTargetGoal(mob, type, true)`，
    /// 亮处直接不启动。
    ///
    /// `Spider.SpiderTargetGoal`: `NearestAttackableTargetGoal(mob, type, true)` that
    /// refuses to start in bright light.
    pub(super) fn spider_target(kind: TargetType) -> Self {
        Self::NearestAttackableTarget {
            kind,
            interval: reduced_tick_delay(10),
            unseen_ticks: 0,
            found: None,
            light_gate: true,
        }
    }

    /// `NearestAttackableTargetGoal(mob, type, true)`：缺省间隔 `reducedTickDelay(10)`。
    ///
    /// `NearestAttackableTargetGoal(mob, type, true)` with the default interval.
    pub(super) fn nearest_target(kind: TargetType) -> Self {
        Self::NearestAttackableTarget {
            kind,
            interval: reduced_tick_delay(10),
            unseen_ticks: 0,
            found: None,
            light_gate: false,
        }
    }

    /// 构造函数里 `setFlags` 设的标志。
    pub const fn flags(&self) -> &'static [Flag] {
        match self {
            Self::Float | Self::RabbitPowderSnow => &[Flag::Jump],
            Self::EndermanGaze(_) => &[Flag::Target],
            Self::EndermanFreeze => &[Flag::Jump, Flag::Move],
            Self::EndermanBlock { .. } => &[],
            Self::RabbitRaid(_) => &[Flag::Move, Flag::Jump],
            Self::Panic { .. }
            | Self::Stroll { .. }
            | Self::Swell { .. }
            | Self::Avoid(_)
            | Self::Swim { .. } => &[Flag::Move],
            Self::Fox(goal) => goal.flags(),
            Self::Tame(goal) => goal.flags(),
            Self::Wolf(goal) => goal.flags(),
            Self::Cat(goal) => goal.flags(),
            Self::Parrot(goal) => goal.flags(),
            Self::Breed { .. }
            | Self::Tempt { .. }
            | Self::OcelotAttack { .. }
            | Self::RandomLookAround { .. } => &[Flag::Move, Flag::Look],
            Self::FollowParent { .. } => &[],
            Self::LookAtPlayer { .. } => &[Flag::Look],
            Self::Unavailable(_) | Self::RandomStand { .. } => &[],
            Self::RunAroundLikeCrazy { .. } => &[Flag::Move],
            Self::MeleeAttack { .. } | Self::Bow(_) | Self::RangedAttack { .. } => {
                &[Flag::Move, Flag::Look]
            }
            Self::HurtByTarget { .. }
            | Self::NearestAttackableTarget { .. }
            | Self::LlamaHurtByTarget { .. } => &[Flag::Target],
            Self::LeapAtTarget { .. } => &[Flag::Jump, Flag::Move],
            Self::RestrictSun => &[],
            Self::FleeSun { .. } | Self::TurtleEgg { .. } => &[Flag::Move],
            // `MoveToBlockGoal` 锁 MOVE + JUMP；`TurtleGoHomeGoal` 没设标志位。
            // `MoveToBlockGoal` locks MOVE and JUMP; `TurtleGoHomeGoal` sets none.
            Self::TurtleLayEgg { .. } | Self::TurtleGoToWater { .. } => &[Flag::Move, Flag::Jump],
            Self::TurtleGoHome { .. } | Self::TurtleTravel { .. } => &[],
            Self::BreakDoor => &[Flag::Move],
            Self::EatBlock { .. } => &[Flag::Move, Flag::Look, Flag::Jump],
            Self::CubeFloat
            | Self::CubeAttack { .. }
            | Self::CubeRandomDirection { .. }
            | Self::CubeKeepJumping => super::slime::flags(self),
            Self::DrownedGoToWater { .. } | Self::RandomStroll { .. } => &[Flag::Move],
            // `MoveToBlockGoal` 同时锁 MOVE 和 JUMP。
            //
            // `MoveToBlockGoal` locks MOVE and JUMP.
            Self::DrownedGoToBeach { .. } => &[Flag::Move, Flag::Jump],
            // `DrownedSwimUpGoal` 没有 `setFlags`：不锁任何标志。
            // `DrownedSwimUpGoal` never calls `setFlags`, so it locks nothing.
            Self::DrownedSwimUp { .. } => &[],
            Self::WitchHealRaiders { .. } => &[Flag::Target],
            Self::WitchRangedAttack { .. } => &[Flag::Move, Flag::Look],
            Self::PandaSit | Self::AvoidEntity { .. } => &[Flag::Move],
            Self::PandaRoll => &[Flag::Move, Flag::Look, Flag::Jump],
            Self::PandaLieOnBack | Self::PandaSneeze => &[],
            Self::SquidRandomMovement
            | Self::SquidFlee { .. }
            | Self::PufferfishPuff
            | Self::FollowFlock { .. } => &[],
        }
    }

    /// `requiresUpdateEveryTick`。
    pub const fn every_tick(&self) -> bool {
        if let Self::Fox(goal) = self {
            return goal.every_tick();
        }
        if let Self::Cat(goal) = self {
            return goal.every_tick();
        }
        if let Self::Parrot(goal) = self {
            return goal.every_tick();
        }
        // 字节码逐个核过 `Goal.requiresUpdateEveryTick`：
        // Float/RandomLookAround/MeleeAttack/MoveToBlock/DoorInteract 为真，
        // LookAtPlayer/RandomStroll/LookAround 的其它目标为假。
        matches!(
            self,
            Self::Float
                | Self::RabbitPowderSnow
                | Self::RabbitRaid(_)
                | Self::RandomLookAround { .. }
                | Self::MeleeAttack { .. }
                | Self::OcelotAttack { .. }
                | Self::Bow(_)
                | Self::TurtleEgg { .. }
                | Self::CubeFloat
                | Self::CubeAttack { .. }
                | Self::WitchRangedAttack { .. }
                | Self::DrownedGoToBeach { .. }
                | Self::Swell { .. }
                | Self::SquidFlee { .. }
                | Self::RandomStand { .. }
                | Self::RangedAttack { .. }
                | Self::TurtleLayEgg { .. }
                | Self::TurtleGoToWater { .. }
        )
    }

    /// `Goal.isInterruptable`：只有狐狸的扑跳不能被打断。
    ///
    /// `Goal.isInterruptable`: only the fox pounce refuses.
    pub const fn interruptable(&self) -> bool {
        match self {
            Self::Fox(goal) => goal.interruptable(),
            _ => true,
        }
    }

    /// 对拍和调试用的名字，官方的类名。
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Float => "FloatGoal",
            Self::EndermanGaze(_) => "EndermanLookForPlayerGoal",
            Self::EndermanFreeze => "EndermanFreezeWhenLookedAt",
            Self::EndermanBlock { place: true } => "EndermanLeaveBlockGoal",
            Self::EndermanBlock { place: false } => "EndermanTakeBlockGoal",
            Self::RabbitPowderSnow => "ClimbOnTopOfPowderSnowGoal",
            Self::RabbitRaid(_) => "RaidGardenGoal",
            Self::Avoid(_) => "AvoidEntityGoal",
            Self::Panic { .. } => "PanicGoal",
            Self::Breed { .. } => "BreedGoal",
            Self::Tempt { .. } => "TemptGoal",
            Self::OcelotAttack { .. } => "OcelotAttackGoal",
            Self::FollowParent { .. } => "FollowParentGoal",
            Self::Stroll { probability, .. } if *probability < 0.0 => "RandomStrollGoal",
            Self::Stroll { .. } => "WaterAvoidingRandomStrollGoal",
            Self::LookAtPlayer { .. } => "LookAtPlayerGoal",
            Self::RandomLookAround { .. } => "RandomLookAroundGoal",
            Self::Unavailable(name) => name,
            Self::MeleeAttack { class, .. } => class,
            Self::Swell { .. } => "SwellGoal",
            Self::Fox(goal) => goal.name(),
            Self::Tame(goal) => goal.name(),
            Self::Wolf(goal) => goal.name(),
            Self::Cat(goal) => goal.name(),
            Self::Parrot(goal) => goal.name(),
            Self::LeapAtTarget { .. } => "LeapAtTargetGoal",
            Self::Bow(_) => "RangedBowAttackGoal",
            Self::HurtByTarget { .. } => "HurtByTargetGoal",
            Self::NearestAttackableTarget {
                kind: TargetType::Wolf,
                ..
            } => "LlamaAttackWolfGoal",
            Self::NearestAttackableTarget { .. } => "NearestAttackableTargetGoal",
            Self::RestrictSun => "RestrictSunGoal",
            Self::FleeSun { .. } => "FleeSunGoal",
            Self::BreakDoor => "BreakDoorGoal",
            Self::TurtleEgg { .. } => "ZombieAttackTurtleEggGoal",
            Self::RandomStand { .. } => "RandomStandGoal",
            Self::RunAroundLikeCrazy { .. } => "RunAroundLikeCrazyGoal",
            Self::RangedAttack { .. } => "RangedAttackGoal",
            Self::LlamaHurtByTarget { .. } => "LlamaHurtByTargetGoal",
            Self::EatBlock { .. } => "EatBlockGoal",
            Self::CubeFloat => "CubeMobFloatGoal",
            Self::CubeAttack { .. } => "CubeMobAttackGoal",
            Self::CubeRandomDirection { .. } => "CubeMobRandomDirectionGoal",
            Self::CubeKeepJumping => "CubeMobKeepOnJumpingGoal",
            Self::DrownedGoToWater { .. } => "DrownedGoToWaterGoal",
            Self::DrownedSwimUp { .. } => "DrownedSwimUpGoal",
            Self::DrownedGoToBeach { .. } => "DrownedGoToBeachGoal",
            Self::RandomStroll { .. } => "RandomStrollGoal",
            Self::WitchHealRaiders { .. } => "NearestHealableRaiderTargetGoal",
            Self::WitchRangedAttack { .. } => "RangedAttackGoal",
            Self::PandaSit => "PandaSitGoal",
            Self::PandaLieOnBack => "PandaLieOnBackGoal",
            Self::PandaSneeze => "PandaSneezeGoal",
            Self::PandaRoll => "PandaRollGoal",
            Self::AvoidEntity { .. } => "PandaAvoidGoal",
            Self::SquidRandomMovement => "SquidRandomMovementGoal",
            Self::SquidFlee { .. } => "SquidFleeGoal",
            Self::Swim { .. } => "RandomSwimmingGoal",
            Self::PufferfishPuff => "PufferfishPuffGoal",
            Self::FollowFlock { .. } => "FollowFlockLeaderGoal",
            Self::TurtleLayEgg { .. } => "TurtleLayEggGoal",
            Self::TurtleGoHome { .. } => "TurtleGoHomeGoal",
            Self::TurtleGoToWater { .. } => "TurtleGoToWaterGoal",
            Self::TurtleTravel { .. } => "TurtleTravelGoal",
        }
    }

    /// 官方类名。熊猫的若干目标是内部子类，名字按种类换。
    ///
    /// The vanilla class name. Several panda goals are inner subclasses whose names
    /// depend on the mob kind.
    pub fn name_for(&self, mob_type: super::MobType) -> &'static str {
        use super::MobType;
        if mob_type == MobType::Panda {
            return match self {
                Self::Panic { .. } => "PandaPanicGoal",
                Self::Breed { .. } => "PandaBreedGoal",
                Self::MeleeAttack { .. } => "PandaAttackGoal",
                Self::LookAtPlayer { .. } => "PandaLookAtPlayerGoal",
                Self::HurtByTarget { .. } => "PandaHurtByTargetGoal",
                _ => self.name(),
            };
        }
        self.name()
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
    /// 按 `(优先级, 目标)` 建一个选择器，调用顺序就是官方的 `addGoal` 顺序。
    ///
    /// Builds a selector from `(priority, goal)` pairs in vanilla `addGoal` order.
    pub(super) fn from_ordered_pairs(goals: Vec<(i32, Goal)>) -> Self {
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

    /// 按注册顺序给出目标的选择器，别的生物模块用它搭自己的表。
    ///
    /// A selector over goals in registration order, for the per-mob modules.
    pub(super) fn from_goals(goals: Vec<WrappedGoal>) -> Self {
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
                    scare: None,
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
            wrap(3, melee_attack(1.0, false, "ZombieAttackGoal")),
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
                (
                    3,
                    Goal::Avoid(super::avoid::AvoidEntity::new(
                        super::avoid::AvoidClass::Wolf,
                        6.0,
                        1.0,
                        1.2,
                    )),
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
        melee_attack(1.2, false, "MeleeAttackGoal")
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
            Goal::Bow(super::skeleton::BowGoal::for_kind(
                mob.mob_type,
                ctx.blocks().difficulty(),
            ))
        } else {
            Self::skeleton_melee()
        };
    }

    /// `Zombie.addBehaviourGoals` 里的 `targetSelector`。缺实体的目标仍注册，
    /// 尚未实现的类型仍按间隔取数，但不查世界。
    ///
    /// Unimplemented types still draw for the interval but do not search the world.
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
                        light_gate: false,
                    },
                ),
                wrap(
                    3,
                    Goal::NearestAttackableTarget {
                        kind: TargetType::Villager,
                        interval: reduced_tick_delay(10),
                        unseen_ticks: 0,
                        found: None,
                        light_gate: false,
                    },
                ),
                wrap(
                    3,
                    Goal::NearestAttackableTarget {
                        kind: TargetType::IronGolem,
                        interval: reduced_tick_delay(10),
                        unseen_ticks: 0,
                        found: None,
                        light_gate: false,
                    },
                ),
                wrap(
                    5,
                    Goal::NearestAttackableTarget {
                        kind: TargetType::Turtle,
                        interval: reduced_tick_delay(10),
                        unseen_ticks: 0,
                        found: None,
                        light_gate: false,
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
        self.remove_goals(|goal| matches!(goal, Goal::BreakDoor));
    }

    /// `GoalSelector.removeAllGoals`：删掉匹配的目标，并同步清掉或平移 `locked` 下标。
    ///
    /// `GoalSelector.removeAllGoals`: drop matching goals and update `locked` indices.
    pub(super) fn remove_goals(&mut self, predicate: impl Fn(&Goal) -> bool) {
        let mut new_indices = Vec::with_capacity(self.goals.len());
        let mut next = 0usize;
        let mut kept = Vec::with_capacity(self.goals.len());
        for wrapped in self.goals.drain(..) {
            if predicate(&wrapped.goal) {
                new_indices.push(None);
            } else {
                new_indices.push(Some(next));
                next += 1;
                kept.push(wrapped);
            }
        }
        self.goals = kept;
        for slot in &mut self.locked {
            *slot = slot.and_then(|index| new_indices.get(index).copied().flatten());
        }
    }

    /// 对拍的改坏确认用：把目标按优先级排序（官方按注册顺序）。
    pub fn sort_by_priority_for_oracle(&mut self) {
        self.goals.sort_by_key(|wrapped| wrapped.priority);
    }

    /// `setControlFlag`。
    pub fn set_control_flag(&mut self, flag: Flag, enabled: bool) {
        self.disabled[flag as usize] = !enabled;
    }

    /// 正在跑的目标的官方类名：兔子和狐狸的几个目标是子类，名字按生物种类换。
    ///
    /// The vanilla class names of the running goals; a few rabbit and fox goals
    /// are subclasses, renamed by mob type.
    pub fn running_class_names(&self, kind: super::MobType) -> Vec<&'static str> {
        self.running_names(kind)
            .into_iter()
            .map(|name| subclass_name(kind, name))
            .collect()
    }

    /// 正在跑的目标名，按加入顺序。对拍用。
    pub fn running_names(&self, mob_type: super::MobType) -> Vec<&'static str> {
        self.goals
            .iter()
            .filter(|wrapped| wrapped.running)
            .map(|wrapped| wrapped.goal.name_for(mob_type))
            .collect()
    }

    /// 正在跑的目标的官方类名：兔子和狐狸的几个目标是子类，名字按生物种类换。
    /// 别的目标自带 `class`（如溺尸的近战目标是 `Drowned.DrownedAttackGoal`）。
    ///
    /// The vanilla class names of the running goals: a few rabbit and fox goals are
    /// subclasses renamed by mob type; other goals carry their own `class` (the
    /// drowned's melee goal is `Drowned.DrownedAttackGoal`).
    pub fn running_names_of(&self, mob: &Mob) -> Vec<&'static str> {
        self.running_class_names(mob.mob_type)
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
            // `WrappedGoal.canBeReplacedBy`：占着的目标可打断、而且优先级更低。
            // `WrappedGoal.canBeReplacedBy`: the holder is interruptable and lower priority.
            self.locked[*flag as usize].is_none_or(|holder| {
                self.goals[holder].goal.interruptable() && priority < self.goals[holder].priority
            })
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

/// `MeleeAttackGoal(mob, speed, followingTargetEvenIfNotSeen)` 及其子类。
///
/// `MeleeAttackGoal(mob, speed, followingTargetEvenIfNotSeen)` and subclasses.
pub(super) const fn melee_attack(speed: f64, track_target: bool, class: &'static str) -> Goal {
    Goal::MeleeAttack {
        speed,
        ticks_until_next_attack: 0,
        ticks_until_path_recalc: 0,
        pathed_target: Vec3::new(0.0, 0.0, 0.0),
        raise_arm_ticks: 0,
        last_can_use_check: 0,
        start_path: None,
        track_target,
        class,
    }
}

/// 兔子和狐狸把几个通用目标包成内部子类；对拍按官方的子类名比较。
///
/// Rabbits and foxes wrap a few shared goals in inner subclasses; the oracle
/// compares the subclass names.
fn subclass_name(kind: super::MobType, name: &'static str) -> &'static str {
    use super::MobType::{Cat, Fox, Ocelot, Parrot, Rabbit, Tadpole, Turtle, Wolf};
    match (kind, name) {
        // `AbstractFish.FishSwimGoal`。
        (Tadpole, "RandomSwimmingGoal") => "FishSwimGoal",
        (Turtle, "PanicGoal") => "TurtlePanicGoal",
        (Turtle, "BreedGoal") => "TurtleBreedGoal",
        (Turtle, "RandomStrollGoal") => "TurtleRandomStrollGoal",
        (Wolf | Cat | Parrot, "PanicGoal") => "TamableAnimalPanicGoal",
        (Rabbit, "PanicGoal") => "RabbitPanicGoal",
        (Rabbit, "AvoidEntityGoal") => "RabbitAvoidEntityGoal",
        (Fox, "PanicGoal") => "FoxPanicGoal",
        (Fox, "BreedGoal") => "FoxBreedGoal",
        (Fox, "FollowParentGoal") => "FoxFollowParentGoal",
        (Fox, "LookAtPlayerGoal") => "FoxLookAtPlayerGoal",
        (Ocelot, "TemptGoal") => "OcelotTemptGoal",
        (Ocelot, "AvoidEntityGoal") => "OcelotAvoidEntityGoal",
        (Cat, "AvoidEntityGoal") => "CatAvoidEntityGoal",
        _ => name,
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

/// `SwellGoal.canUse`：已经在膨胀，或者目标活着且不到 3 格。
///
/// `SwellGoal.canUse`: already swelling, or a live target closer than 3 blocks.
fn swell_can_use(me: &Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    mob.creeper.swell_dir > 0
        || combat_target(mob, ctx)
            .is_some_and(|target| me.position.distance_to_sqr(target.position) < 9.0)
}

/// `SwellGoal.tick`：记住的目标没了、超过 7 格或看不见就收缩，否则膨胀。
/// 记住的目标按原始引用查（只看死活），不走 `getTarget()` 的可攻击过滤。
///
/// `SwellGoal.tick`: shrink when the remembered target is gone, beyond 7 blocks or out
/// of sight, otherwise swell. The remembered target is a raw reference (alive or not),
/// not filtered like `getTarget()`.
fn swell_tick(target: Option<EntityUid>, me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    let seen = target
        .and_then(|uid| raw_target(ctx, uid))
        .is_some_and(|(uid, position, eye)| {
            me.position.distance_to_sqr(position) <= 49.0
                && living::has_line_of_sight(me, mob, ctx.blocks(), uid, eye)
        });
    mob.creeper.swell_dir = if seen { 1 } else { -1 };
}

/// 按 uid 找一个生物或玩家的位置和眼睛位置（不管死活，给 `OcelotAttackGoal.tick` 用）。
///
/// Position and eye position of a mob or player by uid regardless of alive state
/// (used by `OcelotAttackGoal.tick`).
pub(super) fn entity_look_target(
    ctx: &dyn EntityCtx,
    uid: EntityUid,
) -> Option<(EntityUid, Vec3, Vec3)> {
    if let Some(player) = player_by_uid(ctx.blocks(), uid) {
        return Some((uid, player.position, player.eye_position()));
    }
    let other = ctx.other(uid)?;
    let KindPublic::Mob(target) = &other.kind else {
        return None;
    };
    let (_, _, eye) = target.mob_type.dimensions(target.baby);
    let eye = Vec3::new(
        other.position.x,
        other.position.y + f64::from(eye),
        other.position.z,
    );
    Some((uid, other.position, eye))
}

/// 按 uid 找一个还活着的玩家或生物：（uid，位置，眼睛位置）。
///
/// A living player or mob by uid: (uid, position, eye position).
fn raw_target(ctx: &dyn EntityCtx, uid: EntityUid) -> Option<(EntityUid, Vec3, Vec3)> {
    if let Some(player) = player_by_uid(ctx.blocks(), uid) {
        return player
            .alive
            .then(|| (uid, player.position, player.eye_position()));
    }
    let other = ctx.other(uid)?;
    let KindPublic::Mob(target) = &other.kind else {
        return None;
    };
    if !target.is_alive() {
        return None;
    }
    let (_, _, eye) = target.mob_type.dimensions(target.baby);
    let eye = Vec3::new(
        other.position.x,
        other.position.y + f64::from(eye),
        other.position.z,
    );
    Some((uid, other.position, eye))
}

/// `Goal.reducedTickDelay`。
pub(crate) const fn reduced_tick_delay(ticks: i32) -> i32 {
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

/// `Entity.getLightLevelDependentMagicValue()`：眼睛所在格的亮度换算成 0–1。
/// 主世界环境光为 0，最后一步线性插值等于原值。
///
/// `Entity.getLightLevelDependentMagicValue()` at the eye: 0–1 from the light level.
/// The overworld ambient light is 0, so the closing lerp is the identity.
fn light_dependent_magic(me: &Entity, mob: &Mob, ctx: &dyn EntityCtx) -> f32 {
    let view = ctx.blocks();
    let eye = Vec3::new(
        me.position.x,
        me.position.y + f64::from(mob.eye_height),
        me.position.z,
    );
    let raw =
        view.max_local_raw_brightness(BlockPos::containing(eye), view.sky_darken()) as f32 / 15.0;
    raw / (4.0 - 3.0 * raw)
}

fn can_use(goal: &mut Goal, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) -> bool {
    match goal {
        Goal::Bow(_) => {
            super::skeleton::bow_slot(mob).is_some() && combat_target(mob, ctx).is_some()
        }
        Goal::Float => float_can_use(me, mob),
        Goal::EndermanGaze(goal) => goal.can_use(me, mob, ctx),
        Goal::EndermanFreeze => super::enderman::freeze_target(me, mob, ctx).is_some(),
        Goal::EndermanBlock { place } => {
            super::enderman::block_can_use(me, mob, ctx.blocks(), *place)
        }
        Goal::RabbitPowderSnow => super::rabbit::can_climb_powder_snow(me, mob, ctx.blocks()),
        Goal::RabbitRaid(raid) => raid.can_use(me, mob, ctx.blocks()),
        // `RabbitAvoidEntityGoal`：杀手兔不躲；`OcelotAvoidEntityGoal`：信任的豹猫不躲；
        // `CatAvoidEntityGoal`：驯服的猫不躲。
        // `RabbitAvoidEntityGoal`: the killer bunny never flees; `OcelotAvoidEntityGoal`:
        // trusting ocelots never flee; `CatAvoidEntityGoal`: tamed cats never flee.
        Goal::Avoid(avoid) => {
            !super::rabbit::is_evil(mob)
                && !(mob.mob_type == super::MobType::Ocelot && mob.ocelot.trusting)
                && !(mob.mob_type == super::MobType::Cat && mob.tame.is_tame())
                && avoid.can_use(me, mob, ctx)
        }
        Goal::OcelotAttack { target, .. } => {
            let best = mob.target.filter(|&uid| raw_target(ctx, uid).is_some());
            if best.is_some() {
                *target = best;
            }
            best.is_some()
        }
        Goal::Fox(goal) => goal.can_use(me, mob, ctx),
        Goal::Tame(goal) => goal.can_use(me, mob, ctx),
        Goal::Wolf(goal) => goal.can_use(me, mob, ctx),
        Goal::Cat(goal) => goal.can_use(me, mob, ctx),
        Goal::Parrot(goal) => goal.can_use(me, mob, ctx),
        Goal::LeapAtTarget { .. } => leap_can_use(me, mob, ctx),
        Goal::Panic { target, .. } => {
            // `FoxPanicGoal.shouldPanic`：保护中的狐狸不惊慌。
            // `FoxPanicGoal.shouldPanic`: a defending fox never panics.
            if super::fox::is_defending_fox(mob) || !should_panic(mob, ctx) {
                return false;
            }
            if mob.mob_type == super::MobType::Turtle {
                // `TurtlePanicGoal.canUse`：先找 7 格内的水，找不到再取随机点。
                // `TurtlePanicGoal.canUse`: look for water within seven blocks first.
                if let Some(water) = look_for_water(ctx.blocks(), me.block_position(), 7) {
                    *target = Vec3::new(f64::from(water.x), f64::from(water.y), f64::from(water.z));
                    return true;
                }
                return match default_random_pos(me, mob, ctx.blocks(), 5, 4) {
                    Some(found) => {
                        *target = found;
                        true
                    }
                    None => false,
                };
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
            // `Turtle.TurtleBreedGoal.canUse`：身上有蛋时不求爱。
            // `Turtle.TurtleBreedGoal.canUse`: no breeding while carrying an egg.
            if mob.mob_type == super::MobType::Turtle && !super::turtle::can_fall_in_love(mob) {
                return false;
            }
            *partner = free_partner(me, mob, ctx);
            if partner.is_none() {
                return false;
            }
            // `PandaBreedGoal.canUse`：不乐意时先记下来并在 30 秒内不再试；附近没有竹子就
            // 不繁殖，还要让「看玩家」目标盯着最近的玩家。
            // `PandaBreedGoal.canUse`: an unhappy panda refuses for 30 seconds; with no
            // bamboo nearby it gives up and points its look-at-player goal at the nearest one.
            if mob.mob_type == super::MobType::Panda && !super::panda::breed_can_use(me, mob, ctx) {
                *partner = None;
                return false;
            }
            true
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
            if mob.age >= 0 || super::fox::is_defending_fox(mob) {
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
            // `Turtle.TurtleRandomStrollGoal.canUse`：水里、回家途中、身上有蛋都不逛。
            // `Turtle.TurtleRandomStrollGoal.canUse`: no strolling in water, going home or
            // carrying an egg.
            if mob.mob_type == super::MobType::Turtle
                && (me.in_water || mob.turtle.going_home || mob.turtle.has_egg)
            {
                return false;
            }
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
        Goal::Swim {
            interval, wanted, ..
        } => {
            if mob.no_action_time >= 100 {
                return false;
            }
            if me.random.next_int_bounded(reduced_tick_delay(*interval)) != 0 {
                return false;
            }
            let Some(found) = super::fish::random_swim_pos(me, mob, ctx.blocks()) else {
                return false;
            };
            *wanted = found;
            true
        }
        Goal::PufferfishPuff => super::fish::puff_can_use(me, ctx),
        Goal::FollowFlock { next_start, .. } => {
            super::fish::flock_can_use(next_start, me, mob, ctx)
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
            // `PandaLookAtPlayerGoal.canUse`：已有的目标不重找，繁殖目标塞进来的优先。
            // `PandaLookAtPlayerGoal.canUse`: an existing target is kept, and the breeding
            // goal's pick comes first.
            if mob.mob_type == super::MobType::Panda {
                if look_at.is_none() {
                    *look_at =
                        mob.panda.look_at_override.take().or_else(|| {
                            nearest_look_target(me, mob, ctx, f64::from(*look_distance))
                        });
                }
                return mob.panda_can_perform_action(ctx.blocks().is_thundering())
                    && look_at.is_some();
            }
            *look_at = nearest_look_target(me, mob, ctx, f64::from(*look_distance));
            // `FoxLookAtPlayerGoal`：趴倒或者感兴趣时不看人。
            // `FoxLookAtPlayerGoal`: no looking while faceplanted or interested.
            look_at.is_some() && !super::fox::blocks_look(mob)
        }
        Goal::RandomLookAround { .. } => me.random.next_float() < 0.02,
        Goal::Unavailable(_) => false,
        Goal::Swell { .. } => swell_can_use(me, mob, ctx),
        Goal::RandomStand { next_stand } => {
            super::equine::random_stand_can_use(me, mob, next_stand)
        }
        Goal::RunAroundLikeCrazy { wanted, .. } => {
            super::equine::tame::can_use(me, mob, ctx, wanted)
        }
        Goal::MeleeAttack {
            last_can_use_check,
            start_path,
            ..
        } => {
            // `PandaAttackGoal.canUse`：先要求 `canPerformAction`。
            // `PandaAttackGoal.canUse`: `canPerformAction` comes first.
            if mob.mob_type == super::MobType::Panda
                && !mob.panda_can_perform_action(ctx.blocks().is_thundering())
            {
                return false;
            }
            // `FoxMeleeAttackGoal.canUse`：坐、睡、蹲、趴倒时先短路。
            // `FoxMeleeAttackGoal.canUse` short-circuits first.
            if super::fox::blocks_melee(mob) {
                return false;
            }
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
                    if mob.mob_type == super::MobType::Witch {
                        // `new HurtByTargetGoal(this, Raider.class)`：女巫不报复自己或别的袭击者。
                        //
                        // `new HurtByTargetGoal(this, Raider.class)`: witches ignore themselves
                        // and other raiders.
                        uid != me.id
                            && !ctx.other(uid).is_some_and(|other| {
                                matches!(
                                    &other.kind,
                                    KindPublic::Mob(other_mob)
                                        if other_mob.mob_type == super::MobType::Witch
                                )
                            })
                            && target_by_uid(mob, ctx, uid).is_some()
                    } else {
                        !(mob.mob_type.full_target_rules()
                            || mob.mob_type == super::MobType::PolarBear)
                            || target_by_uid(mob, ctx, uid).is_some()
                    }
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
            light_gate,
            ..
        } => {
            // `PolarBearAttackPlayersGoal.canUse`：幼崽在取数之前就返回。
            // `PolarBearAttackPlayersGoal.canUse`: a cub returns before any draw.
            if *kind == TargetType::GuardedPlayer && mob.is_baby() {
                return false;
            }
            // `SpiderTargetGoal.canUse`：亮处直接返回假，连 `nextInt` 都不取。
            // `SpiderTargetGoal.canUse`: bright light returns false before any draw.
            if *light_gate && light_dependent_magic(me, mob, ctx) >= 0.5 {
                return false;
            }
            // 官方先按 `randomInterval` 取数，再按类型找目标。缺实体的目标
            // 虽跳过搜索，取数次数必须和官方一致，否则随机源会错开。
            if *interval > 0 && me.random.next_int_bounded(*interval) != 0 {
                return false;
            }
            *found = match kind {
                TargetType::FoxPrey => {
                    nearest_mob_target(me, mob, ctx, super::fox_goals::is_small_prey)
                }
                TargetType::Wolf => nearest_wild_wolf_target(me, mob, ctx),
                // `NearestAttackableTargetGoal<Fox>`：北极熊找狐狸，幼崽不打。
                // 不要把这个分支并回下面的玩家搜索：08:40 的回归就是 Fox 走了玩家分支，
                // 出生点玩家被设成目标，愤怒结束时间和世界随机源从第 1 刻起分叉。
                //
                // `NearestAttackableTargetGoal<Fox>`: a polar bear looks for foxes; cubs pass.
                // Never fold this arm back into the player search below: the 08:40 regression
                // was Fox taking the player branch, which targeted the spawn player and diverged
                // the anger end time and world random source from tick 1.
                TargetType::Fox => {
                    if mob.is_baby() {
                        None
                    } else {
                        nearest_mob_target(me, mob, ctx, |kind| kind == super::MobType::Fox)
                    }
                }
                TargetType::Chicken => {
                    nearest_mob_target(me, mob, ctx, |kind| kind == super::MobType::Chicken)
                }
                TargetType::BabyTurtleOnLand => nearest_mob_target_with(me, mob, ctx, |public| {
                    public.mob_type == super::MobType::Turtle && public.baby && !public.in_water
                }),
                TargetType::Player | TargetType::GuardedPlayer | TargetType::AngerPlayer => {
                    nearest_attackable(me, mob, ctx, *kind)
                }
                // 这些目标在官方仍注册、仍按间隔取数，但 Bamboo 没有对应实体可搜，返回 None。
                // 美西螈是 main 追加的占位变体（溺尸的目标），同样只保留取数、不查世界。
                // Vanilla registers these goals and still draws the interval, but Bamboo has no
                // matching entity to search, so they return None. The axolotl is a placeholder
                // variant added on main (a drowned target); it keeps the draw but skips the search.
                TargetType::Villager
                | TargetType::IronGolem
                | TargetType::Turtle
                | TargetType::Axolotl
                | TargetType::Fish
                | TargetType::Endermite => None,
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
        Goal::CubeFloat => living::is_in_liquid(me, mob),
        Goal::CubeAttack { .. } => combat_target(mob, ctx).is_some(),
        Goal::CubeRandomDirection { .. } => {
            combat_target(mob, ctx).is_none() && super::slime::can_pick_direction(me, mob)
        }
        // `!isPassenger()`：没有骑乘。
        // `!isPassenger()`: riding is not modelled.
        Goal::CubeKeepJumping => true,
        Goal::DrownedGoToWater { wanted, .. } => {
            // `DrownedGoToWaterGoal.canUse`：白天、不在水里、找得到水。
            // `DrownedGoToWaterGoal.canUse`: daylight, not in water and water found.
            if !ctx.blocks().is_bright_outside() || me.in_water {
                return false;
            }
            let Some(found) = water_pos(me, ctx.blocks()) else {
                return false;
            };
            *wanted = found;
            true
        }
        // `DrownedSwimUpGoal.canUse`：夜里、在水里、低于海平面 2 格。
        Goal::DrownedSwimUp { .. } => {
            !ctx.blocks().is_bright_outside()
                && me.in_water
                && me.position.y < f64::from(ctx.blocks().sea_level() - 2)
        }
        // `DrownedGoToBeachGoal.canUse`：先走 `MoveToBlockGoal.canUse`（倒计时、取
        // `nextInt(200)`、找岸边），再看天色、是否泡水和海平面。
        //
        // `DrownedGoToBeachGoal.canUse`: `MoveToBlockGoal.canUse` first (countdown,
        // `nextInt(200)`, search), then the sky, water and sea level checks.
        Goal::DrownedGoToBeach {
            next_start_tick,
            block_pos,
            ..
        } => {
            if *next_start_tick > 0 {
                *next_start_tick -= 1;
                return false;
            }
            *next_start_tick = reduced_tick_delay(200 + me.random.next_int_bounded(200));
            let Some(found) = find_beach_target(me, mob, ctx, 8, 2) else {
                return false;
            };
            *block_pos = Some(found);
            !ctx.blocks().is_bright_outside()
                && me.in_water
                && me.position.y >= f64::from(ctx.blocks().sea_level() - 3)
        }
        Goal::RandomStroll {
            interval,
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
            let Some(found) = default_random_pos(me, mob, ctx.blocks(), 10, 7) else {
                return false;
            };
            *wanted = found;
            *force_trigger = false;
            true
        }
        // `NearestHealableRaiderTargetGoal.canUse`：冷却后先抽一个 `nextBoolean`，
        // 再问 `hasActiveRaid()`。Bamboo 没有袭击，所以恒假，但取数保留。
        Goal::WitchHealRaiders { cooldown } => {
            if *cooldown > 0 {
                return false;
            }
            let _rolled = me.random.next_boolean();
            false
        }
        Goal::WitchRangedAttack { target, .. } => {
            let alive = mob.target.filter(|&uid| alive_target(ctx, uid).is_some());
            if alive.is_some() {
                *target = alive;
            }
            alive.is_some()
        }
        Goal::SquidRandomMovement => true,
        Goal::SquidFlee { .. } => super::squid::flee_can_use(me, mob, ctx),
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
        Goal::PandaSit => super::panda::sit_can_use(me, mob, ctx),
        Goal::PandaLieOnBack => super::panda::lie_can_use(me, mob, ctx.blocks().is_thundering()),
        Goal::PandaSneeze => super::panda::sneeze_can_use(me, mob, ctx.blocks().is_thundering()),
        Goal::PandaRoll => super::panda::roll_can_use(me, mob, ctx),
        Goal::AvoidEntity {
            target,
            max_dist,
            walk_speed,
            sprint_speed,
            avoid,
            path,
        } => {
            // `PandaAvoidGoal.canUse`：先要求担心基因并做得了动作。
            // `PandaAvoidGoal.canUse`: the worried gene and a free action come first.
            if !mob.panda.is_worried()
                || !mob.panda_can_perform_action(ctx.blocks().is_thundering())
            {
                return false;
            }
            let found = nearest_avoidable(me, mob, ctx, *target, *max_dist);
            let Some((uid, at)) = found else {
                return false;
            };
            *avoid = Some(uid);
            let Some(pos) = away_random_pos(me, mob, ctx.blocks(), 16, 7, at) else {
                return false;
            };
            if at.distance_to_sqr(pos) < at.distance_to_sqr(me.position) {
                return false;
            }
            let nav = living::nav_mob(me, mob, ctx.blocks());
            *path = mob
                .navigation
                .create_path(ctx.blocks(), &nav, BlockPos::containing(pos), 0);
            let _ = (walk_speed, sprint_speed);
            path.is_some()
        }
        Goal::TurtleLayEgg { .. } => super::turtle::lay_egg_can_use(me, mob, ctx),
        Goal::TurtleGoHome { .. } => super::turtle::go_home_can_use(me, mob),
        Goal::TurtleGoToWater { .. } => super::turtle::go_to_water_can_use(me, mob, ctx),
        Goal::TurtleTravel { .. } => super::turtle::travel_can_use(me, mob),
    }
}

fn can_continue(goal: &mut Goal, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) -> bool {
    match goal {
        Goal::EndermanGaze(goal) => goal.can_continue(me, mob, ctx),
        Goal::EndermanFreeze => super::enderman::freeze_target(me, mob, ctx).is_some(),
        Goal::EndermanBlock { .. } => can_use(goal, me, mob, ctx),
        Goal::Bow(_) => {
            super::skeleton::bow_slot(mob).is_some()
                && (combat_target(mob, ctx).is_some() || !mob.navigation.is_done())
        }
        Goal::RabbitPowderSnow => super::rabbit::can_climb_powder_snow(me, mob, ctx.blocks()),
        Goal::RabbitRaid(raid) => raid.can_continue(),
        Goal::Avoid(_) => {
            !(mob.mob_type == super::MobType::Ocelot && mob.ocelot.trusting)
                && !(mob.mob_type == super::MobType::Cat && mob.tame.is_tame())
                && super::avoid::AvoidEntity::can_continue(mob)
        }
        Goal::OcelotAttack { target, .. } => {
            let Some((_, pos, _)) = target.and_then(|uid| raw_target(ctx, uid)) else {
                return false;
            };
            if me.position.distance_to_sqr(pos) > 225.0 {
                return false;
            }
            !mob.navigation.is_done() || can_use(goal, me, mob, ctx)
        }
        Goal::Fox(goal) => goal.can_continue(me, mob, ctx),
        Goal::Tame(goal) => goal.can_continue(me, mob, ctx),
        Goal::Wolf(goal) => goal.can_continue(me, mob, ctx),
        Goal::Cat(goal) => goal.can_continue(me, mob, ctx),
        Goal::Parrot(goal) => goal.can_continue(me, mob, ctx),
        Goal::LeapAtTarget { .. } => !me.on_ground,
        // `Drowned.DrownedAttackGoal.canContinueToUse`：僵尸的近战条件加 `okTarget`。
        Goal::RandomStroll { .. } | Goal::DrownedGoToWater { .. } => !mob.navigation.is_done(),
        Goal::DrownedSwimUp { stuck, .. } => {
            !*stuck
                && !ctx.blocks().is_bright_outside()
                && me.in_water
                && me.position.y < f64::from(ctx.blocks().sea_level() - 2)
        }
        // `MoveToBlockGoal.canContinueToUse`：尝试次数没超限，目标格仍然合格。
        //
        // `MoveToBlockGoal.canContinueToUse`: within the try limits and the target still valid.
        Goal::DrownedGoToBeach {
            try_ticks,
            max_stay_ticks,
            block_pos,
            ..
        } => {
            *try_ticks >= -*max_stay_ticks
                && *try_ticks <= 1200
                && block_pos.is_some_and(|pos| beach_target_valid(ctx.blocks(), pos))
        }
        Goal::WitchHealRaiders { .. } => {
            combat_target(mob, ctx).is_some_and(|target| target_in_follow_range(me, mob, target))
        }
        // `RangedAttackGoal.canContinueToUse`：`canUse` 或目标还活着且导航没完。
        //
        // `RangedAttackGoal.canContinueToUse`: `canUse` or the stored target is alive while navigating.
        Goal::WitchRangedAttack { target, .. } => {
            let stored = *target;
            can_use(goal, me, mob, ctx)
                || (stored.is_some_and(|uid| alive_target(ctx, uid).is_some())
                    && !mob.navigation.is_done())
        }
        Goal::Panic { .. } => {
            // `PandaPanicGoal.canContinueToUse`：坐下时停下导航。
            // `PandaPanicGoal.canContinueToUse`: sitting stops the navigation.
            if mob.mob_type == super::MobType::Panda && mob.panda.is_sitting() {
                mob.navigation.stop();
                return false;
            }
            !mob.navigation.is_done()
        }
        Goal::Stroll { .. } | Goal::Swim { .. } => !mob.navigation.is_done(),
        Goal::FollowFlock { .. } => super::fish::flock_can_continue(me, mob, ctx),
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
        Goal::Tempt { player, scare, .. } => {
            // `TemptGoal.canContinueToUse`：会受惊且距离 < 6 格时，玩家位移或转头超过阈值就惊跑。
            // `TemptGoal.canContinueToUse`: when scareable within 6 blocks, player movement
            // or rotation past the threshold stops the tempt.
            if let Some(scare) = scare.as_mut()
                && !(mob.mob_type == super::MobType::Ocelot && mob.ocelot.trusting)
            {
                let Some(found) = player.and_then(|uid| player_by_uid(ctx.blocks(), uid)) else {
                    return false;
                };
                if me.position.distance_to_sqr(found.position) < 36.0 {
                    let prev = Vec3::new(scare.px, scare.py, scare.pz);
                    if found.position.distance_to_sqr(prev) > 0.010_000_000_000_000_002 {
                        return false;
                    }
                    if (f64::from(found.pitch) - scare.p_rot_x).abs() > 5.0
                        || (f64::from(found.yaw) - scare.p_rot_y).abs() > 5.0
                    {
                        return false;
                    }
                } else {
                    scare.px = found.position.x;
                    scare.py = found.position.y;
                    scare.pz = found.position.z;
                }
                scare.p_rot_x = f64::from(found.pitch);
                scare.p_rot_y = f64::from(found.yaw);
            }
            can_use(goal, me, mob, ctx)
        }
        Goal::SquidRandomMovement | Goal::SquidFlee { .. } | Goal::PufferfishPuff => {
            can_use(goal, me, mob, ctx)
        }
        Goal::FollowParent { parent, .. } => {
            if mob.age >= 0 || super::fox::is_defending_fox(mob) {
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
            *look_time > 0 && !super::fox::blocks_look(mob)
        }
        Goal::RandomLookAround { look_time, .. } => *look_time >= 0,
        Goal::Float
        | Goal::Swell { .. }
        | Goal::CubeFloat
        | Goal::CubeRandomDirection { .. }
        | Goal::CubeKeepJumping => can_use(goal, me, mob, ctx),
        Goal::CubeAttack { grow_tired_timer } => {
            if combat_target(mob, ctx).is_none() {
                return false;
            }
            *grow_tired_timer -= 1;
            *grow_tired_timer > 0
        }
        Goal::Unavailable(_) | Goal::RandomStand { .. } => false,
        Goal::RunAroundLikeCrazy { .. } => super::equine::tame::can_continue(mob),
        Goal::MeleeAttack {
            track_target,
            class,
            ..
        } => {
            // `Spider.SpiderAttackGoal.canContinueToUse`：亮处每刻先取一次
            // `nextInt(100)`，命中 0 就清掉目标并停下，否则走 `super`。
            // `Spider.SpiderAttackGoal.canContinueToUse`: each bright tick draws
            // `nextInt(100)`; zero drops the target, anything else defers to `super`.
            if *class == "SpiderAttackGoal"
                && light_dependent_magic(me, mob, ctx) >= 0.5
                && me.random.next_int_bounded(100) == 0
            {
                super::enderman::set_target(me, mob, None);
                return false;
            }
            // 官方 `followingTargetEvenIfNotSeen=false`：目标活着且
            // `!navigation.isDone()` 才继续；路径走完就停。为真时只看目标在不在家附近。
            // With `followingTargetEvenIfNotSeen` the goal only checks the home range.
            let Some(target) = combat_target(mob, ctx) else {
                return false;
            };
            if mob.mob_type == super::MobType::Drowned
                && !super::drowned::ok_target(
                    ctx,
                    Some((target.position, combat_target_in_water(ctx, &target))),
                )
            {
                return false;
            }
            if *track_target {
                within_home(mob, BlockPos::containing(target.position))
            } else {
                !mob.navigation.is_done()
            }
        }
        Goal::NearestAttackableTarget {
            unseen_ticks, kind, ..
        } if mob.mob_type == super::MobType::PolarBear => {
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
        Goal::NearestAttackableTarget { unseen_ticks, .. }
            if mob.mob_type.full_target_rules() || mob.mob_type == super::MobType::Slime =>
        {
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
            }

            *unseen_ticks <= reduced_tick_delay(60)
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
        Goal::HurtByTarget { .. } => {
            // `PandaHurtByTargetGoal.canContinueToUse`：吃到了竹子或咬过一口就停手。
            // `PandaHurtByTargetGoal.canContinueToUse`: stop after eating bamboo or biting.
            if mob.mob_type == super::MobType::Panda && (mob.panda.got_bamboo || mob.panda.did_bite)
            {
                mob.target = None;
                return false;
            }
            // `TargetGoal.canContinueToUse`：`getTarget()` 把打不了的目标（死了、创造、
            // 旁观、和平难度）读成 null，回退到的 `targetMob` 是同一个玩家，同样打不了；
            // 打得了还要在跟随距离内。`mustSee` 的视线记忆本期不建模（目标恒看得见）。
            combat_target(mob, ctx).is_some_and(|target| target_in_follow_range(me, mob, target))
        }
        Goal::NearestAttackableTarget { kind, .. } => {
            // `LlamaAttackWolfGoal.getFollowDistance` 是跟随距离的 0.25 倍。
            // `LlamaAttackWolfGoal.getFollowDistance` is 0.25× `FOLLOW_RANGE`.
            combat_target(mob, ctx).is_some_and(|target| {
                target_in_scaled_follow_range(me, mob, target, kind.follow_scale())
            })
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
        Goal::PandaSit => super::panda::sit_can_continue(me, mob),
        Goal::PandaLieOnBack => super::panda::lie_can_continue(me, mob),
        Goal::PandaSneeze | Goal::PandaRoll => false,
        Goal::AvoidEntity { .. } => !mob.navigation.is_done(),
        Goal::TurtleLayEgg { .. } => super::turtle::lay_egg_can_continue(me, mob, ctx),
        Goal::TurtleGoHome { .. } => super::turtle::go_home_can_continue(me, mob),
        Goal::TurtleGoToWater { .. } => super::turtle::go_to_water_can_continue(me, mob, ctx),
        Goal::TurtleTravel { stuck, .. } => super::turtle::travel_can_continue(mob, *stuck),
    }
}

fn start_goal(goal: &mut Goal, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    match goal {
        Goal::EndermanGaze(goal) => goal.start(mob),
        Goal::EndermanFreeze => mob.navigation.stop(),
        Goal::EndermanBlock { .. } => {}
        Goal::RabbitRaid(raid) => raid.start(me, mob, ctx.blocks()),
        Goal::RabbitPowderSnow => {}
        Goal::Avoid(avoid) => avoid.start(me, mob, ctx),
        Goal::Fox(goal) => goal.start(me, mob, ctx),
        Goal::Tame(goal) => goal.start(me, mob, ctx),
        Goal::Wolf(goal) => goal.start(me, mob, ctx),
        Goal::Cat(goal) => goal.start(me, mob, ctx),
        Goal::Parrot(goal) => goal.start(me, mob, ctx),
        Goal::LeapAtTarget { yd } => leap_start(me, mob, ctx, *yd),
        Goal::Bow(_) => mob.aggressive = true,
        Goal::Panic { speed, target, .. } => {
            let nav = living::nav_mob(me, mob, ctx.blocks());
            mob.navigation.move_to(ctx.blocks(), &nav, *target, *speed);
        }
        Goal::FollowParent {
            time_to_recalc_path,
            ..
        } => {
            // `FoxFollowParentGoal.start`：先 `clearStates`。
            // `FoxFollowParentGoal.start` clears the fox states first.
            super::fox::clear_states_if_fox(mob);
            *time_to_recalc_path = 0;
        }
        Goal::Stroll { speed, wanted, .. } => {
            let nav = living::nav_mob(me, mob, ctx.blocks());
            mob.navigation.move_to(ctx.blocks(), &nav, *wanted, *speed);
        }
        Goal::RandomStroll { speed, wanted, .. } | Goal::DrownedGoToWater { speed, wanted } => {
            let nav = living::nav_mob(me, mob, ctx.blocks());
            mob.navigation.move_to(ctx.blocks(), &nav, *wanted, *speed);
        }
        // `DrownedSwimUpGoal.start` 打开「正在找陆地」。
        Goal::DrownedSwimUp { stuck, .. } => {
            mob.searching_for_land = true;
            *stuck = false;
        }
        // `DrownedGoToBeachGoal.start`：先关掉「正在找陆地」，再走 `MoveToBlockGoal.start`。
        //
        // `DrownedGoToBeachGoal.start`: clear "searching for land", then `MoveToBlockGoal.start`.
        Goal::DrownedGoToBeach {
            speed,
            try_ticks,
            max_stay_ticks,
            block_pos,
            ..
        } => {
            mob.searching_for_land = false;
            if let Some(pos) = *block_pos {
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
            *try_ticks = 0;
            let inner = me.random.next_int_bounded(1200);
            *max_stay_ticks = me.random.next_int_bounded(inner + 1200) + 1200;
        }
        Goal::WitchHealRaiders { cooldown } => *cooldown = reduced_tick_delay(200),
        Goal::Swim { speed, wanted, .. } => {
            let nav = living::nav_mob(me, mob, ctx.blocks());
            mob.navigation.move_to(ctx.blocks(), &nav, *wanted, *speed);
        }
        Goal::PufferfishPuff => super::fish::puff_start(mob),
        Goal::FollowFlock { time_to_recalc, .. } => *time_to_recalc = 0,
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
        Goal::Breed { partner, .. } => {
            // `FoxBreedGoal.start`：两只狐狸都 `clearStates`。
            // `FoxBreedGoal.start` clears the states of both foxes.
            if mob.mob_type == super::MobType::Fox
                && let Some(partner) = *partner
            {
                mob.fox.clear_states();
                ctx.emit(
                    crate::entity::ctx::Command::FoxClearStates { target: partner },
                    me,
                );
            }
        }
        Goal::Tempt { player, scare, .. } => {
            if let Some(scare) = scare.as_mut()
                && let Some(found) = player.and_then(|uid| player_by_uid(ctx.blocks(), uid))
            {
                scare.px = found.position.x;
                scare.py = found.position.y;
                scare.pz = found.position.z;
            }
        }
        Goal::Float
        | Goal::OcelotAttack { .. }
        | Goal::Unavailable(_)
        | Goal::CubeFloat
        | Goal::CubeRandomDirection { .. }
        | Goal::CubeKeepJumping
        | Goal::WitchRangedAttack { .. }
        | Goal::SquidRandomMovement => {}
        Goal::CubeAttack { grow_tired_timer } => *grow_tired_timer = reduced_tick_delay(300),
        Goal::Swell { target } => {
            // `SwellGoal.start`：停下导航，记住这一刻的目标。
            // `SwellGoal.start`: stop navigating and remember the current target.
            mob.navigation.stop();
            *target = combat_target(mob, ctx).map(|found| found.uid);
        }
        Goal::SquidFlee { flee_ticks } => *flee_ticks = 0,
        Goal::RandomStand { .. } => super::equine::random_stand_start(me, mob),
        Goal::RunAroundLikeCrazy { speed, wanted } => {
            super::equine::tame::start(me, mob, ctx, *wanted, *speed)
        }
        Goal::MeleeAttack {
            speed,
            ticks_until_next_attack,
            ticks_until_path_recalc,
            raise_arm_ticks,
            start_path,
            ..
        } => {
            // `FoxMeleeAttackGoal.start`：先取消感兴趣。
            // `FoxMeleeAttackGoal.start` clears the interested flag first.
            if mob.mob_type == super::MobType::Fox {
                mob.fox.set(super::fox::FLAG_INTERESTED, false);
            }
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
            super::enderman::set_target(me, mob, mob.last_hurt_by_mob);
            *unseen_ticks = 0;
            // `PolarBearHurtByTargetGoal.start`：幼崽叫来成年北极熊，自己当场 `stop()`。
            // `PolarBearHurtByTargetGoal.start`: a cub alerts the adults and stops at once.
            if mob.mob_type == super::MobType::PolarBear && mob.is_baby() {
                super::polar_bear::alert_adults(me, mob, ctx);
                mob.target = None;
            }
            // `PandaHurtByTargetGoal` 带 `setAlertOthers()`，只叫好斗的熊猫。
            // `PandaHurtByTargetGoal` sets `setAlertOthers()`, calling aggressive pandas.
            if mob.mob_type == super::MobType::Panda {
                super::panda::alert_pandas(me, mob, ctx);
            }
        }
        // `HurtByTargetGoal.start` 再 `TargetGoal.start`：`unseenTicks` 清零。
        // `HurtByTargetGoal.start`, then `TargetGoal.start` clears `unseenTicks`.
        Goal::LlamaHurtByTarget {
            timestamp,
            unseen_ticks,
        } => {
            *timestamp = mob.last_hurt_by_mob_timestamp;
            super::enderman::set_target(me, mob, mob.last_hurt_by_mob);
            *unseen_ticks = 0;
        }
        Goal::RangedAttack { .. } => {}
        Goal::NearestAttackableTarget {
            found,
            unseen_ticks,
            ..
        } => {
            super::enderman::set_target(me, mob, *found);
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
        Goal::PandaSit => super::panda::sit_start(me, mob, ctx),
        Goal::PandaLieOnBack => super::panda::lie_start(me, mob),
        Goal::PandaSneeze => mob.panda.sneeze(true),
        Goal::PandaRoll => mob.panda.roll(true),
        Goal::AvoidEntity {
            path, walk_speed, ..
        } => {
            if let Some(path) = path.take() {
                let nav = living::nav_mob(me, mob, ctx.blocks());
                mob.navigation
                    .move_to_path(ctx.blocks(), &nav, Some(path), *walk_speed);
            }
        }
        Goal::TurtleLayEgg { speed } => super::turtle::lay_egg_start(me, mob, ctx, *speed),
        Goal::TurtleGoHome { .. } => super::turtle::go_home_start(mob),
        Goal::TurtleGoToWater { speed } => super::turtle::go_to_water_start(me, mob, ctx, *speed),
        Goal::TurtleTravel { stuck, .. } => *stuck = super::turtle::travel_start(me, mob, ctx),
    }
}

fn stop_goal(goal: &mut Goal, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    match goal {
        Goal::EndermanGaze(goal) => goal.stop(me, mob),
        Goal::EndermanFreeze | Goal::EndermanBlock { .. } => {}
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
        Goal::OcelotAttack { target, .. } => {
            *target = None;
            mob.navigation.stop();
        }
        Goal::FollowParent { parent, .. } => *parent = None,
        Goal::Stroll { .. } => mob.navigation.stop(),
        Goal::Avoid(avoid) => avoid.stop(),
        Goal::Fox(goal) => goal.stop_with_world(me, mob, ctx),
        Goal::Tame(goal) => goal.stop(mob),
        Goal::Wolf(goal) => goal.stop(mob),
        Goal::Cat(goal) => goal.stop(mob),
        Goal::Parrot(goal) => goal.stop(mob),
        Goal::LeapAtTarget { .. } => {}
        Goal::Swim { .. } => mob.navigation.stop(),
        // `MoveToBlockGoal` 没有覆写 `stop`：不停导航。
        // `MoveToBlockGoal` does not override `stop`, so the navigation keeps going.
        Goal::TurtleLayEgg { .. } | Goal::TurtleGoToWater { .. } => {}
        Goal::TurtleGoHome { .. } => super::turtle::go_home_stop(mob),
        Goal::TurtleTravel { .. } => mob.turtle.travel_pos = None,
        Goal::PufferfishPuff => super::fish::puff_stop(mob),
        Goal::FollowFlock { .. } => super::fish::flock_stop(mob, ctx, me),
        Goal::LookAtPlayer { look_at, .. } => *look_at = None,
        Goal::Swell { target } => *target = None,
        Goal::Float
        | Goal::RabbitRaid(_)
        | Goal::RabbitPowderSnow
        | Goal::Panic { .. }
        | Goal::RandomLookAround { .. }
        | Goal::Unavailable(_)
        | Goal::CubeFloat
        | Goal::CubeAttack { .. }
        | Goal::CubeRandomDirection { .. }
        | Goal::CubeKeepJumping
        | Goal::DrownedGoToWater { .. }
        | Goal::RandomStroll { .. }
        | Goal::RandomStand { .. }
        | Goal::WitchHealRaiders { .. }
        | Goal::RunAroundLikeCrazy { .. } => {}
        // `DrownedSwimUpGoal.stop` 关掉「正在找陆地」。
        Goal::DrownedSwimUp { .. } => mob.searching_for_land = false,
        // `DrownedGoToBeachGoal.stop` 只调 `super.stop()`，而 `MoveToBlockGoal` 没有覆写
        // `stop`，所以官方这一步什么都不做。
        //
        // `DrownedGoToBeachGoal.stop` only calls `super.stop()`, and `MoveToBlockGoal` does
        // not override `stop`, so vanilla does nothing here.
        Goal::DrownedGoToBeach { .. } => {}
        Goal::WitchRangedAttack {
            target,
            attack_time,
            see_time,
        } => {
            *target = None;
            *see_time = 0;
            *attack_time = -1;
        }
        Goal::RestrictSun => mob.navigation.avoid_sun = false,
        // `TargetGoal.stop`：`setTarget(null)`，`targetMob = null`。
        Goal::HurtByTarget { .. } | Goal::LlamaHurtByTarget { .. } => {
            super::enderman::set_target(me, mob, None)
        }
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
            super::enderman::set_target(me, mob, None);
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
                super::enderman::set_target(me, mob, None);
            }
            // `PolarBearMeleeAttackGoal.stop`：先放下身子。
            // `PolarBearMeleeAttackGoal.stop`: drop down first.
            mob.polar_bear.standing = false;
            mob.aggressive = false;
            mob.navigation.stop();
        }
        Goal::FleeSun { .. } | Goal::SquidRandomMovement | Goal::SquidFlee { .. } => {}
        Goal::BreakDoor => {
            mob.door_break_time = 0;
            mob.door_break_progress = -1;
        }
        Goal::EatBlock { eat_tick } => *eat_tick = 0,
        Goal::TurtleEgg { .. } => {
            // `RemoveBlockGoal.stop`：落地前的摔落距离按 1 格算。
            me.fall_distance = 1.0;
        }
        Goal::PandaSit => super::panda::sit_stop(me, mob, ctx),
        Goal::PandaLieOnBack => super::panda::lie_stop(me, mob),
        Goal::PandaSneeze | Goal::PandaRoll => {}
        Goal::AvoidEntity { avoid, path, .. } => {
            *avoid = None;
            *path = None;
        }
    }
}

fn tick_goal(goal: &mut Goal, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    match goal {
        Goal::EndermanGaze(goal) => goal.tick(me, mob, ctx),
        Goal::EndermanFreeze => {
            if let Some(player) = mob.target.and_then(|uid| player_by_uid(ctx.blocks(), uid)) {
                let eye = player.eye_position();
                mob.look.set_look_at_default(eye.x, eye.y, eye.z);
            }
        }
        Goal::EndermanBlock { place } => super::enderman::sample_block(me, mob, *place),
        Goal::EatBlock { eat_tick } => eat_block_tick(eat_tick, me, mob, ctx),
        Goal::CubeFloat => {
            if me.random.next_float() < 0.8 {
                mob.jump.jump = true;
            }
            super::slime::set_wanted_movement(mob, 1.2);
        }
        Goal::CubeAttack { .. } => {
            let target = combat_target(mob, ctx);
            super::slime::attack_tick(me, mob, target);
        }
        Goal::CubeRandomDirection {
            chosen_degrees,
            next_randomize_time,
        } => {
            *next_randomize_time -= 1;
            if *next_randomize_time <= 0 {
                *next_randomize_time =
                    adjusted_tick_delay(false, 40 + me.random.next_int_bounded(60));
                *chosen_degrees = me.random.next_int_bounded(360) as f32;
            }
            mob.cube.move_y_rot = *chosen_degrees;
            mob.cube.aggressive = false;
        }
        Goal::CubeKeepJumping => super::slime::set_wanted_movement(mob, 1.0),
        // `DrownedSwimUpGoal.tick`：离终点够近或没有路径时，朝海平面方向重新找点。
        Goal::DrownedSwimUp { speed, stuck } => {
            let sea = f64::from(ctx.blocks().sea_level());
            if me.position.y < sea - 1.0
                && (mob.navigation.is_done() || super::drowned::close_to_next_pos(me, mob))
            {
                let towards = Vec3::new(me.position.x, sea - 1.0, me.position.z);
                let Some(next) = pos_towards(
                    me,
                    mob,
                    ctx.blocks(),
                    4,
                    8,
                    towards,
                    std::f64::consts::FRAC_PI_2,
                ) else {
                    *stuck = true;
                    return;
                };
                let nav = living::nav_mob(me, mob, ctx.blocks());
                mob.navigation.move_to(ctx.blocks(), &nav, next, *speed);
            }
        }
        // `MoveToBlockGoal.tick`：走到目标块上方，够不到就计一次尝试，每 40 次重算路径。
        //
        // `MoveToBlockGoal.tick`: step onto the block above the target; count a try when it
        // is out of reach and recompute the path every forty.
        Goal::DrownedGoToBeach {
            speed,
            try_ticks,
            block_pos,
            reached_target,
            ..
        } => {
            let Some(pos) = *block_pos else {
                return;
            };
            let above = BlockPos::new(pos.x, pos.y + 1, pos.z);
            let dx = f64::from(above.x) + 0.5 - me.position.x;
            let dy = f64::from(above.y) + 0.5 - me.position.y;
            let dz = f64::from(above.z) + 0.5 - me.position.z;
            if dx * dx + dy * dy + dz * dz >= 1.0 {
                *reached_target = false;
                *try_ticks += 1;
                if *try_ticks % 40 == 0 {
                    let nav = living::nav_mob(me, mob, ctx.blocks());
                    mob.navigation.move_to(
                        ctx.blocks(),
                        &nav,
                        Vec3::new(
                            f64::from(above.x) + 0.5,
                            f64::from(above.y),
                            f64::from(above.z) + 0.5,
                        ),
                        *speed,
                    );
                }
            } else {
                *reached_target = true;
                *try_ticks -= 1;
            }
        }
        // `RangedAttackGoal.tick`：视线、距离、导航、注视，再按 60 刻的间隔攻击。
        //
        // `RangedAttackGoal.tick`: line of sight, distance, navigation, look, then attack every 60 ticks.
        Goal::WitchRangedAttack {
            target,
            attack_time,
            see_time,
        } => {
            let Some((target, _)) = target.and_then(|uid| target_view(ctx, uid)) else {
                return;
            };
            let dist_sqr = me.position.distance_to_sqr(target.position);
            let has_los =
                living::has_line_of_sight(me, mob, ctx.blocks(), target.uid, target.eye_position());
            if has_los {
                *see_time += 1;
            } else {
                *see_time = 0;
            }
            let radius = 10.0_f64;
            if dist_sqr <= radius * radius && *see_time >= 5 {
                mob.navigation.stop();
            } else {
                let nav = living::nav_mob(me, mob, ctx.blocks());
                mob.navigation
                    .move_to_entity(ctx.blocks(), &nav, target.position, 1.0, 1);
            }
            let eye = target.eye_position();
            mob.look
                .set_look_at(target.position.x, eye.y, target.position.z, 30.0, 30.0);
            *attack_time -= 1;
            if *attack_time == 0 {
                if !has_los {
                    return;
                }
                let dist = (dist_sqr.sqrt() / radius) as f32;
                let power = mth::clamp_f32(dist, 0.1, 1.0);
                super::witch::perform_ranged_attack(me, mob, ctx, target, power);
                // `attackIntervalMin == attackIntervalMax == 60`。
                // `attackIntervalMin == attackIntervalMax == 60`.
                *attack_time = 60;
            } else if *attack_time < 0 {
                // `Mth.floor(Mth.lerp(d, 60, 60))` 恒等于 60。
                // `Mth.floor(Mth.lerp(d, 60, 60))` is always 60.
                *attack_time = 60;
            }
        }
        Goal::Swell { target } => swell_tick(*target, me, mob, ctx),
        Goal::SquidRandomMovement => super::squid::random_movement_tick(me, mob),
        Goal::SquidFlee { flee_ticks } => super::squid::flee_tick(flee_ticks, me, mob, ctx),
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
        // 没有每刻逻辑的目标（`tick()` 是空方法）。
        Goal::DrownedGoToWater { .. }
        | Goal::RandomStroll { .. }
        | Goal::WitchHealRaiders { .. } => {}
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
                max_head_x_rot(mob),
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
                max_head_x_rot(mob),
            );
            if me.position.distance_to_sqr(player.position) < 2.5 * 2.5 {
                mob.navigation.stop();
            } else {
                let nav = living::nav_mob(me, mob, ctx.blocks());
                mob.navigation
                    .move_to_entity(ctx.blocks(), &nav, player.position, *speed, 1);
            }
        }
        Goal::OcelotAttack {
            target,
            attack_time,
        } => super::ocelot::attack_tick(*target, attack_time, me, mob, ctx),
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
            mob.look.set_look_at(
                player.position.x,
                player.eye_y(),
                player.position.z,
                super::control::HEAD_ROT_SPEED as f32,
                max_head_x_rot(mob),
            );
            *look_time -= 1;
        }
        Goal::RandomLookAround {
            rel_x,
            rel_z,
            look_time,
        } => {
            *look_time -= 1;
            let eye = me.position.y + f64::from(mob.eye_height);
            mob.look.set_look_at(
                me.position.x + *rel_x,
                eye,
                me.position.z + *rel_z,
                super::control::HEAD_ROT_SPEED as f32,
                max_head_x_rot(mob),
            );
        }
        Goal::Panic { speed, .. } if mob.mob_type == super::MobType::Rabbit => {
            super::rabbit::set_speed_modifier(me, mob, *speed);
        }
        Goal::RabbitRaid(raid) => raid.tick(me, mob, ctx),
        Goal::RabbitPowderSnow => mob.jump.jump = true,
        Goal::Avoid(avoid) => avoid.tick(me, mob, ctx),
        Goal::Fox(goal) => goal.tick(me, mob, ctx),
        Goal::Tame(goal) => goal.tick(me, mob, ctx),
        Goal::Wolf(goal) => goal.tick(me, mob, ctx),
        Goal::Cat(goal) => goal.tick(me, mob, ctx),
        Goal::Parrot(goal) => goal.tick(me, mob, ctx),
        // `TamableAnimalPanicGoal.tick`：能走向主人且离得远时先瞬移过去。
        // `TamableAnimalPanicGoal.tick` teleports to a distant owner first.
        Goal::Panic { .. } if super::tame::is_tamable(mob.mob_type) => {
            let view = ctx.blocks();
            if !super::tame::unable_to_move_to_owner(mob, view)
                && super::tame::should_try_teleport_to_owner(me, mob, view)
            {
                super::tame::try_to_teleport_to_owner(me, mob, view);
            }
        }
        Goal::Panic { .. } | Goal::Stroll { .. } => {}
        Goal::RunAroundLikeCrazy { .. } => super::equine::tame::tick(me, mob, ctx),
        Goal::Swim { .. } | Goal::PufferfishPuff => {}
        Goal::FollowFlock { time_to_recalc, .. } => {
            super::fish::flock_tick(time_to_recalc, me, mob, ctx)
        }
        Goal::Unavailable(_)
        | Goal::HurtByTarget { .. }
        | Goal::LlamaHurtByTarget { .. }
        | Goal::LeapAtTarget { .. }
        | Goal::RestrictSun
        | Goal::RandomStand { .. }
        // `TargetGoal` 没有 `tick`：`unseenTicks` 只在 `canContinueToUse` 里按视线刷新。
        // `TargetGoal` has no `tick`: `unseenTicks` is only refreshed by line of sight in
        // `canContinueToUse`, so ticking the target goal here is a no-op.
        | Goal::NearestAttackableTarget { .. } => {}
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
        Goal::MeleeAttack {
            speed,
            ticks_until_next_attack,
            ticks_until_path_recalc,
            pathed_target,
            raise_arm_ticks,
            track_target,
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
            // `followingTargetEvenIfNotSeen` 为假（僵尸、骷髅、苦力怕都是）时，
            // 最前面还有一条「看得见目标」：看不见就不重算路径，也不取随机数。
            // With `followingTargetEvenIfNotSeen` false (zombies, skeletons, creepers) a
            // line-of-sight check comes first: unseen targets cause no repath and no draws.
            let visible = *track_target
                || living::has_line_of_sight(
                    me,
                    mob,
                    ctx.blocks(),
                    target.uid,
                    target.eye_position(),
                );
            if visible
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
                // `FoxMeleeAttackGoal.checkAndPerformAttack`：不挥手，咬一口出声。
                // `FoxMeleeAttackGoal.checkAndPerformAttack`: no swing, a bite sound.
                if mob.mob_type == super::MobType::Fox {
                    living::play_sound(me, mob, "minecraft:entity.fox.bite", 1.0, 1.0);
                }
            }
            // `ZombieAttackGoal.tick`：`super.tick()` 之后抬手计时加一，按新的
            // `ticksUntilNextAttack` 决定举不举手。
            if matches!(
                mob.mob_type,
                super::MobType::Zombie | super::MobType::Husk | super::MobType::Drowned
            ) {
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
        Goal::PandaSit => super::panda::sit_tick(me, mob),
        Goal::AvoidEntity {
            avoid,
            walk_speed,
            sprint_speed,
            ..
        } => {
            let Some(target) = *avoid else {
                return;
            };
            let close = ctx
                .other(target)
                .is_some_and(|other| me.position.distance_to_sqr(other.position) < 49.0);
            mob.navigation.speed_modifier = if close { *sprint_speed } else { *walk_speed };
        }
        Goal::PandaLieOnBack | Goal::PandaSneeze | Goal::PandaRoll => {}
        Goal::TurtleLayEgg { speed } => super::turtle::lay_egg_tick(me, mob, ctx, *speed),
        Goal::TurtleGoHome { speed } => super::turtle::go_home_tick(me, mob, ctx, *speed),
        Goal::TurtleGoToWater { speed } => super::turtle::go_to_water_tick(me, mob, ctx, *speed),
        Goal::TurtleTravel { speed, stuck } => {
            super::turtle::travel_tick(me, mob, ctx, *speed, stuck);
        }
    }
}

// ---- 目标用到的查询 ----

/// `FloatGoal.canUse`。
pub(super) fn float_can_use(me: &Entity, mob: &Mob) -> bool {
    living::fluid_height(mob, fluid_tags::ENTITY_FLOATABLE) > living::fluid_jump_threshold(mob)
        || living::is_in_lava(me, mob)
}

/// `PanicGoal.shouldPanic`：最近 40 刻的伤害在 `#panic_causes` 里。
fn should_panic(mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    // 成年北极熊只怕环境伤害（`PANIC_ENVIRONMENTAL_CAUSES`），幼崽和别的动物一样；
    // 狼的 `TamableAnimalPanicGoal` 也只认环境伤害。
    // Adult polar bears panic from environmental damage only; cubs panic like any animal.
    // The wolf's `TamableAnimalPanicGoal` only reacts to `#panic_environmental_causes`.
    let environmental = (mob.mob_type == super::MobType::PolarBear && !mob.is_baby())
        || mob.mob_type == super::MobType::Wolf;
    living::last_damage_source(mob, ctx.blocks().game_time()).is_some_and(|source| {
        if environmental {
            damage_type_tagged("minecraft:panic_environmental_causes", source.kind)
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

/// 一种伤害在不在某个 `minecraft:damage_type` 标签里。
pub(super) fn damage_type_tagged(tag: &str, kind: super::DamageKind) -> bool {
    let Some(id) = mc_bamboo_data::damage_type_id(kind.name()) else {
        return false;
    };
    mc_bamboo_data::tags::TAG_GROUPS
        .iter()
        .filter(|group| group.registry == "minecraft:damage_type")
        .flat_map(|group| group.tags.iter())
        .any(|entry| entry.name == tag && entry.ids.contains(&id))
}

/// `PanicGoal.lookForWater`：脚下没有碰撞时，按曼哈顿距离找最近的水。
pub(super) fn look_for_water(
    view: &dyn BlockView,
    centre: BlockPos,
    range: i32,
) -> Option<BlockPos> {
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
pub(super) fn targeting_test(
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
        // `notRiding(mob)`：忽略骑在自己身上的玩家。
        // `notRiding(mob)`: ignore players riding this mob.
        if mob.passengers.contains(&player.uid)
            || !targeting_test(me, mob, view, &player, range, true)
        {
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
pub(super) fn nearest_tempting_player(
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
            } else if mob.mob_type == super::MobType::Camel {
                // 骆驼是马科，但 `TemptingSensor` 用 `Camel.isFood`（`#camel_food`），
                // 不是马科的 `TemptGoal` 那张表。
                // The camel is equine, but `TemptingSensor` uses `Camel.isFood`
                // (`#camel_food`), not the equine `TemptGoal` table.
                super::interact::is_food(mob.mob_type, item)
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
/// `BreedGoal.getFreePartner`: the nearest in-love, non-panicking partner within
/// 8 blocks. Equines use `canMate`, where both must `canParent` and horses pair
/// with donkeys.
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
            || !super::wolf::can_mate(mob, public)
            || !super::cat::can_mate(mob, public)
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
pub(super) fn within_home(mob: &Mob, pos: BlockPos) -> bool {
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

/// `RandomPos.generateRandomDirectionWithinRadians`。
///
/// `RandomPos.generateRandomDirectionWithinRadians`.
fn random_direction_within_radians(
    me: &mut Entity,
    horizontal: (f64, f64),
    vertical: i32,
    flying_height: i32,
    x_dir: f64,
    z_dir: f64,
    max_radians: f64,
) -> Option<BlockPos> {
    let centre = mth::atan2(z_dir, x_dir) as f64 - f64::from(std::f32::consts::FRAC_PI_2);
    // 扇形的偏移在 f32 里算完再提升成 f64，和官方一致。
    // The fan offset is computed in f32 and then widened to f64, as vanilla does.
    let spread = f64::from(2.0_f32 * me.random.next_float() - 1.0);
    let radians = centre + spread * max_radians;
    let dist = mth::lerp(me.random.next_double().sqrt(), horizontal.0, horizontal.1)
        * f64::from(std::f32::consts::SQRT_2);
    let x = -dist * radians.sin();
    let z = dist * radians.cos();
    if x.abs() > horizontal.1 || z.abs() > horizontal.1 {
        return None;
    }
    let y = me.random.next_int_bounded(2 * vertical + 1) - vertical + flying_height;
    Some(BlockPos::containing(Vec3::new(x, f64::from(y), z)))
}

/// `DefaultRandomPos.getPosAway`。
///
/// `DefaultRandomPos.getPosAway`.
pub(super) fn away_random_pos(
    me: &mut Entity,
    mob: &Mob,
    view: &dyn BlockView,
    horizontal: i32,
    vertical: i32,
    avoid: Vec3,
) -> Option<Vec3> {
    let dir_away = me.position.subtract(avoid);
    let restricted = mob_restricted(me, mob, f64::from(horizontal));
    pick_best(mob, view, || {
        let direction = random_direction_within_radians(
            me,
            (0.0, f64::from(horizontal)),
            vertical,
            0,
            dir_away.x,
            dir_away.z,
            std::f64::consts::FRAC_PI_2,
        )?;
        let pos = toward_direction(me, mob, f64::from(horizontal), direction);
        // `GoalUtils.isNotStable(mob.getNavigation(), pos)`：问的是这只生物自己的导航，
        // 游泳的导航认的是「不是实心方块」。
        // `GoalUtils.isNotStable(mob.getNavigation(), pos)` asks the mob's own navigation;
        // a swimming one accepts anything that is not a solid block.
        if outside_limits(view, pos)
            || (restricted && !within_home(mob, pos))
            || !stable_destination(mob, view, pos)
            || has_malus(mob, view, pos)
        {
            return None;
        }
        Some(pos)
    })
}

/// `AvoidEntityGoal.canUse` 里的 `getNearestEntity`。
///
/// The `getNearestEntity` of `AvoidEntityGoal.canUse`.
fn nearest_avoidable(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    target: AvoidTarget,
    max_dist: f32,
) -> Option<(EntityUid, Vec3)> {
    let area = me
        .bounding_box()
        .inflate(f64::from(max_dist), 3.0, f64::from(max_dist));
    let mut best = f64::MAX;
    let mut found = None;
    match target {
        AvoidTarget::Player => {
            let view = ctx.blocks();
            view.for_each_player(&mut |player| {
                if player.spectator
                    || !player.alive
                    || me.position.distance_to_sqr(player.position) > f64::from(max_dist * max_dist)
                {
                    return;
                }
                if !living::has_line_of_sight(me, mob, view, player.uid, player.eye_position()) {
                    return;
                }
                let distance = me.position.distance_to_sqr(player.position);
                if distance < best {
                    best = distance;
                    found = Some((player.uid, player.position));
                }
            });
        }
        AvoidTarget::Monster => {
            ctx.for_each_other_in(area, &mut |other| {
                let KindPublic::Mob(public) = &other.kind else {
                    return;
                };
                if !public.mob_type.is_monster() || !public.is_alive() || other.removed {
                    return;
                }
                let distance = me.position.distance_to_sqr(other.position);
                if distance > f64::from(max_dist * max_dist) || distance >= best {
                    return;
                }
                best = distance;
                found = Some((other.uid, other.position));
            });
        }
    }
    found
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
    mob.pathfinding_malus(static_path_type(view, pos)) != 0.0
}

/// `Animal.getWalkTargetValue`：脚下是草方块给 10，否则按亮度；
/// `Monster.getWalkTargetValue` 取负的光照路径代价。
pub(super) fn walk_target_value(mob: &Mob, view: &dyn BlockView, pos: BlockPos) -> f32 {
    // 史莱姆不是 `PathfinderMob`，`Mob.checkSpawnRules` 恒真。
    // Slimes are not `PathfinderMob`s; `Mob.checkSpawnRules` is always true.
    if matches!(
        mob.mob_type,
        super::MobType::Slime | super::MobType::Enderman
    ) {
        return 0.0;
    }
    if mob.mob_type == super::MobType::Turtle {
        return super::turtle::walk_target_value(mob, view, pos);
    }
    if mob.mob_type.is_monster() {
        return monster_walk_target_value(view, pos);
    }
    if mob.mob_type == super::MobType::Mooshroom {
        return super::mooshroom::walk_target_value(view, pos);
    }
    // 不是 `Animal` 的生物用 `PathfinderMob.getWalkTargetValue` 的缺省值 0。
    // Mobs that are not `Animal`s keep the `PathfinderMob.getWalkTargetValue` default of 0.
    if mob.mob_type.is_ageable_water_creature() || mob.mob_type.is_water_animal() {
        return 0.0;
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
            || !stable_destination(mob, view, pos)
            || has_malus(mob, view, pos)
        {
            return None;
        }
        Some(pos)
    })
}

/// `DefaultRandomPos.getPosTowards`：和 `getPos` 一样取 10 次，只是方向限制在朝目标
/// `max_radians` 的扇形里。
///
/// `DefaultRandomPos.getPosTowards`: the same ten rolls as `getPos`, with the direction
/// restricted to a fan of `max_radians` around the target.
pub(crate) fn pos_towards(
    me: &mut Entity,
    mob: &Mob,
    view: &dyn BlockView,
    horizontal: i32,
    vertical: i32,
    towards: Vec3,
    max_radians: f64,
) -> Option<Vec3> {
    let restricted = mob_restricted(me, mob, f64::from(horizontal));
    let dir_x = towards.x - me.position.x;
    let dir_z = towards.z - me.position.z;
    pick_best(mob, view, || {
        let direction = random_direction_within_radians_flat(
            me,
            f64::from(horizontal),
            vertical,
            dir_x,
            dir_z,
            max_radians,
        )?;
        let pos = toward_direction(me, mob, f64::from(horizontal), direction);
        if outside_limits(view, pos)
            || (restricted && !within_home(mob, pos))
            || !stable_destination(mob, view, pos)
            || has_malus(mob, view, pos)
        {
            return None;
        }
        Some(pos)
    })
}

/// `DefaultRandomPos.getPosAway`：在背离 `avoid` 的 ±90° 扇形里取点。
///
/// `DefaultRandomPos.getPosAway`: sample within ±90° of the direction away
/// from `avoid`.
pub(crate) fn default_random_pos_away(
    me: &mut Entity,
    mob: &Mob,
    view: &dyn BlockView,
    horizontal: i32,
    vertical: i32,
    avoid: Vec3,
) -> Option<Vec3> {
    let away_x = me.position.x - avoid.x;
    let away_z = me.position.z - avoid.z;
    let restricted = mob_restricted(me, mob, f64::from(horizontal));
    pick_best(mob, view, || {
        let direction = random_direction_within_radians(
            me,
            (0.0, f64::from(horizontal)),
            vertical,
            0,
            away_x,
            away_z,
            f64::from(std::f32::consts::FRAC_PI_2),
        )?;
        let pos = toward_direction(me, mob, f64::from(horizontal), direction);
        // `GoalUtils.isNotStable(mob.getNavigation(), pos)`：问的是这只生物自己的导航，
        // 游泳的导航认的是「不是实心方块」。
        // `GoalUtils.isNotStable(mob.getNavigation(), pos)` asks the mob's own navigation;
        // a swimming one accepts anything that is not a solid block.
        if outside_limits(view, pos)
            || (restricted && !within_home(mob, pos))
            || !stable_destination(mob, view, pos)
            || has_malus(mob, view, pos)
        {
            return None;
        }
        Some(pos)
    })
}

/// 导航的 `isStableDestination`；海龟有远行目标时（`TurtlePathNavigation`）只认水格。
///
/// The navigation's `isStableDestination`; a turtle with a travel target
/// (`TurtlePathNavigation`) only accepts water cells.
fn stable_destination(mob: &Mob, view: &dyn BlockView, pos: BlockPos) -> bool {
    if super::turtle::is_turtle(mob.mob_type) && mob.turtle.travel_pos.is_some() {
        return view.block_state(pos).is(blocks::WATER);
    }
    mob.navigation.stable_destination(view, pos)
}

/// `DefaultRandomPos.getPosTowards`：在朝向 `towards` 的 ±`angle` 扇形里取点。
///
/// `DefaultRandomPos.getPosTowards`: sample within ±`angle` of the direction towards
/// `towards`.
pub(crate) fn default_random_pos_towards(
    me: &mut Entity,
    mob: &Mob,
    view: &dyn BlockView,
    horizontal: i32,
    vertical: i32,
    towards: Vec3,
    angle: f32,
) -> Option<Vec3> {
    let dir_x = towards.x - me.position.x;
    let dir_z = towards.z - me.position.z;
    let restricted = mob_restricted(me, mob, f64::from(horizontal));
    pick_best(mob, view, || {
        let direction = random_direction_within_radians_flat(
            me,
            f64::from(horizontal),
            vertical,
            dir_x,
            dir_z,
            f64::from(angle),
        )?;
        let pos = toward_direction(me, mob, f64::from(horizontal), direction);
        if outside_limits(view, pos)
            || (restricted && !within_home(mob, pos))
            || !stable_destination(mob, view, pos)
            || has_malus(mob, view, pos)
        {
            return None;
        }
        Some(pos)
    })
}

/// `RandomPos.generateRandomDirectionWithinRadians`，最小水平距离 0、飞行高度 0。
///
/// `RandomPos.generateRandomDirectionWithinRadians` with a minimum horizontal
/// distance of 0 and a flying height of 0.
fn random_direction_within_radians_flat(
    me: &mut Entity,
    max_horizontal: f64,
    vertical: i32,
    dir_x: f64,
    dir_z: f64,
    max_radians: f64,
) -> Option<BlockPos> {
    // 官方在 f32 里算扇形偏移和 π/2，再提升成 f64；SQRT_OF_TWO 是双精度常量。
    // Vanilla computes the fan offset and π/2 in f32, then widens; SQRT_OF_TWO is a double.
    let centre = mth::atan2(dir_z, dir_x) - f64::from(std::f32::consts::FRAC_PI_2);
    let spread = f64::from(2.0_f32 * me.random.next_float() - 1.0);
    let yaw = centre + spread * max_radians;
    let distance = mth::lerp(me.random.next_double().sqrt(), 0.0, max_horizontal)
        * f64::from(std::f32::consts::SQRT_2);
    let x = -distance * yaw.sin();
    let z = distance * yaw.cos();
    if x.abs() > max_horizontal || z.abs() > max_horizontal {
        return None;
    }
    let y = me.random.next_int_bounded(2 * vertical + 1) - vertical;
    Some(BlockPos::containing(Vec3::new(x, f64::from(y), z)))
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
            || !stable_destination(mob, view, pos)
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

/// `HoverRandomPos.getPos`：朝视线方向找一个能悬停的高度。
///
/// `HoverRandomPos.getPos`: hover a few blocks above the ground towards the facing.
#[expect(clippy::too_many_arguments, reason = "官方方法就是这几个参数")]
pub(crate) fn hover_random_pos(
    me: &mut Entity,
    mob: &Mob,
    view: &dyn BlockView,
    horizontal: i32,
    vertical: i32,
    x_dir: f64,
    z_dir: f64,
    max_radians: f64,
    hover_max_height: i32,
    hover_min_height: i32,
) -> Option<Vec3> {
    let restricted = mob_restricted(me, mob, f64::from(horizontal));
    pick_best(mob, view, || {
        let direction = random_direction_within_radians(
            me,
            (0.0, f64::from(horizontal)),
            vertical,
            0,
            x_dir,
            z_dir,
            max_radians,
        )?;
        let pos =
            land_toward_direction(me, mob, view, f64::from(horizontal), restricted, direction)?;
        let above = me
            .random
            .next_int_bounded(hover_max_height - hover_min_height + 1)
            + hover_min_height;
        let pos = move_up_to_above_solid(view, pos, above);
        if view.fluid_state(pos).is_tagged(fluid_tags::WATER) || has_malus(mob, view, pos) {
            return None;
        }
        Some(pos)
    })
}

/// `AirAndWaterRandomPos.getPos`：朝视线方向找空中或水里的落点。
///
/// `AirAndWaterRandomPos.getPos`: an air or water spot towards the facing.
#[expect(clippy::too_many_arguments, reason = "官方方法就是这几个参数")]
pub(crate) fn air_and_water_random_pos(
    me: &mut Entity,
    mob: &Mob,
    view: &dyn BlockView,
    horizontal: i32,
    vertical: i32,
    flying_height: i32,
    x_dir: f64,
    z_dir: f64,
    max_radians: f64,
) -> Option<Vec3> {
    let restricted = mob_restricted(me, mob, f64::from(horizontal));
    pick_best(mob, view, || {
        let direction = random_direction_within_radians(
            me,
            (0.0, f64::from(horizontal)),
            vertical,
            flying_height,
            x_dir,
            z_dir,
            max_radians,
        )?;
        let pos = toward_direction(me, mob, f64::from(horizontal), direction);
        if outside_limits(view, pos) || (restricted && !within_home(mob, pos)) {
            return None;
        }
        let pos = move_up_out_of_solid(view, pos);
        if has_malus(mob, view, pos) {
            return None;
        }
        Some(pos)
    })
}

/// `LandRandomPos.generateRandomPosTowardDirection`。
fn land_toward_direction(
    me: &mut Entity,
    mob: &Mob,
    view: &dyn BlockView,
    max_distance: f64,
    restricted: bool,
    direction: BlockPos,
) -> Option<BlockPos> {
    let pos = toward_direction(me, mob, max_distance, direction);
    if outside_limits(view, pos)
        || (restricted && !within_home(mob, pos))
        // `GoalUtils.isNotStable(mob.getNavigation(), pos)`：官方的第三个判定。
        // `GoalUtils.isNotStable(mob.getNavigation(), pos)`, vanilla's third check.
        || !stable_destination(mob, view, pos)
    {
        return None;
    }
    Some(pos)
}

/// `RandomPos.moveUpOutOfSolid`。
fn move_up_out_of_solid(view: &dyn BlockView, pos: BlockPos) -> BlockPos {
    if !view.block_state(pos).is_solid() {
        return pos;
    }
    let mut moved = pos.above();
    while moved.y <= view.max_y() && view.block_state(moved).is_solid() {
        moved = moved.above();
    }
    moved
}

/// `RandomPos.moveUpToAboveSolid`。
fn move_up_to_above_solid(view: &dyn BlockView, pos: BlockPos, above_solid: i32) -> BlockPos {
    if !view.block_state(pos).is_solid() {
        return pos;
    }
    let mut moved = pos.above();
    while moved.y <= view.max_y() && view.block_state(moved).is_solid() {
        moved = moved.above();
    }
    let first_non_solid = moved.y;
    while moved.y <= view.max_y() && moved.y - first_non_solid < above_solid {
        moved = moved.above();
        if view.block_state(moved).is_solid() {
            moved = moved.below();
            break;
        }
    }
    moved
}

// ---- 敌对目标的查询 ----

/// 当前岛上可攻击的生物快照；跨岛目标读作缺失。
///
/// A local attack target snapshot. Unavailable cross-island targets are absent.
#[derive(Clone, Copy)]
pub(crate) struct CombatTarget {
    pub(crate) uid: EntityUid,
    pub(crate) position: Vec3,
    pub(crate) delta: Vec3,
    eye: f32,
    pub(crate) height: f64,
    pub(crate) health: f32,
    pub(crate) effects: crate::entity::ctx::TargetEffects,
    pub(crate) is_raider: bool,
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
            delta: view.delta,
            eye: view.eye_height,
            height,
            health: view.health,
            effects: view.effects,
            is_raider: false,
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

    pub(crate) fn bounds(&self) -> Aabb {
        self.hitbox
    }

    pub(crate) fn eye_y(&self) -> f64 {
        self.position.y + f64::from(self.eye)
    }

    /// `getHitbox()`。
    pub(crate) const fn hitbox(&self) -> Aabb {
        self.hitbox
    }

    pub(crate) fn eye_position(&self) -> Vec3 {
        Vec3::new(self.position.x, self.eye_y(), self.position.z)
    }
}

pub(super) fn combat_target(mob: &Mob, ctx: &dyn EntityCtx) -> Option<CombatTarget> {
    target_by_uid(mob, ctx, mob.target?)
}

fn target_in_follow_range(me: &Entity, mob: &Mob, target: CombatTarget) -> bool {
    target_in_scaled_follow_range(me, mob, target, 1.0)
}

fn target_in_scaled_follow_range(me: &Entity, mob: &Mob, target: CombatTarget, scale: f64) -> bool {
    let range = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE)
        * scale;
    me.position.distance_to_sqr(target.position) <= range * range
}

/// `TargetGoal.canContinueToUse` 不看视线的部分：目标还能打、在跟随距离内。
///
/// `TargetGoal.canContinueToUse` without the sight memory.
pub(super) fn target_goal_can_continue(me: &Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    combat_target(mob, ctx).is_some_and(|target| target_in_follow_range(me, mob, target))
}

/// `Mob.getMaxHeadXRot`：坐着的狼是 20，其余 40。
///
/// `Mob.getMaxHeadXRot`: 20 for a sitting wolf, otherwise 40.
pub(super) fn max_head_x_rot(mob: &Mob) -> f32 {
    if mob.mob_type == super::MobType::Wolf {
        super::wolf::max_head_x_rot(mob)
    } else {
        super::control::MAX_HEAD_X_ROT as f32
    }
}

fn target_by_uid(mob: &Mob, ctx: &dyn EntityCtx, uid: EntityUid) -> Option<CombatTarget> {
    // 骷髅、苦力怕、蜘蛛、末影人、北极熊、狐狸、狼、羊驼、女巫和豹猫会把别的生物当目标。
    // Skeletons, creepers, spiders, endermen, polar bears, foxes, wolves, llamas,
    // witches and ocelots can target other mobs.
    let mobs_allowed = mob.mob_type.full_target_rules()
        || matches!(
            mob.mob_type,
            super::MobType::PolarBear
                | super::MobType::Fox
                | super::MobType::Wolf
                | super::MobType::Llama
                | super::MobType::Witch
                | super::MobType::Ocelot
                | super::MobType::Cat
        );
    if !mobs_allowed && player_by_uid(ctx.blocks(), uid).is_none() {
        return None;
    }
    attack_candidate(mob, ctx, uid)
}

/// `Mob.getTarget` / `TargetingConditions` 共用的「能不能打」：活着、不是旁观或创造玩家、
/// 和平难度不打玩家，可驯服动物不打主人。
///
/// The shared "can attack" test: alive, not a spectator or creative player,
/// no players on peaceful, and tamed animals spare their owner.
pub(super) fn attack_candidate(
    mob: &Mob,
    ctx: &dyn EntityCtx,
    uid: EntityUid,
) -> Option<CombatTarget> {
    // 骷髅、北极熊、羊驼和用完整目标规则的敌对生物按精确的玩家碰撞箱算，其余用一个近似的。
    // Skeletons, polar bears, llamas and the hostile mobs with full target rules use
    // the exact player box; other mobs use an approximation.
    let exact_player_box = mob.mob_type.full_target_rules()
        || matches!(
            mob.mob_type,
            super::MobType::PolarBear | super::MobType::Llama
        );
    if let Some(player) = player_by_uid(ctx.blocks(), uid) {
        // `LivingEntity.canAttack`：和平难度只排除玩家目标。
        // `LivingEntity.canAttack`: peaceful only rules out player targets.
        return (ctx.blocks().difficulty() != 0
            && player.alive
            && !player.spectator
            && !player.creative
            && !super::tame::refuses_target(mob, player.uuid))
        .then(|| CombatTarget::player(player, exact_player_box));
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
        delta: other.delta,
        eye,
        height: f64::from(target.dimensions().1),
        health: target.health,
        effects: target.effects,
        is_raider: target.mob_type == super::MobType::Witch,
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
/// 当前目标的快照；没有目标或目标不在本岛时为 `None`。
///
/// The current target's snapshot; `None` without a target or when it is on another island.
pub(crate) fn current_combat_target(mob: &Mob, ctx: &dyn EntityCtx) -> Option<CombatTarget> {
    combat_target(mob, ctx)
}

/// 当前目标是不是在水里；没有目标时为假。
///
/// Whether the current target is in water; false without a target.
pub(crate) fn current_target_in_water(mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    combat_target(mob, ctx).is_some_and(|target| combat_target_in_water(ctx, &target))
}

/// 目标是不是在水里。生物读 `MobPublic.in_water`（上一刻的流体跟踪），玩家没有这个字段，
/// 按脚下那一格是不是水判断。
///
/// Whether the target is in water. Mobs read `MobPublic.in_water` (last tick's fluid
/// tracking); players have no such field, so the block under their feet decides.
fn combat_target_in_water(ctx: &dyn EntityCtx, target: &CombatTarget) -> bool {
    let view = ctx.blocks();
    if let Some(other) = ctx.other(target.uid) {
        return match &other.kind {
            KindPublic::Mob(mob) => mob.in_water,
            _ => view
                .fluid_state(BlockPos::containing(other.position))
                .is_tagged(fluid_tags::WATER),
        };
    }
    let mut found = false;
    view.for_each_player(&mut |player| {
        if player.uid == target.uid {
            found = view
                .fluid_state(BlockPos::containing(player.position))
                .is_tagged(fluid_tags::WATER);
        }
    });
    found
}

/// `Drowned.DrownedGoToWaterGoal.getWaterPos`：10 次随机偏移，找水方块。
///
/// `Drowned.DrownedGoToWaterGoal.getWaterPos`: ten random offsets, looking for water.
fn water_pos(me: &mut Entity, view: &dyn BlockView) -> Option<Vec3> {
    let base = me.block_position();
    for _ in 0..10 {
        let pos = BlockPos::new(
            base.x + me.random.next_int_bounded(20) - 10,
            base.y + 2 - me.random.next_int_bounded(8),
            base.z + me.random.next_int_bounded(20) - 10,
        );
        if view.block_state(pos).is(mc_bamboo_data::blocks::WATER) {
            return Some(Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y),
                f64::from(pos.z) + 0.5,
            ));
        }
    }
    None
}

/// `MoveToBlockGoal.findNearestBlock` 的扫描顺序：纵向 0、1、-1、2、-2，再按半径
/// 0..R 和方环。`isWithinHome` 对没有家的生物恒真。
///
/// The `MoveToBlockGoal.findNearestBlock` scan order: vertical 0, 1, -1, 2, -2, then
/// radius 0..R around a square ring. `isWithinHome` is always true without a home.
fn find_beach_target(
    me: &Entity,
    _mob: &Mob,
    ctx: &dyn EntityCtx,
    search_range: i32,
    vertical_range: i32,
) -> Option<BlockPos> {
    let view = ctx.blocks();
    let mob_pos = me.block_position();
    let mut y = 0;
    while y <= vertical_range {
        for r in 0..search_range {
            let mut x = 0;
            while x <= r {
                let mut z = if x < r && x > -r { r } else { 0 };
                while z <= r {
                    let pos = BlockPos::new(mob_pos.x + x, mob_pos.y + y - 1, mob_pos.z + z);
                    if beach_target_valid(view, pos) {
                        return Some(pos);
                    }
                    z = if z > 0 { -z } else { 1 - z };
                }
                x = if x > 0 { -x } else { 1 - x };
            }
        }
        y = if y > 0 { -y } else { 1 - y };
    }
    let _ = view;
    None
}

/// `DrownedGoToBeachGoal.isValidTarget`：上面两格空、脚下这格能站。
///
/// `DrownedGoToBeachGoal.isValidTarget`: two empty blocks above and a standable floor.
fn beach_target_valid(view: &dyn BlockView, pos: BlockPos) -> bool {
    view.block_state(pos.above()).is_air()
        && view.block_state(pos.above().above()).is_air()
        && view.is_face_sturdy(pos, mc_bamboo_core::Direction::Up, SupportType::Full)
}

fn nearest_attackable(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    kind: TargetType,
) -> Option<EntityUid> {
    if ctx.blocks().difficulty() == 0 {
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
        // 战斗用的 `TargetingConditions.test`：`targeter.canAttack(target)` 要求
        // `Player.canBeSeenAsEnemy()`，能力里 `invulnerable` 的玩家（创造模式）在测距离和
        // 视线之前就被排除，目标选择器不会为他启动。
        // Combat `TargetingConditions.test`: `targeter.canAttack(target)` needs
        // `Player.canBeSeenAsEnemy()`, so a player with the `invulnerable` ability
        // (creative) is rejected before the range and sight checks and never starts the goal.
        if player.creative {
            continue;
        }
        // 选择器（`isAngryAt`）在 `TargetingConditions.test` 里先于距离和视线。
        // The selector (`isAngryAt`) runs before range and sight in `TargetingConditions.test`.
        if kind == TargetType::AngerPlayer
            && !super::neutral::is_angry_at_player(mob, &player, difficulty)
        {
            continue;
        }
        // `Slime.addTargetingGoals` 的选择器：高度差不超过 4。
        // The `Slime.addTargetingGoals` selector: at most 4 blocks of height difference.
        if mob.mob_type == super::MobType::Slime
            && !super::slime::accepts_target_height(me, player.position.y)
        {
            continue;
        }
        // `Drowned.okTarget` 是玩家那条目标的子选择器：白天只打在水里的玩家。
        //
        // The drowned's `okTarget` subselector: outside daylight it only targets wet players.
        if mob.mob_type == super::MobType::Drowned
            && view.is_bright_outside()
            && !view
                .fluid_state(BlockPos::containing(player.position))
                .is_tagged(fluid_tags::WATER)
        {
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

/// `LlamaAttackWolfGoal.findTarget`：四分之一跟随距离内、看得见的最近野狼（`!wolf.isTame()`）。
///
/// `LlamaAttackWolfGoal.findTarget`: the nearest untamed wolf in sight within
/// a quarter of `FOLLOW_RANGE`.
fn nearest_wild_wolf_target(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> Option<EntityUid> {
    let range = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE)
        * 0.25;
    let area = me.bounding_box().inflate(range, range, range);
    let mut candidates = Vec::new();
    ctx.for_each_other_in(area, &mut |other| {
        let KindPublic::Mob(public) = &other.kind else {
            return;
        };
        if other.uid == me.id
            || other.removed
            || !public.is_alive()
            || public.mob_type != super::MobType::Wolf
            || public.tame_flags & super::tame::FLAG_TAME != 0
        {
            return;
        }
        let (_, _, eye) = public.mob_type.dimensions(public.baby);
        candidates.push((other.uid, other.position, eye));
    });
    let eye = Vec3::new(
        me.position.x,
        me.position.y + f64::from(mob.eye_height),
        me.position.z,
    );
    let limit = mth::java_max(range, 2.0);
    let mut best = -1.0_f64;
    let mut found = None;
    for (uid, position, target_eye) in candidates {
        if me.position.distance_to_sqr(position) > limit * limit {
            continue;
        }
        let target_eye = Vec3::new(position.x, position.y + f64::from(target_eye), position.z);
        if !living::has_line_of_sight(me, mob, ctx.blocks(), uid, target_eye) {
            continue;
        }
        let distance = position.distance_to_sqr(eye);
        if best == -1.0 || distance < best {
            best = distance;
            found = Some(uid);
        }
    }
    found
}

/// `NearestAttackableTargetGoal.findTarget` 的生物分支：`FOLLOW_RANGE` 方盒里、
/// 类型符合、看得见、在跟随距离内的最近一个（从眼睛高度量）。
///
/// The mob branch of `NearestAttackableTargetGoal.findTarget`: the nearest
/// matching mob in sight within the follow range, measured from the eyes.
pub(super) fn nearest_mob_target(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    matches: impl Fn(super::MobType) -> bool,
) -> Option<EntityUid> {
    nearest_mob_target_with(me, mob, ctx, |public| matches(public.mob_type))
}

/// 按完整 [`super::MobPublic`] 谓词找最近的可攻击生物目标。
///
/// Find the nearest attackable mob target matching a full [`super::MobPublic`] predicate.
pub(super) fn nearest_mob_target_with(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    matches: impl Fn(&super::MobPublic) -> bool,
) -> Option<EntityUid> {
    let range = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE);
    let area = me.bounding_box().inflate(range, range, range);
    let mut candidates = Vec::new();
    ctx.for_each_other_in(area, &mut |other| {
        let KindPublic::Mob(public) = &other.kind else {
            return;
        };
        if other.uid == me.id || other.removed || !public.is_alive() || !matches(public) {
            return;
        }
        let (_, _, eye) = public.mob_type.dimensions(public.baby);
        candidates.push((other.uid, other.position, eye));
    });
    let eye = Vec3::new(
        me.position.x,
        me.position.y + f64::from(mob.eye_height),
        me.position.z,
    );
    let limit = mth::java_max(range, 2.0);
    let mut best = -1.0_f64;
    let mut found = None;
    for (uid, position, target_eye) in candidates {
        if me.position.distance_to_sqr(position) > limit * limit {
            continue;
        }
        let target_eye = Vec3::new(position.x, position.y + f64::from(target_eye), position.z);
        if !living::has_line_of_sight(me, mob, ctx.blocks(), uid, target_eye) {
            continue;
        }
        let distance = position.distance_to_sqr(eye);
        if best == -1.0 || distance < best {
            best = distance;
            found = Some(uid);
        }
    }
    found
}

/// `LeapAtTargetGoal.canUse`。
fn leap_can_use(me: &mut Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    let Some(target) = combat_target(mob, ctx) else {
        return false;
    };
    let distance = me.position.distance_to_sqr(target.position);
    if !(4.0..=16.0).contains(&distance) {
        return false;
    }
    me.on_ground && me.random.next_int_bounded(reduced_tick_delay(5)) == 0
}

/// `LeapAtTargetGoal.start`。
fn leap_start(me: &mut Entity, mob: &Mob, ctx: &dyn EntityCtx, yd: f32) {
    let Some(target) = combat_target(mob, ctx) else {
        return;
    };
    let movement = me.delta;
    let mut delta = Vec3::new(
        target.position.x - me.position.x,
        0.0,
        target.position.z - me.position.z,
    );
    if delta.length_sqr() > 1.0e-7 {
        delta = delta.normalize().scale(0.4).add(movement.scale(0.2));
    }
    me.delta = Vec3::new(delta.x, f64::from(yd), delta.z);
}

/// `PathfinderMob.isWithinMeleeAttackRange`：用实体交互范围算的方形距离。
pub(super) fn within_melee_range(me: &Entity, _mob: &Mob, target: CombatTarget) -> bool {
    // `Mob.DEFAULT_ATTACK_REACH = sqrt(2.04f) - 0.6f`（26.3 字节码）。
    // 官方用 `getAttackBoundingBox(reach).intersects(target.getHitbox())`，
    // 这里按双方 Aabb 膨胀后的相交判定，而不是旧的「交互范围 + 1」方格。
    let reach = 2.039_999_961_853_027_3_f64.sqrt() - f64::from(0.6_f32);
    let (width, height) = _mob.dimensions();
    let half = f64::from(width) / 2.0;
    // `getAttackBoundingBox` 只在水平方向膨胀（`inflate(reach, 0, reach)`）。
    // `getAttackBoundingBox` only grows horizontally (`inflate(reach, 0, reach)`).
    let ours = crate::shape::Aabb::new(
        me.position.x - half - reach,
        me.position.y,
        me.position.z - half - reach,
        me.position.x + half + reach,
        me.position.y + f64::from(height),
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

pub(super) fn can_target_player(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    player: PlayerView,
) -> bool {
    !player.creative
        && ctx.blocks().difficulty() != 0
        && targeting_test(
            me,
            mob,
            ctx.blocks(),
            &player,
            mob.attributes
                .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE),
            true,
        )
}
