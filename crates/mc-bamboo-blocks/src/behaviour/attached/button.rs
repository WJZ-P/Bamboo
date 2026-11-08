//! `ButtonBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use super::{attached_direct_signal, attached_own_signal, update_attached_neighbours};
use crate::{
    behaviour::{
        Behaviour,
        common::{FaceAttached, field_int},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, POWERED},
    world::World,
};

/// `ButtonBlock`。
pub(super) struct Button;

impl Behaviour for Button {
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
        if state.get(POWERED) {
            return InteractionResult::Consume;
        }
        let pos = ctx.clicked_pos();
        let block = state.block();
        world.set_block_and_update(pos, state.with(POWERED, true));
        update_attached_neighbours(state, world, pos);
        world.schedule_tick(pos, block, field_int(block, "ticksToStayPressed"));
        InteractionResult::Success
    }

    /// `checkPressed`。官方还要看按钮上有没有箭，这个世界没有实体，到时间就弹回来。
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(POWERED) {
            world.set_block_and_update(pos, state.with(POWERED, false));
            update_attached_neighbours(state, world, pos);
        }
    }

    fn own_signal(&self, state: BlockState, _world: &World, _pos: BlockPos) -> i32 {
        attached_own_signal(state)
    }

    fn direct_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        attached_direct_signal(state, direction)
    }
}
