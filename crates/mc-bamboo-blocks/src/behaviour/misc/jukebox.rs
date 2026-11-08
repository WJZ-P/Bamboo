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

/// `popOutTheItem` 拿走唱片以后的部分：`setTheItem(EMPTY)` 里的 `stop`，再弹出掉落物。
///
/// 掉落物实体还没有，但官方算它的位置时从世界随机源取了两个数
/// （`offsetRandomXZ(level.getRandom(), 0.7F)`），后面的随机数要接在这里。
fn pop_out_the_item(state: BlockState, world: &mut World, pos: BlockPos) {
    // `JukeboxSongPlayer.stop`：只有正在放歌时才通知。
    if world.redstone_memory().is_jukebox_playing(pos) {
        world.redstone_memory_mut().set_jukebox_playing(pos, false);
        on_song_changed(state, world, pos);
    }
    world.random().next_float();
    world.random().next_float();
    on_song_changed(state, world, pos);
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
        // `JukeboxBlock.affectNeighborsAfterRemoval` 只有 `Containers.updateNeighboursAfterDestroy`，
        // 不取随机数。两个 `nextFloat` 在更早的 `JukeboxBlockEntity.preRemoveSideEffects`：
        // `LevelChunk.setBlockState` 先调它（`popOutTheItem`，物品非空才 `offsetRandomXZ`），
        // 再调这个方法。数在 `block_entity::pre_remove_side_effects` 里已经取过，这里再取会错开后面的随机数。
        // 方块此时已经换成空气，`notifyItemChangedInJukebox` 对不上旧状态，不会再写方块；
        // 有唱片时 `popOutTheItem` 末尾的 `onSongChanged` 仍会通知邻居。
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
        // `popOutTheItem` → `setTheItem(EMPTY)`：先 `notifyItemChangedInJukebox` 写方块，再停歌、弹出。
        world.set_block(pos, state.with(HAS_RECORD, false), UPDATE_CLIENTS);
        pop_out_the_item(state, world, pos);
        InteractionResult::Success
    }
}
