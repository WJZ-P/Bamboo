//! 容器菜单：打开、点击、同步和关闭。
//!
//! 对应官方 `AbstractContainerMenu` 和 `ChestMenu`、`HopperMenu`、`DispenserMenu`、
//! `ShulkerBoxMenu` 的 `quickMoveStack`。容器的内容在 [`crate::container`]，
//! 这里只管一名玩家和容器之间的那一部分菜单状态。

use std::collections::BTreeMap;

use mc_bamboo_blocks::{BlockState, World, hopper::set_changed};
use mc_bamboo_core::{Aabb, BlockPos, Vec3};
use mc_bamboo_data::{BlockAncestor, blocks, registries::menu};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{
        ContainerClose, ContainerInput, ContainerSetContent, ContainerSetSlot, GameMode,
        HashedStack, OpenScreen, SetCursorItem, SetPlayerInventory,
    },
    text::Component,
};
use tracing::{debug, warn};

use crate::{
    block_items,
    container::{self, CONTAINER_BUFFER, ContainerKind},
    inbound::PlayerId,
    interaction::{BLOCK_INTERACTION_RANGE, rules},
    inventory::{HOTBAR_START, Inventory, OFF_HAND, menu_slot},
    region::Region,
};

/// `state_id` 回绕的掩码，官方 `incrementStateId`。
const STATE_ID_MASK: i32 = 32767;
/// `Container.getMaxStackSize()` 的默认值，方块实体和玩家物品栏都没有覆盖它。
const CONTAINER_STACK_LIMIT: i32 = 99;
/// 双箱的行数，官方 `ChestMenu.sixRows`。
const DOUBLE_CHEST_ROWS: usize = 6;
/// 双箱的标题，官方 `ChestBlock.getName`。
const DOUBLE_CHEST_TITLE: &str = "container.chestDouble";
/// 物品栏菜单里快捷栏第一格的槽位，玩家部分的 36 格从这里开始。
const PLAYER_SLOTS: usize = 36;
/// 物品栏菜单里第一格的位置，`Inventory.slots` 是按菜单槽位存的。
const INVENTORY_MENU_START: usize = 9;

/// 菜单指向的方块。双箱要记住两半，打开和关闭都要作用在两边。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Single(BlockPos),
    Double { first: BlockPos, second: BlockPos },
    EnderChest(BlockPos),
}

impl Target {
    /// 菜单涉及的全部位置，顺序就是槽位顺序。
    pub(crate) fn positions(&self) -> Vec<BlockPos> {
        match *self {
            Self::Single(pos) | Self::EnderChest(pos) => vec![pos],
            Self::Double { first, second } => vec![first, second],
        }
    }

    /// 这个位置是不是菜单指向的方块之一。
    pub(crate) fn contains(&self, pos: BlockPos) -> bool {
        self.positions().contains(&pos)
    }
}

/// 服务端认为客户端手里有的一份槽位内容，官方 `RemoteSlot.Synchronized`。
///
/// `expected` 是发出去的内容，`hash` 是客户端回传的摘要。两者只会有一个。
#[derive(Debug, Clone)]
struct RemoteSlot {
    expected: Option<ItemStack>,
    hash: Option<HashedStack>,
}

impl RemoteSlot {
    /// 新菜单的初始远程槽，官方构造菜单时就是空的。
    fn synchronized() -> Self {
        Self {
            expected: Some(ItemStack::empty()),
            hash: None,
        }
    }

    /// 记下发出去的内容，官方 `force`。
    fn force(&mut self, stack: &ItemStack) {
        self.expected = Some(stack.clone());
        self.hash = None;
    }

    /// 记下客户端说它现在有的内容，官方 `receive`。
    fn receive(&mut self, hash: HashedStack) {
        self.expected = None;
        self.hash = Some(hash);
    }

    /// 客户端手上的内容和本地一致吗。一致时把摘要升级成内容，官方 `matches`。
    fn matches(&mut self, local: &ItemStack) -> bool {
        if let Some(expected) = &self.expected {
            return stack_matches(expected, local);
        }
        if let Some(hash) = &self.hash
            && hashed_matches(hash, local)
        {
            self.expected = Some(local.clone());
            return true;
        }
        false
    }
}

/// 一名玩家打开着的菜单，官方 `AbstractContainerMenu` 加上具体子类的槽位布局。
pub(crate) struct Menu {
    pub(crate) container_id: i32,
    kind: ContainerKind,
    pub(crate) target: Target,
    /// 容器自己的格数。玩家物品栏的 36 格另加在后面。
    container_size: usize,
    carried: ItemStack,
    state_id: i32,
    remote_slots: Vec<RemoteSlot>,
    remote_carried: RemoteSlot,
    quickcraft_status: i32,
    quickcraft_type: i32,
    quickcraft_slots: Vec<usize>,
}

impl Menu {
    fn new(container_id: i32, kind: ContainerKind, target: Target, container_size: usize) -> Self {
        let slots_len = container_size + PLAYER_SLOTS;
        Self {
            container_id,
            kind,
            target,
            container_size,
            carried: ItemStack::empty(),
            state_id: 0,
            remote_slots: vec![RemoteSlot::synchronized(); slots_len],
            remote_carried: RemoteSlot::synchronized(),
            quickcraft_status: 0,
            quickcraft_type: 0,
            quickcraft_slots: Vec::new(),
        }
    }

    /// 菜单一共有多少格。
    fn slots_len(&self) -> usize {
        self.container_size + PLAYER_SLOTS
    }

    /// 官方 `incrementStateId`。
    fn next_state_id(&mut self) -> i32 {
        self.state_id = (self.state_id + 1) & STATE_ID_MASK;
        self.state_id
    }

    /// 官方 `isValidSlotIndex`。注意负数里只有 -1 和 -999 是特殊的，别的负数也会通过这里。
    fn valid_slot_index(&self, slot: i16) -> bool {
        slot == -1 || slot == -999 || i32::from(slot) < self.slots_len() as i32
    }

    /// 官方 `broadcastChanges` 里槽位的那一半：和远程槽不一致的格记下发出去过，
    /// 返回要发的 state_id、槽位和内容。
    fn take_slot_changes(&mut self, slots: &[ItemStack]) -> Vec<(i32, usize, ItemStack)> {
        let mut changes = Vec::new();
        for (index, current) in slots.iter().enumerate().take(self.slots_len()) {
            if !self.remote_slots[index].matches(current) {
                self.remote_slots[index].force(current);
                changes.push((self.next_state_id(), index, current.clone()));
            }
        }
        changes
    }

