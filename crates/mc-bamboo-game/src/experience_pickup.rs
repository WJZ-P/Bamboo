//! `ExperienceOrb.playerTouch` 的玩家侧：拾取冷却、经验修补、加经验和发包。
//!
//! 官方 `Player.aiStep` 每刻只碰一颗经验球（从附近的球里用玩家随机源挑一个），
//! 其余实体逐个碰；球的 `count` 和消失交给 [`mc_bamboo_blocks::entity::pick_up`]。
//!
//! The player side of `ExperienceOrb.playerTouch`: cooldown, mending, experience
//! and packets. Official `Player.aiStep` touches exactly one nearby orb per tick,
//! chosen with the player's random source; the orb's `count` and removal are
//! handled by [`mc_bamboo_blocks::entity::pick_up`].

use mc_bamboo_blocks::entity::{EntityKind, pick_up as take_orb};
use mc_bamboo_data::registries::data_component_type as kinds;
use mc_bamboo_loot::{enchantment_def, has_repair_with_xp, repair_with_xp_factor};
use mc_bamboo_protocol::{
    components::{ComponentValue, PatchEntry, defaults},
    packet::play::EquipmentSlot,
};

use crate::{entity_tracking, inbound::PlayerId, inventory, player::Player, region::Region};

/// 官方 `EquipmentSlot.VALUES` 里玩家有的前六格，顺序不能改：
/// 候选名单的顺序决定 `Util.getRandom` 抽到哪一件。
///
/// The player's first six `EquipmentSlot.VALUES`, in order: the candidate
/// list order decides which item `Util.getRandom` picks.
const EQUIPMENT: [EquipmentSlot; 6] = [
    EquipmentSlot::MainHand,
    EquipmentSlot::OffHand,
    EquipmentSlot::Feet,
    EquipmentSlot::Legs,
    EquipmentSlot::Chest,
    EquipmentSlot::Head,
];

/// 官方 `ExperienceOrb.repairPlayerItems`。返回剩下的经验点数：没有可修补的
/// 装备时就是 `amount`，修完还有剩余时递归找下一件。改过的物品栏槽记进 `changed`。
///
/// Official `ExperienceOrb.repairPlayerItems`. Returns the experience points
/// left for the player: `amount` when nothing can be repaired, and a recursive
/// call when one item did not use all of them. Changed inventory slots go to
/// `changed` so the caller can resend them.
pub(crate) fn repair_player_items(
    player: &mut Player,
    amount: i32,
    changed: &mut Vec<usize>,
) -> i32 {
    let mut candidates = Vec::new();
    for &slot in &EQUIPMENT {
        let Some(stack) = player.inventory.equipment(slot) else {
            continue;
        };
        if !stack.is_damaged() {
            continue;
        }
        let patch = stack.patch();
        let Some(ComponentValue::Enchantments(levels)) = patch.get(kinds::ENCHANTMENTS) else {
            continue;
        };
        // `getRandomItemWith` 对每个附魔条目各加一次候选（同一件装备可能被加两次）。
        // `getRandomItemWith` adds one candidate per matching enchantment entry.
        for (id, level) in levels {
            if has_repair_with_xp(*id) && matching_slot(*id, slot) && *level > 0 {
                candidates.push(slot);
            }
        }
    }
    if candidates.is_empty() {
        return amount;
    }
    // `Util.getRandom` 在候选非空时取一次 `nextInt(size)`；即使冷却没过也要取。
    // `Util.getRandom` draws one `nextInt(size)` whenever the list is not empty,
    // even if the pickup cooldown later rejects the touch.
    let chosen = candidates[player.random.next_int_bounded(candidates.len() as i32) as usize];
    let Some(stack) = player.inventory.equipment_mut(chosen) else {
        return amount;
    };
    let mut patch = stack.patch();
    let damage = match defaults::resolve(stack.item, &patch, kinds::DAMAGE) {
        Some(ComponentValue::Damage(value)) => *value,
        _ => 0,
    };
    let max_damage = match defaults::resolve(stack.item, &patch, kinds::MAX_DAMAGE) {
        Some(ComponentValue::MaxDamage(value)) => *value,
        _ => 0,
    };
    let mut factor = 1.0_f32;
    if let Some(ComponentValue::Enchantments(levels)) = patch.get(kinds::ENCHANTMENTS) {
        for (id, level) in levels {
            if has_repair_with_xp(*id) && matching_slot(*id, chosen) {
                factor *= repair_with_xp_factor(*id, *level);
            }
        }
    }
    // `MutableFloat.intValue()` 先截断再 `Math.max(0, ...)`。
    // `MutableFloat.intValue()` truncates, then `Math.max(0, ...)`.
    let repair_amount = (amount as f32 * factor).max(0.0) as i32;
    let repair = repair_amount.min(damage);
    if repair > 0 {
        let updated = damage.wrapping_sub(repair).clamp(0, max_damage);
        if updated == 0 {
            patch.put(kinds::DAMAGE, PatchEntry::Removed);
        } else {
            patch.put(
                kinds::DAMAGE,
                PatchEntry::Set(ComponentValue::Damage(updated)),
            );
        }
        let (added, removed) = patch.to_opaque();
        stack.added = added;
        stack.removed = removed;
        changed.push(inventory_slot(chosen, player));
        // `amount - repair * amount / repairAmount`，全是 int 运算。
        // All of `amount - repair * amount / repairAmount` is int arithmetic.
        let leftover = amount.wrapping_sub(
            repair
                .wrapping_mul(amount)
                .wrapping_div(repair_amount.max(1)),
        );
        if leftover > 0 {
            return repair_player_items(player, leftover, changed);
        }
    }
    0
}

