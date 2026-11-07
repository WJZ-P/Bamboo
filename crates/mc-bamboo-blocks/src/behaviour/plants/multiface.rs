//! `MultifaceBlock`、`GlowLichenBlock`、`SculkVeinBlock`。后两个只多了蔓延，这里不做。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, registries::fluid};

use crate::{
    behaviour::{
        Behaviour,
        common::{
            default_state, multiface_can_attach_at, multiface_can_attach_to, pipe_property,
            tick_water_if_waterlogged,
        },
    },
    context::PlaceContext,
    state::{BlockState, WATERLOGGED},
    world::World,
};

/// `MultifaceBlock`。
pub(super) struct Multiface;

/// `hasFace`。
fn has_face(state: BlockState, face: Direction) -> bool {
    state.get(pipe_property(face))
}

/// `hasAnyFace`。
fn has_any_face(state: BlockState) -> bool {
    Direction::ALL.into_iter().any(|face| has_face(state, face))
}

/// `removeFace`：最后一面也没了就是空气。
fn remove_face(state: BlockState, face: Direction) -> BlockState {
    let updated = state.with(pipe_property(face), false);
    if has_any_face(updated) {
        updated
    } else {
        BlockState::AIR
    }
}

/// `getStateForPlacement(oldState, level, placementPos, placementDirection)`。
fn state_on_face(
    default: BlockState,
    old: BlockState,
    world: &World,
    pos: BlockPos,
    face: Direction,
) -> Option<BlockState> {
    let same_block = old.is_same_block(default);
    if same_block && has_face(old, face) || !multiface_can_attach_at(world, pos, face) {
        return None;
    }
    let base = if same_block {
        old
    } else {
        default.with(WATERLOGGED, old.fluid().is(fluid::WATER))
    };
    Some(base.with(pipe_property(face), true))
}

impl Behaviour for Multiface {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let old = world.block_state(pos);
        ctx.nearest_looking_directions()
            .into_iter()
            .find_map(|face| state_on_face(default_state(block), old, world, pos, face))
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, _world: &World) -> bool {
        let has_vacant_face = Direction::ALL
            .into_iter()
            .any(|face| !has_face(state, face));
        ctx.item != state.block().item || has_vacant_face
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        has_any_face(state)
            && Direction::ALL
                .into_iter()
                .filter(|&face| has_face(state, face))
                .all(|face| multiface_can_attach_at(world, pos, face))
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
        if !has_any_face(state) {
            return BlockState::AIR;
        }
        if has_face(state, direction)
            && !multiface_can_attach_to(world, direction, neighbor_pos, neighbor)
        {
            remove_face(state, direction)
        } else {
            state
        }
    }
}
