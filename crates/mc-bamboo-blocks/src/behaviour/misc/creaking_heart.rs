//! `CreakingHeartBlock`。放下时看两头的苍白橡木，之后的状态变化在计划刻里，不在范围里。

use mc_bamboo_core::{Axis, BlockPos};
use mc_bamboo_data::{Block, tags::block_tags};

use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, update_neighbours_after_destroy},
    },
    context::PlaceContext,
    state::{AXIS, BlockState, CREAKING_HEART_STATE, CreakingHeartState},
    world::World,
};

/// `CreakingHeartBlock`。
pub(super) struct CreakingHeart;

/// `hasRequiredLogs`：这条轴两头都是同一条轴的苍白橡木。
fn has_required_logs(world: &World, pos: BlockPos, axis: Axis) -> bool {
    [axis.positive(), axis.negative()].into_iter().all(|side| {
        let neighbor = world.block_state(pos.relative(side));
        neighbor.is_tagged(block_tags::PALE_OAK_LOGS) && neighbor.get(AXIS) == axis
    })
}

impl Behaviour for CreakingHeart {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let axis = ctx.clicked_face().axis();
        let state = default_state(block).with(AXIS, axis);
        // 这个世界一直是白天，`CREAKING_ACTIVE` 是假，所以不会是 `AWAKE`。
        Some(if has_required_logs(world, ctx.clicked_pos(), axis) {
            state.with(CREAKING_HEART_STATE, CreakingHeartState::Dormant)
        } else {
            state
        })
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
    }
}
