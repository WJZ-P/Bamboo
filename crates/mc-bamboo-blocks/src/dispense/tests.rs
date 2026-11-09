//! 每种发射行为和失败分支，包括世界随机源的种子。
//!
//! Every dispense behaviour and its failure branch, including the world random seed.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{
    blocks, items,
    registries::{data_component_type as kinds, entity_type},
    tags::item_tags,
};
use mc_bamboo_protocol::{
    components::{ComponentValue, defaults},
    item::ItemStack,
};

use super::dispense_from;
use crate::{
    Aabb, BlockTicks, Effect, LegacyRandom,
    entity::EntityKind,
    item_exchange::{set_component, water_potion_stack},
    item_use::{grow_crop, grow_water_plant},
    level::EquipTarget,
    state::{
        BlockState, FACING, LEVEL_HONEY, LIT, RESPAWN_ANCHOR_CHARGES, ROTATION_16, SLAB_TYPE,
        SlabType, WATERLOGGED, item_is_tagged,
    },
    testing::TestLevel,
    world::{UPDATE_ALL, World},
};

const POS: BlockPos = BlockPos::new(0, 64, 0);

fn empty(count: usize) -> Vec<ItemStack> {
    vec![ItemStack::empty(); count]
}

fn slot(world: &mut World, pos: BlockPos, index: usize) -> ItemStack {
    crate::hopper::query(world, pos, &mut |container| container.get(index)).expect("容器")
}

fn level_events(effects: &[Effect]) -> Vec<(i32, BlockPos, i32)> {
    effects
        .iter()
        .filter_map(|effect| match *effect {
            Effect::LevelEvent {
                event, pos, data, ..
            } => Some((event, pos, data)),
            _ => None,
        })
        .collect()
}

fn sound_ids(effects: &[Effect]) -> Vec<i32> {
    effects
        .iter()
        .filter_map(|effect| match *effect {
            Effect::Sound { sound, .. } => Some(sound),
            _ => None,
        })
        .collect()
}

fn place_dispenser(world: &mut World, facing: Direction) {
    world.set_block(
        POS,
        BlockState(blocks::DISPENSER).with(FACING, facing),
        UPDATE_ALL,
    );
    let _ = world.drain_effects();
}

struct Shot {
    effects: Vec<Effect>,
    seed: i64,
    left: ItemStack,
    front: BlockState,
    primed: usize,
}

fn fire_one(level: &mut TestLevel, facing: Direction, item: ItemStack) -> Shot {
    let mut slots = empty(9);
    slots[0] = item;
    level.set_container(POS, slots);
    let mut ticks = BlockTicks::new();
    let mut world = World::new(level, &mut ticks);
    place_dispenser(&mut world, facing);
    let state = world.block_state(POS);
    dispense_from(&mut world, POS, state);
    let front = world.block_state(POS.relative(facing));
    let left = slot(&mut world, POS, 0);
    let seed = world.random().seed();
    let effects = world.drain_effects();
    drop(world);
    let primed = level
        .entities()
        .iter()
        .filter(|entity| matches!(entity.kind, EntityKind::Tnt(_)))
        .count();
    Shot {
        left,
        seed,
        effects,
        front,
        primed,
    }
}

fn fire_id(level: &mut TestLevel, facing: Direction, item: i32) -> Shot {
    fire_one(level, facing, ItemStack::new(item, 1))
}

fn seed_after(draws: impl FnOnce(&LegacyRandom)) -> i64 {
    let random = LegacyRandom::new(0);
    draws(&random);
    random.seed()
}

fn slot_roll(random: &LegacyRandom) {
    let _ = random.next_int(1);
}

fn throw_draws(random: &LegacyRandom, facing: Direction) {
    let speed = random.next_double() * 0.1 + 0.2;
    let spread = 0.017_227_5 * 6.0;
    let (step_x, _, step_z) = facing.normal();
    let _ = random.triangle(f64::from(step_x) * speed, spread);
    let _ = random.triangle(0.2, spread);
    let _ = random.triangle(f64::from(step_z) * speed, spread);
}

/// `PrimedTnt` 构造函数里的朝向。
fn tnt_yaw(random: &LegacyRandom) {
    let _ = random.next_double();
}

/// `EntityType.create` 的朝向，加上 `Mob.finalizeSpawn` 的 `triangle` 和左手。
fn mob_spawn_draws(random: &LegacyRandom) {
    let _ = random.next_float();
    let _ = random.triangle(0.0, 0.114_850_000_000_000_01);
    let _ = random.next_float();
}

/// `Block.popResource` 的三次位置偏移。
fn pop_resource_offset(random: &LegacyRandom) {
    let _ = random.next_double();
    let _ = random.next_double();
    let _ = random.next_double();
}

fn assert_pair(events: &[(i32, BlockPos, i32)], sound: i32, facing: Direction) {
    let smoke = facing.id();
    assert!(
        events
            .windows(2)
            .any(|pair| pair[0] == (sound, POS, 0) && pair[1] == (2000, POS, smoke)),
        "缺少 {sound} 和 2000，实际 {events:?}"
    );
}

fn has_primed_sound(effects: &[Effect]) -> bool {
    let id = i32::from(mc_bamboo_data::sounds::sound_event(
        "minecraft:entity.tnt.primed",
    ));
    sound_ids(effects).contains(&id)
}

