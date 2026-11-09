//! 物品栏的测试：驱动 `World::tick`，看客户端收到的帧。

use mc_bamboo_blocks::Effect;
use mc_bamboo_core::{BlockPos, Direction, Hand};
use mc_bamboo_data::{blocks, items, packets::play::clientbound, registries::block_entity_type};
use mc_bamboo_protocol::{
    components::{ComponentPatch, PatchEntry},
    item::ItemStack,
    packet::play::{GameMode, PlayRequest, PlayerActionKind},
};

use super::*;
use crate::world::tests::{Client, Frame, Harness};

/// 够得着的地面，上面一格没有玩家。
const GROUND: BlockPos = BlockPos::new(11, -61, 8);
const ABOVE: BlockPos = BlockPos::new(11, -60, 8);

fn settled() -> (Harness, Client) {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    client.take_frames();
    (server, client)
}

fn player_mut<'a>(server: &'a mut Harness, client: &Client) -> &'a mut Player {
    server
        .region()
        .players
        .get_mut(&client.id)
        .expect("玩家应该在世界里")
}

fn inventory<'a>(server: &'a Harness, client: &Client) -> &'a Inventory {
    &server.player(client).inventory
}

/// 丢出物品会生成地上的实体，但不会回一份物品栏修正。
///
/// Dropping spawns a ground item. It does not send an inventory correction.
fn assert_no_inventory_correction(client: &Client) {
    let frames = client.take_frames();
    assert!(frames.iter().all(|frame| {
        frame.id != clientbound::CONTAINER_SET_SLOT
            && frame.id != clientbound::CONTAINER_SET_CONTENT
            && frame.id != clientbound::SET_PLAYER_INVENTORY
    }));
}

fn set_slot(server: &Harness, client: &Client, slot: i16, stack: ItemStack) {
    server.request(client, PlayRequest::SetCreativeModeSlot { slot, stack });
}

/// 不走包，直接把物品栏摆成想要的样子。
fn fill(server: &mut Harness, client: &Client, slots: &[(usize, ItemStack)]) {
    let inventory = &mut player_mut(server, client).inventory;
    for (slot, stack) in slots {
        inventory.slots[*slot] = stack.clone();
    }
}

fn select(server: &Harness, client: &Client, slot: i16) {
    server.request(client, PlayRequest::SetCarriedItem { slot });
}

fn action(server: &Harness, client: &Client, action: PlayerActionKind) {
    server.request(
        client,
        PlayRequest::PlayerAction {
            action,
            pos: BlockPos::new(0, 0, 0),
            face: Direction::Down,
            sequence: 0,
        },
    );
}

fn pick(server: &Harness, client: &Client, pos: BlockPos) {
    server.request(
        client,
        PlayRequest::PickItemFromBlock {
            pos,
            include_data: false,
        },
    );
}

/// Ctrl+中键：官方的 `includeData`，物品还带上方块实体的数据。
fn pick_with_data(server: &Harness, client: &Client, pos: BlockPos) {
    server.request(
        client,
        PlayRequest::PickItemFromBlock {
            pos,
            include_data: true,
        },
    );
}

fn place_on_ground(server: &Harness, client: &Client, hand: Hand) {
    server.request(
        client,
        PlayRequest::UseItemOn {
            hand,
            pos: GROUND,
            face: Direction::Up,
            cursor: (0.5, 1.0, 0.5),
            inside: false,
            world_border_hit: false,
            sequence: 1,
        },
    );
}

fn set_block(server: &mut Harness, pos: BlockPos, state: u32) {
    server.region().chunks.set_block(pos, state);
}

fn block_at(server: &mut Harness, pos: BlockPos) -> Option<u32> {
    server.region().chunks.block(pos)
}

fn set_held_slot(slot: u8) -> Frame {
    Frame {
        id: clientbound::SET_HELD_SLOT,
        body: vec![slot],
    }
}

fn set_slot_frame(body: &[u8]) -> Frame {
    Frame {
        id: clientbound::CONTAINER_SET_SLOT,
        body: body.to_vec(),
    }
}

/// 一个带 `max_stack_size` 组件的物品。
fn with_max_stack_size(item: i32, count: i32) -> ItemStack {
    ItemStack {
        item,
        count,
        added: vec![(data_component_type::MAX_STACK_SIZE, vec![99])],
        removed: Vec::new(),
    }
}

/// 一个附魔过的物品。值的开头是附魔的个数。
fn enchanted(item: i32) -> ItemStack {
    ItemStack {
        item,
        count: 1,
        added: vec![(data_component_type::ENCHANTMENTS, vec![1, 5, 1])],
        removed: Vec::new(),
    }
}

#[test]
fn hands_follow_the_selected_slot() {
    let (mut server, client) = settled();
    fill(
        &mut server,
        &client,
        &[
            (36, ItemStack::new(items::STONE, 1)),
            (40, ItemStack::new(items::DIRT, 1)),
            (45, ItemStack::new(items::SAND, 1)),
        ],
    );

    assert_eq!(
        inventory(&server, &client).held(Hand::Main).item,
        items::STONE
    );
    select(&server, &client, 4);
    server.tick();

    let inventory = inventory(&server, &client);
    assert_eq!(inventory.selected, 4);
    assert_eq!(inventory.held(Hand::Main).item, items::DIRT);
    assert_eq!(inventory.held(Hand::Off).item, items::SAND);
    assert_eq!(
        inventory.equipment(EquipmentSlot::MainHand),
        Some(&ItemStack::new(items::DIRT, 1))
    );
    assert!(client.take_frames().is_empty());
}

#[test]
fn slots_outside_the_hotbar_are_ignored() {
    let (mut server, client) = settled();
    select(&server, &client, 3);

    for slot in [-1, 9, 10, i16::MAX, i16::MIN] {
        select(&server, &client, slot);
    }
    server.tick();

    assert_eq!(inventory(&server, &client).selected, 3);
    assert!(client.take_frames().is_empty());
}

