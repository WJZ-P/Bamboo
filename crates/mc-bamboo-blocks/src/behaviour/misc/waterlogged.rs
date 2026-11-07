//! 只有含水逻辑的方块：`BarrierBlock`、`HeavyCoreBlock`、`MangroveRootsBlock`、
//! `WaterloggedTransparentBlock`、`WeatheringCopperGrateBlock`、`ConduitBlock`、`DriedGhastBlock`、
//! `LightBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, is_full_water, is_water_source, tick_water_if_waterlogged},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, HORIZONTAL_FACING, WATERLOGGED},
    world::World,
};

/// 放进水里就含水的方块。
pub(super) struct Waterlogged;

impl Behaviour for Waterlogged {
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
}

/// `ConduitBlock`：放在流动的水里也算含水，只要那一格是满的。
pub(super) struct Conduit;

impl Behaviour for Conduit {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let waterlogged = is_full_water(world.fluid_state(ctx.clicked_pos()));
        Some(default_state(block).with(WATERLOGGED, waterlogged))
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
        Waterlogged.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

/// `DriedGhastBlock`。吸水和孵化的计划刻不在范围里。
pub(super) struct DriedGhast;

impl Behaviour for DriedGhast {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = Waterlogged.state_for_placement(block, ctx, world)?;
        Some(state.with(HORIZONTAL_FACING, ctx.horizontal_direction().opposite()))
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
        Waterlogged.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

/// `LightBlock`：放置不含水。调亮度要权限，这个世界没有权限，所以只吃掉点击。
pub(super) struct Light;

impl Behaviour for Light {
    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        Waterlogged.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Consume
    }
}
