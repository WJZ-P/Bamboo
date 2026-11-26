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
/// Vanilla `AbstractHorse.getDismountLocationForPassenger`: try the main-hand
/// side (the default player is right-handed), then the other side, and fall
/// back to the vehicle's own position.
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
    let border = mc_bamboo_blocks::WorldBorder::default();
    let chunks = ChunkStore::flat();
    let vehicle = Vec3::new(0.5, -60.0, 0.5);
    let spot = super::dismount::dismount_location(
        &chunks,
        &border,
        &player(),
        vehicle,
        1.396_484_4,
        1.6,
        0.0,
    )
    .expect("a floor exists");
    assert!((spot.y + 60.0).abs() < 1.0e-9, "{spot:?}");
    assert!(spot.x < 0.0, "right hand side is -x at yaw 0: {spot:?}");

    // 主手一侧超出世界边界时，落点换到边界内一侧（`DismountHelper.canDismountTo`）。
    // When the main-hand side falls outside the world border, fall back to the
    // inside side (`DismountHelper.canDismountTo`).
    let mut tight = mc_bamboo_blocks::WorldBorder::default();
    tight.set_center(2.0, 0.5);
    tight.set_size(4.0);
    let spot = super::dismount::dismount_location(
        &chunks,
        &tight,
        &player(),
        vehicle,
        1.396_484_4,
        1.6,
        0.0,
    )
    .expect("the inside side is within the border");
    assert!(spot.x > 0.5, "border pushes dismount to +x: {spot:?}");

    // 主手一侧立一堵墙，落点换到另一侧。
    // A wall on the main-hand side moves the spot to the other side.
    let mut chunks = ChunkStore::flat();
    for y in -60..=-58 {
        chunks.set_block(BlockPos::new(-1, y, 0), blocks::STONE);
    }
    let spot = super::dismount::dismount_location(
        &chunks,
        &border,
        &player(),
        vehicle,
        1.396_484_4,
        1.6,
        0.0,
    )
    .expect("a floor exists");
    assert!(spot.x > 0.5, "the other side is +x: {spot:?}");

    // 两边都没有地：官方返回 `this.position()`。
    // No floor on either side: vanilla returns `this.position()`.
    let air = Vec3::new(0.5, 100.0, 0.5);
    let spot =
        super::dismount::dismount_location(&chunks, &border, &player(), air, 1.396_484_4, 1.6, 0.0)
            .expect("the fallback is always a position");
    assert_eq!(spot, air);
}

/// 两侧都能站时，先被尝试的是主手一侧：右手玩家落在 -x，左手玩家落在 +x。
/// 官方 `getDismountLocationForPassenger` 的主手方向是
/// `yRot + (mainArm == RIGHT ? 90 : -90)`，另一手取反。
///
/// When both sides are valid the main-hand side is tried first: a right-handed
/// rider lands on -x, a left-handed one on +x. Vanilla
/// `getDismountLocationForPassenger` uses `yRot + (mainArm == RIGHT ? 90 : -90)`
/// for the main-hand direction and the opposite for the other hand.
#[test]
fn dismount_side_follows_the_players_main_arm() {
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode, HumanoidArm};
    use mc_bamboo_world::store::ChunkStore;
    use uuid::Uuid;

    use crate::{inbound::PlayerId, player::Player};

    let rider = |main_hand| {
        let info = ClientInformation {
            main_hand,
            ..ClientInformation::default()
        };
        Player::new(
            PlayerId(1),
            Uuid::from_u128(1),
            "Rider".to_owned(),
            GameMode::Survival,
            info,
            Vec3::new(0.5, -60.0, 0.5),
        )
    };
    let border = mc_bamboo_blocks::WorldBorder::default();
    let chunks = ChunkStore::flat();
    let vehicle = Vec3::new(0.5, -60.0, 0.5);
    let side = |main_hand| {
        super::dismount::dismount_location(
            &chunks,
            &border,
            &rider(main_hand),
            vehicle,
            1.396_484_4,
            1.6,
            0.0,
        )
        .expect("a floor exists")
        .x
    };
    assert!(
        side(HumanoidArm::Right) < 0.0,
        "right hand first tries -x at yaw 0"
    );
    assert!(
        side(HumanoidArm::Left) > 0.5,
        "left hand first tries +x at yaw 0"
    );
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
        MobPublic::of(&mob, false)
    };
    let horse = passenger_offset(&public(MobType::Horse), 0.0, 0);
    assert!((horse.y - 1.443_75).abs() < 1.0e-6);
    assert!(horse.z.abs() < 1.0e-6);

    let donkey = passenger_offset(&public(MobType::Donkey), 0.0, 0);
    assert!((donkey.y - 1.112_5).abs() < 1.0e-6);
    assert!(donkey.z.abs() < 1.0e-6);

    let mule = passenger_offset(&public(MobType::Mule), 0.0, 0);
    assert!((mule.y - 1.212_5).abs() < 1.0e-6);

    let llama = passenger_offset(&public(MobType::Llama), 0.0, 0);
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
    let public = MobPublic::of(&mob, false);
    let offset = passenger_offset(&public, 90.0, 0);
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

