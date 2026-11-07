//! `LadderBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, has_sturdy_wall, is_water_source, lost_wall_support,
            tick_water_if_waterlogged, wall_state_for_placement,
        },
    },
    context::PlaceContext,
    state::{BlockState, HORIZONTAL_FACING, WATERLOGGED},
    world::World,
};

/// `LadderBlock`。
pub(super) struct Ladder;

/// 点在一架梯子的正面时放不了：新的梯子会背对着它。
fn clicked_front_of_a_ladder(block: &Block, ctx: &PlaceContext, world: &World) -> bool {
    if ctx.replacing_clicked_on_block() {
        return false;
    }
    let face = ctx.clicked_face();
    let behind = world.block_state(ctx.clicked_pos().relative(face.opposite()));
    behind.is_same_block(default_state(block)) && behind.get(HORIZONTAL_FACING) == face
}

impl Behaviour for Ladder {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        if clicked_front_of_a_ladder(block, ctx, world) {
            return None;
        }
        let water = is_water_source(world, ctx.clicked_pos());
        wall_state_for_placement(default_state(block), ctx, world)
            .map(|state| state.with(WATERLOGGED, water))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        has_sturdy_wall(state, world, pos)
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
        if lost_wall_support(state, world, pos, direction) {
            return BlockState::AIR;
        }
        tick_water_if_waterlogged(state, world, pos);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}
