//! 炼药锅交还物品：创造模式和生存模式都按官方 `createFilledResult`。

use mc_bamboo_blocks::{BlockState, ItemExchange, state::LEVEL_CAULDRON};
use mc_bamboo_core::{BlockPos, Direction, Hand};
use mc_bamboo_data::{blocks, items, recipes, registries::data_component_type as kinds};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry, cursor::Holder, value::BannerLayer},
    item::ItemStack,
    packet::play::{GameMode, PlayRequest},
};

use super::{Inventory, menu_slot};
use crate::{
    player::Player,
    world::tests::{Client, Harness},
};

const GROUND: BlockPos = BlockPos::new(11, -61, 8);

fn water_id() -> i32 {
    recipes::POTIONS
        .iter()
        .position(|name| *name == "minecraft:water")
        .unwrap()
        .try_into()
        .unwrap()
}

fn stack(item: i32, count: i32, kind: i32, value: ComponentValue) -> ItemStack {
    let mut patch = ComponentPatch::new();
    patch.put(kind, PatchEntry::Set(value));
    let (added, removed) = patch.to_opaque();
    ItemStack {
        item,
        count,
        added,
        removed,
    }
}

fn water_potion(count: i32) -> ItemStack {
    stack(
        items::POTION,
        count,
        kinds::POTION_CONTENTS,
        ComponentValue::PotionContents {
            potion: Some(water_id()),
            custom_color: None,
            custom_effects: Vec::new(),
            custom_name: None,
        },
    )
}

fn glass(count: i32) -> ItemStack {
    ItemStack::new(items::GLASS_BOTTLE, count)
}

fn dyed(item: i32, count: i32) -> ItemStack {
    stack(
        item,
        count,
        kinds::DYED_COLOR,
        ComponentValue::DyedColor(0x336699),
    )
}

fn base_pattern() -> i32 {
    mc_bamboo_data::registries::SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == "minecraft:banner_pattern")
        .and_then(|registry| {
            registry
                .entries
                .iter()
                .position(|name| *name == "minecraft:base")
        })
        .unwrap()
        .try_into()
        .unwrap()
}

fn banner(count: i32, colors: &[i32]) -> ItemStack {
    stack(
        items::WHITE_BANNER,
        count,
        kinds::BANNER_PATTERNS,
        ComponentValue::BannerPatterns(
            colors
                .iter()
                .copied()
                .map(|color| BannerLayer {
                    pattern: Holder::Id(base_pattern()),
                    color,
                })
                .collect(),
        ),
    )
}

fn fill(replacement: ItemStack, limit_creative: bool) -> ItemExchange {
    ItemExchange::Fill {
        replacement,
        limit_creative,
    }
}

fn hand(inventory: &Inventory) -> &ItemStack {
    &inventory.slots[36]
}

fn apply(inventory: &mut Inventory, exchange: &ItemExchange, creative: bool) {
    inventory.apply_exchange(Hand::Main, exchange, creative);
}

#[test]
fn creative_adds_the_glass_bottle_and_keeps_the_potion() {
    let mut inventory = Inventory::default();
    inventory.slots[36] = water_potion(1);
    apply(&mut inventory, &fill(glass(1), true), true);
    assert!(hand(&inventory).matches(&water_potion(1)));
    assert!(inventory.slots[37].matches(&glass(1)));
}

#[test]
fn creative_does_not_add_a_glass_bottle_it_already_has() {
    let mut inventory = Inventory::default();
    inventory.slots[36] = water_potion(1);
    inventory.slots[10] = glass(1);
    apply(&mut inventory, &fill(glass(1), true), true);
    assert!(hand(&inventory).matches(&water_potion(1)));
    assert!(inventory.slots[37].is_empty());
    assert!(inventory.slots[10].matches(&glass(1)));
}

#[test]
fn creative_finds_the_existing_result_in_an_armour_slot() {
    let mut inventory = Inventory::default();
    inventory.slots[36] = water_potion(1);
    inventory.slots[5] = glass(1);
    apply(&mut inventory, &fill(glass(1), true), true);
    assert!(inventory.slots[37].is_empty());
    assert!(inventory.slots[5].matches(&glass(1)));
}