    /// 官方 `broadcastChanges` 里光标的那一半。
    fn take_carried_change(&mut self) -> Option<ItemStack> {
        if self.remote_carried.matches(&self.carried) {
            return None;
        }
        self.remote_carried.force(&self.carried);
        Some(self.carried.clone())
    }

    /// 官方 `sendAllDataToRemote`：每个槽都记成发出去过，返回这一包的 state_id。
    fn take_full_state(&mut self, slots: &[ItemStack]) -> i32 {
        for (index, stack) in slots.iter().enumerate().take(self.slots_len()) {
            self.remote_slots[index].force(stack);
        }
        self.remote_carried.force(&self.carried);
        self.next_state_id()
    }
}

/// 一次点击的全部字段，官方 `ServerboundContainerClickPacket`。
pub(crate) struct ClickPacket {
    pub(crate) container_id: i32,
    pub(crate) state_id: i32,
    pub(crate) slot: i16,
    pub(crate) button: i8,
    pub(crate) input: ContainerInput,
    pub(crate) changed_slots: BTreeMap<i16, HashedStack>,
    pub(crate) carried: HashedStack,
}

/// 点击时菜单和物品栏的联合视图，对应官方 `doClick` 里能碰到的东西。
struct Ctx<'a> {
    kind: ContainerKind,
    container_size: usize,
    /// 点这个菜单的人。容器格里别人放的组件只能滤过以后交给他，见 [`Ctx::taken_by_viewer`]。
    viewer: PlayerId,
    container: &'a mut [ItemStack],
    /// 每个容器格的组件来自哪名玩家，和 `container` 一一对应。
    sources: &'a mut [Option<PlayerId>],
    inventory: &'a mut Inventory,
    carried: &'a mut ItemStack,
    quickcraft_status: &'a mut i32,
    quickcraft_type: &'a mut i32,
    quickcraft_slots: &'a mut Vec<usize>,
    creative: bool,
}

impl Ctx<'_> {
    fn slots_len(&self) -> usize {
        self.container_size + PLAYER_SLOTS
    }

    /// 菜单里的槽位换算成 `Inventory.slots` 的下标，玩家部分从菜单的第 9 格开始。
    fn inv_slot(&self, index: usize) -> usize {
        index - self.container_size + INVENTORY_MENU_START
    }

    /// 这个槽现在的物品。
    fn get(&self, index: usize) -> ItemStack {
        self.container_or_inventory(index).clone()
    }

    fn container_or_inventory(&self, index: usize) -> &ItemStack {
        if index < self.container_size {
            &self.container[index]
        } else {
            &self.inventory.slots[self.inv_slot(index)]
        }
    }

    /// 这一格的物品离开容器交到点菜单的人手上时的样子。
    ///
    /// 别人放进来的物品里那几种没法完全校验的组件不跟着走：它们只能发回给来源玩家，
    /// 一路跟到这名玩家的物品栏里就没法再分辨了。官方不滤，这是我们多出来的一道。
    fn taken_by_viewer(&self, index: usize, stack: ItemStack) -> ItemStack {
        if index < self.container_size && self.sources[index] != Some(self.viewer) {
            return stack.filtered();
        }
        stack
    }

    fn set(&mut self, index: usize, stack: ItemStack) {
        if index < self.container_size {
            let previous = &self.container[index];
            if stack.is_empty() {
                self.sources[index] = None;
            } else if previous.is_empty() || !same_component_bytes(previous, &stack) {
                self.sources[index] = Some(self.viewer);
            }
            // 原来那一格非空、组件字节又没变，就是合并（只有数量变了），来源跟着不变。
            self.container[index] = stack;
        } else {
            let slot = self.inv_slot(index);
            self.inventory.slots[slot] = stack;
        }
    }

    /// 官方 `Slot.mayPlace`。只有潜影盒的槽拒绝潜影盒。
    fn may_place(&self, index: usize, stack: &ItemStack) -> bool {
        if self.kind != ContainerKind::ShulkerBox || index >= self.container_size {
            return true;
        }
        !is_shulker_box_item(stack)
    }

    /// 官方 `Slot.tryRemove`，`allowModification` 和 `mayPickup` 在我们的槽上恒为真。
    fn try_remove(&mut self, index: usize, amount: i32, max_amount: i32) -> Option<ItemStack> {
        let amount = amount.min(max_amount);
        let current = self.get(index);
        if current.is_empty() || amount <= 0 {
            return None;
        }
        let taken = copy_with_count(&current, amount.min(current.count));
        let leftover = current.count - taken.count;
        if leftover <= 0 {
            self.set(index, ItemStack::empty());
        } else {
            self.set(index, copy_with_count(&current, leftover));
        }
        Some(self.taken_by_viewer(index, taken))
    }

    /// 官方 `Slot.safeTake`。`onTake` 对我们什么也不做。
    fn safe_take(&mut self, index: usize, amount: i32) -> ItemStack {
        self.try_remove(index, amount, i32::MAX)
            .unwrap_or_else(ItemStack::empty)
    }

    /// 官方 `Slot.safeClone`：克隆一份满堆。
    fn safe_clone(&mut self, index: usize) -> ItemStack {
        let current = self.get(index);
        if current.is_empty() {
            return ItemStack::empty();
        }
        let cloned = copy_with_count(&current, item_max(&current));
        self.taken_by_viewer(index, cloned)
    }

    /// 官方 `Slot.safeInsert`。返回没放进槽里的剩余物品。
    fn safe_insert(&mut self, index: usize, mut input: ItemStack, amount: i32) -> ItemStack {
        if input.is_empty() || !self.may_place(index, &input) {
            return input;
        }
        let slot_stack = self.get(index);
        let transferable = amount
            .min(input.count)
            .min(stack_limit(&input) - slot_stack.count);
        if transferable <= 0 {
            return input;
        }
        if slot_stack.is_empty() {
            self.set(index, copy_with_count(&input, transferable));
        } else if is_same_item_same_components(&slot_stack, &input) {
            let mut grown = slot_stack;
            grown.count += transferable;
            self.set(index, grown);
        } else {
            return input;
        }
        input.count -= transferable;
        if input.count <= 0 {
            ItemStack::empty()
        } else {
            input
        }
    }

    /// 官方 `moveItemStackTo`。合并同种物品，然后找一个空格放下。
    fn move_item_stack_to(
        &mut self,
        stack: &mut ItemStack,
        start: usize,
        end: usize,
        backwards: bool,
    ) -> bool {
        let mut anything_changed = false;
        if is_stackable(stack) {
            let mut dest = if backwards {
                end as i64 - 1
            } else {
                start as i64
            };
            while !stack.is_empty() && in_range(dest, start, end, backwards) {
                let target = self.get(dest as usize);
                if !target.is_empty() && is_same_item_same_components(stack, &target) {
                    let total = target.count + stack.count;
                    let max = stack_limit(&target);
                    if total <= max {
                        let mut merged = target;
                        merged.count = total;
                        self.set(dest as usize, merged);
                        *stack = ItemStack::empty();
                        anything_changed = true;
                    } else if target.count < max {
                        let added = max - target.count;
                        stack.count -= added;
                        if stack.count <= 0 {
                            *stack = ItemStack::empty();
                        }
                        let mut merged = target;
                        merged.count = max;
                        self.set(dest as usize, merged);
                        anything_changed = true;
                    }
                }
                dest += if backwards { -1 } else { 1 };
            }
        }

        if !stack.is_empty() {
            let mut dest = if backwards {
                end as i64 - 1
            } else {
                start as i64
            };
            while in_range(dest, start, end, backwards) {
                let index = dest as usize;
                if self.get(index).is_empty() && self.may_place(index, stack) {
                    let amount = stack.count.min(stack_limit(stack));
                    let taken = split(stack, amount);
                    self.set(index, taken);
                    anything_changed = true;
                    break;
                }
                dest += if backwards { -1 } else { 1 };
            }
        }
        anything_changed
    }

    /// 各个菜单的 `quickMoveStack`。容器侧移到物品栏，物品栏侧移到容器。
    fn quick_move(&mut self, index: usize) -> ItemStack {
        let clicked = self.get(index);
        if clicked.is_empty() {
            return ItemStack::empty();
        }
        // 从容器格移到物品栏时物品换了主人，组件照 `taken_by_viewer` 过一遍。
        let mut stack = self.taken_by_viewer(index, clicked.clone());
        let moved = if index < self.container_size {
            self.move_item_stack_to(&mut stack, self.container_size, self.slots_len(), true)
        } else {
            self.move_item_stack_to(&mut stack, 0, self.container_size, false)
        };
        if !moved {
            return ItemStack::empty();
        }
        // 官方的 `moveItemStackTo` 直接改槽里那堆物品，来源格自动只剩没搬走的部分。
        // 这里搬的是副本，剩下多少都要写回去，否则没搬完的那部分会凭空复制一份。
        self.set(index, stack.clone());
        // `DispenserMenu` 比别的菜单多这一句。
        if matches!(self.kind, ContainerKind::Dispenser | ContainerKind::Dropper)
            && stack.count == clicked.count
        {
            return ItemStack::empty();
        }
        clicked
    }

    /// 官方 `Inventory.add`：先找能叠上去的槽，再找空格。腾不出地方时返回 `false`。
    fn add_to_inventory(&mut self, mut stack: ItemStack) -> bool {
        if stack.is_empty() {
            return false;
        }
        let mut last;
        loop {
            last = stack.count;
            stack.count = self.add_resource(&stack);
            if stack.is_empty() || stack.count >= last {
                break;
            }
        }
        if stack.count == last && self.creative {
            return true;
        }
        stack.count < last
    }

    /// 官方 `Inventory.addResource`，返回还剩下的数量。
    fn add_resource(&mut self, stack: &ItemStack) -> i32 {
        let slot =
            remaining_space_slot(self.inventory, stack).or_else(|| free_slot(self.inventory));
        match slot {
            None => stack.count,
            Some(slot) => add_resource_to(self.inventory, slot, stack),
        }
    }
}

