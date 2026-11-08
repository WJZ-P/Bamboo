//! `RedStoneOreBlock`：点一下就亮，随机刻时熄灭。

use mc_bamboo_core::BlockPos;

use crate::{
    behaviour::Behaviour,
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, LIT, block_of_item},
    world::World,
};

/// `RedStoneOreBlock`。
pub(super) struct RedstoneOre;

impl Behaviour for RedstoneOre {
    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if !state.get(LIT) {
            world.set_block_and_update(ctx.clicked_pos(), state.with(LIT, true));
        }
        let is_block_item = !block_of_item(ctx.item).is_air();
        if is_block_item && PlaceContext::new(*ctx, world).can_place(world) {
            InteractionResult::Pass
        } else {
            InteractionResult::Success
        }
    }

    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(LIT) {
            world.set_block_and_update(pos, state.with(LIT, false));
        }
    }
}