/// 客户端报的载具位置要过官方 `handleMoveVehicle` 的方块校验：平地上的位移照收。
///
/// The client's vehicle position goes through vanilla `handleMoveVehicle`'s block
/// check: a move over open ground is accepted.
#[test]
fn a_vehicle_move_over_open_ground_is_accepted() {
    let (mut server, client, uid) = staged_horse();
    let at = server
        .region()
        .entities
        .public_at(uid)
        .expect("horse")
        .position;
    let target = Vec3::new(at.x + 0.5, at.y, at.z);
    super::on_move_vehicle(server.region(), client.id, target, 0.0, 0.0, true);
    let public = server.region().entities.public_at(uid).expect("horse");
    assert_eq!(public.position, target);
}

/// 穿过墙的位移被拉回：官方的 `vehicle.move` 撞在墙上，和包里的位置差超过 0.25 格，
/// 旧位置又没卡在方块里，就 `absSnapTo` 回原位并发一包载具位置。
///
/// A move through a wall is rejected: vanilla `vehicle.move` stops at the wall, the
/// gap to the packet position is over 0.25 blocks and the old spot is clear, so it
/// snaps back and sends the vehicle position.
#[test]
fn a_vehicle_move_through_a_wall_is_rejected() {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::blocks;
    use mc_bamboo_protocol::packet::{ClientboundPacket, play::MoveVehicle};

    use crate::world::tests::frames_with_id;

    let (mut server, client, uid) = staged_horse();
    let at = server
        .region()
        .entities
        .public_at(uid)
        .expect("horse")
        .position;
    let wall = (at.x + 1.5) as i32;
    let base = at.y as i32;
    for y in base..base + 3 {
        for z in (at.z as i32 - 3)..(at.z as i32 + 4) {
            server.set_block(BlockPos::new(wall, y, z), blocks::STONE);
        }
    }
    let target = Vec3::new(at.x + 1.5, at.y, at.z);
    super::on_move_vehicle(server.region(), client.id, target, 30.0, 15.0, true);
    let public = server.region().entities.public_at(uid).expect("horse");
    assert_eq!(public.position, at, "the wall keeps the vehicle back");
    // 朝向仍按包里的设，位置包也带着这个朝向。
    assert_eq!(public.yaw, 30.0);
    server.region().links.flush();
    let frames = client.take_frames();
    let snaps = frames_with_id(&frames, MoveVehicle::ID);
    assert_eq!(snaps.len(), 1, "one position packet");
}

/// 造一个已上马的测试场景：玩家在出生点，上鞍的马就在他脚下，并且跑过一刻
/// `riding::tick`（`last_vehicle` 从这一刻起才算数）。
///
/// Build a mounted scene: the player at spawn with a saddled horse under them, after
/// one `riding::tick` so `last_vehicle` is recorded.
fn staged_horse() -> (
    crate::world::tests::Harness,
    crate::world::tests::Client,
    i32,
) {
    staged_horse_with_saddle(true)
}

