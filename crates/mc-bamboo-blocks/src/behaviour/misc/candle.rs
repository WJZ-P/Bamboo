//! `CandleBlock` 和父类 `AbstractCandleBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{can_support_center, default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, CANDLES, LIT, WATERLOGGED},
    world::{UPDATE_ALL_IMMEDIATE, World},
};

/// `CandleBlock`。
pub(super) struct Candle;

/// `MAX_CANDLES`。
const MAX_CANDLES: i32 = 4;

/// `AbstractCandleBlock.extinguish`。
pub(super) fn extinguish(state: BlockState, world: &mut World, pos: BlockPos) {
    world.set_block(pos, state.with(LIT, false), UPDATE_ALL_IMMEDIATE);
}

impl Behaviour for Candle {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = ctx.replaced_state(world);
        if state.is(block.default_state) {
            return Some(state.cycle(CANDLES));
        }
        let waterlogged = is_water_source(world, ctx.clicked_pos());
        Some(default_state(block).with(WATERLOGGED, waterlogged))
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, world: &World) -> bool {
        let adds_a_candle = !ctx.is_secondary_use_active()
            && ctx.item == state.block().item
            && state.get(CANDLES) < MAX_CANDLES;
        adds_a_candle || Plain.can_be_replaced(state, ctx, world)
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        can_support_center(world, pos.below(), Direction::Up)
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
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if ctx.hand_is_empty() && state.get(LIT) {
            extinguish(state, world, ctx.clicked_pos());
            return InteractionResult::Success;
        }
        Plain.use_item_on(state, world, ctx)
    }
}