fn damage_of(stack: &ItemStack) -> i32 {
    match defaults::resolve(stack.item, &stack.patch(), kinds::DAMAGE) {
        Some(ComponentValue::Damage(value)) => *value,
        other => panic!("没有耐久组件 {other:?}"),
    }
}

/// 没写上耐久组件时是 0。耐久附魔把伤害减成 0 时官方不调用 `setDamageValue`。
fn damage_or_zero(stack: &ItemStack) -> i32 {
    match defaults::resolve(stack.item, &stack.patch(), kinds::DAMAGE) {
        Some(ComponentValue::Damage(value)) => *value,
        _ => 0,
    }
}

fn player_at(pos: BlockPos, spectator: bool, head: ItemStack) -> EquipTarget {
    let mut equipment = std::array::from_fn(|_| Some(ItemStack::empty()));
    equipment[4] = Some(head);
    EquipTarget {
        entity_id: 7,
        bounds: Aabb::new(
            f64::from(pos.x),
            f64::from(pos.y),
            f64::from(pos.z),
            f64::from(pos.x) + 1.0,
            f64::from(pos.y) + 1.0,
            f64::from(pos.z) + 1.0,
        ),
        spectator,
        entity_type: entity_type::PLAYER,
        equipment,
    }
}

#[test]
fn an_empty_dispenser_plays_1001_and_draws_nothing() {
    let mut level = TestLevel::new();
    level.set_container(POS, empty(9));
    let mut ticks = BlockTicks::new();
    let mut world = World::new(&mut level, &mut ticks);
    place_dispenser(&mut world, Direction::East);
    let state = world.block_state(POS);
    dispense_from(&mut world, POS, state);
    assert_eq!(level_events(&world.drain_effects()), [(1001, POS, 0)]);
    assert_eq!(world.random().seed(), LegacyRandom::new(0).seed());
}

#[test]
fn a_missing_block_entity_does_not_play_1001() {
    let mut level = TestLevel::new();
    let mut ticks = BlockTicks::new();
    let mut world = World::new(&mut level, &mut ticks);
    place_dispenser(&mut world, Direction::North);
    let state = world.block_state(POS);
    dispense_from(&mut world, POS, state);
    assert!(world.drain_effects().is_empty());
    assert_eq!(world.random().seed(), LegacyRandom::new(0).seed());
}

#[test]
fn a_plain_item_is_thrown_with_spawn_item_random() {
    let shot = fire_id(&mut TestLevel::new(), Direction::East, items::STONE);
    assert!(shot.left.is_empty());
    assert_pair(&level_events(&shot.effects), 1000, Direction::East);
    assert_eq!(
        shot.seed,
        seed_after(|random| {
            slot_roll(random);
            throw_draws(random, Direction::East);
        })
    );
}

/// 默认抛出真的生成物品实体：位置在方块中心沿朝向外 0.7 格，再按方向下移。
#[test]
fn a_default_throw_spawns_a_ground_item() {
    let mut level = TestLevel::new();
    let mut slots = empty(9);
    slots[0] = ItemStack::new(items::STONE, 2);
    level.set_container(POS, slots);
    let mut ticks = BlockTicks::new();
    let mut world = World::new(&mut level, &mut ticks);
    place_dispenser(&mut world, Direction::East);
    let state = world.block_state(POS);
    dispense_from(&mut world, POS, state);
    let thrown: Vec<_> = world
        .entity_ids()
        .into_iter()
        .filter_map(|id| world.entity(id))
        .filter_map(|entity| {
            entity
                .ground()
                .map(|item| (entity.position, entity.delta, item.stack.clone()))
        })
        .collect();
    assert_eq!(thrown.len(), 1);
    let (position, delta, stack) = &thrown[0];
    assert_eq!(*stack, ItemStack::new(items::STONE, 1));
    assert!((position.x - 1.2).abs() < 1.0e-9);
    assert!((position.y - 64.5 + 0.15625).abs() < 1.0e-9);
    assert!((position.z - 0.5).abs() < 1.0e-9);
    assert!(delta.x > 0.0 && (delta.y - 0.2).abs() < 0.2 && delta.z.abs() < 0.2);
    assert_eq!(slot(&mut world, POS, 0), ItemStack::new(items::STONE, 1));
}

/// 塞不回发射器的余料也抛出真的物品实体，随机数只取一次。
#[test]
fn an_unplaceable_remainder_spawns_a_ground_item() {
    let mut level = TestLevel::new();
    level.set_container(POS, vec![ItemStack::new(items::WATER_BUCKET, 2); 9]);
    let mut ticks = BlockTicks::new();
    let mut world = World::new(&mut level, &mut ticks);
    place_dispenser(&mut world, Direction::Up);
    let state = world.block_state(POS);
    dispense_from(&mut world, POS, state);
    let items: Vec<_> = world
        .entity_ids()
        .into_iter()
        .filter_map(|id| world.entity(id))
        .filter_map(|entity| entity.ground().map(|item| (entity.position, &item.stack)))
        .collect();
    assert_eq!(items.len(), 1);
    assert_eq!(*items[0].1, ItemStack::new(items::BUCKET, 1));
    assert!((items[0].0.y - 65.2 + 0.125).abs() < 1.0e-9);
}

