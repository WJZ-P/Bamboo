//! `FenceGateBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, tags::block_tags};

use crate::{
    behaviour::{Behaviour, Plain, common::default_state},
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, HORIZONTAL_FACING, IN_WALL, OPEN, POWERED},
    world::{UPDATE_CLIENTS, UPDATE_IMMEDIATE, World},
};

/// `FenceGateBlock`。
pub(super) struct FenceGate;

/// `connectsToDirection`：栅栏门只从两侧连接。
pub(super) fn connects_to_direction(gate: BlockState, direction: Direction) -> bool {
    gate.get(HORIZONTAL_FACING).axis() == direction.clockwise().axis()
}

fn is_wall(state: BlockState) -> bool {
    state.is_tagged(block_tags::WALLS)
}

/// 门的两侧有墙时，门整体矮一点，和墙对齐。
fn beside_a_wall(world: &World, pos: BlockPos, facing: Direction) -> bool {
    let side = facing.clockwise();
    is_wall(world.block_state(pos.relative(side)))
        || is_wall(world.block_state(pos.relative(side.opposite())))
}

impl Behaviour for FenceGate {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let open = world.has_neighbor_signal(pos);
        let facing = ctx.horizontal_direction();
        let state = default_state(block)
            .with(HORIZONTAL_FACING, facing)
            .with(OPEN, open)
            .with(POWERED, open)
            .with(IN_WALL, beside_a_wall(world, pos, facing));
        Some(state)
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
        if state.get(HORIZONTAL_FACING).clockwise().axis() != direction.axis() {
            return Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor);
        }
        let opposite = world.block_state(pos.relative(direction.opposite()));
        state.with(IN_WALL, is_wall(neighbor) || is_wall(opposite))
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        let power = world.has_neighbor_signal(pos);
        if state.get(POWERED) != power {
            let updated = state.with(POWERED, power).with(OPEN, power);
            world.set_block(pos, updated, UPDATE_CLIENTS);
        }
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let toggled = if state.get(OPEN) {
            state.with(OPEN, false)
        } else {
            // 从背面打开时门朝玩家的方向转过去，总是往远离玩家的一侧开。
            let direction = ctx.actor.direction();
            let turned = if state.get(HORIZONTAL_FACING) == direction.opposite() {
                state.with(HORIZONTAL_FACING, direction)
            } else {
                state
            };
            turned.with(OPEN, true)
        };
        world.set_block(
            ctx.clicked_pos(),
            toggled,
            UPDATE_CLIENTS | UPDATE_IMMEDIATE,
        );
        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        behaviour::building::test_support::{FLOOR, level_with_floor, top_of},
        testing::{actor_holding, use_item},
    };

    #[test]
    fn opening_from_behind_swings_the_gate_away() {
        let gate = FLOOR.above();
        let mut level = level_with_floor();
        let facing_the_player =
            BlockState(blocks::OAK_FENCE_GATE).with(HORIZONTAL_FACING, Direction::North);
        level.set(gate, facing_the_player);
        // 玩家朝南，门朝北，也就是从门的背面打开。
        use_item(&mut level, &actor_holding(items::AIR), top_of(gate));
        let opened = level.get(gate);
        assert!(opened.get(OPEN));
        assert_eq!(opened.get(HORIZONTAL_FACING), Direction::South);

        use_item(&mut level, &actor_holding(items::AIR), top_of(gate));
        let closed = level.get(gate);
        assert!(!closed.get(OPEN));
        assert_eq!(closed.get(HORIZONTAL_FACING), Direction::South);
    }
}
