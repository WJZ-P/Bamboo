//! 胡萝卜钓竿加速猪：骑乘、控制者、耐久和统计。

use mc_bamboo_blocks::{
    BlockState, World,
    dispense::dispense_from,
    durability::{damage_of, set_damage_for_oracle},
    entity::{Entity, EntityRandom, JavaRandom, mob::MobType},
    state::FACING,
};
use mc_bamboo_core::{BlockPos, Direction, Hand, Vec3};
use mc_bamboo_data::items;
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{ClientInformation, EquipmentSlot, GameMode},
};
use uuid::Uuid;

use crate::{
    container::ContainerKind,
    inbound::PlayerId,
    interaction::on_use_item,
    player::Player,
    region::Region,
    riding::{mount, player_vehicles},
};

fn setup(item: i32, damage: Option<i32>) -> (Region, PlayerId, i32) {
    let mut region = Region::new(2, 20, false);
    let id = PlayerId(1);
    region.insert_player(Player::new(
        id,
        Uuid::from_u128(1),
        "Rider".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, 64.0, 0.5),
    ));
    let mut held = ItemStack::new(item, 1);
    if let Some(damage) = damage {
        set_damage_for_oracle(&mut held, damage);
    }
    let slot = region.players[&id].inventory.hand_slot(Hand::Main);
    region.players.get_mut(&id).unwrap().inventory.slots[slot] = held;
    let pig_id = region
        .entities
        .spawn(Entity::by_mob_type(MobType::Pig, Vec3::new(0.5, 64.0, 0.5)));
    region.entities.with_mut(pig_id, |entity| {
        entity.with_mob(|_, mob| {
            mob.equipment
                .set(EquipmentSlot::Saddle, ItemStack::new(items::SADDLE, 1));
            mob.equipment.set_drop_chance(EquipmentSlot::Saddle, 1.0);
        })
    });
    mount(&mut region, id, pig_id);
    (region, id, pig_id)
}

fn pig_data(region: &Region, pig_id: i32) -> (i32, bool) {
    region
        .entities
        .public_at(pig_id)
        .and_then(|public| match &public.kind {
            mc_bamboo_blocks::entity::KindPublic::Mob(mob) => {
                Some((mob.pig_boost_time, mob.needs_sync))
            }
            _ => None,
        })
        .expect("pig")
}

fn use_stick(region: &mut Region, id: PlayerId) {
    on_use_item(region, id, Hand::Main, 1, (0.0, 0.0));
}

#[test]
fn right_click_boosts_a_saddled_pig_and_spends_seven_durability() {
    let (mut region, id, pig_id) = setup(items::CARROT_ON_A_STICK, None);
    use_stick(&mut region, id);
    let (boost_time, needs_sync) = pig_data(&region, pig_id);
    assert!((140..=980).contains(&boost_time), "boost time {boost_time}");
    assert!(needs_sync);
    let held = region.players[&id].inventory.held(Hand::Main);
    assert_eq!(held.item, items::CARROT_ON_A_STICK);
    assert_eq!(damage_of(held), Some(7));
}

#[test]
fn a_failed_boost_only_awards_the_item_used_stat() {
    let (mut region, id, pig_id) = setup(items::CARROT_ON_A_STICK, None);
    // 没上鞍就没有控制者，官方 `getControlledVehicle` 返回空。
    region.entities.with_mut(pig_id, |entity| {
        entity.with_mob(|_, mob| {
            mob.equipment.set(EquipmentSlot::Saddle, ItemStack::empty());
        })
    });
    use_stick(&mut region, id);
    assert_eq!(
        region
            .players
            .get_mut(&id)
            .unwrap()
            .item_use_stats
            .take_dirty(),
        vec![(items::CARROT_ON_A_STICK, 1)]
    );
    assert_eq!(
        damage_of(region.players[&id].inventory.held(Hand::Main)),
        Some(0)
    );
}

#[test]
fn a_broken_carrot_on_a_stick_becomes_a_fishing_rod() {
    let (mut region, id, _) = setup(items::CARROT_ON_A_STICK, Some(24));
    use_stick(&mut region, id);
    let held = region.players[&id].inventory.held(Hand::Main);
    assert_eq!(held.item, items::FISHING_ROD);
    assert_eq!(held.count, 1);
    assert_eq!(damage_of(held), Some(0));
}

#[test]
fn a_second_right_click_during_a_boost_does_not_start_another_one() {
    let (mut region, id, pig_id) = setup(items::CARROT_ON_A_STICK, None);
    use_stick(&mut region, id);
    let (first_boost, _) = pig_data(&region, pig_id);
    use_stick(&mut region, id);
    let (second_boost, _) = pig_data(&region, pig_id);
    assert_eq!(first_boost, second_boost);
    assert_eq!(
        region
            .players
            .get_mut(&id)
            .unwrap()
            .item_use_stats
            .take_dirty(),
        vec![(items::CARROT_ON_A_STICK, 1)]
    );
}

