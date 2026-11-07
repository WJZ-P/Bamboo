//! `ScaffoldingBlock`。放置目标格的算法在 [`placement`](crate::placement) 里，距离在 `common` 里。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use super::fall;
use crate::{
    behaviour::{
        Behaviour,
        common::{
            SCAFFOLDING_MAX_DISTANCE, default_state, is_water_source, scaffolding_distance,
            tick_water_if_waterlogged,
        },
    },
    context::PlaceContext,
    state::{BOTTOM, BlockState, STABILITY_DISTANCE, WATERLOGGED},
    world::{UPDATE_LIMIT, World},
};

/// `ScaffoldingBlock`。
pub(super) struct Scaffolding;

/// `TICK_DELAY`。
const TICK_DELAY: i32 = 1;

/// `isBottom`：悬空的脚手架最下面一层，要画出底板。
fn is_bottom(state: BlockState, world: &World, pos: BlockPos, distance: i32) -> bool {
    distance > 0 && !world.block_state(pos.below()).is_same_block(state)
}

impl Behaviour for Scaffolding {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let distance = scaffolding_distance(world, pos);
        let state = default_state(block);
        Some(
            state
                .with(WATERLOGGED, is_water_source(world, pos))
                .with(STABILITY_DISTANCE, distance)
                .with(BOTTOM, is_bottom(state, world, pos, distance)),
        )
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, _world: &World) -> bool {
        ctx.item == state.block().item
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        scaffolding_distance(world, pos) < SCAFFOLDING_MAX_DISTANCE
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
        tick_water_if_waterlogged(state, world, pos);
        world.schedule_tick(pos, state.block(), TICK_DELAY);
        state
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        world.schedule_tick(pos, state.block(), TICK_DELAY);
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let distance = scaffolding_distance(world, pos);
        let updated = state
            .with(STABILITY_DISTANCE, distance)
            .with(BOTTOM, is_bottom(state, world, pos, distance));
        if distance != SCAFFOLDING_MAX_DISTANCE {
            if updated != state {
                world.set_block_and_update(pos, updated);
            }
        } else if state.get(STABILITY_DISTANCE) == SCAFFOLDING_MAX_DISTANCE {
            fall(world, pos, updated);
        } else {
            world.destroy_block(pos, UPDATE_LIMIT);
        }
    }
}
