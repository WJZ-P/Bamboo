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
fn a_skeleton_rider_uses_the_undead_horse_passenger_point() {
    // `EntityTypes.SKELETON_HORSE` / `ZOMBIE_HORSE` 都注册
    // `passengerAttachments([1.31875f])`，不是马的高度 1.6；骷髅的 VEHICLE 点是
    // `ridingOffset(-0.7f)`。两处都是 f32，转成 f64 后相减，和官方逐位一致。
    // Both undead horses register passengerAttachments([1.31875f]), not the
    // horse's 1.6 height; the skeleton's VEHICLE point is ridingOffset(-0.7f).
    let expected = 64.0 + f64::from(1.31875_f32) - f64::from(0.7_f32);
    for mount in [MobType::SkeletonHorse, MobType::ZombieHorse] {
        let mut horse = entity(1, mount, 0.5).split().public;
        horse.yaw = 0.0;
        let mut skeleton = entity(2, MobType::Skeleton, 10.0).split().public;
        skeleton.yaw = 0.0;
        let at = rider_position(&horse, &skeleton, 0);
        assert_eq!(at.x, 0.5, "{mount:?}");
        assert_eq!(at.z, 0.5, "{mount:?}");
        assert!(
            (at.y - expected).abs() < 1.0e-12,
            "{mount:?}: {at:?} != {expected}"
        );
    }
}

