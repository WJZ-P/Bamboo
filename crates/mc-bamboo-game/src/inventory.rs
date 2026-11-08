//! 创造模式物品栏。
//!
//! 物品栏由客户端决定，服务端只是一份镜像：客户端改的槽不回包，
//! 服务端自己改了（换手、中键）才发 `container_set_slot`。

use mc_bamboo_blocks::{BlockState, ItemExchange, wash_dyed_color};
use mc_bamboo_core::{BlockPos, Hand};
use mc_bamboo_data::{block_of_state, registries::data_component_type};
use mc_bamboo_protocol::{
    components::{ComponentValue, defaults},
    item::ItemStack,
    packet::play::{ContainerSetSlot, EquipmentSlot, GameMode, SetHeldSlot},
};
use tracing::debug;

use crate::{
    block_items, inbound::PlayerId, interaction::within_reach, links::Links, player::Player,
    region::Region,
};

/// `InventoryMenu` 的槽位数。
const MENU_SLOTS: usize = 46;
/// 头盔的菜单槽位，盔甲、背包、快捷栏和副手从这里一直排到最后，官方 `Inventory` 迭代的就是这些槽。
const ARMOR_START: usize = 5;
/// 快捷栏第一格的菜单槽位。
pub(crate) const HOTBAR_START: usize = 36;
pub(crate) const OFF_HAND: usize = 45;
/// 快捷栏的格数。
const HOTBAR_SIZE: usize = 9;
/// 官方 `Inventory.items` 的长度：快捷栏和背包。
const INVENTORY_ITEMS: usize = 36;
/// 官方 `Inventory.SLOT_OFFHAND`。副手不在 `items` 里。
///
/// `Inventory.SLOT_OFFHAND`. The off hand is not part of `items`.
const OFFHAND_INDEX: usize = 40;
/// 官方 `Container.getMaxStackSize` 的默认值。
///
/// The default `Container.getMaxStackSize`.
const CONTAINER_STACK_LIMIT: i32 = 99;
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
    fn pick(&mut self, stack: &ItemStack) -> Vec<usize> {
        match self.find_slot_matching_item(stack) {
            Some(index) if index < HOTBAR_SIZE => {
                self.select(index);
                Vec::new()
            }
            Some(index) => {
                let hotbar = self.select_suitable_hotbar_slot();
                self.slots.swap(hotbar, menu_slot(index));
                vec![menu_slot(index), hotbar]
            }
            None => self.add_and_pick(stack),
        }
    }

    /// 选中的格子有东西时挪到第一个空槽，没有空槽就覆盖掉，和官方一样。
    fn add_and_pick(&mut self, stack: &ItemStack) -> Vec<usize> {
        let hotbar = self.select_suitable_hotbar_slot();
        let mut changed = Vec::with_capacity(2);
        if !self.slots[hotbar].is_empty()
            && let Some(free) = self.first_free_slot()
        {
            self.slots.swap(hotbar, free);
            changed.push(free);
        }
        self.slots[hotbar] = stack.clone();
        changed.push(hotbar);
        changed
    }

    fn select(&mut self, hotbar_index: usize) {
        self.selected = hotbar_index as u8;
    }

    /// 官方 `Inventory.findSlotMatchingItem`：第一个同物品、同组件的格子。
    ///
    /// 下标是官方 `Inventory.items` 的下标。
    fn find_slot_matching_item(&self, stack: &ItemStack) -> Option<usize> {
        (0..INVENTORY_ITEMS).find(|&index| {
            let candidate = &self.slots[menu_slot(index)];
            !candidate.is_empty() && candidate.is_same_item_same_components(stack)
        })
    }

    fn first_free_slot(&self) -> Option<usize> {
        (0..INVENTORY_ITEMS)
            .map(menu_slot)
            .find(|&slot| self.slots[slot].is_empty())
    }

    /// `Inventory.contains`：盔甲、背包、快捷栏或者副手里有同物品、同组件的东西。
    fn contains(&self, stack: &ItemStack) -> bool {
        self.slots[ARMOR_START..]
            .iter()
            .any(|candidate| !candidate.is_empty() && candidate.is_same_item_same_components(stack))
    }

    /// 创造模式的 `ItemUtils.createFilledResult`：物品栏里没有时放进第一个空槽，返回那个菜单槽位。
    ///
    /// 舀起来的东西都不能叠放，所以不用找能叠上去的槽。没有空槽时官方直接丢掉它。
    fn add_filled(&mut self, item: i32) -> Option<usize> {
        let stack = ItemStack::new(item, 1);
        if self.contains(&stack) {
            return None;
        }
        let slot = self.first_free_slot()?;
        self.slots[slot] = stack;
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

    /// 官方 `ItemUtils.createFilledResult` 和洗染料的 `remove`。
    ///
    /// 返回物品变了的菜单槽位。生存模式放不下的那一份没有物品实体，直接消失。
    ///
    /// `ItemUtils.createFilledResult`, and `remove` for dyed items.
    ///
    /// Returns the menu slots that changed. A survival remainder has no item entity, so it vanishes.
    fn apply_exchange(
        &mut self,
        hand: Hand,
        exchange: &ItemExchange,
        creative: bool,
    ) -> Vec<usize> {
        let before = self.slots.clone();
        match exchange {
            ItemExchange::RemoveDyedColor => {
                let slot = self.hand_slot(hand);
                wash_dyed_color(&mut self.slots[slot]);
            }
            ItemExchange::Fill {
                replacement,
                limit_creative,
            } => self.fill_hand(hand, replacement.clone(), creative, *limit_creative),
        }
        self.slots
            .iter()
            .zip(&before)
            .enumerate()
            .filter(|(_, (now, was))| *now != *was)
            .map(|(slot, _)| slot)
            .collect()
    }

    /// 官方 `createFilledResult`：先处理创造模式的上限，再消耗手里的一个。
    ///
    /// `createFilledResult`: apply the creative limit, then consume one from the hand.
    fn fill_hand(
        &mut self,
        hand: Hand,
        replacement: ItemStack,
        creative: bool,
        limit_creative: bool,
    ) {
        if limit_creative && creative {
            if !self.contains(&replacement) {
                self.add(replacement, true);
            }
            return;
        }
        let menu = self.hand_slot(hand);
        if !creative {
            // 官方 `ItemStack.consume`：没有无限材料才减数量。
            // `ItemStack.consume`: the count shrinks only without infinite materials.
            let stack = &mut self.slots[menu];
            stack.count -= 1;
            if stack.is_empty() {
                *stack = ItemStack::empty();
            }
        }
        if self.slots[menu].is_empty() {
            self.slots[menu] = replacement;
        } else {
            self.add(replacement, creative);
        }
    }

    /// 官方 `Inventory.add(ItemStack)`，也就是 `add(-1, stack)`。
    ///
    /// `Inventory.add(ItemStack)`, which is `add(-1, stack)`.
    fn add(&mut self, mut incoming: ItemStack, creative: bool) -> bool {
        if incoming.is_empty() {
            return false;
        }
        if incoming.is_damaged() {
            return self.add_damaged(incoming, creative);
        }
        let mut last;
        loop {
            last = incoming.count;
            let left = self.add_resource(&incoming);
            incoming.count = left;
            if incoming.is_empty() || incoming.count >= last {
                break;
            }
        }
        if incoming.count == last && creative {
            true
        } else {
            incoming.count < last
        }
    }

    /// 损坏的物品只进空格子，不和别的堆叠。
    ///
    /// A damaged stack only goes into an empty slot. It does not stack.
    fn add_damaged(&mut self, incoming: ItemStack, creative: bool) -> bool {
        match self.free_slot() {
            Some(index) => {
                let menu = self.menu_index(index);
                self.slots[menu] = incoming;
                true
            }
            None if creative => true,
            None => false,
        }
    }

    /// 官方 `addResource(ItemStack)`：先找还能叠的格子，没有再找空格子。
    ///
    /// `addResource(ItemStack)`: a slot with room first, otherwise an empty slot.
    fn add_resource(&mut self, incoming: &ItemStack) -> i32 {
        let index = self.slot_with_space(incoming).or_else(|| self.free_slot());
        match index {
            Some(index) => self.add_resource_to(index, incoming),
            None => incoming.count,
        }
    }

    /// 官方 `addResource(int, ItemStack)`。
    ///
    /// `addResource(int, ItemStack)`.
    fn add_resource_to(&mut self, index: usize, incoming: &ItemStack) -> i32 {
        let count = incoming.count;
        let menu = self.menu_index(index);
        let limit = if self.slots[menu].is_empty() {
            inventory_max(incoming)
        } else {
            inventory_max(&self.slots[menu])
        };
        let to_add = count.min(limit - self.slots[menu].count);
        if to_add == 0 {
            return count;
        }
        if self.slots[menu].is_empty() {
            let mut placed = incoming.clone();
            placed.count = 0;
            self.slots[menu] = placed;
        }
        self.slots[menu].count += to_add;
        count - to_add
    }

    /// 官方 `getSlotWithRemainingSpace`：选中格、副手，再从头扫 `items`。
    ///
    /// `getSlotWithRemainingSpace`: the selected slot, the off hand, then `items` from the start.
    fn slot_with_space(&self, incoming: &ItemStack) -> Option<usize> {
        let selected = usize::from(self.selected);
        if has_space(self.stack_at(selected), incoming) {
            return Some(selected);
        }
        if has_space(self.stack_at(OFFHAND_INDEX), incoming) {
            return Some(OFFHAND_INDEX);
        }
        (0..INVENTORY_ITEMS).find(|&index| has_space(self.stack_at(index), incoming))
    }

    /// 官方 `getFreeSlot`：只扫 `items`，不放进盔甲或副手。
    ///
    /// `getFreeSlot`: only `items`. Armour and the off hand are not free slots.
    fn free_slot(&self) -> Option<usize> {
        (0..INVENTORY_ITEMS).find(|&index| self.stack_at(index).is_empty())
    }

    fn stack_at(&self, index: usize) -> &ItemStack {
        &self.slots[self.menu_index(index)]
    }

    /// `items` 的下标，或者副手的 40，换成菜单槽位。
    ///
    /// An `items` index, or off-hand index 40, as a menu slot.
    fn menu_index(&self, index: usize) -> usize {
        if index == OFFHAND_INDEX {
            OFF_HAND
        } else {
            menu_slot(index)
        }
    }
}