#[test]
fn the_scheduled_tick_uses_the_same_dispense() {
    let mut level = TestLevel::new();
    let mut slots = empty(9);
    slots[0] = ItemStack::new(items::STONE, 1);
    level.set_container(POS, slots);
    let mut ticks = BlockTicks::new();
    let mut world = World::new(&mut level, &mut ticks);
    place_dispenser(&mut world, Direction::South);
    let state = world.block_state(POS);
    state.tick(&mut world, POS);
    assert!(slot(&mut world, POS, 0).is_empty());
    assert_pair(
        &level_events(&world.drain_effects()),
        1000,
        Direction::South,
    );
}

#[test]
fn water_lava_and_powder_snow_pour_into_air() {
    let water = fire_id(&mut TestLevel::new(), Direction::Up, items::WATER_BUCKET);
    assert!(water.front.is(blocks::WATER));
    assert_eq!(water.left, ItemStack::new(items::BUCKET, 1));
    assert_pair(&level_events(&water.effects), 1000, Direction::Up);
    assert_eq!(water.seed, seed_after(slot_roll));

    let mut level = TestLevel::new();
    let lava = fire_id(&mut level, Direction::Up, items::LAVA_BUCKET);
    assert!(lava.front.is(blocks::LAVA));
    assert_eq!(lava.left, ItemStack::new(items::BUCKET, 1));

    let mut level = TestLevel::new();
    let snow = fire_id(&mut level, Direction::Up, items::POWDER_SNOW_BUCKET);
    assert!(snow.front.is(blocks::POWDER_SNOW));
    assert_eq!(snow.left, ItemStack::new(items::BUCKET, 1));
    assert_eq!(snow.seed, seed_after(slot_roll));
}

#[test]
fn a_blocked_filled_bucket_nests_a_default_throw() {
    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::STONE));
    let shot = fire_id(&mut level, Direction::East, items::WATER_BUCKET);
    assert!(shot.left.is_empty());
    assert!(shot.front.is(blocks::STONE));
    let events = level_events(&shot.effects);
    assert_eq!(events.iter().filter(|event| event.0 == 1000).count(), 2);
    assert_eq!(events.iter().filter(|event| event.0 == 2000).count(), 2);
    assert_eq!(
        shot.seed,
        seed_after(|random| {
            slot_roll(random);
            throw_draws(random, Direction::East);
        })
    );
}

#[test]
fn a_fish_bucket_pours_water_and_skips_the_mob() {
    let shot = fire_id(&mut TestLevel::new(), Direction::Up, items::SALMON_BUCKET);
    assert!(shot.front.is(blocks::WATER));
    assert_eq!(shot.left, ItemStack::new(items::BUCKET, 1));
    assert_eq!(
        shot.seed,
        seed_after(|random| {
            slot_roll(random);
            mob_spawn_draws(random);
        })
    );

    let axolotl = fire_id(&mut TestLevel::new(), Direction::Up, items::AXOLOTL_BUCKET);
    assert!(axolotl.front.is(blocks::WATER));
    assert_eq!(
        axolotl.seed,
        seed_after(|random| {
            slot_roll(random);
            let _ = random.next_float();
        })
    );
}

#[test]
fn a_sulfur_cube_bucket_plays_the_empty_sound_and_places_nothing() {
    let shot = fire_id(
        &mut TestLevel::new(),
        Direction::Up,
        items::SULFUR_CUBE_BUCKET,
    );
    assert!(shot.front.is_air());
    assert_eq!(shot.left, ItemStack::new(items::BUCKET, 1));
    assert!(!sound_ids(&shot.effects).is_empty());
    assert_eq!(
        shot.seed,
        seed_after(|random| {
            slot_roll(random);
            mob_spawn_draws(random);
        })
    );
}

#[test]
fn an_empty_bucket_picks_up_water_powder_snow_and_a_waterlogged_slab() {
    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::WATER));
    let water = fire_id(&mut level, Direction::East, items::BUCKET);
    assert!(water.front.is_air());
    assert_eq!(water.left, ItemStack::new(items::WATER_BUCKET, 1));
    assert_pair(&level_events(&water.effects), 1000, Direction::East);

    let mut level = TestLevel::new();
    level.set(
        POS.relative(Direction::East),
        BlockState(blocks::POWDER_SNOW),
    );
    let snow = fire_id(&mut level, Direction::East, items::BUCKET);
    assert!(snow.front.is_air());
    assert_eq!(snow.left, ItemStack::new(items::POWDER_SNOW_BUCKET, 1));
    assert!(snow.effects.iter().any(|effect| matches!(
        effect,
        Effect::Destroyed { pos, .. } if *pos == POS.relative(Direction::East)
    )));

    let mut level = TestLevel::new();
    let slab = BlockState(blocks::OAK_SLAB)
        .with(SLAB_TYPE, SlabType::Bottom)
        .with(WATERLOGGED, true);
    level.set(POS.relative(Direction::East), slab);
    let wet = fire_id(&mut level, Direction::East, items::BUCKET);
    assert!(!wet.front.get(WATERLOGGED));
    assert_eq!(wet.left, ItemStack::new(items::WATER_BUCKET, 1));

    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::STONE));
    let miss = fire_id(&mut level, Direction::East, items::BUCKET);
    assert!(miss.left.is_empty());
    assert_eq!(
        level_events(&miss.effects)
            .iter()
            .filter(|event| event.0 == 1000)
            .count(),
        1
    );
    assert_eq!(
        miss.seed,
        seed_after(|random| {
            slot_roll(random);
            throw_draws(random, Direction::East);
        })
    );
}

