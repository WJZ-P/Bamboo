//! 劫掠兽：官方 `Raider` → `Ravager`。100 血、0.3 移速、0.75 击退抗性、12 攻击、
//! 1.5 攻击击退、32 跟随距离、1 格台阶高度；三个同步计时字段 `AttackTick` /
//! `StunTick` / `RoarTick` 存档。
//!
//! 咆哮、撞击、踩坏树叶 / 作物和骑手生成属于袭击波次交互，先按官方注册顺序
//! 占位；属性、计时字段和存档按 26.3 字节码。
//!
//! Ravager: vanilla `Raider` → `Ravager`. 100 health, 0.3 speed, 0.75 knockback
//! resistance, 12 attack damage, 1.5 attack knockback, 32 follow range and a
//! 1-block step height; the three synced timers `AttackTick` / `StunTick` /
//! `RoarTick` are saved. Roaring, charging, breaking leaves / crops and riders
//! are raid-wave interactions and keep their registration slots for now.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::registries::attribute;
use mc_bamboo_nbt::Tag;

use super::{
    Mob,
    attributes::Attributes,
    goals::{Goal, GoalSelector, TargetType},
    navigation::Navigation,
    raider::RaiderGoal,
};
use crate::{entity::Entity, random::RandomSource};

/// `Ravager` 的三个同步计时字段，按官方存档键 `AttackTick` / `StunTick` / `RoarTick`。
///
/// Ravager's three synced timers, saved as `AttackTick` / `StunTick` / `RoarTick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RavagerState {
    pub attack_tick: i32,
    pub stunned_tick: i32,
    pub roar_tick: i32,
}

/// `Ravager.createAttributes()`。
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::skeleton();
    table.set_base(attribute::MAX_HEALTH, 100.0);
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.3_f32));
    table.set_base(attribute::KNOCKBACK_RESISTANCE, 0.75);
    table.set_base(attribute::ATTACK_DAMAGE, 12.0);
    table.set_base(attribute::ATTACK_KNOCKBACK, f64::from(1.5_f32));
    table.set_base(attribute::FOLLOW_RANGE, 32.0);
    table.set_base(attribute::STEP_HEIGHT, 1.0);
    table
}

/// `Ravager.registerGoals` 的注册顺序：父类四个袭击目标、漂浮、原地对峙、
/// 近战、闲逛、看玩家 / 看生物。
///
/// The `Ravager.registerGoals` order: the four superclass raid goals, float,
/// hold ground, melee, stroll and the two look goals.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    let mut goals = vec![(4, Goal::Raider(RaiderGoal::patrol()))];
    goals.extend(super::raider::raider_goals(
        super::raider::RaiderGoal::raid_goals(),
    ));
    goals.extend([
        (0, Goal::Float),
        (
            4,
            Goal::Raider(RaiderGoal::HoldGround {
                hostile_radius_sqr: 10.0 * 10.0,
            }),
        ),
        (5, Goal::melee(1.0, false)),
        (
            8,
            Goal::Stroll {
                speed: 0.6,
                interval: 120,
                probability: super::goals::PLAIN_STROLL,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        (
            9,
            Goal::LookAtPlayer {
                look_distance: 3.0,
                probability: 1.0,
                look_at: None,
                look_time: 0,
            },
        ),
        (
            10,
            Goal::Raider(RaiderGoal::LookAtMob {
                look_distance: 8.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            }),
        ),
    ]);
    navigation.flags.can_float = true;
    GoalSelector::from_ordered_pairs(goals)
}

/// `Ravager` 的目标选择器。
pub(super) fn targets() -> GoalSelector {
    GoalSelector::from_ordered_pairs(vec![
        (
            1,
            Goal::HurtByTarget {
                timestamp: -1,
                unseen_ticks: 0,
            },
        ),
        (2, Goal::nearest_target(TargetType::Player)),
        (3, Goal::nearest_target_without_sight(TargetType::Villager)),
        (3, Goal::nearest_target(TargetType::IronGolem)),
    ])
}

/// `Ravager.finalizeSpawn`：`Raider` 的 `canJoinRaid`、PatrollingMonster 的队长 /
/// 巡逻状态，以及 `Mob` 的初始化，按官方顺序取随机数。
pub(super) fn finalize_spawn(entity: &mut Entity, random: &dyn RandomSource) {
    entity.with_mob(|_, mob| {
        mob.raider.can_join_raid = true;
        super::raider::finalize_patrol(mob, random, false, false);
        super::spawn::finalize_mob(random, mob);
    });
}

/// `Ravager.addAdditionalSaveData` 的三个计时字段。
pub(super) fn save(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    out.push(("AttackTick".to_owned(), Tag::Int(mob.ravager.attack_tick)));
    out.push(("StunTick".to_owned(), Tag::Int(mob.ravager.stunned_tick)));
    out.push(("RoarTick".to_owned(), Tag::Int(mob.ravager.roar_tick)));
}

/// `Ravager.readAdditionalSaveData` 的三个计时字段。
pub(super) fn load(tag: &Tag) -> RavagerState {
    use crate::archive::tag;
    RavagerState {
        attack_tick: tag::int_or(tag, "AttackTick", 0),
        stunned_tick: tag::int_or(tag, "StunTick", 0),
        roar_tick: tag::int_or(tag, "RoarTick", 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_nbt::Tag;

    #[test]
    fn attributes_follow_vanilla() {
        let table = attributes();
        assert_eq!(table.value(attribute::MAX_HEALTH), 100.0);
        assert_eq!(table.value(attribute::MOVEMENT_SPEED) as f32, 0.3);
        assert_eq!(table.value(attribute::KNOCKBACK_RESISTANCE), 0.75);
        assert_eq!(table.value(attribute::ATTACK_DAMAGE), 12.0);
        assert_eq!(table.value(attribute::ATTACK_KNOCKBACK) as f32, 1.5);
        assert_eq!(table.value(attribute::FOLLOW_RANGE), 32.0);
        assert_eq!(table.value(attribute::STEP_HEIGHT), 1.0);
    }

    #[test]
    fn timers_round_trip() {
        let mut mob = Mob::new(super::super::MobType::Ravager);
        mob.ravager = RavagerState {
            attack_tick: 7,
            stunned_tick: 11,
            roar_tick: 20,
        };
        let mut out = Vec::new();
        save(&mob, &mut out);
        let tag = Tag::Compound(out);
        let loaded = load(&tag);
        assert_eq!(loaded.attack_tick, 7);
        assert_eq!(loaded.stunned_tick, 11);
        assert_eq!(loaded.roar_tick, 20);
    }
}