#[test]
fn official_carried_item_bytes_select_slot_four() {
    let (mut server, client) = settled();

    let request = PlayRequest::decode(&[0x36, 0x00, 0x04]).unwrap();
    server.request(&client, request);
    server.tick();

    assert_eq!(inventory(&server, &client).selected, 4);
}

#[test]
fn creative_slots_are_stored_without_an_echo() {
    let (mut server, client) = settled();
    let stack = ItemStack::new(items::STONE, 64);

    for slot in 1..=45 {
        set_slot(&server, &client, slot, stack.clone());
    }
    server.tick();

    assert!(client.take_frames().is_empty());
    let inventory = inventory(&server, &client);
    assert!(inventory.slots[0].is_empty());
    assert!(inventory.slots[1..].iter().all(|slot| *slot == stack));
}

#[test]
fn official_creative_slot_bytes_fill_the_hotbar() {
    let (mut server, client) = settled();

    // `blocks-inventory.md` 3.1 的实测字节：槽 36 放 64 个 stone，再清空。
    let fill = PlayRequest::decode(&[0x39, 0x00, 0x24, 0x40, 0x01, 0x00, 0x00]).unwrap();
    server.request(&client, fill);
    server.tick();
    assert_eq!(
        inventory(&server, &client).slots[36],
        ItemStack::new(items::STONE, 64)
    );

    let clear = PlayRequest::decode(&[0x39, 0x00, 0x24, 0x00]).unwrap();
    server.request(&client, clear);
    server.tick();
    assert!(inventory(&server, &client).slots[36].is_empty());
}

#[test]
fn the_crafting_result_and_slots_past_the_menu_are_ignored() {
    let (mut server, client) = settled();

    for slot in [0, 46, 47, i16::MAX] {
        set_slot(&server, &client, slot, ItemStack::new(items::STONE, 1));
    }
    server.tick();

    assert!(client.take_frames().is_empty());
    let inventory = inventory(&server, &client);
    assert!(inventory.slots.iter().all(ItemStack::is_empty));
    assert_eq!(inventory.slots.len(), MENU_SLOTS);
}

#[test]
fn an_empty_stack_clears_the_slot() {
    let (mut server, client) = settled();
    set_slot(&server, &client, 36, ItemStack::new(items::STONE, 1));
    set_slot(&server, &client, 36, ItemStack::empty());
    server.tick();

    assert!(inventory(&server, &client).slots[36].is_empty());
}

#[test]
fn counts_over_the_item_limit_are_ignored() {
    let (mut server, client) = settled();
    let over_limit = [
        ItemStack::new(items::STONE, 65),
        ItemStack::new(items::ENDER_PEARL, 17),
        ItemStack::new(items::DIAMOND_SWORD, 2),
    ];

    for stack in over_limit {
        set_slot(&server, &client, 36, stack);
    }
    server.tick();

    assert!(inventory(&server, &client).slots[36].is_empty());
    assert!(client.take_frames().is_empty());
}

#[test]
fn counts_at_the_item_limit_are_accepted() {
    let (mut server, client) = settled();
    let at_limit = [
        ItemStack::new(items::STONE, 64),
        ItemStack::new(items::ENDER_PEARL, 16),
        ItemStack::new(items::DIAMOND_SWORD, 1),
    ];

    for (slot, stack) in (9..).zip(at_limit.clone()) {
        set_slot(&server, &client, slot, stack);
    }
    server.tick();

    assert_eq!(inventory(&server, &client).slots[9..12], at_limit);
}

#[test]
fn a_max_stack_size_component_lifts_the_default_limit() {
    let (mut server, client) = settled();
    let stack = with_max_stack_size(items::DIAMOND_SWORD, 99);

    set_slot(&server, &client, 36, stack.clone());
    server.tick();

    assert_eq!(inventory(&server, &client).slots[36], stack);
}

#[test]
fn throwing_an_item_out_of_the_screen_touches_no_slot() {
    let (mut server, client) = settled();
    fill(
        &mut server,
        &client,
        &[(36, ItemStack::new(items::DIRT, 1))],
    );

    for slot in [-1, i16::MIN] {
        set_slot(&server, &client, slot, ItemStack::new(items::STONE, 1));
        set_slot(&server, &client, slot, ItemStack::new(items::STONE, 65));
        set_slot(&server, &client, slot, ItemStack::empty());
    }
    server.tick();

    assert_no_inventory_correction(&client);
    let inventory = inventory(&server, &client);
    assert_eq!(inventory.slots[36].item, items::DIRT);
    assert!(inventory.slots[37..].iter().all(ItemStack::is_empty));
}

#[test]
fn only_creative_players_can_write_their_slots() {
    let (mut server, client) = settled();
    player_mut(&mut server, &client).game_mode = GameMode::Survival;

    set_slot(&server, &client, 36, ItemStack::new(items::STONE, 1));
    server.tick();

    assert!(inventory(&server, &client).slots[36].is_empty());
}

#[test]
fn q_takes_one_item_without_a_reply() {
    let (mut server, client) = settled();
    let stack = ItemStack {
        added: vec![(1, vec![0x10])],
        ..ItemStack::new(items::STONE, 2)
    };
    fill(&mut server, &client, &[(36, stack.clone())]);

    action(&server, &client, PlayerActionKind::DropItem);
    server.tick();

    // 组件留在剩下的物品上。
    let expected = ItemStack { count: 1, ..stack };
    assert_eq!(inventory(&server, &client).slots[36], expected);
    // 物品栏不回包。地上的物品实体会另外发给客户端。
    // The inventory is not corrected. The ground item entity is sent separately.
}

#[test]
fn q_on_the_last_item_empties_the_slot() {
    let (mut server, client) = settled();
    fill(
        &mut server,
        &client,
        &[(36, ItemStack::new(items::STONE, 1))],
    );

    action(&server, &client, PlayerActionKind::DropItem);
    action(&server, &client, PlayerActionKind::DropItem);
    server.tick();

    assert_eq!(inventory(&server, &client).slots[36], ItemStack::empty());
    assert_no_inventory_correction(&client);
}

