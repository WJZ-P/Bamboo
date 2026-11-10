//! `RedstoneTorchBlock`、`RedstoneWallTorchBlock`。
//!
//! 火把被它贴着的那一格充能时熄灭，没信号了再亮回来，每次都隔两刻。
//! 一格在 60 刻里开关到第八次就烧坏：冒一股烟，160 刻以后才重新判断。
//! 官方把开关记录放在按世界分组的静态表里，这里在
//! [`RedstoneMemory`](crate::RedstoneMemory)。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{Block, BlockClass, blocks};

use crate::{
    behaviour::{
        Behaviour,
        common::{BaseTorch, default_state, has_sturdy_wall, lost_wall_support},
        state_for_placement,
    },
    context::PlaceContext,
    level::Effect,
    state::{BlockState, HORIZONTAL_FACING, LIT},
    world::World,
};

/// `RECENT_TOGGLE_TIMER`：这么多刻以前的开关记录不算了。
const RECENT_TOGGLE_TIMER: i64 = 60;
/// `MAX_RECENT_TOGGLES`：记录攒到这个数就算烧坏。
const MAX_RECENT_TOGGLES: usize = 8;
/// `RESTART_DELAY`：烧坏以后隔这么多刻再看一次。
const RESTART_DELAY: i32 = 160;
/// `TOGGLE_DELAY`：输入变了，隔两刻才跟着变。
const TOGGLE_DELAY: i32 = 2;

/// `RedstoneTorchBlock`。
pub(super) struct RedstoneTorch;

/// `notifyNeighbors`：信号穿过周围六格，所以通知的是隔一格的方块。
fn notify_neighbors(state: BlockState, world: &mut World, pos: BlockPos) {
    for direction in Direction::ALL {
        world.update_neighbors_at(pos.relative(direction), state.block());
    }
}

/// `hasNeighborSignal`：火把看的只有它贴着的那一格。
fn has_neighbor_signal(state: BlockState, world: &World, pos: BlockPos) -> bool {
    let from = if state.class() == BlockClass::RedstoneWallTorchBlock {
        state.get(HORIZONTAL_FACING).opposite()
    } else {
        Direction::Down
    };
    world.has_signal(pos.relative(from), from)
}

/// `isToggledTooFrequently`：`add` 为真时先记一次，再看这一格攒了多少次。
fn toggled_too_frequently(world: &mut World, pos: BlockPos, add: bool) -> bool {
    if add {
        let now = world.game_time();
        world.record_torch_toggle(pos, now);
    }
    world.torch_toggle_count(pos) >= MAX_RECENT_TOGGLES
}

/// `tick`。
fn tick(state: BlockState, world: &mut World, pos: BlockPos) {
    let neighbor_signal = has_neighbor_signal(state, world, pos);
    let cutoff = world.game_time() - RECENT_TOGGLE_TIMER;
    world.forget_torch_toggles_before(cutoff);
    if !state.get(LIT) {
        if !neighbor_signal && !toggled_too_frequently(world, pos, false) {
            world.set_block_and_update(pos, state.with(LIT, true));
        }
        return;
    }
    if !neighbor_signal {
        return;
    }
    world.set_block_and_update(pos, state.with(LIT, false));
    if toggled_too_frequently(world, pos, true) {
        world.push_effect(Effect::TorchBurnout { pos });
        // 官方在熄灭之后才读这一格，火把已经被别的连锁换掉时排的是新方块。
        let block = world.block_state(pos).block();
        world.schedule_tick(pos, block, RESTART_DELAY);
    }
}

/// `neighborChanged`：状态和输入对不上就排一刻去改，已经排了的不重复排。
fn neighbor_changed(state: BlockState, world: &mut World, pos: BlockPos) {
    if state.get(LIT) == has_neighbor_signal(state, world, pos)
        && !world.will_tick_this_tick(pos, state.block())
    {
        world.schedule_tick(pos, state.block(), TOGGLE_DELAY);
    }
}

/// `ownSignal`。
fn own_signal(state: BlockState) -> i32 {
    if state.get(LIT) { 15 } else { 0 }
}

/// `getDirectSignal`：只有正下方算强充能。
fn direct_signal(state: BlockState, world: &World, pos: BlockPos, direction: Direction) -> i32 {
    if direction == Direction::Down {
        state.signal(world, pos, direction)
    } else {
        0
    }
}

impl Behaviour for RedstoneTorch {
    fn can_survive(&self, state: BlockState, world: &World, pos: BlockPos) -> bool {
        BaseTorch.can_survive(state, world, pos)
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
        BaseTorch.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        notify_neighbors(state, world, pos);
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        notify_neighbors(state, world, pos);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        neighbor_changed(state, world, pos);
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        tick(state, world, pos);
    }

    fn own_signal(&self, state: BlockState, _world: &World, _pos: BlockPos) -> i32 {
        own_signal(state)
    }

    fn signal(&self, state: BlockState, world: &World, pos: BlockPos, direction: Direction) -> i32 {
        // 立在地上的火把不给上面那一格信号。
        if direction == Direction::Up {
            0
        } else {
            state.own_signal(world, pos)
        }
    }

    fn direct_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        direct_signal(state, world, pos, direction)
    }
}

/// `RedstoneWallTorchBlock`。
pub(super) struct RedstoneWallTorch;

impl Behaviour for RedstoneWallTorch {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let wall_torch = BlockState(blocks::WALL_TORCH).block();
        let placed = state_for_placement(wall_torch, ctx, world)?;
        Some(default_state(block).with(HORIZONTAL_FACING, placed.get(HORIZONTAL_FACING)))
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
        _neighbor_pos: BlockPos,
        _neighbor: BlockState,
    ) -> BlockState {
        if lost_wall_support(state, world, pos, direction) {
            BlockState::AIR
        } else {
            state
        }
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, _old: BlockState) {
        notify_neighbors(state, world, pos);
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        notify_neighbors(state, world, pos);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        neighbor_changed(state, world, pos);
    }

    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        tick(state, world, pos);
    }

    fn own_signal(&self, state: BlockState, _world: &World, _pos: BlockPos) -> i32 {
        own_signal(state)
    }

    fn signal(&self, state: BlockState, world: &World, pos: BlockPos, direction: Direction) -> i32 {
        // 墙上的火把不给它贴着的那一面信号。
        if state.get(HORIZONTAL_FACING) == direction {
            0
        } else {
            state.own_signal(world, pos)
        }
    }

    fn direct_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        direct_signal(state, world, pos, direction)
    }
}