/// 这个下标在 `start..end` 里（`backwards` 时从 `end - 1` 往下数）。
fn in_range(dest: i64, start: usize, end: usize, backwards: bool) -> bool {
    if backwards {
        dest >= start as i64
    } else {
        dest < end as i64
    }
}

/// 官方的一次 `doClick`。`slot_index` 和 `button` 是网络上的原值，可以是负数。
fn do_click(ctx: &mut Ctx, slot_index: i32, button: i32, input: ContainerInput) {
    if input == ContainerInput::QuickCraft {
        quick_craft(ctx, slot_index, button);
    } else if *ctx.quickcraft_status != 0 {
        reset_quick_craft(ctx);
    } else if matches!(input, ContainerInput::Pickup | ContainerInput::QuickMove)
        && is_container_click_button(button)
    {
        pickup_or_quick_move(ctx, slot_index, button, input == ContainerInput::QuickMove);
    } else if input == ContainerInput::Swap && (is_hotbar_button(button) || button == 40) {
        swap_with_inventory(ctx, slot_index, button);
    } else if input == ContainerInput::Clone
        && ctx.creative
        && ctx.carried.is_empty()
        && slot_index >= 0
    {
        clone_slot(ctx, slot_index);
    } else if input == ContainerInput::Throw && ctx.carried.is_empty() && slot_index >= 0 {
        throw_slot(ctx, slot_index, button);
    } else if input == ContainerInput::PickupAll && slot_index >= 0 {
        pickup_all(ctx, slot_index, button);
    }
}

fn pickup_or_quick_move(ctx: &mut Ctx, slot_index: i32, button: i32, quick: bool) {
    if slot_index == -999 {
        if !ctx.carried.is_empty() {
            if button == 0 {
                *ctx.carried = ItemStack::empty();
            } else {
                split(ctx.carried, 1);
            }
        }
        return;
    }
    let Some(index) = positive_index(ctx, slot_index) else {
        return;
    };
    if quick {
        let mut clicked = ctx.quick_move(index);
        while !clicked.is_empty() && is_same_item(&ctx.get(index), &clicked) {
            clicked = ctx.quick_move(index);
        }
    } else {
        pickup(ctx, index, button == 0);
    }
}

fn pickup(ctx: &mut Ctx, index: usize, primary: bool) {
    let clicked = ctx.get(index);
    let carried = ctx.carried.clone();
    if clicked.is_empty() {
        if !carried.is_empty() {
            let amount = if primary { carried.count } else { 1 };
            *ctx.carried = ctx.safe_insert(index, carried, amount);
        }
    } else if carried.is_empty() {
        let amount = if primary {
            clicked.count
        } else {
            (clicked.count + 1) / 2
        };
        if let Some(taken) = ctx.try_remove(index, amount, i32::MAX) {
            *ctx.carried = taken;
        }
    } else if ctx.may_place(index, &carried) {
        if is_same_item_same_components(&clicked, &carried) {
            let amount = if primary { carried.count } else { 1 };
            *ctx.carried = ctx.safe_insert(index, carried, amount);
        } else if carried.count <= stack_limit(&carried) {
            // 两边换位置：拿到手上的那一份换了主人。
            *ctx.carried = ctx.taken_by_viewer(index, clicked);
            ctx.set(index, carried);
        }
    } else if is_same_item_same_components(&clicked, &carried) {
        let max_amount = item_max(&carried) - carried.count;
        if let Some(taken) = ctx.try_remove(index, clicked.count, max_amount) {
            ctx.carried.count += taken.count;
        }
    }
}

