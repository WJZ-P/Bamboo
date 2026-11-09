//! 这个 crate 的入口：玩家的动作和到期的计划刻，对应官方 `ServerPlayerGameMode` 的同名方法。
//!
//! 够距检查、序列号和发包由调用方处理。

use mc_bamboo_core::{BlockPos, Hand};
use mc_bamboo_data::{BlockAncestor, items};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    context::{Actor, BlockHit, UseContext},
    fluid::{
        self,
        bucket::{self, Bucket},
    },
    item_exchange::ItemExchange,
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
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub result: InteractionResult,
    /// 这次点击走了方块的 `useWithoutItem`。容器就是在这里打开的，调用方要接着接线。
    pub used_without_item: bool,
    /// `BlockItem.place` 放下的那一格。调用方要把物品上的组件搬进这一格的方块实体，
    /// 见 [`World::placed_at`]。
    pub placed_at: Option<BlockPos>,
    /// 这次右键要换给玩家的物品。没有时是 `None`。
    ///
    /// The item this click hands back to the player, or `None` when the hand stays as it is.
    pub item_exchange: Option<ItemExchange>,
    /// 要从方块拿进物品栏的一堆。没有物品实体，放不下的部分会消失。
    ///
    /// A stack to add to the inventory. There is no item entity, so a remainder vanishes.
    pub gained: Option<mc_bamboo_protocol::item::ItemStack>,
    pub effects: Vec<Effect>,
}

/// `ServerPlayerGameMode.useItemOn` 里非旁观者的那一支。
///
/// 旁观者在游戏层单独处理：有菜单就打开，传送门发提示，其余直接放过，不会进到这里。
pub fn use_item_on(
    level: &mut dyn Level,
    ticks: &mut BlockTicks,
    actor: &Actor,
    hand: Hand,
    hit: &BlockHit,
) -> Outcome {
    use_item_on_checked(level, ticks, actor, hand, hit, |_| true)
}

/// 和 [`use_item_on`] 一样，`may_use_on` 是官方 `ItemStack.useOn` 开头那道检查：
/// 冒险模式且不能建造时，手里的 `can_place_on` 必须匹配点到的方块。
///
/// 检查在方块自己的右键之后、物品的 `useOn` 之前，看的是点到的这一格**现在**的状态。
/// 返回 `false` 时不调用物品的 `useOn`，结果和官方一样是 [`InteractionResult::Pass`]。
pub fn use_item_on_checked(
    level: &mut dyn Level,
    ticks: &mut BlockTicks,
    actor: &Actor,
    hand: Hand,
    hit: &BlockHit,
    may_use_on: impl FnOnce(BlockState) -> bool,
) -> Outcome {
    use_item_on_checked_with_stack(level, ticks, actor, hand, hit, None, may_use_on)
}

/// 和 [`use_item_on_checked`] 一样，另外带上这只手里的物品堆。
///
/// 炼药锅要看 `potion_contents`、`dyed_color` 和 `banner_patterns`。没传物品堆时按默认组件判断。
///
/// [`use_item_on_checked`] plus the stack in that hand.
///
/// Cauldrons read `potion_contents`, `dyed_color` and `banner_patterns`. Without a stack, those
/// checks use the item's default components.
pub fn use_item_on_checked_with_stack(
    level: &mut dyn Level,
    ticks: &mut BlockTicks,
    actor: &Actor,
    hand: Hand,
    hit: &BlockHit,
    stack: Option<&ItemStack>,
    may_use_on: impl FnOnce(BlockState) -> bool,
) -> Outcome {
    let mut world = World::new(level, ticks);
    let (result, used_without_item) =
        run_use_item_on(&mut world, actor, hand, *hit, stack, may_use_on);
    outcome_from(world, result, used_without_item)
}

/// [`use_item_on`] 带上这只手里的物品堆。
///
/// [`use_item_on`] with the stack in that hand.
pub fn use_item_on_with_stack(
    level: &mut dyn Level,
    ticks: &mut BlockTicks,
    actor: &Actor,
    hand: Hand,
    hit: &BlockHit,
    stack: &ItemStack,
) -> Outcome {
    use_item_on_checked_with_stack(level, ticks, actor, hand, hit, Some(stack), |_| true)
}

