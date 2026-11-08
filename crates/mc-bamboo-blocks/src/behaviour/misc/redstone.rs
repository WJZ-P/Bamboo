//! 只跟信号打交道的几个方块：`PoweredBlock`（红石块）、`RedstoneLampBlock`、`TargetBlock`。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::Block;

use crate::{
    behaviour::{Behaviour, common::default_state},
    context::PlaceContext,
    state::{BlockState, LIT, POWER},
    world::{UPDATE_CLIENTS, UPDATE_KNOWN_SHAPE, World},
};

/// `PoweredBlock`：红石块，哪一面都是满信号。
pub(super) struct Powered;

impl Behaviour for Powered {
    fn own_signal(&self, _state: BlockState, _world: &World, _pos: BlockPos) -> i32 {
        15
    }
}

/// `RedstoneLampBlock`。亮起来是立刻的，灭掉要等 4 刻，中途重新通电就不灭。
pub(super) struct RedstoneLamp;

/// 灯灭掉之前等的刻数。
const LAMP_OFF_DELAY: i32 = 4;

impl Behaviour for RedstoneLamp {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let lit = world.has_neighbor_signal(ctx.clicked_pos());
        Some(default_state(block).with(LIT, lit))
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        let lit = state.get(LIT);
        if lit == world.has_neighbor_signal(pos) {
            return;
        }
        if lit {
            world.schedule_tick(pos, state.block(), LAMP_OFF_DELAY);
        } else {
            world.set_block(pos, state.cycle(LIT), UPDATE_CLIENTS);
        }
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(LIT) && !world.has_neighbor_signal(pos) {
            world.set_block(pos, state.cycle(LIT), UPDATE_CLIENTS);
        }
    }
}

/// `TargetBlock`。只有弹射物打中才会输出信号，这个世界里没有弹射物，
/// 所以这里只有计划刻把信号归零，还有放下来时清掉状态里带着的信号。
pub(super) struct Target;

impl Behaviour for Target {
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if old.is_same_block(state) || state.get(POWER) == 0 {
            return;
        }
        if !world.has_scheduled_tick(pos, state.block()) {
            let cleared = state.with(POWER, 0);
            world.set_block(pos, cleared, UPDATE_KNOWN_SHAPE | UPDATE_CLIENTS);
        }
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(POWER) != 0 {
            world.set_block_and_update(pos, state.with(POWER, 0));
        }
    }

    fn own_signal(&self, state: BlockState, _world: &World, _pos: BlockPos) -> i32 {
        state.get(POWER)
    }
}