#[test]
fn ctrl_q_empties_only_the_selected_slot() {
    let (mut server, client) = settled();
    fill(
        &mut server,
        &client,
        &[
            (36, ItemStack::new(items::DIRT, 3)),
            (38, ItemStack::new(items::STONE, 64)),
        ],
    );
    select(&server, &client, 2);

    action(&server, &client, PlayerActionKind::DropAllItems);
    server.tick();

    let inventory = inventory(&server, &client);
    assert_eq!(inventory.slots[38], ItemStack::empty());
    assert_eq!(inventory.slots[36].count, 3);
    assert_no_inventory_correction(&client);
}

#[test]
fn swapping_hands_sends_both_slots_with_growing_state_ids() {
    let (mut server, client) = settled();
    fill(
        &mut server,
        &client,
        &[(36, ItemStack::new(items::STONE, 1))],
    );

    action(&server, &client, PlayerActionKind::SwapItemWithOffhand);
    server.tick();

    let inventory = inventory(&server, &client);
    assert!(inventory.slots[36].is_empty());
    assert_eq!(inventory.slots[45], ItemStack::new(items::STONE, 1));
    assert_eq!(
        client.take_frames(),
        [
            set_slot_frame(&[0x00, 0x01, 0x00, 0x24, 0x00]),
            set_slot_frame(&[0x00, 0x02, 0x00, 0x2d, 0x01, 0x01, 0x00, 0x00]),
        ]
    );
}

#[test]
fn swapping_back_continues_the_state_ids() {
    let (mut server, client) = settled();
    fill(
        &mut server,
        &client,
        &[(36, ItemStack::new(items::STONE, 1))],
    );

    action(&server, &client, PlayerActionKind::SwapItemWithOffhand);
    action(&server, &client, PlayerActionKind::SwapItemWithOffhand);
    server.tick();

    let frames = client.take_frames();
    assert_eq!(frames.len(), 4);
    assert_eq!(
        frames[2],
        set_slot_frame(&[0x00, 0x03, 0x00, 0x24, 0x01, 0x01, 0x00, 0x00])
    );
    assert_eq!(frames[3], set_slot_frame(&[0x00, 0x04, 0x00, 0x2d, 0x00]));
}

#[test]
fn state_id_wraps_at_fifteen_bits() {
    let mut inventory = Inventory {
        state_id: 32766,
        ..Inventory::default()
    };

    assert_eq!(inventory.next_state_id(), 32767);
    assert_eq!(inventory.next_state_id(), 0);
}

#[test]
fn swapping_identical_hands_sends_nothing() {
    let (mut server, client) = settled();
    let stack = ItemStack::new(items::STONE, 5);
    fill(
        &mut server,
        &client,
        &[(36, stack.clone()), (45, stack.clone())],
    );

    action(&server, &client, PlayerActionKind::SwapItemWithOffhand);
    action(&server, &client, PlayerActionKind::SwapItemWithOffhand);
    server.tick();

    assert!(client.take_frames().is_empty());
}

#[test]
fn swapping_empty_hands_sends_nothing() {
    let (mut server, client) = settled();

    action(&server, &client, PlayerActionKind::SwapItemWithOffhand);
    server.tick();

    assert!(client.take_frames().is_empty());
}

#[test]
fn swapping_uses_the_selected_slot_and_keeps_components_for_the_owner() {
    let (mut server, client) = settled();
    let potion = ItemStack {
        item: 1,
        count: 2,
        added: vec![(1, vec![0x10])],
        removed: vec![12],
    };
    // `blocks-inventory.md` 2.5 的实测字节：delimited 的 `02 01 01 01 01 01 10 0c`。
    set_slot(&server, &client, 45, potion.clone());
    select(&server, &client, 2);

    action(&server, &client, PlayerActionKind::SwapItemWithOffhand);
    server.tick();

    assert_eq!(inventory(&server, &client).slots[38], potion);
    // 发回给本人时是 trusted 格式，只少了长度。
    assert_eq!(
        client.take_frames(),
        [
            set_slot_frame(&[
                0x00, 0x01, 0x00, 0x26, 0x02, 0x01, 0x01, 0x01, 0x01, 0x10, 0x0c
            ]),
            set_slot_frame(&[0x00, 0x02, 0x00, 0x2d, 0x00]),
        ]
    );
}

/// 我们只校验最外层的那几种组件也要原样还给物品主人：它的客户端刚把这些字节发过来。
#[test]
fn components_we_only_shape_check_come_back_to_the_owner() {
    let (mut server, client) = settled();
    let mut lock = Vec::new();
    mc_bamboo_nbt::Tag::Compound(Vec::new()).write_network(&mut lock);
    let locked = ItemStack {
        item: items::CHEST,
        count: 1,
        added: vec![(data_component_type::LOCK, lock)],
        removed: Vec::new(),
    };

    set_slot(&server, &client, 45, locked.clone());
    select(&server, &client, 2);
    action(&server, &client, PlayerActionKind::SwapItemWithOffhand);
    server.tick();

    // 换手之后组件还在，服务端发回去的那一份也带着它。
    assert_eq!(inventory(&server, &client).slots[38], locked);
    let frames = client.take_frames();
    let sent = frames
        .iter()
        .find(|frame| frame.id == clientbound::CONTAINER_SET_SLOT && frame.body.len() > 5)
        .expect("换手要把主手那一格发回来");
    // 槽位 38 之后是 trusted 格式的物品：数量、物品、一项增加的组件、零项移除。
    assert_eq!(
        sent.body,
        [
            0x00, 0x01, 0x00, 0x26, 0x01, 0x94, 0x03, 0x01, 0x00, 0x51, 0x0a, 0x00
        ]
    );
}

#[test]
fn the_block_placed_is_the_one_in_the_selected_slot() {
    let (mut server, client) = settled();
    set_slot(&server, &client, 36, ItemStack::new(items::DIRT, 1));
    set_slot(&server, &client, 41, ItemStack::new(items::STONE, 1));
    select(&server, &client, 5);

    place_on_ground(&server, &client, Hand::Main);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::STONE));
}

