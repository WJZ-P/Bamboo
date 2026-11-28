//! 生存模式的中键选取。官方 `tryPickItem`：物品栏里有就选中或换到快捷栏，没有的话只有
//! `hasInfiniteMaterials()`（创造模式）才 `addAndPickItem` 变一个出来。
//!
//! Pick block in survival. Vanilla `tryPickItem` selects or swaps in a stack the
//! inventory already holds; only `hasInfiniteMaterials()` (creative) conjures one with
//! `addAndPickItem`.

use super::*;

fn survival() -> (Harness, Client) {
    let (mut server, client) = settled();
    player_mut(&mut server, &client).game_mode = GameMode::Survival;
    (server, client)
}

fn owned(server: &Harness, client: &Client, item: i32) -> i32 {
    inventory(server, client)
        .slots
        .iter()
        .filter(|stack| stack.item == item)
        .map(|stack| stack.count)
        .sum()
}

#[test]
fn a_survival_player_is_not_given_a_block_they_do_not_have() {
    let (mut server, client) = survival();
    set_block(&mut server, ABOVE, blocks::DIAMOND_BLOCK);
    fill(
        &mut server,
        &client,
        &[(36, ItemStack::new(items::DIRT, 5))],
    );

    pick(&server, &client, ABOVE);
    server.tick();

    assert_eq!(owned(&server, &client, items::DIAMOND_BLOCK), 0);
    assert_eq!(
        inventory(&server, &client).slots[36],
        ItemStack::new(items::DIRT, 5),
        "手上的东西原样留着"
    );
}

#[test]
fn a_survival_player_still_picks_a_block_from_the_backpack() {
    let (mut server, client) = survival();
    set_block(&mut server, ABOVE, blocks::STONE);
    fill(
        &mut server,
        &client,
        &[(9, ItemStack::new(items::STONE, 3))],
    );

    pick(&server, &client, ABOVE);
    server.tick();

    assert_eq!(
        inventory(&server, &client).slots[36],
        ItemStack::new(items::STONE, 3)
    );
    assert_eq!(owned(&server, &client, items::STONE), 3);
}
