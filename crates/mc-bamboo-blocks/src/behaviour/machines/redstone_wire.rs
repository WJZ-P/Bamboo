//! `RedstoneWireBlock` 和官方的 `DefaultRedstoneWireEvaluator`。
//!
//! 世界里没有信号，放下的线永远是 0。状态里写着 `power` 的线只会慢慢熄掉，
//! 这和官方在没有信号时的结果相同，所以强度的计算照官方写。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor, BlockClass, blocks};

use crate::{
    behaviour::{Behaviour, common::default_state},
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{
        BlockState, EAST_REDSTONE, FACING, HORIZONTAL_FACING, NORTH_REDSTONE, POWER, Property,
        RedstoneSide, SOUTH_REDSTONE, SupportType, WEST_REDSTONE,
    },
    world::{UPDATE_CLIENTS, World},
};

/// `RedstoneWireBlock`。
pub(super) struct RedstoneWire;

/// `PROPERTY_BY_DIRECTION`。
const fn side_property(direction: Direction) -> Property<RedstoneSide> {
    match direction {
        Direction::North => NORTH_REDSTONE,
        Direction::East => EAST_REDSTONE,
        Direction::South => SOUTH_REDSTONE,
        Direction::West => WEST_REDSTONE,
        Direction::Up | Direction::Down => panic!("redstone wire only connects sideways"),
    }
}

/// `RedstoneSide.isConnected`。
fn is_connected(state: BlockState, direction: Direction) -> bool {
    state.get(side_property(direction)) != RedstoneSide::None
}

/// `isCross`。
fn is_cross(state: BlockState) -> bool {
    Direction::HORIZONTAL
        .into_iter()
        .all(|direction| is_connected(state, direction))
}

/// `isDot`。
fn is_dot(state: BlockState) -> bool {
    !Direction::HORIZONTAL
        .into_iter()
        .any(|direction| is_connected(state, direction))
}

/// `crossState`：四面都连着，官方放下时从它开始算。
fn cross_state(wire: BlockState) -> BlockState {
    let mut state = wire.default_state();
    for direction in Direction::HORIZONTAL {
        state = state.with(side_property(direction), RedstoneSide::Side);
    }
    state
}

/// `canSurviveOn`。形状跟着渲染偏移走的方块都不是整面，查表的结果在任何位置上都对。
fn can_survive_on(state: BlockState) -> bool {
    state.is_face_sturdy(Direction::Up, SupportType::Full) || state.is(blocks::HOPPER)
}

/// `shouldRedstoneWireConnectTo`，官方的 `direction` 是空时这里是 `None`。
fn should_connect_to(state: BlockState, direction: Option<Direction>) -> bool {
    match state.class() {
        BlockClass::RedstoneWireBlock => true,
        BlockClass::ObserverBlock => direction == Some(state.get(FACING)),
        BlockClass::RepeaterBlock => {
            let facing = state.get(HORIZONTAL_FACING);
            direction == Some(facing) || direction == Some(facing.opposite())
        }
        _ => state.is_signal_source() && direction.is_some(),
    }
}

/// `getConnectingSide`。
fn connecting_side(
    world: &World,
    pos: BlockPos,
    direction: Direction,
    can_connect_up: bool,
) -> RedstoneSide {
    let relative_pos = pos.relative(direction);
    let relative = world.block_state(relative_pos);
    if can_connect_up {
        let placeable_above =
            relative.is_a(BlockAncestor::TrapDoorBlock) || can_survive_on(relative);
        let above = world.block_state(relative_pos.above());
        if placeable_above && should_connect_to(above, None) {
            return if relative.is_face_sturdy(direction.opposite(), SupportType::Full) {
                RedstoneSide::Up
            } else {
                RedstoneSide::Side
            };
        }
    }
    let below = world.block_state(relative_pos.below());
    if should_connect_to(relative, Some(direction))
        || !relative.is_redstone_conductor() && should_connect_to(below, None)
    {
        RedstoneSide::Side
    } else {
        RedstoneSide::None
    }
}

/// 上面一格不导电时，线才能顺着墙往上连。
fn can_connect_up(world: &World, pos: BlockPos) -> bool {
    !world.block_state(pos.above()).is_redstone_conductor()
}

