//! `RedStoneOreBlock`：点一下就亮，熄灭在随机刻里，不在范围里。

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
}
