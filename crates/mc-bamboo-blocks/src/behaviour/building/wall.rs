//! `WallBlock`。

use mc_bamboo_core::{Aabb, BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor, tags::block_tags};

use super::{fence_gate, has_full_face};
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, is_exception_for_connection, is_water_source, tick_water_if_waterlogged,
        },
    },
    context::PlaceContext,
    shape::{self, BooleanOp, CollisionContext},
    state::{
        BlockState, EAST_WALL, NORTH_WALL, Property, SOUTH_WALL, UP, WATERLOGGED, WEST_WALL,
        WallSide,
    },
    world::World,
};

/// `TEST_SHAPE_POST`：上面的方块盖住这一块时，柱子可以不升高。
const TEST_POST: &[Aabb] = &[sixteenths(7.0, 0.0, 7.0, 9.0, 16.0, 9.0)];
/// `TEST_SHAPES_WALL`：上面的方块盖住这一块时，这一侧的墙升到顶。
const TEST_NORTH: &[Aabb] = &[sixteenths(7.0, 0.0, 0.0, 9.0, 16.0, 9.0)];
const TEST_EAST: &[Aabb] = &[sixteenths(7.0, 0.0, 7.0, 16.0, 16.0, 9.0)];
const TEST_SOUTH: &[Aabb] = &[sixteenths(7.0, 0.0, 7.0, 9.0, 16.0, 16.0)];
const TEST_WEST: &[Aabb] = &[sixteenths(0.0, 0.0, 7.0, 9.0, 16.0, 9.0)];

/// `Block.box`：参数以十六分之一格为单位。
const fn sixteenths(
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
) -> Aabb {
    Aabb::new(
        min_x / 16.0,
        min_y / 16.0,
        min_z / 16.0,
        max_x / 16.0,
        max_y / 16.0,
        max_z / 16.0,
    )
}

const fn test_shape(side: Direction) -> &'static [Aabb] {
    match side {
        Direction::North => TEST_NORTH,
        Direction::East => TEST_EAST,
        Direction::South => TEST_SOUTH,
        Direction::West => TEST_WEST,
        Direction::Up | Direction::Down => panic!("walls only have horizontal sides"),
    }
}

/// `PROPERTY_BY_DIRECTION`。
const fn wall_property(side: Direction) -> Property<WallSide> {
    match side {
        Direction::North => NORTH_WALL,
        Direction::East => EAST_WALL,
        Direction::South => SOUTH_WALL,
        Direction::West => WEST_WALL,
        Direction::Up | Direction::Down => panic!("walls only have horizontal sides"),
    }
}

/// `WallBlock`。
pub(super) struct Wall;

/// `connectsTo`。`face` 是 `other` 朝着墙的面。
fn connects_to(other: BlockState, face: Direction) -> bool {
    let gate =
        other.is_a(BlockAncestor::FenceGateBlock) && fence_gate::connects_to_direction(other, face);
    other.is_tagged(block_tags::WALLS)
        || !is_exception_for_connection(other) && has_full_face(other, face)
        || other.is_a(BlockAncestor::IronBarsBlock)
        || gate
}

/// `isCovered`。
fn is_covered(above_shape: &[Aabb], test_shape: &[Aabb]) -> bool {
    !shape::join_is_not_empty(test_shape, above_shape, BooleanOp::OnlyFirst)
}

/// `isConnected`，每一侧按 [`Direction::HORIZONTAL`] 的顺序。
fn connections_of(state: BlockState) -> [bool; 4] {
    Direction::HORIZONTAL.map(|side| state.get(wall_property(side)) != WallSide::None)
}

/// `makeWallState`。
fn wall_side(connected: bool, above_shape: &[Aabb], side: Direction) -> WallSide {
    if !connected {
        WallSide::None
    } else if is_covered(above_shape, test_shape(side)) {
        WallSide::Tall
    } else {
        WallSide::Low
    }
}

