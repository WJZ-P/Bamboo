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
fn play_note(state: BlockState, world: &mut World, pos: BlockPos) {
    let instrument = state.get(NOTEBLOCK_INSTRUMENT);
    if !works_above_note_block(instrument) && !world.block_state(pos.above()).is_air() {
        return;
    }
    // `triggerEvent`：头上有自定义声音的头颅时要找方块实体，没有就什么都不放。
    // 头颅的方块实体还没有，`getCustomSoundId` 按找不到算，官方此时直接返回、不取随机数。
    if instrument == NoteBlockInstrument::CustomHead {
        return;
    }
    // 官方在方块事件阶段取一次 `nextLong` 当声音的随机种子，见 `Effect::NoteBlockPlayed`。
    let seed = world.random().next_long();
    let note = state.get(NOTE);
    world.push_effect(Effect::NoteBlockPlayed {
        pos,
        instrument,
        note,
        seed,
    });
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
            world.set_block_and_update(pos, state.with(POWERED, signal));
        }
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
        world.set_block_and_update(ctx.clicked_pos(), state.cycle(NOTE));
        InteractionResult::Success
    }
}
