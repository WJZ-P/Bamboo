//! `IceBlock` 和它的子类 `FrostedIceBlock`：冰在亮的地方化成水，霜冰一级级化掉。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks};

use crate::{
    behaviour::Behaviour,
    state::{AGE_3, BlockState},
    world::{UPDATE_CLIENTS, World},
};

/// `FrostedIceBlock.MAX_AGE`：到这一级再化就变成水。
const MAX_AGE: i32 = 3;
/// `NEIGHBORS_TO_AGE`：同种邻居少于这个数时一定会化。
const NEIGHBORS_TO_AGE: usize = 4;
/// `NEIGHBORS_TO_MELT`。
const NEIGHBORS_TO_MELT: usize = 2;
/// 冰要比这个亮（减去它自己挡的光）才化。
const MELT_BRIGHTNESS: i32 = 11;
/// 霜冰放下后第一次化的延迟，`Mth.nextInt(random, 60, 120)`。
const FIRST_MELT_DELAY: (i32, i32) = (60, 120);
/// 之后每一级之间的延迟，`Mth.nextInt(random, 20, 40)`。
const MELT_DELAY: (i32, i32) = (20, 40);

/// `IceBlock.melt`，主世界的水不会蒸发。
fn melt(world: &mut World, pos: BlockPos) {
    let water = BlockState::WATER;
    world.set_block_and_update(pos, water);
    world.neighbor_changed(pos, water.block());
}

/// `IceBlock.randomTick`：方块光够亮就化。
fn melt_in_block_light(state: BlockState, world: &mut World, pos: BlockPos) {
    if world.block_light(pos) > MELT_BRIGHTNESS - i32::from(state.light_dampening()) {
        melt(world, pos);
    }
}

/// `IceBlock`。
pub(super) struct Ice;

impl Behaviour for Ice {
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        melt_in_block_light(state, world, pos);
    }
}

/// `FrostedIceBlock`。
pub(super) struct FrostedIce;

/// `fewerNeigboursThan`。
fn has_fewer_neighbours(state: BlockState, world: &World, pos: BlockPos, limit: usize) -> bool {
    Direction::ALL
        .into_iter()
        .filter(|&side| world.block_state(pos.relative(side)).is_same_block(state))
        .count()
        < limit
}

/// `Mth.nextInt(random, min, max)` 刻以后再化一次。
fn schedule_melt(world: &mut World, pos: BlockPos, (min, max): (i32, i32)) {
    let delay = world.random().mth_next_int(min, max);
    world.schedule_tick(pos, BlockState(blocks::FROSTED_ICE).block(), delay);
}

/// `slightlyMelt`：长一级，已经是最后一级时化成水，返回真。
fn slightly_melt(state: BlockState, world: &mut World, pos: BlockPos) -> bool {
    let age = state.get(AGE_3);
    if age < MAX_AGE {
        world.set_block(pos, state.with(AGE_3, age + 1), UPDATE_CLIENTS);
        false
    } else {
        melt(world, pos);
        true
    }
}

/// 自己化掉以后，周围还没到最后一级的霜冰各长一级、过一会儿再化。
fn melt_neighbours(state: BlockState, world: &mut World, pos: BlockPos) {
    for side in Direction::ALL {
        let neighbor_pos = pos.relative(side);
        let neighbor = world.block_state(neighbor_pos);
        if neighbor.is_same_block(state) && !slightly_melt(neighbor, world, neighbor_pos) {
            schedule_melt(world, neighbor_pos, MELT_DELAY);
        }
    }
}

impl Behaviour for FrostedIce {
    fn on_place(&self, _state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        schedule_melt(world, pos, FIRST_MELT_DELAY);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        source: &'static Block,
    ) {
        if BlockState(source.default_state).is(blocks::FROSTED_ICE)
            && has_fewer_neighbours(state, world, pos, NEIGHBORS_TO_MELT)
        {
            melt(world, pos);
        }
    }

    /// 三分之一的机会，或者同种邻居少的时候，够亮就化一级。主世界看天空和方块光里亮的那个。
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if world.random().next_int(3) == 0 || has_fewer_neighbours(state, world, pos, NEIGHBORS_TO_AGE)
        {
            let threshold =
                MELT_BRIGHTNESS - state.get(AGE_3) - i32::from(state.light_dampening());
            if world.max_local_raw_brightness(pos) > threshold
                && slightly_melt(state, world, pos)
            {
                melt_neighbours(state, world, pos);
                return;
            }
        }
        schedule_melt(world, pos, MELT_DELAY);
    }

    /// 继承 `IceBlock.randomTick`。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        melt_in_block_light(state, world, pos);
    }
}
