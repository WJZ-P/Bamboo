//! `TripWireBlock`。
//!
//! 世界里看不到实体，线不会被踩下，`tick` 和 `checkPressed` 不做。
//! 线放下和拿走时要告诉两头的钩子，钩子再改整条线的 `attached`，这部分是方块状态。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, items};

use crate::{
    behaviour::{
        Behaviour, Plain,
        attached::tripwire_hook::{WIRE_DIST_MAX, WireSource, calculate_state},
        common::{default_state, field_block, pipe_property},
    },
    context::{Actor, PlaceContext},
    state::{BlockState, DISARMED, HORIZONTAL_FACING, POWERED},
    world::{UPDATE_NONE, World},
};

/// `TripWireBlock`。
pub(super) struct TripWire;

/// `shouldConnectTo`：连另一段线，或者朝着这边的钩子。
fn should_connect_to(wire: BlockState, neighbor: BlockState, direction: Direction) -> bool {
    let hook = field_block(wire.block(), "hook");
    if neighbor.is(hook.default_state) {
        neighbor.get(HORIZONTAL_FACING) == direction.opposite()
    } else {
        neighbor.is_same_block(wire)
    }
}

/// `updateSource`：往南和往西找到这段线的钩子，它们再管另一头。
fn update_source(state: BlockState, world: &mut World, pos: BlockPos) {
    let hook = field_block(state.block(), "hook");
    for direction in [Direction::South, Direction::West] {
        for distance in 1..WIRE_DIST_MAX {
            let test_pos = pos.relative_by(direction, distance);
            let block = world.block_state(test_pos);
            if block.is(hook.default_state) {
                if block.get(HORIZONTAL_FACING) == direction.opposite() {
                    let source = WireSource { distance, state };
                    calculate_state(world, test_pos, block, false, true, Some(source));
                }
                break;
            }
            if !block.is_same_block(state) {
                break;
            }
        }
    }
}

impl Behaviour for TripWire {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let wire = default_state(block);
        let mut state = wire;
        for direction in Direction::HORIZONTAL {
            let neighbor = world.block_state(pos.relative(direction));
            state = state.with(
                pipe_property(direction),
                should_connect_to(wire, neighbor, direction),
            );
        }
        Some(state)
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
        if direction.axis().is_horizontal() {
            return state.with(
                pipe_property(direction),
                should_connect_to(state, neighbor, direction),
            );
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if !old.is_same_block(state) {
            update_source(state, world, pos);
        }
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_source(state.with(POWERED, true), world, pos);
    }

    /// 用剪刀剪线时不触发钩子，线要先变成解除状态。
    fn player_will_destroy(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        player: &Actor,
    ) -> BlockState {
        if player.main_hand == items::SHEARS {
            world.set_block(pos, state.with(DISARMED, true), UPDATE_NONE);
        }
        Plain.player_will_destroy(state, world, pos, player)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{
        BlockTicks, destroy_block,
        state::ATTACHED,
        testing::{TestLevel, actor_holding, break_block, top_hit, use_item},
    };

    const WEST_HOOK: BlockPos = BlockPos::new(0, 64, 0);
    const EAST_HOOK: BlockPos = BlockPos::new(3, 64, 0);
    const WIRES: [BlockPos; 2] = [BlockPos::new(1, 64, 0), BlockPos::new(2, 64, 0)];

    /// 两个钩子面对面，最后一根线还没放。
    fn line_missing_a_wire() -> TestLevel {
        let mut level = TestLevel::new();
        let stone = BlockState(blocks::STONE);
        level.set(WEST_HOOK.west(), stone);
        level.set(EAST_HOOK.east(), stone);
        level.set(WIRES[1].below(), stone);
        let hook = BlockState(blocks::TRIPWIRE_HOOK);
        level.set(WEST_HOOK, hook.with(HORIZONTAL_FACING, Direction::East));
        level.set(EAST_HOOK, hook.with(HORIZONTAL_FACING, Direction::West));
        level.set(WIRES[0], BlockState(blocks::TRIPWIRE));
        level
    }

    fn finished_line() -> TestLevel {
        let mut level = line_missing_a_wire();
        let actor = actor_holding(items::STRING);
        use_item(&mut level, &actor, top_hit(WIRES[1].below()));
        level
    }

    #[test]
    fn the_last_wire_attaches_the_whole_line() {
        let level = finished_line();
        for pos in [WEST_HOOK, EAST_HOOK, WIRES[0], WIRES[1]] {
            assert!(level.get(pos).get(ATTACHED), "{pos:?}");
        }
    }

    /// 线被徒手弄断时钩子先触发，隔一会儿钩子自己的刻才断开。
    #[test]
    fn breaking_a_wire_by_hand_fires_the_hooks() {
        let mut level = finished_line();
        break_block(&mut level, WIRES[0]);
        assert!(level.get(WIRES[0]).is_air());
        for hook in [WEST_HOOK, EAST_HOOK] {
            assert!(level.get(hook).get(ATTACHED), "{hook:?}");
            assert!(level.get(hook).get(POWERED), "{hook:?}");
        }
    }

    #[test]
    fn shears_cut_the_line_without_firing_it() {
        let mut level = finished_line();
        let actor = actor_holding(items::SHEARS);
        destroy_block(&mut level, &mut BlockTicks::new(), &actor, WIRES[0]);
        assert!(level.get(WIRES[0]).is_air());
        for hook in [WEST_HOOK, EAST_HOOK] {
            assert!(!level.get(hook).get(ATTACHED), "{hook:?}");
            assert!(!level.get(hook).get(POWERED), "{hook:?}");
        }
        assert!(!level.get(WIRES[1]).get(ATTACHED));
    }

    /// 只有朝着这段线的钩子才算数。
    #[test]
    fn a_hook_facing_away_stays_out_of_it() {
        let mut level = line_missing_a_wire();
        let hook = level.get(WEST_HOOK);
        level.set(WEST_HOOK, hook.with(HORIZONTAL_FACING, Direction::North));
        let actor = actor_holding(items::STRING);
        use_item(&mut level, &actor, top_hit(WIRES[1].below()));
        assert!(!level.get(EAST_HOOK).get(ATTACHED));
        assert!(!level.get(WIRES[0]).get(ATTACHED));
    }
}
