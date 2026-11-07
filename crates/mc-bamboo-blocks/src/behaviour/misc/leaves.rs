//! `LeavesBlock` 和它的子类 `TintedParticleLeavesBlock`、`UntintedParticleLeavesBlock`、
//! `MangroveLeavesBlock`。子类只多了粒子和骨粉。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor, tags::block_tags};

use crate::{
    behaviour::{
        Behaviour,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    state::{BlockState, DISTANCE, PERSISTENT, WATERLOGGED},
    world::World,
};

/// `LeavesBlock`。
pub(super) struct Leaves;

/// `DECAY_DISTANCE`。
const DECAY_DISTANCE: i32 = 7;
/// `TICK_DELAY`。
const TICK_DELAY: i32 = 1;

/// `getDistanceAt`。
///
/// 官方看方块有没有 `DISTANCE` 属性。脚手架也有叫 `distance` 的属性，但那是另一个属性，
/// 这里的属性按名字查，所以要看类。
fn distance_at(state: BlockState) -> i32 {
    if state.is_tagged(block_tags::PREVENTS_NEARBY_LEAF_DECAY) {
        0
    } else if state.is_a(BlockAncestor::LeavesBlock) {
        state.get(DISTANCE)
    } else {
        DECAY_DISTANCE
    }
}

/// `updateDistance`：比最近的邻居多一格，最多是 7。
fn update_distance(state: BlockState, world: &World, pos: BlockPos) -> BlockState {
    let distance = Direction::ALL
        .into_iter()
        .map(|side| distance_at(world.block_state(pos.relative(side))) + 1)
        .fold(DECAY_DISTANCE, i32::min);
    state.with(DISTANCE, distance)
}

impl Behaviour for Leaves {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let state = default_state(block)
            .with(PERSISTENT, true)
            .with(WATERLOGGED, is_water_source(world, pos));
        Some(update_distance(state, world, pos))
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        tick_water_if_waterlogged(state, world, pos);
        let from_neighbor = distance_at(neighbor) + 1;
        if from_neighbor != 1 || state.get(DISTANCE) != from_neighbor {
            world.schedule_tick(pos, state.block(), TICK_DELAY);
        }
        state
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let updated = update_distance(state, world, pos);
        world.set_block_and_update(pos, updated);
    }
}
