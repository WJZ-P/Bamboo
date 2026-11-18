//! 真实 `World` 里，实体逻辑能看见本岛玩家：牛看向 / 走向拿小麦的人，
//! 经验球朝附近玩家加速。这里不用 `testing.rs` 的假世界。
//!
//! In a real `World`, entity logic sees the island's players: a cow looks at
//! and walks to a wheat-holding player, and an experience orb accelerates
//! toward a nearby player. No fake world from `testing.rs` is used.

use mc_bamboo_blocks::entity::{Entity, EntityKind, mob::goals::Goal};
use mc_bamboo_core::{ChunkPos, Hand, Vec3};
use mc_bamboo_data::items;
use mc_bamboo_protocol::item::ItemStack;

use super::tests::{Harness, SPAWN_Y};

/// 已加载模式里实体只在「半径 2 的邻居都已交付」的区块上 tick；
/// 无限超平坦模式下 `ensure_loaded` 直接返回。两种模式都让场景可 tick。
///
/// In loaded mode entities tick only when the radius-2 neighbourhood of
/// their chunk has been delivered; `ensure_loaded` is a no-op in infinite
/// flat mode. Both modes therefore get a tickable scene.
fn ensure_ticking(server: &mut Harness) {
    let positions: Vec<ChunkPos> = (-2..=2)
        .flat_map(|x| (-2..=2).map(move |z| ChunkPos::new(x, z)))
        .collect();
    server.ensure_loaded(&positions);
}

/// 直接跑实体阶段，不牵扯区块发送和进服包序。
fn entity_tick(server: &mut Harness) {
    crate::drops::pickup_before_entities(server.region());
    let (mut level, ticks) = server.region().block_level();
    let _ = mc_bamboo_blocks::run_entity_tick(&mut level, ticks);
}

/// 把一名玩家搬到指定位置，手里放指定物品。
fn place_player(server: &mut Harness, id: crate::inbound::PlayerId, position: Vec3, held: i32) {
    let region = server.region();
    let slot = {
        let player = region.players.get(&id).expect("玩家已进服");
        player.inventory.hand_slot(Hand::Main)
    };
    let player = region.players.get_mut(&id).expect("玩家已进服");
    player.set_position(&mut region.player_grid, position);
    player.inventory.slots[slot] = if held == 0 {
        ItemStack::empty()
    } else {
        ItemStack::new(held, 1)
    };
}

/// 只留下一个目标，排除牛的随机游走，让断言只反映「有没有看见玩家」。
fn keep_goal(server: &mut Harness, cow: i32, goal: Goal) {
    use mc_bamboo_blocks::entity::mob::goals::WrappedGoal;
    server
        .region()
        .entities
        .with_mut(cow, |entity| {
            let mob = entity.kind.mob_mut().expect("牛是生物");
            mob.goals.goals = vec![WrappedGoal {
                priority: 0,
                running: false,
                goal,
            }];
        })
        .expect("牛还在表里");
}

fn cow_goal(server: &mut Harness, cow: i32) -> Goal {
    let entity = server.region().entities.get(cow).expect("牛还在表里");
    entity.kind.mob().expect("牛是生物").goals.goals[0]
        .goal
        .clone()
}

#[test]
fn a_wheat_holding_player_makes_a_cow_look_and_walk_over() {
    let mut server = Harness::new(2);
    let client = server.join("Farmer");
    server.tick(); // 处理进服
    ensure_ticking(&mut server);
    let _ = client.take_frames();
    let at = Vec3::new(12.5, SPAWN_Y, 8.5);
    place_player(&mut server, client.id, at, items::WHEAT);

    let cow = server.spawn_entity(Entity::cow(Vec3::new(8.5, SPAWN_Y, 8.5)));
    keep_goal(
        &mut server,
        cow,
        Goal::Tempt {
            speed: 1.25,
            player: None,
            calm_down: 0,
        },
    );
    for _ in 0..30 {
        entity_tick(&mut server);
    }
    let goal = cow_goal(&mut server, cow);
    assert!(
        matches!(goal, Goal::Tempt { player: Some(uid), .. } if uid == client.id.0),
        "拿小麦的玩家应该成为牛的诱惑目标：{goal:?}"
    );
    let entity = server.region().entities.get(cow).expect("牛还在表里");
    let mob = entity.kind.mob().expect("牛是生物");
    assert!(
        entity.position.x > 8.6,
        "牛应该朝玩家走过去，实际 x = {}",
        entity.position.x
    );
    assert!(mob.y_head_rot != 0.0, "牛应该转头看着玩家");
}

