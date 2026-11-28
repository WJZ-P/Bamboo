//! 玩家断开时手上和菜单里的东西。官方 `Player.remove` 调 `inventoryMenu.removed` 和
//! `doCloseContainer`，这时 `hasDisconnected()` 为真，`dropOrPlaceInInventory` 走
//! `player.drop(stack, false)`：光标上的、付款格里的都掉在脚下，不会消失。
//!
//! What a leaving player holds on the cursor or inside a menu. Vanilla `Player.remove`
//! calls `inventoryMenu.removed` and `doCloseContainer` with `hasDisconnected()` set,
//! so `dropOrPlaceInInventory` uses `player.drop(stack, false)`: the stacks land at the
//! player's feet instead of vanishing.

use super::{property::ground_count, *};

/// 物品栏菜单（容器 ID 0）里的一次点击。
///
/// One click in the inventory menu (container id 0).
fn click_inventory(fixture: &mut Fixture, slot: i16, button: i8, input: ContainerInput) {
    let packet = ClickPacket {
        container_id: 0,
        state_id: fixture.player().inventory.state_id(),
        slot,
        button,
        input,
        changed_slots: BTreeMap::new(),
        carried: HashedStack::Empty,
    };
    on_click(&mut fixture.region, fixture.id, packet);
}

/// 留在玩家物品栏里的数量。
///
/// How many of `item` remain in the player's inventory.
fn kept(fixture: &Fixture, item: i32) -> i32 {
    fixture
        .player()
        .inventory
        .slots
        .iter()
        .filter(|stack| stack.item == item)
        .map(|stack| stack.count)
        .sum()
}

#[test]
fn leaving_with_a_chest_open_drops_the_carried_stack() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    *fixture.inventory_slot(36) = ItemStack::new(items::DIAMOND, 5);
    fixture.click(Fixture::hotbar(0), 0, ContainerInput::Pickup);

    on_leave(&mut fixture.region, fixture.id);

    assert_eq!(
        ground_count(&fixture, items::DIAMOND) + kept(&fixture, items::DIAMOND),
        5
    );
    assert_eq!(fixture.entry(pos).open_count, 0);
}

#[test]
fn leaving_with_the_inventory_open_drops_the_carried_stack() {
    let mut fixture = Fixture::new();
    *fixture.inventory_slot(36) = ItemStack::new(items::DIAMOND, 5);
    click_inventory(&mut fixture, 36, 0, ContainerInput::Pickup);
    assert!(fixture.inventory_slot(36).is_empty());

    on_leave(&mut fixture.region, fixture.id);

    assert_eq!(
        ground_count(&fixture, items::DIAMOND) + kept(&fixture, items::DIAMOND),
        5
    );
}

#[test]
fn leaving_with_a_crafting_table_open_keeps_the_grid_and_the_carried_stack() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CRAFTING_TABLE);
    fixture.open(pos);
    *fixture.inventory_slot(36) = ItemStack::new(items::OAK_LOG, 3);
    *fixture.inventory_slot(37) = ItemStack::new(items::DIAMOND, 5);
    fixture.click(37, 0, ContainerInput::Pickup);
    fixture.click(1, 0, ContainerInput::Pickup);
    fixture.click(38, 0, ContainerInput::Pickup);

    on_leave(&mut fixture.region, fixture.id);

    assert_eq!(
        ground_count(&fixture, items::OAK_LOG) + kept(&fixture, items::OAK_LOG),
        3
    );
    assert_eq!(
        ground_count(&fixture, items::DIAMOND) + kept(&fixture, items::DIAMOND),
        5
    );
    assert_eq!(
        ground_count(&fixture, items::OAK_PLANKS) + kept(&fixture, items::OAK_PLANKS),
        0,
        "结果预览不是玩家的东西"
    );
}
