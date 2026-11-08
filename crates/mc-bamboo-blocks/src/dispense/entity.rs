//! 会生成实体的注册表行为。实体不生成，控制流走官方在生成失败或条件不够时的分支。
//!
//! 船和矿车放得下时只扣物品，不取 `level.random`，外层一次 1000 和 2000。放不下时套一层默认抛出。
//! 盔甲架和带实体数据的刷怪蛋走 `spawn` 返回 null：物品留着，一次 1000。
//! 没有实体数据的刷怪蛋是默认抛出。TNT 物品走 `tntExplodes` 为假：物品留着，1001。
//! 箭、蛋、雪球、药水、烟花、火焰弹和风弹走默认抛出。官方那些用的是弹射物事件，不是这套随机数。
//!
//! Registry behaviours that spawn an entity. The entity is not created. Control follows the
//! branch official uses when the spawn fails or the condition is not met.
//!
//! A boat or minecart that can be placed only consumes the item. It does not draw from
//! `level.random`, and the outer dispense plays 1000 and 2000 once. Otherwise it nests a
//! default throw. An armour stand, and a spawn egg that carries entity data, follow `spawn`
//! returning null: the item stays and 1000 plays once. A spawn egg without entity data is a
//! default throw. A TNT item follows `tntExplodes == false`: the item stays and 1001 plays.
//! Arrows, eggs, snowballs, potions, fireworks, fire charges and wind charges take the default
//! throw. Official uses a projectile event for those, not these random draws.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{
    items, registries::data_component_type as kinds, tags::block_tags, tags::fluid_tags,
};
use mc_bamboo_protocol::{components::defaults, item::ItemStack};

use super::{Shot, nested_default, throw_one};
use crate::{behaviour::common::item_of, dispenser::without_one, world::World};

pub(super) fn handles(item: i32) -> bool {
    is_projectile(item)
        || is_boat(item)
        || is_minecart(item)
        || item == items::ARMOR_STAND
        || item == items::TNT
}

pub(super) fn execute(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: ItemStack,
) -> Shot {
    let item = stack.item;
    if is_projectile(item) {
        return Shot::plain(throw_one(world, stack));
    }
    if item == items::ARMOR_STAND {
        // `EntityType.spawn` 返回 null：不减数量。外层仍放 1000 和 2000。
        //
        // `EntityType.spawn` returned null: the count stays. The outer dispense still plays
        // 1000 and 2000.
        return Shot::plain(stack);
    }
    if item == items::TNT {
        // `GameRules.TNT_EXPLODES` 为假时成功是假，物品留着。这里没有这条规则，引爆会生成实体。
        //
        // When `GameRules.TNT_EXPLODES` is false, success is false and the item stays.
        // There is no game-rule store here, and priming would spawn an entity.
        return Shot::optional(stack, false);
    }
    if is_boat(item) {
        return boat(world, pos, facing, stack);
    }
    minecart(world, pos, facing, stack)
}

pub(super) fn is_spawn_egg(item: i32) -> bool {
    item_name(item).is_some_and(|name| name.ends_with("_spawn_egg"))
}

/// 带 `ENTITY_DATA` 时走 `spawn` 返回 null：物品留着，不取 `nextFloat`。
/// 没有这份数据时是默认抛出。
///
/// With `ENTITY_DATA`, follow `spawn` returning null: the item stays and `nextFloat` is not drawn.
/// Without that component this is a default throw.
pub(super) fn spawn_egg(world: &mut World, stack: ItemStack) -> Shot {
    let has_data = defaults::resolve(stack.item, &stack.patch(), kinds::ENTITY_DATA).is_some();
    if !has_data {
        return Shot::plain(throw_one(world, stack));
    }
    // `type.spawn` 返回 null，或者 `getType` 是 null：物品原样返回，不取 `nextFloat`。
    //
    // `type.spawn` returned null, or `getType` is null: the stack is returned unchanged.
    // The `nextFloat` is not drawn.
    Shot::plain(stack)
}

