//! 玩家弓和弩的端到端测试：发使用包、推进刻、松手，看世界里的箭和物品栏。
//!
//! End-to-end bow and crossbow tests: use, tick, release, then inspect the
//! arrows and the inventory.

use mc_bamboo_blocks::entity::arrow::Pickup;
use mc_bamboo_core::{BlockPos, Direction, Hand};
use mc_bamboo_data::{items, packets::play::clientbound, registries::data_component_type as kinds};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry, defaults},
    item::ItemStack,
    packet::play::{ClientCommandKind, GameMode, PlayRequest, PlayerActionKind},
};

use super::*;

const GROUND: BlockPos = BlockPos::new(11, -61, 8);

fn use_item(server: &mut Harness, client: &Client, sequence: i32) {
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

fn release_item(server: &mut Harness, client: &Client) {
    server.request(
        client,
        PlayRequest::PlayerAction {
            action: PlayerActionKind::ReleaseUseItem,
            pos: GROUND,
            face: Direction::Up,
            sequence: 0,
        },
    );
}

fn arrows(server: &mut Harness) -> Vec<mc_bamboo_blocks::entity::arrow::ArrowPublic> {
    use mc_bamboo_blocks::entity::KindPublic;
    let region = server.region();
    region
        .entities
        .ids()
        .into_iter()
        .filter_map(|id| match region.entities.public_at(id)?.kind {
            KindPublic::Arrow(arrow) => Some(arrow),
            _ => None,
        })
        .collect()
}

fn enchantment_id(name: &str) -> i32 {
    mc_bamboo_data::registries::SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == "minecraft:enchantment")
        .and_then(|registry| registry.entries.iter().position(|entry| *entry == name))
        .expect("enchantment is registered") as i32
}

fn enchant(
    server: &mut Harness,
    client: &Client,
    slot: usize,
    name: &str,
    level: i32,
) -> ItemStack {
    let player = server.region().players.get(&client.id).unwrap();
    let mut stack = player.inventory.slots[slot].clone();
    let mut patch = ComponentPatch::new();
    patch.put(
        kinds::ENCHANTMENTS,
        PatchEntry::Set(ComponentValue::Enchantments(vec![(
            enchantment_id(name),
            level,
        )])),
    );
    let (added, removed) = patch.to_opaque();
    stack.added = added;
    stack.removed = removed;
    stack
}