/// 官方 `Inventory.getMaxStackSize(ItemStack)`：容器上限和物品上限里较小的那个。
///
/// `Inventory.getMaxStackSize(ItemStack)`: the smaller of the container limit and the item limit.
fn inventory_max(stack: &ItemStack) -> i32 {
    stack.max_stack_size().min(CONTAINER_STACK_LIMIT)
}

/// 官方 `hasRemainingSpaceForItem`。
///
/// `hasRemainingSpaceForItem`.
fn has_space(slot: &ItemStack, incoming: &ItemStack) -> bool {
    !slot.is_empty()
        && slot.is_same_item_same_components(incoming)
        && slot.is_stackable()
        && slot.count < inventory_max(slot)
}

/// 官方 `Inventory.items` 的下标换算成菜单槽位。
pub(crate) const fn menu_slot(index: usize) -> usize {
    if index < HOTBAR_SIZE {
        HOTBAR_START + index
    } else {
        index
    }
}

/// 官方 `ItemStack.isEnchanted`：解析好的 `enchantments` 组件不是空的。
fn is_enchanted(stack: &ItemStack) -> bool {
    matches!(
        defaults::resolve(stack.item, &stack.patch(), data_component_type::ENCHANTMENTS),
        Some(ComponentValue::Enchantments(levels)) if !levels.is_empty()
    )
}

