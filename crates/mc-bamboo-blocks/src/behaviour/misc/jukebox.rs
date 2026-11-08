//! `JukeboxBlock`。唱片在方块实体的物品栏里，和漏斗、玩家右键是同一份。
//!
//! `JukeboxBlock`. The disc lives in the block entity inventory, shared with hoppers and the
//! player's right click.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    behaviour::{Behaviour, common::update_neighbours_after_destroy},
    block_container,
    context::UseContext,
    interact::InteractionResult,
    state::{BlockState, HAS_RECORD},
    world::World,
};

/// `JukeboxBlock`。
pub(super) struct Jukebox;

impl Behaviour for Jukebox {
    /// `JukeboxBlock.ownSignal`：歌在放时发 15。
    ///
    /// `JukeboxBlock.ownSignal`: 15 while a song is playing.
    fn own_signal(&self, _state: BlockState, world: &World, pos: BlockPos) -> i32 {
        if world.redstone_memory().is_jukebox_playing(pos) {
            15
        } else {
            0
        }
    }

    /// `JukeboxBlock.getAnalogOutputSignal`：读唱片的 `comparator_output`，没在放也一样。
    ///
    /// `JukeboxBlock.getAnalogOutputSignal`: the disc's `comparator_output`, whether or not it
    /// is playing.
    fn analog_output_signal(
        &self,
        _state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        block_container::jukebox_signal(world, pos)
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        // `JukeboxBlock.affectNeighborsAfterRemoval` 只有 `Containers.updateNeighboursAfterDestroy`。
        // 弹出唱片的 `onSongChanged`、1011 和两个 `nextFloat` 在更早的 `preRemoveSideEffects` 里。
        //
        // `JukeboxBlock.affectNeighborsAfterRemoval` is only `Containers.updateNeighboursAfterDestroy`.
        // The pop-out `onSongChanged`, the 1011, and the two `nextFloat`s already ran in
        // `preRemoveSideEffects`.
        update_neighbours_after_destroy(state, world, pos);
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if state.get(HAS_RECORD) || !block_container::item_has_jukebox_song(ctx.item) {
            return InteractionResult::TryEmptyHandInteraction;
        }
        // `setTheItem` 里的 `play` 会报 1010。这里再报一次就会重复。
        // `play` inside `setTheItem` sends 1010. Sending it again here would duplicate it.
        block_container::set_jukebox_item(world, ctx.clicked_pos(), ItemStack::new(ctx.item, 1));
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
        // `has_record` 为真就返回成功。物品是空的时 `popOutTheItem` 什么也不做，状态保持不变。
        //
        // Success whenever `has_record` is set. `popOutTheItem` does nothing when the item is
        // empty, and the state stays as it is.
        block_container::pop_jukebox_record(world, ctx.clicked_pos());
        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::{blocks, items, sounds};

    use super::*;
    use crate::{
        LevelRandom,
        level::Effect,
        testing::{TestLevel, actor_holding, top_hit, use_item},
    };

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn level_events(effects: &[Effect]) -> Vec<(i32, i32)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::LevelEvent { event, data, .. } => Some((*event, *data)),
                _ => None,
            })
            .collect()
    }

    /// 放进唱片时报 1010，data 是歌典的注册表 ID。
    #[test]
    fn putting_a_disc_in_announces_the_song() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::JUKEBOX));
        let outcome = use_item(
            &mut level,
            &actor_holding(items::MUSIC_DISC_13),
            top_hit(POS),
        );
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(level.get(POS).get(HAS_RECORD));
        let song = sounds::jukebox_song(items::MUSIC_DISC_13).expect("唱片有歌典");
        assert_eq!(level_events(&outcome.effects), [(1010, song)]);
    }

    /// 空手弹出唱片：歌停了，只抽两次随机数给掉落物的位置。
    #[test]
    fn taking_the_disc_out_stops_the_song_and_draws_twice() {
        let mut level = TestLevel::new();
        level.set(POS, BlockState(blocks::JUKEBOX));
        use_item(
            &mut level,
            &actor_holding(items::MUSIC_DISC_13),
            top_hit(POS),
        );
        let outcome = use_item(&mut level, &actor_holding(items::AIR), top_hit(POS));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(!level.get(POS).get(HAS_RECORD));
        // 正在放的歌：`stop` 报一次 1011。`setRemoved` 不在玩家取出这条路上。
        // A playing song: `stop` sends one 1011. `setRemoved` is not on the player's eject path.
        assert_eq!(level_events(&outcome.effects), [(1011, 0)]);
        let expected = LevelRandom::new(0);
        expected.source().next_float();
        expected.source().next_float();
        assert_eq!(level.random.source().seed(), expected.source().seed());
    }
}
