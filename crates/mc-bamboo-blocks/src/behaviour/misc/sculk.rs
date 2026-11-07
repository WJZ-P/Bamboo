//! `SculkSensorBlock`、`CalibratedSculkSensorBlock`、`SculkShriekerBlock`、`SculkCatalystBlock`。
//!
//! 这个世界没有振动和实体，感测器不会被触发，幽匿尖啸体也不会叫。
//! 这里只有放置、含水、把状态放回去的计划刻，还有被破坏时的邻居更新。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    state::{
        BLOOM, BlockState, HORIZONTAL_FACING, SCULK_SENSOR_PHASE, SHRIEKING, SculkSensorPhase,
        WATERLOGGED,
    },
    world::World,
};

/// `SculkSensorBlock`。
pub(super) struct SculkSensor;

/// `updateNeighbours`。
fn update_neighbours(state: BlockState, world: &mut World, pos: BlockPos) {
    world.update_neighbors_at(pos, state.block());
    world.update_neighbors_at(pos.below(), state.block());
}

impl Behaviour for SculkSensor {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let waterlogged = is_water_source(world, ctx.clicked_pos());
        Some(default_state(block).with(WATERLOGGED, waterlogged))
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
        tick_water_if_waterlogged(state, world, pos);
        state
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(SCULK_SENSOR_PHASE) == SculkSensorPhase::Active {
            update_neighbours(state, world, pos);
        }
    }
}

/// `CalibratedSculkSensorBlock`。
pub(super) struct CalibratedSculkSensor;

impl Behaviour for CalibratedSculkSensor {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = SculkSensor.state_for_placement(block, ctx, world)?;
        Some(state.with(HORIZONTAL_FACING, ctx.horizontal_direction()))
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
        SculkSensor.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        SculkSensor.affect_neighbors_after_removal(state, world, pos);
    }
}

/// `SculkShriekerBlock`。
pub(super) struct SculkShrieker;

impl Behaviour for SculkShrieker {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        SculkSensor.state_for_placement(block, ctx, world)
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
        SculkSensor.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(SHRIEKING) {
            world.set_block_and_update(pos, state.with(SHRIEKING, false));
        }
    }
}

/// `SculkCatalystBlock`。
pub(super) struct SculkCatalyst;

impl Behaviour for SculkCatalyst {
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(BLOOM) {
            world.set_block_and_update(pos, state.with(BLOOM, false));
        }
    }
}
