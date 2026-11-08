//! `CrafterBlock`。被信号触发时排一个计划刻。到期时发失败事件 1050：
//! 真正的合成要方块实体里的物品，不在范围里。官方合成成功的题由 oracle 整题排除。
//!
//! `CrafterBlock`. A redstone signal schedules a tick. The tick plays fail event 1050,
//! because crafting needs the block entity's items, which are out of scope. The oracle
//! excludes a case where the crafter actually crafted.

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, update_neighbours_after_destroy},
    },
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, CRAFTING, FrontAndTop, ORIENTATION, PropertyValue, TRIGGERED},
    world::{UPDATE_CLIENTS, World},
};

/// `CRAFTING_TICK_DELAY`：被触发之后隔这么多刻合成。
/// Ticks between the redstone trigger and the craft attempt.
const CRAFTING_TICK_DELAY: i32 = 4;
/// `LevelEvent.SOUND_CRAFTER_FAIL`。没有配方，或者产物是空的。合成成功不发这个。
/// No recipe, or the result is empty. A successful craft does not send this.
const SOUND_CRAFTER_FAIL: i32 = 1050;

/// `CrafterBlock`。
pub(super) struct Crafter;

/// `FrontAndTop.fromFrontAndTop`：枚举的序列化名就是两个方向的名字。
fn front_and_top(front: Direction, top: Direction) -> FrontAndTop {
    FrontAndTop::from_name(&format!("{}_{}", front.name(), top.name()))
        .expect("the crafter only combines directions that FrontAndTop has")
}

impl Behaviour for Crafter {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let front = ctx.nearest_looking_direction().opposite();
        let top = match front {
            Direction::Down => ctx.horizontal_direction().opposite(),
            Direction::Up => ctx.horizontal_direction(),
            _ => Direction::Up,
        };
        Some(
            default_state(block)
                .with(ORIENTATION, front_and_top(front, top))
                .with(TRIGGERED, world.has_neighbor_signal(ctx.clicked_pos())),
        )
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        if state.get(TRIGGERED) {
            world.schedule_tick(pos, state.block(), CRAFTING_TICK_DELAY);
        }
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        let should_trigger = world.has_neighbor_signal(pos);
        if should_trigger && !state.get(TRIGGERED) {
            world.schedule_tick(pos, state.block(), CRAFTING_TICK_DELAY);
            world.set_block(pos, state.with(TRIGGERED, true), UPDATE_CLIENTS);
        } else if !should_trigger && state.get(TRIGGERED) {
            let released = state.with(TRIGGERED, false).with(CRAFTING, false);
            world.set_block(pos, released, UPDATE_CLIENTS);
        }
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }

    fn tick(&self, _state: BlockState, world: &mut World, pos: BlockPos) {
        // `CrafterBlock.tick` -> `dispenseFrom`。没有配方时官方只 `levelEvent(1050)`。
        // 合成成功会改格子并发 1049/2010，那部分不在范围里，oracle 整题排除。
        // With no recipe, official only sends `levelEvent(1050)`. A successful craft changes
        // the slots and sends 1049/2010; that path is out of scope and the oracle excludes it.
        world.level_event(SOUND_CRAFTER_FAIL, pos, 0);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::blocks;

    use super::*;
    use crate::{BlockState, BlockTicks, World, level::Effect, testing::TestLevel};

    #[test]
    fn a_crafter_tick_plays_the_fail_event() {
        let mut level = TestLevel::new();
        let pos = BlockPos::new(0, 64, 0);
        level.set(pos, BlockState(blocks::CRAFTER));
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.block_state(pos).tick(&mut world, pos);
        assert_eq!(
            world.into_effects(),
            vec![Effect::LevelEvent {
                event: SOUND_CRAFTER_FAIL,
                pos,
                data: 0,
                except_initiator: false,
            }]
        );
    }
}
