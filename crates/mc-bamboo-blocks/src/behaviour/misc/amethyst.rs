//! `AmethystClusterBlock`：紫晶芽和紫晶簇。`BuddingAmethystBlock`：紫晶母岩。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks, registries::fluid};

/// 母岩每次随机刻五分之一的机会长。
const GROWTH_CHANCE: i32 = 5;
/// 满格流体的 `getAmount`。
const FULL_WATER: i32 = 8;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    state::{BlockState, FACING, SupportType, WATERLOGGED},
    world::World,
};

/// `AmethystClusterBlock`。
pub(super) struct AmethystCluster;

impl Behaviour for AmethystCluster {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let waterlogged = is_water_source(world, ctx.clicked_pos());
        Some(
            default_state(block)
                .with(WATERLOGGED, waterlogged)
                .with(FACING, ctx.clicked_face()),
        )
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        let facing = state.get(FACING);
        world.is_face_sturdy(pos.relative(facing.opposite()), facing, SupportType::Full)
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
        tick_water_if_waterlogged(state, world, pos);
        if direction == state.get(FACING).opposite() && !state.can_survive(world, pos) {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }
}

/// `BuddingAmethystBlock`：在随机一面长出紫晶芽，再一级级长成紫晶簇。
pub(super) struct BuddingAmethyst;

/// 紫晶芽的四个阶段，后一个是前一个长大的样子。
const STAGES: [u32; 4] = [
    blocks::SMALL_AMETHYST_BUD,
    blocks::MEDIUM_AMETHYST_BUD,
    blocks::LARGE_AMETHYST_BUD,
    blocks::AMETHYST_CLUSTER,
];

/// `canClusterGrowAtState`：空气或者满格的水。
fn can_cluster_grow_at(state: BlockState) -> bool {
    state.is_air() || state.is(blocks::WATER) && state.fluid().amount() == FULL_WATER
}

/// 朝着 `direction` 的那一格接下来长成什么，不能长时是 `None`。
fn next_stage(existing: BlockState, direction: Direction) -> Option<u32> {
    if can_cluster_grow_at(existing) {
        return Some(STAGES[0]);
    }
    let index = STAGES.iter().position(|&stage| existing.is(stage))?;
    let next = STAGES.get(index + 1)?;
    (existing.get(FACING) == direction).then_some(*next)
}

impl Behaviour for BuddingAmethyst {
    fn random_tick(&self, _state: BlockState, world: &mut World, pos: BlockPos) {
        if world.random().next_int(GROWTH_CHANCE) != 0 {
            return;
        }
        let direction = world.random().direction();
        let grow_pos = pos.relative(direction);
        let existing = world.block_state(grow_pos);
        if let Some(stage) = next_stage(existing, direction) {
            let grown = BlockState(stage)
                .with(FACING, direction)
                .with(WATERLOGGED, existing.fluid().is(fluid::WATER));
            world.set_block_and_update(grow_pos, grown);
        }
    }
}