#[test]
fn a_second_bucket_is_inserted_and_a_full_dispenser_throws_the_leftover() {
    let mut level = TestLevel::new();
    let mut slots = empty(9);
    slots[0] = ItemStack::new(items::WATER_BUCKET, 2);
    level.set_container(POS, slots);
    let mut ticks = BlockTicks::new();
    let mut world = World::new(&mut level, &mut ticks);
    place_dispenser(&mut world, Direction::Up);
    let state = world.block_state(POS);
    dispense_from(&mut world, POS, state);
    assert_eq!(
        slot(&mut world, POS, 0),
        ItemStack::new(items::WATER_BUCKET, 1)
    );
    assert_eq!(slot(&mut world, POS, 1), ItemStack::new(items::BUCKET, 1));
    assert_eq!(
        level_events(&world.drain_effects())
            .iter()
            .filter(|event| event.0 == 1000)
            .count(),
        1
    );

    let mut level = TestLevel::new();
    let slots = vec![ItemStack::new(items::WATER_BUCKET, 2); 9];
    level.set_container(POS, slots);
    let mut ticks = BlockTicks::new();
    let mut world = World::new(&mut level, &mut ticks);
    place_dispenser(&mut world, Direction::Up);
    let state = world.block_state(POS);
    dispense_from(&mut world, POS, state);
    let kept = (0..9)
        .filter(|&index| {
            let stack = slot(&mut world, POS, index);
            stack.item == items::WATER_BUCKET && stack.count == 1
        })
        .count();
    assert_eq!(kept, 1);
    assert_eq!(
        level_events(&world.drain_effects())
            .iter()
            .filter(|event| event.0 == 1000)
            .count(),
        2
    );
    assert_eq!(
        world.random().seed(),
        seed_after(|random| {
            for odds in 1..=9 {
                let _ = random.next_int(odds);
            }
            throw_draws(random, Direction::East);
        })
    );
}

#[test]
fn bone_meal_on_wheat_matches_grow_crop_and_stone_fails() {
    let target = POS.relative(Direction::East);
    let mut level = TestLevel::new();
    level.set(target.below(), BlockState(blocks::FARMLAND));
    level.set(target, BlockState(blocks::WHEAT));
    let shot = fire_id(&mut level, Direction::East, items::BONE_MEAL);
    let mut twin = TestLevel::new();
    twin.set(
        POS,
        BlockState(blocks::DISPENSER).with(FACING, Direction::East),
    );
    twin.set(target.below(), BlockState(blocks::FARMLAND));
    twin.set(target, BlockState(blocks::WHEAT));
    let mut ticks = BlockTicks::new();
    let mut twin_world = World::new(&mut twin, &mut ticks);
    twin_world.random().next_int(1);
    let grew = if grow_crop(&mut twin_world, target) {
        true
    } else {
        grow_water_plant(&mut twin_world, target)
    };
    assert!(grew);
    assert!(shot.left.is_empty());
    assert!(level_events(&shot.effects).contains(&(1505, target, 15)));
    assert_pair(&level_events(&shot.effects), 1000, Direction::East);
    assert_eq!(shot.seed, twin_world.random().seed());

    let mut level = TestLevel::new();
    level.set(target, BlockState(blocks::STONE));
    let miss = fire_id(&mut level, Direction::East, items::BONE_MEAL);
    assert_eq!(miss.left, ItemStack::new(items::BONE_MEAL, 1));
    assert_pair(&level_events(&miss.effects), 1001, Direction::East);
    assert_eq!(miss.seed, seed_after(slot_roll));
}

#[test]
fn bone_meal_grows_seagrass_in_water() {
    let target = POS.relative(Direction::East);
    let mut level = TestLevel::new();
    level.set(target, BlockState(blocks::WATER));
    let shot = fire_id(&mut level, Direction::East, items::BONE_MEAL);
    let mut twin = TestLevel::new();
    twin.set(
        POS,
        BlockState(blocks::DISPENSER).with(FACING, Direction::East),
    );
    twin.set(target, BlockState(blocks::WATER));
    let mut ticks = BlockTicks::new();
    let mut twin_world = World::new(&mut twin, &mut ticks);
    twin_world.random().next_int(1);
    assert!(!grow_crop(&mut twin_world, target));
    assert!(grow_water_plant(&mut twin_world, target));
    assert!(shot.left.is_empty());
    assert!(level_events(&shot.effects).contains(&(1505, target, 15)));
    assert_eq!(shot.seed, twin_world.random().seed());
    assert_eq!(shot.front, twin_world.block_state(target));
}