#[test]
fn after_a_swap_the_other_item_is_placed() {
    let (mut server, client) = settled();
    set_slot(&server, &client, 36, ItemStack::new(items::DIRT, 1));
    set_slot(&server, &client, 45, ItemStack::new(items::STONE, 1));

    action(&server, &client, PlayerActionKind::SwapItemWithOffhand);
    place_on_ground(&server, &client, Hand::Main);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::STONE));
}

#[test]
fn the_off_hand_places_its_own_item() {
    let (mut server, client) = settled();
    set_slot(&server, &client, 36, ItemStack::new(items::DIRT, 1));
    set_slot(&server, &client, 45, ItemStack::new(items::STONE, 1));

    place_on_ground(&server, &client, Hand::Off);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::STONE));
}

#[test]
fn after_q_nothing_is_left_to_place() {
    let (mut server, client) = settled();
    set_slot(&server, &client, 36, ItemStack::new(items::STONE, 1));

    action(&server, &client, PlayerActionKind::DropItem);
    place_on_ground(&server, &client, Hand::Main);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
}

#[test]
fn picking_a_new_block_fills_the_selected_empty_slot() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::STONE);

    pick(&server, &client, ABOVE);
    server.tick();

    let inventory = inventory(&server, &client);
    assert_eq!(inventory.slots[36], ItemStack::new(items::STONE, 1));
    assert_eq!(inventory.selected, 0);
    assert_eq!(
        client.take_frames(),
        [
            set_held_slot(0),
            set_slot_frame(&[0x00, 0x01, 0x00, 0x24, 0x01, 0x01, 0x00, 0x00]),
        ]
    );
}

#[test]
fn official_pick_bytes_pick_the_block_at_the_spawn() {
    let (mut server, client) = settled();

    // `blocks-inventory.md` 3.1 的实测字节：(8,-61,8)。
    let request =
        PlayRequest::decode(&[0x24, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x8f, 0xc3, 0x00]).unwrap();
    server.request(&client, request);
    server.tick();

    assert_eq!(
        inventory(&server, &client).slots[36],
        ItemStack::new(items::GRASS_BLOCK, 1)
    );
}

#[test]
fn picking_something_on_the_hotbar_only_selects_it() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::STONE);
    fill(
        &mut server,
        &client,
        &[(40, ItemStack::new(items::STONE, 12))],
    );

    pick(&server, &client, ABOVE);
    server.tick();

    let inventory = inventory(&server, &client);
    assert_eq!(inventory.selected, 4);
    assert_eq!(inventory.slots[40].count, 12);
    assert_eq!(client.take_frames(), [set_held_slot(4)]);
}

#[test]
fn picking_from_the_backpack_swaps_into_the_hotbar() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::STONE);
    let dirt = ItemStack::new(items::DIRT, 1);
    fill(
        &mut server,
        &client,
        &[
            (36, dirt.clone()),
            (37, dirt.clone()),
            (38, dirt),
            (20, ItemStack::new(items::STONE, 7)),
        ],
    );

    pick(&server, &client, ABOVE);
    server.tick();

    let inventory = inventory(&server, &client);
    assert_eq!(inventory.selected, 3);
    assert_eq!(inventory.slots[39], ItemStack::new(items::STONE, 7));
    assert!(inventory.slots[20].is_empty());
    assert_eq!(
        client.take_frames(),
        [
            set_held_slot(3),
            set_slot_frame(&[0x00, 0x01, 0x00, 0x14, 0x00]),
            set_slot_frame(&[0x00, 0x02, 0x00, 0x27, 0x07, 0x01, 0x00, 0x00]),
        ]
    );
}

#[test]
fn picking_searches_empty_hotbar_slots_from_the_selected_one() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::STONE);
    let dirt = ItemStack::new(items::DIRT, 1);
    let mut slots: Vec<(usize, ItemStack)> = (36..=44).map(|slot| (slot, dirt.clone())).collect();
    slots.retain(|(slot, _)| *slot != 37);
    fill(&mut server, &client, &slots);
    select(&server, &client, 6);

    pick(&server, &client, ABOVE);
    server.tick();

    // 从第 6 格往后找，绕回来才找到第 1 格。
    let inventory = inventory(&server, &client);
    assert_eq!(inventory.selected, 1);
    assert_eq!(inventory.slots[37], ItemStack::new(items::STONE, 1));
}

#[test]
fn picking_with_a_full_hotbar_moves_the_old_item_to_the_backpack() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::STONE);
    let dirt = ItemStack::new(items::DIRT, 3);
    let slots: Vec<(usize, ItemStack)> = (36..=44).map(|slot| (slot, dirt.clone())).collect();
    fill(&mut server, &client, &slots);

    pick(&server, &client, ABOVE);
    server.tick();

    let inventory = inventory(&server, &client);
    assert_eq!(inventory.selected, 0);
    assert_eq!(inventory.slots[36], ItemStack::new(items::STONE, 1));
    assert_eq!(inventory.slots[9], dirt);
    assert_eq!(
        client.take_frames(),
        [
            set_held_slot(0),
            set_slot_frame(&[0x00, 0x01, 0x00, 0x09, 0x03, items::DIRT as u8, 0x00, 0x00]),
            set_slot_frame(&[0x00, 0x02, 0x00, 0x24, 0x01, 0x01, 0x00, 0x00]),
        ]
    );
}

#[test]
fn picking_with_a_full_inventory_overwrites_the_selected_item() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::STONE);
    let dirt = ItemStack::new(items::DIRT, 1);
    let slots: Vec<(usize, ItemStack)> = (9..=44).map(|slot| (slot, dirt.clone())).collect();
    fill(&mut server, &client, &slots);

    pick(&server, &client, ABOVE);
    server.tick();

    assert_eq!(
        inventory(&server, &client).slots[36],
        ItemStack::new(items::STONE, 1)
    );
    assert_eq!(
        client.take_frames(),
        [
            set_held_slot(0),
            set_slot_frame(&[0x00, 0x01, 0x00, 0x24, 0x01, 0x01, 0x00, 0x00]),
        ]
    );
}

