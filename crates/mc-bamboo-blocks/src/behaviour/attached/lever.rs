//! `LeverBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use super::update_attached_neighbours;
use crate::{
    behaviour::{Behaviour, common::FaceAttached},
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, POWERED},
    world::World,
};

/// `LeverBlock`。
pub(super) struct Lever;

impl Behaviour for Lever {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        FaceAttached.state_for_placement(block, ctx, world)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        FaceAttached.can_survive(state, world, pos)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        FaceAttached.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(POWERED) {
            update_attached_neighbours(state, world, pos);
        }
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let pos = ctx.clicked_pos();
        let pulled = state.cycle(POWERED);
        world.set_block_and_update(pos, pulled);
        update_attached_neighbours(pulled, world, pos);
        InteractionResult::Success
    }
}