/// 同 [`staged_horse`]，`saddle` 决定马有没有鞍（没鞍的载具能骑，但控制权不在客户端）。
///
/// Like [`staged_horse`]; `saddle` decides whether the horse has one (an unsaddled
/// mount can still be ridden, but the client does not control it).
fn staged_horse_with_saddle(
    saddle: bool,
) -> (
    crate::world::tests::Harness,
    crate::world::tests::Client,
    i32,
) {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_data::items;
    use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

    use crate::world::tests::Harness;

    let mut server = Harness::new(2);
    let client = server.join("Rider");
    server.tick();
    let _ = client.take_frames();
    let at = server.region().players[&client.id].position();
    let mut horse = Entity::by_mob_type(MobType::Horse, at);
    horse.with_mob(|_, mob| {
        if saddle {
            mob.equipment
                .set(EquipmentSlot::Saddle, ItemStack::new(items::SADDLE, 1));
        }
    });
    let uid = server.spawn_entity(horse);
    super::mount(server.region(), client.id, uid);
    server.tick();
    let _ = client.take_frames();
    (server, client, uid)
}

/// 载具已经卡在方块里时，撞墙的位移也不拉回：官方 `fail && level.noCollision(vehicle, oldAABB)`
/// 的前一半是「旧位置没卡住」，卡住时放行，否则永远出不来。
///
/// A vehicle already stuck inside blocks is not snapped back on a blocked move: the
/// first half of vanilla `fail && level.noCollision(vehicle, oldAABB)` is "the old
/// spot is clear", and letting a stuck vehicle through is how it escapes.
#[test]
fn a_vehicle_stuck_in_blocks_may_move_out() {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::blocks;

    let (mut server, client, uid) = staged_horse();
    let at = server
        .region()
        .entities
        .public_at(uid)
        .expect("horse")
        .position;
    let here = BlockPos::new(at.x as i32, at.y as i32, at.z as i32);
    server.set_block(here, blocks::STONE);
    let wall = (at.x + 1.5) as i32;
    let base = at.y as i32;
    for y in base..base + 3 {
        for z in (at.z as i32 - 3)..(at.z as i32 + 4) {
            server.set_block(BlockPos::new(wall, y, z), blocks::STONE);
        }
    }
    let target = Vec3::new(at.x + 1.5, at.y, at.z);
    super::on_move_vehicle(server.region(), client.id, target, 0.0, 0.0, true);
    let public = server.region().entities.public_at(uid).expect("horse");
    assert_eq!(public.position, target);
}

/// 没上鞍的马也能骑，但客户端不控制它：官方 `getControllingPassenger` 要求有鞍，
/// `tickPlayer` 因此不记 `lastVehicle`，`move_vehicle` 报上来的位置一律不收。
///
/// An unsaddled horse can be ridden but the client does not control it: vanilla
/// `getControllingPassenger` needs a saddle, so `tickPlayer` records no
/// `lastVehicle` and no `move_vehicle` position is accepted.
#[test]
fn an_unsaddled_mount_is_not_client_controlled() {
    let (mut server, client, uid) = staged_horse_with_saddle(false);
    assert_eq!(
        server.region().players[&client.id].riding.last_vehicle,
        None
    );
    let at = server
        .region()
        .entities
        .public_at(uid)
        .expect("horse")
        .position;
    super::on_move_vehicle(
        server.region(),
        client.id,
        Vec3::new(at.x + 0.5, at.y, at.z),
        0.0,
        0.0,
        true,
    );
    let public = server.region().entities.public_at(uid).expect("horse");
    assert_eq!(public.position, at, "the client cannot move it");
}

/// 不控制的载具被报了一个一格以外的位置时，官方立刻把载具位置发回去
/// （`tickCount - vehiclePositionLastResetAt > 20 || distanceToSqr > 1`）。
///
/// A non-controlled vehicle reported more than one block away gets its position
/// sent back at once (vanilla: `... > 20 || distanceToSqr > 1`).
#[test]
fn a_foreign_vehicle_position_far_away_is_reset_at_once() {
    use mc_bamboo_protocol::packet::{ClientboundPacket, play::MoveVehicle};

    use crate::world::tests::frames_with_id;

    let (mut server, client, uid) = staged_horse_with_saddle(false);
    let at = server
        .region()
        .entities
        .public_at(uid)
        .expect("horse")
        .position;
    super::on_move_vehicle(
        server.region(),
        client.id,
        Vec3::new(at.x + 2.0, at.y, at.z),
        0.0,
        0.0,
        true,
    );
    server.region().links.flush();
    let frames = client.take_frames();
    assert_eq!(frames_with_id(&frames, MoveVehicle::ID).len(), 1);
    // 紧接着报一个一格以内的位置：离上次重置不到 20 刻，不发。
    super::on_move_vehicle(
        server.region(),
        client.id,
        Vec3::new(at.x + 0.5, at.y, at.z),
        0.0,
        0.0,
        true,
    );
    server.region().links.flush();
    assert!(frames_with_id(&client.take_frames(), MoveVehicle::ID).is_empty());
}

