//! `EquipmentDispenseItemBehavior`。
//!
//! 前方一格的盒子里，第一个能穿的生物拿走一件。玩家的格子写进物品栏，并发给本人。
//! 附近的人靠原有的装备同步看到。穿不上时 `execute` 抛一次。南瓜和凋灵头只调用
//! [`equip`]，失败不抛。
//!
//! `EquipmentDispenseItemBehavior`.
//!
//! The first living entity in the cell in front that can wear the item takes one.
//! A player's slot is written into the inventory and sent to that player. Nearby viewers
//! see it through the existing equipment sync. `execute` throws once when nothing can wear it.
//! Pumpkins and wither skulls call [`equip`] only. A miss does not throw.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{registries::data_component_type as kinds, tags};
use mc_bamboo_protocol::{
    components::{ComponentValue, cursor::HolderSet, defaults},
    item::ItemStack,
};

use super::{Shot, throw_one};
use crate::{
    container::copy_with_count,
    dispenser::without_one,
    level::{Effect, EquipTarget},
    shape::Aabb,
    world::World,
};

pub(super) fn is_equippable(stack: &ItemStack) -> bool {
    equippable(stack).is_some()
}

/// `execute`：穿不上就 `super.execute`。
///
/// `execute`: a miss is `super.execute`.
pub(super) fn execute(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    mut stack: ItemStack,
) -> Shot {
    if equip(world, pos, facing, &mut stack) {
        Shot::plain(stack)
    } else {
        Shot::plain(throw_one(world, facing, stack))
    }
}

/// `dispenseEquipment`。成功时 `stack` 已经少了一个。
///
/// `dispenseEquipment`. On success `stack` has already lost one.
pub(super) fn equip(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: &mut ItemStack,
) -> bool {
    let Some(equippable) = equippable(stack) else {
        return false;
    };
    let target_pos = pos.relative(facing);
    let cell = block_box(target_pos);
    for target in world.dispenser_targets() {
        if !target.bounds.intersects(cell) || !can_equip(&target, &equippable) {
            continue;
        }
        let piece = copy_with_count(stack, 1);
        let Some(menu) = world.set_dispenser_equipment(target.entity_id, equippable.slot, piece)
        else {
            continue;
        };
        world.push_effect(Effect::DispenserEquipped {
            entity_id: target.entity_id,
            menu_slot: menu,
        });
        *stack = without_one(stack);
        return true;
    }
    false
}

fn block_box(pos: BlockPos) -> Aabb {
    Aabb::new(
        f64::from(pos.x),
        f64::from(pos.y),
        f64::from(pos.z),
        f64::from(pos.x) + 1.0,
        f64::from(pos.y) + 1.0,
        f64::from(pos.z) + 1.0,
    )
}

fn equippable(stack: &ItemStack) -> Option<mc_bamboo_protocol::components::value::Equippable> {
    match defaults::resolve(stack.item, &stack.patch(), kinds::EQUIPPABLE) {
        Some(ComponentValue::Equippable(value)) => Some(value.clone()),
        _ => None,
    }
}

/// `LivingEntity.canEquipWithDispenser`。`canUseSlot` 对玩家是真，没有这一格的实体除外。
///
/// `LivingEntity.canEquipWithDispenser`. `canUseSlot` is true for a player, except a slot
/// the entity does not have.
fn can_equip(
    target: &EquipTarget,
    equippable: &mc_bamboo_protocol::components::value::Equippable,
) -> bool {
    if target.spectator || !equippable.dispensable {
        return false;
    }
    let Some(slot) = usize::try_from(equippable.slot).ok() else {
        return false;
    };
    let Some(worn) = target.equipment.get(slot).and_then(Option::as_ref) else {
        return false;
    };
    if !worn.is_empty() {
        return false;
    }
    allowed(equippable.allowed_entities.as_ref(), target.entity_type)
}

/// `Equippable.canBeEquippedBy`。允许名单空着就是谁都能穿。
///
/// `Equippable.canBeEquippedBy`. An empty allow-list means every entity type.
fn allowed(set: Option<&HolderSet>, entity_type: i32) -> bool {
    let Some(set) = set else {
        return true;
    };
    match set {
        HolderSet::Ids(ids) => ids.contains(&entity_type),
        HolderSet::Tag(name) => entity_tag(name).is_some_and(|ids| ids.contains(&entity_type)),
    }
}

fn entity_tag(name: &str) -> Option<&'static [i32]> {
    tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == "minecraft:entity_type")
        .and_then(|group| group.tags.iter().find(|tag| tag.name == name))
        .map(|tag| tag.ids)
}
