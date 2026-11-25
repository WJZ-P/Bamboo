//! `LeadItem.useOn`：对着栅栏把玩家牵着的生物系到栅栏上。

use crate::{InteractionResult, context::UseContext, world::World};

/// `LeadItem.useOn`：`blockState(pos).is(#fences)` 且非客户端且玩家非空时
/// `bindPlayerMobs(player, level, pos)`，否则 `PASS`。
pub(super) fn use_on(world: &mut World, ctx: &UseContext) -> InteractionResult {
    let pos = ctx.clicked_pos();
    if world
        .block_state(pos)
        .is_tagged(mc_bamboo_data::tags::block_tags::FENCES)
    {
        crate::entity::leash_knot::bind_player_mobs(world, ctx.actor.uid, pos)
    } else {
        InteractionResult::Pass
    }
}