#[test]
fn flint_lights_fire_and_a_campfire_and_still_spends_durability_on_stone() {
    let target = POS.relative(Direction::East);
    let mut level = TestLevel::new();
    level.set(target.below(), BlockState(blocks::STONE));
    let fire = fire_id(&mut level, Direction::East, items::FLINT_AND_STEEL);
    assert!(fire.front.is(blocks::FIRE));
    assert_eq!(damage_of(&fire.left), 1);
    assert_pair(&level_events(&fire.effects), 1000, Direction::East);
    let mut twin = TestLevel::new();
    twin.set(
        POS,
        BlockState(blocks::DISPENSER).with(FACING, Direction::East),
    );
    twin.set(target.below(), BlockState(blocks::STONE));
    let mut ticks = BlockTicks::new();
    let mut twin_world = World::new(&mut twin, &mut ticks);
    twin_world.random().next_int(1);
    if crate::behaviour::misc::fire::can_be_placed_at(&twin_world, target, Direction::East) {
        let fire_state = crate::behaviour::misc::fire::state_at(&twin_world, target);
        twin_world.set_block_and_update(target, fire_state);
    }
    assert_eq!(fire.seed, twin_world.random().seed());

    let mut level = TestLevel::new();
    level.set(target, BlockState(blocks::CAMPFIRE).with(LIT, false));
    let campfire = fire_id(&mut level, Direction::East, items::FLINT_AND_STEEL);
    assert!(campfire.front.get(LIT));
    assert_eq!(damage_of(&campfire.left), 1);

    let mut level = TestLevel::new();
    level.set(target, BlockState(blocks::TNT));
    let tnt = fire_id(&mut level, Direction::East, items::FLINT_AND_STEEL);
    assert!(tnt.front.is_air());
    assert_eq!(tnt.primed, 1);
    assert_eq!(damage_of(&tnt.left), 1);
    assert!(has_primed_sound(&tnt.effects));
    assert_pair(&level_events(&tnt.effects), 1000, Direction::East);
    assert_eq!(
        tnt.seed,
        seed_after(|random| {
            slot_roll(random);
            tnt_yaw(random);
        })
    );

    let mut level = TestLevel::new();
    level.set(target, BlockState(blocks::STONE));
    let stone = fire_id(&mut level, Direction::East, items::FLINT_AND_STEEL);
    assert!(stone.front.is(blocks::STONE));
    assert_eq!(damage_of(&stone.left), 1);
    assert_pair(&level_events(&stone.effects), 1000, Direction::East);
}

#[test]
fn unbreaking_flint_draws_one_float_after_the_slot_roll() {
    let &[entry] = mc_bamboo_data::item_damage::ITEM_DAMAGE else {
        panic!("vanilla 26.3 has one item_damage enchantment");
    };
    assert!(!item_is_tagged(
        items::FLINT_AND_STEEL,
        item_tags::ENCHANTABLE_ARMOR
    ));
    let mut stack = ItemStack::new(items::FLINT_AND_STEEL, 1);
    set_component(
        &mut stack,
        kinds::ENCHANTMENTS,
        ComponentValue::Enchantments(vec![(entry.enchantment, 1)]),
    );
    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::STONE));
    let shot = fire_one(&mut level, Direction::East, stack);
    let random = LegacyRandom::new(0);
    slot_roll(&random);
    let saved = random.next_float() < 0.5;
    assert_eq!(shot.seed, random.seed());
    assert_eq!(damage_or_zero(&shot.left), i32::from(!saved));
    assert_eq!(shot.left.count, 1);
    assert_pair(&level_events(&shot.effects), 1000, Direction::East);
}

#[test]
fn tnt_stays_when_the_rule_is_off() {
    let mut level = TestLevel::new();
    level.rules.tnt_explodes = false;
    let item = fire_id(&mut level, Direction::East, items::TNT);
    assert_eq!(item.left, ItemStack::new(items::TNT, 1));
    assert_eq!(item.primed, 0);
    assert!(!has_primed_sound(&item.effects));
    assert_pair(&level_events(&item.effects), 1001, Direction::East);
    assert_eq!(item.seed, seed_after(slot_roll));

    let mut level = TestLevel::new();
    level.rules.tnt_explodes = false;
    level.set(POS.relative(Direction::East), BlockState(blocks::TNT));
    let flint = fire_id(&mut level, Direction::East, items::FLINT_AND_STEEL);
    assert!(flint.front.is(blocks::TNT));
    assert_eq!(flint.primed, 0);
    assert_eq!(flint.left.count, 1);
    assert_eq!(damage_or_zero(&flint.left), 0);
    assert!(!has_primed_sound(&flint.effects));
    assert_pair(&level_events(&flint.effects), 1001, Direction::East);
    assert_eq!(flint.seed, seed_after(slot_roll));
}

#[test]
fn a_shulker_box_places_into_air_and_stays_in_stone() {
    let placed = fire_id(&mut TestLevel::new(), Direction::East, items::SHULKER_BOX);
    assert!(placed.front.is(blocks::SHULKER_BOX));
    assert!(placed.left.is_empty());
    assert_pair(&level_events(&placed.effects), 1000, Direction::East);

    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::STONE));
    let blocked = fire_id(&mut level, Direction::East, items::SHULKER_BOX);
    assert_eq!(blocked.left, ItemStack::new(items::SHULKER_BOX, 1));
    assert!(blocked.front.is(blocks::STONE));
    assert_pair(&level_events(&blocked.effects), 1001, Direction::East);
    assert_eq!(blocked.seed, seed_after(slot_roll));
}

