//! `DiodeBlock` 和它的子类 `RepeaterBlock`、`ComparatorBlock`。
//!
//! 官方把 `getDelay`、`isLocked`、`getInputSignal`、`shouldTurnOn`、`getOutputSignal`
//! 和 `checkTickOnNeighbor` 都写成虚方法，子类各覆盖一部分。这里的自由函数按
//! `state.is_a(...)` 分派，效果和虚调用一样。
//!
//! 比较器的输出强度存在方块实体里，这里放在 [`RedstoneMemory`](crate::RedstoneMemory)。
//! 容器和实体都没有，所以模拟信号只有方块状态能决定的那些有值。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockAncestor, blocks, sounds::sound_event};

use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{facing_the_player, field_bool},
    },
    context::{Actor, PlaceContext, UseContext},
    interact::InteractionResult,
    state::{
        BlockState, ComparatorMode, DELAY, HORIZONTAL_FACING, LOCKED, MODE_COMPARATOR, POWER,
        POWERED, SupportType,
    },
    ticks::TickPriority,
    world::{UPDATE_CLIENTS, World},
};

/// `DiodeBlock`。
struct Diode;

/// `canSurviveOn`：下面那一格是 `moving_piston` 时，形状来自它的方块实体。
fn can_survive_on(world: &World, below: BlockPos, state: BlockState) -> bool {
    world.is_state_face_sturdy(state, below, Direction::Up, SupportType::Rigid)
}

/// `updateNeighborsInFront`：输出的方向是 `facing` 的反方向。
fn update_neighbors_in_front(state: BlockState, world: &mut World, pos: BlockPos) {
    let facing = state.get(HORIZONTAL_FACING);
    let front = pos.relative(facing.opposite());
    world.neighbor_changed(front, state.block());
    world.update_neighbors_at_except_from_facing(front, state.block(), facing);
}

/// `getDelay`：中继器按档位，比较器固定两刻。
fn delay(state: BlockState) -> i32 {
    if state.is_a(BlockAncestor::RepeaterBlock) {
        state.get(DELAY) * 2
    } else {
        2
    }
}

/// `getAlternateSignal`：两侧的输入里强的那个。
fn alternate_signal(state: BlockState, world: &World, pos: BlockPos) -> i32 {
    let facing = state.get(HORIZONTAL_FACING);
    let diodes_only = field_bool(state.block(), "sideInputDiodesOnly()");
    let side = |side: Direction| world.control_input_signal(pos.relative(side), side, diodes_only);
    side(facing.clockwise()).max(side(facing.counter_clockwise()))
}

/// `isLocked`：只有中继器会被两侧的中继器或者比较器锁住。
fn is_locked(state: BlockState, world: &World, pos: BlockPos) -> bool {
    state.is_a(BlockAncestor::RepeaterBlock) && alternate_signal(state, world, pos) > 0
}

/// `DiodeBlock.getInputSignal`：正后方那一格的信号，导线的功率要单独看一眼。
fn diode_input_signal(state: BlockState, world: &World, pos: BlockPos) -> i32 {
    let facing = state.get(HORIZONTAL_FACING);
    let target_pos = pos.relative(facing);
    let input = world.signal(target_pos, facing);
    if input >= 15 {
        return input;
    }
    let target = world.block_state(target_pos);
    let wire_power = if target.is(blocks::REDSTONE_WIRE) {
        target.get(POWER)
    } else {
        0
    };
    input.max(wire_power)
}

/// `ComparatorBlock.getInputSignal`：后面是导体时，隔着它读下一格的模拟信号。
/// 官方还要看那一格的物品展示框，这个世界里没有实体。
fn comparator_input_signal(state: BlockState, world: &World, pos: BlockPos) -> i32 {
    let signal = diode_input_signal(state, world, pos);
    let facing = state.get(HORIZONTAL_FACING);
    let target_pos = pos.relative(facing);
    let target = world.block_state(target_pos);
    if target.has_analog_output_signal() {
        return target.analog_output_signal(world, target_pos, facing.opposite());
    }
    if signal >= 15 || !target.is_redstone_conductor() {
        return signal;
    }
    let behind_pos = target_pos.relative(facing);
    let behind = world.block_state(behind_pos);
    if behind.has_analog_output_signal() {
        behind.analog_output_signal(world, behind_pos, facing.opposite())
    } else {
        signal
    }
}

