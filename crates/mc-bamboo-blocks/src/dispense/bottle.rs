//! 玻璃瓶和水瓶。
//!
//! 玻璃瓶先看满蜜的蜂巢，再看水标签里的流体。水不消失。失败时 `super.execute` 只抛一次，
//! 成功标志保持假，所以外层是 1001。装到的东西用 `consumeWithRemainder`，游戏事件记在发射器那一格。
//! 水瓶不是水，或者前方不是能变成泥巴的方块时，套一层默认抛出。否则在发射器顶上取 10 个
//! `nextDouble`（溅射粒子，这里不发包），放空瓶的声音，把目标换成泥巴，剩下玻璃瓶。
//!
//! Glass bottles and water potions.
//!
//! A glass bottle checks a full hive first, then a fluid in the water tag. The water stays.
//! Failure is `super.execute`, one throw, and success stays false, so the outer sound is 1001.
//! A filled result uses `consumeWithRemainder`. The game event is recorded at the dispenser.
//! A potion that is not water, or a target that cannot become mud, nests a default throw.
//! Otherwise ten `nextDouble` draws happen above the dispenser (splash particles, not sent),
//! the empty-bottle sound plays, the target becomes mud, and a glass bottle is left.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{
    BlockAncestor, blocks, items,
    registries::data_component_type as kinds,
    sounds::sound_event,
    tags::{block_tags, fluid_tags},
};
use mc_bamboo_protocol::{
    components::{ComponentValue, defaults},
    item::ItemStack,
};

use super::{Shot, consume_with_remainder, nested_default, throw_one};
use crate::{
    item_exchange::{water_potion_id, water_potion_stack},
    state::{BlockState, LEVEL_HONEY},
    world::World,
};

/// `BeehiveBlock.MAX_HONEY_LEVELS`。
const FULL_HONEY: i32 = 5;

pub(super) fn handles(item: i32) -> bool {
    item == items::GLASS_BOTTLE || item == items::POTION
}

pub(super) fn execute(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: ItemStack,
) -> Shot {
    if stack.item == items::GLASS_BOTTLE {
        glass_bottle(world, pos, facing, stack)
    } else {
        water_potion(world, pos, facing, stack)
    }
}

fn glass_bottle(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let target = pos.relative(facing);
    let state = world.block_state(target);
    if full_hive(state) {
        world.set_block_and_update(target, state.with(LEVEL_HONEY, 0));
        let left = take_liquid(
            world,
            pos,
            facing,
            stack,
            ItemStack::new(items::HONEY_BOTTLE, 1),
        );
        return Shot::optional(left, true);
    }
    if world.fluid_state(target).is_tagged(fluid_tags::WATER) {
        let left = take_liquid(world, pos, facing, stack, water_potion_stack());
        return Shot::optional(left, true);
    }
    Shot::optional(throw_one(world, stack), false)
}

fn full_hive(state: BlockState) -> bool {
    state.is_tagged(block_tags::BEEHIVES)
        && state.is_a(BlockAncestor::BeehiveBlock)
        && state.get(LEVEL_HONEY) >= FULL_HONEY
}

/// `takeLiquid`：游戏事件在发射器的位置，不在目标格。这里没有游戏事件可发。
fn take_liquid(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: ItemStack,
    filled: ItemStack,
) -> ItemStack {
    consume_with_remainder(world, pos, facing, stack, filled)
}

fn water_potion(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let target = pos.relative(facing);
    if !is_water(&stack)
        || !world
            .block_state(target)
            .is_tagged(block_tags::CONVERTIBLE_TO_MUD)
    {
        return Shot::plain(nested_default(world, pos, facing, stack));
    }
    // 五个溅射粒子，每个取两个 `nextDouble`。粒子不发，数照取。
    //
    // Five splash particles, two `nextDouble` draws each. The particles are not sent.
    for _ in 0..5 {
        let _x = world.random().next_double();
        let _z = world.random().next_double();
    }
    world.play_block_sound(pos, sound_event("minecraft:item.bottle.empty"), 1.0, 1.0);
    world.set_block_and_update(target, BlockState(blocks::MUD));
    let left = consume_with_remainder(
        world,
        pos,
        facing,
        stack,
        ItemStack::new(items::GLASS_BOTTLE, 1),
    );
    Shot::plain(left)
}

/// `PotionContents.is(Potions.WATER)`。没有药水组件时官方用空内容，不算水。
fn is_water(stack: &ItemStack) -> bool {
    let Some(water) = water_potion_id() else {
        return false;
    };
    match defaults::resolve(stack.item, &stack.patch(), kinds::POTION_CONTENTS) {
        Some(ComponentValue::PotionContents {
            potion: Some(id), ..
        }) => *id == water,
        _ => false,
    }
}
