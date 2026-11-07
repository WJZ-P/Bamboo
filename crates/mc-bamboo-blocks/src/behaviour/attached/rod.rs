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

/// `LightningRodBlock`。`onPlace` 和 `tick` 只管闪电打中以后的 8 刻信号，这个世界没有闪电，
/// 不移植，oracle 的计划刻排除表里有这一项。
pub(super) struct LightningRod;

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
            let front = state.get(FACING).opposite();
            world.update_neighbors_at(pos.relative(front), state.block());
        }
    }
}