#[test]
fn the_pig_is_the_player_controlled_vehicle() {
    let (region, id, pig_id) = setup(items::CARROT_ON_A_STICK, None);
    assert_eq!(player_vehicles(&region).get(&id), Some(&pig_id));
}

/// `Pig.tickRidden`：朝向跟着骑手，俯仰取一半；`getRiddenSpeed` 用 MOVEMENT_SPEED*0.225。
#[test]
fn ridden_rotation_and_speed_follow_the_rider() {
    use mc_bamboo_blocks::entity::mob::pig;
    use mc_bamboo_core::Vec3;

    let mut entity = Entity::by_mob_type(MobType::Pig, Vec3::new(0.5, 64.0, 0.5));
    entity.with_mob(|me, mob| {
        let mut steering = mob.pig_steering;
        assert!(steering.boost(&EntityRandom(std::cell::RefCell::new(JavaRandom::new(7)))));
        mob.pig_steering = steering;
        pig::tick_ridden(me, mob, 90.0, 60.0);
        assert_eq!(me.y_rot, 90.0);
        assert_eq!(mob.x_rot, 30.0);
        assert_eq!(mob.y_rot_o, 90.0);
        assert_eq!(mob.y_body_rot, 90.0);
        assert_eq!(mob.y_head_rot, 90.0);
        assert_eq!(mob.pig_steering.boost_time, 1);
        assert_eq!(
            pig::ridden_speed(0.25, &mob.pig_steering),
            (0.25 * 0.225 * f64::from(mob.pig_steering.boost_factor())) as f32
        );
    });
}

/// `Entity.interact` 的剪刀卸装：被骑 / 潜行时不动，否则卸鞍、扣剪刀耐久并掉落。
#[test]
fn shears_remove_a_pig_saddle() {
    use mc_bamboo_blocks::entity::mob::interact::{MobInteractRequest, interact};
    use mc_bamboo_protocol::item::ItemStack;

    let mut entity = Entity::by_mob_type(MobType::Pig, Vec3::new(0.5, 64.0, 0.5));
    let saddle = ItemStack::new(items::SADDLE, 1);
    entity.with_mob(|me, mob| {
        mob.equipment.set(EquipmentSlot::Saddle, saddle.clone());
        let request = MobInteractRequest {
            player_uid: 1,
            player_uuid: [0; 16],
            hand: Hand::Main,
            held: ItemStack::new(items::SHEARS, 1),
            creative: false,
            position: Vec3::default(),
            sneaking: false,
            difficulty: 2,
        };
        let outcome = interact(me, mob, &request, 0, false);
        assert_eq!(outcome.result, mc_bamboo_blocks::InteractionResult::Success);
        assert_eq!(outcome.drop_stack, Some(saddle));
        assert!(outcome.damage_held);
        assert!(mob.equipment.get(EquipmentSlot::Saddle).is_empty());
    });
}

/// 发射器前方格里的成年空鞍猪拿走一个鞍，并标记必掉 / 持久化。
#[test]
fn dispenser_saddles_an_adult_pig() {
    let mut region = Region::new(2, 20, false);
    let pig_id = region
        .entities
        .spawn(Entity::by_mob_type(MobType::Pig, Vec3::new(1.5, 64.0, 0.5)));
    let pos = BlockPos::new(0, 64, 0);
    crate::container::entry_at(&mut region, pos, ContainerKind::Dispenser).slots[0] =
        ItemStack::new(items::SADDLE, 1);
    {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        let state = BlockState(mc_bamboo_data::blocks::DISPENSER)
            .default_state()
            .with(FACING, Direction::East);
        world.set_block(pos, state, 0);
        dispense_from(&mut world, pos, state);
    }
    assert_eq!(
        crate::container::entry_at(&mut region, pos, ContainerKind::Dispenser).slots[0].count,
        0
    );
    region
        .entities
        .public_at(pig_id)
        .and_then(|public| match &public.kind {
            mc_bamboo_blocks::entity::KindPublic::Mob(mob) => {
                assert_eq!(
                    mob.equipment
                        .iter()
                        .find(|(slot, _)| *slot == EquipmentSlot::Saddle)
                        .map(|(_, stack)| stack.item),
                    Some(items::SADDLE)
                );
                assert!(mob.persistence_required);
                Some(())
            }
            _ => None,
        })
        .expect("pig");
}

/// 猪脚下 y=63 铺一层石头，让相邻格有地面。
fn floor(region: &mut Region) {
    for x in -3..=3 {
        for z in -3..=3 {
            region
                .chunks
                .set_block(BlockPos::new(x, 63, z), mc_bamboo_data::blocks::STONE);
        }
    }
}

