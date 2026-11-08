//! 讲台上的书和 `LecternMenu`。
//!
//! 书按真实物品存在这一层，比较器用的页码在 `mc-bamboo-blocks` 的红石记忆里。
//! 菜单只有一格，不含玩家物品栏。世界里没有物品实体：物品栏放不下的书、拆掉讲台时台上的书都消失。
//!
//! The book is a real item stack. Comparator facts live in the block layer.
//! The menu has one slot and no player inventory. This world has no item entities,
//! so a book that does not fit, and a book on a lectern that is broken, both disappear.

use std::collections::BTreeMap;

use mc_bamboo_blocks::{
    BlockState, LecternFacts, World, reset_lectern, signal_lectern_page,
    state::{self, HAS_BOOK},
};
use mc_bamboo_core::{BlockPos, Hand};
use mc_bamboo_data::{blocks, items, registries::menu, tags::item_tags};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{
        ContainerClose, ContainerSetContent, ContainerSetData, ContainerSetSlot, GameMode,
        OpenScreen,
    },
    text::Component,
};
use tracing::debug;

use crate::{
    book::{self, PlacedBook},
    container::CONTAINER_BUFFER,
    inbound::PlayerId,
    interaction::{self, send_effects},
    inventory::{self, HOTBAR_START, OFF_HAND, menu_slot},
    menu as menus,
    region::{Region, RegionTick},
};

/// `LecternMenu` 的按钮。100 起是跳页。
const BUTTON_PREV: i32 = 1;
const BUTTON_NEXT: i32 = 2;
const BUTTON_TAKE: i32 = 3;
const BUTTON_JUMP: i32 = 100;
/// `incrementStateId` 的掩码。
const STATE_ID_MASK: i32 = 32767;
/// `Inventory.add` 用的容器上限，官方 `Container.getMaxStackSize` 默认是 99。
const STACK_LIMIT: i32 = 99;
/// 官方 `Inventory.items` 的长度。
const STORAGE_SLOTS: usize = 36;

/// 这一区域里每座讲台的书，以及谁正在看。
#[derive(Debug, Default)]
pub(crate) struct Tables {
    books: BTreeMap<BlockPos, Book>,
    views: BTreeMap<PlayerId, View>,
    pending: BTreeMap<PlayerId, PendingSlots>,
}

/// 放在讲台上的那一本。`page` 为 -1 只出现在总页数是 0 的夹取之后。
#[derive(Debug, Clone)]
struct Book {
    stack: ItemStack,
    page: i32,
    page_count: i32,
    has_content: bool,
    /// 放书的人。别人看到的是 [`ItemStack::filtered`]。
    owner: Option<PlayerId>,
}

/// 一名玩家打开的讲台菜单。
#[derive(Debug, Clone, Copy)]
struct View {
    container_id: i32,
    pos: BlockPos,
    state_id: i32,
    remote_page: i32,
    remote_empty: bool,
}

/// 取书之后要发给背包菜单的格子。讲台还开着时先攒着。
#[derive(Debug, Default)]
struct PendingSlots {
    slots: Vec<usize>,
    deliver_at: RegionTick,
}

/// 菜单 oracle 重放 `LecternMenu` 之后的样子。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LecternSnapshot {
    pub state_id: i32,
    pub carried: ItemStack,
    pub slots: Vec<ItemStack>,
    pub page: i32,
}

/// 官方 `LecternMenu` 的按钮序列，不碰世界。
///
/// 页码夹取和 `removeItemNoUpdate` 清页跟 `LecternBlockEntity` 一样。
/// 打开时 `state_id` 是 1。翻页不推进它，取书清空槽位才推进。
///
/// Button replay of `LecternMenu` with no world attached. Page clamping and the
/// page reset in `removeItemNoUpdate` follow `LecternBlockEntity`. Opening leaves
/// `state_id` at 1. Turning a page does not advance it; taking the book does.
#[derive(Debug, Clone)]
pub struct LecternReplay {
    item: i32,
    count: i32,
    page: i32,
    page_count: i32,
    may_build: bool,
    state_id: i32,
    empty: bool,
}

impl LecternReplay {
    /// 打开菜单。`page_count` 是书的页数，起始页是 0。
    ///
    /// Opens the menu on page 0. `page_count` is the book's page count.
    pub fn open(
        item: i32,
        count: i32,
        page_count: i32,
        may_build: bool,
    ) -> (Self, LecternSnapshot) {
        let replay = Self {
            item,
            count,
            page: 0,
            page_count,
            may_build,
            state_id: 1,
            empty: false,
        };
        let snapshot = replay.snapshot();
        (replay, snapshot)
    }