/// 扬蹄时 `AbstractHorse.getPassengerAttachmentPoint` 按 `standAnimO` 把骑手抬高并后移；
/// 羊驼覆写后不叠加扬蹄偏移。
///
/// Rearing shifts the rider up and back by `standAnimO` on horses, donkeys and
/// mules; llamas override the attachment point without a rearing offset.
#[test]
fn rearing_shifts_the_passenger_offset_for_horses_only() {
    let mut horse = Mob::new(MobType::Horse);
    horse.equine.as_mut().expect("horse").stand_anim_o = 1.0;
    let public = MobPublic::of(&horse, false);
    let offset = super::passenger_offset(&public, 0.0, 0);
    assert!((offset.x - 0.0).abs() < 1e-9);
    assert!((offset.y - (1.443_75 + 0.15)).abs() < 1e-6);
    assert!((offset.z - (-0.7)).abs() < 1e-6);

    let rotated = super::passenger_offset(&public, 90.0, 0);
    assert!((rotated.x - 0.7).abs() < 1e-4);
    assert!((rotated.y - (1.443_75 + 0.15)).abs() < 1e-6);
    assert!(rotated.z.abs() < 1e-4);

    let mut llama = Mob::new(MobType::Llama);
    llama.equine.as_mut().expect("llama").stand_anim_o = 1.0;
    let llama_public = MobPublic::of(&llama, false);
    let llama_offset = super::passenger_offset(&llama_public, 0.0, 0);
    assert!((llama_offset.y - 1.37).abs() < 1e-6);
    assert!((llama_offset.z - (-0.3)).abs() < 1e-6);
}

/// 一名玩家和一只生物共用同一份乘客名单：服务端把玩家插到生物前面，
/// `getFirstPassenger` / `getControllingPassenger` 都从这份名单算，
/// 玩家侧的 `Riding.vehicle` 与名单双向一致。
#[test]
fn players_and_mobs_share_one_ordered_passenger_list() {
    use mc_bamboo_blocks::entity::{Entity, FIRST_ENTITY_ID};
    use mc_bamboo_protocol::packet::play::{ClientInformation, EquipmentSlot, GameMode};
    use uuid::Uuid;

    use crate::{inbound::PlayerId, player::Player, region::Region};

    let mut region = Region::new(2, 20, false);
    let id = PlayerId(7);
    region.insert_player(Player::new(
        id,
        Uuid::from_u128(1),
        "Rider".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    ));
    let horse = region.entities.spawn(Entity::by_mob_type(
        MobType::Horse,
        Vec3::new(0.5, 64.0, 0.5),
    ));
    // 先上一名生物乘客，再让玩家上车：官方 `Entity.addPassenger` 把玩家插到 0 号。
    let mob_rider = region
        .entities
        .spawn(Entity::cow(Vec3::new(0.5, 64.0, 0.5)));
    region.entities.with_mut(horse, |entity| {
        entity.with_mob(|_, mob| {
            mc_bamboo_blocks::entity::riding::add_passenger(&mut mob.passengers, mob_rider, false);
        })
    });
    region.entities.with_mut(mob_rider, |entity| {
        entity.vehicle = Some(horse);
    });
    // 改装 `mount` 的一人限制只在「名单非空且没有玩家」时放行：这里名单里已有
    // 一名生物，按官方 `canAddPassenger` 本应拒绝；用统一的 `add_passenger`
    // 直接构造混合名单，验证读取侧而不是交互策略。
    region.entities.with_mut(horse, |entity| {
        entity.with_mob(|_, mob| {
            mc_bamboo_blocks::entity::riding::add_passenger(&mut mob.passengers, id.0, true);
        })
    });
    if let Some(player) = region.players.get_mut(&id) {
        player.riding.vehicle = Some(horse);
    }
    let list = super::passengers(&region, horse);
    assert_eq!(list, vec![id.0, mob_rider]);
    assert!(id.0 < FIRST_ENTITY_ID);
    // `Mob.getFirstPassenger` / `getControllingPassenger` 都读这一份名单。
    let (first, controlling) = region
        .entities
        .get(horse)
        .map(|entity| {
            let mob = entity.kind.mob().expect("horse is a mob");
            (mob.first_passenger(), mob.controlling_passenger())
        })
        .unwrap();
    assert_eq!(first, Some(id.0));
    assert_eq!(controlling, None, "no saddle yet");
    assert!(super::check_consistency(&region).is_empty());
    assert_eq!(super::player_vehicles(&region).get(&id), Some(&horse));
    assert_eq!(super::player_passenger_count(&region, horse), 1);
    // 上鞍之后控制乘客就是名单里的第一个（这名玩家）。
    region.entities.with_mut(horse, |entity| {
        entity.with_mob(|_, mob| {
            mob.equipment.set(
                EquipmentSlot::Saddle,
                mc_bamboo_protocol::item::ItemStack::new(mc_bamboo_data::items::SADDLE, 1),
            );
        })
    });
    let controlling = region
        .entities
        .get(horse)
        .and_then(|entity| entity.kind.mob().map(|mob| mob.controlling_passenger()))
        .unwrap();
    assert_eq!(controlling, Some(id.0));
}

