//! 玩家死亡、掉落、重生、回血的岛上重放测试。
//!
//! 客户端包的逐字节对拍在 `mob-oracle`；这里核对 `Region` 里的状态、
//! `Harness` 上的包 ID 顺序，以及多岛时「在岛 A 死、重生点在岛 B」的搬运。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{gamerules, items, packets::play::clientbound};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{ClientCommandKind, GameMode, PlayRequest},
};

use super::{
    island_split_tests::{Scene, set_up, with_stack},
    tests::{Client, Harness, frames_with_id},
};

/// 把规则改成指定值（`GameRuleSet::assign` 的公开包装在 gamerules 模块里）。
fn rule(harness: &mut Harness, index: usize, value: &str) {
    harness.world.master.global.rules_mut().assign(index, value);
    harness.world.publish_global();
}

/// 让一名玩家死掉：`/kill @s` 走 `genericKill` 的完整死亡流程。
fn kill_self(scene: &mut Scene, client: &Client) {
    super::island_crossing_tests::make_op(scene, client);
    scene
        .server
        .request(client, PlayRequest::unsigned_command("kill @s".to_owned()));
    scene.server.tick();
}

#[test]
fn a_player_death_drops_inventory_experience_and_shows_the_death_screen() {
    let mut server = Harness::new(2);
    let client = server.join_and_settle("Kitten");
    server
        .region()
        .players
        .get_mut(&client.id)
        .expect("玩家")
        .permission_level = 4;
    server.tick();
    let _ = client.take_frames();
    {
        let region = server.region();
        let player = region.players.get_mut(&client.id).expect("玩家");
        player.game_mode = GameMode::Survival;
        player.experience.level = 9;
        player.experience.total = 123;
        // 留一包经验球让经验等级在下一刻变化（`/kill` 本身的死亡消息不会加经验）。
        player.inventory.slots[36] = ItemStack::new(items::STONE, 3);
    }
    server.request(&client, PlayRequest::unsigned_command("kill @s".to_owned()));
    server.tick();
    let frames = client.take_frames();
    assert_eq!(
        frames_with_id(&frames, clientbound::PLAYER_COMBAT_KILL).len(),
        1,
        "一次死亡一个死亡界面包"
    );
    let region = server.region();
    let player = &region.players[&client.id];
    assert!(player.dead);
    assert!(player.awaiting_respawn);
    assert_eq!(player.health.health, 0.0);
    assert!(player.experience.level >= 9, "死亡这一刻官方还没清经验");
    assert!(player.inventory.slots[36].is_empty(), "物品栏已掉空");
    assert!(player.last_death_location.is_some());
    let items = region
        .entities
        .iter_public()
        .filter(|public| {
            matches!(
                public.kind,
                mc_bamboo_blocks::entity::KindPublic::Item { .. }
            )
        })
        .count();
    let orbs = region
        .entities
        .iter_public()
        .filter(|public| {
            matches!(
                public.kind,
                mc_bamboo_blocks::entity::KindPublic::ExperienceOrb { .. }
            )
        })
        .count();
    assert_eq!(items, 1, "一件地面物品");
    assert!(orbs > 0, "等级 9 掉 63 点经验，拆成球");
}

#[test]
fn keep_inventory_and_immediate_respawn_keep_everything() {
    let mut server = Harness::new(2);
    rule(&mut server, gamerules::KEEP_INVENTORY, "true");
    rule(&mut server, gamerules::IMMEDIATE_RESPAWN, "true");
    let client = server.join_and_settle("Kitten");
    server
        .region()
        .players
        .get_mut(&client.id)
        .expect("玩家")
        .permission_level = 4;
    server.tick();
    let _ = client.take_frames();
    {
        let region = server.region();
        let player = region.players.get_mut(&client.id).expect("玩家");
        player.game_mode = GameMode::Survival;
        player.experience.level = 5;
        player.inventory.slots[36] = ItemStack::new(items::STONE, 3);
    }
    server.request(&client, PlayRequest::unsigned_command("kill @s".to_owned()));
    server.tick();
    let frames = client.take_frames();
    assert_eq!(
        frames_with_id(&frames, clientbound::RESPAWN).len(),
        1,
        "immediate_respawn 当场重生"
    );
    let region = server.region();
    let player = &region.players[&client.id];
    assert!(!player.dead, "已经重生");
    assert!(!player.awaiting_respawn);
    assert_eq!(player.health.health, 20.0);
    assert_eq!(player.experience.level, 5);
    assert_eq!(player.inventory.slots[36].item, items::STONE);
    assert_eq!(
        region
            .entities
            .iter_public()
            .filter(|public| matches!(
                public.kind,
                mc_bamboo_blocks::entity::KindPublic::Item { .. }
            ))
            .count(),
        0,
        "keepInventory 不掉落"
    );
}

