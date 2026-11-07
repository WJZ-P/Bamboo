//! `AmethystClusterBlock`：紫晶芽和紫晶簇。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    state::{BlockState, FACING, SupportType, WATERLOGGED},
    world::World,
};

/// `AmethystClusterBlock`。
pub(super) struct AmethystCluster;

impl Behaviour for AmethystCluster {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let waterlogged = is_water_source(world, ctx.clicked_pos());
        Some(
            default_state(block)
                .with(WATERLOGGED, waterlogged)
                .with(FACING, ctx.clicked_face()),
        )
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let facing = state.get(FACING);
        world.is_face_sturdy(pos.relative(facing.opposite()), facing, SupportType::Full)
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
        tick_water_if_waterlogged(state, world, pos);
        if direction == state.get(FACING).opposite() && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}
