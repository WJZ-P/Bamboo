//! `SnowyBlock`、`GrassBlock`、`MyceliumBlock`，后两个的父类 `SpreadingSnowyBlock` 只多了随机刻。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, tags::block_tags};

use crate::{
    behaviour::{Behaviour, Plain, common::default_state},
    context::PlaceContext,
    state::{BlockState, SNOWY},
    world::World,
};

/// `SnowyBlock`：上面有雪时换上雪地的贴图。
pub(super) struct Snowy;

/// `isSnowySetting`。
fn is_snowy_setting(above: BlockState) -> bool {
    above.is_tagged(block_tags::SNOW)
}

impl Behaviour for Snowy {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let above = world.block_state(ctx.clicked_pos().above());
        Some(default_state(block).with(SNOWY, is_snowy_setting(above)))
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
        if direction == Direction::Up {
            state.with(SNOWY, is_snowy_setting(neighbor))
        } else {
            Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
        }
    }
}
