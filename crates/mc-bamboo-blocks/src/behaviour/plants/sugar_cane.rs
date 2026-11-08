//! `SugarCaneBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::tags::{block_tags, fluid_tags};

use super::{break_if_unsupported, schedule_break_if_unsupported};
use crate::{
    behaviour::{Behaviour, Plain},
    state::{AGE_15, BlockState},
    world::{UPDATE_NONE, World},
};

/// 甘蔗最多长到三格高。
const MAX_HEIGHT: i32 = 3;
/// `age` 数到这里就长一格。
const MAX_AGE: i32 = 15;
/// 官方写 `age` 用的标记，260：`UPDATE_NONE`。
const GROWTH_FLAGS: u32 = UPDATE_NONE;

/// `SugarCaneBlock`。
pub(super) struct SugarCane;

/// 这一格能给旁边的甘蔗供水。
fn waters_sugar_cane(world: &World, pos: BlockPos) -> bool {
    world
        .fluid_state(pos)
        .is_tagged(fluid_tags::SUPPORTS_SUGAR_CANE_ADJACENTLY)
        || world
            .block_state(pos)
            .is_tagged(block_tags::SUPPORTS_SUGAR_CANE_ADJACENTLY)
}

impl Behaviour for SugarCane {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below = world.block_state(below_pos);
        below.is_same_block(state)
            || below.is_tagged(block_tags::SUPPORTS_SUGAR_CANE)
                && Direction::HORIZONTAL
                    .into_iter()
                    .any(|side| waters_sugar_cane(world, below_pos.relative(side)))
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
        schedule_break_if_unsupported(state, world, pos);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        break_if_unsupported(state, world, pos);
    }

    /// 上面空着、不到三格高时，`age` 数到 15 就往上长一格。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if !world.is_empty_block(pos.above()) || stack_height(state, world, pos) >= MAX_HEIGHT {
            return;
        }
        let age = state.get(AGE_15);
        if age == MAX_AGE {
            world.set_block_and_update(pos.above(), state.default_state());
            world.set_block(pos, state.with(AGE_15, 0), GROWTH_FLAGS);
        } else {
            world.set_block(pos, state.with(AGE_15, age + 1), GROWTH_FLAGS);
        }
    }
}

/// 从这一格往下数，连着的甘蔗有几格，算上自己。
fn stack_height(state: BlockState, world: &World, pos: BlockPos) -> i32 {
    let mut height = 1;
    while world
        .block_state(pos.relative_by(Direction::Down, height))
        .is_same_block(state)
    {
        height += 1;
    }
    height
}
