//! `FarmlandBlock`、`PathBlock`：上面压着东西时变回泥土。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor, blocks, tags::block_tags};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, field_block},
    },
    context::PlaceContext,
    state::BlockState,
    world::World,
};

/// `FarmlandBlock`。
pub(super) struct Farmland;

/// `PathBlock`。
pub(super) struct DirtPath;

/// 两个类的 `updateShape`：上面被压住时下一刻再变回去。
fn update_shape_under_pressure(
    state: BlockState,
    world: &mut World,
    pos: BlockPos,
    direction: Direction,
    neighbor_pos: BlockPos,
    neighbor: BlockState,
) -> BlockState {
    if direction == Direction::Up && !state.can_survive(world, pos) {
        world.schedule_tick(pos, state.block(), 1);
    }
    Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
}

/// 两个类的 `turnToBaseBlock`：这个世界看不到实体，不用把它们往上推。
fn turn_to_base_block(state: BlockState, world: &mut World, pos: BlockPos) {
    let base = default_state(field_block(state.block(), "baseBlock"));
    world.set_block_and_update(pos, base);
}

impl Behaviour for Farmland {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = default_state(block);
        Some(if state.can_survive(world, ctx.clicked_pos()) {
            state
        } else {
            default_state(field_block(block, "baseBlock"))
        })
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        let above = world.block_state(pos.above());
        !above.is_solid() || above.is_tagged(block_tags::MAINTAINS_FARMLAND)
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
        update_shape_under_pressure(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if !state.can_survive(world, pos) {
            turn_to_base_block(state, world, pos);
        }
    }
}

impl Behaviour for DirtPath {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = default_state(block);
        Some(if state.can_survive(world, ctx.clicked_pos()) {
            state
        } else {
            BlockState(blocks::DIRT)
        })
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        let above = world.block_state(pos.above());
        !above.is_solid() || above.is_a(BlockAncestor::FenceGateBlock)
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
        update_shape_under_pressure(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        turn_to_base_block(state, world, pos);
    }
}
