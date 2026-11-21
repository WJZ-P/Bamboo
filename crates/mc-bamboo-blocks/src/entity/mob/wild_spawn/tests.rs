use mc_bamboo_core::Vec3;
use mc_bamboo_data::registries::biome;

use super::*;
use crate::random::LegacyRandom;

/// `Mob.finalizeSpawn` 的取数：跟随距离的 `triangle` 和左撇子的 `nextFloat`。
///
/// The draws of `Mob.finalizeSpawn`: the follow-range `triangle` and the
/// left-handed `nextFloat`.
fn replay_mob_layer(random: &LegacyRandom) {
    let _ = random.triangle(0.0, 0.114_850_000_000_000_01);
    let _ = random.next_float();
}

fn spawn(
    kind: MobType,
    biome: i32,
    random: &LegacyRandom,
    group: &mut Option<SpawnGroup>,
) -> Entity {
    let mut entity = Entity::by_mob_type(kind, Vec3::default());
    finalize(kind, &mut entity, biome, random, group);
    entity
}

fn is_baby(entity: &Entity) -> bool {
    entity.kind.mob().is_some_and(|mob| mob.is_baby())
}

#[test]
fn rabbits_after_the_first_are_babies_of_the_same_variant() {
    let random = LegacyRandom::new(7);
    let expected = LegacyRandom::new(7);
    let mut group = None;
    let first = spawn(MobType::Rabbit, biome::PLAINS, &random, &mut group);
    let _ = expected.next_int(100);
    replay_mob_layer(&expected);
    assert!(!is_baby(&first));
    let variant = first.kind.mob().expect("兔子").rabbit.variant;
    for _ in 0..2 {
        // 换成沙漠也还是组里的品种，但那一次 `nextInt(100)` 照取。
        // The desert roll still happens, yet the group variant wins.
        let next = spawn(MobType::Rabbit, biome::DESERT, &random, &mut group);
        let _ = expected.next_int(100);
        let _ = expected.next_float();
        replay_mob_layer(&expected);
        assert!(is_baby(&next));
        assert_eq!(next.kind.mob().expect("兔子").rabbit.variant, variant);
    }
    assert_eq!(random.seed(), expected.seed());
}

#[test]
fn a_desert_rabbit_group_is_gold() {
    let random = LegacyRandom::new(3);
    let mut group = None;
    let first = spawn(MobType::Rabbit, biome::DESERT, &random, &mut group);
    assert_eq!(
        first.kind.mob().expect("兔子").rabbit.variant,
        rabbit::Variant::Gold
    );
}

#[test]
fn the_third_fox_of_a_group_is_a_baby_and_all_share_the_first_variant() {
    let random = LegacyRandom::new(11);
    let mut group = None;
    let biomes = [biome::SNOWY_TAIGA, biome::TAIGA, biome::TAIGA, biome::TAIGA];
    let foxes = biomes.map(|biome| spawn(MobType::Fox, biome, &random, &mut group));
    let babies = foxes.each_ref().map(is_baby);
    assert_eq!(babies, [false, false, true, true]);
    for fox in &foxes {
        assert_eq!(
            fox.kind.mob().expect("狐狸").fox.variant,
            fox::Variant::Snow
        );
    }
}

#[test]
fn a_lone_fox_takes_the_biome_variant() {
    let random = LegacyRandom::new(11);
    let fox = spawn(MobType::Fox, biome::TAIGA, &random, &mut None);
    assert_eq!(fox.kind.mob().expect("狐狸").fox.variant, fox::Variant::Red);
    assert!(!is_baby(&fox));
}

#[test]
fn a_wolf_pack_shares_the_first_variant_and_only_selects_once() {
    let random = LegacyRandom::new(5);
    let expected = LegacyRandom::new(5);
    let mut group = None;
    let first = spawn(MobType::Wolf, biome::FOREST, &random, &mut group);
    let _ = expected.next_int(1);
    let _ = expected.next_int(7);
    replay_mob_layer(&expected);
    let variant = first.kind.mob().expect("狼").wolf.variant;
    for _ in 0..3 {
        let next = spawn(MobType::Wolf, biome::SNOWY_TAIGA, &random, &mut group);
        let _ = expected.next_int(7);
        replay_mob_layer(&expected);
        assert!(!is_baby(&next));
        assert_eq!(next.kind.mob().expect("狼").wolf.variant, variant);
    }
    assert_eq!(random.seed(), expected.seed());
    // 没有狼群数据时按群系另选。
    // Without pack data the biome decides again.
    let lone = spawn(MobType::Wolf, biome::SNOWY_TAIGA, &random, &mut None);
    assert_ne!(lone.kind.mob().expect("狼").wolf.variant, variant);
}

#[test]
fn group_data_of_another_species_is_replaced() {
    let random = LegacyRandom::new(9);
    let mut group = None;
    spawn(MobType::Wolf, biome::FOREST, &random, &mut group);
    let fox = spawn(MobType::Fox, biome::SNOWY_TAIGA, &random, &mut group);
    assert_eq!(
        fox.kind.mob().expect("狐狸").fox.variant,
        fox::Variant::Snow
    );
    let Some(SpawnGroup::Wild(data)) = group else {
        panic!("狐狸的组数据");
    };
    assert_eq!(data.group_size, 1);
    assert_eq!(data.pack, Pack::Fox(fox::Variant::Snow));
}
