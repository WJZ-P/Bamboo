//! `ShelfBlock`，连同只有它实现的接口 `SideChainPartBlock`：通了红石的架子左右连成一排。
//!
//! 架子上的物品不做，所以这里的架子总是空的。

use mc_bamboo_core::{BlockPos, Direction, Hand};
use mc_bamboo_data::{Block, tags::block_tags};

use super::slot_container::hit_slot;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, is_water_source, tick_water_if_waterlogged,
            update_neighbours_after_destroy,
        },
    },
    context::{PlaceContext, UseContext},
    interact::InteractionResult,
    state::{BlockState, HORIZONTAL_FACING, POWERED, SIDE_CHAIN_PART, SideChainPart, WATERLOGGED},
    world::World,
};

/// `getRows`。
const ROWS: usize = 1;
/// `getColumns`，也是一个架子的格子数。
const COLUMNS: usize = 3;
/// `getMaxChainLength`。
const MAX_CHAIN_LENGTH: usize = 3;
/// 快捷栏的格子数。
const HOTBAR_SLOTS: usize = 9;

impl SideChainPart {
    /// `isConnected`。
    fn is_connected(self) -> bool {
        self != Self::Unconnected
    }

    /// `isConnectionTowards`。
    fn is_connection_towards(self, end_part: Self) -> bool {
        self == Self::Center || self == end_part
    }

    /// `isChainEnd`。
    fn is_chain_end(self) -> bool {
        self != Self::Center
    }

    /// `whenConnectedToTheRight`。
    fn when_connected_to_the_right(self) -> Self {
        match self {
            Self::Unconnected | Self::Left => Self::Left,
            Self::Right | Self::Center => Self::Center,
        }
    }

    /// `whenConnectedToTheLeft`。
    fn when_connected_to_the_left(self) -> Self {
        match self {
            Self::Unconnected | Self::Right => Self::Right,
            Self::Center | Self::Left => Self::Center,
        }
    }

    /// `whenDisconnectedFromTheRight`。
    fn when_disconnected_from_the_right(self) -> Self {
        match self {
            Self::Unconnected | Self::Left => Self::Unconnected,
            Self::Right | Self::Center => Self::Right,
        }
    }

    /// `whenDisconnectedFromTheLeft`。
    fn when_disconnected_from_the_left(self) -> Self {
        match self {
            Self::Unconnected | Self::Right => Self::Unconnected,
            Self::Center | Self::Left => Self::Left,
        }
    }
}

/// `isConnectable`。
fn is_connectable(state: BlockState) -> bool {
    state.is_tagged(block_tags::WOODEN_SHELVES) && state.try_get(POWERED) == Some(true)
}

/// `setPart`。
fn set_part(world: &mut World, pos: BlockPos, part: SideChainPart) {
    let state = world.block_state(pos);
    if state.get(SIDE_CHAIN_PART) != part {
        world.set_block_and_update(pos, state.with(SIDE_CHAIN_PART, part));
    }
}

/// `SideChainPartBlock.Neighbor`：旁边的一格。接不上时 `part` 是 `None`，官方的 `EmptyNeighbor`。
#[derive(Clone, Copy)]
struct Neighbor {
    pos: BlockPos,
    part: Option<SideChainPart>,
}

impl Neighbor {
    /// `Neighbors.createNewNeighbor`。
    fn read(world: &World, pos: BlockPos, facing: Direction) -> Self {
        let state = world.block_state(pos);
        let connects = is_connectable(state) && state.get(HORIZONTAL_FACING) == facing;
        Self {
            pos,
            part: connects.then(|| state.get(SIDE_CHAIN_PART)),
        }
    }

    /// `Neighbors.left`。朝向是架子朝外的方向，所以面对架子时的左边是它的顺时针方向。
    fn left(world: &World, center: BlockPos, facing: Direction) -> Self {
        Self::read(world, center.relative(facing.clockwise()), facing)
    }

