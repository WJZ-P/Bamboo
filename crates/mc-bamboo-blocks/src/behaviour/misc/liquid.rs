//! `LiquidBlock`、`BubbleColumnBlock`。
//!
//! 流动本身在 [`crate::fluid`] 里，这里是方块这一层：什么时候给流体排刻、岩浆碰到水凝固、
//! 用桶舀起来，以及水底下灵魂沙和岩浆块顶起来的气泡柱。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{
    Block, BlockAncestor, blocks, items,
    registries::fluid,
    tags::{block_tags, fluid_tags},
};

use crate::{
    behaviour::Behaviour,
    fluid::Fluid,
    state::{BlockState, DRAG, LEVEL},
    world::{UPDATE_ALL_IMMEDIATE, UPDATE_CLIENTS, World},
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
/// `LiquidBlock.BUBBLE_COLUMN_CHECK_DELAY`。
const BUBBLE_COLUMN_CHECK_DELAY: i32 = 20;
/// 气泡柱的邻居变了以后过这么久重新长一遍。
const BUBBLE_COLUMN_UPDATE_DELAY: i32 = 5;

/// 字段 `fluid`：这种液体方块的流体。
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
/// 官方凝固时还有一个嘶嘶声的 `levelEvent`，这个世界没有音效。
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

/// `shouldBubbleColumnOccupy`：满格的水源才能长气泡柱。
fn should_bubble_column_occupy(state: BlockState) -> bool {
    let liquid = state.fluid();
    liquid.is_tagged(fluid_tags::BUBBLE_COLUMN_CAN_OCCUPY)
        && liquid.is_source()
        && liquid.amount() == 8
}

/// 下面是灵魂沙或者岩浆块，能顶起或者拉下气泡柱。
fn makes_bubble_column(below: BlockState) -> bool {
    below.is_tagged(block_tags::ENABLES_BUBBLE_COLUMN_DRAG_DOWN)
        || below.is_tagged(block_tags::ENABLES_BUBBLE_COLUMN_PUSH_UP)
}

/// `tryScheduleBubbleBlockColumn`。
fn try_schedule_bubble_column(
    state: BlockState,
    world: &mut World,
    pos: BlockPos,
    below: BlockState,
) {
    if makes_bubble_column(below) {
        world.schedule_tick(pos, state.block(), BUBBLE_COLUMN_CHECK_DELAY);
    }
}

/// `onPlace` 和 `neighborChanged` 相同的部分。
fn check_spread(state: BlockState, world: &mut World, pos: BlockPos) {
    if should_spread(state, world, pos) {
        world.schedule_fluid_tick(pos, state.fluid().fluid());
    }
    if should_bubble_column_occupy(state) {
        let below = world.block_state(pos.below());
        try_schedule_bubble_column(state, world, pos, below);
    }
}

impl Behaviour for Liquid {
    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        if state.fluid().is_source() || neighbor.fluid().is_source() {
            world.schedule_fluid_tick(pos, state.fluid().fluid());
        }
        if direction == Direction::Down && should_bubble_column_occupy(state) {
            try_schedule_bubble_column(state, world, pos, neighbor);
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

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if should_bubble_column_occupy(state) {
            let occupied = world.block_state(pos);
            let below = world.block_state(pos.below());
            update_column(world, pos, occupied, below);
        }
    }

    /// 只有源头舀得起来。
    fn pickup_block(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _by_player: bool,
    ) -> i32 {
        if state.get(LEVEL) != 0 {
            return items::AIR;
        }
        world.set_block(pos, BlockState::AIR, UPDATE_ALL_IMMEDIATE);
        fluid_of(state).bucket()
    }
}

/// `BubbleColumnBlock`。
pub(super) struct BubbleColumn;

/// `canOccupy`：气泡柱能长进这一格。
fn can_occupy(state: BlockState) -> bool {
    if state.is(blocks::BUBBLE_COLUMN) {
        return true;
    }
    let liquid = state.fluid();
    liquid.is_tagged(fluid_tags::BUBBLE_COLUMN_CAN_OCCUPY)
        && state.is_a(BlockAncestor::LiquidBlock)
        && liquid.is_source()
        && liquid.amount() >= 8
}

/// `getColumnState`：下面的方块决定气泡柱往上还是往下，没有灵魂沙或者岩浆块时变回水。
fn column_state(below: BlockState, occupied: BlockState) -> BlockState {
    let column = BlockState(blocks::BUBBLE_COLUMN);
    if below.is(blocks::BUBBLE_COLUMN) {
        below
    } else if below.is_tagged(block_tags::ENABLES_BUBBLE_COLUMN_PUSH_UP) {
        column.with(DRAG, false)
    } else if below.is_tagged(block_tags::ENABLES_BUBBLE_COLUMN_DRAG_DOWN) {
        column.with(DRAG, true)
    } else if occupied.is(blocks::BUBBLE_COLUMN) {
        BlockState::WATER
    } else {
        occupied
    }
}

/// `BubbleColumnBlock.updateColumn`：从 `occupy_at` 往上，把能占的格子都换成同一种气泡柱。
fn update_column(world: &mut World, occupy_at: BlockPos, occupied: BlockState, below: BlockState) {
    if !can_occupy(occupied) {
        return;
    }
    let column = column_state(below, occupied);
    world.set_block(occupy_at, column, UPDATE_CLIENTS);
    let mut pos = occupy_at.above();
    while can_occupy(world.block_state(pos)) {
        if !world.set_block(pos, column, UPDATE_CLIENTS) {
            return;
        }
        pos = pos.above();
    }
}

impl Behaviour for BubbleColumn {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = world.block_state(pos.below());
        below.is_same_block(state) || makes_bubble_column(below)
    }

    fn update_shape(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor: BlockState,
    ) -> BlockState {
        world.schedule_fluid_tick(pos, fluid::WATER);
        let grows_up =
            direction == Direction::Up && !neighbor.is_same_block(state) && can_occupy(neighbor);
        if !state.can_survive(world, pos) || direction == Direction::Down || grows_up {
            world.schedule_tick(pos, state.block(), BUBBLE_COLUMN_UPDATE_DELAY);
        }
        state
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let below = world.block_state(pos.below());
        update_column(world, pos, state, below);
    }

    fn pickup_block(
        &self,
        _state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _by_player: bool,
    ) -> i32 {
        world.set_block(pos, BlockState::AIR, UPDATE_ALL_IMMEDIATE);
        items::WATER_BUCKET
    }
}
