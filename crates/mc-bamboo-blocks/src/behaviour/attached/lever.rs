//! `LeverBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, sounds::sound_event};

use super::{attached_direct_signal, attached_own_signal, update_attached_neighbours};
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
        // `LeverBlock.playSound`：拉下的音高是 0.6，弹回是 0.5。`useWithoutItem` 传的玩家是
        // `null`，所以发起者自己也收得到。
        let pitch = if pulled.get(POWERED) { 0.6 } else { 0.5 };
        world.play_block_sound(pos, sound_event("minecraft:block.lever.click"), 0.3, pitch);
        InteractionResult::Success
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
