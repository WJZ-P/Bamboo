//! 蜜脾、荧石、剪刀和刷子。
//!
//! 蜜脾涂得上蜡时发 3003 和涂蜡的声音，不处理玩家路径里双箱子的第二声。涂不上就抛一次，成功仍是真。
//! 荧石对着充能不满的重生锚才充能。满了成功是假。不是锚就抛一次，成功仍是真。
//! 剪刀只收满蜜的蜂巢。蜜脾掉落的表走自己的 `random_sequence`，不碰 `level.random`；
//! `popResource` 仍从 `level.random` 取三次位置偏移，蜜脾实体不生成。没有羊可剪，
//! 所以蜂巢失败就是失败。刷子前面没有犰狳，成功是假，物品留着。
//!
//! Honeycomb, glowstone, shears and the brush.
//!
//! Waxed copper emits 3003 and the wax sound. The player's extra sound on the other half of a
//! double chest is not part of this path. A miss throws once and success stays true.
//! Glowstone charges a respawn anchor that is not full. A full anchor fails. Anything else
//! throws once and success stays true.
//! Shears only harvest a full hive. The honeycomb loot table uses its own `random_sequence`
//! and does not touch `level.random`. `popResource` still draws three position offsets from
//! `level.random`. The honeycomb entity is not spawned. There is no sheep to shear, so a hive
//! miss is a failure. A brush with no armadillo fails and keeps the item.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{BlockAncestor, blocks, items, sounds::sound_event, tags::block_tags};
use mc_bamboo_protocol::item::ItemStack;

use super::{Shot, durability::hurt_and_break, throw_one};
use crate::{
    dispenser::without_one,
    item_use::waxed,
    state::{BlockState, LEVEL_HONEY, RESPAWN_ANCHOR_CHARGES},
    world::World,
};

/// `RespawnAnchorBlock.MAX_CHARGES`。
const MAX_CHARGES: i32 = 4;

pub(super) fn handles(item: i32) -> bool {
    matches!(
        item,
        items::HONEYCOMB | items::GLOWSTONE | items::SHEARS | items::BRUSH
    )
}

pub(super) fn execute(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: ItemStack,
) -> Shot {
    match stack.item {
        items::HONEYCOMB => honeycomb(world, pos, facing, stack),
        items::GLOWSTONE => glowstone(world, pos, facing, stack),
        items::SHEARS => shears(world, pos, facing, stack),
        _ => Shot::optional(stack, false),
    }
}

fn honeycomb(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let target = pos.relative(facing);
    let Some(waxed) = waxed(world.block_state(target)) else {
        return Shot::optional(throw_one(world, stack), true);
    };
    world.set_block_and_update(target, waxed);
    world.level_event(3003, target, 0);
    world.play_block_sound(
        target,
        sound_event("minecraft:item.honeycomb.wax_on"),
        1.0,
        1.0,
    );
    Shot::optional(without_one(&stack), true)
}

/// `RespawnAnchorBlock.charge`。满了不减物品。不是锚时 `super.execute`，成功标志已经是真。
///
/// `RespawnAnchorBlock.charge`. A full anchor does not shrink the item. Anything else is
/// `super.execute`, and success was already set true.
fn glowstone(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let target = pos.relative(facing);
    let state = world.block_state(target);
    if state.is(blocks::RESPAWN_ANCHOR) {
        if state.get(RESPAWN_ANCHOR_CHARGES) != MAX_CHARGES {
            charge(world, target, state);
            Shot::optional(without_one(&stack), true)
        } else {
            Shot::optional(stack, false)
        }
    } else {
        Shot::optional(throw_one(world, stack), true)
    }
}

fn charge(world: &mut World, pos: BlockPos, state: BlockState) {
    let charges = state.get(RESPAWN_ANCHOR_CHARGES) + 1;
    world.set_block_and_update(pos, state.with(RESPAWN_ANCHOR_CHARGES, charges));
    world.play_block_sound(
        pos,
        sound_event("minecraft:block.respawn_anchor.charge"),
        1.0,
        1.0,
    );
}

/// `ShearsDispenseItemBehavior`。蜂巢成功才扣 1 点耐久。
///
/// `ShearsDispenseItemBehavior`. Durability is spent only when the hive succeeds.
fn shears(world: &mut World, pos: BlockPos, facing: Direction, stack: ItemStack) -> Shot {
    let target = pos.relative(facing);
    if !shear_hive(world, target) {
        return Shot::optional(stack, false);
    }
    Shot::optional(hurt_and_break(stack, 1), true)
}

/// `tryShearBeehive`。声音在掉落之前，坐标是蜂巢。
///
/// `tryShearBeehive`. The sound plays before the drop, at the hive.
fn shear_hive(world: &mut World, pos: BlockPos) -> bool {
    let state = world.block_state(pos);
    let full = state.is_tagged(block_tags::BEEHIVES)
        && state.is_a(BlockAncestor::BeehiveBlock)
        && state.get(LEVEL_HONEY) >= 5;
    if !full {
        return false;
    }
    world.play_block_sound(pos, sound_event("minecraft:block.beehive.shear"), 1.0, 1.0);
    // `dropHoneycomb`：表是 `minecraft:harvest/beehive`，固定一组 3 个蜜脾，随机序列不碰
    // `level.random`。接着 `popResource` 为这一组取三次位置偏移。物品实体不生成。
    //
    // `dropHoneycomb`: table `minecraft:harvest/beehive` is one stack of 3. Its random
    // sequence does not touch `level.random`. `popResource` then draws three position
    // offsets for that stack. The item entity is not spawned.
    pop_resource_offset(world);
    world.set_block_and_update(pos, state.with(crate::state::LEVEL_HONEY, 0));
    true
}

/// `Block.popResource` 里的三次 `Mth.nextDouble(random, -0.25, 0.25)`。
///
/// 三次都在 `min < max` 时各调用一次 `nextDouble`，发生在物品实体构造之前。
///
/// The three `Mth.nextDouble(random, -0.25, 0.25)` calls in `Block.popResource`.
///
/// Each one calls `nextDouble` once because `min < max`, and that happens before the
/// item entity is constructed.
fn pop_resource_offset(world: &mut World) {
    let _x = world.random().next_double();
    let _y = world.random().next_double();
    let _z = world.random().next_double();
}
