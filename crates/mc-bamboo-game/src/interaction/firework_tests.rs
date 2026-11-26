//! 玩家滑翔时右键烟花的端到端测试：`FireworkRocketItem.use` 生成附着烟花、扣物品，
//! 不滑翔不生成，创造模式不扣物品。
//!
//! End-to-end tests for `FireworkRocketItem.use`: an attached rocket spawns and one item
//! is spent while gliding, nothing spawns when not gliding, and creative keeps the rocket.

use mc_bamboo_blocks::entity::KindPublic;
use mc_bamboo_core::Hand;
use mc_bamboo_data::{items, registries::stat_type};
use mc_bamboo_protocol::{
    codec::write_varint,
    item::ItemStack,
    packet::play::{ClientCommandKind, GameMode, PlayRequest},
};

use super::*;

fn hold(server: &mut Harness, client: &Client, mode: GameMode) {
    let player = server.region().players.get_mut(&client.id).expect("player");
    player.inventory.slots[36] = ItemStack::new(items::FIREWORK_ROCKET, 3);
    player.game_mode = mode;
}

/// `sync.set_fall_flying(true)`：`process_inbox`（处理 `use_item`）在 `player_glide::tick`
/// 之前跑，所以这一刻的 `use` 看得到滑翔标志。
fn start_gliding(server: &mut Harness, client: &Client) {
    server
        .region()
        .players
        .get_mut(&client.id)
        .expect("player")
        .sync
        .set_fall_flying(true);
}

fn use_firework(server: &mut Harness, client: &Client, sequence: i32) {
    server.request(
        client,
        PlayRequest::UseItem {
            hand: Hand::Main,
            sequence,
            yaw: 0.0,
            pitch: 0.0,
        },
    );
    server.tick();
}

/// 收集世界里的烟花：`(owner, attached_to_target)`。
fn rockets(server: &mut Harness) -> Vec<(Option<i32>, Option<i32>)> {
    let region = server.region();
    region
        .entities
        .ids()
        .into_iter()
        .filter_map(|id| match &region.entities.public_at(id)?.kind {
            KindPublic::FireworkRocket(firework) => {
                Some((firework.owner, firework.attached_to_target))
            }
            _ => None,
        })
        .collect()
}

/// 让客户端请求统计，返回所有 `AWARD_STATS` 包的正文。
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

#[test]
fn gliding_spawns_an_attached_firework_and_spends_one() {
    let (mut server, client) = settled();
    hold(&mut server, &client, GameMode::Survival);
    start_gliding(&mut server, &client);
    use_firework(&mut server, &client, 1);

    let rockets = rockets(&mut server);
    assert_eq!(rockets.len(), 1, "滑翔右键生成一枚附着烟花");
    assert_eq!(
        rockets[0],
        (Some(client.id.0), Some(client.id.0)),
        "主人和附着目标都是玩家"
    );
    assert_eq!(
        server.player(&client).inventory.slots[36].count,
        2,
        "生存模式扣一个"
    );
    // `FireworkRocketItem.use` 记一次 `Stats.ITEM_USED`。
    let mut body = Vec::new();
    for value in [1, stat_type::USED, items::FIREWORK_ROCKET, 1] {
        write_varint(&mut body, value);
    }
    assert_eq!(request_stats(&mut server, &client), [body], "记了使用统计");
}

#[test]
fn use_while_not_gliding_spawns_and_spends_nothing() {
    let (mut server, client) = settled();
    hold(&mut server, &client, GameMode::Survival);
    use_firework(&mut server, &client, 1);

    assert!(rockets(&mut server).is_empty(), "不滑翔不生成烟花");
    assert_eq!(
        server.player(&client).inventory.slots[36].count,
        3,
        "不滑翔不扣物品"
    );
}

#[test]
fn creative_keeps_the_rocket_but_still_spawns() {
    let (mut server, client) = settled();
    hold(&mut server, &client, GameMode::Creative);
    start_gliding(&mut server, &client);
    use_firework(&mut server, &client, 1);

    assert_eq!(rockets(&mut server).len(), 1, "创造模式照样生成");
    assert_eq!(
        server.player(&client).inventory.slots[36].count,
        3,
        "创造模式不扣物品"
    );
}
