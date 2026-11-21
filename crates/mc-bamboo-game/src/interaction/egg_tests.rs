//! 玩家扔鸡蛋的端到端测试：发使用包，看世界里的蛋和物品栏。
//!
//! End-to-end egg throwing: send a use packet, then inspect the egg and the inventory.

use mc_bamboo_blocks::entity::KindPublic;
use mc_bamboo_core::Hand;
use mc_bamboo_data::{items, packets::play::clientbound, registries::stat_type};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{ClientCommandKind, GameMode, PlayRequest},
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

#[test]
fn egg_item_stats_are_cumulative_by_variant_and_sent_once_per_change() {
    let (mut server, client) = settled();
    for (step, (item, mode, expected)) in [
        (items::EGG, GameMode::Creative, Some(1)),
        (items::BLUE_EGG, GameMode::Survival, Some(1)),
        (items::BROWN_EGG, GameMode::Survival, Some(1)),
        (items::EGG, GameMode::Survival, Some(2)),
        (items::SNOWBALL, GameMode::Survival, None),
    ]
    .into_iter()
    .enumerate()
    {
        hold(&mut server, &client, ItemStack::new(item, 1), mode);
        throw(&mut server, &client, step as i32 + 1);
        server.tick();
        let mut body = Vec::new();
        if let Some(count) = expected {
            for value in [1, stat_type::USED, item, count] {
                mc_bamboo_protocol::codec::write_varint(&mut body, value);
            }
        } else {
            body.push(0);
        }
        assert_eq!(request_stats(&mut server, &client), [body]);
        assert_eq!(request_stats(&mut server, &client), [vec![0]]);
    }
}

fn request_stats(server: &mut Harness, client: &Client) -> Vec<Vec<u8>> {
    server.request(
        client,
        PlayRequest::ClientCommand {
            action: ClientCommandKind::RequestStats,
        },
    );
    server.tick();
    client
        .take_frames()
        .into_iter()
        .filter(|frame| frame.id == clientbound::AWARD_STATS)
        .map(|frame| frame.body)
        .collect()
}