#[test]
fn survival_replaces_a_single_potion_with_the_glass_bottle() {
    let mut inventory = Inventory::default();
    inventory.slots[36] = water_potion(1);
    apply(&mut inventory, &fill(glass(1), true), false);
    assert!(hand(&inventory).matches(&glass(1)));
    assert!(inventory.slots[37].is_empty());
}

#[test]
fn survival_keeps_the_rest_of_a_stack_and_stores_the_bottle() {
    let mut inventory = Inventory::default();
    inventory.slots[36] = water_potion(2);
    apply(&mut inventory, &fill(glass(1), true), false);
    assert!(hand(&inventory).matches(&water_potion(1)));
    assert!(inventory.slots[37].matches(&glass(1)));
}

#[test]
fn survival_stacks_the_bottle_onto_the_off_hand_before_an_empty_slot() {
    let mut inventory = Inventory::default();
    inventory.slots[36] = water_potion(2);
    inventory.slots[45] = glass(10);
    apply(&mut inventory, &fill(glass(1), true), false);
    assert!(hand(&inventory).matches(&water_potion(1)));
    assert!(inventory.slots[45].matches(&glass(11)));
    assert!(inventory.slots[37].is_empty());
}

#[test]
fn a_full_inventory_drops_the_survival_remainder_and_deletes_the_creative_one() {
    let mut survival = Inventory::default();
    let stone = ItemStack::new(
        items::STONE,
        items::ITEMS[items::STONE as usize].max_stack_size,
    );
    for index in 0..36 {
        survival.slots[menu_slot(index)] = stone.clone();
    }
    survival.slots[45] = stone.clone();
    survival.slots[36] = water_potion(2);
    apply(&mut survival, &fill(glass(1), true), false);
    assert!(hand(&survival).matches(&water_potion(1)));
    assert!(
        survival
            .slots
            .iter()
            .all(|slot| slot.item != items::GLASS_BOTTLE)
    );

    let mut creative = survival;
    creative.slots[36] = water_potion(2);
    apply(&mut creative, &fill(glass(1), true), true);
    assert!(hand(&creative).matches(&water_potion(2)));
    assert!(
        creative
            .slots
            .iter()
            .all(|slot| slot.item != items::GLASS_BOTTLE)
    );
    for slot in 5..9 {
        assert!(creative.slots[slot].is_empty());
    }
}

#[test]
fn creative_still_gives_a_washed_banner_when_one_is_already_there() {
    let mut inventory = Inventory::default();
    inventory.slots[36] = banner(3, &[1, 4]);
    inventory.slots[37] = banner(1, &[1]);
    apply(&mut inventory, &fill(banner(1, &[1]), false), true);
    assert!(hand(&inventory).matches(&banner(3, &[1, 4])));
    assert!(inventory.slots[37].matches(&banner(2, &[1])));
}

#[test]
fn survival_washes_one_banner_and_leaves_the_rest_patterned() {
    let mut inventory = Inventory {
        selected: 3,
        ..Inventory::default()
    };
    inventory.slots[39] = banner(2, &[1, 4]);
    apply(&mut inventory, &fill(banner(1, &[1]), false), false);
    assert!(inventory.slots[39].matches(&banner(1, &[1, 4])));
    assert!(inventory.slots[36].matches(&banner(1, &[1])));
}

#[test]
fn the_off_hand_loses_one_banner_and_the_washed_one_goes_to_the_hotbar() {
    let mut inventory = Inventory::default();
    inventory.slots[45] = banner(3, &[2]);
    inventory.apply_exchange(Hand::Off, &fill(banner(1, &[]), false), false);
    assert!(inventory.slots[45].matches(&banner(2, &[2])));
    assert!(inventory.slots[36].matches(&banner(1, &[])));
}

#[test]
fn washing_dye_edits_the_whole_stack_in_either_mode() {
    for creative in [false, true] {
        let mut inventory = Inventory::default();
        inventory.slots[36] = dyed(items::LEATHER_HELMET, 1);
        inventory.apply_exchange(Hand::Main, &ItemExchange::RemoveDyedColor, creative);
        assert_eq!(hand(&inventory).item, items::LEATHER_HELMET);
        assert_eq!(hand(&inventory).count, 1);
        assert!(hand(&inventory).patch().entry(kinds::DYED_COLOR).is_none());
        assert!(inventory.slots[37].is_empty());
    }
}

