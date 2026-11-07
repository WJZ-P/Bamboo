//! `SignBlock` 和它的子类 `StandingSignBlock`、`WallSignBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, items};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, is_water_source, lost_wall_support, rotation_segment,
            tick_water_if_waterlogged, wall_state_for_placement,
        },
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, HORIZONTAL_FACING, ROTATION_16, WATERLOGGED},
    world::World,
};

/// `SignBlock`。告示牌的内容不做，所以这里的告示牌总是空白的，没上蜡，也没有别人在编辑。
pub(super) struct Sign;

impl Behaviour for Sign {
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
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    /// 染料和墨囊只对有字的告示牌起作用，空白的告示牌只能上蜡。
    fn use_item_on(
        &self,
        _state: BlockState,
        _world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if ctx.item == items::HONEYCOMB {
            InteractionResult::Success
        } else {
            InteractionResult::TryEmptyHandInteraction
        }
    }

    /// 官方在这里打开编辑界面。
    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::SuccessServer
    }
}

/// `StandingSignBlock`。
pub(super) struct StandingSign;

impl Behaviour for StandingSign {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = default_state(block)
            .with(ROTATION_16, rotation_segment(ctx.rotation() + 180.0))
            .with(WATERLOGGED, is_water_source(world, ctx.clicked_pos()));
        Some(state)
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        world.block_state(pos.below()).is_solid()
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
        Sign.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Sign.use_item_on(state, world, ctx)
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Sign.use_without_item(state, world, ctx)
    }
}

/// `WallSignBlock`。
pub(super) struct WallSign;

impl Behaviour for WallSign {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let water = is_water_source(world, ctx.clicked_pos());
        wall_state_for_placement(default_state(block), ctx, world)
            .map(|state| state.with(WATERLOGGED, water))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let support = state.get(HORIZONTAL_FACING).opposite();
        world.block_state(pos.relative(support)).is_solid()
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
        if lost_wall_support(state, world, pos, direction) {
            return BlockState::AIR;
        }
        Sign.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Sign.use_item_on(state, world, ctx)
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Sign.use_without_item(state, world, ctx)
    }
}
