//! `BigDripleafBlock`、`BigDripleafStemBlock`。`SmallDripleafBlock` 是高植物，
//! 在 [`double_plant`](super::double_plant) 里。
//!
//! 叶子只有实体和弹射物才能压倒，这里没有实体，`tick` 不用移植。
//! 官方的 `neighborChanged` 只看红石信号，世界里没有信号，也不用移植。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks, tags::block_tags};

use super::{break_if_unsupported, schedule_break_if_unsupported};
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    state::{BlockState, HORIZONTAL_FACING, WATERLOGGED},
    world::World,
};

/// `BigDripleafBlock`。
pub(super) struct BigDripleaf;

impl Behaviour for BigDripleaf {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let below = world.block_state(pos.below());
        let on_dripleaf = below.is(blocks::BIG_DRIPLEAF) || below.is(blocks::BIG_DRIPLEAF_STEM);
        let facing = if on_dripleaf {
            below.get(HORIZONTAL_FACING)
        } else {
            ctx.horizontal_direction().opposite()
        };
        let state = default_state(block)
            .with(WATERLOGGED, is_water_source(world, pos))
            .with(HORIZONTAL_FACING, facing);
        Some(state)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = world.block_state(pos.below());
        below.is_same_block(state)
            || below.is(blocks::BIG_DRIPLEAF_STEM)
            || below.is_tagged(block_tags::SUPPORTS_BIG_DRIPLEAF)
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
        if direction == Direction::Down && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        tick_water_if_waterlogged(state, world, pos);
        if direction == Direction::Up && neighbor.is_same_block(state) {
            return state.copied_onto(BlockState(blocks::BIG_DRIPLEAF_STEM).block());
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

/// `BigDripleafStemBlock`。
pub(super) struct BigDripleafStem;

impl Behaviour for BigDripleafStem {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = world.block_state(pos.below());
        let above = world.block_state(pos.above());
        (below.is_same_block(state) || below.is_tagged(block_tags::SUPPORTS_BIG_DRIPLEAF))
            && (above.is_same_block(state) || above.is(blocks::BIG_DRIPLEAF))
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
        if direction.axis().is_vertical() {
            schedule_break_if_unsupported(state, world, pos);
        }
        tick_water_if_waterlogged(state, world, pos);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        break_if_unsupported(state, world, pos);
    }
}