/// `shouldRaisePost`。
fn should_raise_post(state: BlockState, top: BlockState, above_shape: &[Aabb]) -> bool {
    if top.is_a(BlockAncestor::WallBlock) && top.get(UP) {
        return true;
    }
    let [north, east, south, west] =
        Direction::HORIZONTAL.map(|side| state.get(wall_property(side)));
    let none = WallSide::None;
    let all_none = north == none && south == none && west == none && east == none;
    if all_none || (north == none) != (south == none) || (west == none) != (east == none) {
        return true;
    }
    let tall = WallSide::Tall;
    if north == tall && south == tall || east == tall && west == tall {
        return false;
    }
    top.is_tagged(block_tags::WALL_POST_OVERRIDE) || is_covered(above_shape, TEST_POST)
}

/// 私有的 `updateShape(level, state, topPos, topNeighbour, north, east, south, west)`。
fn with_connections(
    world: &World,
    state: BlockState,
    top_pos: BlockPos,
    top: BlockState,
    connections: [bool; 4],
) -> BlockState {
    let collision = top.collision_boxes(world, top_pos, CollisionContext::Empty);
    let above_shape = shape::face_shape(&collision, Direction::Down);
    let sides = Direction::HORIZONTAL.into_iter().zip(connections).fold(
        state,
        |state, (side, connected)| {
            state.with(
                wall_property(side),
                wall_side(connected, &above_shape, side),
            )
        },
    );
    sides.with(UP, should_raise_post(sides, top, &above_shape))
}

/// `topUpdate`。
fn top_update(world: &World, state: BlockState, top_pos: BlockPos, top: BlockState) -> BlockState {
    with_connections(world, state, top_pos, top, connections_of(state))
}

/// `sideUpdate`。
fn side_update(
    world: &World,
    pos: BlockPos,
    state: BlockState,
    neighbor: BlockState,
    direction: Direction,
) -> BlockState {
    let mut connections = connections_of(state);
    for (connection, side) in connections.iter_mut().zip(Direction::HORIZONTAL) {
        if side == direction {
            *connection = connects_to(neighbor, direction.opposite());
        }
    }
    let above = pos.above();
    with_connections(world, state, above, world.block_state(above), connections)
}

impl Behaviour for Wall {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let above = pos.above();
        let connections = Direction::HORIZONTAL
            .map(|side| connects_to(world.block_state(pos.relative(side)), side.opposite()));
        let state = default_state(block).with(WATERLOGGED, is_water_source(world, pos));
        Some(with_connections(
            world,
            state,
            above,
            world.block_state(above),
            connections,
        ))
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
        match direction {
            Direction::Down => {
                Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
            }
            Direction::Up => top_update(world, state, neighbor_pos, neighbor),
            side => side_update(world, pos, state, neighbor, side),
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        behaviour::building::test_support::{FLOOR, level_with_floor, top_of},
        testing::{actor_holding, use_item},
    };

    fn state_at(level: &crate::testing::TestLevel, pos: BlockPos, side: Direction) -> WallSide {
        level.get(pos).get(wall_property(side))
    }

    #[test]
    fn a_wall_under_a_full_block_goes_up_to_the_top() {
        let mut level = level_with_floor();
        let wall = FLOOR.above();
        level.set(wall.west(), BlockState(blocks::STONE));
        use_item(
            &mut level,
            &actor_holding(items::COBBLESTONE_WALL),
            top_of(FLOOR),
        );
        assert_eq!(state_at(&level, wall, Direction::West), WallSide::Low);
        assert!(level.get(wall).get(UP));

        use_item(&mut level, &actor_holding(items::STONE), top_of(wall));
        assert_eq!(state_at(&level, wall, Direction::West), WallSide::Tall);
    }

    #[test]
    fn full_faces_on_the_exception_list_do_not_connect() {
        let mut level = level_with_floor();
        let wall = FLOOR.above();
        level.set(wall.north(), BlockState(blocks::BIRCH_LEAVES));
        level.set(wall.south(), BlockState(blocks::STONE));
        use_item(
            &mut level,
            &actor_holding(items::COBBLESTONE_WALL),
            top_of(FLOOR),
        );
        assert_eq!(state_at(&level, wall, Direction::North), WallSide::None);
        assert_eq!(state_at(&level, wall, Direction::South), WallSide::Low);
    }
}