/// `BoatDispenseItemBehavior`。前方的流体是水，或者前方是空气且下面是水，才算放得下。
///
/// `BoatDispenseItemBehavior`. It can be placed when the front fluid is water, or the front
/// block is air and the block below is water.
fn boat(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let front = pos.relative(facing);
    let on_water = world.fluid_state(front).is_tagged(fluid_tags::WATER)
        || world.block_state(front).is_air()
            && world
                .fluid_state(front.below())
                .is_tagged(fluid_tags::WATER);
    if on_water {
        Shot::plain(without_one(&stack))
    } else {
        Shot::plain(nested_default(world, pos, facing, stack))
    }
}

/// `MinecartDispenseItemBehavior`。前方是铁轨，或者前方是空气且下面是铁轨。
///
/// `MinecartDispenseItemBehavior`. The front is a rail, or the front is air and the block
/// below is a rail.
fn minecart(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let front = pos.relative(facing);
    let state = world.block_state(front);
    let on_rail = state.is_tagged(block_tags::RAILS)
        || state.is_air()
            && world
                .block_state(front.below())
                .is_tagged(block_tags::RAILS);
    if on_rail {
        Shot::plain(without_one(&stack))
    } else {
        Shot::plain(nested_default(world, pos, facing, stack))
    }
}

fn is_projectile(item: i32) -> bool {
    matches!(
        item,
        items::ARROW
            | items::TIPPED_ARROW
            | items::SPECTRAL_ARROW
            | items::EGG
            | items::BLUE_EGG
            | items::BROWN_EGG
            | items::SNOWBALL
            | items::EXPERIENCE_BOTTLE
            | items::SPLASH_POTION
            | items::LINGERING_POTION
            | items::FIREWORK_ROCKET
            | items::FIRE_CHARGE
            | items::WIND_CHARGE
    )
}

/// `bootStrap` 里登记过的船和竹筏。没登记的物品不能因为名字像船就被扣掉。
///
/// Boats and rafts registered in `bootStrap`. An unregistered item is not consumed
/// just because its name looks like a boat.
fn is_boat(item: i32) -> bool {
    matches!(
        item,
        items::OAK_BOAT
            | items::SPRUCE_BOAT
            | items::BIRCH_BOAT
            | items::JUNGLE_BOAT
            | items::DARK_OAK_BOAT
            | items::ACACIA_BOAT
            | items::CHERRY_BOAT
            | items::MANGROVE_BOAT
            | items::POPLAR_BOAT
            | items::PALE_OAK_BOAT
            | items::BAMBOO_RAFT
            | items::OAK_CHEST_BOAT
            | items::SPRUCE_CHEST_BOAT
            | items::BIRCH_CHEST_BOAT
            | items::JUNGLE_CHEST_BOAT
            | items::DARK_OAK_CHEST_BOAT
            | items::ACACIA_CHEST_BOAT
            | items::CHERRY_CHEST_BOAT
            | items::MANGROVE_CHEST_BOAT
            | items::POPLAR_CHEST_BOAT
            | items::PALE_OAK_CHEST_BOAT
            | items::BAMBOO_CHEST_RAFT
    )
}

/// `bootStrap` 里登记过的矿车。刷怪笼矿车不在表里，走默认抛出。
///
/// Minecarts registered in `bootStrap`. A spawner minecart is not in the table and uses
/// the default throw.
fn is_minecart(item: i32) -> bool {
    matches!(
        item,
        items::MINECART
            | items::CHEST_MINECART
            | items::FURNACE_MINECART
            | items::TNT_MINECART
            | items::HOPPER_MINECART
            | items::COMMAND_BLOCK_MINECART
    )
}

fn item_name(item: i32) -> Option<&'static str> {
    item_of(item).map(|item| item.name)
}
