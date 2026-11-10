//! 只有界面的工作方块：`AnvilBlock`、`GrindstoneBlock`、`StonecutterBlock`、`LoomBlock`。
//!
//! 铁砧继承 `FallingBlock`，会掉下去；它只在 `falling(entity)` 里多设了砸实体的伤害，
//! 而实体伤害不在范围里，所以剩下的和沙土一样。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{FaceAttached, default_state, facing_the_player},
        misc::falling::{DELAY, fall_if_free, schedule_fall},
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, HORIZONTAL_FACING},
    world::World,
};

/// `AnvilBlock`。
pub(super) struct Anvil;

impl Behaviour for Anvil {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        let facing = ctx.horizontal_direction().clockwise();
        Some(default_state(block).with(HORIZONTAL_FACING, facing))
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        schedule_fall(state, world, pos, DELAY);
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
        schedule_fall(state, world, pos, DELAY);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if let Some(id) = fall_if_free(state, world, pos) {
            // `AnvilBlock.falling`：`setHurtsEntities(2.0F, 40)`。
            world.with_entity_mut(id, &mut |entity| entity.set_hurts_entities(2.0, 40));
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

/// `GrindstoneBlock`。官方的 `canSurvive` 永远是真，所以父类的 `updateShape` 什么也不做。
pub(super) struct Grindstone;

impl Behaviour for Grindstone {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        FaceAttached.state_for_placement(block, ctx, world)
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

/// `StonecutterBlock`、`LoomBlock`，两个类除了界面完全相同。
pub(super) struct Workstation;

impl Behaviour for Workstation {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_player(block, ctx))
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
