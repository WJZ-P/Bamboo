//! `JukeboxBlock`。唱片本身在方块实体里，不在范围里，这里只有 `has_record` 和它引起的邻居更新。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::sounds::jukebox_song;

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
        // `JukeboxSongPlayer.play` 报 1010，data 是歌典的注册表 ID；物品组件里带着这个引用。
        if let Some(song) = jukebox_song(ctx.item) {
            world.level_event(1010, pos, song);
        }
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
        // `popOutTheItem` 生成掉落物时用 `offsetRandomXZ` 抽两个 `nextFloat`。这里没有实体，
        // 只照抽，让后面的音高和官方用同一段随机数。
        world.random().next_float();
        world.random().next_float();
        // `popOutTheItem` 之后歌停了。
        world.redstone_memory_mut().set_jukebox_playing(pos, false);
        on_song_changed(state, world, pos);
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
        level.set(POS, BlockState(blocks::JUKEBOX).with(HAS_RECORD, true));
        let outcome = use_item(&mut level, &actor_holding(items::AIR), top_hit(POS));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(!level.get(POS).get(HAS_RECORD));
        assert!(level_events(&outcome.effects).is_empty());
        let expected = LevelRandom::new(0);
        expected.source().next_float();
        expected.source().next_float();
        assert_eq!(level.random.source().seed(), expected.source().seed());
    }
}