    /// `handleContainerButtonClick`：先 `stillValid`，再 `clickMenuButton`，成功再广播。
    ///
    /// `stillValid` first, then `clickMenuButton`, then one `broadcastChanges` on success.
    pub fn press(&mut self, button_id: i32) -> LecternSnapshot {
        if self.empty {
            return self.snapshot();
        }
        if button_id == BUTTON_TAKE {
            if self.may_build {
                self.empty = true;
                self.page = 0;
                self.page_count = 0;
                self.state_id = (self.state_id + 1) & STATE_ID_MASK;
            }
            return self.snapshot();
        }
        if button_id == BUTTON_PREV || button_id == BUTTON_NEXT || button_id >= BUTTON_JUMP {
            let requested = requested_page(self.page, button_id);
            self.page = clamp_page(requested, self.page_count);
        }
        self.snapshot()
    }

    fn snapshot(&self) -> LecternSnapshot {
        let slot = if self.empty {
            ItemStack::empty()
        } else {
            ItemStack::new(self.item, self.count)
        };
        LecternSnapshot {
            state_id: self.state_id,
            carried: ItemStack::empty(),
            slots: vec![slot],
            page: self.page,
        }
    }
}

/// 这名玩家正看着讲台。箱子菜单的同步要躲开容器 0。
pub(crate) fn menu_open(region: &Region, id: PlayerId) -> bool {
    region.lecterns.views.contains_key(&id)
}

/// 讲台还开着，这一格等它关上再发给背包菜单。
pub(crate) fn defer_slot(region: &mut Region, id: PlayerId, slot: usize) {
    let at = region.tick.plus(2);
    let pending = region.lecterns.pending.entry(id).or_default();
    if !pending.slots.contains(&slot) {
        pending.slots.push(slot);
    }
    pending.deliver_at = at;
}

/// 右键空讲台之前，把手里这本书的页数交给方块层。
///
/// Deposits page facts before the block use, so `placeBook` can record them.
pub(crate) fn arm_book(region: &mut Region, id: PlayerId, pos: BlockPos, hand: Hand) -> bool {
    let Some(player) = region.players.get(&id) else {
        return false;
    };
    let stack = player.inventory.held(hand);
    if stack.is_empty() || !state::item_is_tagged(stack.item, item_tags::LECTERN_BOOKS) {
        return false;
    }
    let Some(block) = lectern_state(region, pos) else {
        return false;
    };
    if block.get(HAS_BOOK) {
        return false;
    }
    let PlacedBook {
        page_count,
        has_content,
    } = book::placed_book(stack);
    region.redstone.set_pending_lectern(LecternFacts {
        page: 0,
        page_count,
        has_content,
    });
    true
}

/// 放书成功就收下那一本副本，创造模式不消耗。没放上的预记页数丢掉。
///
/// Stores the placed copy. Creative mode does not consume. Leftover pending facts are dropped.
pub(crate) fn complete_use(
    region: &mut Region,
    id: PlayerId,
    pos: BlockPos,
    hand: Hand,
    armed: bool,
) {
    let pending_left = region.redstone.take_pending_lectern().is_some();
    if !armed || pending_left {
        return;
    }
    if !lectern_state(region, pos).is_some_and(|state| state.get(HAS_BOOK)) {
        return;
    }
    let Some(slot) = hand_slot(region, id, hand) else {
        return;
    };
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let mut copy = player.inventory.slots[slot].clone();
    if copy.is_empty() {
        return;
    }
    copy.count = 1;
    book::resolve_for_item(&mut copy);
    let creative = player.game_mode == GameMode::Creative;
    let PlacedBook {
        page_count,
        has_content,
    } = book::placed_book(&copy);
    region.lecterns.books.insert(
        pos,
        Book {
            stack: copy,
            page: 0,
            page_count,
            has_content,
            owner: Some(id),
        },
    );
    if creative {
        return;
    }
    let spent = {
        let Some(player) = region.players.get_mut(&id) else {
            return;
        };
        let stack = &mut player.inventory.slots[slot];
        if stack.is_empty() {
            false
        } else {
            stack.count -= 1;
            if stack.count <= 0 {
                *stack = ItemStack::empty();
            }
            true
        }
    };
    if spent {
        book::sync_edited_slot(region, id, slot);
    }
}

/// `useWithoutItem` 里的 `openScreen`。没有书时返回 `false`。
///
/// `LecternBlock.useWithoutItem` opens the menu when the lectern has a book.
pub(crate) fn open(region: &mut Region, id: PlayerId, pos: BlockPos) -> bool {
    if !lectern_state(region, pos).is_some_and(|state| state.get(HAS_BOOK)) {
        return false;
    }
    if !region.players.contains_key(&id) {
        return false;
    }
    ensure_book(region, pos);
    if region.menus.contains_key(&id) {
        menus::close_menu(region, id);
    }
    close_view(region, id, true);
    let Some(container_id) = next_container_id(region, id) else {
        return false;
    };
    send_open(region, id, pos, container_id);
    true
}

