//! 书与笔的编辑和署名。
//!
//! 官方 `ServerGamePacketListenerImpl.handleEditBook`、`updateBookContents`、`signBook`。
//! 没有聊天过滤，`TextFilter.DUMMY` 给出的 `FilteredText` 是原文，
//! `filterableFromOutgoing` 因此没有 `filtered`。

use mc_bamboo_data::{items, registries::data_component_type as kinds};
use mc_bamboo_protocol::{
    codec::DecodeError,
    components::{ComponentValue, cursor::Cursor, defaults, value::Filterable},
    item::ItemStack,
    text::Component,
};

use crate::{
    inbound::PlayerId,
    inventory::{self, HOTBAR_START, OFF_HAND},
    region::Region,
};

/// 官方 `Inventory.isHotbarSlot`：快捷栏是 0 到 8。副手是另一项判断，下标 40。
fn menu_slot_for(slot: i32) -> Option<usize> {
    match slot {
        0..=8 => Some(HOTBAR_START + slot as usize),
        40 => Some(OFF_HAND),
        _ => None,
    }
}

/// `edit_book`。槽位不合法、物品没有 `writable_book_content` 时什么也不做，和官方一样。
pub(crate) fn on_edit_book(
    region: &mut Region,
    id: PlayerId,
    slot: i32,
    pages: Vec<String>,
    title: Option<String>,
) {
    let Some(menu_slot) = menu_slot_for(slot) else {
        return;
    };
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    if !has_component(
        &player.inventory.slots[menu_slot],
        kinds::WRITABLE_BOOK_CONTENT,
    ) {
        return;
    }

    // 作者是档案名，官方 `getPlainTextName`。离线模式就是进服时的名字。
    let author = player.name.clone();
    let changed = {
        let stack = &mut player.inventory.slots[menu_slot];
        let before = stack.clone();
        match title {
            Some(title) => sign_book(stack, &title, &pages, &author),
            None => edit_pages(stack, &pages),
        }
        *stack != before
    };
    if changed {
        inventory::send_slot(player, &mut region.links, menu_slot);
    }
}

/// 官方 `updateBookContents`：整份换成新的页面，物品种类不变。
fn edit_pages(stack: &mut ItemStack, pages: &[String]) {
    let content =
        ComponentValue::WritableBookContent(pages.iter().map(|page| plain(page.clone())).collect());
    set_component(stack, kinds::WRITABLE_BOOK_CONTENT, &content);
}

/// 官方 `signBook`：`transmuteCopy(WRITTEN_BOOK)`，去掉书与笔的内容，写上成书。
fn sign_book(stack: &mut ItemStack, title: &str, pages: &[String], author: &str) {
    *stack = transmute_copy(stack, items::WRITTEN_BOOK);
    remove_component(stack, kinds::WRITABLE_BOOK_CONTENT);
    set_component(
        stack,
        kinds::WRITTEN_BOOK_CONTENT,
        &written_content(title, pages, author),
    );
}

/// 代数 0、`resolved` 为真。页面是 `Component.literal`，没有过滤文本。
fn written_content(title: &str, pages: &[String], author: &str) -> ComponentValue {
    ComponentValue::WrittenBookContent {
        title: plain(title.to_owned()),
        author: author.to_owned(),
        generation: 0,
        pages: pages
            .iter()
            .map(|page| plain(Component::text(page.as_str())))
            .collect(),
        resolved: true,
    }
}

/// 没有过滤时的 `Filterable`：只有原文。
fn plain<T>(raw: T) -> Filterable<T> {
    Filterable {
        raw,
        filtered: None,
    }
}

/// 官方 `ItemStack.has`：空物品没有组件；补丁标了移除就没有；否则补丁或默认组件里有就算有。
fn has_component(stack: &ItemStack, kind: i32) -> bool {
    if stack.is_empty() {
        return false;
    }
    if stack.removed.contains(&kind) {
        return false;
    }
    stack.added.iter().any(|(id, _)| *id == kind)
        || defaults::of_item(stack.item).get(kind).is_some()
}

