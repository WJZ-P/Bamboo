//! `FenceBlock`。
//!
//! 官方的 `useWithoutItem` 是把拴绳系到栅栏上。这个世界看不到实体，结果总是 `PASS`，
//! 和默认实现相同，所以这里没有写。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor, tags::block_tags};

use super::{fence_gate, has_full_face};
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, is_exception_for_connection, is_water_source, pipe_property,
            tick_water_if_waterlogged,
        },
    },
    context::PlaceContext,
    state::{BlockState, WATERLOGGED},
    world::World,
};

/// `FenceBlock`。
pub(super) struct Fence;

/// `isSameFence`：木栅栏只连木栅栏，下界砖栅栏只连下界砖栅栏。
fn is_same_fence(fence: BlockState, other: BlockState) -> bool {
    other.is_tagged(block_tags::FENCES)
        && other.is_tagged(block_tags::WOODEN_FENCES) == fence.is_tagged(block_tags::WOODEN_FENCES)
}

/// `connectsTo`。`face` 是 `other` 朝着栅栏的面。
fn connects_to(fence: BlockState, other: BlockState, face: Direction) -> bool {
    let gate =
        other.is_a(BlockAncestor::FenceGateBlock) && fence_gate::connects_to_direction(other, face);
    !is_exception_for_connection(other) && has_full_face(other, face)
        || is_same_fence(fence, other)
        || gate
}

impl Behaviour for Fence {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let default = default_state(block);
        let state = Direction::HORIZONTAL
            .into_iter()
            .fold(default, |state, side| {
                let neighbor = world.block_state(pos.relative(side));
                state.with(
                    pipe_property(side),
                    connects_to(default, neighbor, side.opposite()),
                )
            });
        Some(state.with(WATERLOGGED, is_water_source(world, pos)))
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
        tick_water_if_waterlogged(state, world, pos);
        if direction.axis().is_horizontal() {
            let connected = connects_to(state, neighbor, direction.opposite());
            state.with(pipe_property(direction), connected)
        } else {
            Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        behaviour::building::test_support::{FLOOR, level_with_floor, top_of},
        state::{NORTH, SOUTH},
        testing::{actor_holding, use_item},
    };

    #[test]
    fn full_faces_on_the_exception_list_do_not_connect() {
        let mut level = level_with_floor();
        let fence = FLOOR.above();
        level.set(fence.north(), BlockState(blocks::OAK_LEAVES));
        level.set(fence.south(), BlockState(blocks::STONE));
        use_item(&mut level, &actor_holding(items::OAK_FENCE), top_of(FLOOR));
        let placed = level.get(fence);
        assert!(!placed.get(NORTH));
        assert!(placed.get(SOUTH));
    }
}
