//! `CocoaBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, tags::block_tags};

use crate::{
    behaviour::{Behaviour, Plain, common::default_state},
    context::PlaceContext,
    state::{BlockState, HORIZONTAL_FACING},
    world::World,
};

/// `CocoaBlock`：朝向就是贴着的树干的方向。
pub(super) struct Cocoa;

impl Behaviour for Cocoa {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let default = default_state(block);
        ctx.nearest_looking_directions()
            .into_iter()
            .filter(|direction| direction.axis().is_horizontal())
            .map(|direction| default.with(HORIZONTAL_FACING, direction))
            .find(|state| state.can_survive(world, ctx.clicked_pos()))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        world
            .block_state(pos.relative(state.get(HORIZONTAL_FACING)))
            .is_tagged(block_tags::SUPPORTS_COCOA)
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
        if direction == state.get(HORIZONTAL_FACING) && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}