/// 双向一致检查能抓到「玩家侧说在骑、名单里没有他」和「名单里有、玩家侧没有」。
#[test]
fn consistency_check_reports_both_sides_of_a_broken_link() {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{inbound::PlayerId, player::Player, region::Region};

    let mut region = Region::new(2, 20, false);
    let id = PlayerId(3);
    region.insert_player(Player::new(
        id,
        Uuid::from_u128(2),
        "Rider".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    ));
    let horse = region.entities.spawn(Entity::by_mob_type(
        MobType::Horse,
        Vec3::new(0.5, 64.0, 0.5),
    ));
    super::mount(&mut region, id, horse);
    assert!(super::check_consistency(&region).is_empty());
    region.players.get_mut(&id).unwrap().riding.vehicle = None;
    assert_eq!(super::check_consistency(&region).len(), 1);
    region.players.get_mut(&id).unwrap().riding.vehicle = Some(horse);
    region.entities.with_mut(horse, |entity| {
        entity.with_mob(|_, mob| mob.passengers.clear())
    });
    assert_eq!(super::check_consistency(&region).len(), 1);
}

/// 载具死亡 / 被移除时，玩家和生物乘客走同一条 `removePassenger`：方块层把
/// 玩家记进待下车队列，游戏层的 `rideTick` 用 `getDismountLocationForPassenger`
/// 落位；生物乘客停在原地。同步阶段只发一份含两份乘客的 `set_passengers`。
#[test]
fn removing_the_vehicle_dismounts_player_and_mob_together() {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{entity_tracking::Snapshot, inbound::PlayerId, player::Player, region::Region};

    let mut region = Region::new(2, 20, false);
    let id = PlayerId(5);
    region.insert_player(Player::new(
        id,
        Uuid::from_u128(3),
        "Rider".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    ));
    let horse = region.entities.spawn(Entity::by_mob_type(
        MobType::Horse,
        Vec3::new(0.5, 64.0, 0.5),
    ));
    let mob_rider = region
        .entities
        .spawn(Entity::cow(Vec3::new(0.5, 64.0, 0.5)));
    super::mount(&mut region, id, horse);
    region.entities.with_mut(horse, |entity| {
        entity.with_mob(|_, mob| {
            mc_bamboo_blocks::entity::riding::add_passenger(&mut mob.passengers, mob_rider, false);
        })
    });
    region.entities.with_mut(mob_rider, |entity| {
        entity.vehicle = Some(horse);
    });
    // 同步层的快照直接读这唯一一份名单，配对时按它发一次 `set_passengers`。
    let snapshot = Snapshot::of(region.entities.public_at(horse).unwrap());
    assert_eq!(snapshot.passengers, vec![id.0, mob_rider]);
    let index = crate::entity_tracking::PassengerIndex::of(std::slice::from_ref(&snapshot));
    assert_eq!(index.list(horse), Some(&[id.0, mob_rider][..]));

    // `setRemoved` 把生物乘客当场摘掉，玩家只进队列。
    region.entities.remove(horse);
    assert_eq!(region.entities.take_ejected_players(), vec![id.0]);
    assert_eq!(region.entities.public_at(mob_rider).unwrap().vehicle, None);
    let position = region.entities.public_at(mob_rider).unwrap().position;
    super::tick(&mut region);
    assert_eq!(region.players[&id].riding.vehicle, None);
    assert_eq!(
        region.entities.public_at(mob_rider).unwrap().position,
        position
    );
    assert!(super::check_consistency(&region).is_empty());
    // 载具离表之后没有新的 `set_passengers` 包：移除包由实体跟踪发。
}

