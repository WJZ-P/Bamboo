//! `CactusBlock`。`CactusFlowerBlock` 只换了支撑条件，在 [`vegetation`](super::vegetation) 里。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::tags::block_tags;

use super::{break_if_unsupported, schedule_break_if_unsupported};
use crate::{
    behaviour::{Behaviour, Plain},
    state::BlockState,
    world::World,
};

/// `CactusBlock`。
pub(super) struct Cactus;

/// 仙人掌不能贴着实心方块和岩浆。
fn crowds_cactus(world: &World, pos: BlockPos) -> bool {
    world.block_state(pos).is_solid() || world.fluid_state(pos).is_lava()
}

impl Behaviour for Cactus {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let crowded = Direction::HORIZONTAL
            .into_iter()
            .any(|side| crowds_cactus(world, pos.relative(side)));
        let below = world.block_state(pos.below());
        !crowded
            && (below.is_same_block(state) || below.is_tagged(block_tags::SUPPORTS_CACTUS))
            && !world.block_state(pos.above()).is_liquid()
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
