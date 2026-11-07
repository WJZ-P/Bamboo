//! `TripWireHookBlock`。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{default_state, has_sturdy_wall, lost_wall_support, wall_state_for_placement},
    },
    context::{Actor, PlaceContext},
    state::{ATTACHED, BlockState, DISARMED, HORIZONTAL_FACING, POWERED},
    world::World,
};

/// `WIRE_DIST_MAX`：钩子最远够得着这么多格，不含这一格。
pub const WIRE_DIST_MAX: i32 = 42;
/// `RECHECK_PERIOD`：绊线通知过钩子之后，隔这么多刻再看一次。
const RECHECK_PERIOD: i32 = 10;

/// `calculateState` 的 `wireSource` 和 `wireSourceState`：绊线通知钩子时，它在几格外、
/// 现在算什么状态。世界里这一格可能已经不是这个状态，所以要传进来。
#[derive(Debug, Clone, Copy)]
pub struct WireSource {
    pub distance: i32,
    pub state: BlockState,
}

/// 沿着钩子的朝向看到的东西。
struct WireScan {
    attached: bool,
    powered: bool,
    /// 对面钩子的距离，没有时是 0。
    receiver: i32,
    /// 路上的绊线，官方的 `wireStates`。
    wires: Vec<(BlockPos, BlockState)>,
}

/// `calculateState` 前面的循环。
fn scan_wires(
    world: &mut World,
    pos: BlockPos,
    state: BlockState,
    is_being_destroyed: bool,
    wire_source: Option<WireSource>,
) -> WireScan {
    let direction = state.get(HORIZONTAL_FACING);
    let mut scan = WireScan {
        attached: !is_being_destroyed,
        powered: false,
        receiver: 0,
        wires: Vec::new(),
    };
    for distance in 1..WIRE_DIST_MAX {
        let wire_pos = pos.relative_by(direction, distance);
        let found = world.block_state(wire_pos);
        if found.is(blocks::TRIPWIRE_HOOK) {
            if found.get(HORIZONTAL_FACING) == direction.opposite() {
                scan.receiver = distance;
            }
            break;
        }
        let source = wire_source.filter(|source| source.distance == distance);
        if !found.is(blocks::TRIPWIRE) && source.is_none() {
            scan.attached = false;
            continue;
        }
        let wire = source.map_or(found, |source| source.state);
        let armed = !wire.get(DISARMED);
        scan.powered |= armed && wire.get(POWERED);
        scan.wires.push((wire_pos, wire));
        if source.is_some() {
            world.schedule_tick(pos, state.block(), RECHECK_PERIOD);
            scan.attached &= armed;
        }
    }
    scan
}

/// `notifyNeighbors`。
fn notify_neighbors(block: &'static Block, world: &mut World, pos: BlockPos, direction: Direction) {
    let front = direction.opposite();
    world.update_neighbors_at(pos, block);
    world.update_neighbors_at(pos.relative(front), block);
}

/// `calculateState` 最后的循环：钩子的连接变了，路上的绊线跟着变。
fn update_wires(world: &mut World, scan: &WireScan, attached: bool) {
    for &(pos, wire) in &scan.wires {
        let current = world.block_state(pos);
        if current.is(blocks::TRIPWIRE) || current.is(blocks::TRIPWIRE_HOOK) {
            world.set_block_and_update(pos, wire.with(ATTACHED, attached));
        }
    }
}

/// `calculateState`：重新算这个钩子、对面的钩子和中间的绊线。
///
/// `TripWireBlock` 的方块变化时也要调用它。
pub fn calculate_state(
    world: &mut World,
    pos: BlockPos,
    state: BlockState,
    is_being_destroyed: bool,
    can_update: bool,
    wire_source: Option<WireSource>,
) {
    let direction = state.get(HORIZONTAL_FACING);
    let was_attached = state.get(ATTACHED);
    let block = state.block();
    let scan = scan_wires(world, pos, state, is_being_destroyed, wire_source);
    let attached = scan.attached && scan.receiver > 1;
    let powered = scan.powered && attached;
    let new_state = default_state(block)
        .with(ATTACHED, attached)
        .with(POWERED, powered);

    if scan.receiver > 0 {
        let receiver_pos = pos.relative_by(direction, scan.receiver);
        let opposite = direction.opposite();
        world.set_block_and_update(receiver_pos, new_state.with(HORIZONTAL_FACING, opposite));
        notify_neighbors(block, world, receiver_pos, opposite);
        if !world.block_state(pos).is(blocks::TRIPWIRE_HOOK) {
            // 官方这里传的是还没写朝向的新状态，所以朝向是默认值。
            on_removed(new_state, world, pos);
            return;
        }
    }
    if !is_being_destroyed {
        world.set_block_and_update(pos, new_state.with(HORIZONTAL_FACING, direction));
        if can_update {
            notify_neighbors(block, world, pos, direction);
        }
    }
    if was_attached != attached && scan.receiver > 0 {
        update_wires(world, &scan, attached);
    }
}