#[test]
fn perform_respawn_sends_the_official_sequence_in_order() {
    let mut server = Harness::new(2);
    let client = server.join_and_settle("Kitten");
    server
        .region()
        .players
        .get_mut(&client.id)
        .expect("玩家")
        .permission_level = 4;
    server.tick();
    let _ = client.take_frames();
    {
        server
            .region()
            .players
            .get_mut(&client.id)
            .expect("玩家")
            .game_mode = GameMode::Survival;
    }
    server.request(&client, PlayRequest::unsigned_command("kill @s".to_owned()));
    server.tick();
    let _ = client.take_frames();
    server.request(
        &client,
        PlayRequest::ClientCommand {
            action: ClientCommandKind::PerformRespawn,
        },
    );
    server.tick();
    let ids = client.take_ids();
    let first = ids
        .iter()
        .position(|id| *id == clientbound::RESPAWN)
        .expect("重生包");
    let rest = &ids[first..];
    assert_eq!(rest[0], clientbound::RESPAWN);
    assert_eq!(rest[1], clientbound::PLAYER_POSITION);
    assert_eq!(rest[2], clientbound::SET_DEFAULT_SPAWN_POSITION);
    assert_eq!(rest[3], clientbound::CHANGE_DIFFICULTY);
    assert_eq!(rest[4], clientbound::SET_EXPERIENCE);
    assert_eq!(rest[5], clientbound::INITIALIZE_BORDER);
    assert_eq!(rest[6], clientbound::SET_TIME);
    assert_eq!(rest[7], clientbound::SET_DEFAULT_SPAWN_POSITION);
    assert_eq!(rest[8], clientbound::GAME_EVENT);
    assert_eq!(rest[9], clientbound::ENTITY_EVENT);
    assert_eq!(rest[10], clientbound::COMMANDS);
    assert_eq!(rest[11], clientbound::SET_ENTITY_DATA);
    assert_eq!(rest[12], clientbound::SET_EQUIPMENT);
    assert_eq!(rest[13], clientbound::CONTAINER_SET_CONTENT);
    assert_eq!(rest[14], clientbound::SET_HELD_SLOT);
    assert_eq!(rest[15], clientbound::PLAYER_ABILITIES);
    let region = server.region();
    let player = &region.players[&client.id];
    assert!(!player.dead);
    assert_eq!(player.health.health, 20.0);
    assert_eq!(
        region
            .player_grid
            .near(player.position().x, player.position().z, 1.0)
            .count(),
        1
    );
}

#[test]
fn vanishing_curse_items_are_removed_without_dropping() {
    let mut server = Harness::new(2);
    let client = server.join_and_settle("Kitten");
    server
        .region()
        .players
        .get_mut(&client.id)
        .expect("玩家")
        .permission_level = 4;
    server.tick();
    let _ = client.take_frames();
    {
        let player = server.region().players.get_mut(&client.id).expect("玩家");
        player.game_mode = GameMode::Survival;
        player.inventory.slots[36] = crate::player_death::vanishing_stack(items::DIAMOND_SWORD);
        player.inventory.slots[37] = ItemStack::new(items::STONE, 1);
    }
    server.request(&client, PlayRequest::unsigned_command("kill @s".to_owned()));
    server.tick();
    let region = server.region();
    let dropped: Vec<_> = region
        .entities
        .iter_public()
        .filter(|public| {
            matches!(
                public.kind,
                mc_bamboo_blocks::entity::KindPublic::Item { .. }
            )
        })
        .collect();
    assert_eq!(dropped.len(), 1, "只有没有诅咒的那件掉出来");
}

#[test]
fn peaceful_and_food_regeneration_follow_the_official_interval() {
    let mut server = Harness::new(2);
    let client = server.join_and_settle("Kitten");
    server
        .region()
        .players
        .get_mut(&client.id)
        .expect("玩家")
        .permission_level = 4;
    server.tick();
    server.world.master.global.state_mut().difficulty = crate::world_state::Difficulty::Peaceful;
    server.world.publish_global();
    {
        let player = server.region().players.get_mut(&client.id).expect("玩家");
        player.game_mode = GameMode::Survival;
        player.health.health = 10.0;
        player.health.food.food_level = 20;
        player.health.food.saturation = 5.0;
        player.health.food.exhaustion = 0.0;
    }
    // 官方 tickCount 每 20 刻回 1 血（tick 数从 1 开始，第 20 刻触发），
    // 满饥饿 + 饱和度每 10 刻再回 min(sat,6)/6。
    for _ in 0..10 {
        crate::player_hurt::tick(server.region());
    }
    let player = &server.region().players[&client.id];
    assert_eq!(player.health.food.tick_timer, 0, "第 10 刻触发回血后归零");
    assert!(player.health.health > 10.0, "饱和度回血生效");
}

