//! 物品栏的测试：驱动 `World::tick`，看客户端收到的帧。

use mc_bamboo_core::{BlockPos, Direction, Hand};
use mc_bamboo_data::{blocks, items, packets::play::clientbound};
use mc_bamboo_protocol::{
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

    assert!(client.take_frames().is_empty());
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
    assert!(client.take_frames().is_empty());
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
    assert!(client.take_frames().is_empty());
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
    assert!(client.take_frames().is_empty());
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
        added: vec![(5, vec![0x00])],
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
fn heights_outside_the_world_are_not_loaded() {
    let (mut server, client) = settled();
    server.request(
        &client,
        PlayRequest::MovePlayer {
            position: Some(mc_bamboo_core::Vec3::new(8.5, 318.0, 8.5)),
            rotation: None,
            on_ground: false,
        },
    );

    pick(&server, &client, BlockPos::new(8, 320, 8));
    server.tick();

    assert!(client.take_frames().is_empty());
}