    /// `Neighbors.right`。
    fn right(world: &World, center: BlockPos, facing: Direction) -> Self {
        Self::read(world, center.relative(facing.counter_clockwise()), facing)
    }

    /// `isUnconnectableOrChainEnd`。
    fn is_unconnectable_or_chain_end(self) -> bool {
        self.part.is_none_or(SideChainPart::is_chain_end)
    }

    /// `connectsTowards`。
    fn connects_towards(self, end_part: SideChainPart) -> bool {
        self.part
            .is_some_and(|part| part.is_connection_towards(end_part))
    }

    /// `connectToTheRight` 这几个方法：`part` 是读到这个邻居时的值，不是世界里现在的值。
    fn change_part(self, world: &mut World, change: fn(SideChainPart) -> SideChainPart) {
        if let Some(part) = self.part {
            set_part(world, self.pos, change(part));
        }
    }
}

/// `addBlocksConnectingTowards`：从 `pos` 往 `side` 走，连向 `end_part` 这一头的架子。
fn blocks_connecting_towards(
    world: &World,
    pos: BlockPos,
    facing: Direction,
    side: Direction,
    end_part: SideChainPart,
) -> Vec<BlockPos> {
    let mut found = Vec::new();
    for steps in 1..MAX_CHAIN_LENGTH as i32 {
        let neighbor = Neighbor::read(world, pos.relative_by(side, steps), facing);
        if neighbor.connects_towards(end_part) {
            found.push(neighbor.pos);
        }
        if neighbor.is_unconnectable_or_chain_end() {
            break;
        }
    }
    found
}

/// `getAllBlocksConnectedTo`：这一排架子，从左到右。
fn all_blocks_connected_to(world: &World, pos: BlockPos) -> Vec<BlockPos> {
    let state = world.block_state(pos);
    if !is_connectable(state) {
        return Vec::new();
    }
    let facing = state.get(HORIZONTAL_FACING);
    let left = facing.clockwise();
    let right = facing.counter_clockwise();
    let mut chain = blocks_connecting_towards(world, pos, facing, left, SideChainPart::Left);
    chain.reverse();
    chain.push(pos);
    chain.extend(blocks_connecting_towards(
        world,
        pos,
        facing,
        right,
        SideChainPart::Right,
    ));
    chain
}

/// `updateNeighborsAfterPoweringDown`。
fn update_neighbors_after_powering_down(world: &mut World, pos: BlockPos, state: BlockState) {
    let facing = state.get(HORIZONTAL_FACING);
    let left = Neighbor::left(world, pos, facing);
    left.change_part(world, SideChainPart::when_disconnected_from_the_right);
    // 官方到这里才读右边：左边的连锁可能已经改了它。
    let right = Neighbor::right(world, pos, facing);
    right.change_part(world, SideChainPart::when_disconnected_from_the_left);
}

/// `isBeingUpdatedByNeighbor`：这次变化是别的架子连过来造成的，不用再连一次。
fn is_being_updated_by_neighbor(state: BlockState, old: BlockState) -> bool {
    state.get(SIDE_CHAIN_PART).is_connected()
        || is_connectable(old) && old.get(SIDE_CHAIN_PART).is_connected()
}

/// 这个邻居所在的那一排有几个架子。
fn existing_chain(world: &World, neighbor: Neighbor) -> usize {
    if neighbor.part.is_some() {
        all_blocks_connected_to(world, neighbor.pos).len()
    } else {
        0
    }
}

/// `canConnect`。
fn can_connect(new_blocks: usize, current_length: usize) -> bool {
    new_blocks > 0 && current_length + new_blocks <= MAX_CHAIN_LENGTH
}