fn swap_with_inventory(ctx: &mut Ctx, slot_index: i32, button: i32) {
    let Some(index) = positive_index(ctx, slot_index) else {
        return;
    };
    let Some(source_slot) = swap_slot(button) else {
        return;
    };
    let source = ctx.inventory.slots[source_slot].clone();
    // 交换和「放不下时收进物品栏」都会把容器格里的物品搬进这名玩家的物品栏。
    let target = ctx.taken_by_viewer(index, ctx.get(index));
    if source.is_empty() && target.is_empty() {
        return;
    }
    if source.is_empty() {
        // `mayPickup` 在我们的槽上恒为真，`onSwapCraft` 和 `onTake` 什么也不做。
        ctx.inventory.slots[source_slot] = target;
        ctx.set(index, ItemStack::empty());
    } else if target.is_empty() {
        if ctx.may_place(index, &source) {
            let max = stack_limit(&source);
            if source.count > max {
                let taken = split(&mut ctx.inventory.slots[source_slot], max);
                ctx.set(index, taken);
            } else {
                ctx.inventory.slots[source_slot] = ItemStack::empty();
                ctx.set(index, source);
            }
        }
    } else if ctx.may_place(index, &source) {
        let max = stack_limit(&source);
        if source.count > max {
            let taken = split(&mut ctx.inventory.slots[source_slot], max);
            ctx.set(index, taken);
            ctx.add_to_inventory(target);
        } else {
            ctx.inventory.slots[source_slot] = target;
            ctx.set(index, source);
        }
    }
}

fn clone_slot(ctx: &mut Ctx, slot_index: i32) {
    let Some(index) = positive_index(ctx, slot_index) else {
        return;
    };
    let cloned = ctx.safe_clone(index);
    if !cloned.is_empty() {
        *ctx.carried = cloned;
    }
}

fn throw_slot(ctx: &mut Ctx, slot_index: i32, button: i32) {
    let Some(index) = positive_index(ctx, slot_index) else {
        return;
    };
    let amount = if button == 0 { 1 } else { ctx.get(index).count };
    let mut item = ctx.safe_take(index, amount);
    // 没有物品实体，丢掉的东西就没了。
    if button == 1 {
        while !item.is_empty() && is_same_item(&ctx.get(index), &item) {
            item = ctx.safe_take(index, amount);
        }
    }
}

fn pickup_all(ctx: &mut Ctx, slot_index: i32, button: i32) {
    let Some(index) = positive_index(ctx, slot_index) else {
        return;
    };
    let target = ctx.get(index);
    // `mayPickup` 恒为真，所以只有点在空格上时才会收集。
    if ctx.carried.is_empty() || !target.is_empty() {
        return;
    }
    let start: i64 = if button == 0 {
        0
    } else {
        ctx.slots_len() as i64 - 1
    };
    let step: i64 = if button == 0 { 1 } else { -1 };
    for pass in 0..2 {
        let mut cursor = start;
        while cursor >= 0
            && (cursor as usize) < ctx.slots_len()
            && ctx.carried.count < item_max(ctx.carried)
        {
            let index = cursor as usize;
            let item = ctx.get(index);
            if !item.is_empty()
                && can_item_quick_replace(ctx, index, ctx.carried, true)
                && (pass != 0 || item.count != item_max(&item))
            {
                let max_amount = item_max(ctx.carried) - ctx.carried.count;
                if let Some(removed) = ctx.try_remove(index, item.count, max_amount) {
                    ctx.carried.count += removed.count;
                }
            }
            cursor += step;
        }
    }
}

fn quick_craft(ctx: &mut Ctx, slot_index: i32, button: i32) {
    let expected_status = *ctx.quickcraft_status;
    let status = button & 3;
    *ctx.quickcraft_status = status;
    if ((expected_status != 1 || status != 2) && expected_status != status)
        || ctx.carried.is_empty()
    {
        reset_quick_craft(ctx);
    } else if status == 0 {
        let kind = (button >> 2) & 3;
        *ctx.quickcraft_type = kind;
        if kind == 0 || kind == 1 || (kind == 2 && ctx.creative) {
            *ctx.quickcraft_status = 1;
            ctx.quickcraft_slots.clear();
        } else {
            reset_quick_craft(ctx);
        }
    } else if status == 1 {
        let Some(index) = positive_index(ctx, slot_index) else {
            return;
        };
        let carried = ctx.carried.clone();
        if can_item_quick_replace(ctx, index, &carried, true)
            && ctx.may_place(index, &carried)
            && (*ctx.quickcraft_type == 2 || carried.count > ctx.quickcraft_slots.len() as i32)
            // 官方用的是 Set<Slot>，同一个槽位拖过两次只算一次。
            && !ctx.quickcraft_slots.contains(&index)
        {
            ctx.quickcraft_slots.push(index);
        }
    } else if status == 2 {
        finish_quick_craft(ctx);
    } else {
        reset_quick_craft(ctx);
    }
}

fn finish_quick_craft(ctx: &mut Ctx) {
    if !ctx.quickcraft_slots.is_empty() {
        if ctx.quickcraft_slots.len() == 1 {
            let index = ctx.quickcraft_slots[0];
            reset_quick_craft(ctx);
            do_click(
                ctx,
                index as i32,
                *ctx.quickcraft_type,
                ContainerInput::Pickup,
            );
            return;
        }
        let source = ctx.carried.clone();
        if source.is_empty() {
            reset_quick_craft(ctx);
            return;
        }
        let size = ctx.quickcraft_slots.len();
        let mut remaining = ctx.carried.count;
        for slot in 0..size {
            let index = ctx.quickcraft_slots[slot];
            let carried = ctx.carried.clone();
            if can_item_quick_replace(ctx, index, &carried, true)
                && ctx.may_place(index, &carried)
                && (*ctx.quickcraft_type == 2 || carried.count >= size as i32)
            {
                let carry = ctx.get(index).count;
                let max_size = item_max(&source);
                let new_count =
                    (place_count(size, *ctx.quickcraft_type, &source) + carry).min(max_size);
                remaining -= new_count - carry;
                ctx.set(index, copy_with_count(&source, new_count));
            }
        }
        let mut result = source;
        result.count = remaining;
        *ctx.carried = if result.count <= 0 {
            ItemStack::empty()
        } else {
            result
        };
    }
    reset_quick_craft(ctx);
}

