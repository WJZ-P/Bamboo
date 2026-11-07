//! `VineBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::Block;

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, multiface_can_attach_to, pipe_property},
    },
    context::PlaceContext,
    state::{BlockState, UP},
    world::World,
};

/// `PROPERTY_BY_DIRECTION` 的方向：藤蔓没有朝下的面。
const FACES: [Direction; 5] = [
    Direction::Up,
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// `VineBlock`。
pub(super) struct Vine;

/// `countFaces`。
fn count_faces(state: BlockState) -> usize {
    FACES
        .into_iter()
        .filter(|&face| state.get(pipe_property(face)))
        .count()
}

/// `isAcceptableNeighbour`。
fn is_acceptable_neighbour(world: &World, neighbor_pos: BlockPos, direction: Direction) -> bool {
    multiface_can_attach_to(
        world,
        direction,
        neighbor_pos,
        world.block_state(neighbor_pos),
    )
}

/// `canSupportAtFace`：这一面贴着方块，或者上面的藤蔓在同一面垂下来。`vine` 是任意一个藤蔓状态。
fn can_support_at_face(vine: BlockState, world: &World, pos: BlockPos, face: Direction) -> bool {
    if face == Direction::Down {
        return false;
    }
    if is_acceptable_neighbour(world, pos.relative(face), face) {
        return true;
    }
    let above = world.block_state(pos.above());
    face.axis().is_horizontal() && above.is_same_block(vine) && above.get(pipe_property(face))
}

/// `getUpdatedState`：去掉已经贴不住的面。
fn updated_state(state: BlockState, world: &World, pos: BlockPos) -> BlockState {
    let mut updated = state;
    if updated.get(UP) {
        // 官方传的方向是朝下，看的是上面一格的顶面，这里保持一样。
        let attached = is_acceptable_neighbour(world, pos.above(), Direction::Down);
        updated = updated.with(UP, attached);
    }
    for face in Direction::HORIZONTAL {
        let property = pipe_property(face);
        if updated.get(property) {
            let attached = can_support_at_face(state, world, pos, face);
            updated = updated.with(property, attached);
        }
    }
    updated
}

impl Behaviour for Vine {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let default = default_state(block);
        let clicked = world.block_state(pos);
        let clicked_vine = clicked.is_same_block(default);
        let base = if clicked_vine { clicked } else { default };
        let free_face = ctx.nearest_looking_directions().into_iter().find(|&face| {
            face != Direction::Down
                && !(clicked_vine && clicked.get(pipe_property(face)))
                && can_support_at_face(default, world, pos, face)
        });
        match free_face {
            Some(face) => Some(base.with(pipe_property(face), true)),
            None => clicked_vine.then_some(base),
        }
    }

    fn can_be_replaced(&self, state: BlockState, ctx: &PlaceContext, world: &World) -> bool {
        let clicked = world.block_state(ctx.clicked_pos());
        if clicked.is_same_block(state) {
            count_faces(clicked) < FACES.len()
        } else {
            Plain.can_be_replaced(state, ctx, world)
        }
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        count_faces(updated_state(state, world, pos)) > 0
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
        if direction == Direction::Down {
            return Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor);
        }
        let updated = updated_state(state, world, pos);
        if count_faces(updated) > 0 {
            updated
        } else {
            BlockState::AIR
        }
    }
}
