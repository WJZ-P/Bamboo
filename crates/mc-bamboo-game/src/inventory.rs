//! 创造模式物品栏。
//!
//! 物品栏由客户端决定，服务端只是一份镜像：客户端改的槽不回包，
//! 服务端自己改了（换手、中键）才发 `container_set_slot`。

use mc_bamboo_core::{BlockPos, Hand};
use mc_bamboo_data::{block_of_state, items, registries::data_component_type};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{ContainerSetSlot, EquipmentSlot, GameMode, SetHeldSlot},
};
use tracing::debug;

use crate::{
    inbound::PlayerId,
    interaction::{rules, within_reach},
    links::Links,
    player::Player,
    region::Region,
};

/// `InventoryMenu` 的槽位数。
const MENU_SLOTS: usize = 46;
/// 头盔的菜单槽位，盔甲、背包、快捷栏和副手从这里一直排到最后，官方 `Inventory` 迭代的就是这些槽。
const ARMOR_START: usize = 5;
/// 快捷栏第一格的菜单槽位。
const HOTBAR_START: usize = 36;
const OFF_HAND: usize = 45;
/// 快捷栏的格数。
const HOTBAR_SIZE: usize = 9;
/// 官方 `Inventory.items` 的长度：快捷栏和背包。
const INVENTORY_ITEMS: usize = 36;
/// 玩家物品栏的容器 ID。
const PLAYER_CONTAINER: i32 = 0;
/// `state_id` 回绕的掩码。
const STATE_ID_MASK: i32 = 32767;

/// 一名玩家的物品栏。`blocks-inventory.md` 8.3。
#[derive(Debug)]
pub(crate) struct Inventory {
    /// 下标是菜单槽位，`blocks-inventory.md` 8.1。
    pub(crate) slots: Vec<ItemStack>,
    /// 选中的快捷栏格，0 到 8。
    pub(crate) selected: u8,
    /// 每发一个 `container_set_slot` 加一。创造模式不发 `container_click`，所以客户端不会回传。
    state_id: i32,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: vec![ItemStack::empty(); MENU_SLOTS],
            selected: 0,
            state_id: 0,
        }
    }
}

impl Inventory {
    /// 这个装备槽现在的物品。主手是快捷栏选中的那格。玩家没有 `Body` 和 `Saddle` 槽。
    pub(crate) fn equipment(&self, slot: EquipmentSlot) -> Option<&ItemStack> {
        let index = match slot {
            EquipmentSlot::MainHand => self.hand_slot(Hand::Main),
            EquipmentSlot::OffHand => self.hand_slot(Hand::Off),
            EquipmentSlot::Feet => 8,
            EquipmentSlot::Legs => 7,
            EquipmentSlot::Chest => 6,
            EquipmentSlot::Head => 5,
            EquipmentSlot::Body | EquipmentSlot::Saddle => return None,
        };
        self.slots.get(index)
    }

    /// 这只手里的物品。
    pub(crate) fn held(&self, hand: Hand) -> &ItemStack {
        &self.slots[self.hand_slot(hand)]
    }

    /// 这只手的菜单槽位。
    fn hand_slot(&self, hand: Hand) -> usize {
        match hand {
            Hand::Main => HOTBAR_START + usize::from(self.selected),
            Hand::Off => OFF_HAND,
        }
    }

    fn next_state_id(&mut self) -> i32 {
        self.state_id = (self.state_id + 1) & STATE_ID_MASK;
        self.state_id
    }

    /// 中键选取，返回物品变了的菜单槽位，升序。`blocks-inventory.md` 8.4。
    fn pick(&mut self, item: i32) -> Vec<usize> {
        match self.find_plain_stack(item) {
            Some(index) if index < HOTBAR_SIZE => {
                self.select(index);
                Vec::new()
            }
            Some(index) => {
                let hotbar = self.select_suitable_hotbar_slot();
                self.slots.swap(hotbar, menu_slot(index));
                vec![menu_slot(index), hotbar]
            }
            None => self.add_and_pick(item),
        }
    }