#[test]
fn picking_passes_over_enchanted_hotbar_slots() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::STONE);
    let sword = enchanted(items::DIAMOND_SWORD);
    let mut slots: Vec<(usize, ItemStack)> = (36..=44)
        .map(|slot| (slot, ItemStack::new(items::DIRT, 1)))
        .collect();
    slots[0].1 = sword.clone();
    slots[1].1 = sword.clone();
    fill(&mut server, &client, &slots);

    pick(&server, &client, ABOVE);
    server.tick();

    let inventory = inventory(&server, &client);
    assert_eq!(inventory.selected, 2);
    assert_eq!(inventory.slots[36], sword);
    assert_eq!(inventory.slots[38], ItemStack::new(items::STONE, 1));
}

#[test]
fn picking_with_only_enchanted_items_replaces_the_selected_one() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::STONE);
    let sword = enchanted(items::DIAMOND_SWORD);
    let slots: Vec<(usize, ItemStack)> = (36..=44).map(|slot| (slot, sword.clone())).collect();
    fill(&mut server, &client, &slots);
    select(&server, &client, 7);

    pick(&server, &client, ABOVE);
    server.tick();

    let inventory = inventory(&server, &client);
    assert_eq!(inventory.selected, 7);
    assert_eq!(inventory.slots[43], ItemStack::new(items::STONE, 1));
    assert_eq!(inventory.slots[9], sword);
}

#[test]
fn an_empty_enchantment_list_is_not_enchanted() {
    let stack = ItemStack {
        added: vec![(data_component_type::ENCHANTMENTS, vec![0])],
        ..ItemStack::new(items::DIAMOND_SWORD, 1)
    };

    assert!(!is_enchanted(&stack));
    assert!(!is_enchanted(&ItemStack::new(items::DIAMOND_SWORD, 1)));
    assert!(is_enchanted(&enchanted(items::DIAMOND_SWORD)));
}

#[test]
fn a_stack_with_components_is_not_the_same_item() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::STONE);
    let named = ItemStack {
        added: vec![(data_component_type::MAX_STACK_SIZE, vec![9])],
        ..ItemStack::new(items::STONE, 1)
    };
    fill(&mut server, &client, &[(40, named.clone())]);

    pick(&server, &client, ABOVE);
    server.tick();

    let inventory = inventory(&server, &client);
    assert_eq!(inventory.selected, 0);
    assert_eq!(inventory.slots[36], ItemStack::new(items::STONE, 1));
    assert_eq!(inventory.slots[40], named);
}

/// 旗帜的 `getCloneItemStack` 走方块实体，中键不按 Ctrl 也带上图案。
#[test]
fn picking_a_banner_brings_its_patterns_along() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::WHITE_BANNER);
    let layers =
        ComponentValue::BannerPatterns(vec![mc_bamboo_protocol::components::value::BannerLayer {
            pattern: mc_bamboo_protocol::components::cursor::Holder::Id(12),
            color: 14,
        }]);
    let mut patch = ComponentPatch::new();
    patch.put(
        data_component_type::BANNER_PATTERNS,
        PatchEntry::Set(layers.clone()),
    );
    server.region().block_items.insert(ABOVE, patch);

    pick(&server, &client, ABOVE);
    server.tick();

    let picked = inventory(&server, &client).slots[36].clone();
    assert_eq!(picked.item, items::WHITE_BANNER);
    assert_eq!(
        picked.patch().get(data_component_type::BANNER_PATTERNS),
        Some(&layers)
    );
}

/// Ctrl+中键：箱子带上里面的东西和自定义名，官方 `addBlockDataToItem`。
#[test]
fn picking_a_chest_with_data_brings_its_contents_along() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::CHEST);
    let stone = ItemStack::new(items::STONE, 5);
    crate::container::entry_at(
        server.region(),
        ABOVE,
        crate::container::ContainerKind::Chest,
    )
    .slots[1] = stone.clone();

    pick_with_data(&server, &client, ABOVE);
    server.tick();

    let picked = inventory(&server, &client).slots[36].clone();
    assert_eq!(picked.item, items::CHEST);
    let contents = picked.patch().get(data_component_type::CONTAINER).cloned();
    assert_eq!(
        contents,
        Some(ComponentValue::Container(vec![
            None,
            Some(mc_bamboo_protocol::components::stack::ItemStackTemplate {
                item: items::STONE,
                count: 5,
                components: ComponentPatch::new(),
            }),
        ]))
    );

    // 不按 Ctrl 的中键还是一个干净的箱子。组件不一样，所以放进下一个空格。
    pick(&server, &client, ABOVE);
    server.tick();
    let selected = usize::from(inventory(&server, &client).selected);
    assert_eq!(selected, 1);
    let plain = inventory(&server, &client).slots[36 + selected].clone();
    assert_eq!(plain, ItemStack::new(items::CHEST, 1));
}

/// Ctrl+中键选取告示牌：文字进 `block_entity_data` 组件，官方不把它算进隐式组件。
#[test]
fn picking_a_sign_with_data_keeps_the_text() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::OAK_SIGN);
    let mut content = mc_bamboo_core::sign::SignContent::BLANK;
    content.front.messages[0] = "hello".to_owned();
    assert!(server.region().chunks.set_sign(ABOVE, content));

    pick_with_data(&server, &client, ABOVE);
    server.tick();

    let picked = inventory(&server, &client).slots[36].clone();
    assert_eq!(picked.item, items::OAK_SIGN);
    let data = picked
        .patch()
        .get(data_component_type::BLOCK_ENTITY_DATA)
        .cloned();
    let Some(ComponentValue::BlockEntityData(data)) = data else {
        panic!("中键带数据的告示牌要有 block_entity_data：{picked:?}");
    };
    assert_eq!(data.kind, block_entity_type::SIGN);
    assert!(
        format!("{:?}", data.tag).contains("hello"),
        "{:?}",
        data.tag
    );
}