#[test]
fn a_pumpkin_needs_a_golem_base_and_a_wither_skull_needs_the_base() {
    let bare = fire_id(
        &mut TestLevel::new(),
        Direction::East,
        items::CARVED_PUMPKIN,
    );
    assert_eq!(bare.left, ItemStack::new(items::CARVED_PUMPKIN, 1));
    assert!(bare.front.is_air());
    assert_pair(&level_events(&bare.effects), 1001, Direction::East);
    assert_eq!(bare.seed, seed_after(slot_roll));

    let target = POS.relative(Direction::East);
    let mut level = TestLevel::new();
    level.set(target.below(), BlockState(blocks::SNOW_BLOCK));
    level.set(target.below().below(), BlockState(blocks::SNOW_BLOCK));
    let snow = fire_id(&mut level, Direction::East, items::CARVED_PUMPKIN);
    assert!(snow.left.is_empty());
    assert!(snow.front.is_air());
    assert_pair(&level_events(&snow.effects), 1000, Direction::East);

    let skull = fire_id(
        &mut TestLevel::new(),
        Direction::East,
        items::WITHER_SKELETON_SKULL,
    );
    assert_eq!(skull.left, ItemStack::new(items::WITHER_SKELETON_SKULL, 1));
    assert_pair(&level_events(&skull.effects), 1001, Direction::East);

    // 图案原点在头颅这一格，向 +x 展开。`BlockPattern.find` 只往正方向搜。
    // The pattern origin is the skull cell and extends toward +x.
    // `BlockPattern.find` only searches the positive cube.
    let mut level = TestLevel::new();
    for x in 1..4 {
        level.set(BlockPos::new(x, 63, 0), BlockState(blocks::SOUL_SAND));
    }
    level.set(BlockPos::new(2, 62, 0), BlockState(blocks::SOUL_SAND));
    let built = fire_id(&mut level, Direction::East, items::WITHER_SKELETON_SKULL);
    assert!(built.left.is_empty());
    assert_eq!(built.front.get(ROTATION_16), 12);
    assert_pair(&level_events(&built.effects), 1000, Direction::East);
}

#[test]
fn a_glass_bottle_fills_from_water_and_a_hive_and_throws_at_stone() {
    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::WATER));
    let water = fire_id(&mut level, Direction::East, items::GLASS_BOTTLE);
    assert!(water.front.is(blocks::WATER));
    assert_eq!(water.left, water_potion_stack());
    assert_pair(&level_events(&water.effects), 1000, Direction::East);
    assert_eq!(water.seed, seed_after(slot_roll));

    let mut level = TestLevel::new();
    level.set(
        POS.relative(Direction::East),
        BlockState(blocks::BEEHIVE).with(LEVEL_HONEY, 5),
    );
    let honey = fire_id(&mut level, Direction::East, items::GLASS_BOTTLE);
    assert_eq!(honey.front.get(LEVEL_HONEY), 0);
    assert_eq!(honey.left, ItemStack::new(items::HONEY_BOTTLE, 1));
    assert_pair(&level_events(&honey.effects), 1000, Direction::East);

    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::STONE));
    let miss = fire_id(&mut level, Direction::East, items::GLASS_BOTTLE);
    assert!(miss.left.is_empty());
    assert_pair(&level_events(&miss.effects), 1001, Direction::East);
    assert_eq!(
        miss.seed,
        seed_after(|random| {
            slot_roll(random);
            throw_draws(random, Direction::East);
        })
    );
}

#[test]
fn a_water_potion_makes_mud_and_anything_else_nests_a_throw() {
    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::DIRT));
    let mud = fire_one(&mut level, Direction::East, water_potion_stack());
    assert!(mud.front.is(blocks::MUD));
    assert_eq!(mud.left, ItemStack::new(items::GLASS_BOTTLE, 1));
    assert!(!sound_ids(&mud.effects).is_empty());
    assert_pair(&level_events(&mud.effects), 1000, Direction::East);
    assert_eq!(
        mud.seed,
        seed_after(|random| {
            slot_roll(random);
            for _ in 0..10 {
                let _ = random.next_double();
            }
        })
    );

    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::STONE));
    let stone = fire_one(&mut level, Direction::East, water_potion_stack());
    assert!(stone.front.is(blocks::STONE));
    assert!(stone.left.is_empty());
    assert_eq!(
        level_events(&stone.effects)
            .iter()
            .filter(|event| event.0 == 1000)
            .count(),
        2
    );

    let mut mundane = ItemStack::new(items::POTION, 1);
    set_component(
        &mut mundane,
        kinds::POTION_CONTENTS,
        ComponentValue::PotionContents {
            potion: Some(1),
            custom_color: None,
            custom_effects: Vec::new(),
            custom_name: None,
        },
    );
    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::DIRT));
    let other = fire_one(&mut level, Direction::East, mundane);
    assert!(other.front.is(blocks::DIRT));
    assert_eq!(
        level_events(&other.effects)
            .iter()
            .filter(|event| event.0 == 1000)
            .count(),
        2
    );
}

