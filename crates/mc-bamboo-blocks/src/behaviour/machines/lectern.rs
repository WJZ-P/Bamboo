//! `LecternBlock`。
//!
//! 翻页的红石脉冲要界面才排得出来，这里只有到期时把信号收回来的 `tick`。
//!
//! 比较器读出来的强度看书翻到第几页。这里的物品都是默认组件，书一页也没有，
//! 官方这时按读完了算：`floor(1.0 * 14) + 1`，所以有书就是 15。

use mc_bamboo_core::{BlockPos, Direction, Hand};
use mc_bamboo_data::{Block, sounds::sound_event, tags::item_tags};

use crate::{
    behaviour::{Behaviour, common::facing_the_player},
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, HAS_BOOK, POWERED, item_is_tagged},
    world::World,
};

/// `LecternBlock`。
pub(super) struct Lectern;

/// 没有页的书放在讲台上时比较器读到的强度。
const BLANK_BOOK_SIGNAL: i32 = 15;

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
    // `placeBook` 最后才出声。
    world.play_block_sound(pos, sound_event("minecraft:item.book.put"), 1.0, 1.0);
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

    /// 翻页的脉冲到期：`changePowered(false)`。
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        world.set_block_and_update(pos, state.with(POWERED, false));
        update_below(state, world, pos);
    }

    fn own_signal(&self, state: BlockState, _world: &World, _pos: BlockPos) -> i32 {
        if state.get(POWERED) { 15 } else { 0 }
    }

    /// 只强充能脚下那一格。
    fn direct_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        if direction == Direction::Up {
            state.own_signal(world, pos)
        } else {
            0
        }
    }

    /// 见模块的说明，没有书时是 0。
    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        if state.get(HAS_BOOK) {
            BLANK_BOOK_SIGNAL
        } else {
            0
        }
    }
}
