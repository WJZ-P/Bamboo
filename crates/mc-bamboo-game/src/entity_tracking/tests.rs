//! 跟踪范围、更新间隔和配对包。

use mc_bamboo_data::registries::entity_type;

use super::*;

#[test]
fn tracking_ranges_come_from_the_entity_type() {
    assert_eq!(entity_tracking_range(entity_type::FALLING_BLOCK), 160);
    assert_eq!(entity_tracking_range(entity_type::TNT), 160);
}

#[test]
fn update_intervals_come_from_the_entity_type() {
    assert_eq!(update_interval(entity_type::FALLING_BLOCK), Some(20));
    assert_eq!(update_interval(entity_type::TNT), Some(10));
}

#[test]
fn entity_uuids_are_unique() {
    assert_ne!(entity_uuid(1), entity_uuid(2));
    assert_eq!(entity_uuid(1), entity_uuid(1));
}