    /// 选中的格子有东西时挪到第一个空槽，没有空槽就覆盖掉，和官方一样。
    fn add_and_pick(&mut self, item: i32) -> Vec<usize> {
        let hotbar = self.select_suitable_hotbar_slot();
        let mut changed = Vec::with_capacity(2);
        if !self.slots[hotbar].is_empty()
            && let Some(free) = self.first_free_slot()
        {
            self.slots.swap(hotbar, free);
            changed.push(free);
        }
        self.slots[hotbar] = ItemStack::new(item, 1);
        changed.push(hotbar);
        changed
    }

    fn select(&mut self, hotbar_index: usize) {
        self.selected = hotbar_index as u8;
    }

    /// 第一个不带组件的同种物品，下标是官方 `Inventory.items` 的下标。
    fn find_plain_stack(&self, item: i32) -> Option<usize> {
        (0..INVENTORY_ITEMS).find(|&index| {
            let stack = &self.slots[menu_slot(index)];
            !stack.is_empty()
                && stack.item == item
                && stack.added.is_empty()
                && stack.removed.is_empty()
        })
    }

    fn first_free_slot(&self) -> Option<usize> {
        (0..INVENTORY_ITEMS)
            .map(menu_slot)
            .find(|&slot| self.slots[slot].is_empty())
    }

    /// `Inventory.contains`：盔甲、背包、快捷栏或者副手里有不带组件的这种物品。
    fn contains_plain(&self, item: i32) -> bool {
        self.slots[ARMOR_START..].iter().any(|stack| {
            !stack.is_empty()
                && stack.item == item
                && stack.added.is_empty()
                && stack.removed.is_empty()
        })
    }

    /// 创造模式的 `ItemUtils.createFilledResult`：物品栏里没有时放进第一个空槽，返回那个菜单槽位。
    ///
    /// 舀起来的东西都不能叠放，所以不用找能叠上去的槽。没有空槽时官方直接丢掉它。
    fn add_filled(&mut self, item: i32) -> Option<usize> {
        if self.contains_plain(item) {
            return None;
        }
        let slot = self.first_free_slot()?;
        self.slots[slot] = ItemStack::new(item, 1);
        Some(slot)
    }

    /// 选中官方 `getSuitableHotbarSlot` 选的格子，返回它的菜单槽位：
    /// 从当前格往后第一个空格，没有空格时第一个没附魔的格子。
    fn select_suitable_hotbar_slot(&mut self) -> usize {
        let selected = usize::from(self.selected);
        let order = || (0..HOTBAR_SIZE).map(|offset| (selected + offset) % HOTBAR_SIZE);
        let stack = |index: usize| &self.slots[HOTBAR_START + index];
        let index = order()
            .find(|&index| stack(index).is_empty())
            .or_else(|| order().find(|&index| !is_enchanted(stack(index))))
            .unwrap_or(selected);
        self.select(index);
        HOTBAR_START + index
    }
}

/// 官方 `Inventory.items` 的下标换算成菜单槽位。
const fn menu_slot(index: usize) -> usize {
    if index < HOTBAR_SIZE {
        HOTBAR_START + index
    } else {
        index
    }
}

/// 官方的 `isEnchanted`。组件的值以附魔的个数开头，默认值是空的。
fn is_enchanted(stack: &ItemStack) -> bool {
    stack.added.iter().any(|(kind, value)| {
        *kind == data_component_type::ENCHANTMENTS && value.first() != Some(&0)
    })
}

/// 官方的 `count <= getMaxStackSize()`。空物品总是合法的。
///
/// 上限可以被组件改掉，这里不解析组件的值：改过上限的物品只受解码时 1 到 99 的限制。
fn count_is_valid(stack: &ItemStack) -> bool {
    stack.is_empty()
        || stack
            .added
            .iter()
            .any(|(kind, _)| *kind == data_component_type::MAX_STACK_SIZE)
        || rules::item(stack.item).is_some_and(|item| stack.count <= item.max_stack_size)
}

