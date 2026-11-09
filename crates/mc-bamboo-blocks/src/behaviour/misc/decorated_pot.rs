//! `DecoratedPotBlock`。罐子里的那一格和漏斗共用。玩家右键能叠才放进去并摇晃，漏斗放进去不摇。
//!
//! `DecoratedPotBlock`. The single slot is shared with hoppers. A player's right click inserts
//! only when the item stacks, and wobbles; a hopper insert does not wobble.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, sounds::sound_event, tags::item_tags};

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

/// 放进去之后的音高：`0.7 + 0.5 * (count / maxStackSize)`。
///
/// Pitch after the insert: `0.7 + 0.5 * (count / maxStackSize)`.
fn pot_insert_pitch(world: &World, pos: BlockPos) -> f32 {
    let stack = world.slots_at(pos).into_iter().next();
    let (count, max_stack) = match stack {
        Some(stack) if !stack.is_empty() => {
            let max_stack = usize::try_from(stack.item)
                .ok()
                .and_then(|index| mc_bamboo_data::items::ITEMS.get(index))
                .map(|item| item.max_stack_size)
                .filter(|size| *size > 0)
                .unwrap_or(1);
            (stack.count, max_stack)
        }
        _ => (1, 1),
    };
    0.7 + 0.5 * (count as f32 / max_stack as f32)
}

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
        // `wobble(POSITIVE)` 在 `setTheItem` 之前。漏斗不走这个方法。
        // 音高是放进去之后的 `count / maxStackSize`。`DUST_PLUME` 是 `sendParticles`，这里没有粒子包。
        //
        // `wobble(POSITIVE)` runs before `setTheItem`. Hoppers do not use this method.
        // Pitch is `count / maxStackSize` after the insert. `DUST_PLUME` is `sendParticles`;
        // there is no particle packet here.
        world.block_event(pos, state.block(), 1, 0);
        insert_pot_item(world, pos, ctx.item);
        world.play_block_sound(
            pos,
            sound_event("minecraft:block.decorated_pot.insert"),
            1.0,
            pot_insert_pitch(world, pos),
        );
        world.update_neighbour_for_output_signal(pos, state.block());
        InteractionResult::Success
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        // 失败音在 `wobble(NEGATIVE)` 之前。`except` 是空。
        // The fail sound comes before `wobble(NEGATIVE)`. `except` is null.
        world.play_block_sound(
            ctx.clicked_pos(),
            sound_event("minecraft:block.decorated_pot.insert_fail"),
            1.0,
            1.0,
        );
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