/// 官方 `handleSetCreativeModeSlot` 里的 `validData`：`count <= getMaxStackSize()`。
///
/// 上限看解析好的组件，所以改过 `max_stack_size` 的物品按新上限算。
fn count_is_valid(stack: &ItemStack) -> bool {
    stack.is_empty() || stack.count <= stack.max_stack_size()
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

/// 炼药锅记下的物品交换。创造模式就是官方的 `hasInfiniteMaterials`。
///
/// Apply a cauldron's item exchange. Creative mode is vanilla `hasInfiniteMaterials`.
pub(crate) fn apply_item_exchange(
    region: &mut Region,
    id: PlayerId,
    hand: Hand,
    exchange: &ItemExchange,
) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let creative = player.game_mode == GameMode::Creative;
    let changed = player.inventory.apply_exchange(hand, exchange, creative);
    for slot in changed {
        send_slot(player, &mut region.links, slot);
    }
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

/// 把一个槽发给这名玩家。组件原样发：这些字节本来就是它的客户端发来的，或者来自服务端
/// 自己的数据，见 [`ItemStack::write`]。
pub(crate) fn send_slot(player: &mut Player, links: &mut Links, slot: usize) {
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
/// 客户端不预测，这里不处理的话中键就没有效果。`include_data` 是 Ctrl+中键，官方只在创造模式
/// 下看它，为真时物品还带上方块实体的数据，见 [`block_items::add_block_data`]。
pub(crate) fn on_pick_item_from_block(
    region: &mut Region,
    id: PlayerId,
    pos: BlockPos,
    include_data: bool,
) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if !within_reach(player, pos) {
        return;
    }
    // 官方的 `hasInfiniteMaterials`，也就是创造模式。
    let include_data = include_data && player.game_mode == GameMode::Creative;
    // 高度超出世界时官方也当作没加载。状态 ID 不在表里同样当没加载。
    let Some(state_id) = region
        .chunks
        .block(pos)
        .filter(|state| block_of_state(*state).is_some())
    else {
        return;
    };
    // 官方先 `getCloneItemStack`，空物品堆（水、火、传送门、移动中的活塞）就到此为止。
    let mut stack = clone::clone_item_stack(region, pos, BlockState(state_id), include_data);
    if stack.is_empty() {
        return;
    }
    if include_data {
        block_items::add_block_data(region, pos, &mut stack);
    }

    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let changed = player.inventory.pick(&stack);
    let slot = i32::from(player.inventory.selected);
    region.links.send(id, &SetHeldSlot { slot });
    for slot in changed {
        send_slot(player, &mut region.links, slot);
    }
}

mod clone;

#[cfg(test)]
mod exchange;
#[cfg(test)]
mod tests;
