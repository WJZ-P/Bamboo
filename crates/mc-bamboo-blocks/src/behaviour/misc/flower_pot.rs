//! `FlowerPotBlock`：空花盆和种着植物的花盆是不同的方块。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{block_maps::FLOWER_POTS, blocks};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{block_by_id, default_state, field_bool},
    },
    context::UseContext,
    interact::InteractionResult,
    state::{BlockState, block_of_item},
    world::World,
};

/// `FlowerPotBlock`。
pub(super) struct FlowerPot;

/// `POTTED_BY_CONTENT`：手上的物品种下去是什么花盆。官方的表里也有空气，它对应空花盆，不用种。
fn potted_with(item: i32) -> Option<BlockState> {
    let plant = block_of_item(item);
    if plant.is_air() {
        return None;
    }
    let plant = plant.block_id() as i32;
    FLOWER_POTS
        .iter()
        .find(|&&(content, _)| content == plant)
        .map(|&(_, pot)| default_state(block_by_id(pot)))
}

fn is_empty(state: BlockState) -> bool {
    field_bool(state.block(), "isEmpty()")
}

impl Behaviour for FlowerPot {
    fn update_shape(
        &self,
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

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let Some(potted) = potted_with(ctx.item) else {
            return InteractionResult::TryEmptyHandInteraction;
        };
        if !is_empty(state) {
            return InteractionResult::Consume;
        }
        world.set_block_and_update(ctx.clicked_pos(), potted);
        InteractionResult::Success
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if is_empty(state) {
            return InteractionResult::Consume;
        }
        world.set_block_and_update(ctx.clicked_pos(), BlockState(blocks::FLOWER_POT));
        InteractionResult::Success
    }
}
