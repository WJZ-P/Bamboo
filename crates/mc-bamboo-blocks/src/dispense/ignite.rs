//! `FlintAndSteelDispenseItemBehavior`。
//!
//! 一开始就把成功设为真。前方没有硫磺立方，`tryIgniteExplosiveEntities` 直接返回假，不取随机数。
//! 能生火就生火，否则点着营火、蜡烛或蜡烛蛋糕。前方是 TNT 方块时走 `TntBlock.prime`：
//! 规则为真就生成点燃的 TNT、放 `entity.tnt.primed`，再 `removeBlock`；规则为假时 `prime` 返回假，
//! 不取朝向，成功改回假，方块留着。成功才 `hurtAndBreak(1)`。什么都点不着时成功仍是真，照样扣耐久。
//!
//! `FlintAndSteelDispenseItemBehavior`.
//!
//! Success starts true. There is no sulfur cube in front, so `tryIgniteExplosiveEntities` returns
//! false and draws nothing. Fire is placed when it can be, otherwise a campfire, candle or candle
//! cake is lit. A TNT block follows `TntBlock.prime`: when the rule is true it spawns primed TNT,
//! plays `entity.tnt.primed`, then `removeBlock`. When the rule is false `prime` returns false,
//! draws no yaw, success goes back to false, and the block stays. `hurtAndBreak(1)` runs only on
//! success. When nothing lights, success stays true and durability is still spent.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{BlockAncestor, items};
use mc_bamboo_protocol::item::ItemStack;

use super::Shot;
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
        // `TntBlock.prime` 成功才 `removeBlock`。规则为假时不取朝向的 `nextDouble`。
        // 发射器的来源是 null，冒险模式那一截不会走进去。
        //
        // `removeBlock` runs only when `TntBlock.prime` succeeds. A false rule draws no yaw
        // `nextDouble`. The dispenser source is null, so the adventure-mode check does not run.
        if world.spawn_tnt(target) >= 0 {
            world.remove_block(target);
        } else {
            success = false;
        }
    }
    let stack = if success {
        crate::durability::hurt_and_break(stack, 1, world.random())
    } else {
        stack
    };
    Shot::optional(stack, success)
}
