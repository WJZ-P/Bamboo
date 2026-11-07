//! 这个 crate 的入口：玩家的动作和到期的计划刻，对应官方 `ServerPlayerGameMode` 的同名方法。
//!
//! 够距检查、序列号和发包由调用方处理。

use mc_bamboo_core::{BlockPos, Hand};
use mc_bamboo_data::{BlockAncestor, items};

use crate::{
    context::{Actor, BlockHit, UseContext},
    level::{Effect, Level},
    placement,
    state::BlockState,
    ticks::BlockTicks,
    world::World,
};

/// `InteractionResult`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionResult {
    /// `SUCCESS`：客户端已经预测了挥手。
    Success,
    /// `SUCCESS_SERVER`：客户端没有预测，服务端要发挥手动画。
    SuccessServer,
    /// `CONSUME`：吃掉这次点击，不挥手。
    Consume,
    /// `FAIL`。
    Fail,
    /// `PASS`。
    Pass,
    /// `TRY_WITH_EMPTY_HAND`：方块不认这个物品，官方接着当空手再试一次。
    TryEmptyHandInteraction,
}

impl InteractionResult {
    /// `consumesAction`。
    pub const fn consumes_action(self) -> bool {
        matches!(self, Self::Success | Self::SuccessServer | Self::Consume)
    }
}

/// 一次操作的结果。世界的变化已经通过 [`Level::write_block`] 写进去了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub result: InteractionResult,
    pub effects: Vec<Effect>,
}

/// `ServerPlayerGameMode.useItemOn`，去掉观察者分支。
pub fn use_item_on(
    level: &mut dyn Level,
    ticks: &mut BlockTicks,
    actor: &Actor,
    hand: Hand,
    hit: &BlockHit,
) -> Outcome {
    let mut world = World::new(level, ticks);
    let result = run_use_item_on(&mut world, actor, hand, *hit);
    finish(world, result)
}

fn run_use_item_on(
    world: &mut World,
    actor: &Actor,
    hand: Hand,
    hit: BlockHit,
) -> InteractionResult {
    let state = world.block_state(hit.pos);
    let ctx = UseContext::new(actor, hand, hit);
    let holds_something = actor.main_hand != items::AIR || actor.off_hand != items::AIR;
    if !(actor.secondary_use && holds_something)
        && let Some(result) = use_block(state, world, &ctx)
    {
        return result;
    }
    if ctx.hand_is_empty() {
        InteractionResult::Pass
    } else {
        placement::use_on(world, &ctx)
    }
}

/// 方块自己的右键。不消耗这次点击时返回 `None`，官方接着让物品试。
fn use_block(state: BlockState, world: &mut World, ctx: &UseContext) -> Option<InteractionResult> {
    let item_use = state.use_item_on(world, ctx);
    if item_use.consumes_action() {
        return Some(item_use);
    }
    if item_use == InteractionResult::TryEmptyHandInteraction && ctx.hand == Hand::Main {
        let hand_use = state.use_without_item(world, ctx);
        if hand_use.consumes_action() {
            return Some(hand_use);
        }
    }
    None
}

/// `ServerPlayerGameMode.useItem` 里和方块有关的部分：对着空气或者水用物品。
pub fn use_item(
    level: &mut dyn Level,
    ticks: &mut BlockTicks,
    actor: &Actor,
    hand: Hand,
) -> Outcome {
    let mut world = World::new(level, ticks);
    let result = placement::use_in_air(&mut world, actor, hand);
    finish(world, result)
}

/// `ServerPlayerGameMode.destroyBlock` 里 `canDestroyBlock` 之后的部分。
///
/// 这个世界没有权限系统，官方的 `GameMasterBlock` 破坏不了，返回 [`InteractionResult::Fail`]，
/// 调用方要把这一格发回给客户端。
pub fn destroy_block(
    level: &mut dyn Level,
    ticks: &mut BlockTicks,
    actor: &Actor,
    pos: BlockPos,
) -> Outcome {
    let mut world = World::new(level, ticks);
    let state = world.block_state(pos);
    if state.is_a(BlockAncestor::GameMasterBlock) {
        return finish(world, InteractionResult::Fail);
    }
    state.player_will_destroy(&mut world, pos, actor);
    world.remove_block(pos);
    finish(world, InteractionResult::Success)
}

