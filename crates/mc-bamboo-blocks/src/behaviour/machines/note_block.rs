//! `NoteBlock`。
//!
//! 音符本身是方块事件和声音，这里只有状态：调音、乐器和 `powered`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, tags::item_tags};

use crate::{
    behaviour::{Behaviour, Plain, common::default_state},
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    level::Effect,
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

/// `NoteBlock.playNote`：上面是头颅，或者上面是空气，敲下去才响。
///
/// 声音本身是下一 tick 的方块事件，见 [`Note::trigger_event`]。
fn play_note(state: BlockState, world: &mut World, pos: BlockPos) {
    let instrument = state.get(NOTEBLOCK_INSTRUMENT);
    if !works_above_note_block(instrument) && !world.block_state(pos.above()).is_air() {
        return;
    }
    world.block_event(pos, state.block(), 0, 0);
}

/// `NoteBlock.getPitchFromNote`。
fn pitch_from_note(note: i32) -> f32 {
    2.0f64.powf(f64::from(note - 12) / 12.0) as f32
}

impl Behaviour for Note {
    /// `NoteBlock.attack`：拿手打一下和右键一样，响一声。
    fn attack(&self, state: BlockState, world: &mut World, pos: BlockPos, _player: &Actor) {
        play_note(state, world, pos);
    }

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

    /// `NoteBlock.triggerEvent`：算音高，取一个 `nextLong` 当音效种子。
    ///
    /// 自定义头颅的声音要从上面那格的方块实体读，Bamboo 还没有头颅方块实体，
    /// 按 `getCustomSoundId` 找不到处理：返回 `false`，官方也不会取随机数。
    fn trigger_event(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _param_a: i32,
        _param_b: i32,
    ) -> bool {
        let instrument = state.get(NOTEBLOCK_INSTRUMENT);
        let note = state.get(NOTE);
        let pitch = if works_above_note_block(instrument) {
            1.0
        } else {
            pitch_from_note(note)
        };
        if instrument == NoteBlockInstrument::CustomHead {
            return false;
        }
        let seed = world.random().next_long();
        world.push_effect(Effect::NoteBlockPlayed {
            pos,
            instrument,
            note,
            pitch,
            seed,
        });
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