/// 打开箱子之前先关掉讲台，官方 `openMenu` 会先 `closeContainer`。
pub(crate) fn close_for_other_menu(region: &mut Region, id: PlayerId) {
    close_view(region, id, true);
}

/// 客户端自己关了菜单，不再回 `container_close`。
pub(crate) fn on_player_close(region: &mut Region, id: PlayerId) {
    close_view(region, id, false);
}

/// 玩家离开。连接已经拆了，不发包。书留在讲台上。
pub(crate) fn on_leave(region: &mut Region, id: PlayerId) {
    close_view(region, id, false);
    region.lecterns.pending.remove(&id);
}

/// `handleContainerButtonClick`。旁观者的按钮全部忽略。
pub(crate) fn on_button(region: &mut Region, id: PlayerId, container_id: i32, button_id: i32) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if player.game_mode == GameMode::Spectator {
        return;
    }
    let may_build = player.game_mode != GameMode::Adventure;
    let Some(pos) = region
        .lecterns
        .views
        .get(&id)
        .filter(|view| view.container_id == container_id)
        .map(|view| view.pos)
    else {
        return;
    };
    if !still_valid(region, id, pos) {
        debug!(
            player = id.0,
            "ignoring a lectern button; the menu is no longer valid"
        );
        return;
    }
    if button_id == BUTTON_TAKE {
        if may_build {
            take_book(region, id, pos);
        }
        return;
    }
    if button_id != BUTTON_PREV && button_id != BUTTON_NEXT && button_id < BUTTON_JUMP {
        return;
    }
    if turn_page(region, pos, button_id) {
        let effects = with_lectern(region, |world| signal_lectern_page(world, pos));
        send_effects(region, None, &effects);
    }
    sync_page(region, id);
}

/// 先同步还开着的菜单，再丢掉已经不是讲台的书，最后补发物品栏。
pub(crate) fn tick(region: &mut Region) {
    let viewers: Vec<PlayerId> = region.lecterns.views.keys().copied().collect();
    for id in viewers {
        sync_view(region, id);
    }
    let stale: Vec<BlockPos> = region
        .lecterns
        .books
        .keys()
        .copied()
        .filter(|pos| !block_keeps_book(region, *pos))
        .collect();
    for pos in stale {
        region.lecterns.books.remove(&pos);
        debug!(
            ?pos,
            "a lectern book disappeared with its block; this world has no item entities"
        );
    }
    flush_pending(region);
}

fn sync_view(region: &mut Region, id: PlayerId) {
    let Some(pos) = region.lecterns.views.get(&id).map(|view| view.pos) else {
        return;
    };
    let book_here = region.lecterns.books.contains_key(&pos);
    if !book_here {
        notify_empty(region, id);
        close_view(region, id, true);
        return;
    }
    if !block_keeps_book(region, pos) {
        close_view(region, id, true);
        return;
    }
    sync_page(region, id);
    if !still_valid(region, id, pos) {
        debug!(
            player = id.0,
            "closing a lectern menu whose block is gone or out of reach"
        );
        close_view(region, id, true);
    }
}

fn turn_page(region: &mut Region, pos: BlockPos, button_id: i32) -> bool {
    let Some(book) = region.lecterns.books.get_mut(&pos) else {
        return false;
    };
    let next = clamp_page(requested_page(book.page, button_id), book.page_count);
    if next == book.page {
        return false;
    }
    book.page = next;
    let facts = LecternFacts {
        page: book.page,
        page_count: book.page_count,
        has_content: book.has_content,
    };
    region.redstone.set_lectern(pos, facts);
    true
}

fn take_book(region: &mut Region, id: PlayerId, pos: BlockPos) {
    let Some(book) = region.lecterns.books.remove(&pos) else {
        return;
    };
    let effects = with_lectern(region, |world| {
        reset_lectern(world, pos, false);
        let block = world.block_state(pos).block();
        world.update_neighbour_for_output_signal(pos, block);
    });
    send_effects(region, None, &effects);
    remember_return(region, id, book.stack);
    notify_empty(region, id);
}

fn remember_return(region: &mut Region, id: PlayerId, stack: ItemStack) {
    let Some(player) = region.players.get_mut(&id) else {
        debug!(
            player = id.0,
            "dropping a lectern book; the player is already gone and this world has no item entities"
        );
        return;
    };
    let creative = player.game_mode == GameMode::Creative;
    let (kept, slots) = give(&mut player.inventory, creative, stack);
    if !kept {
        debug!(
            player = id.0,
            "dropping a lectern book; the inventory is full and this world has no item entities"
        );
    }
    if slots.is_empty() {
        return;
    }
    let pending = region.lecterns.pending.entry(id).or_default();
    for slot in slots {
        if !pending.slots.contains(&slot) {
            pending.slots.push(slot);
        }
    }
    pending.deliver_at = region.tick.plus(2);
}

