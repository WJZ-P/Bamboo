//! `ShelfBlock`，连同只有它实现的接口 `SideChainPartBlock`：通了红石的架子左右连成一排。
//!
//! 三格物品在关卡的容器表里，漏斗和投掷器当普通 `Container` 用。玩家右键按官方对换，
//! 快捷栏只认手上那一格，其余按空的算。
//!
//! The three item slots live in the level's container table. Hoppers and droppers treat the
//! shelf as a plain `Container`. A player's click follows vanilla's swap, with only the held
//! slot modelled and the rest of the hotbar treated as empty.

use mc_bamboo_core::{BlockPos, Direction, Hand};
use mc_bamboo_data::{Block, sounds::sound_event, tags::block_tags};
use mc_bamboo_protocol::item::ItemStack;

use super::slot_container::hit_slot;
use crate::{
    behaviour::{
        Behaviour, Plain,
        common::{
            default_state, is_water_source, tick_water_if_waterlogged,
            update_neighbours_after_destroy,
        },
    },
    container::stack_limit,
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

/// 手上的物品堆。没传堆时按这件物品一个来算。
///
/// The stack in the hand. Without a stack, that is one of the held item.
fn held_stack(ctx: &UseContext) -> ItemStack {
    if let Some(stack) = ctx.stack {
        return if stack.is_empty() {
            ItemStack::empty()
        } else {
            stack.clone()
        };
    }
    if ctx.hand_is_empty() {
        ItemStack::empty()
    } else {
        ItemStack::new(ctx.item, 1)
    }
}

fn limit_held(stack: ItemStack) -> ItemStack {
    if stack.is_empty() {
        return ItemStack::empty();
    }
    let mut stack = stack;
    let limit = stack_limit(&stack);
    if stack.count > limit {
        stack.count = limit;
    }
    stack
}

/// 快捷栏近似：第 0 格是手上的物品，其余是空的。换下来的物品不写回玩家。
///
/// Hotbar approximation: slot 0 is the held stack and the rest are empty. Removed items are
/// not written back onto the player.
fn hotbar_stack(held: &ItemStack, inventory_slot: i32) -> Option<ItemStack> {
    if !(0..HOTBAR_SLOTS as i32).contains(&inventory_slot) {
        return None;
    }
    Some(if inventory_slot == 0 {
        held.clone()
    } else {
        ItemStack::empty()
    })
}

fn notify_shelf(world: &mut World, pos: BlockPos) {
    crate::machine_container::mark_container_changed(world, pos);
}

/// 没通红石时换点中的那一格。两格都空才是 `PASS`，否则 `SUCCESS_SERVER`。总是 `setChanged`。
///
/// Swaps the clicked slot while unpowered. Both empty is `PASS`; otherwise `SUCCESS_SERVER`.
/// `setChanged` always runs.
fn swap_single_item(world: &mut World, ctx: &UseContext, slot: usize) -> InteractionResult {
    let held = held_stack(ctx);
    let mut removed = ItemStack::empty();
    let wrote = world.with_slots(ctx.clicked_pos(), COLUMNS, &mut |slots| {
        removed = slots.get(slot).cloned().unwrap_or_else(ItemStack::empty);
        if let Some(stored) = slots.get_mut(slot) {
            *stored = limit_held(held.clone());
        }
    });
    if !wrote {
        return InteractionResult::Pass;
    }
    notify_shelf(world, ctx.clicked_pos());
    if removed.is_empty() && held.is_empty() {
        return InteractionResult::Pass;
    }
    // 拿出东西是 `take_item` 或 `single_swap`，空架子放上东西是 `place_item`。
    //
    // Taking an item is `take_item` or `single_swap`. Placing onto an empty slot is `place_item`.
    let sound = if removed.is_empty() {
        "minecraft:block.shelf.place_item"
    } else if held.is_empty() {
        "minecraft:block.shelf.take_item"
    } else {
        "minecraft:block.shelf.single_swap"
    };
    world.play_block_sound(ctx.clicked_pos(), sound_event(sound), 1.0, 1.0);
    InteractionResult::SuccessServer
}

/// `swapHotbar`：这一排架子和快捷栏对换。没换到东西是 `CONSUME`，换到了是 `SUCCESS_SERVER`。
///
/// 每个架子写完就 `setChanged`。快捷栏只建模第 0 格。
///
/// `swapHotbar`: swap this row of shelves with the hotbar. Nothing moved is `CONSUME`;
/// something moved is `SUCCESS_SERVER`.
///
/// Each shelf calls `setChanged` as soon as it is written. Only hotbar slot 0 is modelled.
fn swap_hotbar(world: &mut World, ctx: &UseContext) -> InteractionResult {
    let chain = all_blocks_connected_to(world, ctx.clicked_pos());
    if chain.is_empty() {
        return InteractionResult::Consume;
    }
    let held = held_stack(ctx);
    let len = chain.len() as i32;
    let mut any_swapped = false;
    for (index, part) in chain.iter().copied().enumerate() {
        let wrote = world.with_slots(part, COLUMNS, &mut |slots| {
            for (slot, stored) in slots.iter_mut().enumerate() {
                let inventory_slot =
                    HOTBAR_SLOTS as i32 - (len - index as i32) * COLUMNS as i32 + slot as i32;
                let Some(placed) = hotbar_stack(&held, inventory_slot) else {
                    continue;
                };
                let removed = stored.clone();
                *stored = limit_held(placed.clone());
                if !placed.is_empty() || !removed.is_empty() {
                    any_swapped = true;
                }
            }
        });
        if wrote {
            notify_shelf(world, part);
        }
    }
    if any_swapped {
        world.play_block_sound(
            ctx.clicked_pos(),
            sound_event("minecraft:block.shelf.multi_swap"),
            1.0,
            1.0,
        );
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
        let sound = if signal {
            "minecraft:block.shelf.activate"
        } else {
            "minecraft:block.shelf.deactivate"
        };
        world.play_block_sound(pos, sound_event(sound), 1.0, 1.0);
    }

    /// `ShelfBlock.getAnalogOutputSignal`：只从背面读，三格占用各占一位。
    ///
    /// `ShelfBlock.getAnalogOutputSignal` reads only the back. Each occupied slot is one bit.
    fn analog_output_signal(
        &self,
        state: BlockState,
        world: &World,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        if direction != state.get(HORIZONTAL_FACING).opposite() {
            return 0;
        }
        let slots = world.slots_at(pos);
        let bit = |index: usize| u8::from(slots.get(index).is_some_and(|stack| !stack.is_empty()));
        i32::from(bit(0) | bit(1) << 1 | bit(2) << 2)
    }

    fn use_item_on(
        &self,
        state: BlockState,
        world: &mut World,
        ctx: &UseContext,
    ) -> InteractionResult {
        let facing = state.get(HORIZONTAL_FACING);
        let Some(slot) = hit_slot(&ctx.hit, facing, ROWS, COLUMNS) else {
            return InteractionResult::Pass;
        };
        if ctx.hand == Hand::Off {
            return InteractionResult::Pass;
        }
        if state.get(POWERED) {
            swap_hotbar(world, ctx)
        } else {
            swap_single_item(world, ctx, slot)
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{BlockPos, Direction, Hand, Vec3};
    use mc_bamboo_data::{blocks, items};
    use mc_bamboo_protocol::item::ItemStack;

    use super::*;
    use crate::{
        BlockState, BlockTicks,
        behaviour::Behaviour,
        context::{BlockHit, UseContext},
        state::{HORIZONTAL_FACING, POWERED, SIDE_CHAIN_PART, SideChainPart},
        testing::{TestLevel, actor_holding},
        world::World,
    };

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn south_hit(pos: BlockPos) -> BlockHit {
        BlockHit {
            pos,
            face: Direction::South,
            location: Vec3::new(pos.x as f64 + 0.1, pos.y as f64 + 0.5, pos.z as f64 + 1.0),
            inside: false,
        }
    }

    fn shelf(facing: Direction, powered: bool) -> BlockState {
        BlockState(blocks::OAK_SHELF)
            .with(HORIZONTAL_FACING, facing)
            .with(POWERED, powered)
    }

    fn click(world: &mut World, state: BlockState, pos: BlockPos, item: i32) -> InteractionResult {
        let actor = actor_holding(item);
        let ctx = UseContext::new(&actor, Hand::Main, south_hit(pos));
        Shelf.use_item_on(state, world, &ctx)
    }

    #[test]
    fn an_empty_hand_clears_one_shelf_slot() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let state = shelf(Direction::South, false);
        level.set(POS, state);
        let mut world = World::new(&mut level, &mut ticks);
        world.with_slots(POS, COLUMNS, &mut |slots| {
            slots[0] = ItemStack::new(items::DIRT, 1);
        });

        assert_eq!(
            click(&mut world, state, POS, items::AIR),
            InteractionResult::SuccessServer
        );
        assert!(world.slots_at(POS)[0].is_empty());
    }

    #[test]
    fn an_empty_click_on_an_empty_shelf_passes_and_placing_schedules_the_comparator() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let state = shelf(Direction::South, false);
        let comparator = POS.north();
        level.set(POS, state);
        // 比较器要立在完整方块上，否则 `neighborChanged` 会把它拆掉，排不上刻。
        //
        // A comparator has to stand on a full block, or `neighborChanged` breaks it
        // before it can schedule.
        level.set(comparator.below(), BlockState(blocks::STONE));
        level.set(
            comparator,
            BlockState(blocks::COMPARATOR).with(HORIZONTAL_FACING, Direction::South),
        );
        let mut world = World::new(&mut level, &mut ticks);
        let comparator_block = BlockState(blocks::COMPARATOR).block();

        assert_eq!(
            click(&mut world, state, POS, items::AIR),
            InteractionResult::Pass
        );
        assert_eq!(
            click(&mut world, state, POS, items::DIRT),
            InteractionResult::SuccessServer
        );
        assert_eq!(world.slots_at(POS)[0], ItemStack::new(items::DIRT, 1));
        assert!(world.has_scheduled_tick(comparator, comparator_block));
        assert_eq!(state.analog_output_signal(&world, POS, Direction::North), 1);
        assert_eq!(state.analog_output_signal(&world, POS, Direction::South), 0);
    }

    #[test]
    fn a_powered_shelf_swaps_with_the_last_hotbar_slots() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let state = shelf(Direction::South, true);
        level.set(POS, state);
        let mut world = World::new(&mut level, &mut ticks);
        world.with_slots(POS, COLUMNS, &mut |slots| {
            slots[0] = ItemStack::new(items::DIRT, 1);
        });

        assert_eq!(
            click(&mut world, state, POS, items::AIR),
            InteractionResult::SuccessServer
        );
        assert!(world.slots_at(POS).iter().all(ItemStack::is_empty));
    }

    #[test]
    fn a_powered_row_puts_the_held_item_on_the_leftmost_shelf() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let center = POS;
        let west = center.west();
        let east = center.east();
        let part = |end: SideChainPart| shelf(Direction::South, true).with(SIDE_CHAIN_PART, end);
        level.set(west, part(SideChainPart::Left));
        level.set(center, part(SideChainPart::Center));
        level.set(east, part(SideChainPart::Right));
        let mut world = World::new(&mut level, &mut ticks);

        assert_eq!(
            click(&mut world, part(SideChainPart::Center), center, items::DIRT),
            InteractionResult::SuccessServer
        );
        assert_eq!(world.slots_at(west)[0], ItemStack::new(items::DIRT, 1));
        assert!(world.slots_at(center).iter().all(ItemStack::is_empty));
        assert!(world.slots_at(east).iter().all(ItemStack::is_empty));
    }
}
