//! `LiquidBlock`、`BubbleColumnBlock`。
//!
//! 流体不流动，流体的计划刻什么也不做。气泡柱的计划刻只用来改柱子，也不排。
//! 这里真正改世界的只有岩浆碰到水。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks, tags::block_tags};

use crate::{
    behaviour::Behaviour,
    state::BlockState,
    world::{Fluid, World},
};

/// `LiquidBlock`。
pub(super) struct Liquid;

/// `POSSIBLE_FLOW_DIRECTIONS` 的反方向，也就是岩浆看的邻居。
const LAVA_NEIGHBORS: [Direction; 5] = [
    Direction::Up,
    Direction::North,
    Direction::South,
    Direction::West,
    Direction::East,
];

/// 这种液体的流体。
fn fluid_of(state: BlockState) -> Fluid {
    if state.is(blocks::LAVA) {
        Fluid::Lava
    } else {
        Fluid::Water
    }
}

/// 岩浆源头碰到水变黑曜石，流动的岩浆变圆石。
fn hardened_lava(world: &World, pos: BlockPos) -> BlockState {
    if world.fluid_state(pos).is_source() {
        BlockState(blocks::OBSIDIAN)
    } else {
        BlockState(blocks::COBBLESTONE)
    }
}

/// `shouldSpreadLiquid`：岩浆碰到水或者灵魂土上的蓝冰时凝固，这时不用流了。
fn should_spread(state: BlockState, world: &mut World, pos: BlockPos) -> bool {
    if !state.is(blocks::LAVA) {
        return true;
    }
    let over_soul_soil = world.block_state(pos.below()).is(blocks::SOUL_SOIL);
    for side in LAVA_NEIGHBORS {
        let neighbor = pos.relative(side);
        let hardened = if world.is_water_at(neighbor) {
            hardened_lava(world, pos)
        } else if over_soul_soil && world.block_state(neighbor).is(blocks::BLUE_ICE) {
            BlockState(blocks::BASALT)
        } else {
            continue;
        };
        world.set_block_and_update(pos, hardened);
        return false;
    }
    true
}

/// `onPlace` 和 `neighborChanged` 相同的部分。
fn check_spread(state: BlockState, world: &mut World, pos: BlockPos) {
    if should_spread(state, world, pos) {
        world.schedule_fluid_tick(pos, fluid_of(state));
    }
}

impl Behaviour for Liquid {
    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        if state.fluid().is_source() || neighbor.fluid().is_source() {
            world.schedule_fluid_tick(pos, fluid_of(state));
        }
        state
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        check_spread(state, world, pos);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        check_spread(state, world, pos);
    }
}

/// `BubbleColumnBlock`。
pub(super) struct BubbleColumn;

impl Behaviour for BubbleColumn {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = world.block_state(pos.below());
        below.is_same_block(state)
            || below.is_tagged(block_tags::ENABLES_BUBBLE_COLUMN_PUSH_UP)
            || below.is_tagged(block_tags::ENABLES_BUBBLE_COLUMN_DRAG_DOWN)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        world.schedule_fluid_tick(pos, Fluid::Water);
        state
    }
}