/// `set_carried_item`：选中了快捷栏的另一格。`blocks-inventory.md` 3.1。
pub(crate) fn on_set_carried_item(region: &mut Region, id: PlayerId, slot: i16) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    match usize::try_from(slot) {
        Ok(index) if index < HOTBAR_SIZE => player.inventory.select(index),
        _ => debug!(player = id.0, slot, "ignoring an invalid carried item slot"),
    }
}

/// `set_creative_mode_slot`：改了一个槽。`blocks-inventory.md` 8.2、8.3。
///
/// 不回包。负数的槽是丢到界面外，M2 没有物品实体，物品就这么没了，所以什么也不用做。
/// 槽 0 是合成结果，它和超出范围、数量超限的包一样静默忽略。
pub(crate) fn on_set_creative_mode_slot(
    region: &mut Region,
    id: PlayerId,
    slot: i16,
    stack: ItemStack,
) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    if player.game_mode != GameMode::Creative || !count_is_valid(&stack) {
        return;
    }
    if (1..MENU_SLOTS as i16).contains(&slot) {
        player.inventory.slots[slot as usize] = stack;
    }
}

/// Q 和 Ctrl+Q：客户端已经在本地扣掉了，这里跟上，不回包。`blocks-inventory.md` 8.3。
pub(crate) fn drop_held_item(region: &mut Region, id: PlayerId, all: bool) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let slot = player.inventory.hand_slot(Hand::Main);
    let stack = &mut player.inventory.slots[slot];
    stack.count -= 1;
    if all || stack.count <= 0 {
        *stack = ItemStack::empty();
    }
}

/// F：客户端不预测，所以要把两个槽发回去。`blocks-inventory.md` 8.3。
pub(crate) fn swap_hands(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let main_hand = player.inventory.hand_slot(Hand::Main);
    if player.inventory.slots[main_hand] == player.inventory.slots[OFF_HAND] {
        return;
    }
    player.inventory.slots.swap(main_hand, OFF_HAND);
    send_slot(player, &mut region.links, main_hand);
    send_slot(player, &mut region.links, OFF_HAND);
}

/// 用空桶舀起了 `item`。客户端在本地放进了同一格，官方在这一刻末尾把这一格同步过去，这里直接发。
pub(crate) fn add_filled_bucket(region: &mut Region, id: PlayerId, item: i32) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    if let Some(slot) = player.inventory.add_filled(item) {
        send_slot(player, &mut region.links, slot);
    }
}

/// 把一个槽发给物品的主人，组件只能发回给它。
fn send_slot(player: &mut Player, links: &mut Links, slot: usize) {
    let state_id = player.inventory.next_state_id();
    let packet = ContainerSetSlot {
        container_id: PLAYER_CONTAINER,
        state_id,
        slot: slot as i16,
        stack: &player.inventory.slots[slot],
    };
    links.send(player.id, &packet);
}

/// `pick_item_from_block`：中键选取方块。`blocks-inventory.md` 8.4。
///
/// 客户端不预测，这里不处理的话中键就没有效果。Ctrl+中键的 `include_data` 不用看，M2 当普通中键处理。
pub(crate) fn on_pick_item_from_block(region: &mut Region, id: PlayerId, pos: BlockPos) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    if !within_reach(player, pos) {
        return;
    }
    // 高度超出世界时官方也当作没加载。
    let Some(block) = region.chunks.block(pos).and_then(block_of_state) else {
        return;
    };
    // 水、火、传送门这些方块没有物品。
    if block.item == items::AIR {
        return;
    }

    let changed = player.inventory.pick(block.item);
    let slot = i32::from(player.inventory.selected);
    region.links.send(id, &SetHeldSlot { slot });
    for slot in changed {
        send_slot(player, &mut region.links, slot);
    }
}

#[cfg(test)]
mod tests;