/// `getInputSignal`。
fn input_signal(state: BlockState, world: &World, pos: BlockPos) -> i32 {
    if state.is_a(BlockAncestor::ComparatorBlock) {
        comparator_input_signal(state, world, pos)
    } else {
        diode_input_signal(state, world, pos)
    }
}

/// `ComparatorBlock.shouldTurnOn`：比较模式下两边一样大也算亮。
fn comparator_should_turn_on(state: BlockState, world: &World, pos: BlockPos) -> bool {
    let input = input_signal(state, world, pos);
    if input == 0 {
        return false;
    }
    let side = alternate_signal(state, world, pos);
    input > side || input == side && state.get(MODE_COMPARATOR) == ComparatorMode::Compare
}

/// `shouldTurnOn`。
fn should_turn_on(state: BlockState, world: &World, pos: BlockPos) -> bool {
    if state.is_a(BlockAncestor::ComparatorBlock) {
        comparator_should_turn_on(state, world, pos)
    } else {
        input_signal(state, world, pos) > 0
    }
}

/// `getOutputSignal`：中继器永远是满的，比较器输出算好的那个数。
fn output_signal(state: BlockState, world: &World, pos: BlockPos) -> i32 {
    if state.is_a(BlockAncestor::ComparatorBlock) {
        world.redstone_memory().comparator_output(pos)
    } else {
        15
    }
}

/// `shouldPrioritize`：正后方是一个不冲着自己的二极管，也就是官方说的“排成一列”。
fn should_prioritize(state: BlockState, world: &World, pos: BlockPos) -> bool {
    let behind = state.get(HORIZONTAL_FACING).opposite();
    let other = world.block_state(pos.relative(behind));
    other.is_a(BlockAncestor::DiodeBlock) && other.get(HORIZONTAL_FACING) != behind
}

/// `ComparatorBlock.calculateOutputSignal`。
fn calculate_output_signal(state: BlockState, world: &World, pos: BlockPos) -> i32 {
    let input = input_signal(state, world, pos);
    if input == 0 {
        return 0;
    }
    let alternate = alternate_signal(state, world, pos);
    if alternate > input {
        return 0;
    }
    if state.get(MODE_COMPARATOR) == ComparatorMode::Subtract {
        input - alternate
    } else {
        input
    }
}

/// `ComparatorBlock.refreshOutputState`：输出强度变了，或者在比较模式下，都要更新前方。
fn refresh_output_state(state: BlockState, world: &mut World, pos: BlockPos) {
    let output = calculate_output_signal(state, world, pos);
    let old = world.redstone_memory().comparator_output(pos);
    world
        .redstone_memory_mut()
        .set_comparator_output(pos, output);
    if old == output && state.get(MODE_COMPARATOR) != ComparatorMode::Compare {
        return;
    }
    let source_on = should_turn_on(state, world, pos);
    if state.get(POWERED) != source_on {
        world.set_block(pos, state.with(POWERED, source_on), UPDATE_CLIENTS);
    }
    update_neighbors_in_front(state, world, pos);
}

/// `ComparatorBlock.checkTickOnNeighbor`：输出强度也算变化，所以比中继器多看一个数。
fn comparator_check_tick_on_neighbor(state: BlockState, world: &mut World, pos: BlockPos) {
    if world.will_tick_this_tick(pos, state.block()) {
        return;
    }
    let output = calculate_output_signal(state, world, pos);
    let old = world.redstone_memory().comparator_output(pos);
    if output == old && state.get(POWERED) == should_turn_on(state, world, pos) {
        return;
    }
    let priority = if should_prioritize(state, world, pos) {
        TickPriority::High
    } else {
        TickPriority::Normal
    };
    world.schedule_tick_with_priority(pos, state.block(), 2, priority);
}

/// `DiodeBlock.checkTickOnNeighbor`。
fn diode_check_tick_on_neighbor(state: BlockState, world: &mut World, pos: BlockPos) {
    if is_locked(state, world, pos) {
        return;
    }
    let on = state.get(POWERED);
    let turning_on = should_turn_on(state, world, pos);
    if on == turning_on || world.will_tick_this_tick(pos, state.block()) {
        return;
    }
    let priority = if should_prioritize(state, world, pos) {
        TickPriority::ExtremelyHigh
    } else if on {
        TickPriority::VeryHigh
    } else {
        TickPriority::High
    };
    world.schedule_tick_with_priority(pos, state.block(), delay(state), priority);
}