/// 官方 `Animal.getDismountLocationForPassenger`：落在相邻格的正中，不是马科的逃逸向量落点。
/// 朝向 0（南）时右手一侧是 -x 那一格。
#[test]
fn a_dismount_from_a_pig_lands_at_the_centre_of_the_neighbouring_block() {
    let (mut region, id, _) = setup(items::CARROT_ON_A_STICK, None);
    region.use_infinite_flat_chunks();
    floor(&mut region);
    crate::riding::dismount(&mut region, id);
    let spot = region.players[&id].position();
    assert_eq!((spot.x, spot.y, spot.z), (-0.5, 64.0, 0.5));
}

/// 四周都堵死时落在猪的包围盒顶上（`Entity.getDismountLocationForPassenger` 的退路）。
#[test]
fn a_boxed_in_pig_drops_the_rider_on_top_of_itself() {
    use mc_bamboo_data::blocks;
    let (mut region, id, _) = setup(items::CARROT_ON_A_STICK, None);
    region.use_infinite_flat_chunks();
    floor(&mut region);
    for x in -1..=1 {
        for z in -1..=1 {
            if (x, z) != (0, 0) {
                for y in 64..=66 {
                    region
                        .chunks
                        .set_block(BlockPos::new(x, y, z), blocks::STONE);
                }
            }
        }
    }
    crate::riding::dismount(&mut region, id);
    let spot = region.players[&id].position();
    assert_eq!(
        (spot.x, spot.y, spot.z),
        (0.5, 64.0 + f64::from(0.9_f32), 0.5)
    );
}

/// 官方 `Mob.isSaddled` 要 `Pig.canUseSlot`：幼年猪鞍槽里有鞍也不算上了鞍，右键骑不上去。
#[test]
fn a_saddled_baby_pig_is_not_saddled() {
    let mut pig = Entity::by_mob_type(MobType::Pig, Vec3::new(0.5, 64.0, 0.5));
    pig.with_mob(|_, mob| {
        mob.equipment
            .set(EquipmentSlot::Saddle, ItemStack::new(items::SADDLE, 1));
        assert!(mc_bamboo_blocks::entity::mob::pig::is_saddled(mob));
        mob.age = -24000;
        assert!(!mc_bamboo_blocks::entity::mob::pig::is_saddled(mob));
    });
}

/// 官方 `Entity.setRot` 先取 `% 360`，所以 `tickRidden` 之后朝向落在 (-360, 360) 里。
#[test]
fn tick_ridden_wraps_the_rider_yaw_like_set_rot() {
    let mut pig = Entity::by_mob_type(MobType::Pig, Vec3::new(0.5, 64.0, 0.5));
    pig.with_mob(|me, mob| {
        mc_bamboo_blocks::entity::mob::pig::tick_ridden(me, mob, 362.0, 10.0);
        assert_eq!(me.y_rot, 2.0);
        assert_eq!(mob.y_head_rot, 2.0);
        assert_eq!(mob.x_rot, 5.0);
    });
}

/// 骑着上鞍的猪下线再上线：`RootVehicle` 带回猪（鞍、血量）和玩家的乘客关系，
/// 重新手持胡萝卜钓竿后仍是猪的控制者。
#[test]
fn a_rider_on_a_saddled_pig_survives_a_save_and_load() {
    use crate::riding::{controls_vehicle, load_root_vehicle, root_vehicle_save};

    let (mut region, id, pig_id) = setup(items::CARROT_ON_A_STICK, None);
    region.entities.with_mut(pig_id, |entity| {
        entity.with_mob(|_, mob| mob.health = 4.0);
    });
    let tag = root_vehicle_save(&region, id).expect("riding player saves RootVehicle");
    region.entities.remove(pig_id);
    region.players.get_mut(&id).unwrap().riding.vehicle = None;
    load_root_vehicle(&mut region, id, &tag);
    let vehicle = region.players[&id].riding.vehicle.expect("remounted");
    assert!(crate::riding::check_consistency(&region).is_empty());
    assert_eq!(crate::riding::passengers(&region, vehicle), vec![id.0]);
    let (saddled, health, pig_type) = region
        .entities
        .with_mut(vehicle, |entity| {
            entity.with_mob(|_, mob| {
                (
                    mc_bamboo_blocks::entity::mob::pig::is_saddled(mob),
                    mob.health,
                    mob.mob_type,
                )
            })
        })
        .flatten()
        .expect("pig came back");
    assert_eq!(pig_type, MobType::Pig);
    assert!(saddled);
    assert_eq!(health, 4.0);
    assert!(controls_vehicle(&mut region, id, vehicle));
    assert_eq!(root_vehicle_save(&region, id), Some(tag));
}
