//! `FrostedIceBlock`。慢慢化掉的计划刻不在范围里，只做旁边的霜冰化了之后跟着化。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks};

use crate::{behaviour::Behaviour, state::BlockState, world::World};

/// `FrostedIceBlock`。
pub(super) struct FrostedIce;

/// `NEIGHBORS_TO_MELT`。
const NEIGHBORS_TO_MELT: usize = 2;

/// `IceBlock.melt`，主世界的水不会蒸发。
fn melt(world: &mut World, pos: BlockPos) {
    let water = BlockState::WATER;
    world.set_block_and_update(pos, water);
    world.neighbor_changed(pos, water.block());
}

/// `fewerNeigboursThan`。
fn has_fewer_neighbours(state: BlockState, world: &World, pos: BlockPos, limit: usize) -> bool {
    Direction::ALL
        .into_iter()
        .filter(|&side| world.block_state(pos.relative(side)).is_same_block(state))
        .count()
        < limit
}

impl Behaviour for FrostedIce {
    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        source: &'static Block,
    ) {
        if BlockState(source.default_state).is(blocks::FROSTED_ICE)
            && has_fewer_neighbours(state, world, pos, NEIGHBORS_TO_MELT)
        {
            melt(world, pos);
        }
    }
}