#[test]
fn wall_variants_pick_the_item_of_the_main_block() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::WALL_TORCH);

    pick(&server, &client, ABOVE);
    server.tick();

    assert_eq!(inventory(&server, &client).slots[36].item, items::TORCH);
}

#[test]
fn the_next_block_placed_is_the_picked_one() {
    let (mut server, client) = settled();
    let target = BlockPos::new(11, -59, 8);
    set_block(&mut server, ABOVE, blocks::STONE);

    pick(&server, &client, ABOVE);
    server.request(
        &client,
        PlayRequest::UseItemOn {
            hand: Hand::Main,
            pos: ABOVE,
            face: Direction::Up,
            cursor: (0.5, 1.0, 0.5),
            inside: false,
            world_border_hit: false,
            sequence: 1,
        },
    );
    server.tick();

    assert_eq!(block_at(&mut server, target), Some(blocks::STONE));
}

#[test]
fn nothing_happens_when_there_is_nothing_to_pick() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::WATER);
    let far = BlockPos::new(30, -61, 8);
    let above_world = BlockPos::new(8, 320, 8);
    let empty = BlockPos::new(11, -59, 8);

    for pos in [ABOVE, far, above_world, empty] {
        pick(&server, &client, pos);
    }
    server.tick();

    assert!(client.take_frames().is_empty());
    assert!(
        inventory(&server, &client)
            .slots
            .iter()
            .all(ItemStack::is_empty)
    );
}

#[test]
fn a_filled_bucket_goes_into_the_first_free_slot_only_once() {
    let mut inventory = Inventory::default();
    inventory.slots[36] = ItemStack::new(items::BUCKET, 1);
    assert_eq!(inventory.add_filled(items::WATER_BUCKET), Some(37));
    assert_eq!(inventory.slots[37], ItemStack::new(items::WATER_BUCKET, 1));
    assert_eq!(inventory.add_filled(items::WATER_BUCKET), None);

    // 副手和盔甲槽也算物品栏。
    let mut other = Inventory::default();
    other.slots[45] = ItemStack::new(items::LAVA_BUCKET, 1);
    assert_eq!(other.add_filled(items::LAVA_BUCKET), None);
}

#[test]
fn a_full_inventory_drops_the_filled_bucket_like_the_game() {
    let mut inventory = Inventory::default();
    for slot in 9..=44 {
        inventory.slots[slot] = ItemStack::new(items::STONE, 1);
    }
    assert_eq!(inventory.add_filled(items::WATER_BUCKET), None);
}

#[test]
fn clone_overrides_swap_in_the_vanilla_item() {
    let cases = [
        (blocks::WHEAT, items::WHEAT_SEEDS),
        (blocks::CARROTS, items::CARROT),
        (blocks::POTATOES, items::POTATO),
        (blocks::BEETROOTS, items::BEETROOT_SEEDS),
        (blocks::TORCHFLOWER_CROP, items::TORCHFLOWER_SEEDS),
        (blocks::CAVE_VINES, items::GLOW_BERRIES),
        (blocks::CAVE_VINES_PLANT, items::GLOW_BERRIES),
        (blocks::KELP_PLANT, items::KELP),
        (blocks::WEEPING_VINES_PLANT, items::WEEPING_VINES),
        (blocks::TWISTING_VINES_PLANT, items::TWISTING_VINES),
        (blocks::SWEET_BERRY_BUSH, items::SWEET_BERRIES),
        (blocks::NETHER_WART, items::NETHER_WART),
        (blocks::BAMBOO_SAPLING, items::BAMBOO),
        (blocks::BIG_DRIPLEAF_STEM, items::BIG_DRIPLEAF),
        (blocks::TALL_SEAGRASS, items::SEAGRASS),
        (blocks::FLOWER_POT, items::FLOWER_POT),
        (blocks::POTTED_DANDELION, items::DANDELION),
        (blocks::POTTED_OAK_SAPLING, items::OAK_SAPLING),
        (blocks::ATTACHED_PUMPKIN_STEM, items::PUMPKIN_SEEDS),
        (blocks::ATTACHED_MELON_STEM, items::MELON_SEEDS),
        (blocks::CANDLE_CAKE, items::CAKE),
        (blocks::BLACK_CANDLE_CAKE, items::CAKE),
    ];
    for (state, item) in cases {
        let picked = pick_clone(state, false);
        assert_eq!(picked, ItemStack::new(item, 1), "state {state}");
    }
}

#[test]
fn piston_heads_clone_as_the_piston_they_belong_to() {
    let sticky = mc_bamboo_blocks::state::BlockState(blocks::PISTON_HEAD)
        .with(
            mc_bamboo_blocks::state::PISTON_TYPE,
            mc_bamboo_blocks::state::PistonType::Sticky,
        )
        .0;
    let normal = mc_bamboo_blocks::state::BlockState(blocks::PISTON_HEAD)
        .with(
            mc_bamboo_blocks::state::PISTON_TYPE,
            mc_bamboo_blocks::state::PistonType::Normal,
        )
        .0;
    assert_eq!(
        pick_clone(sticky, false),
        ItemStack::new(items::STICKY_PISTON, 1)
    );
    assert_eq!(pick_clone(normal, false), ItemStack::new(items::PISTON, 1));
}

#[test]
fn blocks_whose_clone_stack_is_empty_are_not_picked() {
    for state in [
        blocks::MOVING_PISTON,
        blocks::NETHER_PORTAL,
        blocks::END_PORTAL,
        blocks::END_GATEWAY,
        blocks::FROSTED_ICE,
    ] {
        let (mut server, client) = settled();
        set_block(&mut server, ABOVE, state);
        pick(&server, &client, ABOVE);
        server.tick();
        assert!(
            client.take_frames().is_empty(),
            "state {state} should not be picked"
        );
        assert!(inventory(&server, &client).slots[36].is_empty());
    }
}