fn reset_quick_craft(ctx: &mut Ctx) {
    *ctx.quickcraft_status = 0;
    ctx.quickcraft_slots.clear();
}

fn is_container_click_button(button: i32) -> bool {
    button == 0 || button == 1
}

fn is_hotbar_button(button: i32) -> bool {
    (0..9).contains(&button)
}

/// 数字键和 F 对应的物品栏槽位，参数是官方 `Inventory` 的下标。
fn swap_slot(button: i32) -> Option<usize> {
    if is_hotbar_button(button) {
        Some(HOTBAR_START + button as usize)
    } else if button == 40 {
        Some(OFF_HAND)
    } else {
        None
    }
}

fn positive_index(ctx: &Ctx, slot_index: i32) -> Option<usize> {
    usize::try_from(slot_index)
        .ok()
        .filter(|&index| index < ctx.slots_len())
}

/// 官方 `canItemQuickReplace`。
fn can_item_quick_replace(ctx: &Ctx, index: usize, stack: &ItemStack, ignore_size: bool) -> bool {
    let target = ctx.get(index);
    if target.is_empty() {
        return true;
    }
    is_same_item_same_components(stack, &target)
        && target.count + if ignore_size { 0 } else { stack.count } <= item_max(stack)
}

/// 官方 `getQuickCraftPlaceCount`。
fn place_count(size: usize, kind: i32, stack: &ItemStack) -> i32 {
    match kind {
        0 => (stack.count as f32 / size as f32).floor() as i32,
        1 => 1,
        2 => item_max(stack),
        _ => stack.count,
    }
}

/// 打开一个容器方块，对应官方 `useWithoutItem` 里的 `player.openMenu`。
///
/// 打不开（不是容器、被挡住、潜影盒被挡住）时返回 `false`，调用方据此判断有没有菜单。
pub(crate) fn open_block(region: &mut Region, id: PlayerId, pos: BlockPos) -> bool {
    let Some(kind) = container::kind_at(region, pos) else {
        return false;
    };
    if !region.players.contains_key(&id) {
        return false;
    }
    let Some((target, container_size, menu_type, title)) = menu_shape(region, pos, kind) else {
        return false;
    };
    if region.menus.contains_key(&id) {
        close_menu(region, id);
    }
    let container_id = {
        let Some(player) = region.players.get_mut(&id) else {
            return false;
        };
        player.container_counter = player.container_counter % 100 + 1;
        player.container_counter
    };
    let menu = Menu::new(container_id, kind, target, container_size);
    if kind == ContainerKind::EnderChest
        && let Some(player) = region.players.get_mut(&id)
    {
        player.ender_chest.active = Some(pos);
    }
    region.menus.insert(id, menu);
    // 官方每个容器的 `startOpen` 都先看 `isSpectator`：旁观者打开不计数，盖子不动也不响。
    if !is_spectator(region, id) {
        let positions = region.menus[&id].target.positions();
        for target_pos in positions {
            container::start_open(region, target_pos, kind);
        }
    }
    let title = menu_title(region, &region.menus[&id].target, title);
    region.links.send(
        id,
        &OpenScreen {
            container_id,
            menu_type,
            title: &title,
        },
    );
    let Some(mut menu) = region.menus.remove(&id) else {
        return false;
    };
    broadcast_full_state(region, id, &mut menu);
    region.menus.insert(id, menu);
    true
}

/// 官方 `ContainerUser.getLivingEntity().isSpectator()`。
fn is_spectator(region: &Region, id: PlayerId) -> bool {
    region
        .players
        .get(&id)
        .is_some_and(|player| player.game_mode == GameMode::Spectator)
}

/// 菜单的标题，官方 `Nameable.getDisplayName`：方块实体有自定义名就用它。
///
/// 双箱看官方 `ChestBlock.MENU_PROVIDER_COMBINER`：先看主箱，再看另一半，都没有才用默认标题。
fn menu_title(region: &Region, target: &Target, default: &str) -> Component {
    for pos in target.positions() {
        if let Some(name) = block_items::custom_name(region, pos) {
            return name.clone();
        }
    }
    Component::translate(default, [])
}

/// 这个容器打开后是什么形状。返回 `None` 时官方也不会打开菜单。
fn menu_shape(
    region: &mut Region,
    pos: BlockPos,
    kind: ContainerKind,
) -> Option<(Target, usize, i32, &'static str)> {
    Some(match kind {
        ContainerKind::Chest => match container::chest_shape(&region.chunks, pos) {
            Err(()) => return None,
            Ok(None) => (
                Target::Single(pos),
                kind.slots(),
                kind.menu_type(),
                kind.title(),
            ),
            Ok(Some(chest)) => (
                Target::Double {
                    first: chest.first,
                    second: chest.second,
                },
                DOUBLE_CHEST_ROWS * 9,
                menu::GENERIC_9X6,
                DOUBLE_CHEST_TITLE,
            ),
        },
        ContainerKind::EnderChest => {
            if container::blocked(&region.chunks, pos) {
                return None;
            }
            // 末影箱的 27 格在玩家身上，方块实体自己一格也没有。
            (Target::EnderChest(pos), 27, kind.menu_type(), kind.title())
        }
        ContainerKind::ShulkerBox => {
            if !container::can_open_shulker(region, pos) {
                return None;
            }
            (
                Target::Single(pos),
                kind.slots(),
                kind.menu_type(),
                kind.title(),
            )
        }
        // 雕纹书架、唱片机和陶罐官方没有菜单。合成器的 `CrafterMenu` 这次不打开。
        //
        // Chiseled bookshelves, jukeboxes and pots have no vanilla menu. The crafter's
        // `CrafterMenu` is not opened in this change.
        ContainerKind::ChiseledBookshelf
        | ContainerKind::Jukebox
        | ContainerKind::DecoratedPot
        | ContainerKind::Crafter => return None,
        _ => (
            Target::Single(pos),
            kind.slots(),
            kind.menu_type(),
            kind.title(),
        ),
    })
}

