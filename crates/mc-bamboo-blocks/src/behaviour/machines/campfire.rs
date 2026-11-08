//! `CampfireBlock`。
//!
//! 右键食物走 `placeFood`。熄灭和被水浇灭都不把食物弹出来：26.3 的 `douse` 和
//! `placeLiquid` 只改 `lit`。浇灭不扣工具耐久，耐久还没接进右键。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, block_maps, blocks, registries, tags::item_tags};
use mc_bamboo_protocol::item::ItemStack;

use super::campfire_entity::CampfireEntity;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    item_exchange::ItemExchange,
    state::{
        BlockState, FluidState, HORIZONTAL_FACING, LIT, SIGNAL_FIRE, WATERLOGGED, item_is_tagged,
    },
    world::World,
};

/// `CampfireBlock`。
pub(super) struct Campfire;

/// `isSmokeSource`：下面是干草块时烟飘得更高。
fn is_smoke_source(state: BlockState) -> bool {
    state.is(blocks::HAY_BLOCK)
}

impl Behaviour for Campfire {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let water = is_water_source(world, pos);
        let state = default_state(block)
            .with(WATERLOGGED, water)
            .with(SIGNAL_FIRE, is_smoke_source(world.block_state(pos.below())))
            .with(LIT, !water)
            .with(HORIZONTAL_FACING, ctx.horizontal_direction());
        Some(state)
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
        tick_water_if_waterlogged(state, world, pos);
        if direction == Direction::Down {
            return state.with(SIGNAL_FIRE, is_smoke_source(neighbor));
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if block_maps::CAMPFIRE_INPUTS.contains(&ctx.item) {
            return place_food(state, world, ctx);
        }
        if item_is_tagged(ctx.item, item_tags::DOUSES_CAMPFIRES) && state.get(LIT) {
            world.set_block_and_update(ctx.clicked_pos(), state.with(LIT, false));
            return InteractionResult::Success;
        }
        InteractionResult::TryEmptyHandInteraction
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) && !world.has_block_entity(pos) {
            world.set_block_entity(Box::new(CampfireEntity::new(pos)));
        }
    }

    /// 进水的同时熄火。食物留在格子里，下一刻走 `cooldownTick`。
    ///
    /// Waterlogs and extinguishes. Food stays in the slots; the next tick is `cooldownTick`.
    fn place_liquid(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        fluid: FluidState,
    ) -> bool {
        if state.get(WATERLOGGED) || !fluid.is(registries::fluid::WATER) {
            return false;
        }
        let doused = state.with(WATERLOGGED, true).with(LIT, false);
        world.set_block_and_update(pos, doused);
        world.schedule_fluid_tick(pos, fluid.fluid());
        true
    }
}

/// 官方 `useItemOn` 里 `CAMPFIRE_INPUT` 那一支。
///
/// 放成功是 `SUCCESS_SERVER`，格子满了或者没有配方是 `CONSUME`。
/// 创造模式不消耗手里那一个，和 `consumeAndReturn` 一样。
///
/// The `CAMPFIRE_INPUT` branch of vanilla `useItemOn`.
///
/// A successful place is `SUCCESS_SERVER`. A full campfire or a missing recipe is `CONSUME`.
/// Creative mode does not consume the held item, matching `consumeAndReturn`.
fn place_food(state: BlockState, world: &mut World, ctx: &UseContext) -> InteractionResult {
    let pos = ctx.clicked_pos();
    if !state.has_block_entity() {
        return InteractionResult::Consume;
    }
    if !world.has_block_entity(pos) {
        world.set_block_entity(Box::new(CampfireEntity::new(pos)));
    }
    let owned;
    let stack = if let Some(stack) = ctx.stack {
        stack
    } else {
        owned = ItemStack::new(ctx.item, 1);
        &owned
    };
    let placed = world
        .with_block_entity::<CampfireEntity, _>(pos, |entity, world| {
            entity.place_food(world, stack)
        })
        .unwrap_or(false);
    if !placed {
        return InteractionResult::Consume;
    }
    if !ctx.actor.prevents_block_drops {
        world.set_item_exchange(ItemExchange::Shrink);
    }
    InteractionResult::SuccessServer
}
