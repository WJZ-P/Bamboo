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
        projectile: ThrowableKind::Egg,
    }
}

/// 一枚带玩家主人的末影珍珠。
fn pearl_shot(position: Vec3, owner: u32) -> EggShot {
    EggShot {
        position,
        direction: Vec3::new(0.0, -1.0, 0.0),
        power: 1.0,
        uncertainty: 0.0,
        source_movement: None,
        owner: Some(owner as i32),
        owner_uuid: Some([1; 16]),
        item: ItemStack::new(items::ENDER_PEARL, 1),
        projectile: ThrowableKind::EnderPearl,
    }
}

fn player(uid: u32, position: Vec3, yaw: f32) -> crate::entity::ctx::PlayerView {
    crate::entity::ctx::PlayerView {
        shoulder_left: None,
        shoulder_right: None,
        uid: uid as i32,
        uuid: Some([1; 16]),
        position,
        delta: Vec3::default(),
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::default(),
        gaze_disguise: false,
        alive: true,
        spectator: false,
        game_mode: 0,
        mining_fatigue: None,
        creative: false,
        flying: false,
        explosion_knockback_resistance: 0.0,
        sneaking: false,
        sprinting: false,
        swimming: false,
        invisible: false,
        armor_cover: 0.0,
        main_hand: 0,
        off_hand: 0,
        team: None,
        combat: Default::default(),
        effects: Default::default(),
        yaw,
        pitch: 12.5,
    }
}

/// 命中时按珍珠自己的随机源取完 32 组传送门粒子的数之后，`nextFloat() < 0.05` 的种子。
fn mite_seed() -> i64 {
    (0..)
        .find(|&seed| {
            let mut random = JavaRandom::new(seed);
            for _ in 0..32 {
                let _ = random.next_double();
                let _ = random.next_gaussian();
                let _ = random.next_gaussian();
            }
            random.next_float() < 0.05
        })
        .unwrap()
}

/// 同样取完 96 个数之后 `nextFloat() >= 0.05` 的种子。
fn no_mite_seed() -> i64 {
    (0..)
        .find(|&seed| {
            let mut random = JavaRandom::new(seed);
            for _ in 0..32 {
                let _ = random.next_double();
                let _ = random.next_gaussian();
                let _ = random.next_gaussian();
            }
            random.next_float() >= 0.05
        })
        .unwrap()
}

/// 珍珠落地：落点是命中前的 `oldPosition()`，不是方块命中的 `location`；再传主人、扣 5 点、
/// 发传送声。
#[test]
fn a_pearl_lands_at_old_position_and_teleports_its_owner() {
    use crate::level::Effect;

    let mut level = crate::testing::TestLevel::new();
    level.set(BlockPos::new(0, 63, 0), BlockState(blocks::STONE));
    let owner = player(7, Vec3::new(3.5, 64.0, 0.5), 30.0);
    level.players.push(owner);
    let mut ticks = crate::BlockTicks::default();
    let mut world = World::new(&mut level, &mut ticks);
    let mut pearl = Entity::thrown_egg(&pearl_shot(Vec3::new(0.5, 65.0, 0.5), owner.uid as u32));
    pearl.delta = Vec3::new(0.0, -2.0, 0.0);
    let id = world.spawn_entity(pearl);
    world.with_entity_mut(id, &mut |pearl| {
        pearl.random = JavaRandom::new(no_mite_seed())
    });
    world.tick_entities();
    assert!(world.entity(id).is_none(), "命中后消失");
    let effects = world.drain_effects();
    let teleport = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::EnderPearlTeleport { target, position } => Some((*target, *position)),
            _ => None,
        })
        .expect("传送效果");
    assert_eq!(teleport.0, owner.uid);
    // 命中点是 64.0，落点用的是命中前的 65.0。
    assert_eq!(teleport.1, Vec3::new(0.5, 65.0, 0.5));
    let hurt = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::HurtPlayer {
                target,
                source,
                amount,
            } => Some((*target, *source, *amount)),
            _ => None,
        })
        .expect("5 点伤害");
    assert_eq!(hurt.0, owner.uid);
    assert_eq!(hurt.1.kind, DamageKind::EnderPearl);
    assert_eq!(hurt.2, 5.0);
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::Sound { sound, .. }
                if *sound == i32::from(mc_bamboo_data::sounds::sound_event(
                    "minecraft:entity.player.teleport"
                ))
        )),
        "传送声"
    );
    assert!(
        !world
            .entity_ids()
            .into_iter()
            .any(|uid| world.entity(uid).is_some_and(|entity| entity
                .kind
                .mob()
                .is_some_and(|mob| mob.mob_type == MobType::Endermite))),
        "这个种子不刷末影螨"
    );
}

