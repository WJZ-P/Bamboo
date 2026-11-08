//! `CocoaBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, tags::block_tags};

use crate::{
    behaviour::{Behaviour, Plain, common::default_state},
    context::PlaceContext,
    state::{AGE_2, BlockState, HORIZONTAL_FACING},
    world::{UPDATE_CLIENTS, World},
};

/// `CocoaBlock.MAX_AGE`。
const MAX_AGE: i32 = 2;
/// 每次随机刻五分之一的机会长一级。
const GROWTH_CHANCE: i32 = 5;

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

    /// 五分之一的机会长一级。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if world.random().next_int(GROWTH_CHANCE) == 0 {
            let age = state.get(AGE_2);
            if age < MAX_AGE {
                world.set_block(pos, state.with(AGE_2, age + 1), UPDATE_CLIENTS);
            }
        }
    }

    fn is_valid_bonemeal_target(&self, state: BlockState, _world: &World, _pos: BlockPos) -> bool {
        state.get(AGE_2) < MAX_AGE
    }

    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        world.set_block(pos, state.with(AGE_2, state.get(AGE_2) + 1), UPDATE_CLIENTS);
    }
}