fn flush_pending(region: &mut Region) {
    let due: Vec<PlayerId> = region
        .lecterns
        .pending
        .iter()
        .filter(|(_, pending)| region.tick.reached(pending.deliver_at))
        .map(|(id, _)| *id)
        .collect();
    for id in due {
        if region.menus.contains_key(&id) {
            region.lecterns.pending.remove(&id);
            continue;
        }
        if region.lecterns.views.contains_key(&id) {
            if let Some(pending) = region.lecterns.pending.get_mut(&id) {
                pending.deliver_at = region.tick.plus(1);
            }
            continue;
        }
        let Some(pending) = region.lecterns.pending.remove(&id) else {
            continue;
        };
        for slot in pending.slots {
            let Some(player) = region.players.get_mut(&id) else {
                continue;
            };
            inventory::send_slot(player, &mut region.links, slot);
        }
    }
}

fn ensure_book(region: &mut Region, pos: BlockPos) {
    if region.lecterns.books.contains_key(&pos) {
        return;
    }
    let stack = ItemStack::new(items::WRITABLE_BOOK, 1);
    let placed = book::placed_book(&stack);
    // 已经有比较器记录时沿用它的页数。没有记录才按这本默认可写书补上。
    // Keep comparator facts that are already stored. Otherwise record this default book.
    let recorded = region.redstone.lectern(pos);
    let page = recorded.map(|facts| facts.page).unwrap_or(0);
    let page_count = recorded
        .map(|facts| facts.page_count)
        .unwrap_or(placed.page_count);
    let has_content = recorded
        .map(|facts| facts.has_content)
        .unwrap_or(placed.has_content);
    if recorded.is_none() {
        region.redstone.set_lectern(
            pos,
            LecternFacts {
                page,
                page_count,
                has_content,
            },
        );
    }
    region.lecterns.books.insert(
        pos,
        Book {
            stack,
            page,
            page_count,
            has_content,
            owner: None,
        },
    );
}

fn send_open(region: &mut Region, id: PlayerId, pos: BlockPos, container_id: i32) {
    let Some((shown, page)) = region.lecterns.books.get(&pos).map(|book| {
        let shown = if book.owner == Some(id) {
            book.stack.clone()
        } else {
            book.stack.filtered()
        };
        (shown, book.page)
    }) else {
        return;
    };
    let title = Component::translate("container.lectern", []);
    let carried = ItemStack::empty();
    region.links.send(
        id,
        &OpenScreen {
            container_id,
            menu_type: menu::LECTERN,
            title: &title,
        },
    );
    region.links.send(
        id,
        &ContainerSetContent {
            container_id,
            state_id: 1,
            items: std::slice::from_ref(&shown),
            carried: &carried,
        },
    );
    region.links.send(
        id,
        &ContainerSetData {
            container_id,
            id: 0,
            value: page as i16,
        },
    );
    region.lecterns.views.insert(
        id,
        View {
            container_id,
            pos,
            state_id: 1,
            remote_page: page,
            remote_empty: shown.is_empty(),
        },
    );
}

fn sync_page(region: &mut Region, id: PlayerId) {
    let Some(view) = region.lecterns.views.get(&id).copied() else {
        return;
    };
    let Some(page) = region.lecterns.books.get(&view.pos).map(|book| book.page) else {
        return;
    };
    if page == view.remote_page {
        return;
    }
    region.links.send(
        id,
        &ContainerSetData {
            container_id: view.container_id,
            id: 0,
            value: page as i16,
        },
    );
    if let Some(view) = region.lecterns.views.get_mut(&id) {
        view.remote_page = page;
    }
}

fn notify_empty(region: &mut Region, id: PlayerId) {
    let Some(view) = region.lecterns.views.get(&id).copied() else {
        return;
    };
    let mut state_id = view.state_id;
    if !view.remote_empty {
        state_id = (state_id + 1) & STATE_ID_MASK;
        let empty = ItemStack::empty();
        region.links.send(
            id,
            &ContainerSetSlot {
                container_id: view.container_id,
                state_id,
                slot: 0,
                stack: &empty,
            },
        );
    }
    if view.remote_page != 0 {
        region.links.send(
            id,
            &ContainerSetData {
                container_id: view.container_id,
                id: 0,
                value: 0,
            },
        );
    }
    if let Some(view) = region.lecterns.views.get_mut(&id) {
        view.state_id = state_id;
        view.remote_page = 0;
        view.remote_empty = true;
    }
}

fn close_view(region: &mut Region, id: PlayerId, notify: bool) {
    let Some(view) = region.lecterns.views.remove(&id) else {
        return;
    };
    if notify {
        region.links.send(
            id,
            &ContainerClose {
                container_id: view.container_id,
            },
        );
    }
}

