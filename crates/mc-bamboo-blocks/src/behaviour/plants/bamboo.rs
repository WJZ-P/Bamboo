//! `BambooSaplingBlock`、`BambooStalkBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks, tags::block_tags};

use super::{break_if_unsupported, schedule_break_if_unsupported};
use crate::{
    behaviour::{Behaviour, Plain, common::default_state},
    context::PlaceContext,
    state::{AGE_1, BAMBOO_LEAVES, BambooLeaves, BlockState, STAGE},
    world::World,
};

/// 竹笋和竹子上面要这么亮才长。
const GROWTH_MIN_BRIGHTNESS: i32 = 9;
/// 竹子最多 16 节，数高度时也最多数到 16。
const MAX_HEIGHT: i32 = 16;
/// 长到这么高以后，每长一节有四分之一的机会停止生长。
const MATURE_HEIGHT: i32 = 11;
const STOP_CHANCE: f32 = 0.25;

/// 两种竹子的 `canSurvive`。
fn has_bamboo_support(world: &World, pos: BlockPos) -> bool {
    world
        .block_state(pos.below())
        .is_tagged(block_tags::SUPPORTS_BAMBOO)
}

/// `BambooSaplingBlock`：竹笋。
pub(super) struct BambooSapling;

impl Behaviour for BambooSapling {
    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        has_bamboo_support(world, pos)
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
        if !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        if direction == Direction::Up && neighbor.is(blocks::BAMBOO) {
            return BlockState(blocks::BAMBOO);
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    /// 三分之一的机会，上面空着又够亮时长出第一节竹子。
    fn random_tick(&self, _state: BlockState, world: &mut World, pos: BlockPos) {
        let above = pos.above();
        if world.random().next_int(3) == 0
            && world.is_empty_block(above)
            && world.raw_brightness(above) >= GROWTH_MIN_BRIGHTNESS
        {
            grow_shoot(world, pos);
        }
    }

    fn is_valid_bonemeal_target(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        let above = pos.above();
        world.block_state(above).is_air() && world.is_inside_build_height(above.y)
    }

    fn perform_bonemeal(&self, _state: BlockState, world: &mut World, pos: BlockPos) {
        grow_shoot(world, pos);
    }
}

/// `BambooSaplingBlock.growBamboo`：竹笋上面放一节带小叶子的竹子。
fn grow_shoot(world: &mut World, pos: BlockPos) {
    let shoot = BlockState(blocks::BAMBOO).with(BAMBOO_LEAVES, BambooLeaves::Small);
    world.set_block_and_update(pos.above(), shoot);
}

/// `getHeightAboveUpToMax`：上面连着几节竹子，最多数到 16。
fn height_above(world: &World, pos: BlockPos) -> i32 {
    count_stalks(world, pos, Direction::Up)
}

/// `getHeightBelowUpToMax`：下面连着几节竹子，最多数到 16。
fn height_below(world: &World, pos: BlockPos) -> i32 {
    count_stalks(world, pos, Direction::Down)
}

fn count_stalks(world: &World, pos: BlockPos, direction: Direction) -> i32 {
    let mut height = 0;
    while height < MAX_HEIGHT
        && world
            .block_state(pos.relative_by(direction, height + 1))
            .is(blocks::BAMBOO)
    {
        height += 1;
    }
    height
}

/// `BambooStalkBlock.growBamboo`：在 `pos` 上面长一节。`height` 是长之前算上 `pos` 的总高度。
///
/// 下面一节有叶子时新的一节是大叶子，再往下两节的叶子跟着往下挪；够高以后可能停止生长。
fn grow_stalk(state: BlockState, world: &mut World, pos: BlockPos, height: i32) {
    let below_pos = pos.below();
    let below = world.block_state(below_pos);
    let two_below_pos = pos.relative_by(Direction::Down, 2);
    let two_below = world.block_state(two_below_pos);
    let mut leaves = BambooLeaves::None;
    if height >= 1 {
        if !below.is(blocks::BAMBOO) || below.get(BAMBOO_LEAVES) == BambooLeaves::None {
            leaves = BambooLeaves::Small;
        } else {
            leaves = BambooLeaves::Large;
            if two_below.is(blocks::BAMBOO) {
                world.set_block_and_update(below_pos, below.with(BAMBOO_LEAVES, BambooLeaves::Small));
                world.set_block_and_update(
                    two_below_pos,
                    two_below.with(BAMBOO_LEAVES, BambooLeaves::None),
                );
            }
        }
    }
    let age = if state.get(AGE_1) == 1 || two_below.is(blocks::BAMBOO) {
        1
    } else {
        0
    };
    // `nextFloat` 只在够高时取，和官方的短路一样。
    let done = (height >= MATURE_HEIGHT && world.random().next_float() < STOP_CHANCE)
        || height == MAX_HEIGHT - 1;
    let stalk = BlockState(blocks::BAMBOO)
        .with(AGE_1, age)
        .with(BAMBOO_LEAVES, leaves)
        .with(STAGE, i32::from(done));
    world.set_block_and_update(pos.above(), stalk);
}

/// `BambooStalkBlock`。
pub(super) struct BambooStalk;

impl Behaviour for BambooStalk {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let below = world.block_state(pos.below());
        if !world.fluid_state(pos).is_empty() || !below.is_tagged(block_tags::SUPPORTS_BAMBOO) {
            return None;
        }
        let stalk = default_state(block);
        if below.is(blocks::BAMBOO_SAPLING) {
            return Some(stalk.with(AGE_1, 0));
        }
        if below.is(blocks::BAMBOO) {
            return Some(stalk.with(AGE_1, below.get(AGE_1)));
        }
        let above = world.block_state(pos.above());
        Some(if above.is(blocks::BAMBOO) {
            stalk.with(AGE_1, above.get(AGE_1))
        } else {
            BlockState(blocks::BAMBOO_SAPLING)
        })
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        has_bamboo_support(world, pos)
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
        if direction == Direction::Up
            && neighbor.is(blocks::BAMBOO)
            && neighbor.get(AGE_1) > state.get(AGE_1)
        {
            return state.cycle(AGE_1);
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        break_if_unsupported(state, world, pos);
    }