fn run_use_item_on(
    world: &mut World,
    actor: &Actor,
    hand: Hand,
    hit: BlockHit,
    stack: Option<&ItemStack>,
    may_use_on: impl FnOnce(BlockState) -> bool,
) -> (InteractionResult, bool) {
    let state = world.block_state(hit.pos);
    let mut ctx = UseContext::new(actor, hand, hit);
    ctx.stack = stack;
    // 空物品堆在官方就是空气，不覆盖调用方已经放好的物品 ID。
    // An empty stack is air in vanilla, so it does not replace the item id the caller set.
    if let Some(stack) = stack.filter(|stack| !stack.is_empty()) {
        ctx.item = stack.item;
    }
    let holds_something = actor.main_hand != items::AIR || actor.off_hand != items::AIR;
    if !(actor.secondary_use && holds_something)
        && let Some((result, used_without_item)) = use_block(state, world, &ctx)
    {
        return (result, used_without_item);
    }
    if ctx.hand_is_empty() {
        (InteractionResult::Pass, false)
    } else if may_use_on(world.block_state(hit.pos)) {
        (placement::use_on(world, &ctx), false)
    } else {
        (InteractionResult::Pass, false)
    }
}

/// 方块自己的右键。不消耗这次点击时返回 `None`，官方接着让物品试。
///
/// 第二个返回值表示结果来自 `useWithoutItem`。
fn use_block(
    state: BlockState,
    world: &mut World,
    ctx: &UseContext,
) -> Option<(InteractionResult, bool)> {
    let item_use = state.use_item_on(world, ctx);
    if item_use.consumes_action() {
        return Some((item_use, false));
    }
    if item_use == InteractionResult::TryEmptyHandInteraction && ctx.hand == Hand::Main {
        let hand_use = state.use_without_item(world, ctx);
        if hand_use.consumes_action() {
            return Some((hand_use, true));
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
    let result = match Bucket::of(actor.item_in(hand)) {
        Some(bucket) => bucket::use_bucket(&mut world, actor, bucket),
        None => placement::use_in_air(&mut world, actor, hand),
    };
    outcome_from(world, result, false)
}

/// `state.attack(level, pos, player)`：生存模式开始挖时打的那一下。
///
/// 红石矿会亮起来、龙蛋会传送、音符盒会响，别的方块什么也不做。
/// 官方的 `Player.attack` 还管实体，这里只有方块的这一半。
pub fn attack(
    level: &mut dyn Level,
    ticks: &mut BlockTicks,
    actor: &Actor,
    pos: BlockPos,
) -> Outcome {
    let mut world = World::new(level, ticks);
    let state = world.block_state(pos);
    state.attack(&mut world, pos, actor);
    outcome_from(world, InteractionResult::Pass, false)
}

/// `ServerPlayerGameMode.destroyBlock` 里 `canDestroyBlock` 之后的部分。
///
/// 调试棒的 `canDestroyBlock` 恒返回假，方块不会被挖掉。`GameMasterBlock` 只有
/// `canUseGameMasterBlocks` 为真才挖得动。
///
/// The part of `ServerPlayerGameMode.destroyBlock` after `canDestroyBlock`.
///
/// A debug stick's `canDestroyBlock` always returns false, so the block stays.
/// A `GameMasterBlock` breaks only when `canUseGameMasterBlocks` is true.
pub fn destroy_block(
    level: &mut dyn Level,
    ticks: &mut BlockTicks,
    actor: &Actor,
    pos: BlockPos,
) -> Outcome {
    let mut world = World::new(level, ticks);
    let state = world.block_state(pos);
    // `DebugStickItem.canDestroyBlock` 对任何玩家都返回 false。左键改属性在游戏层做。
    //
    // `DebugStickItem.canDestroyBlock` returns false for every player. The left-click
    // property change happens in the game layer.
    if actor.main_hand == items::DEBUG_STICK {
        return outcome_from(world, InteractionResult::Fail, false);
    }
    // `ServerPlayerGameMode.destroyBlock`：`GameMasterBlock` 且没有权限时发一次方块更新并返回 false。
    //
    // `ServerPlayerGameMode.destroyBlock` sends a block update and returns false for a
    // `GameMasterBlock` when the player lacks permission.
    if state.is_a(BlockAncestor::GameMasterBlock) && !actor.can_use_game_master_blocks {
        return outcome_from(world, InteractionResult::Fail, false);
    }
    let state = state.player_will_destroy(&mut world, pos, actor);
    if world.remove_block(pos) {
        state.destroy(&mut world, pos);
    }
    // `Block.dropResources` 先看 `GameRules.BLOCK_DROPS`，再看创造模式的 `preventsBlockDrops`。
    // 物品实体还没接上。默认开着，这一步不取随机数。
    //
    // `Block.dropResources` checks `GameRules.BLOCK_DROPS`, then creative `preventsBlockDrops`.
    // Item entities are not wired yet. The default is on, and this step draws no random numbers.
    let _allow_drops =
        crate::block_drops_allowed(world.block_game_rules(), actor.prevents_block_drops);
    // `preventsBlockDrops`（创造模式）在 `mineBlock` 之前返回。剪刀和普通工具都在这里扣耐久。
    //
    // `preventsBlockDrops` (creative) returns before `mineBlock`. Shears and other tools spend durability here.
    if !actor.prevents_block_drops {
        crate::durability::mine_block(&world, state);
    }
    outcome_from(world, InteractionResult::Success, false)
}

/// 一刻里的世界，`now` 是这一刻的 `gameTime`。
///
/// 和官方 `ServerLevel.tick` 一样按顺序跑：到期的方块刻、到期的流体刻、方块事件、方块实体。
/// 实体刻不在这里。官方的方块 oracle 不 tick 实体，游戏循环另外调 [`run_entity_tick`]。
///
/// Scheduled block ticks, fluid ticks, block events, then block entities, matching
/// that part of `ServerLevel.tick`. Entity ticks are separate: the block oracle does
/// not tick entities, and the game loop calls [`run_entity_tick`] on its own.
pub fn run_scheduled_ticks(level: &mut dyn Level, ticks: &mut BlockTicks, now: i64) -> Outcome {
    ticks.collect_due(now);
    let mut world = World::in_world_tick(level, ticks);
    run_block_ticks(&mut world);
    world.ticks_mut().collect_due_fluids();
    while let Some((pos, fluid)) = world.ticks_mut().next_fluid_due() {
        fluid::tick(&mut world, pos, fluid);
    }
    world.run_block_events();
    world.end_handling_tick();
    world.tick_block_entities();
    outcome_from(world, InteractionResult::Pass, false)
}

/// `ServerLevel.tick` 开头的两段：到期的方块刻，然后到期的流体刻。`handlingTick` 为真。
///
/// 游戏循环按官方顺序分几步调用：[`run_due_ticks`]、随机刻（[`tick_chunk`](crate::tick_chunk)）、
/// [`run_block_event_phase`]、[`run_entity_tick`]、[`run_block_entity_tick`]。
/// [`run_scheduled_ticks`] 是去掉随机刻和实体的整段，给只关心方块的测试用。
///
/// The first two parts of `ServerLevel.tick`: due block ticks, then due fluid ticks, with
/// `handlingTick` set. The game loop calls the phases in vanilla order: [`run_due_ticks`], random
/// ticks ([`tick_chunk`](crate::tick_chunk)), [`run_block_event_phase`], [`run_entity_tick`], then
/// [`run_block_entity_tick`]. [`run_scheduled_ticks`] runs the whole thing without random ticks
/// and entities, for tests that only care about blocks.
pub fn run_due_ticks(level: &mut dyn Level, ticks: &mut BlockTicks, now: i64) -> Outcome {
    ticks.collect_due(now);
    let mut world = World::in_world_tick(level, ticks);
    run_block_ticks(&mut world);
    world.ticks_mut().collect_due_fluids();
    while let Some((pos, fluid)) = world.ticks_mut().next_fluid_due() {
        fluid::tick(&mut world, pos, fluid);
    }
    outcome_from(world, InteractionResult::Pass, false)
}

/// `ServerLevel.runBlockEvents`：在随机刻之后、实体之前，`handlingTick` 仍为真。
///
/// `ServerLevel.runBlockEvents`: after random ticks and before entities, while `handlingTick`
/// is still set.
pub fn run_block_event_phase(level: &mut dyn Level, ticks: &mut BlockTicks) -> Outcome {
    let mut world = World::in_world_tick(level, ticks);
    world.run_block_events();
    outcome_from(world, InteractionResult::Pass, false)
}

/// `Level.tickBlockEntities`：一刻的最后一段，在实体之后，`handlingTick` 已经放回假。
///
/// `Level.tickBlockEntities`: the last part of the tick, after entities, once `handlingTick`
/// is cleared.
pub fn run_block_entity_tick(level: &mut dyn Level, ticks: &mut BlockTicks) -> Outcome {
    let mut world = World::new(level, ticks);
    world.tick_block_entities();
    // `CrafterBlockEntity.serverTick` 也排在方块实体这一段。方块 oracle 的假世界不登记
    // 合成器的 ticker，settle 走的是 `run_scheduled_ticks`，所以只有这里会减倒计时。
    //
    // `CrafterBlockEntity.serverTick` runs in this phase too. The block oracle's fake level does
    // not register a crafter ticker, and its settle uses `run_scheduled_ticks`, so only this
    // phase decrements the countdown.
    crate::crafter::tick_crafting_clocks(&mut world);
    outcome_from(world, InteractionResult::Pass, false)
}

/// oracle 的 tick 题：官方的题在动作里直接调用这一格的 `state.tick`，还没进 `ServerLevel.tick`，
/// 所以只跑到期的方块刻，流体刻、方块事件和方块实体都留到后面的刻。
pub fn run_action_block_ticks(level: &mut dyn Level, ticks: &mut BlockTicks, now: i64) -> Outcome {
    ticks.collect_due(now);
    let mut world = World::new(level, ticks);
    run_block_ticks(&mut world);
    outcome_from(world, InteractionResult::Pass, false)
}

/// `EntityTickList.forEach`：世界里的每个实体各走一个 tick。tick 期间生成的实体
/// 下一 tick 才开始动。
///
/// `EntityTickList.forEach`: each live entity ticks once. Entities spawned during
/// the tick wait until the next one.
pub fn run_entity_tick(level: &mut dyn Level, ticks: &mut BlockTicks) -> Outcome {
    let mut world = World::new(level, ticks);
    world.tick_entities();
    finish(world, InteractionResult::Pass)
}

/// 到期的方块计划刻，`ServerLevel.tick` 和 tick 题的动作都用这一段。
fn run_block_ticks(world: &mut World) {
    while let Some((pos, block)) = world.ticks_mut().next_due() {
        let state = world.block_state(pos);
        if state.block_id() == block {
            state.tick(world, pos);
        }
    }
}

pub(crate) fn finish(world: World, result: InteractionResult) -> Outcome {
    outcome_from(world, result, false)
}

/// 把这一次操作收成 [`Outcome`]。
///
/// Fold one operation into an [`Outcome`].
fn outcome_from(world: World, result: InteractionResult, used_without_item: bool) -> Outcome {
    let placed_at = world.placed_at();
    let item_exchange = world.item_exchange().cloned();
    let gained = world.gained().cloned();
    Outcome {
        result,
        used_without_item,
        placed_at,
        item_exchange,
        gained,
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
    fn using_a_chest_reports_the_empty_hand_path() {
        let mut level = level();
        level.set(FLOOR.above(), BlockState(blocks::CHEST).default_state());
        let outcome = click(
            &mut level,
            &actor_holding(items::AIR),
            top_hit(FLOOR.above()),
        );
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(outcome.used_without_item);
    }

    #[test]
    fn a_plain_block_with_an_empty_hand_does_not_use_the_empty_hand_path() {
        let mut level = level();
        let outcome = click(&mut level, &actor_holding(items::AIR), top_hit(FLOOR));
        assert_eq!(outcome.result, InteractionResult::Pass);
        assert!(!outcome.used_without_item);
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
    fn the_use_on_gate_sees_the_clicked_block_and_skips_the_item() {
        let mut level = level();
        let actor = actor_holding(items::DIRT);
        let seen = std::cell::Cell::new(None);
        let mut ticks = BlockTicks::new();
        let outcome = use_item_on_checked(
            &mut level,
            &mut ticks,
            &actor,
            Hand::Main,
            &top_hit(FLOOR),
            |state| {
                seen.set(Some(state));
                false
            },
        );
        // 官方 `ItemStack.useOn` 看的是点到的这一格。
        assert_eq!(seen.get(), Some(BlockState(blocks::STONE)));
        assert_eq!(outcome.result, InteractionResult::Pass);
        assert!(level.get(FLOOR.above()).is_air());

        let mut ticks = BlockTicks::new();
        let outcome = use_item_on_checked(
            &mut level,
            &mut ticks,
            &actor,
            Hand::Main,
            &top_hit(FLOOR),
            |_| true,
        );
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(level.get(FLOOR.above()), BlockState(blocks::DIRT));
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
