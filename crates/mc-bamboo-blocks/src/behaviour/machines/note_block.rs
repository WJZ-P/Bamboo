//! `NoteBlock`。
//!
//! 音符本身是方块事件和声音，这里只有状态：调音、乐器和 `powered`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, tags::item_tags};

use crate::{
    behaviour::{Behaviour, Plain, common::default_state},
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, NOTE, NOTEBLOCK_INSTRUMENT, NoteBlockInstrument, POWERED, item_is_tagged},
    world::World,
};

/// `NoteBlock`。
pub(super) struct Note;

/// `NoteBlockInstrument.worksAboveNoteBlock`：头颅放在音符盒上面才算数，别的方块放在下面。
const fn works_above_note_block(instrument: NoteBlockInstrument) -> bool {
    matches!(
        instrument,
        NoteBlockInstrument::Zombie
            | NoteBlockInstrument::Skeleton
            | NoteBlockInstrument::Creeper
            | NoteBlockInstrument::Dragon
            | NoteBlockInstrument::WitherSkeleton
            | NoteBlockInstrument::Piglin
            | NoteBlockInstrument::CustomHead
    )
}

/// `NoteBlock.playNote`：乐器是头颅，或者上面那格是空气时排一个方块事件。
/// 声音本身和游戏事件不在这里，排进队列的 0 号事件由 [`Note::trigger_event`] 处理。
fn play_note(state: BlockState, world: &mut World, pos: BlockPos) {
    let instrument = state.get(NOTEBLOCK_INSTRUMENT);
    let works_above = works_above_note_block(instrument);
    if works_above || world.block_state(pos.above()).is_air() {
        world.block_event(pos, state.block(), 0, 0);
    }
}

/// `NoteBlockInstrument.hasCustomSound`：只有自定义头颅。歌名来自头颅方块实体的组件，
/// Bamboo 不保存组件，所以这种乐器官方的 `getCustomSoundId` 也是空，方块事件处理失败、不取随机数。
const fn has_custom_sound(instrument: NoteBlockInstrument) -> bool {
    matches!(instrument, NoteBlockInstrument::CustomHead)
}

/// `setInstrument`。
fn with_instrument(state: BlockState, world: &World, pos: BlockPos) -> BlockState {
    let above = world.block_state(pos.above()).instrument();
    if works_above_note_block(above) {
        return state.with(NOTEBLOCK_INSTRUMENT, above);
    }
    let below = world.block_state(pos.below()).instrument();
    let instrument = if works_above_note_block(below) {
        NoteBlockInstrument::Harp
    } else {
        below
    };
    state.with(NOTEBLOCK_INSTRUMENT, instrument)
}

impl Behaviour for Note {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        Some(with_instrument(
            default_state(block),
            world,
            ctx.clicked_pos(),
        ))
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
        if direction.axis().is_vertical() {
            return with_instrument(state, world, pos);
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        let signal = world.has_neighbor_signal(pos);
        if signal != state.get(POWERED) {
            if signal {
                play_note(state, world, pos);
            }
            world.set_block_and_update(pos, state.with(POWERED, signal));
        }
    }

    /// `NoteBlock.triggerEvent`。声音的种子在官方是 `level.getRandom().nextLong()`，
    /// 这里没有声音，但随机数照取，后面的方块刻才和官方对得上。
    fn trigger_event(
        &self,
        state: BlockState,
        world: &mut World,
        _pos: BlockPos,
        _param_a: i32,
        _param_b: i32,
    ) -> bool {
        if has_custom_sound(state.get(NOTEBLOCK_INSTRUMENT)) {
            return false;
        }
        world.random().next_long();
        true
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let is_top_instrument = item_is_tagged(ctx.item, item_tags::NOTEBLOCK_TOP_INSTRUMENTS);
        if is_top_instrument && ctx.clicked_face() == Direction::Up {
            return InteractionResult::Pass;
        }
        Plain.use_item_on(state, world, ctx)
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let state = state.cycle(NOTE);
        world.set_block_and_update(ctx.clicked_pos(), state);
        play_note(state, world, ctx.clicked_pos());
        InteractionResult::Success
    }
}
