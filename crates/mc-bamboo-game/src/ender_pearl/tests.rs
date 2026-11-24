//! 末影珍珠绑定的单元测试：冷却、注册、区块票、存档往返和传送。

use mc_bamboo_blocks::entity::{EntityKind, thrown_egg::ThrowableKind};
use mc_bamboo_core::{ChunkPos, Hand, Vec3};
use mc_bamboo_data::items;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{ClientInformation, GameMode},
};
use mc_bamboo_storage::player_data::PlayerData;
use mc_bamboo_world::{director::TicketOp, ticket::TicketType};
use uuid::Uuid;

use super::*;
use crate::{inbound::PlayerId, player::Player, region::Region};

fn region_with_pearl_player() -> (Region, PlayerId) {
    let mut region = Region::new(2, 20, false);
    let id = PlayerId(1 << 24);
    let player = Player::new(
        id,
        Uuid::from_u128(1),
        "Pearl".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    region.insert_player(player);
    (region, id)
}

fn pearl_shot(owner: PlayerId) -> mc_bamboo_blocks::entity::thrown_egg::EggShot {
    mc_bamboo_blocks::entity::thrown_egg::EggShot {
        position: Vec3::new(0.5, 65.52, 0.5),
        direction: Vec3::new(1.0, 0.0, 0.0),
        power: 1.5,
        uncertainty: 0.0,
        projectile: ThrowableKind::EnderPearl,
        source_movement: None,
        owner: Some(owner.0),
        owner_uuid: Some(*Uuid::from_u128(1).as_bytes()),
        item: ItemStack::new(items::ENDER_PEARL, 1),
    }
}

#[test]
fn the_pearls_use_cooldown_component_is_twenty_ticks() {
    let stack = ItemStack::new(items::ENDER_PEARL, 3);
    assert_eq!(use_cooldown(&stack), (20, None));
    let egg = ItemStack::new(items::EGG, 3);
    assert_eq!(use_cooldown(&egg), (0, None));
}

#[test]
fn throwing_a_pearl_registers_it_and_the_cooldown_blocks_the_next_throw() {
    let (mut region, id) = region_with_pearl_player();
    let main = region.players[&id].inventory.hand_slot(Hand::Main);
    region.players.get_mut(&id).unwrap().inventory.slots[main] =
        ItemStack::new(items::ENDER_PEARL, 3);
    assert!(crate::player_egg::throw(&mut region, id, Hand::Main));
    let player = &region.players[&id];
    assert_eq!(player.inventory.slots[main].count, 2);
    assert_eq!(player.ender_pearls.len(), 1);
    let uid = *player.ender_pearls.iter().next().unwrap();
    assert!(matches!(
        &region.entities.get(uid).unwrap().kind,
        EntityKind::Egg(egg) if egg.kind == ThrowableKind::EnderPearl
    ));
    // 20 刻内再右键直接 PASS，不生成第二枚珍珠、也不扣物品。
    assert!(!crate::player_egg::throw(&mut region, id, Hand::Main));
    assert_eq!(region.players[&id].inventory.slots[main].count, 2);
    assert_eq!(region.players[&id].ender_pearls.len(), 1);
    for _ in 0..20 {
        tick(&mut region);
    }
    assert!(crate::player_egg::throw(&mut region, id, Hand::Main));
    assert_eq!(region.players[&id].ender_pearls.len(), 2);
}

#[test]
fn a_live_pearl_keeps_a_radius_two_ticket_and_removal_drops_it() {
    let mut tracked = BTreeMap::new();
    let mut live = BTreeMap::new();
    live.insert(7, ChunkPos::new(3, -2));
    let ops = sync_tickets(&live, &mut tracked);
    assert_eq!(
        ops,
        vec![TicketOp::WithRadius {
            ty: TicketType::EnderPearl,
            pos: ChunkPos::new(3, -2),
            radius: 2,
            add: true,
        }]
    );
    // 同一区块里 38 刻不续票，第 39 刻续一次。
    for tick in 1..39 {
        assert!(sync_tickets(&live, &mut tracked).is_empty(), "tick {tick}");
    }
    assert_eq!(sync_tickets(&live, &mut tracked).len(), 1);
    // 跨区块先撤旧票再挂新票。
    live.insert(7, ChunkPos::new(4, -2));
    let ops = sync_tickets(&live, &mut tracked);
    assert_eq!(ops.len(), 2);
    assert!(matches!(ops[0], TicketOp::WithRadius { add: false, .. }));
    assert!(matches!(ops[1], TicketOp::WithRadius { add: true, .. }));
    // 珍珠消失撤票。
    live.clear();
    assert_eq!(
        sync_tickets(&live, &mut tracked),
        vec![TicketOp::WithRadius {
            ty: TicketType::EnderPearl,
            pos: ChunkPos::new(4, -2),
            radius: 2,
            add: false,
        }]
    );
}

#[test]
fn flying_pearls_round_trip_through_the_player_save() {
    let (mut region, id) = region_with_pearl_player();
    let shot = pearl_shot(id);
    let uid = {
        let (mut level, ticks) = region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        world.spawn_egg_shot(&shot)
    };
    register(&mut region, id, uid);
    tick(&mut region);
    let player = &region.players[&id];
    assert_eq!(player.saved_ender_pearls.len(), 1);
    let data = PlayerData {
        ender_pearls: player.saved_ender_pearls.clone(),
        ..PlayerData::default()
    };
    // 官方的 `ender_pearls` 是复合标签列表，每项带维度键。
    let encoded = data.encode();
    let Tag::Compound(entries) = &encoded else {
        unreachable!()
    };
    let list = entries
        .iter()
        .find(|(key, _)| key == "ender_pearls")
        .map(|(_, value)| value)
        .expect("non-empty pearl list is saved");
    let Tag::List(items) = list else {
        unreachable!()
    };
    assert_eq!(items.len(), 1);
    let decoded = PlayerData::decode(&encoded).expect("player save decodes");
    assert_eq!(decoded.ender_pearls.len(), 1);

    let (mut rejoin, rejoin_id) = region_with_pearl_player();
    load(&mut rejoin, rejoin_id, &decoded);
    let loaded = &rejoin.players[&rejoin_id];
    assert_eq!(loaded.ender_pearls.len(), 1);
    let restored_uid = *loaded.ender_pearls.iter().next().unwrap();
    let entity = rejoin.entities.get(restored_uid).expect("pearl restored");
    assert!(matches!(
        &entity.kind,
        EntityKind::Egg(egg) if egg.kind == ThrowableKind::EnderPearl
    ));
}

#[test]
fn a_dead_owner_loses_the_pearl_when_the_rule_is_on() {
    let (mut region, id) = region_with_pearl_player();
    let shot = pearl_shot(id);
    let uid = {
        let (mut level, ticks) = region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        world.spawn_egg_shot(&shot)
    };
    register(&mut region, id, uid);
    let player = region.players.get_mut(&id).unwrap();
    player.dead = true;
    // 死亡流程里 `awaiting_respawn` 在同一刻也置真，这里照着设上：官方只看 `isAlive`。
    player.awaiting_respawn = true;
    remove_dead_owners_pearls(&mut region, true);
    assert!(region.players[&id].ender_pearls.is_empty());
    assert!(region.entities.get(uid).unwrap().removed);
}

#[test]
fn the_rule_off_keeps_a_dead_owners_pearl() {
    let (mut region, id) = region_with_pearl_player();
    let shot = pearl_shot(id);
    let uid = {
        let (mut level, ticks) = region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        world.spawn_egg_shot(&shot)
    };
    register(&mut region, id, uid);
    region.players.get_mut(&id).unwrap().dead = true;
    remove_dead_owners_pearls(&mut region, false);
    assert_eq!(region.players[&id].ender_pearls.len(), 1);
    assert!(!region.entities.get(uid).unwrap().removed);
}

/// 下线：先刷新快照写进存档；再把世界里的珍珠标记移除，和
/// `PlayerList.remove` 一样。
#[test]
fn leaving_saves_the_pearl_and_removes_it_from_the_world() {
    let (mut region, id) = region_with_pearl_player();
    let shot = pearl_shot(id);
    let uid = {
        let (mut level, ticks) = region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        world.spawn_egg_shot(&shot)
    };
    register(&mut region, id, uid);
    refresh_snapshot(&mut region, id);
    let saved = region.players[&id].saved_ender_pearls.clone();
    assert_eq!(saved.len(), 1);
    remove_on_leave(&mut region, id);
    assert!(region.players[&id].ender_pearls.is_empty());
    assert!(region.entities.get(uid).unwrap().removed);
    // 存档快照保留，重新上线时据此把珍珠放回世界。
    assert_eq!(region.players[&id].saved_ender_pearls, saved);
}

#[test]
fn the_landing_teleport_moves_the_player_and_resets_fall_distance() {
    let (mut region, id) = region_with_pearl_player();
    let landing = Vec3::new(12.5, 70.0, -4.5);
    region.players.get_mut(&id).unwrap().motion.fall_distance = 9.0;
    teleport(&mut region, id.0, landing);
    let player = &region.players[&id];
    assert_eq!(player.position(), landing);
    assert_eq!(player.motion.fall_distance, 0.0);
}

/// 珍珠票让远处的区块保持加载、珍珠继续 tick，最后落地把玩家传过去。
///
/// The pearl ticket keeps a far chunk loaded, the pearl keeps ticking, and the
/// landing teleports the player there.
#[test]
fn a_far_pearl_ticket_keeps_its_chunk_loaded_until_it_lands() {
    use crate::world::tests::Harness;
    use mc_bamboo_blocks::entity::thrown_egg::EggShot;
    use mc_bamboo_world::ticket::TicketType;

    let mut server = Harness::new_loaded_with_drive(2, crate::island::IslandDrive::Lockstep);
    let client = server.join_and_settle("Pearl");
    let far = ChunkPos::new(6, 0);
    let landing = Vec3::new(104.5, -50.0, 8.5);
    let pearl = {
        let region = server.region();
        let shot = EggShot {
            position: landing,
            direction: Vec3::new(0.0, 0.0, 0.0),
            power: 1.0,
            uncertainty: 0.0,
            projectile: ThrowableKind::EnderPearl,
            source_movement: None,
            owner: Some(client.id.0),
            owner_uuid: Some(*client.uuid.as_bytes()),
            item: ItemStack::new(items::ENDER_PEARL, 1),
        };
        let uid = {
            let (mut level, ticks) = region.block_level();
            let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
            world.spawn_egg_shot(&shot)
        };
        register(region, client.id, uid);
        uid
    };
    assert!(!server.world_region().chunks.is_loaded(far));
    let mut loaded_while_flying = false;
    let mut fell = false;
    let mut last_y = landing.y;
    for _ in 0..80 {
        server.tick();
        let region = server.world_region();
        if region.chunks.is_loaded(far) && region.entities.public_at(pearl).is_some() {
            loaded_while_flying = true;
            let y = region.entities.public_at(pearl).unwrap().position.y;
            // 珍珠真的在 tick：被加载之后 y 开始下降。
            if y < last_y {
                fell = true;
            }
            last_y = y;
        }
        if region
            .entities
            .public_at(pearl)
            .is_none_or(|public| public.removed)
        {
            break;
        }
    }
    assert!(
        server
            .tickets()
            .tickets_at(far)
            .iter()
            .any(|ticket| ticket.ty == TicketType::EnderPearl),
        "飞行中的珍珠挂着 ender pearl 票"
    );
    assert!(loaded_while_flying, "珍珠票让远处区块保持加载");
    assert!(fell, "珍珠在被加载的区块里继续 tick");
    // 落地后主人被传送到落点附近。
    let position = server.player(&client).position();
    assert!(
        (position.x - 104.5).abs() < 1.0 && (position.z - 8.5).abs() < 1.0,
        "玩家落在远处的珍珠落点，实际 {position:?}"
    );
}