#[test]
fn starvation_damages_a_hungry_player_after_eighty_ticks() {
    let mut server = Harness::new(2);
    let client = server.join_and_settle("Kitten");
    server
        .region()
        .players
        .get_mut(&client.id)
        .expect("玩家")
        .permission_level = 4;
    server
        .world
        .master
        .global
        .set_difficulty(crate::world_state::Difficulty::Normal);
    server.world.publish_global();
    server.tick();
    {
        let player = server.region().players.get_mut(&client.id).expect("玩家");
        player.game_mode = GameMode::Survival;
        player.health.health = 20.0;
        player.health.food.food_level = 0;
        player.health.food.saturation = 0.0;
        player.health.food.exhaustion = 0.0;
        player.health.food.tick_timer = 79;
        // 官方 `ServerPlayer.tickRegeneration` 只在和平难度补饥饿；普通难度
        // 不补，foodLevel 会一直为 0 直到饿到掉血。
        player.health.damage_cooldown = 0;
    }
    crate::player_hurt::tick(server.region());
    let player = &server.region().players[&client.id];
    assert!(player.health.health < 20.0, "第 80 刻造成一点饥饿伤害");
    assert_eq!(player.health.food.tick_timer, 0);
}

/// 在岛 A 死、重生点在岛 B：掉落留在 A，玩家随重生点搬到 B。
#[test]
fn a_death_on_island_a_with_a_spawn_on_island_b_relocates_the_player() {
    with_stack(|| {
        let mut scene = set_up(21, Harness::new_islands(2, 0));
        let client = scene.clients[0].1.clone();
        let (here, there) = (scene.centers[0], scene.centers[1]);
        assert_ne!(
            scene.server.world.islands.island_of_chunk(here),
            scene.server.world.islands.island_of_chunk(there),
            "两个不同的岛"
        );
        {
            let region =
                super::island_split_tests::region_of_player(&mut scene.server.world, client.id);
            let player = region.players.get_mut(&client.id).expect("玩家");
            player.game_mode = GameMode::Survival;
            player.respawn = Some(crate::world_state::RespawnData::of(
                BlockPos::new(there.x * 16 + 8, -59, there.z * 16 + 8),
                0.0,
                0.0,
            ));
            player.inventory.slots[36] = ItemStack::new(items::STONE, 1);
        }
        kill_self(&mut scene, &client);
        {
            let start = scene
                .server
                .world
                .islands
                .island_of_player(client.id)
                .expect("在岛上");
            let region = scene
                .server
                .world
                .islands
                .get_mut(start)
                .expect("岛")
                .region
                .players
                .get(&client.id)
                .expect("玩家");
            assert!(region.dead, "死亡界面等着重生");
            assert!(region.awaiting_respawn);
        }
        scene.server.request(
            &client,
            PlayRequest::ClientCommand {
                action: ClientCommandKind::PerformRespawn,
            },
        );
        for _ in 0..4 {
            scene.server.tick();
        }
        let world = &scene.server.world;
        let end = world
            .islands
            .island_of_player(client.id)
            .expect("重生后还在世界里");
        assert_eq!(
            Some(end),
            world.islands.island_of_chunk(there),
            "玩家搬到了重生点所在的岛"
        );
        let region = &world.islands.get(end).expect("岛").region;
        let player = &region.players[&client.id];
        assert!(!player.dead);
        assert!(
            (player.position().x - f64::from(there.x * 16 + 8) - 0.5).abs() < 0.001,
            "落在重生点中心"
        );
        // 掉落物留在死时所在的岛。
        let start = world.islands.island_of_chunk(here).expect("岛 A");
        let drops = world
            .islands
            .get(start)
            .expect("岛 A")
            .region
            .entities
            .iter_public()
            .any(|public| {
                matches!(
                    public.kind,
                    mc_bamboo_blocks::entity::KindPublic::Item { .. }
                )
            });
        assert!(drops, "掉落物留在岛 A");
        scene.server.world.assert_island_invariants();
    });
}