/// `Player.getProjectile` 找的是 `Inventory` 下标 0..36、盔甲、副手；快捷栏从 36 开始。
fn put(server: &mut Harness, client: &Client, slot: usize, stack: ItemStack) {
    let player = server.region().players.get_mut(&client.id).expect("player");
    player.inventory.slots[slot] = stack;
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

fn item_used_count(item: i32, count: i32) -> Vec<u8> {
    let mut body = Vec::new();
    for value in [1, mc_bamboo_data::registries::stat_type::USED, item, count] {
        mc_bamboo_protocol::codec::write_varint(&mut body, value);
    }
    body
}

#[test]
fn a_full_draw_bow_spawns_an_arrow_and_spends_one() {
    let (mut server, client) = settled();
    put(&mut server, &client, 36, ItemStack::new(items::BOW, 1));
    put(&mut server, &client, 37, ItemStack::new(items::ARROW, 5));
    server
        .region()
        .players
        .get_mut(&client.id)
        .unwrap()
        .game_mode = GameMode::Survival;
    client.take_frames();

    use_item(&mut server, &client, 1);
    server.tick();
    for _ in 0..19 {
        server.tick();
    }
    use_item(&mut server, &client, 2);
    release_item(&mut server, &client);
    server.tick();

    let spawned = arrows(&mut server);
    assert_eq!(spawned.len(), 1, "满弓射出一支箭");
    assert_eq!(spawned[0].pickup, Pickup::Allowed);
    assert!(spawned[0].critical, "满弓的箭是暴击");
    let player = server.region().players.get(&client.id).unwrap();
    assert_eq!(player.inventory.slots[37].count, 4, "生存模式消耗一支箭");
    let weapon = player.inventory.slots[36].clone();
    assert_eq!(
        defaults::resolve(weapon.item, &weapon.patch(), kinds::DAMAGE),
        Some(&ComponentValue::Damage(1)),
        "弓掉一点耐久"
    );
    let slots: Vec<_> = client
        .take_frames()
        .into_iter()
        .filter(|frame| frame.id == clientbound::CONTAINER_SET_SLOT)
        .map(|frame| {
            let mut reader = mc_bamboo_protocol::codec::Reader::new(&frame.body);
            reader.varint().unwrap();
            reader.varint().unwrap();
            reader.i16().unwrap()
        })
        .collect();
    assert!(slots.contains(&37));
    assert_eq!(
        request_stats(&mut server, &client),
        [item_used_count(items::BOW, 1)]
    );
    assert_eq!(request_stats(&mut server, &client), [[0x00]]);
}

#[test]
fn a_bow_released_too_early_spawns_nothing() {
    let (mut server, client) = settled();
    put(&mut server, &client, 36, ItemStack::new(items::BOW, 1));
    put(&mut server, &client, 37, ItemStack::new(items::ARROW, 5));
    server
        .region()
        .players
        .get_mut(&client.id)
        .unwrap()
        .game_mode = GameMode::Survival;

    use_item(&mut server, &client, 1);
    server.tick();
    release_item(&mut server, &client);
    server.tick();

    assert!(arrows(&mut server).is_empty());
    let player = server.region().players.get(&client.id).unwrap();
    assert_eq!(player.inventory.slots[37].count, 5);
    assert_eq!(request_stats(&mut server, &client), [[0x00]]);
}

#[test]
fn infinity_keeps_plain_arrows_and_marks_them_creative_only() {
    let (mut server, client) = settled();
    put(&mut server, &client, 36, ItemStack::new(items::BOW, 1));
    put(&mut server, &client, 37, ItemStack::new(items::ARROW, 5));
    let bow = enchant(&mut server, &client, 36, "minecraft:infinity", 1);
    put(&mut server, &client, 36, bow);
    server
        .region()
        .players
        .get_mut(&client.id)
        .unwrap()
        .game_mode = GameMode::Survival;

    use_item(&mut server, &client, 1);
    server.tick();
    for _ in 0..20 {
        server.tick();
    }
    release_item(&mut server, &client);
    server.tick();

    let player = server.region().players.get(&client.id).unwrap();
    assert_eq!(player.inventory.slots[37].count, 5, "无限附魔不消耗普通箭");
    assert_eq!(arrows(&mut server).len(), 1);
    assert_eq!(arrows(&mut server)[0].pickup, Pickup::CreativeOnly);
}

#[test]
fn a_charged_crossbow_fires_and_multishot_spends_one_arrow() {
    let (mut server, client) = settled();
    put(&mut server, &client, 36, ItemStack::new(items::CROSSBOW, 1));
    put(&mut server, &client, 37, ItemStack::new(items::ARROW, 5));
    let crossbow = enchant(&mut server, &client, 36, "minecraft:multishot", 1);
    put(&mut server, &client, 36, crossbow);
    server
        .region()
        .players
        .get_mut(&client.id)
        .unwrap()
        .game_mode = GameMode::Survival;

    use_item(&mut server, &client, 1);
    server.tick();
    for _ in 0..25 {
        server.tick();
    }
    let player = server.region().players.get(&client.id).unwrap();
    let crossbow = player.inventory.slots[36].clone();
    assert!(
        matches!(
            defaults::resolve(crossbow.item, &crossbow.patch(), kinds::CHARGED_PROJECTILES),
            Some(ComponentValue::ChargedProjectiles(projectiles)) if projectiles.len() == 3
        ),
        "装填后组件里要有三发弹药"
    );

    release_item(&mut server, &client);
    server.tick();
    use_item(&mut server, &client, 2);
    server.tick();

    assert_eq!(arrows(&mut server).len(), 3, "多重射击一次射出三支箭");
    let region = server.region();
    for (index, uid) in region.entities.ids().into_iter().enumerate() {
        let entity = region.entities.get(uid).unwrap();
        let mc_bamboo_blocks::entity::EntityKind::Arrow(data) = &entity.kind else {
            panic!("unexpected entity in crossbow fixture");
        };
        let weapon = data.weapon.as_ref().unwrap();
        assert!(!matches!(
            defaults::resolve(weapon.item, &weapon.patch(), kinds::CHARGED_PROJECTILES),
            Some(ComponentValue::ChargedProjectiles(projectiles)) if !projectiles.is_empty()
        ));
        assert_eq!(
            defaults::resolve(weapon.item, &weapon.patch(), kinds::DAMAGE),
            Some(&ComponentValue::Damage(index as i32))
        );
    }

    let player = server.region().players.get(&client.id).unwrap();
    assert_eq!(player.inventory.slots[37].count, 4, "齐射只消耗一支箭");
    assert_eq!(
        request_stats(&mut server, &client),
        [item_used_count(items::CROSSBOW, 1)]
    );
}

#[test]
fn quick_charge_shortens_the_charge_to_fifteen_ticks() {
    let (mut server, client) = settled();
    put(&mut server, &client, 36, ItemStack::new(items::CROSSBOW, 1));
    put(&mut server, &client, 37, ItemStack::new(items::ARROW, 5));
    let crossbow = enchant(&mut server, &client, 36, "minecraft:quick_charge", 2);
    put(&mut server, &client, 36, crossbow);
    server
        .region()
        .players
        .get_mut(&client.id)
        .unwrap()
        .game_mode = GameMode::Survival;

    use_item(&mut server, &client, 1);
    server.tick();
    for _ in 0..14 {
        server.tick();
    }
    let player = server.region().players.get(&client.id).unwrap();
    let crossbow = player.inventory.slots[36].clone();
    assert!(
        !matches!(
            defaults::resolve(crossbow.item, &crossbow.patch(), kinds::CHARGED_PROJECTILES),
            Some(ComponentValue::ChargedProjectiles(projectiles)) if !projectiles.is_empty()
        ),
        "15 刻之前还没装满"
    );
    server.tick();
    let player = server.region().players.get(&client.id).unwrap();
    let crossbow = player.inventory.slots[36].clone();
    assert!(
        matches!(
            defaults::resolve(crossbow.item, &crossbow.patch(), kinds::CHARGED_PROJECTILES),
            Some(ComponentValue::ChargedProjectiles(projectiles)) if !projectiles.is_empty()
        ),
        "快速装填 II 是 15 刻"
    );
}

#[test]
fn releasing_a_crossbow_at_the_charge_threshold_loads_it() {
    let (mut server, client) = settled();
    put(&mut server, &client, 36, ItemStack::new(items::CROSSBOW, 1));
    put(&mut server, &client, 37, ItemStack::new(items::ARROW, 5));
    let crossbow = enchant(&mut server, &client, 36, "minecraft:quick_charge", 2);
    put(&mut server, &client, 36, crossbow);
    server
        .region()
        .players
        .get_mut(&client.id)
        .unwrap()
        .game_mode = GameMode::Survival;
    use_item(&mut server, &client, 1);
    for _ in 0..15 {
        server.tick();
    }
    release_item(&mut server, &client);
    server.tick();
    let player = &server.region().players[&client.id];
    let crossbow = player.inventory.held(Hand::Main);
    assert!(matches!(
        defaults::resolve(crossbow.item, &crossbow.patch(), kinds::CHARGED_PROJECTILES),
        Some(ComponentValue::ChargedProjectiles(projectiles)) if projectiles.len() == 1
    ));
    assert_eq!(player.inventory.slots[37].count, 4);
    assert!(arrows(&mut server).is_empty());
}

#[test]
fn a_charged_trident_throws_and_consumes_itself() {
    use mc_bamboo_blocks::entity::arrow::ArrowVariant;
    let (mut server, client) = settled();
    put(&mut server, &client, 36, ItemStack::new(items::TRIDENT, 1));
    server
        .region()
        .players
        .get_mut(&client.id)
        .unwrap()
        .game_mode = GameMode::Survival;
    client.take_frames();

    use_item(&mut server, &client, 1);
    server.tick();
    for _ in 0..10 {
        server.tick();
    }
    release_item(&mut server, &client);
    server.tick();

    let spawned = arrows(&mut server);
    assert_eq!(spawned.len(), 1, "蓄力够时扔出一支三叉戟");
    assert_eq!(spawned[0].variant, ArrowVariant::Trident);
    assert_eq!(spawned[0].pickup, Pickup::Allowed);
    let player = server.region().players.get(&client.id).unwrap();
    assert!(
        player.inventory.slots[36].is_empty(),
        "生存模式消耗掉手里的三叉戟"
    );
    assert_eq!(
        request_stats(&mut server, &client),
        [item_used_count(items::TRIDENT, 1)]
    );
}

#[test]
fn a_trident_released_too_early_throws_nothing() {
    let (mut server, client) = settled();
    put(&mut server, &client, 36, ItemStack::new(items::TRIDENT, 1));
    server
        .region()
        .players
        .get_mut(&client.id)
        .unwrap()
        .game_mode = GameMode::Survival;

    use_item(&mut server, &client, 1);
    server.tick();
    for _ in 0..8 {
        server.tick();
    }
    release_item(&mut server, &client);
    server.tick();

    assert!(arrows(&mut server).is_empty(), "蓄力不足 10 刻不扔");
    let player = server.region().players.get(&client.id).unwrap();
    assert_eq!(player.inventory.slots[36].item, items::TRIDENT);
    assert_eq!(request_stats(&mut server, &client), [[0x00]]);
}

#[test]
fn a_creative_trident_is_kept_and_marked_creative_only() {
    let (mut server, client) = settled();
    put(&mut server, &client, 36, ItemStack::new(items::TRIDENT, 1));
    server
        .region()
        .players
        .get_mut(&client.id)
        .unwrap()
        .game_mode = GameMode::Creative;
    client.take_frames();

    use_item(&mut server, &client, 1);
    server.tick();
    for _ in 0..10 {
        server.tick();
    }
    release_item(&mut server, &client);
    server.tick();

    let spawned = arrows(&mut server);
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0].pickup, Pickup::CreativeOnly);
    let player = server.region().players.get(&client.id).unwrap();
    let held = player.inventory.slots[36].clone();
    assert_eq!(held.item, items::TRIDENT, "创造模式不消耗三叉戟");
    assert_eq!(
        defaults::resolve(held.item, &held.patch(), kinds::DAMAGE),
        Some(&ComponentValue::Damage(0)),
        "创造模式不扣耐久"
    );
}

