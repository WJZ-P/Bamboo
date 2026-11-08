//! `ChiseledBookShelfBlock`。
//!
//! 书在方块实体里，但是状态上的 `slot_N_occupied` 是它的镜像，这里只改状态。
//! 比较器读的是最近放进或者拿出书的那一格，记在 [`RedstoneMemory`](crate::RedstoneMemory) 里。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, tags::item_tags};

use super::slot_container::hit_slot;
use crate::{
    behaviour::{
        Behaviour,
        common::{facing_the_player, update_neighbours_after_destroy},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{
        BlockState, HORIZONTAL_FACING, Property, SLOT_0_OCCUPIED, SLOT_1_OCCUPIED, SLOT_2_OCCUPIED,
        SLOT_3_OCCUPIED, SLOT_4_OCCUPIED, SLOT_5_OCCUPIED, item_is_tagged,
    },
    world::World,
};

/// `SLOT_OCCUPIED_PROPERTIES`。
const SLOT_OCCUPIED: [Property<bool>; 6] = [
    SLOT_0_OCCUPIED,
    SLOT_1_OCCUPIED,
    SLOT_2_OCCUPIED,
    SLOT_3_OCCUPIED,
    SLOT_4_OCCUPIED,
    SLOT_5_OCCUPIED,
];
/// `getRows`。
const ROWS: usize = 2;
/// `getColumns`。
const COLUMNS: usize = 3;

/// `ChiseledBookShelfBlock`。
pub(super) struct ChiseledBookShelf;

/// 点中的是第几格，没点在书架朝外的那一面时是 `None`。
fn clicked_slot(state: BlockState, ctx: &UseContext) -> Option<usize> {
    let facing = state.get(HORIZONTAL_FACING);
    hit_slot(&ctx.hit, facing, ROWS, COLUMNS)
}

/// `ChiseledBookShelfBlockEntity.updateState`：先记下动的是哪一格，再把状态换成书的镜像。
fn update_state(state: BlockState, world: &mut World, pos: BlockPos, slot: usize, occupied: bool) {
    world
        .redstone_memory_mut()
        .set_last_bookshelf_slot(pos, slot);
    world.set_block_and_update(pos, state.with(SLOT_OCCUPIED[slot], occupied));
}

impl Behaviour for ChiseledBookShelf {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_player(block, ctx))
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if !item_is_tagged(ctx.item, item_tags::BOOKSHELF_BOOKS) {
            return InteractionResult::TryEmptyHandInteraction;
        }
        let Some(slot) = clicked_slot(state, ctx) else {
            return InteractionResult::Pass;
        };
        if state.get(SLOT_OCCUPIED[slot]) {
            return InteractionResult::TryEmptyHandInteraction;
        }
        update_state(state, world, ctx.clicked_pos(), slot, true);
        InteractionResult::Success
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let Some(slot) = clicked_slot(state, ctx) else {
            return InteractionResult::Pass;
        };
        if !state.get(SLOT_OCCUPIED[slot]) {
            return InteractionResult::Consume;
        }
        update_state(state, world, ctx.clicked_pos(), slot, false);
        InteractionResult::Success
    }

    /// 最近动过的那一格的序号加一，没动过是 0。
    fn analog_output_signal(
        &self,
        _state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        world
            .redstone_memory()
            .last_bookshelf_slot(pos)
            .map_or(0, |slot| slot as i32 + 1)
    }
}
