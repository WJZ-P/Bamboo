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
        BLOOM, BlockState, HORIZONTAL_FACING, POWER, SCULK_SENSOR_PHASE, SHRIEKING,
        SculkSensorPhase, WATERLOGGED,
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

/// 冷却的刻数，官方写在 `deactivate` 的 `scheduleTick` 里。
const COOLDOWN_TICKS: i32 = 10;

/// `deactivate`：信号归零，进入冷却。
fn deactivate(state: BlockState, world: &mut World, pos: BlockPos) {
    let cooling = state
        .with(SCULK_SENSOR_PHASE, SculkSensorPhase::Cooldown)
        .with(POWER, 0);
    world.set_block_and_update(pos, cooling);
    world.schedule_tick(pos, state.block(), COOLDOWN_TICKS);
    update_neighbours(state, world, pos);
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

    /// 激活的感测器到期后冷却，冷却完再回到待命。激活本身要振动，这里没有。
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        match state.get(SCULK_SENSOR_PHASE) {
            SculkSensorPhase::Active => deactivate(state, world, pos),
            SculkSensorPhase::Cooldown => {
                let inactive = state.with(SCULK_SENSOR_PHASE, SculkSensorPhase::Inactive);
                world.set_block_and_update(pos, inactive);
            }
            SculkSensorPhase::Inactive => {}
        }
    }

    fn own_signal(&self, state: BlockState, _world: &World, _pos: BlockPos) -> i32 {
        state.get(POWER)
    }

    /// 只强充能脚下那一格，强度还是走 `getSignal`，校准型会挡掉一面。
    fn direct_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        if direction == Direction::Up {
            state.signal(world, pos, direction)
        } else {
            0
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

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        SculkSensor.tick(state, world, pos);
    }

    fn own_signal(&self, state: BlockState, world: &World, pos: BlockPos) -> i32 {
        SculkSensor.own_signal(state, world, pos)
    }

    /// 校准型的那一面是输入，不往那边输出。
    fn signal(&self, state: BlockState, world: &World, pos: BlockPos, direction: Direction) -> i32 {
        if direction == state.get(HORIZONTAL_FACING) {
            0
        } else {
            SculkSensor.own_signal(state, world, pos)
        }
    }

    fn direct_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        SculkSensor.direct_signal(state, world, pos, direction)
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