/// `Enchantment.matchingSlot`：slots 组里的 `any` 匹配所有槽。
fn matching_slot(enchantment: i32, slot: EquipmentSlot) -> bool {
    let Some(def) = enchantment_def(enchantment) else {
        return false;
    };
    let name = match slot {
        EquipmentSlot::MainHand => "mainhand",
        EquipmentSlot::OffHand => "offhand",
        EquipmentSlot::Feet => "feet",
        EquipmentSlot::Legs => "legs",
        EquipmentSlot::Chest => "chest",
        EquipmentSlot::Head => "head",
        _ => return false,
    };
    def.slots
        .iter()
        .any(|group| group == "any" || group == name)
}

/// 装备槽对应的 `InventoryMenu` 槽位，改完要发 `container_set_slot`。
fn inventory_slot(slot: EquipmentSlot, player: &Player) -> usize {
    match slot {
        EquipmentSlot::MainHand => player.inventory.hand_slot(mc_bamboo_core::Hand::Main),
        EquipmentSlot::OffHand => player.inventory.hand_slot(mc_bamboo_core::Hand::Off),
        EquipmentSlot::Feet => 8,
        EquipmentSlot::Legs => 7,
        EquipmentSlot::Chest => 6,
        EquipmentSlot::Head => 5,
        _ => 0,
    }
}

/// 玩家碰一颗经验球。调用方已经按官方顺序抽过一次随机源。
///
/// One player touches one experience orb. The caller already drew the
/// official random number that chose this orb.
pub(crate) fn pickup_orb(region: &mut Region, id: PlayerId, uid: i32) {
    let value = {
        let Some(entity) = region.entities.get(uid) else {
            return;
        };
        let EntityKind::ExperienceOrb(orb) = &entity.kind else {
            return;
        };
        orb.value
    };
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    if player.take_xp_delay != 0 {
        return;
    }
    player.take_xp_delay = 2;
    // `LivingEntity.take` 的配对包：看得见这颗球的玩家和拾取者本人都收到。
    entity_tracking::broadcast_take(region, uid, id, 1);
    let mut changed = Vec::new();
    let points = repair_player_items(
        region.players.get_mut(&id).expect("玩家还在"),
        value,
        &mut changed,
    );
    if points > 0 {
        region
            .players
            .get_mut(&id)
            .expect("玩家还在")
            .give_experience_points(points);
    }
    for slot in changed {
        if let Some(player) = region.players.get_mut(&id) {
            inventory::send_slot(player, &mut region.links, slot);
        }
    }
    // 官方在加经验之后才减 `count`。
    // Official decrements `count` after awarding the experience.
    region.entities.with_mut(uid, |entity| {
        let _ = take_orb(entity);
    });
}
