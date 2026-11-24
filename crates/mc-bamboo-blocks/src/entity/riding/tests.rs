//! 通用骑乘的单元测试：附着点、上下车名单、落点和冷却。

use mc_bamboo_core::Vec3;

use super::*;
use crate::entity::{Entity, EntityKind, EntityUid, PrimedTnt};

fn entity(id: EntityUid, kind: crate::entity::mob::MobType, x: f64) -> Entity {
    let mut me = Entity::by_mob_type(kind, Vec3::new(x, 64.0, 0.5));
    me.id = id;
    me
}

#[test]
fn a_spider_skeleton_jockey_sits_at_the_registered_attachment_point() {
    let mut spider = entity(1, MobType::Spider, 0.5).split().public;
    spider.yaw = 0.0;
    let mut skeleton = entity(2, MobType::Skeleton, 10.0).split().public;
    skeleton.yaw = 0.0;
    // 蜘蛛的 PASSENGER 是 y=0.765，骷髅的 ridingOffset(-0.7) 让 VEHICLE 变成 y=+0.7。
    let at = rider_position(&spider, &skeleton, 0);
    assert_eq!(at.x, 0.5);
    assert_eq!(at.z, 0.5);
    assert!((at.y - (64.0 + 0.765 - 0.7)).abs() < 1.0e-6, "{at:?}");
}

#[test]
fn a_chicken_jockey_sits_behind_the_chickens_head() {
    let mut chicken = entity(1, MobType::Chicken, 0.5).split().public;
    chicken.yaw = 0.0;
    let mut zombie = entity(2, MobType::Zombie, 10.0).split().public;
    zombie.yaw = 0.0;
    let at = rider_position(&chicken, &zombie, 0);
    assert_eq!(at.x, 0.5);
    assert!((at.z - (0.5 - 0.1)).abs() < 1.0e-6, "{at:?}");
    assert!((at.y - (64.0 + 0.7 - 0.7)).abs() < 1.0e-6, "{at:?}");
}

#[test]
fn a_turned_vehicle_rotates_the_attachment_point() {
    let mut chicken = entity(1, MobType::Chicken, 0.5).split().public;
    chicken.yaw = 90.0;
    let zombie = entity(2, MobType::Zombie, 10.0).split().public;
    let at = rider_position(&chicken, &zombie, 0);
    // `Vec3(0, 0.7, -0.1).yRot(-90°)` → x = +0.1, z = 0。
    assert!((at.x - 0.6).abs() < 1.0e-5, "{at:?}");
    assert!((at.z - 0.5).abs() < 1.0e-5, "{at:?}");
}

#[test]
fn a_player_passenger_is_inserted_ahead_of_a_mob_passenger() {
    let mut list = vec![1 << 24];
    // 玩家 ID 小于 FIRST_ENTITY_ID，插到生物乘客前面。
    assert!(add_passenger(&mut list, 7, true));
    assert_eq!(list, vec![7, 1 << 24]);
}

#[test]
fn a_mob_passenger_appends_in_boarding_order() {
    let mut list = Vec::new();
    assert!(add_passenger(&mut list, 2, false));
    assert_eq!(list, vec![2]);
    // 官方 `canAddPassenger` 只允许一个人，第二个实体乘客被 `startRiding` 拒掉。
    assert!(!can_start_riding(3, None, 0, false, 1, &[1], &list));
}

#[test]
fn starting_a_ride_refuses_cycles_and_the_dismount_cooldown() {
    assert!(!can_start_riding(1, None, 0, false, 1, &[1], &[]));
    assert!(!can_start_riding(1, None, 60, false, 2, &[2], &[]));
    assert!(!can_start_riding(1, None, 0, true, 2, &[2], &[]));
    assert!(can_start_riding(1, None, 0, false, 2, &[2], &[]));
}

#[test]
fn dismounting_clears_both_sides_and_blocks_remounting_for_sixty_ticks() {
    let mut passenger = entity(2, MobType::Skeleton, 0.5);
    passenger.vehicle = Some(1);
    let position = passenger.position;
    let mut list = vec![2];
    dismount(&mut passenger, Some(&mut list));
    assert_eq!(passenger.vehicle, None);
    assert_eq!(passenger.boarding_cooldown, 60);
    // 官方只有玩家会按 `getDismountLocationForPassenger` 挪位置，生物乘客不动。
    assert_eq!(passenger.position, position);
    assert!(list.is_empty());
    // 载具已经离表时也照样把乘客这边摘干净。
    passenger.vehicle = Some(1);
    dismount(&mut passenger, None);
    assert_eq!(passenger.vehicle, None);
}

#[test]
fn a_non_mob_passenger_has_no_vehicle_attachment() {
    let mut tnt = Entity::new(EntityKind::Tnt(PrimedTnt::new()), Vec3::default());
    tnt.id = 3;
    let public = tnt.split().public;
    assert_eq!(vehicle_attachment(&public), Vec3::default());
}
