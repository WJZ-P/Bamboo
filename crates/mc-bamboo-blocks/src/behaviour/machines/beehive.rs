//! `BeehiveBlock`。
//!
//! 蜜蜂在方块实体里，这里的蜂巢总是空的：`updateShape` 和 `playerWillDestroy` 里放蜜蜂、
//! 掉落的部分没有东西要做。采蜜后不管有没有烟，蜜都清零，有烟只是不放蜜蜂。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, items};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    behaviour::{Behaviour, Plain, common::facing_the_player},
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, LEVEL_HONEY},
    world::World,
};

/// `BeehiveBlock.MAX_HONEY_LEVELS`。
const MAX_HONEY_LEVELS: i32 = 5;

/// `BeehiveBlock`。
pub(super) struct Beehive;

impl Behaviour for Beehive {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_player(block, ctx))
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let harvests = ctx.item == items::SHEARS || ctx.item == items::GLASS_BOTTLE;
        if state.get(LEVEL_HONEY) >= MAX_HONEY_LEVELS && harvests {
            // `HARVEST_BEEHIVE` 是常量 3 个蜜脾，`popResource` 在改方块之前。玻璃瓶不掉蜜脾。
            // `HARVEST_BEEHIVE` is a constant 3 honeycomb. `popResource` runs before the block changes.
            // A glass bottle does not drop honeycomb.
            if ctx.item == items::SHEARS {
                world.pop_resource(ctx.clicked_pos(), ItemStack::new(items::HONEYCOMB, 3));
            }
            world.set_block_and_update(ctx.clicked_pos(), state.with(LEVEL_HONEY, 0));
            return InteractionResult::Success;
        }
        Plain.use_item_on(state, world, ctx)
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        state.get(LEVEL_HONEY)
    }
}
