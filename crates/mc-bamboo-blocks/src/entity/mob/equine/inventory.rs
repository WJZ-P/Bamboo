//! 坐骑物品栏（官方 `HorseInventoryMenu`）里属于生物的那一半：能不能打开、
//! 箱子几列、鞍和身体两格收什么、玩家放进去时的 `ArmorSlot.setByPlayer`。
//! 菜单本身（点击、同步）在游戏层。
//!
//! The mob's half of the mount inventory (vanilla `HorseInventoryMenu`): whether
//! it opens, how many chest columns it has, what the saddle and body slots accept,
//! and `ArmorSlot.setByPlayer` when a player puts something in. The menu itself
//! (clicks, sync) lives in the game layer.

use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{Mob, MobType, interact};
use crate::entity::{Entity, mob::equipment::GUARANTEED_DROP_CHANCE};

/// `AbstractHorse.openCustomInventoryScreen` 的地面分支：没人骑且已驯服。
/// 骑乘中按物品栏键由游戏层直接判 `hasPassenger(player) && isTamed()`。
///
/// The ground branch of `openCustomInventoryScreen`: not ridden and tamed.
/// Opening while riding checks `hasPassenger(player) && isTamed()` in the game layer.
pub fn can_open(mob: &Mob) -> bool {
    if crate::entity::mob::nautilus::is_nautilus(mob.mob_type) {
        // `AbstractNautilus.openCustomInventoryScreen`：没被别的生物骑、已驯服，
        // 且（有人骑时）骑手就是这名玩家。
        //
        // `AbstractNautilus.openCustomInventoryScreen`: not ridden by another mob,
        // tame, and (if ridden) ridden by this player.
        return !mob.is_vehicle() || mob.tame.is_tame();
    }
    !mob.is_vehicle() && mob.equine.as_ref().is_some_and(|state| state.tame)
}

/// `getInventoryColumns`：马 0；驴、骡有箱子时 5；羊驼有箱子时等于力量。
///
/// `getInventoryColumns`: 0 for horses; 5 for chested donkeys and mules; the
/// strength for chested llamas.
pub fn columns(mob: &Mob) -> i32 {
    if crate::entity::mob::nautilus::is_nautilus(mob.mob_type) {
        return crate::entity::mob::nautilus::inventory_columns(mob);
    }
    let Some(state) = &mob.equine else {
        return 0;
    };
    if !state.chested {
        return 0;
    }
    match mob.mob_type {
        MobType::Donkey | MobType::Mule => 5,
        MobType::Llama | MobType::TraderLlama => state.strength,
        _ => 0,
    }
}

/// 鞍位的 `canUseSlot`，菜单的 `mayPlace` 要用。身体位恒真。
///
/// `canUseSlot` for the saddle, needed by the menu's `mayPlace`. The body slot is
/// always usable.
pub fn saddle_usable(mob: &Mob) -> bool {
    interact::can_use_slot(mob, EquipmentSlot::Saddle)
}

/// `ArmorSlot.mayPlace` → `LivingEntity.isEquippableInSlot`：物品的可装备组件指向这一格、
/// 这一格能用、实体类型允许。没有组件的物品只能进主手，所以这两格都不收。
///
/// `ArmorSlot.mayPlace` → `isEquippableInSlot`: the equippable component names this
/// slot, the slot is usable and the entity type is allowed. Items without the
/// component only fit the main hand, so neither slot takes them.
pub fn accepts(kind: MobType, saddle_usable: bool, slot: EquipmentSlot, stack: &ItemStack) -> bool {
    let Some(equippable) = interact::equippable(stack) else {
        return false;
    };
    let usable = slot != EquipmentSlot::Saddle || saddle_usable;
    interact::slot_of(&equippable) == Some(slot)
        && usable
        && crate::dispense::equippable_allows(
            equippable.allowed_entities.as_ref(),
            kind.entity_type(),
        )
}

/// `ArmorSlot.setByPlayer`：先 `onEquipItem(slot, previous, stack)`，再经
/// `createEquipmentSlotContainer` 的 `setItem` → `setItemSlot`（又一次 `onEquipItem`），
/// 非空时 `setGuaranteedDrop` 和 `setPersistenceRequired`。所以从菜单穿上会出两次声、
/// 取两次 `nextLong`。
///
/// `ArmorSlot.setByPlayer`: `onEquipItem(slot, previous, stack)`, then the
/// equipment container's `setItem` → `setItemSlot` (a second `onEquipItem`), and
/// for a non-empty stack `setGuaranteedDrop` plus `setPersistenceRequired`. Equipping
/// from the menu therefore plays the sound twice and draws `nextLong` twice.
pub fn set_by_player(
    me: &mut Entity,
    mob: &mut Mob,
    slot: EquipmentSlot,
    previous: &ItemStack,
    stack: &ItemStack,
) {
    interact::on_equip_item(me, mob, slot, previous, stack);
    let old = mob.equipment.get(slot).clone();
    mob.equipment.set(slot, stack.clone());
    interact::on_equip_item(me, mob, slot, &old, stack);
    if !stack.is_empty() {
        mob.equipment.set_drop_chance(slot, GUARANTEED_DROP_CHANCE);
        mob.persistence_required = true;
    }
}

