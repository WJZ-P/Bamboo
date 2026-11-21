//! 玩家扔鸡蛋的端到端测试：发使用包，看世界里的蛋和物品栏。
//!
//! End-to-end egg throwing: send a use packet, then inspect the egg and the inventory.

use mc_bamboo_blocks::entity::KindPublic;
use mc_bamboo_core::Hand;
use mc_bamboo_data::items;
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{GameMode, PlayRequest},
};

use super::*;

fn throw(server: &mut Harness, client: &Client, sequence: i32) {
    server.request(
        client,
        PlayRequest::UseItem {
            hand: Hand::Main,
            sequence,
            yaw: 0.0,
            pitch: 0.0,
        },
    );
}

fn eggs(server: &mut Harness) -> Vec<(i32, ItemStack)> {
    let region = server.region();
    region
        .entities
        .ids()
        .into_iter()
        .filter_map(|id| match &region.entities.public_at(id)?.kind {
            KindPublic::Egg(egg) => Some((egg.owner.unwrap_or(0), egg.item.clone())),
            _ => None,
        })
        .collect()
}

fn hold(server: &mut Harness, client: &Client, stack: ItemStack, mode: GameMode) {
    let player = server.region().players.get_mut(&client.id).expect("player");
    player.inventory.slots[36] = stack;
    player.game_mode = mode;
}

#[test]
fn survival_throws_one_blue_egg_and_spends_it() {
    let (mut server, client) = settled();
    hold(
        &mut server,
        &client,
        ItemStack::new(items::BLUE_EGG, 3),
        GameMode::Survival,
    );
    throw(&mut server, &client, 1);
    server.tick();
    let thrown = eggs(&mut server);
    assert_eq!(thrown.len(), 1);
    assert_eq!(thrown[0].0, client.id.0, "主人是扔蛋的玩家");
    assert_eq!(thrown[0].1, ItemStack::new(items::BLUE_EGG, 1));
    let player = server.region().players.get(&client.id).unwrap();
    assert_eq!(player.inventory.slots[36].count, 2);
}

#[test]
fn creative_keeps_the_egg_and_other_items_are_not_thrown() {
    let (mut server, client) = settled();
    hold(
        &mut server,
        &client,
        ItemStack::new(items::EGG, 1),
        GameMode::Creative,
    );
    throw(&mut server, &client, 1);
    server.tick();
    assert_eq!(eggs(&mut server).len(), 1);
    let player = server.region().players.get(&client.id).unwrap();
    assert_eq!(player.inventory.slots[36].count, 1);

    hold(
        &mut server,
        &client,
        ItemStack::new(items::SNOWBALL, 1),
        GameMode::Survival,
    );
    throw(&mut server, &client, 2);
    server.tick();
    let player = server.region().players.get(&client.id).unwrap();
    assert_eq!(
        player.inventory.slots[36].count, 1,
        "雪球还没有实现，不会扔出"
    );
}