/// 主人睡觉时不传送、不扣血、不放传送声，珍珠照样消失。
#[test]
fn a_sleeping_owner_is_not_teleported() {
    use crate::level::Effect;

    let mut level = crate::testing::TestLevel::new();
    level.set(BlockPos::new(0, 63, 0), BlockState(blocks::STONE));
    let owner = player(7, Vec3::new(3.5, 64.0, 0.5), 30.0);
    level.players.push(owner);
    level.sleeping.push(owner.uid);
    let mut ticks = crate::BlockTicks::default();
    let mut world = World::new(&mut level, &mut ticks);
    let mut pearl = Entity::thrown_egg(&pearl_shot(Vec3::new(0.5, 65.0, 0.5), owner.uid as u32));
    pearl.delta = Vec3::new(0.0, -2.0, 0.0);
    let id = world.spawn_entity(pearl);
    world.with_entity_mut(id, &mut |pearl| {
        pearl.random = JavaRandom::new(no_mite_seed())
    });
    world.tick_entities();
    assert!(world.entity(id).is_none());
    let effects = world.drain_effects();
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::EnderPearlTeleport { .. }))
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::HurtPlayer { .. }))
    );
}

/// `nextFloat() < 0.05` 且刷怪规则开着时，在主人脚下刷一只末影螨，朝向照主人。
#[test]
fn a_pearl_roll_spawns_an_endermite_at_the_owner() {
    let mut level = crate::testing::TestLevel::new();
    level.set(BlockPos::new(0, 63, 0), BlockState(blocks::STONE));
    let owner = player(7, Vec3::new(3.5, 64.0, -2.5), 30.0);
    level.players.push(owner);
    let mut ticks = crate::BlockTicks::default();
    let mut world = World::new(&mut level, &mut ticks);
    let mut pearl = Entity::thrown_egg(&pearl_shot(Vec3::new(0.5, 65.0, 0.5), owner.uid as u32));
    pearl.delta = Vec3::new(0.0, -2.0, 0.0);
    let id = world.spawn_entity(pearl);
    world.with_entity_mut(id, &mut |pearl| pearl.random = JavaRandom::new(mite_seed()));
    world.tick_entities();
    let mites: Vec<_> = world
        .entity_ids()
        .into_iter()
        .filter_map(|uid| world.entity(uid))
        .filter(|entity| {
            entity
                .kind
                .mob()
                .is_some_and(|mob| mob.mob_type == MobType::Endermite)
        })
        .collect();
    assert_eq!(mites.len(), 1);
    let mite = &mites[0];
    assert_eq!(mite.position, owner.position);
    assert_eq!(mite.y_rot, owner.yaw);
    assert_eq!(mite.kind.mob().unwrap().x_rot, owner.pitch);
}

/// `spawnMobs` 关掉时这颗种子不刷末影螨。
#[test]
fn a_pearl_roll_respects_the_spawn_mobs_rule() {
    let mut level = crate::testing::TestLevel::new();
    level.set(BlockPos::new(0, 63, 0), BlockState(blocks::STONE));
    level.rules.spawn_mobs = false;
    let owner = player(7, Vec3::new(3.5, 64.0, 0.5), 0.0);
    level.players.push(owner);
    let mut ticks = crate::BlockTicks::default();
    let mut world = World::new(&mut level, &mut ticks);
    let mut pearl = Entity::thrown_egg(&pearl_shot(Vec3::new(0.5, 65.0, 0.5), owner.uid as u32));
    pearl.delta = Vec3::new(0.0, -2.0, 0.0);
    let id = world.spawn_entity(pearl);
    world.with_entity_mut(id, &mut |pearl| pearl.random = JavaRandom::new(mite_seed()));
    world.tick_entities();
    assert!(
        !world
            .entity_ids()
            .into_iter()
            .any(|uid| world.entity(uid).is_some_and(|entity| entity
                .kind
                .mob()
                .is_some_and(|mob| mob.mob_type == MobType::Endermite))),
        "关掉刷怪规则后不刷末影螨"
    );
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
fn burning_egg_extinguishes_when_crossing_water_into_dry_air() {
    let mut level = crate::testing::TestLevel::new();
    level.set(BlockPos::new(2, 65, 0), BlockState(blocks::WATER));
    let mut ticks = crate::BlockTicks::default();
    let mut world = World::new(&mut level, &mut ticks);
    let mut egg = Entity::thrown_egg(&shot(Vec3::new(0.5, 65.25, 0.5)));
    egg.delta = Vec3::new(5.0, 0.0, 0.0);
    egg.remaining_fire_ticks = 200;
    let id = world.spawn_entity(egg);
    let mut expected = world.entity(id).unwrap().random;
    expected.next_float();
    expected.next_float();
    world.tick_entities();
    let egg = world.entity(id).unwrap();
    assert!(egg.position.x > 5.0 && !egg.in_water);
    assert_eq!(egg.remaining_fire_ticks, 0);
    assert_eq!(egg.random.state(), expected.state());
}
