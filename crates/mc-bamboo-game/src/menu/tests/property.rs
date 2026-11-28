//! 会让玩家白拿或者丢东西的点击：结果格、收纳袋、商人和断线。
//!
//! Clicks that could hand out free items or destroy them: result slots, bundles,
//! merchants and disconnects.

use mc_bamboo_protocol::components::bundle as bundle_component;

use super::*;

/// 工作台菜单里的槽位：0 是结果格，1 到 9 是材料格，快捷栏从 37 开始。
///
/// Crafting menu slots: 0 is the result, 1 to 9 the grid, the hotbar starts at 37.
const CRAFT_RESULT: i16 = 0;
const CRAFT_GRID: i16 = 1;
const CRAFT_HOTBAR: i16 = 37;

fn station_slot(fixture: &Fixture, index: usize) -> ItemStack {
    fixture
        .menu()
        .station
        .as_ref()
        .expect("工作台菜单应该开着")
        .slot_item(index)
}

fn bundled(stack: &ItemStack, item: i32) -> i32 {
    bundle_component::contents(stack)
        .unwrap_or_default()
        .iter()
        .filter(|entry| entry.item == item)
        .map(|entry| entry.count)
        .sum()
}

/// 光标上拿着收纳袋左键点合成结果格：官方 `BundleContents.Mutable.tryTransfer` 走
/// `Slot.safeTake`，结果进袋子的同时 `onTake` 扣掉材料。材料不能留在格子里。
///
/// A bundle on the cursor left-clicking the crafting result: vanilla
/// `BundleContents.Mutable.tryTransfer` goes through `Slot.safeTake`, so the result
/// enters the bundle and `onTake` consumes the ingredients. They must not stay behind.
#[test]
fn a_bundle_taking_a_crafting_result_consumes_the_ingredients() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CRAFTING_TABLE);
    fixture.open(pos);
    *fixture.inventory_slot(36) = ItemStack::new(items::OAK_LOG, 1);
    *fixture.inventory_slot(37) = ItemStack::new(items::BUNDLE, 1);

    fixture.click(CRAFT_HOTBAR, 0, ContainerInput::Pickup);
    fixture.click(CRAFT_GRID, 0, ContainerInput::Pickup);
    assert_eq!(
        station_slot(&fixture, 0),
        ItemStack::new(items::OAK_PLANKS, 4)
    );

    fixture.click(CRAFT_HOTBAR + 1, 0, ContainerInput::Pickup);
    fixture.click(CRAFT_RESULT, 0, ContainerInput::Pickup);

    let carried = fixture.menu().carried().clone();
    assert_eq!(bundled(&carried, items::OAK_PLANKS), 4, "木板进了袋子");
    assert!(
        station_slot(&fixture, 1).is_empty(),
        "原木要被这次合成用掉"
    );
    assert!(station_slot(&fixture, 0).is_empty());
}

/// 袋子装不下整份结果时什么也不拿：结果格 `mayPlace` 为假，`tryRemove` 要求
/// `maxAmount >= count`。
///
/// A bundle without room for the whole result takes nothing: the result slot's
/// `mayPlace` is false, so `tryRemove` requires `maxAmount >= count`.
#[test]
fn a_bundle_without_room_leaves_the_crafting_result_alone() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CRAFTING_TABLE);
    fixture.open(pos);
    *fixture.inventory_slot(36) = ItemStack::new(items::OAK_LOG, 1);
    let mut bag = ItemStack::new(items::BUNDLE, 1);
    let mut filler = ItemStack::new(items::STONE, 62);
    assert_eq!(bundle_component::try_insert(&mut bag, &mut filler), 62);
    *fixture.inventory_slot(37) = bag;

    fixture.click(CRAFT_HOTBAR, 0, ContainerInput::Pickup);
    fixture.click(CRAFT_GRID, 0, ContainerInput::Pickup);
    fixture.click(CRAFT_HOTBAR + 1, 0, ContainerInput::Pickup);
    fixture.click(CRAFT_RESULT, 0, ContainerInput::Pickup);

    let carried = fixture.menu().carried().clone();
    assert_eq!(bundled(&carried, items::OAK_PLANKS), 0);
    assert_eq!(
        station_slot(&fixture, 0),
        ItemStack::new(items::OAK_PLANKS, 4)
    );
    assert_eq!(station_slot(&fixture, 1), ItemStack::new(items::OAK_LOG, 1));
}