#[test]
fn honeycomb_waxes_copper_and_throws_at_stone_with_success_still_true() {
    let mut level = TestLevel::new();
    level.set(
        POS.relative(Direction::East),
        BlockState(blocks::COPPER_BLOCK),
    );
    let waxed = fire_id(&mut level, Direction::East, items::HONEYCOMB);
    assert!(!waxed.front.is(blocks::COPPER_BLOCK));
    assert!(waxed.left.is_empty());
    assert!(
        level_events(&waxed.effects)
            .iter()
            .any(|event| event.0 == 3003)
    );
    assert_pair(&level_events(&waxed.effects), 1000, Direction::East);

    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::STONE));
    let miss = fire_id(&mut level, Direction::East, items::HONEYCOMB);
    assert!(miss.left.is_empty());
    assert!(miss.front.is(blocks::STONE));
    assert_pair(&level_events(&miss.effects), 1000, Direction::East);
    assert_eq!(
        miss.seed,
        seed_after(|random| {
            slot_roll(random);
            throw_draws(random, Direction::East);
        })
    );
}

#[test]
fn glowstone_charges_an_anchor_until_it_is_full() {
    let mut level = TestLevel::new();
    level.set(
        POS.relative(Direction::East),
        BlockState(blocks::RESPAWN_ANCHOR),
    );
    let charged = fire_id(&mut level, Direction::East, items::GLOWSTONE);
    assert_eq!(charged.front.get(RESPAWN_ANCHOR_CHARGES), 1);
    assert!(charged.left.is_empty());
    assert!(!sound_ids(&charged.effects).is_empty());
    assert_pair(&level_events(&charged.effects), 1000, Direction::East);

    let mut level = TestLevel::new();
    level.set(
        POS.relative(Direction::East),
        BlockState(blocks::RESPAWN_ANCHOR).with(RESPAWN_ANCHOR_CHARGES, 4),
    );
    let full = fire_id(&mut level, Direction::East, items::GLOWSTONE);
    assert_eq!(full.front.get(RESPAWN_ANCHOR_CHARGES), 4);
    assert_eq!(full.left, ItemStack::new(items::GLOWSTONE, 1));
    assert_pair(&level_events(&full.effects), 1001, Direction::East);

    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::STONE));
    let miss = fire_id(&mut level, Direction::East, items::GLOWSTONE);
    assert!(miss.left.is_empty());
    assert_pair(&level_events(&miss.effects), 1000, Direction::East);
}

#[test]
fn shears_harvest_a_full_hive_and_draw_the_drop_offset() {
    let mut level = TestLevel::new();
    level.set(
        POS.relative(Direction::East),
        BlockState(blocks::BEEHIVE).with(LEVEL_HONEY, 5),
    );
    let hive = fire_id(&mut level, Direction::East, items::SHEARS);
    assert_eq!(hive.front.get(LEVEL_HONEY), 0);
    assert_eq!(damage_of(&hive.left), 1);
    assert!(!sound_ids(&hive.effects).is_empty());
    assert_pair(&level_events(&hive.effects), 1000, Direction::East);
    assert_eq!(
        hive.seed,
        seed_after(|random| {
            slot_roll(random);
            pop_resource_offset(random);
        })
    );

    // 蜜脾是一组真的物品实体，位置在蜂巢中心附近，三个位置偏移只取一次。
    let mut level = TestLevel::new();
    level.set(
        POS.relative(Direction::East),
        BlockState(blocks::BEEHIVE).with(LEVEL_HONEY, 5),
    );
    let hive_pos = POS.relative(Direction::East);
    level.set_container(POS, {
        let mut slots = empty(9);
        slots[0] = ItemStack::new(items::SHEARS, 1);
        slots
    });
    let mut ticks = BlockTicks::new();
    let mut world = World::new(&mut level, &mut ticks);
    place_dispenser(&mut world, Direction::East);
    let state = world.block_state(POS);
    dispense_from(&mut world, POS, state);
    let honeycomb: Vec<_> = world
        .entity_ids()
        .into_iter()
        .filter_map(|id| world.entity(id))
        .filter_map(|entity| entity.ground().map(|item| (entity.position, &item.stack)))
        .collect();
    assert_eq!(honeycomb.len(), 1);
    assert_eq!(*honeycomb[0].1, ItemStack::new(items::HONEYCOMB, 3));
    assert!((honeycomb[0].0.x - f64::from(hive_pos.x) - 0.5).abs() <= 0.25);
    assert!((honeycomb[0].0.z - f64::from(hive_pos.z) - 0.5).abs() <= 0.25);

    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::STONE));
    let miss = fire_id(&mut level, Direction::East, items::SHEARS);
    assert_eq!(damage_of(&miss.left), 0);
    assert_pair(&level_events(&miss.effects), 1001, Direction::East);
    assert_eq!(miss.seed, seed_after(slot_roll));
}

#[test]
fn a_brush_fails_when_nothing_is_in_front() {
    let shot = fire_id(&mut TestLevel::new(), Direction::East, items::BRUSH);
    assert_eq!(shot.left, ItemStack::new(items::BRUSH, 1));
    assert_pair(&level_events(&shot.effects), 1001, Direction::East);
    assert_eq!(shot.seed, seed_after(slot_roll));
}

