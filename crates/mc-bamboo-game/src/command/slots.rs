//! `/execute if|unless items|slots` 和实体子谓词 `minecraft:slots` 共用的槽位视图。
//!
//! 官方 `ExecuteCommand.countItems` / `countSlots` 把一个 `SlotSource` 交给
//! `ItemAccessor`，由实体或方块实体的 `SlotProvider.getSlot(int)` 取出非空槽，
//! 再数个数或把命中物品谓词的物品数量加起来。`SlotsPredicate.matches` 同样按
//! `SlotProvider.getSlot` 逐格问。这里用「槽位编号 → 物品」的映射表达这个视图：
//! 在表里＝`getSlot` 返回非 null（物品可以是空的），不在表里＝返回 null、跳过。
//!
//! The game's slot view shared by `/execute if|unless items|slots` and the
//! `minecraft:slots` entity sub-predicate. Official `countItems`/`countSlots` hand a
//! `SlotSource` to an `ItemAccessor`, whose `SlotProvider.getSlot(int)` yields the
//! non-null slots; `SlotsPredicate.matches` queries the same provider per slot. Here
//! a `slot id -> stack` map models it: present means `getSlot` returned non-null
//! (the stack may be empty), absent means it returned null and is skipped.

use std::collections::BTreeMap;

use mc_bamboo_blocks::entity::KindPublic;
use mc_bamboo_command::ItemPredicateInput;
use mc_bamboo_core::Hand;
use mc_bamboo_loot::LootStack;
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_protocol::packet::play::EquipmentSlot as WireSlot;

use crate::{
    command::select::Target, interaction::level::game_stack_to_loot, inventory::menu_slot,
    region::Region,
};

/// `LivingEntity.getEquipmentSlot` 认得的八个装备槽编号；生物 / 玩家都有这些槽。
///
/// The eight equipment slot ids recognised by `LivingEntity.getEquipmentSlot`;
/// both mobs and players expose them.
const EQUIPMENT_IDS: [i32; 8] = [98, 99, 100, 101, 102, 103, 105, 106];

/// `EquipmentSlot` 到 `SlotProvider` 的槽位编号，和 `SlotRanges` 的装备项一致。
fn slot_id(slot: WireSlot) -> i32 {
    match slot {
        WireSlot::MainHand => 98,
        WireSlot::OffHand => 99,
        WireSlot::Feet => 100,
        WireSlot::Legs => 101,
        WireSlot::Chest => 102,
        WireSlot::Head => 103,
        WireSlot::Body => 105,
        WireSlot::Saddle => 106,
    }
}

/// `SlotSourceArgument` 的字面量范围名：`SlotRanges.nameToIds` 查表。
/// 非字面量（data-pack 里的具名 `SlotSource`）本实现没有，返回 `None`。
///
/// A literal `SlotSourceArgument` range name, looked up like
/// `SlotRanges.nameToIds`. Named data-pack `SlotSource`s are not modelled, so
/// anything else returns `None`.
pub fn literal_slots(text: &str) -> Option<&'static [i32]> {
    mc_bamboo_data::slot_ranges::slots_of(text)
}

/// `countSlots`：`getSlotsFromRange` 之后 `SlotCollection.size()`，即范围内存在的槽数。
///
/// `countSlots`: `SlotCollection.size()` after `getSlotsFromRange`, i.e. the number
/// of existing slots in the range.
pub fn count_slots(slots: &BTreeMap<i32, LootStack>, ids: &[i32]) -> i32 {
    ids.iter().filter(|id| slots.contains_key(id)).count() as i32
}

/// `countItems`：范围内存在的槽里，命中 `ItemPredicateArgument` 的物品数量之和。
///
/// `countItems`: sum of `ItemStack.getCount()` over the existing slots in the range
/// whose stack matches the `ItemPredicateArgument`.
pub fn count_items(
    slots: &BTreeMap<i32, LootStack>,
    ids: &[i32],
    predicate: &ItemPredicateInput,
) -> i32 {
    ids.iter()
        .filter_map(|id| slots.get(id))
        .filter(|stack| item_matches(predicate, stack))
        .map(|stack| stack.count)
        .sum()
}