/// `ServerPlayer.saveParentVehicle` / `loadAndSpawnParentVehicle`：骑着带生物乘客
/// 的马下线时，存档 `RootVehicle` 带上 `Attach`、整棵 `Passengers` 和玩家重新
/// 骑上的那一份关系。两名玩家乘客时官方不写 `RootVehicle`。
#[test]
fn root_vehicle_save_and_load_round_trips_the_whole_passenger_tree() {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_nbt::Tag;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{inbound::PlayerId, player::Player, region::Region};

    let mut region = Region::new(2, 20, false);
    let id = PlayerId(9);
    region.insert_player(Player::new(
        id,
        Uuid::from_u128(4),
        "Rider".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    ));
    let horse = region.entities.spawn(Entity::by_mob_type(
        MobType::Horse,
        Vec3::new(2.5, 64.0, -3.5),
    ));
    let mob_rider = region
        .entities
        .spawn(Entity::cow(Vec3::new(2.5, 65.0, -3.5)));
    super::mount(&mut region, id, horse);
    region.entities.with_mut(horse, |entity| {
        entity.with_mob(|_, mob| {
            mc_bamboo_blocks::entity::riding::add_passenger(&mut mob.passengers, mob_rider, false);
        })
    });
    region.entities.with_mut(mob_rider, |entity| {
        entity.vehicle = Some(horse);
    });
    assert_eq!(super::player_passenger_count(&region, horse), 1);

    let tag = super::root_vehicle_save(&region, id).expect("riding player saves RootVehicle");
    let Tag::Compound(entries) = &tag else {
        panic!("RootVehicle is a compound");
    };
    assert!(entries.iter().any(|(key, _)| key == "Attach"));
    let entity_tag = entries
        .iter()
        .find(|(key, _)| key == "Entity")
        .map(|(_, value)| value)
        .expect("Entity child");
    let text = entity_tag.to_snbt();
    assert!(text.contains("Passengers"), "{text}");
    assert!(text.contains("minecraft:cow"), "{text}");

    // 下线：移除整棵树，再按存档放回一个新世界（新实体号）。
    for uid in [horse, mob_rider] {
        region.entities.remove(uid);
    }
    region.players.get_mut(&id).unwrap().riding.vehicle = None;
    super::load_root_vehicle(&mut region, id, &tag);
    let vehicle = region.players[&id].riding.vehicle.expect("remounted");
    assert_ne!(vehicle, horse, "read-back entities get fresh ids");
    assert_eq!(super::passengers(&region, vehicle).first(), Some(&id.0));
    assert!(super::check_consistency(&region).is_empty());
    let nested = super::passengers(&region, vehicle)
        .iter()
        .copied()
        .find(|&p| p >= mc_bamboo_blocks::entity::FIRST_ENTITY_ID)
        .expect("the mob rider came back");
    assert_eq!(
        region.entities.public_at(nested).unwrap().vehicle,
        Some(vehicle)
    );

    // 没在骑东西时不写 `RootVehicle`。
    super::stop_riding(&mut region, id);
    assert_eq!(super::root_vehicle_save(&region, id), None);
    // 两名玩家乘客时也不写：玩家档只接管「恰好一名玩家」的载具。
    let other = PlayerId(10);
    region.insert_player(Player::new(
        other,
        Uuid::from_u128(5),
        "Other".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    ));
    // 直接补上第二位玩家（交互层的一人限制不在这里测）。
    region.players.get_mut(&id).unwrap().riding.vehicle = Some(vehicle);
    region.entities.with_mut(vehicle, |entity| {
        entity.with_mob(|_, mob| {
            mc_bamboo_blocks::entity::riding::add_passenger(&mut mob.passengers, id.0, true);
            mc_bamboo_blocks::entity::riding::add_passenger(&mut mob.passengers, other.0, true);
        })
    });
    region.players.get_mut(&other).unwrap().riding.vehicle = Some(vehicle);
    assert_eq!(super::player_passenger_count(&region, vehicle), 2);
    assert_eq!(super::root_vehicle_save(&region, id), None);
}

