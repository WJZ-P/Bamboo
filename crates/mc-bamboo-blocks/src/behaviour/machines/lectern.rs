//! `LecternBlock` 和 `LecternBlockEntity` 里不需要菜单的那一段。
//!
//! 书本身在游戏层。这里只改 `has_book`、`powered`、比较器读数和翻页的两刻脉冲。
//! 没有方块实体记录、但状态已经是 `has_book` 的讲台，按方块 oracle 塞进去的默认可写书算：信号 15。
//!
//! The book item lives in the game layer. This file only changes `has_book`, `powered`,
//! the comparator reading, and the two-tick page pulse. A lectern that is already
//! `has_book` with no recorded facts reads as the default writable book the block
//! oracle inserts, signal 15.

use mc_bamboo_core::{BlockPos, Direction, Hand};
use mc_bamboo_data::{items, sounds::sound_event, tags::item_tags};

use crate::{
    behaviour::{Behaviour, common::facing_the_player},
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    level::{Effect, LecternFacts},
    state::{BlockState, HAS_BOOK, POWERED, item_is_tagged},
    world::World,
};

/// `LecternBlock`。
pub(super) struct Lectern;

/// `LecternBlock.PAGE_CHANGE_IMPULSE_TICKS`。
const PAGE_CHANGE_TICKS: i32 = 2;

/// 世界生成或 oracle 直接摆下的 `has_book` 讲台：一本没有页的可写书。
///
/// A `has_book` lectern placed without a recorded book: an empty writable book.
const DEFAULT_BOOK: LecternFacts = LecternFacts {
    page: 0,
    page_count: 0,
    has_content: true,
};

/// `updateBelow`：信号穿过下面一格。朝向那一项我们没有 `Orientation`，邻居更新不带它。
///
/// Signal through the block below. There is no `Orientation` here, so the neighbour
/// update does not carry one.
fn update_below(state: BlockState, world: &mut World, pos: BlockPos) {
    world.update_neighbors_at(pos.below(), state.block());
}

/// 这一格现在的页码。没有 `has_book` 时比较器是 0，不看记录。
///
/// Facts the comparator reads. Without `has_book` the signal is 0.
fn facts_at(world: &World, pos: BlockPos, state: BlockState) -> Option<LecternFacts> {
    if !state.get(HAS_BOOK) {
        return None;
    }
    Some(world.redstone_memory().lectern(pos).unwrap_or(DEFAULT_BOOK))
}

/// 只有物品 ID、没有组件时的书。可写书默认有空的 `writable_book_content`，成书没有内容。
///
/// A book known only by its item id. A writable book defaults to empty content;
/// a written book has no written content until something adds it.
fn facts_for_item(item: i32) -> LecternFacts {
    LecternFacts {
        page: 0,
        page_count: 0,
        has_content: item == items::WRITABLE_BOOK,
    }
}

/// `placeBook`：先 `setBook`（比较器在 `has_book` 还是假的时候响一次），再 `resetBookState(true)`。
///
/// `setBook` notifies the comparator while `has_book` is still false, then
/// `resetBookState(true)` flips the state.
fn place_book(state: BlockState, world: &mut World, pos: BlockPos, item: i32) {
    let mut facts = world
        .redstone_memory_mut()
        .take_pending_lectern()
        .unwrap_or_else(|| facts_for_item(item));
    // `setBook` 把页码写成 0，不经过 `setPage`，所以不会夹到 -1。
    // `setBook` assigns page 0 directly, not through `setPage`, so it is not clamped to -1.
    facts.page = 0;
    world.redstone_memory_mut().set_lectern(pos, facts);
    world.update_neighbour_for_output_signal(pos, state.block());
    let with_book = state.with(POWERED, false).with(HAS_BOOK, true);
    world.set_block_and_update(pos, with_book);
    update_below(state, world, pos);
    // `placeBook` 最后才出声。
    world.play_block_sound(pos, sound_event("minecraft:item.book.put"), 1.0, 1.0);
}

