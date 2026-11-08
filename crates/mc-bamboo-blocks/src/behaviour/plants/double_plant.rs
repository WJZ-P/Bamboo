//! `DoublePlantBlock`、`TallFlowerBlock`、`TallSeagrassBlock`、`PitcherCropBlock`、`SmallDripleafBlock`。

use mc_bamboo_core::{Axis, BlockPos, Direction};
use mc_bamboo_data::{Block, blocks};

use super::{
    crop::{has_sufficient_light, rolls_growth},
    dripleaf::place_with_random_height,
    vegetation::Vegetation,
};
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
        AGE_4, BlockState, DOUBLE_BLOCK_HALF, DoubleBlockHalf, FluidState, HORIZONTAL_FACING,
        WATERLOGGED,
    },
    world::{UPDATE_CLIENTS, UPDATE_KNOWN_SHAPE, World},
};

/// `PitcherCropBlock.DOUBLE_PLANT_AGE_INTERSECTION`：长到这么大才有上半。
const PITCHER_DOUBLE_AGE: i32 = 3;
/// `PitcherCropBlock.MAX_AGE`。
const PITCHER_MAX_AGE: i32 = 4;

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

    /// 只有 `TallFlowerBlock` 是 `BonemealableBlock`，骨粉总是成功。
    fn is_valid_bonemeal_target(&self, _state: BlockState, _world: &World, _pos: BlockPos) -> bool {
        true
    }

    /// 官方在这里掉一朵同样的花，这个世界没有掉落物，世界不变。
    fn perform_bonemeal(&self, _state: BlockState, _world: &mut World, _pos: BlockPos) {}
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

    /// 高海草本来就泡在水里，装不了别的。
    fn can_place_liquid(
        &self,
        _state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _fluid: i32,
        _by_player: bool,
    ) -> bool {
        false
    }

    fn place_liquid(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        _fluid: FluidState,
    ) -> bool {
        false
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

    /// 和作物一样按周围的耕地算机会，长一级。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if rolls_growth(state.block(), world, pos) {
            grow_pitcher(state, world, pos);
        }
    }

    fn is_valid_bonemeal_target(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        pitcher_lower_half(state, world, pos).is_some_and(|(lower_pos, lower)| {
            pitcher_can_grow(lower, world, lower_pos, lower.get(AGE_4) + 1)
        })
    }

    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if let Some((lower_pos, lower)) = pitcher_lower_half(state, world, pos) {
            grow_pitcher(lower, world, lower_pos);
        }
    }
}

/// 瓶子草的下半。
fn is_pitcher_lower(state: BlockState) -> bool {
    state.is(blocks::PITCHER_CROP) && is_lower(state)
}

/// `getLowerHalf`：点中的是上半时往下找。
fn pitcher_lower_half(
    state: BlockState,
    world: &World,
    pos: BlockPos,
) -> Option<(BlockPos, BlockState)> {
    if is_pitcher_lower(state) {
        return Some((pos, state));
    }
    let below_pos = pos.below();
    let below = world.block_state(below_pos);
    is_pitcher_lower(below).then_some((below_pos, below))
}

/// `canGrow`：没长满、够亮，长到两格高时上面要空着或者已经是瓶子草。
fn pitcher_can_grow(lower: BlockState, world: &World, lower_pos: BlockPos, new_age: i32) -> bool {
    let above = lower_pos.above();
    let above_state = world.block_state(above);
    lower.get(AGE_4) < PITCHER_MAX_AGE
        && has_sufficient_light(world, lower_pos)
        && world.is_inside_build_height(above.y)
        && (new_age < PITCHER_DOUBLE_AGE
            || above_state.is_air()
            || above_state.is(blocks::PITCHER_CROP))
}

/// `grow(level, lowerState, lowerPos, 1)`：长一级，够大时补上上半。
fn grow_pitcher(lower: BlockState, world: &mut World, lower_pos: BlockPos) {
    let age = (lower.get(AGE_4) + 1).min(PITCHER_MAX_AGE);
    if !pitcher_can_grow(lower, world, lower_pos, age) {
        return;
    }
    let grown = lower.with(AGE_4, age);
    world.set_block(lower_pos, grown, UPDATE_CLIENTS);
    if age >= PITCHER_DOUBLE_AGE {
        let upper = grown.with(DOUBLE_BLOCK_HALF, DoubleBlockHalf::Upper);
        world.set_block_and_update(lower_pos.above(), upper);
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

    fn is_valid_bonemeal_target(&self, _state: BlockState, _world: &World, _pos: BlockPos) -> bool {
        true
    }

    /// 下半清掉上半，从自己这格长出大型垂滴叶；点中上半时交给下面那一格。
    /// 下面那一格不是垂滴叶时官方读不到属性会抛异常，那是残缺的结构，这里什么也不做。
    fn perform_bonemeal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let half = state.try_get(DOUBLE_BLOCK_HALF);
        let facing = state.try_get(HORIZONTAL_FACING);
        match (half, facing) {
            (Some(DoubleBlockHalf::Lower), Some(facing)) => {
                let above = pos.above();
                let cleared = world.fluid_state(above).legacy_block();
                world.set_block(above, cleared, UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE);
                place_with_random_height(world, pos, facing);
            }
            (Some(DoubleBlockHalf::Upper), _) => {
                let below = pos.below();
                let below_state = world.block_state(below);
                SmallDripleaf.perform_bonemeal(below_state, world, below);
            }
            _ => {}
        }
    }
}
