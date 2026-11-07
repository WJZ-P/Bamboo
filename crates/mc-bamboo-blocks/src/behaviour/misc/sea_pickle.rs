//! `SeaPickleBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, is_water_source, tick_water_if_waterlogged, vegetation_update_shape,
        },
    },
    context::PlaceContext,
    shape::{self, CollisionContext},
    state::{BlockState, PICKLES, SupportType, WATERLOGGED},
    world::World,
};

/// `SeaPickleBlock`。
pub(super) struct SeaPickle;

/// `MAX_PICKLES`。
const MAX_PICKLES: i32 = 4;

/// `mayPlaceOn`：顶上有碰撞面就行，不用是完整的面。
fn may_place_on(world: &World, pos: BlockPos) -> bool {
    let state = world.block_state(pos);
    let collision = state.collision_boxes(world, pos, CollisionContext::Empty);
    !shape::face_shape(&collision, Direction::Up).is_empty()
        || world.is_face_sturdy(pos, Direction::Up, SupportType::Full)
}

impl Behaviour for SeaPickle {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = ctx.replaced_state(world);
        if state.is(block.default_state) {
            let pickles = state.get(PICKLES) + 1;
            return Some(state.with(PICKLES, pickles.min(MAX_PICKLES)));
        }
        let waterlogged = is_water_source(world, ctx.clicked_pos());
        Some(default_state(block).with(WATERLOGGED, waterlogged))
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, world: &World) -> bool {
        let adds_a_pickle = !ctx.is_secondary_use_active()
            && ctx.item == state.block().item
            && state.get(PICKLES) < MAX_PICKLES;
        adds_a_pickle || Plain.can_be_replaced(state, ctx, world)
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        may_place_on(world, pos.below())
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        if !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        tick_water_if_waterlogged(state, world, pos);
        vegetation_update_shape(state, world, pos)
    }
}
