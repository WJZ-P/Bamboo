//! 卫道士：官方 `PatrollingMonster` → `Raider` → `AbstractIllager` → `Vindicator`。
//!
//! 属性、`Johnny` 彩蛋、主手铁斧和存档键照 26.3 字节码。袭击里的四个通用目标和
//! `VindicatorBreakDoorGoal` 沿用现有的占位注册顺序（`canUse` 恒假、不取随机数），
//! 破门行为留待袭击目标全面接入时补。
//!
//! Vindicator: vanilla `PatrollingMonster` → `Raider` → `AbstractIllager` →
//! `Vindicator`. Attributes, the `Johnny` easter egg, the iron axe and the save
//! keys follow the 26.3 bytecode. The four raid goals and
//! `VindicatorBreakDoorGoal` keep their registration slots for now (`canUse` is
//! false and draws nothing); door breaking arrives with the full raid goals.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{items, registries::attribute};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{
    Mob,
    attributes::Attributes,
    goals::{Goal, GoalSelector, TargetType},
    navigation::Navigation,
    raider::RaiderGoal,
};
use crate::{entity::Entity, random::RandomSource};

/// `Vindicator` 自己的状态：Johnny 彩蛋。
///
/// Vindicator-only state: the Johnny easter egg.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VindicatorState {
    pub johnny: bool,
}

/// `Vindicator.createAttributes()`：24 血、0.35 移速、12 跟随、5 攻击。
///
/// `Vindicator.createAttributes()`: 24 health, 0.35 speed, 12 follow range and
/// 5 attack damage.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::skeleton();
    table.set_base(attribute::MAX_HEALTH, 24.0);
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.35_f32));
    table.set_base(attribute::FOLLOW_RANGE, 12.0);
    table.set_base(attribute::ATTACK_DAMAGE, 5.0);
    table
}

/// `Vindicator.registerGoals` 的注册顺序（父类链 + 自己）。
///
/// The `Vindicator.registerGoals` order (superclass chain + its own goals).
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    let mut goals = vec![(4, Goal::Raider(RaiderGoal::patrol()))];
    goals.extend(super::raider::raider_goals(
        super::raider::RaiderGoal::raid_goals(),
    ));
    goals.extend([
        (0, Goal::Float),
        (1, Goal::Unavailable("AvoidEntityGoal")),
        // `VindicatorBreakDoorGoal(2)`：只在有活跃袭击时能启动。
        (2, Goal::Raider(RaiderGoal::BreakDoor)),
        (3, Goal::Raider(RaiderGoal::OpenDoor)),
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

/// `Vindicator` 的目标选择器：受伤反击、玩家、村民、铁傀儡、Johnny 彩蛋。
///
/// The vindicator's target selector: hurt by, players, villagers, iron golems
/// and the Johnny easter egg.
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
        (
            4,
            Goal::Raider(RaiderGoal::Johnny {
                interval: 0,
                unseen_ticks: 0,
                found: None,
            }),
        ),
    ])
}

/// `Vindicator.populateDefaultEquipmentSlots`：主手铁斧；PatrollingMonster 的
/// 队长 / 巡逻状态和 Mob 的初始化按官方顺序各取随机数。
///
/// `Vindicator.populateDefaultEquipmentSlots` puts an iron axe in the main hand;
/// the patrol and Mob initialization draw their numbers in the official order.
pub(super) fn finalize_spawn(entity: &mut Entity, random: &dyn RandomSource) {
    entity.with_mob(|_, mob| {
        mob.equipment
            .set(EquipmentSlot::MainHand, ItemStack::new(items::IRON_AXE, 1));
        mob.raider.can_join_raid = true;
        super::raider::finalize_patrol(mob, random, false, false);
        super::spawn::finalize_mob(random, mob);
    });
}

/// `Vindicator.addAdditionalSaveData` 的 `Johnny` 键。
///
/// The `Johnny` key of `Vindicator.addAdditionalSaveData`.
pub(super) fn save(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    // 卫道士没有自己的可变状态字段（`Mob` 上不存 Johnny），只读 passthrough 里的值。
    // Vindicator has no separate mutable state on `Mob`; only a saved passthrough key.
    if let Some((_, value)) = mob.passthrough.iter().find(|(key, _)| key == "Johnny") {
        out.retain(|(key, _)| key != "Johnny");
        out.push(("Johnny".to_owned(), value.clone()));
    }
}

/// 读盘时的 `Johnny` 是布尔；名字叫 "Johnny" 时官方 `setCustomName` 也会置位。
///
/// On load, `Johnny` is a boolean; `setCustomName("Johnny")` also sets it.
pub(super) fn load_johnny(tag: &Tag) -> bool {
    crate::archive::tag::bool_or(tag, "Johnny", false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_follow_vanilla() {
        let table = attributes();
        assert_eq!(table.value(attribute::MAX_HEALTH), 24.0);
        assert_eq!(table.value(attribute::MOVEMENT_SPEED) as f32, 0.35);
        assert_eq!(table.value(attribute::FOLLOW_RANGE), 12.0);
        assert_eq!(table.value(attribute::ATTACK_DAMAGE), 5.0);
    }

    #[test]
    fn goal_order_matches_vanilla() {
        let mut navigation = Navigation::new(12.0);
        let selector = goals(&mut navigation);
        let names: Vec<&str> = selector
            .goals
            .iter()
            .map(|wrapped| wrapped.goal.name())
            .collect();
        assert_eq!(names[0], "LongDistancePatrolGoal");
        assert_eq!(names[6], "AvoidEntityGoal");
        assert_eq!(names[7], "VindicatorBreakDoorGoal");
        assert_eq!(names[9], "HoldGroundAttackGoal");
        assert_eq!(names[10], "MeleeAttackGoal");
    }
}