#[test]
fn survival_replaces_one_shulker_and_creative_keeps_it() {
    let dyed_box = stack(
        items::WHITE_SHULKER_BOX,
        1,
        kinds::REPAIR_COST,
        ComponentValue::RepairCost(7),
    );
    let plain = stack(
        items::SHULKER_BOX,
        1,
        kinds::REPAIR_COST,
        ComponentValue::RepairCost(7),
    );
    let mut survival = Inventory::default();
    survival.slots[36] = dyed_box.clone();
    apply(&mut survival, &fill(plain.clone(), false), false);
    assert!(hand(&survival).matches(&plain));

    let mut creative = Inventory::default();
    creative.slots[36] = dyed_box.clone();
    creative.slots[37] = plain.clone();
    apply(&mut creative, &fill(plain.clone(), false), true);
    assert!(hand(&creative).matches(&dyed_box));
    assert!(creative.slots[37].matches(&plain));
    assert!(creative.slots[38].matches(&plain));
}

fn settled() -> (Harness, Client) {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    client.take_frames();
    (server, client)
}

fn put(server: &mut Harness, client: &Client, slot: usize, stack: ItemStack) {
    player_mut(server, client).inventory.slots[slot] = stack;
}

fn player_mut<'a>(server: &'a mut Harness, client: &Client) -> &'a mut Player {
    server.region().players.get_mut(&client.id).expect("player")
}

fn click(server: &Harness, client: &Client) {
    server.request(
        client,
        PlayRequest::UseItemOn {
            hand: Hand::Main,
            pos: GROUND,
            face: Direction::Up,
            cursor: (0.5, 1.0, 0.5),
            inside: false,
            world_border_hit: false,
            sequence: 1,
        },
    );
}

fn water_level(level: i32) -> u32 {
    BlockState(blocks::WATER_CAULDRON)
        .with(LEVEL_CAULDRON, level)
        .0
}

#[test]
fn a_creative_player_fills_an_empty_cauldron_and_is_given_a_bottle() {
    let (mut server, client) = settled();
    server.region().chunks.set_block(GROUND, blocks::CAULDRON);
    put(&mut server, &client, 36, water_potion(1));
    click(&server, &client);
    server.tick();
    assert_eq!(
        server.region().chunks.block(GROUND),
        Some(blocks::WATER_CAULDRON)
    );
    let slots = &server.player(&client).inventory.slots;
    assert!(slots[36].matches(&water_potion(1)));
    assert!(slots[37].matches(&glass(1)));
}

#[test]
fn a_survival_player_trades_the_potion_for_the_bottle() {
    let (mut server, client) = settled();
    player_mut(&mut server, &client).game_mode = GameMode::Survival;
    server.region().chunks.set_block(GROUND, blocks::CAULDRON);
    put(&mut server, &client, 36, water_potion(1));
    click(&server, &client);
    server.tick();
    assert_eq!(
        server.region().chunks.block(GROUND),
        Some(blocks::WATER_CAULDRON)
    );
    let slots = &server.player(&client).inventory.slots;
    assert!(slots[36].matches(&glass(1)));
    assert!(slots[37].is_empty());
}

#[test]
fn washing_a_helmet_removes_the_colour_from_the_held_stack() {
    let (mut server, client) = settled();
    server.region().chunks.set_block(GROUND, water_level(1));
    put(&mut server, &client, 36, dyed(items::LEATHER_HELMET, 1));
    click(&server, &client);
    server.tick();
    assert_eq!(server.region().chunks.block(GROUND), Some(blocks::CAULDRON));
    let held = &server.player(&client).inventory.slots[36];
    assert_eq!(held.item, items::LEATHER_HELMET);
    assert!(held.patch().entry(kinds::DYED_COLOR).is_none());
    assert!(server.player(&client).inventory.slots[37].is_empty());
}

#[test]
fn washing_a_banner_in_creative_keeps_the_stack_and_adds_the_cleaned_one() {
    let (mut server, client) = settled();
    server.region().chunks.set_block(GROUND, water_level(2));
    put(&mut server, &client, 36, banner(3, &[1, 4]));
    click(&server, &client);
    server.tick();
    assert_eq!(server.region().chunks.block(GROUND), Some(water_level(1)));
    let slots = &server.player(&client).inventory.slots;
    assert!(slots[36].matches(&banner(3, &[1, 4])));
    assert!(slots[37].matches(&banner(1, &[1])));
}