fn next_container_id(region: &mut Region, id: PlayerId) -> Option<i32> {
    let player = region.players.get_mut(&id)?;
    player.container_counter = player.container_counter % 100 + 1;
    Some(player.container_counter)
}

fn still_valid(region: &Region, id: PlayerId, pos: BlockPos) -> bool {
    let Some(player) = region.players.get(&id) else {
        return false;
    };
    if !block_keeps_book(region, pos) {
        return false;
    }
    let Some(book) = region.lecterns.books.get(&pos) else {
        return false;
    };
    book.has_content
        && interaction::within_block_interaction_range(
            player.position(),
            player.eye_height(),
            interaction::interaction_range(player.game_mode),
            pos,
            CONTAINER_BUFFER,
        )
}

fn with_lectern(
    region: &mut Region,
    body: impl FnOnce(&mut World),
) -> Vec<mc_bamboo_blocks::Effect> {
    let (mut level, ticks) = region.block_level();
    let mut world = World::new(&mut level, ticks);
    body(&mut world);
    world.drain_effects()
}

fn lectern_state(region: &Region, pos: BlockPos) -> Option<BlockState> {
    let state = BlockState(region.chunks.block(pos)?);
    state
        .is_same_block(BlockState(blocks::LECTERN))
        .then_some(state)
}

fn block_keeps_book(region: &Region, pos: BlockPos) -> bool {
    lectern_state(region, pos).is_some_and(|state| state.get(HAS_BOOK))
}

fn hand_slot(region: &Region, id: PlayerId, hand: Hand) -> Option<usize> {
    let player = region.players.get(&id)?;
    Some(match hand {
        Hand::Main => HOTBAR_START + usize::from(player.inventory.selected),
        Hand::Off => OFF_HAND,
    })
}

/// `Mth.clamp(page, 0, pageCount - 1)`。`pageCount` 为 0 时上限是 -1，结果是 -1。
///
/// Java's `Math.min(Math.max(page, 0), pageCount - 1)`. A page count of 0 clamps to -1.
fn clamp_page(page: i32, page_count: i32) -> i32 {
    page.max(0).min(page_count.wrapping_sub(1))
}

fn requested_page(page: i32, button_id: i32) -> i32 {
    if button_id >= BUTTON_JUMP {
        button_id.wrapping_sub(BUTTON_JUMP)
    } else if button_id == BUTTON_PREV {
        page.wrapping_sub(1)
    } else {
        page.wrapping_add(1)
    }
}

/// 官方 `Inventory.add`。放不下时返回 `false`，调用方丢掉剩下的。
///
/// 创造模式在一次也塞不进去时把剩余数量清零并返回 `true`，物品消失，不掉在地上。
///
/// Vanilla `Inventory.add`. `false` means the caller drops what is left.
/// Creative mode clears the leftover and returns `true` when nothing more fits.
fn give(
    inventory: &mut crate::inventory::Inventory,
    creative: bool,
    mut stack: ItemStack,
) -> (bool, Vec<usize>) {
    if stack.is_empty() {
        return (false, Vec::new());
    }
    let mut changed = Vec::new();
    let mut last;
    loop {
        last = stack.count;
        stack.count = add_resource(inventory, &stack, &mut changed);
        if stack.is_empty() || stack.count >= last {
            break;
        }
    }
    if stack.count == last && creative {
        return (true, changed);
    }
    (stack.count < last, changed)
}

fn add_resource(
    inventory: &mut crate::inventory::Inventory,
    stack: &ItemStack,
    changed: &mut Vec<usize>,
) -> i32 {
    let slot = remaining_slot(inventory, stack).or_else(|| free_slot(inventory));
    match slot {
        Some(slot) => add_at(inventory, slot, stack, changed),
        None => stack.count,
    }
}

fn remaining_slot(inventory: &crate::inventory::Inventory, incoming: &ItemStack) -> Option<usize> {
    let selected = HOTBAR_START + usize::from(inventory.selected);
    std::iter::once(selected)
        .chain(std::iter::once(OFF_HAND))
        .chain((0..STORAGE_SLOTS).map(menu_slot))
        .find(|&slot| has_room(&inventory.slots[slot], incoming))
}

fn has_room(slot_stack: &ItemStack, incoming: &ItemStack) -> bool {
    !slot_stack.is_empty()
        && slot_stack.is_same_item_same_components(incoming)
        && slot_stack.is_stackable()
        && slot_stack.count < limit_of(slot_stack)
}

fn free_slot(inventory: &crate::inventory::Inventory) -> Option<usize> {
    (0..STORAGE_SLOTS)
        .map(menu_slot)
        .find(|&slot| inventory.slots[slot].is_empty())
}

