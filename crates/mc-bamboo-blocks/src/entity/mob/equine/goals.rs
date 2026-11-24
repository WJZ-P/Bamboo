//! 马科的目标表：`AbstractHorse.registerGoals` / `addBehaviourGoals` 和
//! `Llama.registerGoals`，以及 `RandomStandGoal` 的取数。
//!
//! Equine goal tables (`AbstractHorse.registerGoals` / `addBehaviourGoals`,
//! `Llama.registerGoals`) and the draws of `RandomStandGoal`.

use mc_bamboo_core::Vec3;

use super::{Mob, MobType, life};
use crate::entity::{
    Entity,
    mob::{
        goals::{Goal, GoalSelector, TargetType, WrappedGoal, reduced_tick_delay},
        navigation::Navigation,
    },
};

/// `getAmbientStandInterval()` = `getAmbientSoundInterval()`，马科都是 400。
///
/// `getAmbientStandInterval()` is `getAmbientSoundInterval()`, 400 for every equine.
const AMBIENT_STAND_INTERVAL: i32 = 400;

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        running: false,
        goal,
    }
}

fn selector(goals: Vec<WrappedGoal>) -> GoalSelector {
    let mut selector = GoalSelector::default();
    selector.goals = goals;
    selector
}

fn breed() -> Goal {
    Goal::Breed {
        speed: 1.0,
        partner: None,
        love_time: 0,
    }
}

fn follow_parent() -> Goal {
    Goal::FollowParent {
        speed: 1.0,
        parent: None,
        time_to_recalc_path: 0,
    }
}

fn stroll() -> Goal {
    Goal::Stroll {
        speed: 0.7,
        interval: 120,
        probability: 0.001,
        wanted: Vec3::default(),
        force_trigger: false,
    }
}

fn look_at_player() -> Goal {
    Goal::LookAtPlayer {
        look_distance: 6.0,
        probability: 0.02,
        look_at: None,
        look_time: 0,
    }
}

fn random_look() -> Goal {
    Goal::RandomLookAround {
        rel_x: 0.0,
        rel_z: 0.0,
        look_time: 0,
    }
}

fn panic() -> Goal {
    Goal::Panic {
        speed: 1.2,
        target: Vec3::default(),
    }
}

fn tempt() -> Goal {
    Goal::Tempt {
        speed: 1.25,
        steering_item: false,
        player: None,
        calm_down: 0,
        scare: None,
    }
}

/// `RunAroundLikeCrazyGoal(this, 1.2)`。
///
/// `RunAroundLikeCrazyGoal(this, 1.2)`.
fn run_around_like_crazy() -> Goal {
    Goal::RunAroundLikeCrazy {
        speed: 1.2,
        wanted: Vec3::default(),
    }
}

/// 按官方 `addGoal` 的调用顺序建目标表；`FloatGoal` 的构造函数顺手打开导航的浮水。
///
/// 羊驼的商队目标还是不取数的占位（见 README 已知限制）。
///
/// Build the table in vanilla `addGoal` order; `FloatGoal` also turns on floating.
/// The llama caravan goal stays a draw-free placeholder (see the README known
/// limitations).
pub(in crate::entity::mob) fn goals(kind: MobType, navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.can_float = true;
    if kind == MobType::Llama {
        return selector(vec![
            wrap(0, Goal::Float),
            wrap(1, run_around_like_crazy()),
            wrap(2, Goal::Unavailable("LlamaFollowCaravanGoal")),
            wrap(
                3,
                Goal::RangedAttack {
                    target: None,
                    attack_time: -1,
                    see_time: 0,
                },
            ),
            wrap(3, panic()),
            wrap(4, breed()),
            wrap(5, tempt()),
            wrap(6, follow_parent()),
            wrap(7, stroll()),
            wrap(8, look_at_player()),
            wrap(9, random_look()),
        ]);
    }
    selector(vec![
        wrap(1, run_around_like_crazy()),
        wrap(2, breed()),
        wrap(4, follow_parent()),
        wrap(6, stroll()),
        wrap(7, look_at_player()),
        wrap(8, random_look()),
        wrap(
            9,
            Goal::RandomStand {
                next_stand: -AMBIENT_STAND_INTERVAL,
            },
        ),
        wrap(0, Goal::Float),
        // `AbstractHorse.MountPanicGoal`：没被生物控制时就是普通的 `PanicGoal`。
        // `MountPanicGoal` is a plain `PanicGoal` while no mob controls the horse.
        wrap(1, panic()),
        wrap(3, tempt()),
    ])
}

/// 马、驴、骡没有 `targetSelector`；羊驼有回击（吐过一口就停）和打野狼（四分之一跟随距离）两个。
///
/// Horses, donkeys and mules have no target goals; the llama has retaliation
/// (which stops after one spit) and targeting untamed wolves at quarter range.
pub(in crate::entity::mob) fn targets(kind: MobType) -> GoalSelector {
    if kind != MobType::Llama {
        return GoalSelector::default();
    }
    selector(vec![
        wrap(
            1,
            Goal::LlamaHurtByTarget {
                timestamp: 0,
                unseen_ticks: 0,
            },
        ),
        wrap(
            2,
            Goal::NearestAttackableTarget {
                kind: TargetType::Wolf,
                light_gate: false,
                interval: reduced_tick_delay(16),
                unseen_ticks: 0,
                found: None,
                // `LlamaAttackWolfGoal(16, false, true, selector)`：`mustSee=false`。
                // `LlamaAttackWolfGoal(16, false, true, selector)`: `mustSee=false`.
                must_see: false,
            },
        ),
    ])
}

/// `RandomStandGoal.canUse`：先累加，过了零以后按 `nextInt(1000) < nextStand`
/// 抽签，抽中就重置间隔，再要求不僵住并且 `nextInt(10) == 0`。
///
/// Count up; once positive, draw `nextInt(1000) < nextStand`; on success reset
/// the interval, then require a mobile horse and `nextInt(10) == 0`.
pub(in crate::entity::mob) fn random_stand_can_use(
    me: &mut Entity,
    mob: &Mob,
    next_stand: &mut i32,
) -> bool {
    *next_stand += 1;
    if *next_stand <= 0 || me.random.next_int_bounded(1000) >= *next_stand {
        return false;
    }
    *next_stand = -AMBIENT_STAND_INTERVAL;
    !life::is_immobile(mob) && me.random.next_int_bounded(10) == 0
}

/// `RandomStandGoal.start`：扬蹄，再 `playSound(getAmbientStandSound())`（音量、音高都是 1）。
///
/// Rear, then `playSound(getAmbientStandSound())` at volume and pitch 1.
pub(in crate::entity::mob) fn random_stand_start(me: &mut Entity, mob: &mut Mob) {
    life::stand_if_possible(mob);
    let sound = life::voice(mob, life::Voice::Ambient);
    crate::entity::mob::living::play_sound(me, mob, sound, 1.0, 1.0);
}
