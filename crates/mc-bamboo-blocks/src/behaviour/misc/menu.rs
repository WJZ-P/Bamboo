//! 只有界面的方块：`BeaconBlock`、`CartographyTableBlock`、`CraftingTableBlock`、
//! `EnchantingTableBlock`、`SmithingTableBlock`、`BrewingStandBlock`。
//!
//! 酿造台在这里建方块实体。菜单本身在 `mc-bamboo-game`。
//!
//! Menu-only blocks. The brewing stand creates its block entity here. The menu lives in
//! `mc-bamboo-game`.

use mc_bamboo_core::{BlockPos, Direction};

use super::super::machines::BrewingStandEntity;
use crate::{
    behaviour::{Behaviour, common::update_neighbours_after_destroy},
    container::redstone_signal,
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
    /// `newBlockEntity`：放下酿造台时建一个空实体。
    ///
    /// `newBlockEntity`: placing a brewing stand creates an empty entity.
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) && !world.has_block_entity(pos) {
            world.set_block_entity(Box::new(BrewingStandEntity::new(pos)));
        }
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }

    /// `BrewingStandBlock.getAnalogOutputSignal`。
    ///
    /// `BrewingStandBlock.getAnalogOutputSignal`.
    fn analog_output_signal(
        &self,
        _state: BlockState,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        world
            .block_entity::<BrewingStandEntity>(pos)
            .map(|entity| redstone_signal(entity.items().iter(), 5))
            .unwrap_or(0)
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