/// `ItemPredicateArgument` 的结果只认物品或物品标签；用物品注册表 ID 直接比。
///
/// The `ItemPredicateArgument` result only recognises an item or an item tag;
/// compare by item registry id directly.
fn item_matches(predicate: &ItemPredicateInput, stack: &LootStack) -> bool {
    match predicate {
        ItemPredicateInput::Item(expected) => mc_bamboo_data::items::ITEMS
            .iter()
            .position(|item| item.name == expected.name)
            .is_some_and(|id| id as i32 == stack.item),
        ItemPredicateInput::Tag { item_ids, .. } => item_ids.contains(&stack.item),
    }
}

/// 一个命令目标的 `SlotProvider` 视图。
///
/// The `SlotProvider` view of one command target.
pub fn entity_slots(region: &Region, target: Target) -> BTreeMap<i32, LootStack> {
    match target {
        Target::Player(id) => player_slots(region, id),
        Target::World(uid) => world_slots(region, uid),
    }
}

/// `Player.getSlot` 的覆盖：背包、末影箱、合成、光标和装备槽。
///
/// The `Player.getSlot` overrides: inventory, ender chest, crafting, cursor and
/// equipment slots.
fn player_slots(region: &Region, id: crate::inbound::PlayerId) -> BTreeMap<i32, LootStack> {
    let Some(player) = region.players.get(&id) else {
        return BTreeMap::new();
    };
    let inv = &player.inventory;
    let mut out = BTreeMap::new();
    // `Inventory.items` 的 36 格，`menu_slot` 换算成菜单下标。
    // The 36 `Inventory.items` slots, mapped through `menu_slot`.
    for slot in 0..36 {
        out.insert(slot as i32, game_stack_to_loot(&inv.slots[menu_slot(slot)]));
    }
    out.insert(98, game_stack_to_loot(inv.held(Hand::Main)));
    out.insert(99, game_stack_to_loot(inv.held(Hand::Off)));
    for (id, wire) in [
        (100, WireSlot::Feet),
        (101, WireSlot::Legs),
        (102, WireSlot::Chest),
        (103, WireSlot::Head),
    ] {
        let stack = inv
            .equipment(wire)
            .cloned()
            .unwrap_or_else(ItemStack::empty);
        out.insert(id, game_stack_to_loot(&stack));
    }
    // 玩家没有 Body / Saddle 装备槽，但 `LivingEntity.getSlot` 仍返回一格空的。
    // Players have no body/saddle equipment, but `LivingEntity.getSlot` still
    // returns an empty slot for those ids.
    out.insert(105, LootStack::empty());
    out.insert(106, LootStack::empty());
    for (index, stack) in player.ender_chest.slots.iter().take(27).enumerate() {
        out.insert(200 + index as i32, game_stack_to_loot(stack));
    }
    // `InventoryMenu.getCraftSlots()`：菜单槽 1..4。
    // `InventoryMenu.getCraftSlots()`: menu slots 1..4.
    for index in 0..4 {
        out.insert(
            500 + index,
            game_stack_to_loot(&inv.slots[1 + index as usize]),
        );
    }
    out
}

/// 世界实体的 `SlotProvider`：生物按 `LivingEntity.getSlot` 给出八个装备槽，
/// `mob.equipment` 里非空的那几格覆盖进去；其它实体本实现还没有槽位数据。
///
/// The world entity's `SlotProvider`: mobs expose the eight `LivingEntity.getSlot`
/// equipment slots, with the non-empty ones from `mob.equipment` filled in; other
/// entities have no slot data here yet.
fn world_slots(region: &Region, uid: i32) -> BTreeMap<i32, LootStack> {
    let Some(public) = region.entities.public_at(uid) else {
        return BTreeMap::new();
    };
    let KindPublic::Mob(mob) = &public.kind else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for id in EQUIPMENT_IDS {
        out.insert(id, LootStack::empty());
    }
    for (slot, stack) in &mob.equipment {
        out.insert(slot_id(*slot), game_stack_to_loot(stack));
    }
    out
}
