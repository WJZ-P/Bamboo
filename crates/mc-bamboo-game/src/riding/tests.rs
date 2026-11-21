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
