//! `BigDripleafBlock`、`BigDripleafStemBlock`。`SmallDripleafBlock` 是高植物，
//! 在 [`double_plant`](super::double_plant) 里。
//!
//! 叶子只有实体和弹射物才能压倒，这里没有实体，`entityInside` 不用移植。
//! 倾倒之后的几步靠计划刻走，有信号就立刻复位，这两条和实体无关，都移植了。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks, tags::block_tags};

use super::{break_if_unsupported, schedule_break_if_unsupported};
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    state::{BlockState, HORIZONTAL_FACING, TILT, Tilt, WATERLOGGED},
    world::{UPDATE_CLIENTS, World},
};

/// `NO_TICK`：这个倾斜状态之后不再排计划刻。
const NO_TICK: i32 = -1;

/// `DELAY_UNTIL_NEXT_TILT_STATE`。
fn delay_until_next_tilt(tilt: Tilt) -> i32 {
    match tilt {
        Tilt::Unstable | Tilt::Partial => 10,
        Tilt::Full => 100,
        Tilt::None => NO_TICK,
    }
}

/// `setTilt`。
fn set_tilt(state: BlockState, world: &mut World, pos: BlockPos, tilt: Tilt) {
    world.set_block(pos, state.with(TILT, tilt), UPDATE_CLIENTS);
}

/// `setTiltAndScheduleTick`。
fn set_tilt_and_schedule_tick(state: BlockState, world: &mut World, pos: BlockPos, tilt: Tilt) {
    set_tilt(state, world, pos, tilt);
    let delay = delay_until_next_tilt(tilt);
    if delay != NO_TICK {
        world.schedule_tick(pos, state.block(), delay);
    }
}

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

    /// 有信号就复位，没有就倒下去一格。
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if world.has_neighbor_signal(pos) {
            set_tilt(state, world, pos, Tilt::None);
            return;
        }
        match state.get(TILT) {
            Tilt::Unstable => set_tilt_and_schedule_tick(state, world, pos, Tilt::Partial),
            Tilt::Partial => set_tilt_and_schedule_tick(state, world, pos, Tilt::Full),
            Tilt::Full => set_tilt(state, world, pos, Tilt::None),
            Tilt::None => {}
        }
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        if world.has_neighbor_signal(pos) {
            set_tilt(state, world, pos, Tilt::None);
        }
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
