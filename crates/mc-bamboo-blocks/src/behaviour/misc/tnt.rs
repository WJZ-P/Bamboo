//! `TntBlock`。点燃后的实体和爆炸不在范围里，方块和官方一样消失。

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

/// `onPlace` 和 `neighborChanged`：有信号时点燃。世界里没有信号，写成这样是为了以后只改一处。
fn prime_if_powered(world: &mut World, pos: BlockPos) {
    if world.has_neighbor_signal(pos) {
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
        world.set_block(ctx.clicked_pos(), BlockState::AIR, UPDATE_ALL_IMMEDIATE);
        InteractionResult::Success
    }
}
