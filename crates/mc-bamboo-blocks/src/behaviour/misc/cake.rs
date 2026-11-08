//! `CakeBlock`、`CandleCakeBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{
    BlockAncestor, block_maps::CANDLE_CAKES, blocks, items, sounds::sound_event, tags::item_tags,
};

use super::candle::extinguish;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{block_by_id, default_state},
    },
    context::UseContext,
    interact::InteractionResult,
    state::{BITES, BlockState, LIT, block_of_item, item_is_tagged},
    world::World,
};

/// `CakeBlock`。
pub(super) struct Cake;

/// `CandleCakeBlock`。
pub(super) struct CandleCake;

/// `MAX_BITES`。
const MAX_BITES: i32 = 6;

/// `CakeBlock.getOutputSignal`。
const fn output_signal(bites: i32) -> i32 {
    (7 - bites) * 2
}

/// `CakeBlock.FULL_CAKE_SIGNAL`。
const FULL_CAKE_SIGNAL: i32 = output_signal(0);

/// `CandleCakeBlock.byCandle`。
fn candle_cake_of(candle: BlockState) -> Option<BlockState> {
    let candle = candle.block_id() as i32;
    CANDLE_CAKES
        .iter()
        .find(|&&(id, _)| id == candle)
        .map(|&(_, cake)| default_state(block_by_id(cake)))
}

/// `CakeBlock.eat`。创造模式的玩家总能吃。
fn eat(state: BlockState, world: &mut World, pos: BlockPos) -> InteractionResult {
    let bites = state.get(BITES);
    if bites < MAX_BITES {
        world.set_block_and_update(pos, state.with(BITES, bites + 1));
    } else {
        world.remove_block(pos);
    }
    InteractionResult::Success
}

/// 两种蛋糕的 `canSurvive`。
fn on_solid_ground(world: &World, pos: BlockPos) -> bool {
    world.block_state(pos.below()).is_solid()
}

/// 两种蛋糕的 `updateShape`。
fn update_shape_on_ground(
    state: BlockState,
    world: &mut World,
    pos: BlockPos,
    direction: Direction,
    neighbor_pos: BlockPos,
    neighbor: BlockState,
) -> BlockState {
    if direction == Direction::Down && !state.can_survive(world, pos) {
        return BlockState::AIR;
    }
    Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
}

/// `candleHit`：点在上半，也就是蜡烛上。
fn candle_hit(ctx: &UseContext) -> bool {
    ctx.click_location().y - f64::from(ctx.clicked_pos().y) > 0.5
}

impl Behaviour for Cake {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        on_solid_ground(world, pos)
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
        update_shape_on_ground(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let candle = block_of_item(ctx.item);
        if item_is_tagged(ctx.item, item_tags::CANDLES)
            && state.get(BITES) == 0
            && candle.is_a(BlockAncestor::CandleBlock)
            && let Some(cake) = candle_cake_of(candle)
        {
            // `useItemOn`：插蜡烛的声音在写方块之前。
            world.play_block_sound(
                ctx.clicked_pos(),
                sound_event("minecraft:block.cake.add_candle"),
                1.0,
                1.0,
            );
            world.set_block_and_update(ctx.clicked_pos(), cake);
            return InteractionResult::Success;
        }
        InteractionResult::TryEmptyHandInteraction
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        eat(state, world, ctx.clicked_pos())
    }

    fn analog_output_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        output_signal(state.get(BITES))
    }
}

impl Behaviour for CandleCake {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        on_solid_ground(world, pos)
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
        update_shape_on_ground(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if ctx.item == items::FLINT_AND_STEEL || ctx.item == items::FIRE_CHARGE {
            return InteractionResult::Pass;
        }
        if candle_hit(ctx) && ctx.hand_is_empty() && state.get(LIT) {
            extinguish(state, world, ctx.clicked_pos());
            return InteractionResult::Success;
        }
        Plain.use_item_on(state, world, ctx)
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        eat(BlockState(blocks::CAKE), world, ctx.clicked_pos())
    }

    /// 带蜡烛的蛋糕一定是完整的。
    fn analog_output_signal(
        &self,
        _state: BlockState,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        FULL_CAKE_SIGNAL
    }
}
