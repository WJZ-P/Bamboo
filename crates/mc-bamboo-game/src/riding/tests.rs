//! 骑乘的单元测试：附着点、校验和货物点。
//!
//! Unit tests for riding: attachment points, validation and the good positions.

use mc_bamboo_blocks::entity::mob::{Mob, MobPublic, MobType};
use mc_bamboo_core::Vec3;

use super::{contains_invalid_values, passenger_offset, wrap_degrees};

/// 官方 `EntityAttachments` 的乘客附着点：马在高度减 0.125，驴骡还要往后 0.3125，
/// 羊驼往后 0.3。朝向 0 时偏移只在 y 和 z 上。
///
/// The vanilla `EntityAttachments` passenger point: a horse sits at height minus
/// 0.125, donkeys and mules also go 0.3125 back, llamas 0.3 back. At yaw 0 the
/// offset only has y and z.
#[test]
fn passenger_offsets_match_the_attachment_points() {
    let public = |kind| {
        let mob = Mob::new(kind);
        MobPublic::of(&mob, 0)
    };
    let horse = passenger_offset(&public(MobType::Horse), 0.0);
    let height = f64::from(MobType::Horse.dimensions(false).1);
    assert!((horse.y - (height - 0.125)).abs() < 1.0e-6);
    assert!(horse.z.abs() < 1.0e-6);

    let donkey = passenger_offset(&public(MobType::Donkey), 0.0);
    let height = f64::from(MobType::Donkey.dimensions(false).1);
    assert!((donkey.y - (height + 0.031_25)).abs() < 1.0e-6);
    assert!((donkey.z + 0.3125).abs() < 1.0e-6);

    let llama = passenger_offset(&public(MobType::Llama), 0.0);
    let height = f64::from(MobType::Llama.dimensions(false).1);
    assert!((llama.y - (height - 0.25)).abs() < 1.0e-6);
    assert!((llama.z + 0.3).abs() < 1.0e-6);
}

/// 朝向 90 度时，驴的向后偏移转到 x 轴上；官方的 `Vec3.yRot(-yRot)` 也是这个方向。
///
/// At yaw 90 the donkey's backwards offset lands on x; vanilla `Vec3.yRot(-yRot)`
/// turns the same way.
#[test]
fn passenger_offset_rotates_with_the_vehicle() {
    let mob = Mob::new(MobType::Donkey);
    let public = MobPublic::of(&mob, 0);
    let offset = passenger_offset(&public, 90.0);
    assert!((offset.x - 0.3125).abs() < 1.0e-6);
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
