//! `SculkSensorBlock`、`CalibratedSculkSensorBlock`、`SculkShriekerBlock`、`SculkCatalystBlock`。
//!
//! 感测体的振动行为在 [`super::sculk_entity`]；这里管状态、计划刻和红石输出。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, sounds::sound_event};

use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
        misc::sculk_entity,
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
pub(crate) fn update_neighbours(state: BlockState, world: &mut World, pos: BlockPos) {
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
                // 冷却结束响一声，泡在水里时官方不响。音高在换完方块之后才取。
                // The cooldown end plays one sound unless the sensor is waterlogged.
                // The pitch is drawn after the block has changed.
                if !state.get(WATERLOGGED) {
                    let pitch = world.random().next_float() * 0.2 + 0.8;
                    world.play_block_sound(
                        pos,
                        sound_event("minecraft:block.sculk_sensor.clicking_stop"),
                        1.0,
                        pitch,
                    );
                }
            }
            SculkSensorPhase::Inactive => {}
        }
    }

    /// 放下方块时挂上方块实体，和官方 `newBlockEntity` 一致。
    fn on_place(&self, _state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        if !world.has_block_entity(pos) {
            world.set_block_entity(sculk_entity::create_block_entity(pos, false));
        }
    }

    /// ACTIVE 时比较器读最后一次振动频率。
    fn analog_output_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        if state.get(SCULK_SENSOR_PHASE) == SculkSensorPhase::Active {
            world
                .block_entity::<sculk_entity::SculkSensorEntity>(pos)
                .map_or(0, sculk_entity::SculkSensorEntity::last_frequency)
        } else {
            0
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
    fn on_place(&self, _state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        if !world.has_block_entity(pos) {
            world.set_block_entity(sculk_entity::create_block_entity(pos, true));
        }
    }

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

    fn analog_output_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        SculkSensor.analog_output_signal(state, world, pos, direction)
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
        // 官方 `SculkShriekerBlock.tick`：复位尖叫之后 `tryRespond`。
        let _ = world
            .with_block_entity::<crate::behaviour::misc::sculk_shrieker::SculkShriekerEntity, _>(
                pos,
                |entity, world| entity.try_respond(world),
            );
    }

    fn on_place(&self, _state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        if !world.has_block_entity(pos) {
            world.set_block_entity(crate::behaviour::misc::sculk_shrieker::create_block_entity(
                pos,
            ));
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
