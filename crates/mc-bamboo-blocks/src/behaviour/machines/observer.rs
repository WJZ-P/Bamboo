//! `ObserverBlock`。
//!
//! 脸前面那一格变了就排两刻，到时间亮一下再排两刻灭掉，脉冲就是这么来的。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{Behaviour, Plain, common::default_state},
    context::PlaceContext,
    state::{BlockState, FACING, POWERED},
    world::{UPDATE_CLIENTS, UPDATE_KNOWN_SHAPE, World},
};

/// `ObserverBlock`。
pub(super) struct Observer;

/// `PULSE_DELAY`：官方写在 `scheduleTick` 的参数里。
const PULSE_DELAY: i32 = 2;

/// `updateNeighborsInFront`：信号只从背面出去。
fn update_neighbors_in_front(state: BlockState, world: &mut World, pos: BlockPos) {
    let facing = state.get(FACING);
    let behind = pos.relative(facing.opposite());
    world.neighbor_changed(behind, state.block());
    world.update_neighbors_at_except_from_facing(behind, state.block(), facing);
}

/// `startSignal`：已经排了的不重复排。
fn start_signal(state: BlockState, world: &mut World, pos: BlockPos) {
    if !world.has_scheduled_tick(pos, state.block()) {
        world.schedule_tick(pos, state.block(), PULSE_DELAY);
    }
}

impl Behaviour for Observer {
    /// 脸朝着玩家看的方向，也就是玩家看到的是它的背面。
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(default_state(block).with(FACING, ctx.nearest_looking_direction()))
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
        if state.get(FACING) == direction && !state.get(POWERED) {
            start_signal(state, world, pos);
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    /// 放下时亮着又没排刻说明是指令或者结构放的，官方把它灭掉。
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if old.is_same_block(state)
            || !state.get(POWERED)
            || world.has_scheduled_tick(pos, state.block())
        {
            return;
        }
        let unpowered = state.with(POWERED, false);
        world.set_block(pos, unpowered, UPDATE_KNOWN_SHAPE | UPDATE_CLIENTS);
        update_neighbors_in_front(unpowered, world, pos);
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(POWERED) && world.has_scheduled_tick(pos, state.block()) {
            update_neighbors_in_front(state.with(POWERED, false), world, pos);
        }
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(POWERED) {
            world.set_block(pos, state.with(POWERED, false), UPDATE_CLIENTS);
        } else {
            world.set_block(pos, state.with(POWERED, true), UPDATE_CLIENTS);
            world.schedule_tick(pos, state.block(), PULSE_DELAY);
        }
        update_neighbors_in_front(state, world, pos);
    }

    fn own_signal(&self, state: BlockState, _world: &World, _pos: BlockPos) -> i32 {
        if state.get(POWERED) { 15 } else { 0 }
    }

    fn signal(&self, state: BlockState, world: &World, pos: BlockPos, direction: Direction) -> i32 {
        if state.get(FACING) == direction {
            state.own_signal(world, pos)
        } else {
            0
        }
    }

    fn direct_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        state.signal(world, pos, direction)
    }
}