/// 官方 `PatchedDataComponentMap.set`：值和默认组件相同就从补丁里拿掉。
fn set_component(stack: &mut ItemStack, kind: i32, value: &ComponentValue) {
    let equals_default = defaults::of_item(stack.item).get(kind) == Some(value);
    stack.removed.retain(|id| *id != kind);
    if equals_default {
        stack.added.retain(|(id, _)| *id != kind);
        return;
    }
    let mut bytes = Vec::new();
    value.write(&mut bytes);
    if let Some(index) = stack.added.iter().position(|(id, _)| *id == kind) {
        stack.added[index].1 = bytes;
        let mut kept = false;
        stack.added.retain(|(id, _)| {
            if *id != kind {
                return true;
            }
            if kept {
                return false;
            }
            kept = true;
            true
        });
    } else {
        stack.added.push((kind, bytes));
    }
}

/// 官方 `PatchedDataComponentMap.remove`。默认组件里有的话，补丁记一笔移除。
fn remove_component(stack: &mut ItemStack, kind: i32) {
    stack.added.retain(|(id, _)| *id != kind);
    let in_prototype = defaults::of_item(stack.item).get(kind).is_some();
    stack.removed.retain(|id| *id != kind);
    if in_prototype {
        stack.removed.push(kind);
    }
}

/// 官方 `ItemStack.transmuteCopy`：换成另一种物品，数量和补丁留下，再按新物品的默认组件整理。
fn transmute_copy(stack: &ItemStack, new_item: i32) -> ItemStack {
    if stack.is_empty() {
        return ItemStack::empty();
    }
    let mut copy = ItemStack {
        item: new_item,
        count: stack.count,
        added: stack.added.clone(),
        removed: stack.removed.clone(),
    };
    sanitize_patch(&mut copy);
    copy
}

/// 官方 `PatchedDataComponentMap.fromPatch`：和默认值相同的覆盖、默认里没有的移除，都丢掉。
fn sanitize_patch(stack: &mut ItemStack) {
    let prototype = defaults::of_item(stack.item);
    stack
        .added
        .retain(|(kind, bytes)| match read_component(*kind, bytes) {
            Ok(value) => prototype.get(*kind) != Some(&value),
            // 解不开的字节留着，避免把玩家自己的组件丢掉。
            Err(_) => true,
        });
    stack.removed.retain(|kind| prototype.get(*kind).is_some());
}

