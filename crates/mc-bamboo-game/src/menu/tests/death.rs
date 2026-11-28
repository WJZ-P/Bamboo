//! 玩家死亡时 2×2 合成格和光标上的东西。
//!
//! 官方 `Inventory.dropAll` 只掉物品栏、盔甲和副手。合成格和光标属于 `InventoryMenu`，
//! 重生时旧玩家被移除，`InventoryMenu.removed` 把它们丢在死亡地点；结果格只是预览，
//! `resultSlots.clearContent()` 直接清掉。
//!
//! The 2×2 crafting grid and the cursor when a player dies.
//!
//! Vanilla `Inventory.dropAll` drops only the inventory, armour and off hand. The grid
//! and the cursor belong to `InventoryMenu`: the old player is removed on respawn and
//! `InventoryMenu.removed` drops them at the death position. The result slot is only a
//! preview and `resultSlots.clearContent()` clears it.

use super::{property::ground_count, *};
use crate::player_hurt::hurt_player;
use mc_bamboo_blocks::entity::mob::{DamageKind, DamageSource};

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

fn survival(fixture: &mut Fixture) {
    fixture
        .region
        .players
        .get_mut(&fixture.id)
        .expect("玩家应该在区里")
        .game_mode = GameMode::Survival;
}

fn kill(fixture: &mut Fixture) {
    let source = DamageSource::environment(DamageKind::GenericKill);
    hurt_player(&mut fixture.region, fixture.id, source, f32::MAX, &mut None);
    assert!(fixture.player().dead);
}

/// 合成格里放着一根原木时死掉：掉的是原木，结果格预览的四块木板不能跟着掉出来。
///
/// Dying with a log in the grid drops the log. The four planks previewed in the result
/// slot must not drop with it.
#[test]
fn death_does_not_drop_the_crafting_result_preview() {
    let mut fixture = Fixture::new();
    survival(&mut fixture);
    *fixture.inventory_slot(36) = ItemStack::new(items::OAK_LOG, 1);
    click_inventory(&mut fixture, 36, 0, ContainerInput::Pickup);
    click_inventory(&mut fixture, 1, 0, ContainerInput::Pickup);
    assert_eq!(
        fixture.player().inventory.slots[0],
        ItemStack::new(items::OAK_PLANKS, 4)
    );

    kill(&mut fixture);

    assert_eq!(ground_count(&fixture, items::OAK_PLANKS), 0);
    assert_eq!(ground_count(&fixture, items::OAK_LOG), 1);
    assert!(fixture.player().inventory.slots[0].is_empty());
}

/// 光标上拿着东西时死掉：那堆东西掉在地上，不会在重生时被清掉。
///
/// Dying with a stack on the cursor drops it instead of losing it on respawn.
#[test]
fn death_drops_the_stack_on_the_cursor() {
    let mut fixture = Fixture::new();
    survival(&mut fixture);
    *fixture.inventory_slot(36) = ItemStack::new(items::DIAMOND, 3);
    click_inventory(&mut fixture, 36, 0, ContainerInput::Pickup);

    kill(&mut fixture);

    assert_eq!(ground_count(&fixture, items::DIAMOND), 3);
    assert!(fixture.player().inventory_cursor.carried().is_empty());
}
