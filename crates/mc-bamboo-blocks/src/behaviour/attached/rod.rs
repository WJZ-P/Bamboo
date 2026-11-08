//! `EndRodBlock`、`LightningRodBlock`、`WeatheringLightningRodBlock`，都继承 `RodBlock`，
//! 它只有形状。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    state::{BlockState, FACING, POWERED, WATERLOGGED},
    world::World,
};

/// `EndRodBlock`。
pub(super) struct EndRod;

impl Behaviour for EndRod {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let default = default_state(block);
        let face = ctx.clicked_face();
        let behind = world.block_state(ctx.clicked_pos().relative(face.opposite()));
        // 接在另一根末地烛的头上时掉过头来，两根头对头。
        let facing = if behind.is_same_block(default) && behind.get(FACING) == face {
            face.opposite()
        } else {
            face
        };
        Some(default.with(FACING, facing))
    }
}

/// `LightningRodBlock`。这个世界没有闪电，打中以后亮起来的那一步不移植，oracle 的排除表里有这一项。
/// `onPlace` 只管带着信号放下的避雷针，玩家放的不会带。8 刻以后把信号收回来的 `tick` 移植了。
pub(super) struct LightningRod;

/// `updateNeighbours`：信号只穿过它插着的那一格。
fn update_neighbours(state: BlockState, world: &mut World, pos: BlockPos) {
    let front = state.get(FACING).opposite();
    world.update_neighbors_at(pos.relative(front), state.block());
}

impl Behaviour for LightningRod {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = default_state(block)
            .with(FACING, ctx.clicked_face())
            .with(WATERLOGGED, is_water_source(world, ctx.clicked_pos()));
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
        tick_water_if_waterlogged(state, world, pos);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(POWERED) {
            update_neighbours(state, world, pos);
        }
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        world.set_block_and_update(pos, state.with(POWERED, false));
        update_neighbours(state, world, pos);
    }

    fn own_signal(&self, state: BlockState, _world: &World, _pos: BlockPos) -> i32 {
        if state.get(POWERED) { 15 } else { 0 }
    }

    /// 只强充能它插着的那一格。
    fn direct_signal(
        &self,
        state: BlockState,
        _world: &World,
        _pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        if state.get(POWERED) && state.get(FACING) == direction {
            15
        } else {
            0
        }
    }
}