#[test]
fn projectiles_armour_stands_spawn_eggs_and_tnt_take_the_fallback() {
    let arrow = fire_id(&mut TestLevel::new(), Direction::East, items::ARROW);
    assert!(arrow.left.is_empty());
    assert!(
        level_events(&arrow.effects)
            .iter()
            .all(|event| event.0 != 1002)
    );
    assert_eq!(
        arrow.seed,
        seed_after(|random| {
            slot_roll(random);
            throw_draws(random, Direction::East);
        })
    );

    let stand = fire_id(&mut TestLevel::new(), Direction::East, items::ARMOR_STAND);
    assert_eq!(stand.left, ItemStack::new(items::ARMOR_STAND, 1));
    assert_pair(&level_events(&stand.effects), 1000, Direction::East);
    assert_eq!(stand.seed, seed_after(slot_roll));

    let egg = fire_id(
        &mut TestLevel::new(),
        Direction::East,
        items::ZOMBIE_SPAWN_EGG,
    );
    let has_data = defaults::resolve(egg.left.item, &egg.left.patch(), kinds::ENTITY_DATA)
        .is_some()
        || egg.left.is_empty();
    if has_data && !egg.left.is_empty() {
        assert_eq!(egg.left.count, 1);
        assert_pair(&level_events(&egg.effects), 1000, Direction::East);
        assert_eq!(egg.seed, seed_after(slot_roll));
    }

    let tnt = fire_id(&mut TestLevel::new(), Direction::East, items::TNT);
    assert!(tnt.left.is_empty());
    assert!(tnt.front.is_air());
    assert_eq!(tnt.primed, 1);
    assert!(has_primed_sound(&tnt.effects));
    assert_pair(&level_events(&tnt.effects), 1000, Direction::East);
    assert_eq!(
        tnt.seed,
        seed_after(|random| {
            slot_roll(random);
            tnt_yaw(random);
        })
    );
}

#[test]
fn a_boat_and_a_minecart_are_consumed_only_when_they_can_be_placed() {
    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::WATER));
    let boat = fire_id(&mut level, Direction::East, items::OAK_BOAT);
    assert!(boat.left.is_empty());
    assert!(boat.front.is(blocks::WATER));
    assert_eq!(
        level_events(&boat.effects)
            .iter()
            .filter(|event| event.0 == 1000)
            .count(),
        1
    );
    assert_eq!(boat.seed, seed_after(slot_roll));

    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::STONE));
    let grounded = fire_id(&mut level, Direction::East, items::OAK_BOAT);
    assert!(grounded.left.is_empty());
    assert_eq!(
        level_events(&grounded.effects)
            .iter()
            .filter(|event| event.0 == 1000)
            .count(),
        2
    );

    let mut level = TestLevel::new();
    level.set(
        POS.relative(Direction::East).below(),
        BlockState(blocks::STONE),
    );
    level.set(POS.relative(Direction::East), BlockState(blocks::RAIL));
    let cart = fire_id(&mut level, Direction::East, items::MINECART);
    assert!(cart.left.is_empty());
    assert!(cart.front.is(blocks::RAIL));
    assert_eq!(cart.seed, seed_after(slot_roll));

    let mut level = TestLevel::new();
    level.set(POS.relative(Direction::East), BlockState(blocks::STONE));
    let off = fire_id(&mut level, Direction::East, items::MINECART);
    assert!(off.left.is_empty());
    assert_eq!(
        level_events(&off.effects)
            .iter()
            .filter(|event| event.0 == 1000)
            .count(),
        2
    );
}

#[test]
fn armour_equips_the_first_living_target_in_the_front_cell() {
    let target = POS.relative(Direction::East);
    let mut level = TestLevel::new();
    level
        .living
        .push(player_at(target, false, ItemStack::empty()));
    let worn = fire_id(&mut level, Direction::East, items::LEATHER_HELMET);
    assert!(worn.left.is_empty());
    assert_eq!(
        level.living[0].equipment[4]
            .as_ref()
            .map(|stack| stack.item),
        Some(items::LEATHER_HELMET)
    );
    assert!(worn.effects.iter().any(|effect| matches!(
        effect,
        Effect::DispenserEquipped {
            entity_id: 7,
            menu_slot: 4
        }
    )));
    assert_pair(&level_events(&worn.effects), 1000, Direction::East);
    assert_eq!(worn.seed, seed_after(slot_roll));

    let mut level = TestLevel::new();
    level
        .living
        .push(player_at(target, true, ItemStack::empty()));
    let spectator = fire_id(&mut level, Direction::East, items::LEATHER_HELMET);
    assert!(spectator.left.is_empty());
    assert!(
        spectator
            .effects
            .iter()
            .all(|effect| !matches!(effect, Effect::DispenserEquipped { .. }))
    );
    assert_eq!(
        spectator.seed,
        seed_after(|random| {
            slot_roll(random);
            throw_draws(random, Direction::East);
        })
    );

    let mut level = TestLevel::new();
    level
        .living
        .push(player_at(target, false, ItemStack::new(items::STONE, 1)));
    let occupied = fire_id(&mut level, Direction::East, items::LEATHER_HELMET);
    assert!(occupied.left.is_empty());
    assert_eq!(
        level.living[0].equipment[4]
            .as_ref()
            .map(|stack| stack.item),
        Some(items::STONE)
    );

    let mut level = TestLevel::new();
    level
        .living
        .push(player_at(POS.offset(4, 0, 0), false, ItemStack::empty()));
    let away = fire_id(&mut level, Direction::East, items::LEATHER_HELMET);
    assert!(away.left.is_empty());
    assert!(
        away.effects
            .iter()
            .all(|effect| !matches!(effect, Effect::DispenserEquipped { .. }))
    );
}