/// `checkTickOnNeighbor`。
fn check_tick_on_neighbor(state: BlockState, world: &mut World, pos: BlockPos) {
    if state.is_a(BlockAncestor::ComparatorBlock) {
        comparator_check_tick_on_neighbor(state, world, pos);
    } else {
        diode_check_tick_on_neighbor(state, world, pos);
    }
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
        if !world.block_state(pos).is_same_block(state) {
            return;
        }
        if state.can_survive(world, pos) {
            check_tick_on_neighbor(state, world, pos);
            return;
        }
        world.remove_block(pos);
        for direction in Direction::ALL {
            world.update_neighbors_at(pos.relative(direction), state.block());
        }
    }

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, _placer: &Actor) {
        if should_turn_on(state, world, pos) {
            world.schedule_tick(pos, state.block(), 1);
        }
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if is_locked(state, world, pos) {
            return;
        }
        let on = state.get(POWERED);
        let turning_on = should_turn_on(state, world, pos);
        if on && !turning_on {
            world.set_block(pos, state.with(POWERED, false), UPDATE_CLIENTS);
        } else if !on {
            world.set_block(pos, state.with(POWERED, true), UPDATE_CLIENTS);
            if !turning_on {
                // 输入已经没了，再排一刻把自己关掉，这就是中继器的脉冲延长。
                world.schedule_tick_with_priority(
                    pos,
                    state.block(),
                    delay(state),
                    TickPriority::VeryHigh,
                );
            }
        }
    }

    fn own_signal(&self, state: BlockState, world: &World, pos: BlockPos) -> i32 {
        if state.get(POWERED) {
            output_signal(state, world, pos)
        } else {
            0
        }
    }

    fn signal(&self, state: BlockState, world: &World, pos: BlockPos, direction: Direction) -> i32 {
        if state.get(HORIZONTAL_FACING) == direction {
            state.own_signal(world, pos)
        } else {
            0
        }
    }

    fn direct_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        state.signal(world, pos, direction)
    }
}

/// `RepeaterBlock`。
pub(super) struct Repeater;

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
        if direction == Direction::Down && !can_survive_on(world, neighbor_pos, neighbor) {
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

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, placer: &Actor) {
        Diode.set_placed_by(state, world, pos, placer);
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

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        Diode.tick(state, world, pos);
    }

    fn own_signal(&self, state: BlockState, world: &World, pos: BlockPos) -> i32 {
        Diode.own_signal(state, world, pos)
    }

    fn signal(&self, state: BlockState, world: &World, pos: BlockPos, direction: Direction) -> i32 {
        Diode.signal(state, world, pos, direction)
    }

    fn direct_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        Diode.direct_signal(state, world, pos, direction)
    }
}

/// `ComparatorBlock`。
pub(super) struct Comparator;

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
        if direction == Direction::Down && !can_survive_on(world, neighbor_pos, neighbor) {
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

    fn set_placed_by(&self, state: BlockState, world: &mut World, pos: BlockPos, placer: &Actor) {
        Diode.set_placed_by(state, world, pos, placer);
    }

    fn use_without_item(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let pos = ctx.clicked_pos();
        // 官方先按换过模式的状态定音高、出声，再写方块。
        let cycled = state.cycle(MODE_COMPARATOR);
        let pitch = if cycled.get(MODE_COMPARATOR) == ComparatorMode::Subtract {
            0.55
        } else {
            0.5
        };
        world.play_block_sound_except(
            pos,
            sound_event("minecraft:block.comparator.click"),
            0.3,
            pitch,
        );
        world.set_block(pos, cycled, UPDATE_CLIENTS);
        if world.block_state(pos).is_same_block(state) {
            refresh_output_state(cycled, world, pos);
        }
        InteractionResult::Success
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        refresh_output_state(state, world, pos);
    }

    fn own_signal(&self, state: BlockState, world: &World, pos: BlockPos) -> i32 {
        Diode.own_signal(state, world, pos)
    }

    fn signal(&self, state: BlockState, world: &World, pos: BlockPos, direction: Direction) -> i32 {
        Diode.signal(state, world, pos, direction)
    }

    fn direct_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        Diode.direct_signal(state, world, pos, direction)
    }
}