#[test]
fn light_and_test_blocks_carry_their_state_without_ctrl() {
    let light = mc_bamboo_blocks::state::BlockState(blocks::LIGHT)
        .with(mc_bamboo_blocks::state::LEVEL, 3)
        .with(mc_bamboo_blocks::state::WATERLOGGED, true)
        .0;
    let picked = pick_clone(light, false);
    assert_eq!(picked.item, items::LIGHT);
    assert_eq!(
        block_state_component(&picked),
        Some(vec![("level".to_owned(), "3".to_owned())])
    );

    let test_block = mc_bamboo_blocks::state::BlockState(blocks::TEST_BLOCK)
        .with(
            mc_bamboo_blocks::state::TEST_BLOCK_MODE,
            mc_bamboo_blocks::state::TestBlockMode::Fail,
        )
        .0;
    let picked = pick_clone(test_block, false);
    assert_eq!(picked.item, items::TEST_BLOCK);
    assert_eq!(
        block_state_component(&picked),
        Some(vec![("mode".to_owned(), "fail".to_owned())])
    );
}

#[test]
fn beehives_carry_honey_level_only_when_data_is_included() {
    let nest = mc_bamboo_blocks::state::BlockState(blocks::BEE_NEST)
        .with(mc_bamboo_blocks::state::LEVEL_HONEY, 4)
        .0;
    let plain = pick_clone(nest, false);
    assert_eq!(plain, ItemStack::new(items::BEE_NEST, 1));

    let with_data = pick_clone(nest, true);
    assert_eq!(with_data.item, items::BEE_NEST);
    assert_eq!(
        block_state_component(&with_data),
        Some(vec![("honey_level".to_owned(), "4".to_owned())])
    );

    // 蜂蜜是 0 也写进组件，官方 `with(HONEY_LEVEL, 0)` 并不省略。
    let empty = pick_clone(blocks::BEEHIVE, true);
    assert_eq!(
        block_state_component(&empty),
        Some(vec![("honey_level".to_owned(), "0".to_owned())])
    );
}

#[test]
fn copper_statues_carry_pose_and_any_stored_components() {
    let (mut server, client) = settled();
    let state = mc_bamboo_blocks::state::BlockState(blocks::WAXED_OXIDIZED_COPPER_GOLEM_STATUE)
        .with(
            mc_bamboo_blocks::state::COPPER_GOLEM_POSE,
            mc_bamboo_blocks::state::CopperGolemPose::Sitting,
        )
        .0;
    set_block(&mut server, ABOVE, state);
    let name = ComponentValue::CustomName(mc_bamboo_protocol::text::Component::text("Ada"));
    let mut patch = ComponentPatch::new();
    patch.put(
        data_component_type::CUSTOM_NAME,
        PatchEntry::Set(name.clone()),
    );
    server.region().block_items.insert(ABOVE, patch);

    pick(&server, &client, ABOVE);
    server.tick();

    let picked = inventory(&server, &client).slots[36].clone();
    assert_eq!(picked.item, items::WAXED_OXIDIZED_COPPER_GOLEM_STATUE);
    assert_eq!(
        picked.patch().get(data_component_type::CUSTOM_NAME),
        Some(&name)
    );
    assert_eq!(
        block_state_component(&picked),
        Some(vec![("copper_golem_pose".to_owned(), "sitting".to_owned())])
    );
}

fn pick_clone(state: u32, include_data: bool) -> ItemStack {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, state);
    if include_data {
        pick_with_data(&server, &client, ABOVE);
    } else {
        pick(&server, &client, ABOVE);
    }
    server.tick();
    inventory(&server, &client).slots[HOTBAR_START].clone()
}

fn block_state_component(stack: &ItemStack) -> Option<Vec<(String, String)>> {
    match stack.patch().get(data_component_type::BLOCK_STATE) {
        Some(ComponentValue::BlockState(properties)) => Some(properties.clone()),
        _ => None,
    }
}

#[test]
fn heights_outside_the_world_are_not_loaded() {
    let (mut server, client) = settled();
    // 一次飞到世界顶过不了速度检查。测试要的是人已经在那儿，好让挑选落在世界外。
    //
    // One packet to the top of the world fails the speed check. The test wants the player
    // already there, so the pick lands outside the world.
    crate::player_move::place_player(
        server.region(),
        client.id,
        mc_bamboo_core::Vec3::new(8.5, 318.0, 8.5),
    );

    pick(&server, &client, BlockPos::new(8, 320, 8));
    server.tick();

    assert!(client.take_frames().is_empty());
}

/// 官方 `ItemUtils.createFilledResult(empty, player, filled, true)`：
/// 生存模式 `empty.consume(1, player)`，手空了直接换上满桶，没空就 `Inventory.add`。
/// 这里走桶舀水/倒水用的 `ItemExchange::Fill`，`limit_creative` 为真。
#[test]
fn survival_scooping_with_one_empty_bucket_replaces_the_hand() {
    let mut inventory = Inventory::default();
    inventory.slots[36] = ItemStack::new(items::BUCKET, 1);
    let fill = ItemExchange::Fill {
        replacement: ItemStack::new(items::WATER_BUCKET, 1),
        limit_creative: true,
    };
    let (changed, leftover) = inventory.apply_exchange(Hand::Main, &fill, false);
    assert_eq!(changed, [36]);
    assert!(leftover.is_empty());
    assert_eq!(inventory.slots[36], ItemStack::new(items::WATER_BUCKET, 1));
}

#[test]
fn survival_scooping_with_sixteen_buckets_keeps_fifteen_and_adds_the_filled_one() {
    let mut inventory = Inventory::default();
    inventory.slots[36] = ItemStack::new(items::BUCKET, 16);
    let fill = ItemExchange::Fill {
        replacement: ItemStack::new(items::WATER_BUCKET, 1),
        limit_creative: true,
    };
    let (changed, leftover) = inventory.apply_exchange(Hand::Main, &fill, false);
    assert_eq!(changed, [36, 37]);
    assert!(leftover.is_empty());
    assert_eq!(inventory.slots[36], ItemStack::new(items::BUCKET, 15));
    assert_eq!(inventory.slots[37], ItemStack::new(items::WATER_BUCKET, 1));
}

