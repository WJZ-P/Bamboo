//! `TurtleEggBlock`。踩碎和孵化都和实体、随机刻有关，不在范围里。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{Block, tags::block_tags};

use crate::{
    behaviour::{Behaviour, Plain, common::default_state},
    context::PlaceContext,
    state::{BlockState, EGGS},
    world::World,
};

/// `TurtleEggBlock`。
pub(super) struct TurtleEgg;

/// `MAX_EGGS`。
const MAX_EGGS: i32 = 4;

impl Behaviour for TurtleEgg {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = ctx.replaced_state(world);
        if state.is(block.default_state) {
            let eggs = state.get(EGGS) + 1;
            Some(state.with(EGGS, eggs.min(MAX_EGGS)))
        } else {
            Some(default_state(block))
        }
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, world: &World) -> bool {
        let adds_an_egg = !ctx.is_secondary_use_active()
            && ctx.item == state.block().item
            && state.get(EGGS) < MAX_EGGS;
        adds_an_egg || Plain.can_be_replaced(state, ctx, world)
    }

    /// `onPlace`：放在沙子上时发 2012 号粒子事件。
    fn on_place(&self, _state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        if world.block_state(pos.below()).is_tagged(block_tags::SAND) {
            world.level_event(2012, pos, 15);
        }
    }
}
