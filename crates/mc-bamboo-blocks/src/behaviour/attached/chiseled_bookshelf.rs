//! `ChiseledBookShelfBlock`。
//!
//! 书在方块实体的物品栏里。`setItem` 会改 `slot_N_occupied`，并记下最近动过的那一格。
//! 比较器读的是那一格的序号加一，记在 [`RedstoneMemory`](crate::RedstoneMemory) 里。
//!
//! `ChiseledBookShelfBlock`.
//!
//! Books live in the block entity inventory. `setItem` updates `slot_N_occupied` and remembers
//! the slot. The comparator reads that slot's index plus one, stored on
//! [`RedstoneMemory`](crate::RedstoneMemory).

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, tags::item_tags};
use mc_bamboo_protocol::item::ItemStack;

use super::slot_container::hit_slot;
use crate::{
    behaviour::{
        Behaviour,
        common::{facing_the_player, update_neighbours_after_destroy},
    },
    block_container::set_bookshelf_slot,
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    item_exchange::ItemExchange,
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
        set_bookshelf_slot(world, ctx.clicked_pos(), slot, ItemStack::new(ctx.item, 1));
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
        // `removeItem(slot, 1)`，放不进背包时 `Player.drop(..., SERVER_ONLY)`。
        // `removeItem(slot, 1)`. What does not fit is `Player.drop(..., SERVER_ONLY)`.
        let pos = ctx.clicked_pos();
        let current = world
            .slots_at(pos)
            .into_iter()
            .nth(slot)
            .unwrap_or_else(ItemStack::empty);
        let (taken, left) = split_one(current);
        if !taken.is_empty() {
            world.set_item_exchange(ItemExchange::AddOrDrop(taken));
        }
        set_bookshelf_slot(world, pos, slot, left);
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

/// `Container.removeItem(slot, 1)`：拿走一件，剩下的留在格子里。
///
/// `Container.removeItem(slot, 1)`: take one item and leave the rest in the slot.
fn split_one(mut stack: ItemStack) -> (ItemStack, ItemStack) {
    if stack.is_empty() {
        return (ItemStack::empty(), ItemStack::empty());
    }
    let mut taken = stack.clone();
    taken.count = 1;
    stack.count -= 1;
    if stack.count <= 0 {
        stack = ItemStack::empty();
    }
    (taken, stack)
}