    /// 还在长的竹子三分之一的机会，上面空着又够亮时往上长一节，最多 16 节。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(STAGE) != 0 {
            return;
        }
        let above = pos.above();
        if world.random().next_int(3) == 0
            && world.is_empty_block(above)
            && world.raw_brightness(above) >= GROWTH_MIN_BRIGHTNESS
        {
            let height = height_below(world, pos) + 1;
            if height < MAX_HEIGHT {
                grow_stalk(state, world, pos, height);
            }
        }
    }

    fn is_valid_bonemeal_target(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        let above = height_above(world, pos);
        let below = height_below(world, pos);
        let growth = pos.relative_by(Direction::Up, above + 1);
        above + below + 1 < MAX_HEIGHT
            && world
                .block_state(pos.relative_by(Direction::Up, above))
                .get(STAGE)
                != 1
            && world.is_inside_build_height(growth.y)
            && world.is_empty_block(growth)
    }

    /// 从最上面一节往上长一到两节，长不了时停下。
    fn perform_bonemeal(&self, _state: BlockState, world: &mut World, pos: BlockPos) {
        let mut above = height_above(world, pos);
        let mut height = above + height_below(world, pos) + 1;
        let count = 1 + world.random().next_int(2);
        for _ in 0..count {
            let top_pos = pos.relative_by(Direction::Up, above);
            let top = world.block_state(top_pos);
            let growth = top_pos.above();
            if height >= MAX_HEIGHT
                || top.get(STAGE) == 1
                || !world.is_empty_block(growth)
                || !world.is_inside_build_height(growth.y)
            {
                return;
            }
            grow_stalk(top, world, top_pos, height);
            above += 1;
            height += 1;
        }
    }
}