/// 创造模式 `hasInfiniteMaterials` 且 `preventDuplicates`：手里那堆不变，
/// 背包里没有同样的满桶才 `Inventory.add` 一份，重复调用不再给。
#[test]
fn creative_scooping_keeps_the_hand_and_adds_the_filled_bucket_only_once() {
    let mut inventory = Inventory::default();
    inventory.slots[36] = ItemStack::new(items::BUCKET, 3);
    let fill = ItemExchange::Fill {
        replacement: ItemStack::new(items::WATER_BUCKET, 1),
        limit_creative: true,
    };
    let (changed, leftover) = inventory.apply_exchange(Hand::Main, &fill, true);
    assert_eq!(changed, [37]);
    assert!(leftover.is_empty());
    assert_eq!(inventory.slots[36], ItemStack::new(items::BUCKET, 3));
    assert_eq!(inventory.slots[37], ItemStack::new(items::WATER_BUCKET, 1));

    let (changed, leftover) = inventory.apply_exchange(Hand::Main, &fill, true);
    assert!(changed.is_empty());
    assert!(leftover.is_empty());
    assert_eq!(inventory.slots[37], ItemStack::new(items::WATER_BUCKET, 1));
}

/// `BucketItem.use` 倒水成功后 `getEmptySuccessItem` 在生存模式给空桶；
/// 手里那一堆消耗一个，空了换成空桶，没空就 `Inventory.add`。
#[test]
fn survival_pouring_hands_back_an_empty_bucket() {
    let mut inventory = Inventory::default();
    inventory.slots[36] = ItemStack::new(items::WATER_BUCKET, 1);
    let fill = ItemExchange::Fill {
        replacement: ItemStack::new(items::BUCKET, 1),
        limit_creative: true,
    };
    let (changed, leftover) = inventory.apply_exchange(Hand::Main, &fill, false);
    assert_eq!(changed, [36]);
    assert!(leftover.is_empty());
    assert_eq!(inventory.slots[36], ItemStack::new(items::BUCKET, 1));
}

/// 背包满时 `Inventory.add` 返回 false，官方 `player.drop(filled, false, PREDICTED)`
/// 生成真的物品实体。这里走游戏层的 `give_filled_result` 验证 `leftover` 被丢出。
#[test]
fn a_full_inventory_drops_the_filled_bucket_as_an_item_entity() {
    let (mut server, client) = settled();
    player_mut(&mut server, &client).game_mode = GameMode::Survival;
    for slot in 9..=45 {
        player_mut(&mut server, &client).inventory.slots[slot] = ItemStack::new(items::STONE, 1);
    }
    player_mut(&mut server, &client).inventory.slots[36] = ItemStack::new(items::BUCKET, 16);
    let id = client.id;
    give_filled_result(
        server.region(),
        id,
        Hand::Main,
        ItemStack::new(items::WATER_BUCKET, 1),
        true,
    );
    assert_eq!(
        inventory(&server, &client).slots[36],
        ItemStack::new(items::BUCKET, 15)
    );
    // 只剩一个能够接住满桶的空格也没有：所有 9..=45 都被占了。
    let dropped: Vec<_> = server
        .region()
        .entities
        .iter()
        .filter_map(|entity| entity.ground().map(|item| item.stack.clone()))
        .collect();
    assert_eq!(dropped, [ItemStack::new(items::WATER_BUCKET, 1)]);
}

/// `BucketItem.use` 舀水记下 `Effect::FilledBucket { item, hand }`；
/// 游戏层用 `ItemUtils.createFilledResult(empty, player, filled, true)` 交还。
#[test]
fn scooping_water_through_the_effect_hands_back_a_water_bucket() {
    let (mut server, client) = settled();
    player_mut(&mut server, &client).game_mode = GameMode::Survival;
    player_mut(&mut server, &client).inventory.slots[36] = ItemStack::new(items::BUCKET, 1);
    let id = client.id;
    crate::interaction::send_effects(
        server.region(),
        Some(id),
        &[Effect::FilledBucket {
            item: items::WATER_BUCKET,
            hand: Hand::Main,
        }],
    );
    assert_eq!(
        inventory(&server, &client).slots[36],
        ItemStack::new(items::WATER_BUCKET, 1)
    );
}

/// 倒水记下 `Effect::EmptiedBucket { hand }`，`getEmptySuccessItem` 在生存模式给空桶。
#[test]
fn pouring_water_through_the_effect_hands_back_an_empty_bucket() {
    let (mut server, client) = settled();
    player_mut(&mut server, &client).game_mode = GameMode::Survival;
    player_mut(&mut server, &client).inventory.slots[36] = ItemStack::new(items::WATER_BUCKET, 1);
    let id = client.id;
    crate::interaction::send_effects(
        server.region(),
        Some(id),
        &[Effect::EmptiedBucket { hand: Hand::Main }],
    );
    assert_eq!(
        inventory(&server, &client).slots[36],
        ItemStack::new(items::BUCKET, 1)
    );
}

/// 创造模式倒水时 `getEmptySuccessItem` 交还手里原来的满桶：
/// `contains` 为真，`createFilledResult` 不再往背包里加一份。
#[test]
fn creative_pouring_keeps_the_filled_bucket_in_hand() {
    let (mut server, client) = settled();
    player_mut(&mut server, &client).inventory.slots[36] = ItemStack::new(items::WATER_BUCKET, 1);
    let id = client.id;
    crate::interaction::send_effects(
        server.region(),
        Some(id),
        &[Effect::EmptiedBucket { hand: Hand::Main }],
    );
    assert_eq!(
        inventory(&server, &client).slots[36],
        ItemStack::new(items::WATER_BUCKET, 1)
    );
    assert!(
        inventory(&server, &client).slots[37..]
            .iter()
            .all(ItemStack::is_empty)
    );
}
