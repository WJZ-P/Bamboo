//! `JukeboxBlock`。唱片本身在方块实体里，不在范围里，这里只有 `has_record` 和它引起的邻居更新。

use mc_bamboo_core::BlockPos;

use crate::{
    behaviour::{
        Behaviour,
        common::{item_of, update_neighbours_after_destroy},
    },
    context::UseContext,
    interact::InteractionResult,
    state::{BlockState, HAS_RECORD},
    world::{UPDATE_CLIENTS, World},
};

/// `JukeboxBlock`。
pub(super) struct Jukebox;

/// `JukeboxBlockEntity.onSongChanged`：唱机的信号跟着歌变，告诉邻居和比较器。
fn on_song_changed(state: BlockState, world: &mut World, pos: BlockPos) {
    world.update_neighbors_at(pos, state.block());
    world.update_neighbour_for_output_signal(pos, state.block());
}

impl Behaviour for Jukebox {
    /// `JukeboxBlock.ownSignal`：歌在放时发 15。唱片在方块实体里，这里记在 `RedstoneMemory` 上。
    fn own_signal(&self, _state: BlockState, world: &World, pos: BlockPos) -> i32 {
        if world.redstone_memory().is_jukebox_playing(pos) {
            15
        } else {
            0
        }
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        // 方块实体的 `preRemoveSideEffects`：破坏时弹出唱片。
        if state.get(HAS_RECORD) {
            on_song_changed(state, world, pos);
        }
        update_neighbours_after_destroy(state, world, pos);
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if state.get(HAS_RECORD) || !item_of(ctx.item).is_some_and(|item| item.jukebox_playable) {
            return InteractionResult::TryEmptyHandInteraction;
        }
        let pos = ctx.clicked_pos();
        world.set_block(pos, state.with(HAS_RECORD, true), UPDATE_CLIENTS);
        // `JukeboxBlockEntity.setTheItem` 会开始放歌。
        world.redstone_memory_mut().set_jukebox_playing(pos, true);
        on_song_changed(state, world, pos);
        InteractionResult::Success
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if !state.get(HAS_RECORD) {
            return InteractionResult::Pass;
        }
        let pos = ctx.clicked_pos();
        world.set_block(pos, state.with(HAS_RECORD, false), UPDATE_CLIENTS);
        // `popOutTheItem` 之后歌停了。
        world.redstone_memory_mut().set_jukebox_playing(pos, false);
        on_song_changed(state, world, pos);
        InteractionResult::Success
    }
}
