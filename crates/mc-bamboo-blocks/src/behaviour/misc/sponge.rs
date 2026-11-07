//! `SpongeBlock`。`WetSpongeBlock` 只在水会蒸发的维度有行为，这里只有主世界。

use std::collections::{HashSet, VecDeque};

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor, blocks};

use crate::{
    behaviour::Behaviour,
    state::{BlockState, LEVEL, WATERLOGGED},
    world::{UPDATE_ALL_IMMEDIATE, UPDATE_CLIENTS, UPDATE_LIMIT, World},
};

/// `SpongeBlock`。
pub(super) struct Sponge;

/// `MAX_DEPTH`。
const MAX_DEPTH: i32 = 6;
/// `MAX_COUNT` 加上海绵自己。
const MAX_VISITED: i32 = 65;

/// `SimpleWaterloggedBlock.pickupBlock`。
fn drain_waterlogged(state: BlockState, world: &mut World, pos: BlockPos) -> bool {
    if !state.get(WATERLOGGED) {
        return false;
    }
    world.set_block_and_update(pos, state.with(WATERLOGGED, false));
    if !state.can_survive(world, pos) {
        world.destroy_block(pos, UPDATE_LIMIT);
    }
    true
}

/// `BucketPickup.pickupBlock`，`user` 是空的。屏障只让创造模式的玩家舀，海绵不算。
fn pick_up_water(state: BlockState, world: &mut World, pos: BlockPos) -> bool {
    if state.is(blocks::BARRIER) {
        return false;
    }
    if state.is_a(BlockAncestor::SimpleWaterloggedBlock) {
        return drain_waterlogged(state, world, pos);
    }
    let picked_up = state.is(blocks::BUBBLE_COLUMN)
        || state.is_a(BlockAncestor::LiquidBlock) && state.get(LEVEL) == 0;
    if picked_up {
        world.set_block(pos, BlockState::AIR, UPDATE_ALL_IMMEDIATE);
    }
    picked_up
}

/// `removeWaterBreadthFirstSearch` 里处理一格的部分：这一格的水没了就继续往外找。
fn absorb(world: &mut World, pos: BlockPos) -> bool {
    let state = world.block_state(pos);
    if !state.fluid().is_water() {
        return false;
    }
    if state.is_a(BlockAncestor::BucketPickup) && pick_up_water(state, world, pos) {
        return true;
    }
    let removable = state.is_a(BlockAncestor::LiquidBlock)
        || state.is(blocks::KELP)
        || state.is(blocks::KELP_PLANT)
        || state.is(blocks::SEAGRASS)
        || state.is(blocks::TALL_SEAGRASS);
    if removable {
        world.set_block_and_update(pos, BlockState::AIR);
    }
    removable
}

/// `removeWaterBreadthFirstSearch`：`BlockPos.breadthFirstTraversal`，吸到水时返回真。
fn remove_water(world: &mut World, start: BlockPos) -> bool {
    let mut nodes = VecDeque::from([(start, 0)]);
    let mut visited = HashSet::new();
    let mut count = 0;
    while let Some((pos, depth)) = nodes.pop_front() {
        if !visited.insert(pos) || pos != start && !absorb(world, pos) {
            continue;
        }
        count += 1;
        if count >= MAX_VISITED {
            break;
        }
        if depth < MAX_DEPTH {
            nodes.extend(Direction::ALL.map(|side| (pos.relative(side), depth + 1)));
        }
    }
    count > 1
}

/// `tryAbsorbWater`。
fn try_absorb_water(world: &mut World, pos: BlockPos) {
    if remove_water(world, pos) {
        world.set_block(pos, BlockState(blocks::WET_SPONGE), UPDATE_CLIENTS);
    }
}

impl Behaviour for Sponge {
    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) {
            try_absorb_water(world, pos);
        }
    }

    fn neighbor_changed(
        &self,
        _state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        try_absorb_water(world, pos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, behaviour::misc::test_support::set, testing::TestLevel};

    const CENTER: BlockPos = BlockPos::new(0, 64, 0);

    /// 放下的海绵会被第一格水的邻居更新再触发一次，一次放置不止吸 64 格。
    /// 这里不放海绵，只看一轮搜索的上限，随机的场景凑不出这么多水。
    #[test]
    fn one_search_stops_after_sixty_four_blocks() {
        const RADIUS: i32 = 8;
        let mut level = TestLevel::new();
        for x in -RADIUS..=RADIUS {
            for y in -RADIUS..=RADIUS {
                for z in -RADIUS..=RADIUS {
                    level.set(CENTER.offset(x, y, z), BlockState::WATER);
                }
            }
        }
        level.set(CENTER, BlockState::AIR);
        let mut ticks = BlockTicks::new();
        assert!(remove_water(
            &mut World::new(&mut level, &mut ticks),
            CENTER
        ));
        let drained = level.writes.iter().filter(|(_, state)| state.is_air());
        assert_eq!(drained.count(), 64);
    }

    #[test]
    fn a_sponge_reaches_six_blocks_along_a_line() {
        let mut level = TestLevel::new();
        for x in 1..=9 {
            level.set(CENTER.offset(x, 0, 0), BlockState::WATER);
        }
        set(&mut level, CENTER, BlockState(blocks::SPONGE));
        assert!(level.get(CENTER.offset(6, 0, 0)).is_air());
        assert_eq!(level.get(CENTER.offset(7, 0, 0)), BlockState::WATER);
    }

    #[test]
    fn a_dry_sponge_stays_dry() {
        let mut level = TestLevel::new();
        set(&mut level, CENTER, BlockState(blocks::SPONGE));
        assert!(level.get(CENTER).is(blocks::SPONGE));
    }
}
