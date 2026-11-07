//! `RedstoneTorchBlock`、`RedstoneWallTorchBlock`。
//!
//! 世界里没有信号，火把永远亮着：`neighborChanged` 和 `tick` 里熄灭和烧坏的逻辑走不到。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks};

use crate::{
    behaviour::{
        Behaviour,
        common::{BaseTorch, default_state, has_sturdy_wall, lost_wall_support},
        state_for_placement,
    },
    context::PlaceContext,
    state::{BlockState, HORIZONTAL_FACING},
    world::World,
};

/// `RedstoneTorchBlock`。
pub(super) struct RedstoneTorch;

/// `notifyNeighbors`：信号穿过周围六格，所以通知的是隔一格的方块。
fn notify_neighbors(state: BlockState, world: &mut World, pos: BlockPos) {
    for direction in Direction::ALL {
        world.update_neighbors_at(pos.relative(direction), state.block());
    }
}

impl Behaviour for RedstoneTorch {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        BaseTorch.can_survive(state, world, pos)
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
        BaseTorch.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        notify_neighbors(state, world, pos);
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        notify_neighbors(state, world, pos);
    }
}

/// `RedstoneWallTorchBlock`。
pub(super) struct RedstoneWallTorch;

impl Behaviour for RedstoneWallTorch {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let wall_torch = BlockState(blocks::WALL_TORCH).block();
        let placed = state_for_placement(wall_torch, ctx, world)?;
        Some(default_state(block).with(HORIZONTAL_FACING, placed.get(HORIZONTAL_FACING)))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        has_sturdy_wall(state, world, pos)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        if lost_wall_support(state, world, pos, direction) {
            BlockState::AIR
        } else {
            state
        }
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        notify_neighbors(state, world, pos);
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        notify_neighbors(state, world, pos);
    }
}
