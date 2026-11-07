//! `ChorusFlowerBlock`、`ChorusPlantBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks, tags::block_tags};

use super::{break_if_unsupported, schedule_break_if_unsupported};
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, field_block, pipe_property},
    },
    context::PlaceContext,
    state::BlockState,
    world::World,
};

/// `ChorusFlowerBlock`。
pub(super) struct ChorusFlower;

/// 下面是空气时，这朵花要贴着且只贴着一根紫颂植株，其余方向都要是空气。
fn hangs_from_one_plant(plant: BlockState, world: &World, pos: BlockPos) -> bool {
    let mut plants = 0;
    for side in Direction::HORIZONTAL {
        let neighbor = world.block_state(pos.relative(side));
        if neighbor.is_same_block(plant) {
            plants += 1;
        } else if !neighbor.is_air() {
            return false;
        }
    }
    plants == 1
}

impl Behaviour for ChorusFlower {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let plant = default_state(field_block(state.block(), "plant"));
        let below = world.block_state(pos.below());
        below.is_same_block(plant)
            || below.is_tagged(block_tags::SUPPORTS_CHORUS_FLOWER)
            || below.is_air() && hangs_from_one_plant(plant, world, pos)
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
        if direction != Direction::Up {
            schedule_break_if_unsupported(state, world, pos);
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        break_if_unsupported(state, world, pos);
    }
}

/// `ChorusPlantBlock`。
pub(super) struct ChorusPlant;

/// 这一侧的邻居和紫颂植株连在一起。只有下面可以是支撑方块。
fn connects_to(plant: BlockState, neighbor: BlockState, side: Direction) -> bool {
    neighbor.is_same_block(plant)
        || neighbor.is(blocks::CHORUS_FLOWER)
        || side == Direction::Down && neighbor.is_tagged(block_tags::SUPPORTS_CHORUS_PLANT)
}

/// `below` 能托住紫颂植株。
fn holds_plant(plant: BlockState, below: BlockState) -> bool {
    below.is_same_block(plant) || below.is_tagged(block_tags::SUPPORTS_CHORUS_PLANT)
}

impl Behaviour for ChorusPlant {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let default = default_state(block);
        let state = Direction::ALL.into_iter().fold(default, |state, side| {
            let neighbor = world.block_state(pos.relative(side));
            state.with(pipe_property(side), connects_to(default, neighbor, side))
        });
        Some(state)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let below = world.block_state(pos.below());
        let above_and_below = !world.block_state(pos.above()).is_air() && !below.is_air();
        for side in Direction::HORIZONTAL {
            let neighbor_pos = pos.relative(side);
            if world.block_state(neighbor_pos).is_same_block(state) {
                if above_and_below {
                    return false;
                }
                if holds_plant(state, world.block_state(neighbor_pos.below())) {
                    return true;
                }
            }
        }
        holds_plant(state, below)
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
            world.schedule_tick(pos, state.block(), 1);
            return Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor);
        }
        state.with(
            pipe_property(direction),
            connects_to(state, neighbor, direction),
        )
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        break_if_unsupported(state, world, pos);
    }
}
