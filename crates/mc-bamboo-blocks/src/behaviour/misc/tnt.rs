//! `TntBlock`。点燃时生成一个 [`PrimedTnt`](crate::entity::PrimedTnt)，方块随之消失。
//!
//! 火和火焰弹点燃的路子不在范围里：这个世界没有火焰 tick，火焰箭也没有。
//!
//! `TntBlock`. Priming spawns a [`PrimedTnt`](crate::entity::PrimedTnt) and the block
//! disappears. Fire ticks and flaming arrows are out of scope.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{Block, items};

use crate::{
    behaviour::{Behaviour, Plain},
    context::UseContext,
    interact::InteractionResult,
    state::BlockState,
    world::{UPDATE_ALL_IMMEDIATE, World},
};

/// `TntBlock`。
pub(super) struct Tnt;

/// `TntBlock.prime` 加 `removeBlock`：有信号时点燃这一格，方块换成空气或者流体。
///
/// `TntBlock.prime` plus `removeBlock`: a neighbor signal primes the block and
/// replaces it with air or the fluid left behind.
fn prime_if_powered(world: &mut World, pos: BlockPos) {
    if world.has_neighbor_signal(pos) {
        world.spawn_tnt(pos);
        world.remove_block(pos);
    }
}

impl Behaviour for Tnt {
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) {
            prime_if_powered(world, pos);
        }
    }

    fn neighbor_changed(
        &self,
        _state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        prime_if_powered(world, pos);
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if ctx.item != items::FLINT_AND_STEEL && ctx.item != items::FIRE_CHARGE {
            return Plain.use_item_on(state, world, ctx);
        }
        // 官方的 `prime` 先放实体，再用 11 号标志位把方块换成空气。
        // Official `prime` spawns the entity, then `setBlock` with flags 11 replaces the block with air.
        world.spawn_tnt(ctx.clicked_pos());
        world.set_block(ctx.clicked_pos(), BlockState::AIR, UPDATE_ALL_IMMEDIATE);
        InteractionResult::Success
    }
}