/// 服务端主动关掉菜单：先发 `container_close`，再按官方 `removed` 的顺序收尾。
pub(crate) fn close_menu(region: &mut Region, id: PlayerId) {
    let Some(mut menu) = region.menus.remove(&id) else {
        return;
    };
    region.links.send(
        id,
        &ContainerClose {
            container_id: menu.container_id,
        },
    );
    finish_menu(region, id, &mut menu, true);
}

/// 客户端自己关掉了菜单。官方不看 `container_id`，也不回包。
pub(crate) fn on_close_request(region: &mut Region, id: PlayerId) {
    let Some(mut menu) = region.menus.remove(&id) else {
        return;
    };
    finish_menu(region, id, &mut menu, true);
}

/// 玩家断开。官方这时不把光标上的东西放回物品栏，因为没有连接可发了。
pub(crate) fn on_leave(region: &mut Region, id: PlayerId) {
    let Some(mut menu) = region.menus.remove(&id) else {
        return;
    };
    finish_menu(region, id, &mut menu, false);
}

/// 官方 `AbstractContainerMenu.removed` 加子类的 `stopOpen`。
fn finish_menu(region: &mut Region, id: PlayerId, menu: &mut Menu, return_items: bool) {
    let carried = std::mem::take(&mut menu.carried);
    if return_items {
        place_item_back_in_inventory(region, id, carried);
    }
    // `stopOpen` 看的是关菜单这一刻的游戏模式，和官方一样不管打开时是什么模式。
    if !is_spectator(region, id) {
        for pos in menu.target.positions() {
            container::stop_open(region, pos, menu.kind);
        }
    }
    if matches!(menu.target, Target::EnderChest(_))
        && let Some(player) = region.players.get_mut(&id)
    {
        player.ender_chest.active = None;
    }
}

/// 官方 `Inventory.placeItemBackInInventory`。没有物品实体，放不下的就丢掉。
fn place_item_back_in_inventory(region: &mut Region, id: PlayerId, mut stack: ItemStack) {
    if stack.is_empty() {
        return;
    }
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let mut updates: Vec<(i32, ItemStack)> = Vec::new();
    while !stack.is_empty() {
        let slot = remaining_space_slot(&player.inventory, &stack)
            .or_else(|| free_slot(&player.inventory));
        let Some(slot) = slot else {
            debug!(
                player = id.0,
                item = stack.item,
                "dropping the carried stack: there is no inventory space"
            );
            break;
        };
        let existing = &mut player.inventory.slots[slot];
        // 空格按进来的物品算上限，非空的格子按格子里物品的上限算。
        let max = if existing.is_empty() {
            item_max(&stack)
        } else {
            stack_limit(existing)
        };
        let to_add = (max - existing.count).min(stack.count);
        if to_add <= 0 {
            break;
        }
        if existing.is_empty() {
            *existing = copy_with_count(&stack, 0);
        }
        existing.count += to_add;
        stack.count -= to_add;
        if let Some(index) = official_index(slot) {
            updates.push((index, existing.clone()));
        }
    }
    for (slot, stack) in updates {
        region.links.send(
            id,
            &SetPlayerInventory {
                slot,
                stack: &stack,
            },
        );
    }
}

/// 物品栏里还有余量放这种物品的第一个槽，顺序照官方 `getSlotWithRemainingSpace`。
fn remaining_space_slot(inventory: &Inventory, incoming: &ItemStack) -> Option<usize> {
    let selected = HOTBAR_START + usize::from(inventory.selected);
    std::iter::once(selected)
        .chain(std::iter::once(OFF_HAND))
        .chain((0..PLAYER_SLOTS).map(menu_slot))
        .find(|&slot| has_remaining_space(&inventory.slots[slot], incoming))
}

fn has_remaining_space(slot_stack: &ItemStack, incoming: &ItemStack) -> bool {
    !slot_stack.is_empty()
        && is_same_item_same_components(incoming, slot_stack)
        && is_stackable(slot_stack)
        && slot_stack.count < stack_limit(slot_stack)
}

/// 官方 `getFreeSlot`，只找快捷栏和背包。
fn free_slot(inventory: &Inventory) -> Option<usize> {
    (0..PLAYER_SLOTS)
        .map(menu_slot)
        .find(|&slot| inventory.slots[slot].is_empty())
}

/// 官方 `Inventory.add` 往指定槽位加东西，返回剩下的数量。
fn add_resource_to(inventory: &mut Inventory, slot: usize, stack: &ItemStack) -> i32 {
    let count = stack.count;
    if inventory.slots[slot].is_empty() {
        inventory.slots[slot] = copy_with_count(stack, 0);
    }
    let max_to_add = stack_limit(&inventory.slots[slot]) - inventory.slots[slot].count;
    let to_add = count.min(max_to_add);
    if to_add <= 0 {
        return count;
    }
    inventory.slots[slot].count += to_add;
    count - to_add
}

/// 菜单槽位换算成官方 `Inventory` 的下标，`SetPlayerInventory` 要用它。
///
/// 护甲在 `Inventory` 里是 36 到 39，顺序和菜单相反：菜单 5 是头盔，对应物品栏 39，
/// 菜单 8 是靴子，对应 36，所以是 `44 - slot`。
fn official_index(slot: usize) -> Option<i32> {
    match slot {
        5..=8 => Some(44 - slot as i32),
        9..=35 => Some(slot as i32),
        36..=44 => Some(slot as i32 - HOTBAR_START as i32),
        45 => Some(40),
        _ => None,
    }
}

/// 每个 tick：先关掉已经失效的菜单，再把变化发给客户端。官方 `ServerPlayer.tick`。
pub(crate) fn tick(region: &mut Region) {
    let ids: Vec<PlayerId> = region.menus.keys().copied().collect();
    for id in ids {
        // 官方 `ServerPlayer.tick` 先 `broadcastChanges`，再看 `stillValid`：
        // 菜单失效的这一刻，最后的槽位变化也会先发出去。
        // Vanilla `ServerPlayer.tick` calls `broadcastChanges` before `stillValid`, so the last
        // slot changes still go out in the tick the menu becomes invalid.
        let Some(mut menu) = region.menus.remove(&id) else {
            continue;
        };
        broadcast_changes(region, id, &mut menu);
        let valid = still_valid(region, id, &menu);
        region.menus.insert(id, menu);
        if !valid {
            warn!(
                player = id.0,
                "closing a menu whose block is gone or out of reach"
            );
            close_menu(region, id);
        }
    }
}

