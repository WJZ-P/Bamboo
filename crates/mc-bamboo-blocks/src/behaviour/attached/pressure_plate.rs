//! `PressurePlateBlock`、`WeightedPressurePlateBlock`，都继承 `BasePressurePlateBlock`。
//!
//! 压力板只有实体才踩得下去。这个世界没有实体，所以 `tick` 和 `entityInside` 不移植，
//! oracle 的计划刻排除表里有这一项。

use mc_bamboo_core::{BlockPos, Direction};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{can_support_center, can_support_rigid_block},
    },
    state::{BlockState, POWER, POWERED},
    world::World,
};

/// `BasePressurePlateBlock`。
pub(super) struct PressurePlate;

/// `getSignalForState` 大于 0。普通压力板的属性是 `powered`，测重压力板的是 `power`。
fn is_pressed(state: BlockState) -> bool {
    state
        .try_get(POWERED)
        .unwrap_or_else(|| state.get(POWER) > 0)
}

impl Behaviour for PressurePlate {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = pos.below();
        can_support_rigid_block(world, below) || can_support_center(world, below, Direction::Up)
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

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if is_pressed(state) {
            world.update_neighbors_at(pos, state.block());
            world.update_neighbors_at(pos.below(), state.block());
        }
    }
}