/// `RootVehicle` 坏数据（没有 `Entity`、未知实体类型、`Attach` 对不上）不能让
/// 进服路径 panic，也不能留下半棵没挂上玩家的乘客树。
#[test]
fn a_broken_root_vehicle_never_panics_or_leaves_orphans() {
    use mc_bamboo_nbt::Tag;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{inbound::PlayerId, player::Player, region::Region};

    let mut region = Region::new(2, 20, false);
    let id = PlayerId(11);
    region.insert_player(Player::new(
        id,
        Uuid::from_u128(6),
        "Rider".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::default(),
    ));
    let before = region.entities.ids().len();
    for tag in [
        Tag::String("oops".to_owned()),
        Tag::Compound(vec![("Attach".to_owned(), Tag::IntArray(vec![1, 2, 3, 4]))]),
        Tag::Compound(vec![(
            "Entity".to_owned(),
            Tag::Compound(vec![(
                "id".to_owned(),
                Tag::String("minecraft:not_an_entity".to_owned()),
            )]),
        )]),
    ] {
        super::load_root_vehicle(&mut region, id, &tag);
        assert_eq!(region.players[&id].riding.vehicle, None);
        assert_eq!(region.entities.ids().len(), before, "no orphans");
    }
}

/// 跨岛搬迁把整棵骑乘树（根 + 嵌套生物乘客）从原表搬到目标表：uid 不变，
/// 两边的 `vehicle` / `passengers` 链接原样保留，玩家侧的 `Riding.vehicle`
/// 在目标表里依然指向同一只载具。
#[test]
fn moving_a_riding_tree_between_tables_keeps_every_link() {
    use mc_bamboo_blocks::entity::{Entity, FIRST_ENTITY_ID};
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{inbound::PlayerId, player::Player, region::Region};

    let mut source = Region::new(2, 20, false);
    let id = PlayerId(6);
    source.insert_player(Player::new(
        id,
        Uuid::from_u128(1),
        "Rider".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(8.5, 64.0, 8.5),
    ));
    let horse = source.entities.spawn(Entity::by_mob_type(
        MobType::Horse,
        Vec3::new(8.5, 64.0, 8.5),
    ));
    let cow = source
        .entities
        .spawn(Entity::cow(Vec3::new(9.5, 65.0, 8.5)));
    super::mount(&mut source, id, horse);
    source.entities.with_mut(horse, |entity| {
        entity.with_mob(|_, mob| {
            mc_bamboo_blocks::entity::riding::add_passenger(&mut mob.passengers, cow, false);
        })
    });
    source.entities.with_mut(cow, |entity| {
        entity.vehicle = Some(horse);
    });
    assert!(cow >= FIRST_ENTITY_ID);

    let tree = super::riding_entities(&source, id);
    assert_eq!(tree, vec![horse, cow]);
    let chunks = super::riding_tree_chunks(&source, id);
    assert_eq!(chunks.len(), 1);

    let mut target = Region::new(2, 20, false);
    for uid in tree {
        let row = source
            .entities
            .take_row(uid)
            .expect("row leaves the source");
        target.entities.insert_row_preserving(row);
    }
    // 玩家表也跟着搬：这里只改 riding 侧，剩下的表由岛的拆合负责。
    let player = source.players.remove(&id).expect("player");
    target.insert_player(player);
    assert!(source.entities.ids().is_empty());
    assert!(super::check_consistency(&target).is_empty());
    assert_eq!(target.players[&id].riding.vehicle, Some(horse));
    assert_eq!(super::passengers(&target, horse), vec![id.0, cow]);
    assert_eq!(target.entities.public_at(cow).unwrap().vehicle, Some(horse));
}
