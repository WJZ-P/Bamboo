use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{blocks, items};

use super::*;
use crate::BlockState;
use crate::entity::JavaRandom;

fn shot(position: Vec3) -> EggShot {
    EggShot {
        position,
        direction: Vec3::new(1.0, 0.0, 0.0),
        power: 1.5,
        uncertainty: 0.0,
        source_movement: None,
        owner: None,
        owner_uuid: None,
        item: ItemStack::new(items::EGG, 16),
    }
}

/// 第一个让 `nextInt(8) == 0`、但 `nextInt(32) != 0` 的种子：孵一只。
///
/// The first seed whose `nextInt(8)` is 0 and `nextInt(32)` is not: one chick.
fn hatching_seed() -> i64 {
    (0..)
        .find(|&seed| {
            let mut random = JavaRandom::new(seed);
            random.next_int_bounded(8) == 0 && random.next_int_bounded(32) != 0
        })
        .unwrap()
}

#[test]
fn the_egg_keeps_a_single_item_and_the_throw_sets_its_angles() {
    let mut egg = Entity::thrown_egg(&shot(Vec3::new(0.5, 100.0, 0.5)));
    egg.shoot_egg(&shot(Vec3::new(0.5, 100.0, 0.5)));
    let EntityKind::Egg(data) = &egg.kind else {
        unreachable!()
    };
    assert_eq!(data.item.count, 1);
    assert_eq!(egg.delta, Vec3::new(1.5, 0.0, 0.0));
    // 乘折叠常量 `57.295776F`，正东是 89.99999 而不是 90（mob-oracle 对过）。
    // Multiplying by the folded `57.295776F` gives 89.99999 due east, not 90.
    assert_eq!(egg.y_rot, 89.99999);
    assert_eq!(data.x_rot, 0.0);
}

#[test]
fn gravity_comes_before_drag_and_the_egg_moves_by_the_new_velocity() {
    let mut level = crate::testing::TestLevel::new();
    let mut ticks = crate::BlockTicks::default();
    let mut world = World::new(&mut level, &mut ticks);
    let mut egg = Entity::thrown_egg(&shot(Vec3::new(0.5, 100.0, 0.5)));
    egg.delta = Vec3::new(1.0, 0.0, 0.0);
    let id = world.spawn_entity(egg);
    world.tick_entities();
    let egg = world.entity(id).unwrap();
    let drag = f64::from(0.99_f32);
    let delta = Vec3::new(1.0 * drag, -0.03 * drag, 0.0);
    assert_eq!(egg.delta, delta);
    assert_eq!(egg.position, Vec3::new(0.5, 100.0, 0.5).add(delta));
}

#[test]
fn an_egg_hitting_the_floor_breaks_and_hatches_one_baby_on_top() {
    let mut level = crate::testing::TestLevel::new();
    level.set(BlockPos::new(0, 63, 0), BlockState(blocks::STONE));
    let mut ticks = crate::BlockTicks::default();
    let mut world = World::new(&mut level, &mut ticks);
    let mut egg = Entity::thrown_egg(&shot(Vec3::new(0.5, 64.5, 0.5)));
    egg.delta = Vec3::new(0.0, -1.0, 0.0);
    let id = world.spawn_entity(egg);
    let seed = hatching_seed();
    world.with_entity_mut(id, &mut |egg| egg.random = JavaRandom::new(seed));
    world.tick_entities();
    assert!(world.entity(id).is_none());
    let chicks: Vec<_> = world
        .entity_ids()
        .into_iter()
        .filter_map(|uid| world.entity(uid))
        .filter(|entity| entity.kind.mob().is_some())
        .collect();
    assert_eq!(chicks.len(), 1);
    let chick = &chicks[0];
    let mob = chick.kind.mob().unwrap();
    assert_eq!(mob.mob_type, MobType::Chicken);
    assert_eq!(mob.age, -24000);
    assert_eq!(mob.variant, CowVariant::Temperate);
    assert!(
        (chick.position.y - 64.0).abs() < 1.0e-5,
        "{:?}",
        chick.position
    );
    assert!(chick.position.y >= 64.0 - 1.0e-7);
}

#[test]
fn the_free_spot_search_lifts_the_centre_out_of_the_floor() {
    let allowed = of_size(
        Vec3::new(0.5, 64.0, 0.5),
        0.2 + 1.0e-6,
        0.35 + 1.0e-6,
        0.2 + 1.0e-6,
    );
    let floor = Aabb::new(0.0, 63.0, 0.0, 1.0, 64.0, 1.0).inflate(0.1, 0.175, 0.1);
    let free = closest_free_point(allowed, &[floor], Vec3::new(0.5, 64.0, 0.5)).unwrap();
    assert_eq!(free, Vec3::new(0.5, 64.0 + 0.175, 0.5));
    let buried = Aabb::new(-5.0, 60.0, -5.0, 5.0, 70.0, 5.0);
    assert_eq!(
        closest_free_point(allowed, &[buried], Vec3::new(0.5, 64.0, 0.5)),
        None
    );
}