#[test]
fn an_empty_handed_player_does_not_tempt_a_cow() {
    let mut server = Harness::new(2);
    let client = server.join("Farmer");
    server.tick();
    ensure_ticking(&mut server);
    let _ = client.take_frames();
    let at = Vec3::new(12.5, SPAWN_Y, 8.5);
    place_player(&mut server, client.id, at, 0);

    let cow = server.spawn_entity(Entity::cow(Vec3::new(8.5, SPAWN_Y, 8.5)));
    keep_goal(
        &mut server,
        cow,
        Goal::Tempt {
            speed: 1.25,
            player: None,
            calm_down: 0,
        },
    );
    for _ in 0..30 {
        entity_tick(&mut server);
    }
    let goal = cow_goal(&mut server, cow);
    assert!(
        matches!(goal, Goal::Tempt { player: None, .. }),
        "空手不该引诱牛：{goal:?}"
    );
    let entity = server.region().entities.get(cow).expect("牛还在表里");
    assert!(
        entity.position.x <= 8.6,
        "牛没有理由朝玩家走：x = {}",
        entity.position.x
    );
}

#[test]
fn a_cow_looks_at_a_nearby_player() {
    let mut server = Harness::new(2);
    let client = server.join("Watcher");
    server.tick();
    ensure_ticking(&mut server);
    let _ = client.take_frames();
    let at = Vec3::new(11.5, SPAWN_Y, 8.5);
    place_player(&mut server, client.id, at, 0);

    let cow = server.spawn_entity(Entity::cow(Vec3::new(8.5, SPAWN_Y, 8.5)));
    keep_goal(
        &mut server,
        cow,
        Goal::LookAtPlayer {
            look_distance: 6.0,
            probability: 1.0,
            look_at: None,
            look_time: 0,
        },
    );
    for _ in 0..4 {
        entity_tick(&mut server);
    }
    let goal = cow_goal(&mut server, cow);
    assert!(
        matches!(goal, Goal::LookAtPlayer { look_at: Some(uid), .. } if uid == client.id.0),
        "附近的玩家应该成为注视目标：{goal:?}"
    );
}

#[test]
fn an_experience_orb_accelerates_toward_a_nearby_player() {
    let mut server = Harness::new(2);
    let client = server.join("Collector");
    server.tick();
    ensure_ticking(&mut server);
    let _ = client.take_frames();
    let at = Vec3::new(12.5, SPAWN_Y, 8.5);
    place_player(&mut server, client.id, at, 0);

    let orb = server.spawn_entity(Entity::experience_orb(
        Vec3::new(8.5, SPAWN_Y + 1.0, 8.5),
        7,
    ));
    entity_tick(&mut server);
    let entity = server.region().entities.get(orb).expect("经验球还在表里");
    let EntityKind::ExperienceOrb(state) = &entity.kind else {
        panic!("实体种类应该是经验球");
    };
    assert_eq!(state.following, Some(client.id.0), "经验球应该跟随这名玩家");
    assert!(
        entity.delta.x > 0.0,
        "经验球应该朝 +x 的玩家加速，实际 delta.x = {}",
        entity.delta.x
    );
}

/// 并行驱动的算阶段用另一份只读视图，也必须带上同一份玩家名单。
#[test]
fn the_parallel_compute_phase_sees_the_same_players() {
    let mut server = Harness::new(2);
    let client = server.join("Collector");
    server.tick();
    ensure_ticking(&mut server);
    let _ = client.take_frames();
    let at = Vec3::new(12.5, SPAWN_Y, 8.5);
    place_player(&mut server, client.id, at, 0);

    let orb = server.spawn_entity(Entity::experience_orb(
        Vec3::new(8.5, SPAWN_Y + 1.0, 8.5),
        7,
    ));
    crate::interaction::parallel_entity_ticks(1, server.region());
    let entity = server.region().entities.get(orb).expect("经验球还在表里");
    let EntityKind::ExperienceOrb(state) = &entity.kind else {
        panic!("实体种类应该是经验球");
    };
    assert_eq!(state.following, Some(client.id.0), "并行算阶段也要看见玩家");
    assert!(entity.delta.x > 0.0, "并行算阶段也要朝玩家加速");
}

