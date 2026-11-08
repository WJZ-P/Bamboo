//! `NoteBlock`。
//!
//! 音符本身是方块事件和声音，这里只有状态：调音、乐器和 `powered`。

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{Block, sounds, tags::item_tags};

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

/// `NoteBlock.getPitchFromNote`：音高是 2 的 `(note - 12) / 12` 次方，先按 double 算再变 float。
fn pitch_from_note(note: i32) -> f32 {
    2.0_f64.powf(f64::from(note - 12) / 12.0) as f32
}

/// `NoteBlock.playNote`：乐器不是基础乐器时，上方必须是空气才响。方块事件和声音都在这里发出。
fn play_note(world: &mut World, state: BlockState, pos: BlockPos) {
    if !state.get(NOTEBLOCK_INSTRUMENT).works_above_note_block()
        && !world.block_state(pos.above()).is_air()
    {
        return;
    }
    // `Level.blockEvent` 调的是这一格现在的 `triggerEvent`，`useWithoutItem` 写完方块后的
    // 连锁可能已经把乐器换掉了，声音和音高要用读回来的状态。
    world.block_event(pos, 0, 0);
    let current = world.block_state(pos);
    let instrument = current.get(NOTEBLOCK_INSTRUMENT);
    // `triggerEvent`：自定义头颅的声音要读方块实体，这个世界没有方块实体，官方这时返回 false。
    if instrument.has_custom_sound() {
        return;
    }
    let pitch = if instrument.is_tunable() {
        pitch_from_note(current.get(NOTE))
    } else {
        1.0
    };
    // 官方的种子是 `level.getRandom().nextLong()`，在 `playSeededSound` 的参数里取。
    let seed = world.random().next_long();
    world.play_seeded_sound(
        Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + 0.5,
        ),
        instrument.sound(),
        sounds::RECORDS,
        3.0,
        pitch,
        seed,
    );
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
                // 官方在改状态之前响，读的是还有红石信号时的状态。
                play_note(world, state, pos);
            }
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
        let tuned = state.cycle(NOTE);
        world.set_block_and_update(ctx.clicked_pos(), tuned);
        play_note(world, tuned, ctx.clicked_pos());
        InteractionResult::Success
    }
}
