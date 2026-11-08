//! 只有界面和比较器输出的容器：`BarrelBlock`、`ShulkerBoxBlock`、`AbstractFurnaceBlock` 的三个子类、
//! `HopperBlock`、`DispenserBlock`、`DropperBlock`。
//!
//! 内容在方块实体里，这里不做，所以打开木桶不会改 `open`，`tick` 里的 `recheckOpen` 也没有东西要查。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use super::facing_the_view;
use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, facing_the_player, update_neighbours_after_destroy},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, ENABLED, FACING, FACING_HOPPER, TRIGGERED},
    world::{UPDATE_CLIENTS, World},
};

/// `BarrelBlock`。
pub(super) struct Barrel;

impl Behaviour for Barrel {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_view(block, ctx))
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }
}

/// `ShulkerBoxBlock`。盖子打开时的形状跟着方块实体走，这里的盒子永远是关着的。
pub(super) struct ShulkerBox;

impl Behaviour for ShulkerBox {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(default_state(block).with(FACING, ctx.clicked_face()))
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }
}

/// `AbstractFurnaceBlock`：`FurnaceBlock`、`BlastFurnaceBlock`、`SmokerBlock` 只有界面不同。
pub(super) struct Furnace;

impl Behaviour for Furnace {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_player(block, ctx))
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }
}

/// `HopperBlock`。
pub(super) struct Hopper;

/// `checkPoweredState`。
fn check_powered_state(state: BlockState, world: &mut World, pos: BlockPos) {
    let should_be_on = !world.has_neighbor_signal(pos);
    if should_be_on != state.get(ENABLED) {
        world.set_block(pos, state.with(ENABLED, should_be_on), UPDATE_CLIENTS);
    }
}

impl Behaviour for Hopper {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        let toward_clicked = ctx.clicked_face().opposite();
        let facing = if toward_clicked.axis().is_vertical() {
            Direction::Down
        } else {
            toward_clicked
        };
        let state = default_state(block)
            .with(FACING_HOPPER, facing)
            .with(ENABLED, true);
        Some(state)
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) {
            check_powered_state(state, world, pos);
        }
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        check_powered_state(state, world, pos);
    }

    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }
}

/// `TRIGGER_DURATION`：发射器被触发之后隔这么多刻发射。
const TRIGGER_DURATION: i32 = 4;

/// `DispenserBlock` 和 `DropperBlock`。被信号触发时排一个计划刻，真正发射物品要方块实体里的
/// 内容，不在范围里。两个类只有发射的方式不同。
pub(super) struct Dispenser;

impl Behaviour for Dispenser {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_view(block, ctx))
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        let should_trigger =
            world.has_neighbor_signal(pos) || world.has_neighbor_signal(pos.above());
        if should_trigger && !state.get(TRIGGERED) {
            world.schedule_tick(pos, state.block(), TRIGGER_DURATION);
            world.set_block(pos, state.with(TRIGGERED, true), UPDATE_CLIENTS);
        } else if !should_trigger && state.get(TRIGGERED) {
            world.set_block(pos, state.with(TRIGGERED, false), UPDATE_CLIENTS);
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
}