#[test]
fn a_spectator_does_not_attract_an_experience_orb() {
    use mc_bamboo_protocol::packet::play::GameMode;

    let mut server = Harness::new(2);
    let client = server.join("Ghost");
    server.tick();
    ensure_ticking(&mut server);
    let _ = client.take_frames();
    let at = Vec3::new(12.5, SPAWN_Y, 8.5);
    place_player(&mut server, client.id, at, 0);
    server
        .region()
        .players
        .get_mut(&client.id)
        .expect("玩家已进服")
        .game_mode = GameMode::Spectator;

    let orb = server.spawn_entity(Entity::experience_orb(
        Vec3::new(8.5, SPAWN_Y + 1.0, 8.5),
        7,
    ));
    entity_tick(&mut server);
    let entity = server.region().entities.get(orb).expect("经验球还在表里");
    let EntityKind::ExperienceOrb(state) = &entity.kind else {
        panic!("实体种类应该是经验球");
    };
    assert_eq!(state.following, None, "旁观者不该被跟随");
}

#[test]
fn an_orb_on_the_player_grants_experience_and_disappears() {
    let mut server = Harness::new(2);
    let client = server.join("Collector");
    server.tick();
    ensure_ticking(&mut server);
    let _ = client.take_frames();
    let at = Vec3::new(12.5, SPAWN_Y, 8.5);
    place_player(&mut server, client.id, at, 0);

    let orb = server.spawn_entity(Entity::experience_orb(at, 7));
    crate::drops::pickup_before_entities(server.region());
    let player = server.player(&client);
    assert_eq!(player.experience.level, 1);
    assert_eq!(player.experience.total, 7);
    assert_eq!(player.take_xp_delay, 2);
    let entity = server.region().entities.get(orb).expect("这一刻还在表里");
    assert!(entity.removed, "count 归零的球被打上消失标记");
    server.region().links.flush();
    let frames = client.take_frames();
    assert!(
        frames
            .iter()
            .any(|frame| frame.id == mc_bamboo_data::packets::play::clientbound::TAKE_ITEM_ENTITY),
        "拾取要发配对包"
    );
}

#[test]
fn the_pickup_cooldown_blocks_a_second_orb_until_it_expires() {
    let mut server = Harness::new(2);
    let client = server.join("Collector");
    server.tick();
    ensure_ticking(&mut server);
    let _ = client.take_frames();
    let at = Vec3::new(12.5, SPAWN_Y, 8.5);
    place_player(&mut server, client.id, at, 0);

    let first = server.spawn_entity(Entity::experience_orb(at, 7));
    crate::drops::pickup_before_entities(server.region());
    assert!(server.region().entities.get(first).unwrap().removed);
    let _ = client.take_frames();

    let second = server.spawn_entity(Entity::experience_orb(at, 3));
    crate::drops::pickup_before_entities(server.region());
    assert_eq!(
        server.player(&client).experience.total,
        7,
        "冷却中不该加经验"
    );
    assert!(!server.region().entities.get(second).unwrap().removed);
    crate::drops::pickup_before_entities(server.region());
    assert_eq!(
        server.player(&client).experience.total,
        10,
        "第二刻冷却归零"
    );
    assert!(server.region().entities.get(second).unwrap().removed);
}

#[test]
fn a_damaged_mending_item_uses_the_orb_before_experience() {
    use mc_bamboo_data::{items, registries::data_component_type as kinds};
    use mc_bamboo_loot::enchantment_id;
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};

    let mut server = Harness::new(2);
    let client = server.join("Mender");
    server.tick();
    ensure_ticking(&mut server);
    let _ = client.take_frames();
    let at = Vec3::new(12.5, SPAWN_Y, 8.5);
    place_player(&mut server, client.id, at, 0);
    {
        let region = server.region();
        let player = region.players.get_mut(&client.id).unwrap();
        let slot = player.inventory.hand_slot(Hand::Main);
        let mut patch = ComponentPatch::new();
        patch.put(kinds::DAMAGE, PatchEntry::Set(ComponentValue::Damage(100)));
        patch.put(
            kinds::ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(vec![(
                enchantment_id("minecraft:mending").unwrap(),
                1,
            )])),
        );
        let (added, removed) = patch.to_opaque();
        player.inventory.slots[slot] = ItemStack {
            item: items::DIAMOND_SWORD,
            count: 1,
            added,
            removed,
        };
    }

    let orb = server.spawn_entity(Entity::experience_orb(at, 7));
    crate::drops::pickup_before_entities(server.region());
    assert_eq!(server.player(&client).experience.total, 0, "经验全用来修补");
    assert!(server.region().entities.get(orb).unwrap().removed);
    let player = server.player(&client);
    let slot = player.inventory.hand_slot(Hand::Main);
    let patch = player.inventory.slots[slot].patch();
    match patch.get(kinds::DAMAGE) {
        Some(ComponentValue::Damage(value)) => assert_eq!(*value, 86, "7 点经验 ×2 修 14 点"),
        other => panic!("修补后 damage 组件不对：{other:?}"),
    }
}
