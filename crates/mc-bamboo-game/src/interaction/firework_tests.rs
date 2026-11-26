//! 玩家滑翔时右键烟花的端到端测试：`FireworkRocketItem.use` 生成附着烟花、扣物品，
//! 不滑翔不生成，创造模式不扣物品。
//!
//! End-to-end tests for `FireworkRocketItem.use`: an attached rocket spawns and one item
//! is spent while gliding, nothing spawns when not gliding, and creative keeps the rocket.

use mc_bamboo_blocks::entity::{
    Entity, KindPublic,
    mob::{
        MobType,
        leash::{self, LeashDelayed},
    },
};
use mc_bamboo_core::{Hand, Vec3};
use mc_bamboo_data::{items, registries::stat_type, sounds};
use mc_bamboo_protocol::{
    codec::{Reader, write_varint},
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

/// 掉在地上的 `LEAD` 物品数量。
fn lead_drops(server: &mut Harness) -> usize {
    let region = server.region();
    region
        .entities
        .ids()
        .iter()
        .filter(|&&id| {
            region.entities.public_at(id).is_some_and(|public| {
                matches!(&public.kind, KindPublic::Item { stack, .. } if stack.item == items::LEAD)
            })
        })
        .count()
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

/// `FireworkRocketItem.use`：滑翔右键先 `dropAllLeashConnections(null)`，解开的
/// 拴绳要真的断链，并播 `LEAD_BREAK`。
#[test]
fn gliding_firework_drops_leashes_and_sounds_lead_break() {
    let (mut server, client) = settled();
    hold(&mut server, &client, GameMode::Survival);
    start_gliding(&mut server, &client);

    // 一头拴在玩家身上的牛：`leash_holder` 就是玩家实体 id。
    let cow_id = server.spawn_entity(Entity::by_mob_type(
        MobType::Cow,
        Vec3::new(SPAWN_X, SPAWN_Y, SPAWN_Z),
    ));
    let player_uuid = *server.player(&client).uuid.as_bytes();
    server
        .region()
        .entities
        .with_mut(cow_id, |entity: &mut Entity| {
            if let Some(mob) = entity.kind.mob_mut() {
                leash::set_leashed_to(mob, client.id.0, LeashDelayed::Entity(player_uuid));
            }
        });
    let holder_before = server
        .region()
        .entities
        .get(cow_id)
        .and_then(|e| e.kind.mob().and_then(leash::leash_holder));
    assert_eq!(holder_before, Some(client.id.0), "起步时牛拴在玩家身上");
    assert_eq!(lead_drops(&mut server), 0, "起步没有拴绳掉落物");
    client.take_frames();

    use_firework(&mut server, &client, 1);

    // `dropAllLeashConnections` 解开的那根绳子要按 `spawnAtLocation` 掉出来。
    assert_eq!(lead_drops(&mut server), 1, "解开的牛掉了一根拴绳");
    // 拴绳解开了。
    let holder_after = server
        .region()
        .entities
        .get(cow_id)
        .and_then(|e| e.kind.mob().and_then(leash::leash_holder));
    assert_eq!(holder_after, None, "dropAllLeashConnections 解开了拴绳");

    // `LEAD_BREAK` 广播给所有人（`playSound(null, ...)` 不排除发起者）。
    let lead_break = i32::from(sounds::sound_event("minecraft:item.lead.break"));
    let frames = client.take_frames();
    let heard: Vec<i32> = frames_with_id(&frames, clientbound::SOUND)
        .iter()
        .filter_map(|frame| Reader::new(&frame.body).varint().ok())
        .map(|id| id - 1)
        .collect();
    assert!(heard.contains(&lead_break), "播了 LEAD_BREAK 音");
}