/// 到期的计划刻，`now` 是这一刻的 `gameTime`。
pub fn run_scheduled_ticks(level: &mut dyn Level, ticks: &mut BlockTicks, now: i64) -> Outcome {
    ticks.collect_due(now);
    let mut world = World::new(level, ticks);
    while let Some((pos, block)) = world.ticks_mut().next_due() {
        let state = world.block_state(pos);
        if state.block_id() == block {
            state.tick(&mut world, pos);
        }
    }
    finish(world, InteractionResult::Pass)
}

fn finish(world: World, result: InteractionResult) -> Outcome {
    Outcome {
        result,
        effects: world.into_effects(),
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Direction;
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        Behaviour,
        testing::{TestLevel, actor_holding, break_block, top_hit, use_item as click},
    };

    const FLOOR: BlockPos = BlockPos::new(0, 64, 0);

    fn level() -> TestLevel {
        let mut level = TestLevel::new();
        level.set(FLOOR, BlockState(blocks::STONE));
        level
    }

    #[test]
    fn only_success_variants_consume_the_action() {
        assert!(InteractionResult::Success.consumes_action());
        assert!(InteractionResult::SuccessServer.consumes_action());
        assert!(InteractionResult::Consume.consumes_action());
        assert!(!InteractionResult::Fail.consumes_action());
        assert!(!InteractionResult::Pass.consumes_action());
        assert!(!InteractionResult::TryEmptyHandInteraction.consumes_action());
    }

    #[test]
    fn empty_hand_on_a_plain_block_passes() {
        let mut level = level();
        let outcome = click(&mut level, &actor_holding(items::AIR), top_hit(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Pass);
    }

    #[test]
    fn crouching_with_an_item_skips_the_block() {
        let mut level = level();
        let mut actor = actor_holding(items::DIRT);
        actor.secondary_use = true;
        let outcome = click(&mut level, &actor, top_hit(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(level.get(FLOOR.above()).is(blocks::DIRT));
    }

    #[test]
    fn breaking_leaves_the_fluid_behind() {
        let mut level = level();
        level.set(
            FLOOR.above(),
            BlockState(blocks::OAK_SLAB).with(crate::state::WATERLOGGED, true),
        );
        let outcome = break_block(&mut level, FLOOR.above());
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(level.get(FLOOR.above()), BlockState::WATER);
        assert_eq!(
            outcome.effects,
            [Effect::Destroyed {
                pos: FLOOR.above(),
                state: BlockState(blocks::OAK_SLAB).with(crate::state::WATERLOGGED, true),
                except_initiator: true,
            }]
        );
    }

    #[test]
    fn game_master_blocks_cannot_be_broken() {
        let mut level = level();
        level.set(FLOOR.above(), BlockState(blocks::COMMAND_BLOCK));
        let outcome = break_block(&mut level, FLOOR.above());
        assert_eq!(outcome.result, InteractionResult::Fail);
        assert!(level.get(FLOOR.above()).is(blocks::COMMAND_BLOCK));
        assert!(outcome.effects.is_empty());
    }

    #[test]
    fn ticks_run_only_when_the_block_is_still_there() {
        let mut level = level();
        let mut ticks = BlockTicks::new();
        let stone = BlockState(blocks::STONE);
        ticks.schedule(FLOOR, stone.block_id(), 1, crate::TickPriority::Normal);
        ticks.schedule(
            FLOOR.above(),
            stone.block_id(),
            1,
            crate::TickPriority::Normal,
        );
        // 默认的 `tick` 什么也不做，这里只验证到期后队列清空，换了方块也不会出错。
        let outcome = run_scheduled_ticks(&mut level, &mut ticks, 1);
        assert_eq!(outcome.result, InteractionResult::Pass);
        assert_eq!(ticks.pending().count(), 0);
    }

    #[test]
    fn plain_defaults_are_the_official_ones() {
        let plain = crate::behaviour::Plain;
        let mut level = level();
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        let actor = actor_holding(items::STONE);
        let ctx = UseContext::new(&actor, Hand::Main, top_hit(FLOOR));
        let stone = BlockState(blocks::STONE);
        assert_eq!(
            plain.use_item_on(stone, &mut world, &ctx),
            InteractionResult::TryEmptyHandInteraction
        );
        assert_eq!(
            plain.use_without_item(stone, &mut world, &ctx),
            InteractionResult::Pass
        );
        assert!(plain.can_survive(stone, &world, FLOOR));
        assert_eq!(
            plain.update_shape(
                stone,
                &mut world,
                FLOOR,
                Direction::Up,
                FLOOR.above(),
                stone
            ),
            stone
        );
    }
}