/// 官方 `Container.stillValidBlockEntity` 和 `PlayerEnderChestContainer.stillValid`。
fn still_valid(region: &Region, id: PlayerId, menu: &Menu) -> bool {
    let Some(player) = region.players.get(&id) else {
        return false;
    };
    let eye = Vec3::new(
        player.position().x,
        player.position().y + player.eye_height(),
        player.position().z,
    );
    match menu.target {
        Target::Single(pos) => in_reach(region, pos, menu.kind, eye),
        Target::Double { first, second } => {
            in_reach(region, first, menu.kind, eye) && in_reach(region, second, menu.kind, eye)
        }
        Target::EnderChest(pos) => match player.ender_chest.active {
            // 官方的 `stillValid` 在没有打开的箱子时返回真。
            None => true,
            Some(active) => active == pos && in_reach(region, active, menu.kind, eye),
        },
    }
}

/// 方块还在，并且玩家在 `blockInteractionRange + 4` 以内。官方多给的 4 格见 `CONTAINER_BUFFER`。
fn in_reach(region: &Region, pos: BlockPos, kind: ContainerKind, eye: Vec3) -> bool {
    if container::kind_at(region, pos) != Some(kind) {
        return false;
    }
    let range = BLOCK_INTERACTION_RANGE + CONTAINER_BUFFER;
    let x = f64::from(pos.x);
    let y = f64::from(pos.y);
    let z = f64::from(pos.z);
    Aabb::new(x, y, z, x + 1.0, y + 1.0, z + 1.0).distance_to_sqr(eye) < range * range
}

/// 官方 `handleContainerClick`。
pub(crate) fn on_click(region: &mut Region, id: PlayerId, packet: ClickPacket) {
    let Some(menu) = region.menus.get(&id) else {
        return;
    };
    if menu.container_id != packet.container_id {
        debug!(
            player = id.0,
            container_id = packet.container_id,
            "ignoring a click for another menu"
        );
        return;
    }
    if !still_valid(region, id, menu) {
        debug!(
            player = id.0,
            "ignoring a click on a menu that is no longer valid"
        );
        return;
    }
    if !menu.valid_slot_index(packet.slot) {
        debug!(
            player = id.0,
            slot = packet.slot,
            "ignoring a click on an unknown slot"
        );
        return;
    }
    let full_resync = packet.state_id != menu.state_id;
    let mut menu = match region.menus.remove(&id) {
        Some(menu) => menu,
        None => return,
    };
    let ender = matches!(menu.target, Target::EnderChest(_));
    let (mut slots, mut sources) = raw_container_slots(region, id, &menu);
    let creative = region
        .players
        .get(&id)
        .is_some_and(|player| player.game_mode == GameMode::Creative);
    if let Some(player) = region.players.get_mut(&id) {
        let mut ctx = Ctx {
            kind: menu.kind,
            container_size: menu.container_size,
            viewer: id,
            container: &mut slots,
            sources: &mut sources,
            inventory: &mut player.inventory,
            carried: &mut menu.carried,
            quickcraft_status: &mut menu.quickcraft_status,
            quickcraft_type: &mut menu.quickcraft_type,
            quickcraft_slots: &mut menu.quickcraft_slots,
            creative,
        };
        do_click(
            &mut ctx,
            i32::from(packet.slot),
            i32::from(packet.button),
            packet.input,
        );
    }
    if ender {
        if let Some(player) = region.players.get_mut(&id) {
            player
                .ender_chest
                .slots
                .clone_from_slice(&slots[..menu.container_size]);
        }
    } else {
        write_back_containers(region, &menu, &slots, &sources);
    }
    for (slot, hash) in packet.changed_slots {
        match usize::try_from(slot) {
            Ok(index) if index < menu.slots_len() => menu.remote_slots[index].receive(hash),
            _ => debug!(
                player = id.0,
                slot, "ignoring a changed slot outside the menu"
            ),
        }
    }
    menu.remote_carried.receive(packet.carried);
    if full_resync {
        broadcast_full_state(region, id, &mut menu);
    } else {
        broadcast_changes(region, id, &mut menu);
    }
    region.menus.insert(id, menu);
}

/// 官方 `broadcastChanges`：只发和远程槽不一致的格，光标单独发。
fn broadcast_changes(region: &mut Region, id: PlayerId, menu: &mut Menu) {
    let current = network_slots(region, id, menu);
    for (state_id, index, stack) in menu.take_slot_changes(&current) {
        region.links.send(
            id,
            &ContainerSetSlot {
                container_id: menu.container_id,
                state_id,
                slot: index as i16,
                stack: &stack,
            },
        );
    }
    if let Some(carried) = menu.take_carried_change() {
        region.links.send(id, &SetCursorItem { stack: &carried });
    }
}

/// 官方 `broadcastFullState`：每个槽都当成变了，打包成一个 `container_set_content`。
fn broadcast_full_state(region: &mut Region, id: PlayerId, menu: &mut Menu) {
    let items = network_slots(region, id, menu);
    let state_id = menu.take_full_state(&items);
    region.links.send(
        id,
        &ContainerSetContent {
            container_id: menu.container_id,
            state_id,
            items: &items,
            carried: &menu.carried,
        },
    );
}

/// 这名玩家会收到的那一份槽位内容。
///
/// 自己的物品栏、光标和自己的末影箱写完整的补丁：那些字节就是这名玩家的客户端发来的。
/// 共享容器里别人放的格子要滤一遍，见 [`ItemStack::write_for_others`]。
/// 记远程槽的账用的也是这一份，不然记的和发出去的不一样。
fn network_slots(region: &Region, id: PlayerId, menu: &Menu) -> Vec<ItemStack> {
    let (slots, sources) = display(region, id, menu);
    slots
        .into_iter()
        .enumerate()
        .map(|(index, stack)| {
            if index < menu.container_size && sources[index] != Some(id) {
                stack.filtered()
            } else {
                stack
            }
        })
        .collect()
}

/// 菜单里的全部槽位：容器格加上玩家的 36 格。`sources` 只对容器格有意义。
fn display(region: &Region, id: PlayerId, menu: &Menu) -> (Vec<ItemStack>, Vec<Option<PlayerId>>) {
    let (mut slots, mut sources) = raw_container_slots(region, id, menu);
    let Some(player) = region.players.get(&id) else {
        slots.resize(menu.slots_len(), ItemStack::empty());
        sources.resize(menu.slots_len(), None);
        return (slots, sources);
    };
    for index in 0..PLAYER_SLOTS {
        slots.push(player.inventory.slots[INVENTORY_MENU_START + index].clone());
        sources.push(Some(id));
    }
    (slots, sources)
}

