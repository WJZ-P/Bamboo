//! 创造模式物品栏。
//!
//! 物品栏由客户端决定，服务端只是一份镜像：客户端改的槽不回包，
//! 服务端自己改了（换手、中键）才发 `container_set_slot`。

use mc_bamboo_blocks::{BlockState, ItemExchange, wash_dyed_color};
use mc_bamboo_core::{BlockPos, Hand};
use mc_bamboo_data::{block_of_state, items, registries::data_component_type};
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
    pub(crate) fn hand_slot(&self, hand: Hand) -> usize {
        match hand {
            Hand::Main => HOTBAR_START + usize::from(self.selected),
            Hand::Off => OFF_HAND,
        }
    }

    fn next_state_id(&mut self) -> i32 {
        self.state_id = (self.state_id + 1) & STATE_ID_MASK;
        self.state_id
    }

    /// 物品栏菜单（容器 ID 0）的 `state_id`。合成和 `send_slot` 共用这一份。
    ///
    /// `state_id` of the inventory menu (container id 0). Crafting and `send_slot` share it.
    pub(crate) fn state_id(&self) -> i32 {
        self.state_id
    }

    /// 见 [`Inventory::state_id`]。
    ///
    /// See [`Inventory::state_id`].
    pub(crate) fn set_state_id(&mut self, state_id: i32) {
        self.state_id = state_id;
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

    /// 创造模式的 `Inventory.add`：物品栏里已经有同样的结果就不再加。
    /// 放不下时清掉剩下的，调用方看起来像是成功了，官方并不生成掉落。
    ///
    /// Creative `Inventory.add`: skip a result the inventory already holds.
    /// When nothing fits, the leftover is cleared. Vanilla does not spawn a drop.
    #[cfg(test)]
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
    /// 返回物品变了的菜单槽位，以及生存模式放不下、要按 `Player.drop` 丢掉的那一堆。
    /// 创造模式放不下时这一堆是空的：官方 `Inventory.add` 会把剩下的清掉。
    ///
    /// `ItemUtils.createFilledResult`, and `remove` for dyed items.
    ///
    /// Returns the menu slots that changed, and the stack a survival player must `Player.drop`.
    /// Creative mode returns an empty stack: `Inventory.add` clears what does not fit.
    fn apply_exchange(
        &mut self,
        hand: Hand,
        exchange: &ItemExchange,
        creative: bool,
    ) -> (Vec<usize>, ItemStack) {
        let before = self.slots.clone();
        let leftover = match exchange {
            ItemExchange::RemoveDyedColor => {
                let slot = self.hand_slot(hand);
                wash_dyed_color(&mut self.slots[slot]);
                ItemStack::empty()
            }
            ItemExchange::Fill {
                replacement,
                limit_creative,
            } => self.fill_hand(hand, replacement.clone(), creative, *limit_creative),
            ItemExchange::AddOrDrop(stack) => self.receive(stack.clone(), creative),
            ItemExchange::BottleHoney => self.bottle_honey(hand, creative),
            ItemExchange::Shrink => {
                if !creative {
                    let slot = self.hand_slot(hand);
                    let stack = &mut self.slots[slot];
                    if stack.count > 0 {
                        stack.count -= 1;
                    }
                    if stack.is_empty() {
                        *stack = ItemStack::empty();
                    }
                }
                ItemStack::empty()
            }
        };
        let changed = self
            .slots
            .iter()
            .zip(&before)
            .enumerate()
            .filter(|(_, (now, was))| *now != *was)
            .map(|(slot, _)| slot)
            .collect();
        (changed, leftover)
    }

    /// 官方 `createFilledResult`：先处理创造模式的上限，再消耗手里的一个。
    ///
    /// 放不进物品栏的部分返回给调用方。创造模式这一份是空的。
    ///
    /// `createFilledResult`: apply the creative limit, then consume one from the hand.
    ///
    /// What does not fit is returned. Creative mode returns an empty stack.
    fn fill_hand(
        &mut self,
        hand: Hand,
        replacement: ItemStack,
        creative: bool,
        limit_creative: bool,
    ) -> ItemStack {
        if limit_creative && creative {
            if !self.contains(&replacement) {
                self.add(replacement, true);
            }
            return ItemStack::empty();
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
            ItemStack::empty()
        } else {
            self.receive(replacement, creative)
        }
    }

    /// `BeehiveBlock` 的玻璃瓶：先减一，空了就换成蜜瓶，否则 `Inventory.add`。
    ///
    /// A glass bottle on a beehive: shrink by one, replace an empty hand with a honey bottle,
    /// otherwise `Inventory.add`.
    fn bottle_honey(&mut self, hand: Hand, creative: bool) -> ItemStack {
        let menu = self.hand_slot(hand);
        let stack = &mut self.slots[menu];
        if !stack.is_empty() {
            stack.count -= 1;
            if stack.count <= 0 {
                *stack = ItemStack::empty();
            }
        }
        let honey = ItemStack::new(items::HONEY_BOTTLE, 1);
        if self.slots[menu].is_empty() {
            self.slots[menu] = honey;
            ItemStack::empty()
        } else {
            self.receive(honey, creative)
        }
    }

    /// 官方 `Inventory.add(ItemStack)`，也就是 `add(-1, stack)`。
    ///
    /// `Inventory.add(ItemStack)`, which is `add(-1, stack)`.
    pub(crate) fn add(&mut self, incoming: ItemStack, creative: bool) -> bool {
        if incoming.is_empty() {
            return false;
        }
        let before = incoming.count;
        let left = self.receive(incoming, creative);
        left.count < before || (creative && left.is_empty())
    }

    /// 官方 `Inventory.add` 会改掉传入的那一堆。返回没放进去的部分。
    /// 创造模式放不下时把剩下的清成空堆，调用方看起来像是放进去了。
    ///
    /// Vanilla `Inventory.add` mutates the stack it is given. This returns what did not fit.
    /// Creative mode clears the leftover, so the caller sees a successful add.
    pub(crate) fn receive(&mut self, mut incoming: ItemStack, creative: bool) -> ItemStack {
        if incoming.is_empty() {
            return incoming;
        }
        if incoming.is_damaged() {
            return self.receive_damaged(incoming, creative);
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
            ItemStack::empty()
        } else {
            incoming
        }
    }

    fn receive_damaged(&mut self, incoming: ItemStack, creative: bool) -> ItemStack {
        if self.add_damaged(incoming.clone(), creative) {
            ItemStack::empty()
        } else {
            incoming
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
/// 不回包。负数的槽是拖出界面，生成物品实体。
/// 槽 0 是合成结果，它和超出范围、数量超限的包一样静默忽略。
///
/// No inventory reply. A negative slot drops the stack as an item entity.
/// Slot 0 is the crafting result and is ignored, same as an out-of-range or oversized stack.
pub(crate) fn on_set_creative_mode_slot(
    region: &mut Region,
    id: PlayerId,
    slot: i16,
    stack: ItemStack,
) {
    let creative = region
        .players
        .get(&id)
        .is_some_and(|player| player.game_mode == GameMode::Creative);
    if !creative || !count_is_valid(&stack) {
        return;
    }
    // 负数槽是拖出界面。限流在 [`crate::drops::creative_drop_outside`]。
    // A negative slot is a drag outside the screen. The throttle lives in
    // [`crate::drops::creative_drop_outside`].
    if slot < 0 {
        crate::drops::creative_drop_outside(region, id, stack);
        return;
    }
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    if (1..MENU_SLOTS as i16).contains(&slot) {
        player.inventory.slots[slot as usize] = stack;
    }
}

/// Q 和 Ctrl+Q：客户端已经在本地扣掉了，这里跟上，不回包。`blocks-inventory.md` 8.3。
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

/// 官方 `Inventory.add`：放进背包并同步变过的槽，生存模式放不下的按 `Player.drop` 丢掉。
///
/// 创造模式放不下时那一份官方直接清掉，不生成实体。
///
/// `Inventory.add`. Survival drops what does not fit via `Player.drop`. Creative discards it.
pub(crate) fn give_or_drop(region: &mut Region, id: PlayerId, stack: ItemStack) {
    let leftover = {
        let Some(player) = region.players.get_mut(&id) else {
            return;
        };
        let creative = player.game_mode == GameMode::Creative;
        let before = player.inventory.slots.clone();
        let left = player.inventory.receive(stack, creative);
        let changed: Vec<usize> = player
            .inventory
            .slots
            .iter()
            .zip(&before)
            .enumerate()
            .filter(|(_, (now, was))| *now != *was)
            .map(|(slot, _)| slot)
            .collect();
        for slot in changed {
            send_slot(player, &mut region.links, slot);
        }
        left
    };
    if !leftover.is_empty() {
        crate::drops::spawn_throw(region, id, leftover);
    }
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
    let leftover = {
        let Some(player) = region.players.get_mut(&id) else {
            return;
        };
        let creative = player.game_mode == GameMode::Creative;
        let (changed, leftover) = player.inventory.apply_exchange(hand, exchange, creative);
        for slot in changed {
            send_slot(player, &mut region.links, slot);
        }
        leftover
    };
    if !leftover.is_empty() {
        // 官方 `player.drop`。预测和 `SERVER_ONLY` 这里都只在服务端生成一个实体。
        // Vanilla `player.drop`. Predicted and `SERVER_ONLY` drops both spawn one server entity.
        crate::drops::spawn_throw(region, id, leftover);
    }
}

/// `ItemUtils.createFilledResult`：交还舀起或倒出之后的那一堆。
///
/// `ItemUtils.createFilledResult`: hand back the stack from a scoop or a pour.
pub(crate) fn give_filled_result(
    region: &mut Region,
    id: PlayerId,
    hand: Hand,
    replacement: ItemStack,
    limit_creative: bool,
) {
    apply_item_exchange(
        region,
        id,
        hand,
        &ItemExchange::Fill {
            replacement,
            limit_creative,
        },
    );
}

/// 用玩家物品栏的规则收下 `incoming`，返回没放进去的部分。`slots` 是 46 格菜单。
///
/// Accepts `incoming` with the player inventory rules and returns what did not fit.
/// `slots` is the 46-slot menu.
pub fn receive_like_player(
    slots: &mut [ItemStack],
    selected: u8,
    incoming: ItemStack,
    creative: bool,
) -> ItemStack {
    if slots.len() != MENU_SLOTS {
        return incoming;
    }
    let mut inventory = Inventory {
        slots: slots.to_vec(),
        selected,
        state_id: 0,
    };
    let left = inventory.receive(incoming, creative);
    slots.clone_from_slice(&inventory.slots);
    left
}

/// 把物品栏里每个槽发给这名玩家。`/give` 和 `/clear` 改完之后用它同步。
///
/// Sends every inventory slot. `/give` and `/clear` use this after they change the inventory.
pub(crate) fn send_all_slots(player: &mut Player, links: &mut Links) {
    for slot in 0..MENU_SLOTS {
        send_slot(player, links, slot);
    }
}

/// 官方 `Inventory.clearOrCountMatchingItems`。`amount_to_remove` 是负数时不限量。
///
/// Official `Inventory.clearOrCountMatchingItems`. A negative `amount_to_remove` is unlimited.
pub(crate) fn clear_or_count(
    inventory: &mut Inventory,
    predicate: &dyn Fn(&ItemStack) -> bool,
    counting_only: bool,
    amount_to_remove: i32,
) -> i32 {
    let mut count = 0;
    // 顺序和官方 `Inventory` 的容器一致：快捷栏和背包、盔甲、副手，最后是合成格。
    //
    // Same order as official `Inventory`: hotbar and storage, armour, off hand, then the craft grid.
    for index in 0..INVENTORY_ITEMS {
        count += clear_slot(
            &mut inventory.slots[menu_slot(index)],
            predicate,
            amount_to_remove - count,
            counting_only,
        );
    }
    for slot in (ARMOR_START..ARMOR_START + 4).rev() {
        count += clear_slot(
            &mut inventory.slots[slot],
            predicate,
            amount_to_remove - count,
            counting_only,
        );
    }
    count += clear_slot(
        &mut inventory.slots[OFF_HAND],
        predicate,
        amount_to_remove - count,
        counting_only,
    );
    // Bamboo 的 0 号是合成结果，输入格是 1 到 4，和官方的 craftSlots 对应。
    //
    // Slot 0 is the crafting result. Slots 1 through 4 are the inputs, matching official craftSlots.
    for slot in 1..ARMOR_START {
        count += clear_slot(
            &mut inventory.slots[slot],
            predicate,
            amount_to_remove - count,
            counting_only,
        );
    }
    // 光标上的物品官方也会清掉，Bamboo 不保存它。
    //
    // Official also clears the cursor item. Bamboo does not store it.
    count
}

/// 官方 `ContainerHelper.clearOrCountMatchingItems` 处理一个物品堆的部分。
///
/// The per-stack part of official `ContainerHelper.clearOrCountMatchingItems`.
fn clear_slot(
    stack: &mut ItemStack,
    predicate: &dyn Fn(&ItemStack) -> bool,
    amount_to_remove: i32,
    counting_only: bool,
) -> i32 {
    if stack.is_empty() || !predicate(stack) {
        return 0;
    }
    if counting_only {
        return stack.count;
    }
    let removed = if amount_to_remove < 0 {
        stack.count
    } else {
        amount_to_remove.min(stack.count)
    };
    stack.count -= removed;
    if stack.count <= 0 {
        *stack = ItemStack::empty();
    }
    removed
}

/// 发射器穿上的那一格。下标是组件 `equipment_slot`。身体和鞍没有菜单槽。
///
/// The slot a dispenser equips. The index is the component `equipment_slot`.
/// Body and saddle have no menu slot.
pub(crate) fn set_dispenser_slot(
    inventory: &mut Inventory,
    component_slot: i32,
    stack: ItemStack,
) -> Option<usize> {
    let index = match component_slot {
        0 => inventory.hand_slot(Hand::Main),
        1 => 8,
        2 => 7,
        3 => 6,
        4 => 5,
        5 => OFF_HAND,
        _ => return None,
    };
    *inventory.slots.get_mut(index)? = stack;
    Some(index)
}

/// 把一个槽发给这名玩家。组件原样发：这些字节本来就是它的客户端发来的，或者来自服务端
/// 自己的数据，见 [`ItemStack::write`]。
pub(crate) fn send_slot(player: &mut Player, links: &mut Links, slot: usize) {
    let state_id = player.inventory.next_state_id();
    // 这一格已经发给客户端了，物品栏菜单的远程槽跟上，下次合成点击才不会再发一遍。
    // The client already has this slot. Update the inventory menu's remote slot so the next
    // crafting click does not send it again.
    crate::menu::station::note_inventory_slot(player, slot);
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
