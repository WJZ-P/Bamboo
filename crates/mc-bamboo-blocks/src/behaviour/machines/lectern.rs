//! `LecternBlock`。
//!
//! 翻页的红石脉冲要界面才走得到，`tick` 不做。

use mc_bamboo_core::{BlockPos, Hand};
use mc_bamboo_data::{Block, tags::item_tags};

use crate::{
    behaviour::{Behaviour, common::facing_the_player},
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, HAS_BOOK, POWERED, item_is_tagged},
    world::World,
};

/// `LecternBlock`。
pub(super) struct Lectern;

/// `updateBelow`：讲台的信号穿过下面一格。
fn update_below(state: BlockState, world: &mut World, pos: BlockPos) {
    world.update_neighbors_at(pos.below(), state.block());
}

/// `placeBook`：`setBook` 改了方块实体，比较器先知道，然后才换状态。
fn place_book(state: BlockState, world: &mut World, pos: BlockPos) {
    world.update_neighbour_for_output_signal(pos, state.block());
    let with_book = state.with(POWERED, false).with(HAS_BOOK, true);
    world.set_block_and_update(pos, with_book);
    update_below(state, world, pos);
}

impl Behaviour for Lectern {
    /// 官方只给有 `canUseGameMasterBlocks` 的玩家读物品里的书，这里没有权限系统。
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_player(block, ctx).with(HAS_BOOK, false))
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(POWERED) {
            update_below(state, world, pos);
        }
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if state.get(HAS_BOOK) {
            return InteractionResult::TryEmptyHandInteraction;
        }
        if item_is_tagged(ctx.item, item_tags::LECTERN_BOOKS) {
            place_book(state, world, ctx.clicked_pos());
            return InteractionResult::Success;
        }
        if ctx.hand_is_empty() && ctx.hand == Hand::Main {
            InteractionResult::Pass
        } else {
            InteractionResult::TryEmptyHandInteraction
        }
    }

    fn use_without_item(
        &self,
        state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        if state.get(HAS_BOOK) {
            InteractionResult::Success
        } else {
            InteractionResult::Consume
        }
    }
}
