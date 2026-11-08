//! `DecoratedPotBlock`。罐子里的那一格和漏斗共用。玩家右键能叠才放进去并摇晃，漏斗放进去不摇。
//!
//! `DecoratedPotBlock`. The single slot is shared with hoppers. A player's right click inserts
//! only when the item stacks, and wobbles; a hopper insert does not wobble.

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
    block_container::{insert_pot_item, pot_accepts, pot_signal},
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
        let pos = ctx.clicked_pos();
        if ctx.hand_is_empty() || !pot_accepts(world, pos, ctx.item) {
            return InteractionResult::TryEmptyHandInteraction;
        }
        // `wobble(POSITIVE)` 在 `setTheItem` 和 `setChanged` 之前。漏斗不走这个方法。
        //
        // `wobble(POSITIVE)` runs before `setTheItem` and `setChanged`. Hoppers do not use this.
        world.block_event(pos, state.block(), 1, 0);
        insert_pot_item(world, pos, ctx.item);
        world.update_neighbour_for_output_signal(pos, state.block());
        InteractionResult::Success
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        // 放不进去时的 `wobble(NEGATIVE)`。官方同时播失败音，音效不在这次范围里。
        //
        // `wobble(NEGATIVE)` when the item does not fit. Vanilla also plays the fail sound;
        // sounds are out of scope here.
        world.block_event(ctx.clicked_pos(), state.block(), 1, 1);
        InteractionResult::Success
    }

    /// `DecoratedPotBlock.getAnalogOutputSignal`。
    ///
    /// `DecoratedPotBlock.getAnalogOutputSignal`.
    fn analog_output_signal(
        &self,
        _state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        pot_signal(world, pos)
    }

    /// `DecoratedPotBlockEntity.triggerEvent`：事件 1、数据 0 或 1 是摇晃，只改客户端动画。
    ///
    /// `DecoratedPotBlockEntity.triggerEvent`: event 1 with data 0 or 1 is a wobble. It only
    /// drives the client animation.
    fn trigger_event(
        &self,
        _state: BlockState,
        _world: &mut World,
        _pos: BlockPos,
        param_a: i32,
        param_b: i32,
    ) -> bool {
        param_a == 1 && (param_b == 0 || param_b == 1)
    }
}
