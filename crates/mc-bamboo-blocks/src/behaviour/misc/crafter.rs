//! `CrafterBlock`。被信号触发时排一个计划刻。9 格物品栏给漏斗和比较器用，合成和吐出物品不在这次范围里。
//!
//! `CrafterBlock`. A signal schedules a tick. The 9 slots are there for hoppers and the
//! comparator. Crafting and ejecting the result are out of scope.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, update_neighbours_after_destroy},
    },
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, CRAFTING, FrontAndTop, ORIENTATION, PropertyValue, TRIGGERED},
    world::{UPDATE_CLIENTS, World},
};

/// `CRAFTING_TICK_DELAY`：被触发之后隔这么多刻合成。
const CRAFTING_TICK_DELAY: i32 = 4;

/// `CrafterBlock`。
pub(super) struct Crafter;

/// `FrontAndTop.fromFrontAndTop`：枚举的序列化名就是两个方向的名字。
fn front_and_top(front: Direction, top: Direction) -> FrontAndTop {
    FrontAndTop::from_name(&format!("{}_{}", front.name(), top.name()))
        .expect("the crafter only combines directions that FrontAndTop has")
}

impl Behaviour for Crafter {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let front = ctx.nearest_looking_direction().opposite();
        let top = match front {
            Direction::Down => ctx.horizontal_direction().opposite(),
            Direction::Up => ctx.horizontal_direction(),
            _ => Direction::Up,
        };
        Some(
            default_state(block)
                .with(ORIENTATION, front_and_top(front, top))
                .with(TRIGGERED, world.has_neighbor_signal(ctx.clicked_pos())),
        )
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        if state.get(TRIGGERED) {
            world.schedule_tick(pos, state.block(), CRAFTING_TICK_DELAY);
        }
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        let should_trigger = world.has_neighbor_signal(pos);
        if should_trigger && !state.get(TRIGGERED) {
            world.schedule_tick(pos, state.block(), CRAFTING_TICK_DELAY);
            world.set_block(pos, state.with(TRIGGERED, true), UPDATE_CLIENTS);
        } else if !should_trigger && state.get(TRIGGERED) {
            let released = state.with(TRIGGERED, false).with(CRAFTING, false);
            world.set_block(pos, released, UPDATE_CLIENTS);
        }
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }

    /// `CrafterBlock.getAnalogOutputSignal`：非空或禁用的格子数。
    ///
    /// `CrafterBlock.getAnalogOutputSignal`: how many slots are non-empty or disabled.
    fn analog_output_signal(
        &self,
        _state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        crate::block_container::crafter_signal(world, pos)
    }
}