fn add_at(
    inventory: &mut crate::inventory::Inventory,
    slot: usize,
    incoming: &ItemStack,
    changed: &mut Vec<usize>,
) -> i32 {
    let slot_stack = &mut inventory.slots[slot];
    if slot_stack.is_empty() {
        *slot_stack = incoming.clone();
        slot_stack.count = 0;
    }
    let limit = limit_of(slot_stack);
    if slot_stack.count >= limit {
        return incoming.count;
    }
    let added = incoming.count.min(limit - slot_stack.count);
    slot_stack.count += added;
    if !changed.contains(&slot) {
        changed.push(slot);
    }
    incoming.count - added
}

fn limit_of(stack: &ItemStack) -> i32 {
    stack.max_stack_size().min(STACK_LIMIT)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::state::{HAS_BOOK, POWERED};
    use mc_bamboo_core::{BlockPos, Hand};
    use mc_bamboo_data::{
        blocks, items, packets::play::clientbound, registries::data_component_type as kinds,
    };
    use mc_bamboo_protocol::{
        components::{ComponentValue, value::Filterable},
        item::ItemStack,
        packet::play::{GameMode, PlayRequest},
        text::Component,
    };

    use super::*;
    use crate::{
        inventory::{HOTBAR_START, menu_slot},
        world::tests::{Client, Harness, frames_with_id},
    };

    const POS: BlockPos = BlockPos::new(8, -60, 8);

    fn settled() -> (Harness, Client) {
        let mut server = Harness::new(8);
        let client = server.join_and_settle("Kitten");
        client.take_frames();
        (server, client)
    }

    fn lectern(server: &mut Harness) {
        assert!(server.region().chunks.set_block(POS, blocks::LECTERN));
    }

    fn give(server: &mut Harness, client: &Client, stack: ItemStack) {
        server
            .region()
            .players
            .get_mut(&client.id)
            .unwrap()
            .inventory
            .slots[HOTBAR_START] = stack;
    }

    fn mode(server: &mut Harness, client: &Client, game_mode: GameMode) {
        server
            .region()
            .players
            .get_mut(&client.id)
            .unwrap()
            .game_mode = game_mode;
    }

    fn use_on(server: &Harness, client: &Client) {
        server.request(
            client,
            PlayRequest::UseItemOn {
                hand: Hand::Main,
                pos: POS,
                face: mc_bamboo_core::Direction::Up,
                cursor: (0.5, 1.0, 0.5),
                inside: false,
                world_border_hit: false,
                sequence: 1,
            },
        );
    }

    fn use_air(server: &Harness, client: &Client, hand: Hand) {
        server.request(
            client,
            PlayRequest::UseItem {
                hand,
                sequence: 1,
                yaw: 0.0,
                pitch: 0.0,
            },
        );
    }

    fn button(server: &Harness, client: &Client, button_id: i32) {
        server.request(
            client,
            PlayRequest::ContainerButtonClick {
                container_id: 1,
                button_id,
            },
        );
    }

    fn state_of(server: &mut Harness) -> BlockState {
        BlockState(server.region().chunks.block(POS).unwrap())
    }

    fn pages(text: &[&str]) -> ItemStack {
        let mut stack = ItemStack::new(items::WRITABLE_BOOK, 1);
        let value = ComponentValue::WritableBookContent(
            text.iter()
                .map(|page| Filterable {
                    raw: (*page).to_owned(),
                    filtered: None,
                })
                .collect(),
        );
        let mut bytes = Vec::new();
        value.write(&mut bytes);
        stack.added.push((kinds::WRITABLE_BOOK_CONTENT, bytes));
        stack
    }

    fn written(resolved: bool) -> ItemStack {
        let mut stack = ItemStack::new(items::WRITTEN_BOOK, 1);
        let value = ComponentValue::WrittenBookContent {
            title: Filterable {
                raw: "Title".to_owned(),
                filtered: None,
            },
            author: "Kitten".to_owned(),
            generation: 0,
            pages: vec![Filterable {
                raw: Component::text("Hello"),
                filtered: None,
            }],
            resolved,
        };
        let mut bytes = Vec::new();
        value.write(&mut bytes);
        stack.added.push((kinds::WRITTEN_BOOK_CONTENT, bytes));
        stack
    }

    fn fill_storage(server: &mut Harness, client: &Client) {
        let inventory = &mut server
            .region()
            .players
            .get_mut(&client.id)
            .unwrap()
            .inventory;
        for index in 0..STORAGE_SLOTS {
            inventory.slots[menu_slot(index)] = ItemStack::new(items::STONE, 64);
        }
    }

    #[test]
    fn survival_places_one_book_and_creative_does_not_spend_it() {
        let (mut server, client) = settled();
        lectern(&mut server);
        mode(&mut server, &client, GameMode::Survival);
        let mut book = pages(&["a", "b", "c"]);
        book.count = 2;
        give(&mut server, &client, book);
        use_on(&server, &client);
        server.tick();

        let held = &server.player(&client).inventory.slots[HOTBAR_START];
        assert_eq!(held.count, 1);
        assert!(state_of(&mut server).get(HAS_BOOK));
        let facts = server.region().redstone.lectern(POS).unwrap();
        assert_eq!(facts.page, 0);
        assert_eq!(facts.page_count, 3);
        assert_eq!(facts.signal(), 1);
        assert_eq!(server.region().lecterns.books[&POS].stack.count, 1);

        mode(&mut server, &client, GameMode::Creative);
        server.region().chunks.set_block(POS, blocks::LECTERN);
        server.region().lecterns.books.remove(&POS);
        server.region().redstone.clear_lectern(POS);
        give(&mut server, &client, pages(&["a"]));
        use_on(&server, &client);
        server.tick();
        assert_eq!(
            server.player(&client).inventory.slots[HOTBAR_START].count,
            1
        );
        assert!(state_of(&mut server).get(HAS_BOOK));
    }

    #[test]
    fn an_unedited_book_and_quill_reads_fifteen_and_a_bare_written_book_reads_fourteen() {
        let (mut server, client) = settled();
        lectern(&mut server);
        give(
            &mut server,
            &client,
            ItemStack::new(items::WRITABLE_BOOK, 1),
        );
        use_on(&server, &client);
        server.tick();
        assert_eq!(server.region().redstone.lectern(POS).unwrap().signal(), 15);

        server.region().chunks.set_block(POS, blocks::AIR);
        server.region().lecterns.books.remove(&POS);
        server.region().redstone.clear_lectern(POS);
        lectern(&mut server);
        give(&mut server, &client, ItemStack::new(items::WRITTEN_BOOK, 1));
        use_on(&server, &client);
        server.tick();
        assert_eq!(server.region().redstone.lectern(POS).unwrap().signal(), 14);
    }

    #[test]
    fn opening_paging_and_taking_follow_the_menu() {
        let (mut server, client) = settled();
        lectern(&mut server);
        give(&mut server, &client, pages(&["a", "b", "c"]));
        use_on(&server, &client);
        server.tick();
        client.take_frames();

        use_on(&server, &client);
        server.tick();
        let frames = client.take_frames();
        assert_eq!(frames_with_id(&frames, clientbound::OPEN_SCREEN).len(), 1);
        assert_eq!(
            frames_with_id(&frames, clientbound::CONTAINER_SET_CONTENT).len(),
            1
        );
        let data = frames_with_id(&frames, clientbound::CONTAINER_SET_DATA);
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].body, vec![1, 0, 0, 0, 0]);

        button(&server, &client, BUTTON_NEXT);
        server.tick();
        assert_eq!(server.region().redstone.lectern(POS).unwrap().page, 1);
        assert_eq!(server.region().redstone.lectern(POS).unwrap().signal(), 8);
        assert!(state_of(&mut server).get(POWERED));
        let frames = client.take_frames();
        assert!(
            frames
                .iter()
                .any(|frame| frame.id == clientbound::LEVEL_EVENT
                    && frame.body.starts_with(&1043_i32.to_be_bytes()))
        );
        server.tick();
        assert!(!state_of(&mut server).get(POWERED));

        button(&server, &client, 102);
        server.tick();
        assert_eq!(server.region().redstone.lectern(POS).unwrap().page, 2);
        assert_eq!(server.region().redstone.lectern(POS).unwrap().signal(), 15);

        button(&server, &client, BUTTON_TAKE);
        server.tick();
        assert!(!state_of(&mut server).get(HAS_BOOK));
        assert!(!state_of(&mut server).get(POWERED));
        let frames = client.take_frames();
        assert!(frames_with_id(&frames, clientbound::CONTAINER_CLOSE).len() == 1);
        assert!(
            server
                .player(&client)
                .inventory
                .slots
                .iter()
                .any(|stack| { stack.item == items::WRITABLE_BOOK && stack.count == 1 })
        );
        server.tick();
        let frames = client.take_frames();
        assert!(
            frames_with_id(&frames, clientbound::CONTAINER_SET_SLOT)
                .iter()
                .any(|frame| frame.body.first() == Some(&0))
        );
    }

    #[test]
    fn a_full_inventory_loses_the_book_and_breaking_the_lectern_does_too() {
        let (mut server, client) = settled();
        lectern(&mut server);
        mode(&mut server, &client, GameMode::Survival);
        give(&mut server, &client, pages(&["a"]));
        use_on(&server, &client);
        server.tick();
        fill_storage(&mut server, &client);
        use_on(&server, &client);
        server.tick();
        button(&server, &client, BUTTON_TAKE);
        server.tick();
        assert!(
            !server
                .player(&client)
                .inventory
                .slots
                .iter()
                .any(|stack| stack.item == items::WRITABLE_BOOK)
        );
        assert!(!state_of(&mut server).get(HAS_BOOK));

        mode(&mut server, &client, GameMode::Creative);
        give(&mut server, &client, ItemStack::new(items::WRITTEN_BOOK, 1));
        use_on(&server, &client);
        server.tick();
        assert!(server.region().lecterns.books.contains_key(&POS));
        server.request(
            &client,
            PlayRequest::PlayerAction {
                action: mc_bamboo_protocol::packet::play::PlayerActionKind::StartDestroyBlock,
                pos: POS,
                face: mc_bamboo_core::Direction::Up,
                sequence: 2,
            },
        );
        server.tick();
        assert!(!server.region().lecterns.books.contains_key(&POS));
        assert_eq!(
            server.player(&client).inventory.slots[HOTBAR_START].item,
            items::WRITTEN_BOOK
        );
    }

    #[test]
    fn another_viewer_hears_the_new_page_on_the_same_tick() {
        let (mut server, first) = settled();
        let second = server.join_and_settle("Bob");
        second.take_frames();
        lectern(&mut server);
        give(&mut server, &first, pages(&["a", "b"]));
        use_on(&server, &first);
        server.tick();
        use_on(&server, &first);
        use_on(&server, &second);
        server.tick();
        first.take_frames();
        second.take_frames();

        button(&server, &first, BUTTON_NEXT);
        server.tick();
        let frames = second.take_frames();
        assert!(
            frames_with_id(&frames, clientbound::CONTAINER_SET_DATA)
                .iter()
                .any(|frame| frame.body.ends_with(&1_i16.to_be_bytes()))
        );
    }

    #[test]
    fn using_a_written_book_resolves_it_and_opens_it() {
        let (mut server, client) = settled();
        give(&mut server, &client, written(false));
        use_air(&server, &client, Hand::Main);
        server.tick();
        let stack = &server.player(&client).inventory.slots[HOTBAR_START];
        assert!(matches!(
            stack.patch().get(kinds::WRITTEN_BOOK_CONTENT),
            Some(ComponentValue::WrittenBookContent { resolved: true, .. })
        ));
        let frames = client.take_frames();
        let open = frames_with_id(&frames, clientbound::OPEN_BOOK);
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].body, vec![0]);
        let slot = frames_with_id(&frames, clientbound::CONTAINER_SET_SLOT);
        assert_eq!(slot.len(), 1);
        let slot_at = frames
            .iter()
            .position(|frame| frame.id == clientbound::CONTAINER_SET_SLOT);
        let open_at = frames
            .iter()
            .position(|frame| frame.id == clientbound::OPEN_BOOK);
        assert!(slot_at < open_at);

        client.take_frames();
        give(&mut server, &client, written(true));
        use_air(&server, &client, Hand::Main);
        server.tick();
        let frames = client.take_frames();
        assert!(frames_with_id(&frames, clientbound::CONTAINER_SET_SLOT).is_empty());
        assert_eq!(frames_with_id(&frames, clientbound::OPEN_BOOK).len(), 1);

        give(
            &mut server,
            &client,
            ItemStack::new(items::WRITABLE_BOOK, 1),
        );
        use_air(&server, &client, Hand::Main);
        server.tick();
        let frames = client.take_frames();
        assert!(frames_with_id(&frames, clientbound::OPEN_BOOK).is_empty());
    }

    #[test]
    fn an_empty_book_clamps_to_minus_one_without_moving_the_state_id() {
        let (mut replay, open) = LecternReplay::open(items::WRITTEN_BOOK, 1, 0, true);
        assert_eq!(open.state_id, 1);
        assert_eq!(open.page, 0);
        let turned = replay.press(BUTTON_NEXT);
        assert_eq!(turned.page, -1);
        assert_eq!(turned.state_id, 1);
        let taken = replay.press(BUTTON_TAKE);
        assert_eq!(taken.page, 0);
        assert_eq!(taken.state_id, 2);
        assert!(taken.slots[0].is_empty());
        let ignored = replay.press(BUTTON_PREV);
        assert_eq!(ignored.state_id, 2);
        assert!(ignored.slots[0].is_empty());
    }

    #[test]
    fn adventure_mode_can_turn_pages_but_cannot_take_the_book() {
        let (mut replay, _) = LecternReplay::open(items::WRITABLE_BOOK, 1, 3, false);
        let stayed = replay.press(BUTTON_TAKE);
        assert_eq!(stayed.slots[0].count, 1);
        assert_eq!(stayed.state_id, 1);
        let turned = replay.press(102);
        assert_eq!(turned.page, 2);
        assert_eq!(turned.state_id, 1);
    }
}
