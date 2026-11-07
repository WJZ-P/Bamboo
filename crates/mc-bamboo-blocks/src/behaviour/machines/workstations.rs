//! 只有界面的工作方块：`AnvilBlock`、`GrindstoneBlock`、`StonecutterBlock`、`LoomBlock`。
//!
//! 铁砧继承 `FallingBlock`，下落不在范围里，放下和形状更新排的计划刻都不做。

use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour,
        common::{FaceAttached, default_state, facing_the_player},
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
