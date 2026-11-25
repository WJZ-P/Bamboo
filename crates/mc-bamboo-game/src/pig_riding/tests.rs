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
