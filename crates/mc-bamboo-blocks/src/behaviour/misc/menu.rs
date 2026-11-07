//! 只有界面的方块：`BeaconBlock`、`CartographyTableBlock`、`CraftingTableBlock`、
//! `EnchantingTableBlock`、`SmithingTableBlock`、`BrewingStandBlock`。界面不做，这里只吃掉点击。

use mc_bamboo_core::BlockPos;

use crate::{
    behaviour::{Behaviour, common::update_neighbours_after_destroy},
    context::UseContext,
    interact::InteractionResult,
    state::BlockState,
    world::World,
};

/// 打开界面的 `useWithoutItem`。
pub(super) struct Menu;

impl Behaviour for Menu {
    fn use_without_item(
        &self,
        _state: BlockState,
        _world: &mut World,
        _ctx: &UseContext,
    ) -> InteractionResult {
        InteractionResult::Success
    }
}

/// `BrewingStandBlock`。
pub(super) struct BrewingStand;

impl Behaviour for BrewingStand {
    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        Menu.use_without_item(state, world, ctx)
    }
}