#[test]
fn a_baby_chicken_jockey_uses_the_baby_vehicle_attachment_point() {
    let mut chicken = entity(1, MobType::Chicken, 0.5).split().public;
    chicken.yaw = 0.0;
    let mut zombie = entity(2, MobType::Zombie, 10.0);
    zombie.kind.mob_mut().unwrap().set_zombie_baby(true);
    let mut zombie = zombie.split().public;
    zombie.yaw = 0.0;
    // `Zombie.BABY_DIMENSIONS` 的 VEHICLE 点是 (0, 0.1875f, 0)，不是 0.7 折半；
    // 成年鸡的 PASSENGER 点仍是 (0, 0.7, -0.1)，故 y = 64 + 0.7 - 0.1875。
    // The baby zombie's VEHICLE point is 0.1875f (BABY_DIMENSIONS), so the rider
    // sits at y = 64.5125 rather than 64.0 for an adult.
    let at = rider_position(&chicken, &zombie, 0);
    assert!((at.y - 64.5125).abs() < 1.0e-6, "{at:?}");
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

#[test]
fn a_controlling_rider_borrows_the_vehicle_navigation_and_move_control() {
    use mc_bamboo_core::BlockPos;

    let mut level = crate::testing::TestLevel::new();
    let mut ticks = crate::BlockTicks::new();
    let view = crate::World::new(&mut level, &mut ticks);

    let mut spider = entity(1, MobType::Spider, 0.5);
    let mut skeleton = entity(2, MobType::Skeleton, 10.0);
    skeleton.vehicle = Some(1);
    spider.kind.mob_mut().unwrap().passengers.push(2);

    // 载具和自己那份导航/移动控制各给一个可辨认的状态。
    spider.kind.mob_mut().unwrap().navigation.tick = 7;
    spider.kind.mob_mut().unwrap().navigation.target_pos = Some(BlockPos::new(3, 64, 4));
    spider
        .kind
        .mob_mut()
        .unwrap()
        .mover
        .set_wanted_position(9.0, 64.0, 9.0, 1.0);
    skeleton.kind.mob_mut().unwrap().navigation.tick = 42;
    skeleton
        .kind
        .mob_mut()
        .unwrap()
        .mover
        .set_wanted_position(1.0, 64.0, 1.0, 0.5);
    let own_nav_tick = skeleton.kind.mob().unwrap().navigation.tick;
    let (spider_width, _) = spider.kind.mob().unwrap().dimensions();

    assert!(begin_nav_borrow(&mut skeleton, &spider, &view));
    {
        let rider = skeleton.kind.mob().unwrap();
        assert_eq!(rider.navigation.tick, 7, "shared navigation installed");
        assert_eq!(
            rider.borrowed_own_navigation.as_ref().unwrap().tick,
            own_nav_tick
        );
        let snapshot = rider.borrowed_nav_mob.unwrap();
        assert_eq!(snapshot.owner_uid, 1, "path requests belong to the vehicle");
        assert_eq!(snapshot.path.width, spider_width, "vehicle dimensions");
        assert_eq!(rider.mover.wanted_x, 9.0, "vehicle mover installed");
    }

    // 目标代码把路径 / wanted 写进共享状态；tick 完写回载具，恢复自己的。
    {
        let rider = skeleton.kind.mob_mut().unwrap();
        rider.navigation.tick = 8;
        rider.mover.set_wanted_position(12.0, 64.0, 12.0, 1.25);
    }
    let (navigation, mover) = end_nav_borrow(&mut skeleton).expect("borrow ends");
    assert_eq!(navigation.tick, 8);
    assert_eq!(mover.wanted_x, 12.0);
    assert_eq!(skeleton.kind.mob().unwrap().navigation.tick, own_nav_tick);
    assert_eq!(skeleton.kind.mob().unwrap().mover.wanted_x, 1.0);
    assert!(skeleton.kind.mob().unwrap().borrowed_nav_mob.is_none());

    put_borrowed_nav(&mut spider, navigation, mover);
    assert_eq!(spider.kind.mob().unwrap().navigation.tick, 8);
    assert_eq!(spider.kind.mob().unwrap().mover.wanted_x, 12.0);
}

#[test]
fn non_controlling_passengers_do_not_borrow_the_vehicle_navigation() {
    let mut level = crate::testing::TestLevel::new();
    let mut ticks = crate::BlockTicks::new();
    let view = crate::World::new(&mut level, &mut ticks);

    // 史莱姆和硫磺方块怪都在 #non_controlling_rider 里（岩浆怪未建模）。
    let mut spider = entity(1, MobType::Spider, 0.5);
    let mut slime = entity(2, MobType::Slime, 10.0);
    slime.vehicle = Some(1);
    spider.kind.mob_mut().unwrap().passengers.push(2);
    assert!(!begin_nav_borrow(&mut slime, &spider, &view));

    let mut sulfur_cube = entity(4, MobType::SulfurCube, 10.0);
    sulfur_cube.vehicle = Some(1);
    spider.kind.mob_mut().unwrap().passengers = vec![4];
    assert!(!begin_nav_borrow(&mut sulfur_cube, &spider, &view));

    // NoAI 载具不产生控制乘客。
    spider.kind.mob_mut().unwrap().no_ai = true;
    let mut skeleton = entity(3, MobType::Skeleton, 10.0);
    skeleton.vehicle = Some(1);
    spider.kind.mob_mut().unwrap().passengers = vec![3];
    assert!(!begin_nav_borrow(&mut skeleton, &spider, &view));

    // 第二个乘客不是 getFirstPassenger。
    spider.kind.mob_mut().unwrap().no_ai = false;
    spider.kind.mob_mut().unwrap().passengers = vec![2, 3];
    assert!(!begin_nav_borrow(&mut skeleton, &spider, &view));
}

#[test]
fn stop_riding_on_a_live_vehicle_lands_the_rider_on_its_box_top() {
    let mut spider = entity(1, MobType::Spider, 0.5).split().public;
    spider.yaw = 0.0;
    let mut zombie = entity(2, MobType::Zombie, 10.0);
    zombie.vehicle = Some(1);
    let at = dismount_location(&spider);
    assert_eq!(at.x, 0.5);
    assert_eq!(at.y, 64.0 + f64::from(0.9_f32));
    stop_riding(&mut zombie, &spider, None);
    assert_eq!(zombie.vehicle, None);
    assert_eq!(zombie.boarding_cooldown, 60);
    assert_eq!(zombie.position, at);
    assert!(!zombie.on_ground);

    // 载具已经离表时不下落、不水平挪动（官方 `dismountVehicle` 的 removed 分支）。
    let mut removed = spider.clone();
    removed.removed = true;
    let mut zombie = entity(3, MobType::Zombie, 10.0);
    zombie.vehicle = Some(1);
    let before = zombie.position;
    stop_riding(&mut zombie, &removed, None);
    assert_eq!(zombie.vehicle, None);
    assert_eq!(zombie.position, before);
}
