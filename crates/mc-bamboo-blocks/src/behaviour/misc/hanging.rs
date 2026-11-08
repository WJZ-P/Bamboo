//! 长在别的方块上的 `HangingMossBlock`、`SporeBlossomBlock`、`ShelfMushroomBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            can_support_center, default_state, has_sturdy_wall, lost_wall_support,
            multiface_can_attach_at, wall_state_for_placement,
        },
    },
    context::PlaceContext,
    state::{AGE_1, BlockState, TIP},
    world::{UPDATE_CLIENTS, UPDATE_LIMIT, World},
};

/// `HangingMossBlock`。
pub(super) struct HangingMoss;

/// `canStayAtPosition`。
fn can_stay(state: BlockState, world: &World, pos: BlockPos) -> bool {
    multiface_can_attach_at(world, pos, Direction::Up)
        || world.block_state(pos.above()).is_same_block(state)
}

impl Behaviour for HangingMoss {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        can_stay(state, world, pos)
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
        if !can_stay(state, world, pos) {
            world.schedule_tick(pos, state.block(), 1);
        }
        let is_tip = !world.block_state(pos.below()).is_same_block(state);
        state.with(TIP, is_tip)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if !can_stay(state, world, pos) {
            world.destroy_block(pos, UPDATE_LIMIT);
        }
    }

    /// 最下面一节下面是空气时能长。
    fn is_valid_bonemeal_target(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let grow_pos = moss_tip(state, world, pos).below();
        world.block_state(grow_pos).is_air() && world.is_inside_build_height(grow_pos.y)
    }

    /// 在最下面一节下面再挂一节。
    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let grow_pos = moss_tip(state, world, pos).below();
        if world.block_state(grow_pos).is_air() {
            world.set_block_and_update(grow_pos, state.with(TIP, true));
        }
    }
}

/// `HangingMossBlock.getTip`：往下数，最后一节连着的垂苔。
fn moss_tip(state: BlockState, world: &World, pos: BlockPos) -> BlockPos {
    let mut forward = pos.below();
    while world.block_state(forward).is_same_block(state) {
        forward = forward.below();
    }
    forward.above()
}

/// `SporeBlossomBlock`。
pub(super) struct SporeBlossom;

impl Behaviour for SporeBlossom {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        can_support_center(world, pos.above(), Direction::Down) && !world.is_water_at(pos)
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
        if direction == Direction::Up && !self.can_survive(state, world, pos) {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

/// `ShelfMushroomBlock`。
pub(super) struct ShelfMushroom;

impl Behaviour for ShelfMushroom {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        wall_state_for_placement(default_state(block), ctx, world)
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
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn is_valid_bonemeal_target(&self, state: BlockState, _world: &World, _pos: BlockPos) -> bool {
        state.get(AGE_1) < 1
    }

    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        world.set_block(pos, state.with(AGE_1, state.get(AGE_1) + 1), UPDATE_CLIENTS);
    }
}