/// 容器自己的格。末影箱的内容在玩家身上，组件的来源永远是这名玩家。
fn raw_container_slots(
    region: &Region,
    id: PlayerId,
    menu: &Menu,
) -> (Vec<ItemStack>, Vec<Option<PlayerId>>) {
    if matches!(menu.target, Target::EnderChest(_)) {
        let slots = region.players.get(&id).map_or_else(
            || vec![ItemStack::empty(); menu.container_size],
            |player| player.ender_chest.slots.clone(),
        );
        let sources = vec![Some(id); slots.len()];
        return (slots, sources);
    }
    let mut slots = Vec::with_capacity(menu.container_size);
    let mut sources = Vec::with_capacity(menu.container_size);
    for pos in menu.target.positions() {
        match region.containers.get(&pos) {
            Some(entry) => {
                slots.extend(entry.slots.iter().cloned());
                sources.extend(entry.sources.iter().copied());
            }
            None => {
                let len = menu.kind.slots();
                slots.resize(slots.len() + len, ItemStack::empty());
                sources.resize(sources.len() + len, None);
            }
        }
    }
    (slots, sources)
}

/// 把点击之后的容器格写回方块实体。双箱按位置分成前后两段。
///
/// 漏斗和发射器原来没有条目，这里按需新建，否则点开的菜单编辑会悄悄丢掉。
/// 内容变了的格子要 `setChanged`，比较器才会立刻重算。
fn write_back_containers(
    region: &mut Region,
    menu: &Menu,
    slots: &[ItemStack],
    sources: &[Option<PlayerId>],
) {
    let mut offset = 0;
    let mut changed = Vec::new();
    for pos in menu.target.positions() {
        let len = menu.kind.slots();
        if offset + len <= slots.len() {
            let entry = container::entry_in(&mut region.containers, pos, menu.kind);
            if entry.slots.len() == len {
                if entry.slots.as_slice() != &slots[offset..offset + len] {
                    changed.push(pos);
                }
                entry.slots.clone_from_slice(&slots[offset..offset + len]);
                entry
                    .sources
                    .clone_from_slice(&sources[offset..offset + len]);
            }
        }
        offset += len;
    }
    if changed.is_empty() {
        return;
    }
    let (mut level, ticks) = region.block_level();
    let mut world = World::new(&mut level, ticks);
    for pos in changed {
        set_changed(&mut world, pos);
    }
}

/// 官方 `ItemStack.isSameItemSameComponents`。空物品和空物品也算相同。
fn is_same_item_same_components(a: &ItemStack, b: &ItemStack) -> bool {
    a.is_same_item_same_components(b)
}

/// 官方 `ItemStack.matches`：同种物品、同样组件、数量也相同。
fn stack_matches(a: &ItemStack, b: &ItemStack) -> bool {
    a.matches(b)
}

/// 官方 `ItemStack.isSameItem`，只看物品。
fn is_same_item(a: &ItemStack, b: &ItemStack) -> bool {
    a.is_same_item(b)
}

/// 同一格里的物品只是数量变了：物品和组件字节都没动，来源还是原来那名玩家。
///
/// 换了物品就不算，哪怕两种物品的组件字节碰巧一样；不然别人放的新物品会被当成原来那个人的。
fn same_component_bytes(a: &ItemStack, b: &ItemStack) -> bool {
    a.item == b.item && a.added == b.added && a.removed == b.removed
}

/// 官方 `HashedStack.matches` / `HashedPatchMap.matches`。
///
/// 每一项增加的组件按存档 `Codec` 走 `HashOps`（CRC32C）得到 `HashCode.asInt()`，
/// 再和客户端发来的整数比较。移除项只比组件类型。解不开本地组件、或者某项没有
/// 存档 `Codec`（瞬态组件）时不算匹配，结果只是多发一次槽位同步。
///
/// `HashedStack.matches` / `HashedPatchMap.matches`.
///
/// Each added component is hashed with its persistent codec through `HashOps`
/// (CRC32C) and compared as `HashCode.asInt()`. Removals compare component types
/// only. A component that cannot be decoded, or that has no persistent codec,
/// does not match, so the slot is synchronized again.
fn hashed_matches(hash: &HashedStack, local: &ItemStack) -> bool {
    match hash {
        HashedStack::Empty => local.is_empty(),
        HashedStack::Item(hashed) => {
            if local.is_empty() || hashed.item != local.item || hashed.count != local.count {
                return false;
            }
            let Ok(patch) = mc_bamboo_protocol::components::ComponentPatch::from_opaque(local)
            else {
                return false;
            };
            mc_bamboo_protocol::components::archive::stack::matches_hashed(
                &patch,
                &hashed.added,
                &hashed.removed,
            )
        }
    }
}

/// 官方 `ItemStack.getMaxStackSize`：默认组件被补丁覆盖之后的上限。
fn item_max(stack: &ItemStack) -> i32 {
    stack.max_stack_size()
}

/// 官方 `Slot.getMaxStackSize(stack)`：容器上限和物品上限取小的。
fn stack_limit(stack: &ItemStack) -> i32 {
    CONTAINER_STACK_LIMIT.min(item_max(stack))
}

/// 官方 `ItemStack.isStackable`。
fn is_stackable(stack: &ItemStack) -> bool {
    stack.is_stackable()
}

/// 官方 `ItemStack.copyWithCount`。
fn copy_with_count(stack: &ItemStack, count: i32) -> ItemStack {
    if stack.is_empty() {
        return ItemStack::empty();
    }
    let mut copy = stack.clone();
    copy.count = count;
    copy
}

/// 官方 `ItemStack.split`：拿走 `amount` 个，剩下的留在原来的堆里。
fn split(stack: &mut ItemStack, amount: i32) -> ItemStack {
    let taken = copy_with_count(stack, amount.min(stack.count));
    stack.count -= taken.count;
    if stack.count <= 0 {
        *stack = ItemStack::empty();
    }
    taken
}

/// 物品对应的方块是不是潜影盒，`ShulkerBoxSlot.mayPlace` 要用。
fn is_shulker_box_item(stack: &ItemStack) -> bool {
    let Some(block) = rules::item(stack.item).and_then(|item| item.block) else {
        return false;
    };
    let Some(block) = usize::try_from(block)
        .ok()
        .and_then(|id| blocks::BLOCKS.get(id))
    else {
        return false;
    };
    BlockState(block.default_state).is_a(BlockAncestor::ShulkerBoxBlock)
}

pub mod replay;

#[cfg(test)]
mod tests;