/// `LecternBlock.resetBookState`。`has_book` 为假时丢掉页码。
///
/// Clears the recorded pages when the book is going away.
pub fn reset_lectern(world: &mut World, pos: BlockPos, has_book: bool) {
    let state = world.block_state(pos);
    if !state.is_same_block(BlockState(mc_bamboo_data::blocks::LECTERN)) {
        return;
    }
    if !has_book {
        world.redstone_memory_mut().clear_lectern(pos);
    }
    let next = state.with(POWERED, false).with(HAS_BOOK, has_book);
    world.set_block_and_update(pos, next);
    update_below(state, world, pos);
}

/// `LecternBlockEntity.setPage` 里页码真的变了之后的 `signalPageChange`。
///
/// 调用方要先把新页码写进记录。比较器先看到新页（`has_book` 仍为真），然后才把 `powered` 拉高。
///
/// The caller stores the new page first. The comparator sees it while `has_book`
/// is still true, and only then does `powered` turn on.
pub fn signal_lectern_page(world: &mut World, pos: BlockPos) {
    let state = world.block_state(pos);
    if !state.is_same_block(BlockState(mc_bamboo_data::blocks::LECTERN)) {
        return;
    }
    world.update_neighbour_for_output_signal(pos, state.block());
    world.set_block_and_update(pos, state.with(POWERED, true));
    update_below(state, world, pos);
    world.schedule_tick(pos, state.block(), PAGE_CHANGE_TICKS);
    world.push_effect(Effect::PageTurned { pos });
}

impl Behaviour for Lectern {
    /// 官方只给有 `canUseGameMasterBlocks` 的玩家读物品里的书，这里没有权限系统。
    fn state_for_placement(
        &self,
        block: &'static mc_bamboo_data::Block,
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
            place_book(state, world, ctx.clicked_pos(), ctx.item);
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

    /// 翻页的脉冲到期：`changePowered(false)`。状态没变也照样 `updateBelow`。
    ///
    /// The page pulse ends with `changePowered(false)`. `updateBelow` still runs
    /// when the state does not change.
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

    /// `LecternBlock.getAnalogOutputSignal`。没有书是 0。
    ///
    /// No book reads 0. A recorded book uses `LecternBlockEntity.getRedstoneSignal`.
    fn analog_output_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        facts_at(world, pos, state).map_or(0, LecternFacts::signal)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Direction;
    use mc_bamboo_data::{blocks, items};
    use mc_bamboo_protocol::item::ItemStack;

    use super::*;
    use crate::{
        BlockTicks, Level, destroy_block,
        testing::{TestLevel, actor_holding, top_hit},
        use_item_on,
        world::World,
    };

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn lectern() -> BlockState {
        BlockState(blocks::LECTERN)
    }

    fn signal(level: &mut TestLevel) -> i32 {
        let mut ticks = BlockTicks::new();
        let world = World::new(level, &mut ticks);
        let state = world.block_state(POS);
        state.analog_output_signal(&world, POS, Direction::Up)
    }

    fn place(level: &mut TestLevel, ticks: &mut BlockTicks, item: i32) {
        let actor = actor_holding(item);
        use_item_on(level, ticks, &actor, Hand::Main, &top_hit(POS));
    }

    #[test]
    fn a_bookless_lectern_reads_zero_and_a_bare_has_book_reads_fifteen() {
        let mut level = TestLevel::new();
        level.set(POS, lectern());
        assert_eq!(signal(&mut level), 0);

        level.set(POS, lectern().with(HAS_BOOK, true));
        assert_eq!(signal(&mut level), 15);
    }

    #[test]
    fn placing_a_writable_book_reads_fifteen_and_a_written_book_reads_fourteen() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        level.set(POS, lectern());
        place(&mut level, &mut ticks, items::WRITABLE_BOOK);
        assert!(level.get(POS).get(HAS_BOOK));
        assert!(!level.get(POS).get(POWERED));
        assert_eq!(signal(&mut level), 15);

        level.set(POS, lectern());
        level.redstone_mut().clear_lectern(POS);
        place(&mut level, &mut ticks, items::WRITTEN_BOOK);
        assert_eq!(signal(&mut level), 14);
        assert_eq!(
            level.redstone().lectern(POS),
            Some(LecternFacts {
                page: 0,
                page_count: 0,
                has_content: false,
            })
        );
    }

