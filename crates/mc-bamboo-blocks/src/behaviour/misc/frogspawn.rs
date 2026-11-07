//! `FrogspawnBlock`。孵化的计划刻不在范围里。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::tags::{block_tags, fluid_tags};

use crate::{
    behaviour::{Behaviour, Plain},
    state::BlockState,
    world::World,
};

/// `FrogspawnBlock`。
pub(super) struct Frogspawn;

/// `mayPlaceOn`：水面上，再往上没有流体。
fn may_place_on(world: &World, pos: BlockPos) -> bool {
    let supports = world
        .fluid_state(pos)
        .is_tagged(fluid_tags::SUPPORTS_FROGSPAWN)
        || world
            .block_state(pos)
            .is_tagged(block_tags::SUPPORTS_FROGSPAWN);
    supports && world.fluid_state(pos.above()).is_empty()
}

impl Behaviour for Frogspawn {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        may_place_on(world, pos.below())
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
        if self.can_survive(state, world, pos) {
            Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
        } else {
            BlockState::AIR
        }
    }
}
