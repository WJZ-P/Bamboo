//! `ComposterBlock`。
//!
//! 一个物品能不能加一层是随机的，第 1 章说随机分支取一个确定的值，这里总是加一层。
//! 官方必定成功的物品（蛋糕、南瓜派）和这里一样，别的物品 oracle 排除。

use mc_bamboo_core::{BlockPos, Direction};

use crate::{
    behaviour::{Behaviour, common::item_of},
    context::UseContext,
    interact::InteractionResult,
    state::{BlockState, LEVEL_COMPOSTER},
    world::World,
};

/// `ComposterBlock`。
pub(super) struct Composter;

/// `READY`：可以取骨粉了。
const READY: i32 = 8;
/// `MAX_LEVEL`：满了，等计划刻变成 [`READY`]。
const MAX_LEVEL: i32 = 7;
/// 满了之后等多久变成 [`READY`]。
const READY_DELAY: i32 = 20;

/// `addLayer`。
fn add_layer(state: BlockState, world: &mut World, pos: BlockPos) {
    let level = state.get(LEVEL_COMPOSTER) + 1;
    world.set_block_and_update(pos, state.with(LEVEL_COMPOSTER, level));
    if level == MAX_LEVEL {
        world.schedule_tick(pos, state.block(), READY_DELAY);
    }
}

impl Behaviour for Composter {
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        if state.get(LEVEL_COMPOSTER) == MAX_LEVEL {
            world.schedule_tick(pos, state.block(), READY_DELAY);
        }
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let level = state.get(LEVEL_COMPOSTER);
        if level >= READY || !item_of(ctx.item).is_some_and(|item| item.compostable) {
            return InteractionResult::TryEmptyHandInteraction;
        }
        if level < MAX_LEVEL {
            add_layer(state, world, ctx.clicked_pos());
        }
        InteractionResult::Success
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if state.get(LEVEL_COMPOSTER) != READY {
            return InteractionResult::Pass;
        }
        world.set_block_and_update(ctx.clicked_pos(), state.with(LEVEL_COMPOSTER, 0));
        InteractionResult::Success
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(LEVEL_COMPOSTER) == MAX_LEVEL {
            world.set_block_and_update(pos, state.with(LEVEL_COMPOSTER, READY));
        }
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        state.get(LEVEL_COMPOSTER)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        behaviour::misc::test_support::tick,
        testing::{TestLevel, actor_holding, top_hit, use_item},
    };

    const COMPOSTER: BlockPos = BlockPos::new(0, 64, 0);

    /// 官方这里是随机的，oracle 比不了，这里固定下来：种子总是加一层。
    #[test]
    fn seeds_always_add_a_layer() {
        let mut level = TestLevel::new();
        level.set(COMPOSTER, BlockState(blocks::COMPOSTER));
        let actor = actor_holding(items::WHEAT_SEEDS);
        let outcome = use_item(&mut level, &actor, top_hit(COMPOSTER));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(level.get(COMPOSTER).get(LEVEL_COMPOSTER), 1);
    }

    #[test]
    fn a_full_composter_turns_ready_on_its_tick() {
        let mut level = TestLevel::new();
        let full = BlockState(blocks::COMPOSTER).with(LEVEL_COMPOSTER, MAX_LEVEL);
        level.set(COMPOSTER, full);
        tick(&mut level, COMPOSTER);
        assert_eq!(level.get(COMPOSTER).get(LEVEL_COMPOSTER), READY);
    }
}
