//! `BannerBlock`、`WallBannerBlock`。它们的父类 `AbstractBannerBlock` 只管方块实体。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, lost_wall_support, rotation_segment, wall_state_for_placement},
    },
    context::PlaceContext,
    state::{BlockState, HORIZONTAL_FACING, ROTATION_16},
    world::World,
};

/// `BannerBlock`。
pub(super) struct Banner;

impl Behaviour for Banner {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        let rotation = rotation_segment(ctx.rotation() + 180.0);
        Some(default_state(block).with(ROTATION_16, rotation))
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        world.block_state(pos.below()).is_solid()
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
        if direction == Direction::Down && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

/// `WallBannerBlock`。
pub(super) struct WallBanner;

impl Behaviour for WallBanner {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        wall_state_for_placement(default_state(block), ctx, world)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let support = state.get(HORIZONTAL_FACING).opposite();
        world.block_state(pos.relative(support)).is_solid()
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
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}
