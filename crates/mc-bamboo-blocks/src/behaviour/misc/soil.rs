//! `FarmlandBlock`、`PathBlock`：上面压着东西时变回泥土。耕地还会随着附近有没有水变湿变干。
//! `RootedDirtBlock` 只有骨粉。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor, blocks, tags::block_tags};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, field_block},
    },
    context::PlaceContext,
    state::{BlockState, MOISTURE},
    world::{UPDATE_CLIENTS, World},
};

/// 耕地最湿是 7。
const MAX_MOISTURE: i32 = 7;
/// 耕地找水的水平范围。
const WATER_SEARCH_RADIUS: i32 = 4;

/// `FarmlandBlock`。
pub(super) struct Farmland;

/// `PathBlock`。
pub(super) struct DirtPath;

/// 两个类的 `updateShape`：上面被压住时下一刻再变回去。
fn update_shape_under_pressure(
    state: BlockState,
    world: &mut World,
    pos: BlockPos,
    direction: Direction,
    neighbor_pos: BlockPos,
    neighbor: BlockState,
) -> BlockState {
    if direction == Direction::Up && !state.can_survive(world, pos) {
        world.schedule_tick(pos, state.block(), 1);
    }
    Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
}

/// 两个类的 `turnToBaseBlock`：这个世界看不到实体，不用把它们往上推。
fn turn_to_base_block(state: BlockState, world: &mut World, pos: BlockPos) {
    let base = default_state(field_block(state.block(), "baseBlock"));
    world.set_block_and_update(pos, base);
}

impl Behaviour for Farmland {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = default_state(block);
        Some(if state.can_survive(world, ctx.clicked_pos()) {
            state
        } else {
            default_state(field_block(block, "baseBlock"))
        })
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        let above = world.block_state(pos.above());
        !above.is_solid() || above.is_tagged(block_tags::MAINTAINS_FARMLAND)
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
        update_shape_under_pressure(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if !state.can_survive(world, pos) {
            turn_to_base_block(state, world, pos);
        }
    }

    /// 附近有水时湿透；没水又不下雨时一点点变干，干透了上面又没有庄稼就变回泥土。
    fn random_tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        let moisture = state.get(MOISTURE);
        if is_near_water(world, pos) || world.is_raining_at(pos.above()) {
            if moisture < MAX_MOISTURE {
                world.set_block(pos, state.with(MOISTURE, MAX_MOISTURE), UPDATE_CLIENTS);
            }
        } else if moisture > 0 {
            world.set_block(pos, state.with(MOISTURE, moisture - 1), UPDATE_CLIENTS);
        } else if !world
            .block_state(pos.above())
            .is_tagged(block_tags::MAINTAINS_FARMLAND)
        {
            turn_to_base_block(state, world, pos);
        }
    }
}

/// `FarmlandBlock.isNearWater`：水平 4 格、同层和上面一层里有水。
fn is_near_water(world: &World, pos: BlockPos) -> bool {
    (-WATER_SEARCH_RADIUS..=WATER_SEARCH_RADIUS).any(|x| {
        (0..=1).any(|y| {
            (-WATER_SEARCH_RADIUS..=WATER_SEARCH_RADIUS)
                .any(|z| world.fluid_state(pos.offset(x, y, z)).is_water())
        })
    })
}

/// `RootedDirtBlock`：骨粉在下面长出垂根。
pub(super) struct RootedDirt;

impl Behaviour for RootedDirt {
    fn is_valid_bonemeal_target(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = pos.below();
        world.block_state(below).is_air() && world.is_inside_build_height(below.y)
    }

    fn perform_bonemeal(&self, _state: BlockState, world: &mut World, pos: BlockPos) {
        world.set_block_and_update(pos.below(), BlockState(blocks::HANGING_ROOTS));
    }
}

impl Behaviour for DirtPath {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = default_state(block);
        Some(if state.can_survive(world, ctx.clicked_pos()) {
            state
        } else {
            BlockState(blocks::DIRT)
        })
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        let above = world.block_state(pos.above());
        !above.is_solid() || above.is_a(BlockAncestor::FenceGateBlock)
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
        update_shape_under_pressure(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        turn_to_base_block(state, world, pos);
    }
}
