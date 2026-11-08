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
use mc_bamboo_data::{Block, items, sounds::sound_event, tags::item_tags};
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
        let pos = ctx.clicked_pos();
        set_bookshelf_slot(world, pos, slot, ItemStack::new(ctx.item, 1));
        // `addBook`：`setItem` 之后按放进去的是不是附魔书选声音，`except` 是空。
        // `addBook`: after `setItem`, the sound depends on whether the book is enchanted.
        // `except` is null.
        let sound = if ctx.item == items::ENCHANTED_BOOK {
            "minecraft:block.chiseled_bookshelf.insert.enchanted"
        } else {
            "minecraft:block.chiseled_bookshelf.insert"
        };
        world.play_block_sound(pos, sound_event(sound), 1.0, 1.0);
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
        let pos = ctx.clicked_pos();
        // `removeBook` 先取出再出声，声音看取出来的那本是不是附魔书。
        // `removeBook` removes the book first, then plays. The sound follows that book.
        let taken = world
            .slots_at(pos)
            .get(slot)
            .cloned()
            .unwrap_or_else(ItemStack::empty);
        set_bookshelf_slot(world, pos, slot, ItemStack::empty());
        let sound = if !taken.is_empty() && taken.item == items::ENCHANTED_BOOK {
            "minecraft:block.chiseled_bookshelf.pickup.enchanted"
        } else {
            "minecraft:block.chiseled_bookshelf.pickup"
        };
        world.play_block_sound(pos, sound_event(sound), 1.0, 1.0);
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