    #[test]
    fn a_deposited_book_starts_on_page_zero() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        level.set(POS, lectern());
        level.redstone_mut().set_pending_lectern(LecternFacts {
            page: 5,
            page_count: 3,
            has_content: true,
        });
        place(&mut level, &mut ticks, items::WRITTEN_BOOK);
        assert_eq!(
            level.redstone().lectern(POS),
            Some(LecternFacts {
                page: 0,
                page_count: 3,
                has_content: true,
            })
        );
        assert_eq!(signal(&mut level), 1);
    }

    #[test]
    fn turning_a_page_powers_for_two_ticks_and_the_comparator_follows_the_page() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        level.set(POS, lectern().with(HAS_BOOK, true));
        level.redstone_mut().set_lectern(
            POS,
            LecternFacts {
                page: 0,
                page_count: 3,
                has_content: true,
            },
        );

        let effects = {
            let mut world = World::new(&mut level, &mut ticks);
            world.redstone_memory_mut().set_lectern(
                POS,
                LecternFacts {
                    page: 1,
                    page_count: 3,
                    has_content: true,
                },
            );
            signal_lectern_page(&mut world, POS);
            let state = world.block_state(POS);
            assert!(state.get(POWERED));
            assert_eq!(state.analog_output_signal(&world, POS, Direction::Up), 8);
            world.into_effects()
        };
        assert_eq!(effects, vec![Effect::PageTurned { pos: POS }]);

        let still = crate::run_scheduled_ticks(&mut level, &mut ticks, 1);
        assert!(still.effects.is_empty());
        assert!(level.get(POS).get(POWERED));

        let ended = crate::run_scheduled_ticks(&mut level, &mut ticks, 2);
        assert!(ended.effects.is_empty());
        assert!(!level.get(POS).get(POWERED));
        assert!(level.get(POS).get(HAS_BOOK));
        assert_eq!(signal(&mut level), 8);
    }

    #[test]
    fn the_signal_formula_matches_get_redstone_signal() {
        let cases = [
            (0, 0, true, 15),
            (0, 1, true, 15),
            (0, 0, false, 14),
            (0, 3, true, 1),
            (1, 3, true, 8),
            (2, 3, true, 15),
            (0, 2, true, 1),
            (1, 2, true, 15),
            (-1, 0, true, 15),
        ];
        for (page, page_count, has_content, expected) in cases {
            assert_eq!(
                LecternFacts {
                    page,
                    page_count,
                    has_content,
                }
                .signal(),
                expected,
                "page {page} of {page_count}, content {has_content}"
            );
        }
    }

    #[test]
    fn removing_the_book_clears_power_pages_and_the_signal() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        level.set(POS, lectern().with(HAS_BOOK, true).with(POWERED, true));
        level.redstone_mut().set_lectern(
            POS,
            LecternFacts {
                page: 2,
                page_count: 3,
                has_content: true,
            },
        );
        {
            let mut world = World::new(&mut level, &mut ticks);
            reset_lectern(&mut world, POS, false);
            world.update_neighbour_for_output_signal(POS, world.block_state(POS).block());
        }
        let state = level.get(POS);
        assert!(!state.get(HAS_BOOK));
        assert!(!state.get(POWERED));
        assert_eq!(level.redstone().lectern(POS), None);
        assert_eq!(signal(&mut level), 0);
    }

    #[test]
    fn breaking_a_lectern_does_not_draw_extra_random_for_the_book() {
        let mut empty = TestLevel::new();
        empty.set(POS, lectern());
        let mut with_book = TestLevel::new();
        with_book.set(POS, lectern().with(HAS_BOOK, true));
        with_book.redstone_mut().set_lectern(POS, DEFAULT_BOOK);
        with_book.set_container(POS, vec![ItemStack::new(items::WRITABLE_BOOK, 1)]);

        destroy_block(
            &mut empty,
            &mut BlockTicks::new(),
            &actor_holding(items::AIR),
            POS,
        );
        destroy_block(
            &mut with_book,
            &mut BlockTicks::new(),
            &actor_holding(items::AIR),
            POS,
        );
        // `ItemEntity` 用自己的 `RandomSource`，不碰 `level.random`。两边取的数要一样多。
        // The dropped item uses the entity's own `RandomSource`, not `level.random`.
        assert_eq!(
            empty.random.source().seed(),
            with_book.random.source().seed()
        );
        assert_eq!(with_book.redstone().lectern(POS), None);
    }
}