/// `onRemoved`。
fn on_removed(state: BlockState, world: &mut World, pos: BlockPos) {
    let attached = state.get(ATTACHED);
    let powered = state.get(POWERED);
    if attached || powered {
        calculate_state(world, pos, state, true, false, None);
    }
    if powered {
        notify_neighbors(state.block(), world, pos, state.get(HORIZONTAL_FACING));
    }
}

/// `TripWireHookBlock`。
pub(super) struct TripWireHook;

impl Behaviour for TripWireHook {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        wall_state_for_placement(default_state(block), ctx, world)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        has_sturdy_wall(state, world, pos)
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
        if lost_wall_support(state, world, pos, direction) {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        on_removed(state, world, pos);
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        calculate_state(world, pos, state, false, false, None);
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        calculate_state(world, pos, state, false, true, None);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::items;

    use super::*;
    use crate::{
        BlockTicks,
        behaviour::attached::test_support::hit_side,
        interact::run_scheduled_ticks,
        testing::{TestLevel, actor_holding, use_item},
    };

    const FIRST_WALL: BlockPos = BlockPos::new(0, 64, 0);
    const SECOND_WALL: BlockPos = BlockPos::new(5, 64, 0);
    const FIRST_HOOK: BlockPos = FIRST_WALL.east();
    const SECOND_HOOK: BlockPos = SECOND_WALL.west();
    const WIRES: [BlockPos; 2] = [BlockPos::new(2, 64, 0), BlockPos::new(3, 64, 0)];

    /// 两面墙之间拉着绊线，第一个钩子已经挂好，第二个还没放。
    fn half_finished_line() -> TestLevel {
        let mut level = TestLevel::new();
        level.set(FIRST_WALL, BlockState(blocks::STONE));
        level.set(SECOND_WALL, BlockState(blocks::STONE));
        let hook = BlockState(blocks::TRIPWIRE_HOOK).with(HORIZONTAL_FACING, Direction::East);
        level.set(FIRST_HOOK, hook);
        for wire in WIRES {
            level.set(wire, BlockState(blocks::TRIPWIRE));
        }
        level
    }

    fn finished_line() -> TestLevel {
        let mut level = half_finished_line();
        use_item(
            &mut level,
            &actor_holding(items::TRIPWIRE_HOOK),
            hit_side(SECOND_WALL, Direction::West),
        );
        level
    }

    fn assert_line(level: &TestLevel, attached: bool, powered: bool) {
        for hook in [FIRST_HOOK, SECOND_HOOK] {
            assert_eq!(level.get(hook).get(ATTACHED), attached, "{hook:?}");
            assert_eq!(level.get(hook).get(POWERED), powered, "{hook:?}");
        }
    }

    #[test]
    fn the_second_hook_attaches_the_whole_line() {
        let level = finished_line();
        assert_line(&level, true, false);
        assert_eq!(
            level.get(SECOND_HOOK).get(HORIZONTAL_FACING),
            Direction::West
        );
        assert!(level.get(WIRES[0]).get(ATTACHED));
        assert!(level.get(WIRES[1]).get(ATTACHED));
    }

    #[test]
    fn a_gap_in_the_wire_leaves_the_hooks_apart() {
        let mut level = half_finished_line();
        level.set(WIRES[1], BlockState(blocks::STONE));
        use_item(
            &mut level,
            &actor_holding(items::TRIPWIRE_HOOK),
            hit_side(SECOND_WALL, Direction::West),
        );
        assert!(!level.get(SECOND_HOOK).get(ATTACHED));
        assert!(!level.get(FIRST_HOOK).get(ATTACHED));
    }

    /// 绊线被弄断时钩子先触发，隔 10 刻再看一次才断开。`TripWireBlock` 在移除时这样通知钩子。
    #[test]
    fn a_broken_wire_fires_the_line_and_detaches_it_on_the_recheck() {
        let mut level = finished_line();
        let mut ticks = BlockTicks::new();
        let wire = level.get(WIRES[1]);
        level.set(WIRES[1], BlockState::AIR);

        let hook = level.get(SECOND_HOOK);
        let source = WireSource {
            distance: 1,
            state: wire.with(POWERED, true),
        };
        let mut world = World::new(&mut level, &mut ticks);
        calculate_state(&mut world, SECOND_HOOK, hook, false, true, Some(source));
        assert_line(&level, true, true);

        let outcome = run_scheduled_ticks(&mut level, &mut ticks, i64::from(RECHECK_PERIOD));
        assert!(outcome.effects.is_empty());
        assert_line(&level, false, false);
        assert!(!level.get(WIRES[0]).get(ATTACHED));
    }

    #[test]
    fn removing_an_attached_hook_detaches_the_other_one() {
        let mut level = finished_line();
        let hook = level.get(SECOND_HOOK);
        level.set(SECOND_HOOK, BlockState::AIR);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        on_removed(hook, &mut world, SECOND_HOOK);
        assert!(!level.get(FIRST_HOOK).get(ATTACHED));
    }
}
