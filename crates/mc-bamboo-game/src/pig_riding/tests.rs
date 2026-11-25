//! 胡萝卜钓竿加速猪：骑乘、控制者、耐久和统计。

use mc_bamboo_blocks::{
    durability::set_damage_for_oracle,
    entity::{Entity, mob::MobType},
};
use mc_bamboo_core::{Hand, Vec3};
use mc_bamboo_data::items;
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{ClientInformation, EquipmentSlot, GameMode},
};
use uuid::Uuid;

use crate::{
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
    assert_eq!(mc_bamboo_blocks::durability::damage_of(held), Some(7));
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
        mc_bamboo_blocks::durability::damage_of(region.players[&id].inventory.held(Hand::Main)),
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
    assert_eq!(mc_bamboo_blocks::durability::damage_of(held), Some(0));
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
