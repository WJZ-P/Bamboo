//! `BambooSaplingBlock`、`BambooStalkBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks, tags::block_tags};

use super::{break_if_unsupported, schedule_break_if_unsupported};
use crate::{
    behaviour::{Behaviour, Plain, common::default_state},
    context::PlaceContext,
    state::{AGE_1, BlockState},
    world::World,
};

/// 两种竹子的 `canSurvive`。
fn has_bamboo_support(world: &World, pos: BlockPos) -> bool {
    world
        .block_state(pos.below())
        .is_tagged(block_tags::SUPPORTS_BAMBOO)
}

/// `BambooSaplingBlock`：竹笋。
pub(super) struct BambooSapling;

impl Behaviour for BambooSapling {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        has_bamboo_support(world, pos)
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
        if !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        if direction == Direction::Up && neighbor.is(blocks::BAMBOO) {
            return BlockState(blocks::BAMBOO);
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

/// `BambooStalkBlock`。
pub(super) struct BambooStalk;

impl Behaviour for BambooStalk {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let below = world.block_state(pos.below());
        if !world.fluid_state(pos).is_empty() || !below.is_tagged(block_tags::SUPPORTS_BAMBOO) {
            return None;
        }
        let stalk = default_state(block);
        if below.is(blocks::BAMBOO_SAPLING) {
            return Some(stalk.with(AGE_1, 0));
        }
        if below.is(blocks::BAMBOO) {
            return Some(stalk.with(AGE_1, below.get(AGE_1)));
        }
        let above = world.block_state(pos.above());
        Some(if above.is(blocks::BAMBOO) {
            stalk.with(AGE_1, above.get(AGE_1))
        } else {
            BlockState(blocks::BAMBOO_SAPLING)
        })
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        has_bamboo_support(world, pos)
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
        schedule_break_if_unsupported(state, world, pos);
        if direction == Direction::Up
            && neighbor.is(blocks::BAMBOO)
            && neighbor.get(AGE_1) > state.get(AGE_1)
        {
            return state.cycle(AGE_1);
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        break_if_unsupported(state, world, pos);
    }
}