/// `getMissingConnections`。
fn with_missing_connections(state: BlockState, world: &World, pos: BlockPos) -> BlockState {
    let up = can_connect_up(world, pos);
    let mut state = state;
    for direction in Direction::HORIZONTAL {
        if !is_connected(state, direction) {
            let side = connecting_side(world, pos, direction, up);
            state = state.with(side_property(direction), side);
        }
    }
    state
}

/// `getConnectionState`：只连一面的线也要画成一条直线，没有连接的十字点算点。
fn connection_state(state: BlockState, world: &World, pos: BlockPos) -> BlockState {
    let was_dot = is_dot(state);
    let plain = state.default_state().with(POWER, state.get(POWER));
    let mut updated = with_missing_connections(plain, world, pos);
    if was_dot && is_dot(updated) {
        return updated;
    }
    let north_south_empty =
        !is_connected(updated, Direction::North) && !is_connected(updated, Direction::South);
    let east_west_empty =
        !is_connected(updated, Direction::East) && !is_connected(updated, Direction::West);
    if north_south_empty {
        updated = updated.with(WEST_REDSTONE, connected_or_side(updated, Direction::West));
        updated = updated.with(EAST_REDSTONE, connected_or_side(updated, Direction::East));
    }
    if east_west_empty {
        updated = updated.with(NORTH_REDSTONE, connected_or_side(updated, Direction::North));
        updated = updated.with(SOUTH_REDSTONE, connected_or_side(updated, Direction::South));
    }
    updated
}

/// 已经连着时保持原样，没有连着时补一段平的。
fn connected_or_side(state: BlockState, direction: Direction) -> RedstoneSide {
    if is_connected(state, direction) {
        state.get(side_property(direction))
    } else {
        RedstoneSide::Side
    }
}

/// 同一层的线和隔着方块上下一格的线里最强的一根，减一。
fn incoming_wire_signal(world: &World, pos: BlockPos) -> i32 {
    let wire_power = |at: BlockPos| {
        let state = world.block_state(at);
        if state.is(blocks::REDSTONE_WIRE) {
            state.get(POWER)
        } else {
            0
        }
    };
    let above_conducts = world.block_state(pos.above()).is_redstone_conductor();
    let mut strongest = 0;
    for direction in Direction::HORIZONTAL {
        let neighbor_pos = pos.relative(direction);
        strongest = strongest.max(wire_power(neighbor_pos));
        let conducts = world.block_state(neighbor_pos).is_redstone_conductor();
        if conducts && !above_conducts {
            strongest = strongest.max(wire_power(neighbor_pos.above()));
        } else if !conducts {
            strongest = strongest.max(wire_power(neighbor_pos.below()));
        }
    }
    (strongest - 1).max(0)
}

/// `calculateTargetStrength`。
fn target_strength(world: &World, pos: BlockPos) -> i32 {
    let block_signal = world.best_neighbor_signal(pos);
    if block_signal == 15 {
        block_signal
    } else {
        block_signal.max(incoming_wire_signal(world, pos))
    }
}

/// `new HashSet<BlockPos>` 的遍历顺序，官方按它更新七格的邻居。
///
/// 七项放得下默认的 16 个桶，不会扩容。同一个桶里是加入的顺序，所以用稳定排序。
fn java_hash_order(positions: &mut [BlockPos]) {
    positions.sort_by_key(|pos| {
        let hash = pos
            .y
            .wrapping_add(pos.z.wrapping_mul(31))
            .wrapping_mul(31)
            .wrapping_add(pos.x);
        (hash ^ ((hash as u32) >> 16) as i32) & 15
    });
}

/// `DefaultRedstoneWireEvaluator.updatePowerStrength`。
fn update_power_strength(state: BlockState, world: &mut World, pos: BlockPos) {
    let target = target_strength(world, pos);
    if state.get(POWER) == target {
        return;
    }
    if world.block_state(pos) == state {
        world.set_block(pos, state.with(POWER, target), UPDATE_CLIENTS);
    }
    let mut to_update = [pos; 7];
    for (slot, direction) in to_update[1..].iter_mut().zip(Direction::ALL) {
        *slot = pos.relative(direction);
    }
    java_hash_order(&mut to_update);
    for at in to_update {
        world.update_neighbors_at(at, state.block());
    }
}

