//! `CampfireBlock`。
//!
//! 烤架上的食物在方块实体里，这里只看手上的东西能不能放上去，放满四个的情况走不到。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, block_maps, blocks, registries, tags::item_tags};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{
        BlockState, FluidState, HORIZONTAL_FACING, LIT, SIGNAL_FIRE, WATERLOGGED, item_is_tagged,
    },
    world::World,
};

/// `CampfireBlock`。
pub(super) struct Campfire;

/// `isSmokeSource`：下面是干草块时烟飘得更高。
fn is_smoke_source(state: BlockState) -> bool {
    state.is(blocks::HAY_BLOCK)
}

impl Behaviour for Campfire {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let water = is_water_source(world, pos);
        let state = default_state(block)
            .with(WATERLOGGED, water)
            .with(SIGNAL_FIRE, is_smoke_source(world.block_state(pos.below())))
            .with(LIT, !water)
            .with(HORIZONTAL_FACING, ctx.horizontal_direction());
        Some(state)
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
        if direction == Direction::Down {
            return state.with(SIGNAL_FIRE, is_smoke_source(neighbor));
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if block_maps::CAMPFIRE_INPUTS.contains(&ctx.item) {
            // `placeFood` 的 `setChanged`。
            world.update_neighbour_for_output_signal(ctx.clicked_pos(), state.block());
            return InteractionResult::SuccessServer;
        }
        if item_is_tagged(ctx.item, item_tags::DOUSES_CAMPFIRES) && state.get(LIT) {
            world.set_block_and_update(ctx.clicked_pos(), state.with(LIT, false));
            // `hurtAndBreak(1)` 在熄灭之后。1009 音效没有随机数。
            //
            // `hurtAndBreak(1)` happens after the douse. Level event 1009 draws nothing.
            crate::durability::hurt_hand(world, ctx.hand, 1);
            return InteractionResult::Success;
        }
        InteractionResult::TryEmptyHandInteraction
    }

    /// 进水的同时熄火。
    fn place_liquid(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        fluid: FluidState,
    ) -> bool {
        if state.get(WATERLOGGED) || !fluid.is(registries::fluid::WATER) {
            return false;
        }
        let doused = state.with(WATERLOGGED, true).with(LIT, false);
        world.set_block_and_update(pos, doused);
        world.schedule_fluid_tick(pos, fluid.fluid());
        true
    }
}
