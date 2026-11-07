//! `StairBlock`、`WeatheringCopperStairBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor};

use super::clicked_upper_half;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, is_water_source, tick_water_if_waterlogged},
    },
    context::PlaceContext,
    state::{BlockState, HALF, HORIZONTAL_FACING, Half, STAIRS_SHAPE, StairsShape, WATERLOGGED},
    world::World,
};

/// `StairBlock`。
pub(super) struct Stair;

/// `isStairs`。
fn is_stairs(state: BlockState) -> bool {
    state.is_a(BlockAncestor::StairBlock)
}

/// `other` 是和 `state` 同一半的楼梯，朝向和 `state` 垂直。
fn perpendicular_stairs(state: BlockState, other: BlockState) -> Option<Direction> {
    if !is_stairs(other) || state.get(HALF) != other.get(HALF) {
        return None;
    }
    let facing = other.get(HORIZONTAL_FACING);
    (facing.axis() != state.get(HORIZONTAL_FACING).axis()).then_some(facing)
}

/// `canTakeShape`：`neighbour` 方向上有一模一样朝向的楼梯时不转角，那样会断开一条直线。
fn can_take_shape(state: BlockState, world: &World, pos: BlockPos, neighbour: Direction) -> bool {
    let beside = world.block_state(pos.relative(neighbour));
    !is_stairs(beside)
        || beside.get(HORIZONTAL_FACING) != state.get(HORIZONTAL_FACING)
        || beside.get(HALF) != state.get(HALF)
}

/// `getStairsShape`：背后的楼梯形成外角，前面的楼梯形成内角。
fn stairs_shape(state: BlockState, world: &World, pos: BlockPos) -> StairsShape {
    let facing = state.get(HORIZONTAL_FACING);
    let behind = world.block_state(pos.relative(facing));
    if let Some(behind_facing) = perpendicular_stairs(state, behind)
        && can_take_shape(state, world, pos, behind_facing.opposite())
    {
        return if behind_facing == facing.counter_clockwise() {
            StairsShape::OuterLeft
        } else {
            StairsShape::OuterRight
        };
    }

    let front = world.block_state(pos.relative(facing.opposite()));
    if let Some(front_facing) = perpendicular_stairs(state, front)
        && can_take_shape(state, world, pos, front_facing)
    {
        return if front_facing == facing.counter_clockwise() {
            StairsShape::InnerLeft
        } else {
            StairsShape::InnerRight
        };
    }
    StairsShape::Straight
}

impl Behaviour for Stair {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let face = ctx.clicked_face();
        let bottom = face != Direction::Down && (face == Direction::Up || !clicked_upper_half(ctx));
        let half = if bottom { Half::Bottom } else { Half::Top };
        let state = default_state(block)
            .with(HORIZONTAL_FACING, ctx.horizontal_direction())
            .with(HALF, half)
            .with(WATERLOGGED, is_water_source(world, pos));
        Some(state.with(STAIRS_SHAPE, stairs_shape(state, world, pos)))
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
        if direction.axis().is_horizontal() {
            state.with(STAIRS_SHAPE, stairs_shape(state, world, pos))
        } else {
            Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
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

    /// 玩家看着这个朝向时放的楼梯，官方的 `yaw` 0 是南，90 是西。
    fn actor_facing(yaw: f32) -> crate::Actor {
        let mut actor = actor_holding(items::OAK_STAIRS);
        actor.yaw = yaw;
        actor.head_yaw = yaw;
        actor
    }

    #[test]
    fn stairs_follow_the_way_the_player_looks() {
        for (yaw, facing) in [
            (0.0, Direction::South),
            (90.0, Direction::West),
            (180.0, Direction::North),
            (270.0, Direction::East),
        ] {
            let mut level = level_with_floor();
            use_item(&mut level, &actor_facing(yaw), top_of(FLOOR));
            let placed = level.get(FLOOR.above());
            assert!(placed.is(blocks::OAK_STAIRS));
            assert_eq!(placed.get(HORIZONTAL_FACING), facing);
        }
    }

    #[test]
    fn stairs_turn_the_corner_when_stairs_appear_behind_them() {
        let mut level = level_with_floor();
        level.set(FLOOR.south(), BlockState(blocks::STONE));
        use_item(&mut level, &actor_facing(0.0), top_of(FLOOR));
        use_item(&mut level, &actor_facing(90.0), top_of(FLOOR.south()));
        let corner = level.get(FLOOR.above());
        assert_eq!(corner.get(STAIRS_SHAPE), StairsShape::OuterRight);
    }
}
