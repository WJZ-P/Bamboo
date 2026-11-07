//! `HugeMushroomBlock`：蘑菇方块和蘑菇柄。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, pipe_property},
    },
    context::PlaceContext,
    state::BlockState,
    world::World,
};

/// `HugeMushroomBlock`：贴着同种方块的面不显示外皮。
pub(super) struct HugeMushroom;

impl Behaviour for HugeMushroom {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let default = default_state(block);
        let state = Direction::ALL.into_iter().fold(default, |state, side| {
            let same_block = world.block_state(pos.relative(side)).is_same_block(default);
            state.with(pipe_property(side), !same_block)
        });
        Some(state)
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
        if neighbor.is_same_block(state) {
            state.with(pipe_property(direction), false)
        } else {
            Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
        }
    }
}
