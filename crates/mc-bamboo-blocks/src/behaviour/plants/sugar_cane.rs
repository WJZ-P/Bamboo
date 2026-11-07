//! `SugarCaneBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::tags::{block_tags, fluid_tags};

use super::{break_if_unsupported, schedule_break_if_unsupported};
use crate::{
    behaviour::{Behaviour, Plain},
    state::BlockState,
    world::World,
};

/// `SugarCaneBlock`。
pub(super) struct SugarCane;

/// 这一格能给旁边的甘蔗供水。
fn waters_sugar_cane(world: &World, pos: BlockPos) -> bool {
    world
        .fluid_state(pos)
        .is_tagged(fluid_tags::SUPPORTS_SUGAR_CANE_ADJACENTLY)
        || world
            .block_state(pos)
            .is_tagged(block_tags::SUPPORTS_SUGAR_CANE_ADJACENTLY)
}

impl Behaviour for SugarCane {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below = world.block_state(below_pos);
        below.is_same_block(state)
            || below.is_tagged(block_tags::SUPPORTS_SUGAR_CANE)
                && Direction::HORIZONTAL
                    .into_iter()
                    .any(|side| waters_sugar_cane(world, below_pos.relative(side)))
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
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        break_if_unsupported(state, world, pos);
    }
}
