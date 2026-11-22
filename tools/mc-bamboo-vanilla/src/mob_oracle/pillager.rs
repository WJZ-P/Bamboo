//! 掠夺者场景在 Bamboo 一侧的起始状态和逐刻状态，和 Java 的 `PillagerScenes` 一一对应。
//!
//! The Bamboo side of the pillager scenes: start state and per-tick state, mirroring
//! Java's `PillagerScenes`.

use mc_bamboo_blocks::entity::mob::{Mob, crossbow};
use mc_bamboo_core::BlockPos;
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

/// `PillagerScenes.PATROL_TARGET`。
const PATROL_TARGET: BlockPos = BlockPos::new(180, 64, 60);

/// `PillagerScenes.setup`：主手一把弩；巡逻场景里 0 号是带目标的队长，其余只是在巡逻。
///
/// `PillagerScenes.setup`: a crossbow in the main hand; in patrol scenes mob 0 is the
/// leader with the target and the rest are merely patrolling.
pub(super) fn setup(scene: &str, index: usize, mob: &mut Mob) {
    if mob.mob_type != mc_bamboo_blocks::entity::mob::MobType::Pillager {
        return;
    }
    mob.equipment.set(
        EquipmentSlot::MainHand,
        ItemStack::new(mc_bamboo_data::items::CROSSBOW, 1),
    );
    if scene.starts_with("pillager_patrol") || scene.starts_with("pillager_hold") {
        if index == 0 {
            mob.raider.patrol_leader = true;
            mob.raider.patrol_target = Some(PATROL_TARGET);
        }
        mob.raider.patrolling = true;
    }
}

/// `PillagerScenes.state`：巡逻、队长、巡逻目标、装填标志、是否在用物品、已用刻数、弩里的箭数。
///
/// `PillagerScenes.state`: patrolling, leader, patrol target, charging flag, using item,
/// ticks used and the number of loaded arrows.
pub(super) fn state(mob: &Mob) -> String {
    let target = mob.raider.patrol_target.map_or_else(
        || "-".to_owned(),
        |pos| format!("{},{},{}", pos.x, pos.y, pos.z),
    );
    format!(
        "pillager:{}:{}:{}:{}:{}:{}:{}",
        mob.raider.patrolling,
        mob.raider.patrol_leader,
        target,
        mob.crossbow.charging,
        crossbow::is_using(mob),
        crossbow::ticks_using(mob),
        crossbow::charged_count(mob.equipment.get(EquipmentSlot::MainHand)),
    )
}
