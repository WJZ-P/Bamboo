//! `LanternBlock`、`WeatheringLanternBlock`。

use mc_bamboo_core::{Axis, BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::behaviour::weathering::change_over_time;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{can_support_center, default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    state::{BlockState, HANGING, WATERLOGGED},
    world::World,
};

/// `LanternBlock`。
pub(super) struct Lantern;

/// `getConnectedDirection`：灯笼朝外的那个方向，支撑在它的反方向。
fn connected_direction(state: BlockState) -> Direction {
    if state.get(HANGING) {
        Direction::Down
    } else {
        Direction::Up
    }
}

impl Behaviour for Lantern {
    /// `WeatheringLanternBlock` 在随机刻里氧化，别的类不会被随机刻选中。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        change_over_time(state, world, pos);
    }

    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let water = is_water_source(world, pos);
        ctx.nearest_looking_directions()
            .into_iter()
            .filter(|direction| direction.axis() == Axis::Y)
            .map(|direction| default_state(block).with(HANGING, direction == Direction::Up))
            .find(|state| state.can_survive(world, pos))
            .map(|state| state.with(WATERLOGGED, water))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let support = connected_direction(state).opposite();
        can_support_center(world, pos.relative(support), support.opposite())
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
        tick_water_if_waterlogged(state, world, pos);
        if connected_direction(state).opposite() == direction && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}