fn read_component(kind: i32, bytes: &[u8]) -> Result<ComponentValue, DecodeError> {
    let mut cursor = Cursor::new(bytes);
    let value = ComponentValue::read(&mut cursor, kind)?;
    cursor.finish()?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::packets::play::clientbound;
    use mc_bamboo_protocol::{
        components::{ComponentPatch, defaults},
        packet::{ClientboundPacket, play::ContainerSetSlot},
    };

    use super::*;
    use crate::{
        links::encode_frames,
        world::tests::{Client, Frame, Harness, body_of, frames_with_id},
    };

    fn settled() -> (Harness, Client) {
        let mut server = Harness::new(8);
        let client = server.join_and_settle("Kitten");
        client.take_frames();
        (server, client)
    }

    fn give(server: &mut Harness, client: &Client, menu_slot: usize, stack: ItemStack) {
        server
            .region()
            .players
            .get_mut(&client.id)
            .unwrap()
            .inventory
            .slots[menu_slot] = stack;
    }

    fn slot_of<'a>(server: &'a Harness, client: &Client, menu_slot: usize) -> &'a ItemStack {
        &server.player(client).inventory.slots[menu_slot]
    }

    fn edit(server: &Harness, client: &Client, slot: i32, pages: &[&str], title: Option<&str>) {
        server.request(
            client,
            mc_bamboo_protocol::packet::play::PlayRequest::EditBook {
                slot,
                pages: pages.iter().map(|page| (*page).to_owned()).collect(),
                title: title.map(str::to_owned),
            },
        );
    }

    fn resolved(stack: &ItemStack, kind: i32) -> Option<ComponentValue> {
        let patch = ComponentPatch::from_opaque(stack).unwrap();
        defaults::resolve(stack.item, &patch, kind).cloned()
    }

    fn slot_frame(state_id: i32, slot: usize, stack: &ItemStack) -> Frame {
        let packet = ContainerSetSlot {
            container_id: 0,
            state_id,
            slot: slot as i16,
            stack,
        };
        Frame {
            id: ContainerSetSlot::ID,
            body: body_of(encode_frames(&packet)),
        }
    }

    fn slot_echoes(client: &Client) -> Vec<Frame> {
        frames_with_id(&client.take_frames(), clientbound::CONTAINER_SET_SLOT)
            .into_iter()
            .cloned()
            .collect()
    }

    fn book() -> ItemStack {
        ItemStack::new(items::WRITABLE_BOOK, 1)
    }

    #[test]
    fn slots_outside_the_hotbar_and_offhand_are_ignored() {
        let (mut server, client) = settled();
        let menu_slot = HOTBAR_START + 1;
        give(&mut server, &client, menu_slot, book());

        for slot in [-1, 9, 36, 39, 41, 45, i32::MAX] {
            edit(&server, &client, slot, &["no"], None);
        }
        server.tick();

        assert_eq!(slot_of(&server, &client, menu_slot), &book());
        assert!(slot_echoes(&client).is_empty());
    }

    #[test]
    fn only_a_stack_with_writable_content_can_be_edited() {
        let (mut server, client) = settled();
        let menu_slot = HOTBAR_START + 1;
        give(
            &mut server,
            &client,
            menu_slot,
            ItemStack::new(items::STONE, 1),
        );
        edit(&server, &client, 1, &["no"], None);
        server.tick();
        assert_eq!(
            slot_of(&server, &client, menu_slot),
            &ItemStack::new(items::STONE, 1)
        );

        let mut stripped = book();
        stripped.removed.push(kinds::WRITABLE_BOOK_CONTENT);
        give(&mut server, &client, menu_slot, stripped.clone());
        edit(&server, &client, 1, &["no"], Some("title"));
        server.tick();
        assert_eq!(slot_of(&server, &client, menu_slot), &stripped);
        assert!(slot_echoes(&client).is_empty());
    }

    #[test]
    fn editing_writes_the_pages_and_echoes_the_slot() {
        let (mut server, client) = settled();
        let menu_slot = HOTBAR_START + 1;
        give(&mut server, &client, menu_slot, book());

        edit(&server, &client, 1, &["Hello", "第二页"], None);
        server.tick();

        let stack = slot_of(&server, &client, menu_slot).clone();
        assert_eq!(stack.item, items::WRITABLE_BOOK);
        assert_eq!(stack.count, 1);
        assert_eq!(
            resolved(&stack, kinds::WRITABLE_BOOK_CONTENT),
            Some(ComponentValue::WritableBookContent(vec![
                plain("Hello".to_owned()),
                plain("第二页".to_owned()),
            ]))
        );
        assert_eq!(slot_echoes(&client), vec![slot_frame(1, menu_slot, &stack)]);

        // 内容没变就不再发，官方 `broadcastChanges` 也是比过才发。
        edit(&server, &client, 1, &["Hello", "第二页"], None);
        server.tick();
        assert!(slot_echoes(&client).is_empty());
    }

    #[test]
    fn clearing_the_pages_drops_the_component_back_to_the_default() {
        let (mut server, client) = settled();
        let menu_slot = HOTBAR_START;
        give(&mut server, &client, menu_slot, book());
        edit(&server, &client, 0, &["a"], None);
        server.tick();
        client.take_frames();

        edit(&server, &client, 0, &[], None);
        server.tick();

        let stack = slot_of(&server, &client, menu_slot).clone();
        assert!(
            stack
                .added
                .iter()
                .all(|(kind, _)| *kind != kinds::WRITABLE_BOOK_CONTENT)
        );
        assert_eq!(
            resolved(&stack, kinds::WRITABLE_BOOK_CONTENT),
            Some(ComponentValue::WritableBookContent(vec![]))
        );
        assert_eq!(slot_echoes(&client), vec![slot_frame(2, menu_slot, &stack)]);
    }

    #[test]
    fn signing_replaces_the_book_and_keeps_other_components() {
        let (mut server, client) = settled();
        let menu_slot = HOTBAR_START + 2;
        let mut stack = book();
        stack.count = 2;
        let name = ComponentValue::CustomName(Component::text("Nick"));
        let mut name_bytes = Vec::new();
        name.write(&mut name_bytes);
        stack.added.push((kinds::CUSTOM_NAME, name_bytes.clone()));
        // 成书的默认值就是发光，整理补丁时要丢掉。关掉发光则要留下。
        stack
            .added
            .push((kinds::ENCHANTMENT_GLINT_OVERRIDE, vec![1]));
        give(&mut server, &client, menu_slot, stack);

        edit(&server, &client, 2, &["page"], Some("My Title"));
        server.tick();

        let stack = slot_of(&server, &client, menu_slot).clone();
        assert_eq!(stack.item, items::WRITTEN_BOOK);
        assert_eq!(stack.count, 2);
        assert!(
            stack
                .added
                .iter()
                .all(|(kind, _)| *kind != kinds::ENCHANTMENT_GLINT_OVERRIDE)
        );
        assert_eq!(
            stack
                .added
                .iter()
                .find(|(kind, _)| *kind == kinds::CUSTOM_NAME)
                .map(|(_, bytes)| bytes.as_slice()),
            Some(name_bytes.as_slice())
        );
        assert!(resolved(&stack, kinds::WRITABLE_BOOK_CONTENT).is_none());
        assert_eq!(
            resolved(&stack, kinds::WRITTEN_BOOK_CONTENT),
            Some(ComponentValue::WrittenBookContent {
                title: plain("My Title".to_owned()),
                author: "Kitten".to_owned(),
                generation: 0,
                pages: vec![plain(Component::text("page"))],
                resolved: true,
            })
        );
        assert_eq!(
            defaults::of_item(items::WRITTEN_BOOK).get(kinds::ENCHANTMENT_GLINT_OVERRIDE),
            Some(&ComponentValue::EnchantmentGlintOverride(true))
        );
        assert_eq!(slot_echoes(&client), vec![slot_frame(1, menu_slot, &stack)]);
    }

    #[test]
    fn an_empty_title_still_signs_and_the_offhand_is_slot_40() {
        let (mut server, client) = settled();
        let mut stack = book();
        stack
            .added
            .push((kinds::ENCHANTMENT_GLINT_OVERRIDE, vec![0]));
        give(&mut server, &client, OFF_HAND, stack);

        edit(&server, &client, 40, &["only"], Some(""));
        server.tick();

        let stack = slot_of(&server, &client, OFF_HAND).clone();
        assert_eq!(stack.item, items::WRITTEN_BOOK);
        assert_eq!(
            stack
                .added
                .iter()
                .find(|(kind, _)| *kind == kinds::ENCHANTMENT_GLINT_OVERRIDE)
                .map(|(_, bytes)| bytes.as_slice()),
            Some([0].as_slice())
        );
        assert_eq!(
            resolved(&stack, kinds::WRITTEN_BOOK_CONTENT),
            Some(ComponentValue::WrittenBookContent {
                title: plain(String::new()),
                author: "Kitten".to_owned(),
                generation: 0,
                pages: vec![plain(Component::text("only"))],
                resolved: true,
            })
        );
        assert_eq!(slot_echoes(&client), vec![slot_frame(1, OFF_HAND, &stack)]);
    }
}