/// 箱子格的 `SimpleContainer.setItem`：只换内容，不出声不取数。
///
/// `SimpleContainer.setItem` for the chest slots: swap the contents only, with no
/// sound and no random draws.
pub fn set_chest_items(mob: &mut Mob, items: &[ItemStack]) {
    if let Some(state) = &mut mob.equine
        && state.chest_items.len() == items.len()
    {
        state.chest_items.clone_from_slice(items);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Vec3;
    use mc_bamboo_data::items;

    use super::*;
    use crate::entity::mob::MobEvent;

    fn sounds(mob: &Mob) -> Vec<&'static str> {
        mob.events
            .iter()
            .filter_map(|event| match event {
                MobEvent::Sound { sound, .. } => Some(*sound),
                _ => None,
            })
            .collect()
    }

    /// 只有驯服的才开；列数照官方；鞍位要驯服的成年驴，马的鞍位恒可用。
    ///
    /// Only tamed mounts open; columns follow vanilla; a donkey's saddle slot needs
    /// a tamed adult while a horse's is always usable.
    #[test]
    fn opening_columns_and_slot_rules_follow_vanilla() {
        let mut donkey = Mob::new(MobType::Donkey);
        assert!(!can_open(&donkey));
        assert!(!saddle_usable(&donkey));
        assert_eq!(columns(&donkey), 0);
        let state = donkey.equine.as_mut().expect("donkey state");
        state.tame = true;
        state.chested = true;
        assert!(can_open(&donkey));
        assert!(saddle_usable(&donkey));
        assert_eq!(columns(&donkey), 5);
        let mut llama = Mob::new(MobType::Llama);
        let state = llama.equine.as_mut().expect("llama state");
        state.chested = true;
        state.strength = 3;
        assert_eq!(columns(&llama), 3);
        assert_eq!(columns(&Mob::new(MobType::Horse)), 0);
        let saddle = ItemStack::new(items::SADDLE, 1);
        let armor = ItemStack::new(items::IRON_HORSE_ARMOR, 1);
        let carpet = ItemStack::new(items::WHITE_CARPET, 1);
        assert!(accepts(
            MobType::Horse,
            true,
            EquipmentSlot::Saddle,
            &saddle
        ));
        assert!(!accepts(
            MobType::Donkey,
            false,
            EquipmentSlot::Saddle,
            &saddle
        ));
        assert!(accepts(MobType::Horse, true, EquipmentSlot::Body, &armor));
        assert!(!accepts(MobType::Donkey, true, EquipmentSlot::Body, &armor));
        assert!(!accepts(MobType::Horse, true, EquipmentSlot::Body, &carpet));
        assert!(accepts(MobType::Llama, true, EquipmentSlot::Body, &carpet));
        assert!(!accepts(MobType::Horse, true, EquipmentSlot::Body, &saddle));
    }

    /// 从菜单穿鞍：两次 `onEquipItem` 各出一次声、各取一次 `nextLong`，鞍必掉；
    /// 拿下来不出声不取数。
    ///
    /// Saddling from the menu: two `onEquipItem` calls, each with a sound and a
    /// `nextLong`, and the saddle becomes a guaranteed drop; removing it is silent.
    #[test]
    fn menu_saddling_equips_twice_and_removal_is_silent() {
        let mut entity = Entity::by_mob_type(MobType::Horse, Vec3::new(0.5, 64.0, 0.5));
        entity
            .with_mob(|me, mob| {
                let mut expected = me.random;
                let saddle = ItemStack::new(items::SADDLE, 1);
                set_by_player(me, mob, EquipmentSlot::Saddle, &ItemStack::empty(), &saddle);
                expected.next_long();
                expected.next_long();
                assert_eq!(me.random.next_long(), expected.next_long());
                assert_eq!(mob.equipment.get(EquipmentSlot::Saddle), &saddle);
                assert!(mob.persistence_required);
                assert_eq!(
                    sounds(mob),
                    [
                        "minecraft:entity.horse.saddle",
                        "minecraft:entity.horse.saddle"
                    ]
                );
                mob.events.clear();
                set_by_player(me, mob, EquipmentSlot::Saddle, &saddle, &ItemStack::empty());
                assert!(mob.equipment.get(EquipmentSlot::Saddle).is_empty());
                assert!(sounds(mob).is_empty());
            })
            .expect("horse row");
    }
}