/// `checkCornerChangeAt`。
fn check_corner_change_at(wire: BlockState, world: &mut World, pos: BlockPos) {
    if !world.block_state(pos).is_same_block(wire) {
        return;
    }
    world.update_neighbors_at(pos, wire.block());
    for direction in Direction::ALL {
        world.update_neighbors_at(pos.relative(direction), wire.block());
    }
}

/// `updateNeighborsOfNeighboringWires`。
fn update_neighbors_of_neighboring_wires(wire: BlockState, world: &mut World, pos: BlockPos) {
    for direction in Direction::HORIZONTAL {
        check_corner_change_at(wire, world, pos.relative(direction));
    }
    for direction in Direction::HORIZONTAL {
        let target = pos.relative(direction);
        if world.block_state(target).is_redstone_conductor() {
            check_corner_change_at(wire, world, target.above());
        } else {
            check_corner_change_at(wire, world, target.below());
        }
    }
}

/// `updatesOnShapeChange`：右键改了形状后，告诉连接变了的导电方块。
fn updates_on_shape_change(world: &mut World, pos: BlockPos, old: BlockState, new: BlockState) {
    for direction in Direction::HORIZONTAL {
        let relative_pos = pos.relative(direction);
        if is_connected(old, direction) != is_connected(new, direction)
            && world.block_state(relative_pos).is_redstone_conductor()
        {
            world.update_neighbors_at_except_from_facing(
                relative_pos,
                new.block(),
                direction.opposite(),
            );
        }
    }
}

impl Behaviour for RedstoneWire {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let cross = cross_state(default_state(block));
        Some(connection_state(cross, world, ctx.clicked_pos()))
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        can_survive_on(world.block_state(pos.below()))
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
        if direction == Direction::Down {
            return if can_survive_on(neighbor) {
                state
            } else {
                BlockState::AIR
            };
        }
        if direction == Direction::Up {
            return connection_state(state, world, pos);
        }
        let property = side_property(direction);
        let side = connecting_side(world, pos, direction, can_connect_up(world, pos));
        if (side != RedstoneSide::None) == is_connected(state, direction) && !is_cross(state) {
            return state.with(property, side);
        }
        let cross = cross_state(state).with(POWER, state.get(POWER));
        connection_state(cross.with(property, side), world, pos)
    }

    fn update_indirect_neighbour_shapes(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        flags: u32,
        limit: u32,
    ) {
        for direction in Direction::HORIZONTAL {
            let beside = pos.relative(direction);
            if !is_connected(state, direction) || world.block_state(beside).is_same_block(state) {
                continue;
            }
            for wire_pos in [beside.below(), beside.above()] {
                if world.block_state(wire_pos).is_same_block(state) {
                    let neighbor_pos = wire_pos.relative(direction.opposite());
                    let neighbor = world.block_state(neighbor_pos);
                    world.neighbor_shape_changed(
                        direction.opposite(),
                        wire_pos,
                        neighbor_pos,
                        neighbor,
                        flags,
                        limit,
                    );
                }
            }
        }
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if old.is_same_block(state) {
            return;
        }
        update_power_strength(state, world, pos);
        for direction in Direction::VERTICAL {
            world.update_neighbors_at(pos.relative(direction), state.block());
        }
        update_neighbors_of_neighboring_wires(state, world, pos);
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        for direction in Direction::ALL {
            world.update_neighbors_at(pos.relative(direction), state.block());
        }
        update_power_strength(state, world, pos);
        update_neighbors_of_neighboring_wires(state, world, pos);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        if state.can_survive(world, pos) {
            update_power_strength(state, world, pos);
        } else {
            world.remove_block(pos);
        }
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        if !is_cross(state) && !is_dot(state) {
            return InteractionResult::Pass;
        }
        let pos = ctx.clicked_pos();
        let toggled = if is_cross(state) {
            state.default_state()
        } else {
            cross_state(state)
        };
        let updated = connection_state(toggled.with(POWER, state.get(POWER)), world, pos);
        if updated == state {
            return InteractionResult::Pass;
        }
        world.set_block_and_update(pos, updated);
        updates_on_shape_change(world, pos, state, updated);
        InteractionResult::Success
    }
}
