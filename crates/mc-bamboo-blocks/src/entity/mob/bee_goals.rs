//! 蜜蜂的目标表：`Bee.registerGoals` 的优先级和官方类名。
//!
//! 逐刻运动和授粉 / 进巢 / 找巢 / 找花的细节在 [`super::bee`] 已经建模的状态机上，
//! 这里把还没跑起来完整寻路的目标注册成占位：占优先级和标志位锁，不取随机数，
//! 名字仍进逐刻对拍的 running names。
//!
//! Bee goals: the priorities and vanilla class names of `Bee.registerGoals`.
//! Goals whose full pathfinding logic is not wired yet are registered as inactive
//! placeholders so they still occupy their priority slots and locks.

use super::{
    GoalSelector,
    goals::{Goal, WrappedGoal},
    navigation::Navigation,
};

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        running: false,
        goal,
    }
}

/// `Bee.registerGoals`：优先级 0..9 和三个目标目标。
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.fly = true;
    GoalSelector::from_goals(vec![
        // 0 `BeeAttackGoal(1.4, true)`；攻击目标还在实现中，先用近战占位。
        wrap(0, Goal::Unavailable("BeeAttackGoal")),
        wrap(1, Goal::Unavailable("BeeEnterHiveGoal")),
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
        wrap(3, Goal::Unavailable("ValidateHiveGoal")),
        wrap(3, Goal::Unavailable("ValidateFlowerGoal")),
        wrap(4, Goal::Unavailable("BeePollinateGoal")),
        wrap(
            5,
            Goal::FollowParent {
                speed: 1.25,
                parent: None,
                time_to_recalc_path: 0,
            },
        ),
        wrap(5, Goal::Unavailable("BeeLocateHiveGoal")),
        wrap(5, Goal::Unavailable("BeeGoToHiveGoal")),
        wrap(6, Goal::Unavailable("BeeGoToKnownFlowerGoal")),
        wrap(7, Goal::Unavailable("BeeGrowCropGoal")),
        wrap(8, Goal::Unavailable("BeeWanderGoal")),
        wrap(9, Goal::Float),
    ])
}

/// `Bee.registerGoals` 的 target 选择器。
pub(super) fn target_goals() -> GoalSelector {
    GoalSelector::from_goals(vec![
        wrap(
            1,
            Goal::HurtByTarget {
                timestamp: 0,
                unseen_ticks: 0,
            },
        ),
        wrap(2, Goal::Unavailable("BeeBecomeAngryTargetGoal")),
        wrap(3, Goal::Unavailable("ResetUniversalAngerTargetGoal")),
    ])
}
