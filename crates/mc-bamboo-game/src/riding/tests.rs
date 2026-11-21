//! 骑乘的单元测试：附着点、校验和货物点。
//!
//! Unit tests for riding: attachment points, validation and the good positions.

use mc_bamboo_blocks::entity::mob::{Mob, MobPublic, MobType};
use mc_bamboo_core::Vec3;

use super::{contains_invalid_values, passenger_offset, wrap_degrees};

/// 载具把乘客从名单里摘掉（`ejectPassengers`）之后，下一刻的 `rideTick` 把两边的关系
/// 都清掉：玩家不再是乘客，载具的名单里也没有他。
///
/// Once the vehicle drops the rider from its passenger list
/// (`ejectPassengers`), the next `rideTick` clears both sides: the player stops
/// being a passenger and the list stays empty.
#[test]
fn a_rider_dropped_from_the_passenger_list_is_dismounted() {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{inbound::PlayerId, player::Player, region::Region};

    let mut region = Region::new(2, 20, false);
    let id = PlayerId(1);
    let player = Player::new(
        id,
        Uuid::from_u128(1),
        "Rider".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    region.insert_player(player);
    let horse = region.entities.spawn(Entity::by_mob_type(
        MobType::Horse,
        Vec3::new(0.5, 64.0, 0.5),
    ));
    super::mount(&mut region, id, horse);
    assert_eq!(
        region.players.get(&id).expect("player").riding.vehicle,
        Some(horse)
    );

    // 模拟生物阶段的甩人：这一阶段只能清载具的名单。
    // Simulate ejection in the entity phase, which can only clear the vehicle's list.
    region.entities.with_mut(horse, |entity| {
        entity.with_mob(|_, mob| mob.passengers.clear())
    });
    super::tick(&mut region);

    assert_eq!(
        region.players.get(&id).expect("player").riding.vehicle,
        None
    );
    let passengers = region
        .entities
        .public_at(horse)
        .and_then(|public| match &public.kind {
            mc_bamboo_blocks::entity::KindPublic::Mob(mob) => Some(mob.passengers.clone()),
            _ => None,
        })
        .expect("horse");
    assert!(passengers.is_empty());
}

/// 官方 `Entity.removePassenger` / `baseTick`：下车后 60 刻内不能再上，冷却每刻减一。
///
/// Vanilla `Entity.removePassenger` / `baseTick`: no mounting for 60 ticks after a
/// dismount, and the cooldown drops by one each tick.
#[test]
fn dismounting_starts_the_boarding_cooldown() {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{inbound::PlayerId, player::Player, region::Region};

    let mut region = Region::new(2, 20, false);
    let id = PlayerId(1);
    let player = Player::new(
        id,
        Uuid::from_u128(1),
        "Rider".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    region.insert_player(player);
    let horse = region.entities.spawn(Entity::by_mob_type(
        MobType::Horse,
        Vec3::new(0.5, 64.0, 0.5),
    ));
    super::mount(&mut region, id, horse);
    super::dismount(&mut region, id);
    assert_eq!(
        region
            .players
            .get(&id)
            .expect("player")
            .riding
            .boarding_cooldown,
        60
    );

    super::mount(&mut region, id, horse);
    assert_eq!(
        region.players.get(&id).expect("player").riding.vehicle,
        None,
        "the cooldown blocks the mount"
    );
    for _ in 0..60 {
        super::tick(&mut region);
    }
    assert_eq!(
        region
            .players
            .get(&id)
            .expect("player")
            .riding
            .boarding_cooldown,
        0
    );
    super::mount(&mut region, id, horse);
    assert_eq!(
        region.players.get(&id).expect("player").riding.vehicle,
        Some(horse)
    );
}

/// 官方 `startRiding`：已经骑着别的载具时先 `stopRiding`，旧载具的乘客名单也清空。
///
/// Vanilla `startRiding`: mounting another vehicle first dismounts the old one,
/// and the old vehicle's passenger list is cleared too.
#[test]
fn mounting_another_vehicle_dismounts_the_first() {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{inbound::PlayerId, player::Player, region::Region};

    let mut region = Region::new(2, 20, false);
    let id = PlayerId(1);
    let player = Player::new(
        id,
        Uuid::from_u128(1),
        "Rider".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    region.insert_player(player);
    let first = region.entities.spawn(Entity::by_mob_type(
        MobType::Horse,
        Vec3::new(0.5, 64.0, 0.5),
    ));
    let second = region.entities.spawn(Entity::by_mob_type(
        MobType::Horse,
        Vec3::new(2.5, 64.0, 0.5),
    ));
    super::mount(&mut region, id, first);
    super::mount(&mut region, id, second);

    assert_eq!(
        region.players.get(&id).expect("player").riding.vehicle,
        Some(second)
    );
    let first_passengers = region
        .entities
        .public_at(first)
        .and_then(|public| match &public.kind {
            mc_bamboo_blocks::entity::KindPublic::Mob(mob) => Some(mob.passengers.clone()),
            _ => None,
        })
        .expect("first horse");
    assert!(first_passengers.is_empty());
}

/// 官方 `canRide`：按着潜行键不能上马。
///
/// Vanilla `canRide`: holding sneak blocks mounting.
#[test]
fn sneaking_blocks_mounting() {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{inbound::PlayerId, player::Player, region::Region};

    let mut region = Region::new(2, 20, false);
    let id = PlayerId(1);
    let player = Player::new(
        id,
        Uuid::from_u128(1),
        "Rider".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    );
    region.insert_player(player);
    let horse = region.entities.spawn(Entity::by_mob_type(
        MobType::Horse,
        Vec3::new(0.5, 64.0, 0.5),
    ));
    region
        .players
        .get_mut(&id)
        .expect("player")
        .sync
        .shift_key_down = true;
    super::mount(&mut region, id, horse);
    assert_eq!(
        region.players.get(&id).expect("player").riding.vehicle,
        None
    );
}

/// 官方 `AbstractHorse.getDismountLocationForPassenger`：右脚一侧没地方站时换另一侧，
/// 两边都悬空就落在载具自己的位置。
///
/// Vanilla `AbstractHorse.getDismountLocationForPassenger`: try the right-hand
/// side, then the other side, and fall back to the vehicle's own position.
#[test]
fn a_dismount_lands_beside_the_horse_or_on_it() {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::blocks;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use mc_bamboo_world::store::ChunkStore;
    use uuid::Uuid;

    use crate::{inbound::PlayerId, player::Player};

    let player = || {
        Player::new(
            PlayerId(1),
            Uuid::from_u128(1),
            "Rider".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, -60.0, 0.5),
        )
    };
    // 超平坦的地面在 y=-61，顶面是 -60；马站在 (0.5, -60, 0.5)，朝向 0。
    //
    // The flat floor sits at y=-61 with its top at -60; the horse stands at
    // (0.5, -60, 0.5) facing 0.
    let chunks = ChunkStore::flat();
    let vehicle = Vec3::new(0.5, -60.0, 0.5);
    let spot =
        super::dismount::dismount_location(&chunks, &player(), vehicle, 1.396_484_4, 1.6, 0.0)
            .expect("a floor exists");
    assert!((spot.y + 60.0).abs() < 1.0e-9, "{spot:?}");
    assert!(spot.x < 0.0, "right hand side is -x at yaw 0: {spot:?}");

    // 主手一侧立一堵墙，落点换到另一侧。
    // A wall on the main-hand side moves the spot to the other side.
    let mut chunks = ChunkStore::flat();
    for y in -60..=-58 {
        chunks.set_block(BlockPos::new(-1, y, 0), blocks::STONE);
    }
    let spot =
        super::dismount::dismount_location(&chunks, &player(), vehicle, 1.396_484_4, 1.6, 0.0)
            .expect("a floor exists");
    assert!(spot.x > 0.5, "the other side is +x: {spot:?}");

    // 两边都没有地：官方返回 `this.position()`。
    // No floor on either side: vanilla returns `this.position()`.
    let air = Vec3::new(0.5, 100.0, 0.5);
    let spot = super::dismount::dismount_location(&chunks, &player(), air, 1.396_484_4, 1.6, 0.0)
        .expect("the fallback is always a position");
    assert_eq!(spot, air);
}

/// 官方 `EntityTypes` 的 `passengerAttachments`：马 1.44375，驴 1.1125，骡 1.2125，
/// 羊驼 (1.37, -0.3)。朝向 0 时偏移只在 y 和 z 上。
///
/// The vanilla `EntityTypes` `passengerAttachments`: horse 1.44375, donkey 1.1125,
/// mule 1.2125, llama (1.37, -0.3). At yaw 0 the offset only has y and z.
#[test]
fn passenger_offsets_match_the_attachment_points() {
    let public = |kind| {
        let mob = Mob::new(kind);
        MobPublic::of(&mob, 0)
    };
    let horse = passenger_offset(&public(MobType::Horse), 0.0);
    assert!((horse.y - 1.443_75).abs() < 1.0e-6);
    assert!(horse.z.abs() < 1.0e-6);

    let donkey = passenger_offset(&public(MobType::Donkey), 0.0);
    assert!((donkey.y - 1.112_5).abs() < 1.0e-6);
    assert!(donkey.z.abs() < 1.0e-6);

    let mule = passenger_offset(&public(MobType::Mule), 0.0);
    assert!((mule.y - 1.212_5).abs() < 1.0e-6);

    let llama = passenger_offset(&public(MobType::Llama), 0.0);
    assert!((llama.y - 1.37).abs() < 1.0e-6);
    assert!((llama.z + 0.3).abs() < 1.0e-6);
}

/// 朝向 90 度时，羊驼的向后偏移转到 x 轴上；官方的 `Vec3.yRot(-yRot)` 也是这个方向。
///
/// At yaw 90 the llama's backwards offset lands on x; vanilla `Vec3.yRot(-yRot)`
/// turns the same way.
#[test]
fn passenger_offset_rotates_with_the_vehicle() {
    let mob = Mob::new(MobType::Llama);
    let public = MobPublic::of(&mob, 0);
    let offset = passenger_offset(&public, 90.0);
    assert!((offset.x - 0.3).abs() < 1.0e-6);
    assert!(offset.z.abs() < 1.0e-6);
}

/// 官方的 `containsInvalidValues`：坐标是 NaN 或朝向不是有限值就断开。
///
/// Vanilla `containsInvalidValues`: NaN coordinates or non-finite angles disconnect.
#[test]
fn invalid_vehicle_positions_are_rejected() {
    assert!(!contains_invalid_values(Vec3::new(1.0, 2.0, 3.0), 0.0, 0.0));
    assert!(contains_invalid_values(
        Vec3::new(f64::NAN, 2.0, 3.0),
        0.0,
        0.0
    ));
    assert!(contains_invalid_values(
        Vec3::new(1.0, 2.0, f64::INFINITY),
        0.0,
        0.0
    ));
    assert!(contains_invalid_values(
        Vec3::new(1.0, 2.0, 3.0),
        f32::NAN,
        0.0
    ));
}

/// `Mth.wrapDegrees`：把角度折回 -180 到 180。
///
/// `Mth.wrapDegrees`: fold an angle back into -180 to 180.
#[test]
fn the_vehicle_rotation_is_wrapped() {
    assert_eq!(wrap_degrees(370.0), 10.0);
    assert_eq!(wrap_degrees(-190.0), 170.0);
}

/// `OPEN_INVENTORY` 骑在驯服的马科上打开坐骑物品栏：马的列数是 0，实体 ID 是 int。
///
/// `OPEN_INVENTORY` on a tamed equine opens its inventory: a horse has zero
/// columns and the entity id is an int.
#[test]
fn the_inventory_key_opens_a_tamed_mount() {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_protocol::packet::play::PlayerCommandKind;

    use crate::world::tests::{Harness, frames_with_id};

    let mut server = Harness::new(2);
    let client = server.join("Rider");
    server.tick();
    let _ = client.take_frames();
    let at = server.region().players[&client.id].position();
    let mut horse = Entity::by_mob_type(MobType::Horse, at);
    horse.with_mob(|_, mob| {
        mob.equine.as_mut().expect("horse state").tame = true;
    });
    let uid = server.spawn_entity(horse);
    super::mount(server.region(), client.id, uid);
    crate::entity_sync::on_player_command(
        server.region(),
        client.id,
        PlayerCommandKind::OpenInventory,
        0,
    );
    server.region().links.flush();
    let frames = client.take_frames();
    let opened = frames_with_id(&frames, 42);
    assert_eq!(opened.len(), 1, "one mount_screen_open");
    let container_id = server.region().menus[&client.id].container_id;
    let mut expected = vec![container_id as u8, 0];
    expected.extend_from_slice(&uid.to_be_bytes());
    assert_eq!(opened[0].body, expected);
}

/// 没驯服的马不响应物品栏键：官方 `openCustomInventoryScreen` 要求 `isTamed()`。
///
/// An untamed horse ignores the inventory key; vanilla
/// `openCustomInventoryScreen` requires `isTamed()`.
#[test]
fn the_inventory_key_ignores_an_untamed_mount() {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_protocol::packet::play::PlayerCommandKind;

    use crate::world::tests::{Harness, frames_with_id};

    let mut server = Harness::new(2);
    let client = server.join("Rider");
    server.tick();
    let _ = client.take_frames();
    let at = server.region().players[&client.id].position();
    let horse = Entity::by_mob_type(MobType::Horse, at);
    let uid = server.spawn_entity(horse);
    super::mount(server.region(), client.id, uid);
    crate::entity_sync::on_player_command(
        server.region(),
        client.id,
        PlayerCommandKind::OpenInventory,
        0,
    );
    server.region().links.flush();
    assert!(frames_with_id(&client.take_frames(), 42).is_empty());
    assert!(!server.region().menus.contains_key(&client.id));
}