/// `updateSelfAndNeighborsOnPoweringUp`。
fn update_self_and_neighbors_on_powering_up(
    world: &mut World,
    pos: BlockPos,
    state: BlockState,
    old: BlockState,
) {
    if !is_connectable(state) || is_being_updated_by_neighbor(state, old) {
        return;
    }
    let facing = state.get(HORIZONTAL_FACING);
    let left = Neighbor::left(world, pos, facing);
    let right = Neighbor::right(world, pos, facing);
    let left_chain = existing_chain(world, left);
    let right_chain = existing_chain(world, right);
    let mut part = SideChainPart::Unconnected;
    let mut length = 1;
    if can_connect(left_chain, length) {
        part = part.when_connected_to_the_left();
        left.change_part(world, SideChainPart::when_connected_to_the_right);
        length += left_chain;
    }
    if can_connect(right_chain, length) {
        part = part.when_connected_to_the_right();
        right.change_part(world, SideChainPart::when_connected_to_the_left);
    }
    set_part(world, pos, part);
}

/// 没通红石时换一件物品：架子是空的，拿不到东西，手上有东西才算放了上去。
fn swap_single_item(state: BlockState, world: &mut World, ctx: &UseContext) -> InteractionResult {
    world.update_neighbour_for_output_signal(ctx.clicked_pos(), state.block());
    if ctx.hand_is_empty() {
        InteractionResult::Pass
    } else {
        InteractionResult::SuccessServer
    }
}

/// `swapHotbar` 的结果：这一排架子和快捷栏的最后几格对换。
///
/// [`Actor`](crate::Actor) 只知道手上的物品，这里按手上的物品在快捷栏第一格算，别的格子是空的。
/// 官方的玩家手上的物品在别的格子时，差别只有挥不挥手。
fn swap_hotbar(world: &mut World, ctx: &UseContext) -> InteractionResult {
    let chain = all_blocks_connected_to(world, ctx.clicked_pos());
    for &part in &chain {
        let block = world.block_state(part).block();
        world.update_neighbour_for_output_signal(part, block);
    }
    let reaches_first_slot = chain.len() * COLUMNS >= HOTBAR_SLOTS;
    if reaches_first_slot && !ctx.hand_is_empty() {
        InteractionResult::SuccessServer
    } else {
        InteractionResult::Consume
    }
}

/// `ShelfBlock`。
pub(super) struct Shelf;

impl Behaviour for Shelf {
    fn state_for_placement(
        &self,
        block: &'static Block,
        ctx: &PlaceContext,
        world: &World,
    ) -> Option<BlockState> {
        let pos = ctx.clicked_pos();
        let state = default_state(block)
            .with(HORIZONTAL_FACING, ctx.horizontal_direction().opposite())
            .with(POWERED, world.has_neighbor_signal(pos))
            .with(WATERLOGGED, is_water_source(world, pos));
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
        tick_water_if_waterlogged(state, world, pos);
        Plain.update_shape(state, world, pos, direction, neighbor_pos, neighbor)
    }

    fn on_place(&self, state: BlockState, world: &mut World, pos: BlockPos, old: BlockState) {
        if state.get(POWERED) {
            update_self_and_neighbors_on_powering_up(world, pos, state, old);
        } else {
            update_neighbors_after_powering_down(world, pos, state);
        }
    }

    fn affect_neighbors_after_removal(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        update_neighbours_after_destroy(state, world, pos);
        update_neighbors_after_powering_down(world, pos, state);
    }

    fn neighbor_changed(
        &self,
        state: BlockState,
        world: &mut World,
        pos: BlockPos,
        _source: &'static Block,
    ) {
        let signal = world.has_neighbor_signal(pos);
        if state.get(POWERED) == signal {
            return;
        }
        let mut updated = state.with(POWERED, signal);
        if !signal {
            updated = updated.with(SIDE_CHAIN_PART, SideChainPart::Unconnected);
        }
        world.set_block_and_update(pos, updated);
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let facing = state.get(HORIZONTAL_FACING);
        if ctx.hand == Hand::Off || hit_slot(&ctx.hit, facing, ROWS, COLUMNS).is_none() {
            return InteractionResult::Pass;
        }
        if state.get(POWERED) {
            swap_hotbar(world, ctx)
        } else {
            swap_single_item(state, world, ctx)
        }
    }
}