fn fireworks(
    server: &mut Harness,
) -> Vec<mc_bamboo_blocks::entity::firework_rocket::FireworkRocketPublic> {
    use mc_bamboo_blocks::entity::KindPublic;
    let region = server.region();
    region
        .entities
        .ids()
        .into_iter()
        .filter_map(|id| match &region.entities.public_at(id)?.kind {
            KindPublic::FireworkRocket(firework) => Some(firework.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn crossbow_loads_and_fires_multishot_fireworks_from_offhand() {
    let (mut server, client) = settled();
    put(&mut server, &client, 36, ItemStack::new(items::CROSSBOW, 1));
    // 副手放烟花火箭（槽位 45）。
    put(
        &mut server,
        &client,
        crate::inventory::OFF_HAND,
        ItemStack::new(items::FIREWORK_ROCKET, 4),
    );
    let crossbow = enchant(&mut server, &client, 36, "minecraft:multishot", 1);
    put(&mut server, &client, 36, crossbow);
    server
        .region()
        .players
        .get_mut(&client.id)
        .unwrap()
        .game_mode = GameMode::Survival;

    use_item(&mut server, &client, 1);
    server.tick();
    for _ in 0..25 {
        server.tick();
    }
    release_item(&mut server, &client);
    server.tick();
    use_item(&mut server, &client, 2);
    server.tick();

    let spawned = fireworks(&mut server);
    assert_eq!(spawned.len(), 3, "多重射击弩一次射出三枚烟花火箭");
    assert!(
        spawned.iter().all(|rocket| rocket.shot_at_angle),
        "弩射出的烟花必须是斜射（shotAtAngle）"
    );
    let player = server.region().players.get(&client.id).unwrap();
    assert_eq!(
        player.inventory.slots[crate::inventory::OFF_HAND].count,
        3,
        "齐射只消耗一枚副手烟花"
    );
    let held = &player.inventory.slots[36];
    assert_eq!(
        defaults::resolve(held.item, &held.patch(), kinds::DAMAGE),
        Some(&ComponentValue::Damage(9)),
        "每发烟花扣 3 点耐久，三发共扣 9 点"
    );
}

#[test]
fn bow_ignores_offhand_firework_rocket() {
    let (mut server, client) = settled();
    put(&mut server, &client, 36, ItemStack::new(items::BOW, 1));
    put(
        &mut server,
        &client,
        crate::inventory::OFF_HAND,
        ItemStack::new(items::FIREWORK_ROCKET, 4),
    );
    server
        .region()
        .players
        .get_mut(&client.id)
        .unwrap()
        .game_mode = GameMode::Survival;

    use_item(&mut server, &client, 1);
    server.tick();
    for _ in 0..20 {
        server.tick();
    }
    release_item(&mut server, &client);
    server.tick();

    assert!(arrows(&mut server).is_empty(), "弓不能把副手烟花当箭射");
    assert!(fireworks(&mut server).is_empty(), "弓不能射烟花");
    let player = server.region().players.get(&client.id).unwrap();
    assert_eq!(
        player.inventory.slots[crate::inventory::OFF_HAND].count,
        4,
        "副手烟花数量不变"
    );
}

#[test]
fn firework_use_on_block_spawns_rocket_and_air_use_passes() {
    let (mut server, client) = settled();
    put(
        &mut server,
        &client,
        36,
        ItemStack::new(items::FIREWORK_ROCKET, 3),
    );
    server
        .region()
        .players
        .get_mut(&client.id)
        .unwrap()
        .game_mode = GameMode::Survival;

    // 对空气右键：无鞘翅滑翔时恒为 PASS，不生成烟花也不扣物品。
    use_item(&mut server, &client, 1);
    server.tick();
    assert!(fireworks(&mut server).is_empty());
    assert_eq!(
        server.region().players[&client.id].inventory.slots[36].count,
        3
    );

    // 对方块顶面右键：生成竖直发射的烟花并扣 1 个物品。
    server.request(
        &client,
        PlayRequest::UseItemOn {
            hand: Hand::Main,
            pos: GROUND,
            face: Direction::Up,
            cursor: (0.5, 1.0, 0.5),
            inside: false,
            world_border_hit: false,
            sequence: 2,
        },
    );
    server.tick();
    let spawned = fireworks(&mut server);
    assert_eq!(spawned.len(), 1);
    assert!(!spawned[0].shot_at_angle);
    assert_eq!(spawned[0].owner, Some(client.id.0));
    assert_eq!(
        server.region().players[&client.id].inventory.slots[36].count,
        2
    );
}
