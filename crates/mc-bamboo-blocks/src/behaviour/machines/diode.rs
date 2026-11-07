//! `DiodeBlock` 和它的子类 `RepeaterBlock`、`ComparatorBlock`。
//!
//! 世界里没有信号。输入只可能来自状态里写着 `power` 的红石线，这样的线也是 0，
//! 所以 `tick`、`checkTickOnNeighbor`、`setPlacedBy` 排的计划刻都不做。
//! 这里留下的是存活、形状更新、右键和前方邻居的更新。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, blocks};

use crate::{
    behaviour::{Behaviour, Plain, common::facing_the_player},
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{
        BlockState, ComparatorMode, DELAY, HORIZONTAL_FACING, LOCKED, MODE_COMPARATOR, POWER,
        POWERED, SupportType,
    },
    world::{UPDATE_CLIENTS, World},
};

/// `DiodeBlock`。
struct Diode;

/// `canSurviveOn`：形状跟着渲染偏移走的方块都撑不住，查表的结果在任何位置上都对。
fn can_survive_on(state: BlockState) -> bool {
    state.is_face_sturdy(Direction::Up, SupportType::Rigid)
}

/// `updateNeighborsInFront`：输出的方向是 `facing` 的反方向。
fn update_neighbors_in_front(state: BlockState, world: &mut World, pos: BlockPos) {
    let facing = state.get(HORIZONTAL_FACING);
    let front = pos.relative(facing.opposite());
    world.neighbor_changed(front, state.block());
    world.update_neighbors_at_except_from_facing(front, state.block(), facing);
}

/// `getInputSignal`。比较器读方块的模拟信号的部分不做，那是红石信号。
fn input_signal(state: BlockState, world: &World, pos: BlockPos) -> i32 {
    let facing = state.get(HORIZONTAL_FACING);
    let target_pos = pos.relative(facing);
    let target = world.block_state(target_pos);
    let wire_power = if target.is(blocks::REDSTONE_WIRE) {
        target.get(POWER)
    } else {
        0
    };
    world.signal(target_pos, facing).max(wire_power)
}

impl Behaviour for Diode {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        _world: &World,
    ) -> Option<BlockState> {
        Some(facing_the_player(block, ctx))
    }

    fn can_survive(&self, _state: BlockState, world: &World, pos: BlockPos) -> bool {
        world.is_face_sturdy(pos.below(), Direction::Up, SupportType::Rigid)
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        update_neighbors_in_front(state, world, pos);
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbors_in_front(state, world, pos);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        if !world.block_state(pos).is_same_block(state) || state.can_survive(world, pos) {
            return;
        }
        world.remove_block(pos);
        for direction in Direction::ALL {
            world.update_neighbors_at(pos.relative(direction), state.block());
        }
    }
}

/// `RepeaterBlock`。
pub(super) struct Repeater;

/// `isLocked`：两侧有中继器或者比较器的信号时锁住。
fn is_locked(state: BlockState, world: &World, pos: BlockPos) -> bool {
    let facing = state.get(HORIZONTAL_FACING);
    let side_signal = |side: Direction| world.signal(pos.relative(side), side);
    side_signal(facing.clockwise()).max(side_signal(facing.counter_clockwise())) > 0
}

impl Behaviour for Repeater {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let state = Diode.state_for_placement(block, ctx, world)?;
        Some(state.with(LOCKED, is_locked(state, world, ctx.clicked_pos())))
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        Diode.can_survive(state, world, pos)
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
        if direction == Direction::Down && !can_survive_on(neighbor) {
            return BlockState::AIR;
        }
        if direction.axis() != state.get(HORIZONTAL_FACING).axis() {
            return state.with(LOCKED, is_locked(state, world, pos));
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        Diode.on_place(state, world, pos, old);
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        Diode.affect_neighbors_after_removal(state, world, pos);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        source: &'static Block,
    ) {
        Diode.neighbor_changed(state, world, pos, source);
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        world.set_block_and_update(ctx.clicked_pos(), state.cycle(DELAY));
        InteractionResult::Success
    }
}

/// `ComparatorBlock`。
pub(super) struct Comparator;

/// `refreshOutputState`。两侧没有信号，所以输出就是输入。方块实体里记的旧输出不做，当作 0。
fn refresh_output_state(state: BlockState, world: &mut World, pos: BlockPos) {
    let output = input_signal(state, world, pos);
    if output == 0 && state.get(MODE_COMPARATOR) != ComparatorMode::Compare {
        return;
    }
    let source_on = output > 0;
    if state.get(POWERED) != source_on {
        world.set_block(pos, state.with(POWERED, source_on), UPDATE_CLIENTS);
    }
    update_neighbors_in_front(state, world, pos);
}

impl Behaviour for Comparator {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        Diode.state_for_placement(block, ctx, world)
    }

    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        Diode.can_survive(state, world, pos)
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
        if direction == Direction::Down && !can_survive_on(neighbor) {
            return BlockState::AIR;
        }
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        Diode.on_place(state, world, pos, old);
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        Diode.affect_neighbors_after_removal(state, world, pos);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        source: &'static Block,
    ) {
        Diode.neighbor_changed(state, world, pos, source);
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let pos = ctx.clicked_pos();
        let cycled = state.cycle(MODE_COMPARATOR);
        world.set_block(pos, cycled, UPDATE_CLIENTS);
        if world.block_state(pos).is_same_block(state) {
            refresh_output_state(cycled, world, pos);
        }
        InteractionResult::Success
    }
}
