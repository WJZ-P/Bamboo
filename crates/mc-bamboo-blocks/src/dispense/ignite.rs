//! `FlintAndSteelDispenseItemBehavior`。
//!
//! 一开始就把成功设为真。前方没有硫磺立方。能生火就生火，否则点着营火、蜡烛或蜡烛蛋糕。
//! 前方是 TNT 方块时官方 `TntBlock.prime` 会生成实体，这里走 `prime` 返回假的分支：
//! 成功改回假，方块留着，不扣耐久。什么都点不着时成功仍是真，照样扣 1 点耐久。
//!
//! `FlintAndSteelDispenseItemBehavior`.
//!
//! Success starts true. There is no sulfur cube in front. Fire is placed when it can be,
//! otherwise a campfire, candle or candle cake is lit. A TNT block would be primed into an
//! entity. That follows the branch where `prime` returns false: success goes back to false,
//! the block stays, and durability is not spent. When nothing lights, success stays true and
//! one point of durability is still spent.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{BlockAncestor, items};
use mc_bamboo_protocol::item::ItemStack;

use super::{Shot, durability::hurt_and_break};
use crate::{
    behaviour::misc::fire::{can_be_placed_at, state_at},
    item_use::can_light,
    state::LIT,
    world::World,
};

pub(super) fn handles(item: i32) -> bool {
    item == items::FLINT_AND_STEEL
}

pub(super) fn execute(
    world: &mut World,
    pos: BlockPos,
    facing: Direction,
    stack: ItemStack,
) -> Shot {
    let target = pos.relative(facing);
    let state = world.block_state(target);
    let mut success = true;
    if can_be_placed_at(world, target, facing) {
        world.set_block_and_update(target, state_at(world, target));
    } else if can_light(state) {
        world.set_block_and_update(target, state.with(LIT, true));
    } else if state.is_a(BlockAncestor::TntBlock) {
        // `TntBlock.prime` 生成 `PrimedTnt`。生成失败时官方把成功改回假，不拆方块。
        //
        // `TntBlock.prime` spawns a `PrimedTnt`. When that returns false, official sets
        // success back to false and leaves the block.
        success = false;
    }
    let stack = if success {
        hurt_and_break(stack, 1)
    } else {
        stack
    };
    Shot::optional(stack, success)
}
