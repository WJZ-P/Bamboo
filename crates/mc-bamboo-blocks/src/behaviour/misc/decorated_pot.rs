//! `DecoratedPotBlock`。里面的物品在方块实体里，不在范围里，所以罐子永远是空的，什么物品都放得进去。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, tags::item_tags};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, is_water_source, tick_water_if_waterlogged,
            update_neighbours_after_destroy,
        },
    },
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, CRACKED, HORIZONTAL_FACING, WATERLOGGED, item_is_tagged},
    world::{UPDATE_NONE, World},
};

/// `DecoratedPotBlock`。
pub(super) struct DecoratedPot;

impl Behaviour for DecoratedPot {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let waterlogged = is_water_source(world, ctx.clicked_pos());
        Some(
            default_state(block)
                .with(HORIZONTAL_FACING, ctx.horizontal_direction())
                .with(WATERLOGGED, waterlogged)
                .with(CRACKED, false),
        )
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
        update_neighbours_after_destroy(state, world, pos);
    }

    fn player_will_destroy(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        player: &Actor,
    ) -> BlockState {
        let mut broken = state;
        if item_is_tagged(player.main_hand, item_tags::BREAKS_DECORATED_POTS) {
            broken = state.with(CRACKED, true);
            world.set_block(pos, broken, UPDATE_NONE);
        }
        Plain.player_will_destroy(broken, world, pos, player)
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if ctx.hand_is_empty() {
            return InteractionResult::TryEmptyHandInteraction;
        }
        // 方块实体的 `setChanged`：放进东西后比较器要重新读。
        world.update_neighbour_for_output_signal(ctx.clicked_pos(), state.block());
        InteractionResult::Success
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }
}
