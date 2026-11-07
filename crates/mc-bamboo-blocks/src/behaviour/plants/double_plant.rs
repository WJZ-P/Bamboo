//! `DoublePlantBlock`、`TallFlowerBlock`、`TallSeagrassBlock`、`PitcherCropBlock`、`SmallDripleafBlock`。

use mc_bamboo_core::{Axis, BlockPos, Direction};
use mc_bamboo_data::Block;

use super::{crop::has_sufficient_light, vegetation::Vegetation};
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, is_full_water, prevent_drop_from_bottom_part, tick_water_if_waterlogged,
            vegetation_update_shape,
        },
    },
    context::{Actor, PlaceContext},
    state::{
        AGE_4, BlockState, DOUBLE_BLOCK_HALF, DoubleBlockHalf, HORIZONTAL_FACING, WATERLOGGED,
    },
    world::World,
};

/// `PitcherCropBlock.DOUBLE_PLANT_AGE_INTERSECTION`：长到这么大才有上半。
const PITCHER_DOUBLE_AGE: i32 = 3;

/// `copyWaterloggedFrom`：有含水属性的方块跟着这一格的水走。
fn copy_waterlogged_from(world: &World, pos: BlockPos, state: BlockState) -> BlockState {
    state
        .try_with(WATERLOGGED, world.is_water_at(pos))
        .unwrap_or(state)
}

/// 这是下半。
fn is_lower(state: BlockState) -> bool {
    state.get(DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Lower
}

/// `DoublePlantBlock`。
pub(super) struct DoublePlant;

impl Behaviour for DoublePlant {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let room_above =
            pos.y < world.max_y() && world.block_state(pos.above()).can_be_replaced(ctx, world);
        room_above.then(|| default_state(block))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        if is_lower(state) {
            return Vegetation.can_survive(state, world, pos);
        }
        let below = world.block_state(pos.below());
        below.is_same_block(state) && is_lower(below)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        let toward_other_half =
            direction.axis() == Axis::Y && is_lower(state) == (direction == Direction::Up);
        let is_other_half = neighbor.is_same_block(state) && is_lower(neighbor) != is_lower(state);
        if toward_other_half && !is_other_half {
            return BlockState::AIR;
        }
        // 官方在这里还单独判定了下半朝下的更新，`VegetationBlock.updateShape` 判定的是同一个
        // `canSurvive`，结果一样。
        vegetation_update_shape(state, world, pos)
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        let above = pos.above();
        let upper = state
            .default_state()
            .with(DOUBLE_BLOCK_HALF, DoubleBlockHalf::Upper);
        let upper = copy_waterlogged_from(world, above, upper);
        world.set_block_and_update(above, upper);
    }

    fn player_will_destroy(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        player: &Actor,
    ) -> BlockState {
        prevent_drop_from_bottom_part(world, pos, state);
        Plain.player_will_destroy(state, world, pos, player)
    }
}

/// `TallSeagrassBlock`。
pub(super) struct TallSeagrass;

impl Behaviour for TallSeagrass {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let above = world.fluid_state(ctx.clicked_pos().above());
        DoublePlant
            .state_for_placement(block, ctx, world)
            .filter(|_| is_full_water(above))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        DoublePlant.can_survive(state, world, pos)
            && (!is_lower(state) || is_full_water(world.fluid_state(pos)))
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
        DoublePlant.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, placer: &Actor) {
        DoublePlant.set_placed_by(state, world, pos, placer);
    }

    fn player_will_destroy(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        player: &Actor,
    ) -> BlockState {
        DoublePlant.player_will_destroy(state, world, pos, player)
    }
}

/// `PitcherCropBlock`：放下时只占一格，长大后才有上半。
pub(super) struct PitcherCrop;

impl Behaviour for PitcherCrop {
    fn can_be_replaced(&self, _state: BlockState, _ctx: &PlaceContext, _world: &World) -> bool {
        false
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        (!is_lower(state) || has_sufficient_light(world, pos))
            && DoublePlant.can_survive(state, world, pos)
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
        if state.get(AGE_4) >= PITCHER_DOUBLE_AGE {
            DoublePlant.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
        } else {
            vegetation_update_shape(state, world, pos)
        }
    }

    fn player_will_destroy(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        player: &Actor,
    ) -> BlockState {
        DoublePlant.player_will_destroy(state, world, pos, player)
    }
}

/// `SmallDripleafBlock`。
pub(super) struct SmallDripleaf;

impl Behaviour for SmallDripleaf {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = DoublePlant.state_for_placement(block, ctx, world)?;
        let facing = ctx.horizontal_direction().opposite();
        Some(copy_waterlogged_from(
            world,
            ctx.clicked_pos(),
            state.with(HORIZONTAL_FACING, facing),
        ))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        DoublePlant.can_survive(state, world, pos)
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
        DoublePlant.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        let above = pos.above();
        let upper = state
            .default_state()
            .with(DOUBLE_BLOCK_HALF, DoubleBlockHalf::Upper)
            .with(HORIZONTAL_FACING, state.get(HORIZONTAL_FACING));
        let upper = copy_waterlogged_from(world, above, upper);
        world.set_block_and_update(above, upper);
    }

    fn player_will_destroy(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        player: &Actor,
    ) -> BlockState {
        DoublePlant.player_will_destroy(state, world, pos, player)
    }
}
